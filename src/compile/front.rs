use std::collections::HashMap;
use std::sync::Arc;

use super::ChangeSet;
use crate::config::Project;
use crate::ext::fs;
use crate::ext::sync::{wait_interruptible, CommandResult};
use crate::service::site::SiteFile;
use crate::signal::{Interrupt, Outcome, Product};
use crate::{
    config::LibPackage,
    ext::{
        anyhow::{Context, Result},
        exe::Exe,
    },
    logger::GRAY,
};
use camino::{Utf8Path, Utf8PathBuf};
use tokio::process::Child;
use tokio::{process::Command, sync::broadcast, task::JoinHandle};
use wasm_bindgen_cli_support::Bindgen;

pub async fn front(
    proj: &Arc<Project>,
    changes: &ChangeSet,
) -> JoinHandle<Result<Outcome<Product>>> {
    let proj = proj.clone();
    let changes = changes.clone();
    tokio::spawn(async move {
        let Some(lib) = &proj.lib else {
            return Ok(Outcome::Success(Product::None));
        };

        if !changes.need_front_build() {
            log::trace!("Front no changes to rebuild");
            return Ok(Outcome::Success(Product::None));
        }

        fs::create_dir_all(&proj.site.root_relative_pkg_dir()).await?;

        let (line, process) = front_cargo_process("build", true, lib)?;

        match wait_interruptible("Cargo", process, Interrupt::subscribe_any()).await? {
            CommandResult::Interrupted => return Ok(Outcome::Stopped),
            CommandResult::Failure(_) => return Ok(Outcome::Failed),
            _ => {}
        }
        log::info!("Cargo finished {}", GRAY.paint(line));

        bindgen(&proj).await.dot()
    })
}

pub fn front_cargo_process(cmd: &str, wasm: bool, lib: &LibPackage) -> Result<(String, Child)> {
    let mut command = Command::new("cargo");
    let line = build_cargo_front_cmd(cmd, wasm, lib, &mut command);
    Ok((line, command.spawn()?))
}

pub fn build_cargo_front_cmd(
    cmd: &str,
    wasm: bool,
    lib: &LibPackage,
    command: &mut Command,
) -> String {
    let mut args = vec![
        cmd.to_string(),
        format!("--package={}", lib.name.as_str()),
        "--lib".to_string(),
        "--target-dir=target/front".to_string(),
    ];
    if wasm {
        args.push("--target=wasm32-unknown-unknown".to_string());
    }

    if !lib.default_features {
        args.push("--no-default-features".to_string());
    }

    if !lib.features.is_empty() {
        args.push(format!("--features={}", lib.features.join(",")));
    }

    lib.profile.add_to_args(&mut args);
    command.args(&args);

    format!("cargo {}", args.join(" "))
}

async fn bindgen(proj: &Project) -> Result<Outcome<Product>> {
    let Some(lib) = &proj.lib else {
        return Ok(Outcome::Success(Product::None));
    };

    let wasm_file = &lib.wasm_file;
    let interrupt = Interrupt::subscribe_any();

    // see:
    // https://github.com/rustwasm/wasm-bindgen/blob/main/crates/cli-support/src/lib.rs#L95
    // https://github.com/rustwasm/wasm-bindgen/blob/main/crates/cli/src/bin/wasm-bindgen.rs#L13
    let mut bindgen = Bindgen::new()
        .input_path(&wasm_file.source)
        .web(true)
        .dot()?
        .generate_output()
        .dot()?;

    bindgen.wasm_mut().emit_wasm_file(&wasm_file.dest).dot()?;
    log::trace!("Front wrote wasm to {:?}", wasm_file.dest.as_str());
    if proj.release {
        match optimize(&wasm_file.dest, interrupt).await.dot()? {
            CommandResult::Interrupted => return Ok(Outcome::Stopped),
            CommandResult::Failure(_) => return Ok(Outcome::Failed),
            _ => {}
        }
    }

    let mut js_changed = false;

    js_changed |= write_snippets(proj, bindgen.snippets()).await?;

    js_changed |= write_modules(proj, bindgen.local_modules()).await?;

    let wasm_changed = proj
        .site
        .did_file_change(&lib.wasm_file.as_site_file())
        .await
        .dot()?;
    js_changed |= proj
        .site
        .updated_with(&lib.js_file, bindgen.js().as_bytes())
        .await
        .dot()?;
    log::debug!("Front js changed: {js_changed}");
    log::debug!("Front wasm changed: {wasm_changed}");

    if js_changed || wasm_changed {
        Ok(Outcome::Success(Product::Front))
    } else {
        Ok(Outcome::Success(Product::None))
    }
}

async fn optimize(
    file: &Utf8Path,
    interrupt: broadcast::Receiver<()>,
) -> Result<CommandResult<()>> {
    let wasm_opt = Exe::WasmOpt.get().await.dot()?;

    let args = [file.as_str(), "-Os", "-o", file.as_str()];
    let process = Command::new(wasm_opt)
        .args(args)
        .spawn()
        .context("Could not spawn command")?;
    wait_interruptible("wasm-opt", process, interrupt).await
}

async fn write_snippets(proj: &Project, snippets: &HashMap<String, Vec<String>>) -> Result<bool> {
    let mut js_changed = false;

    // Provide inline JS files
    for (identifier, list) in snippets.iter() {
        for (i, js) in list.iter().enumerate() {
            let name = format!("inline{}.js", i);
            let site_path = Utf8PathBuf::from("snippets").join(identifier).join(name);
            let file_path = proj.site.root_relative_pkg_dir().join(&site_path);

            fs::create_dir_all(file_path.parent().unwrap()).await?;

            let site_file = SiteFile {
                dest: file_path,
                site: site_path,
            };

            js_changed |= proj.site.updated_with(&site_file, js.as_bytes()).await?;
        }
    }
    Ok(js_changed)
}

async fn write_modules(proj: &Project, modules: &HashMap<String, String>) -> Result<bool> {
    let mut js_changed = false;
    // Provide snippet files from JS snippets
    for (path, js) in modules.iter() {
        let site_path = Utf8PathBuf::from("snippets").join(path);
        let file_path = proj.site.root_relative_pkg_dir().join(&site_path);

        fs::create_dir_all(file_path.parent().unwrap()).await?;

        let site_file = SiteFile {
            dest: file_path,
            site: site_path,
        };

        js_changed |= proj.site.updated_with(&site_file, js.as_bytes()).await?;
    }
    Ok(js_changed)
}
