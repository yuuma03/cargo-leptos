use std::sync::Arc;

use super::ChangeSet;
use crate::{
    config::{BinPackage, Project},
    ext::anyhow::{Context, Result},
    ext::sync::{wait_interruptible, CommandResult},
    logger::GRAY,
    signal::{Interrupt, Outcome, Product},
};
use tokio::{
    process::{Child, Command},
    task::JoinHandle,
};

pub async fn server(
    proj: &Arc<Project>,
    changes: &ChangeSet,
) -> JoinHandle<Result<Outcome<Product>>> {
    let proj = proj.clone();
    let changes = changes.clone();
    tokio::spawn(async move {
        let Some(bin) = &proj.bin else {
            return Ok(Outcome::Success(Product::None));
        };

        if !changes.need_server_build() {
            return Ok(Outcome::Success(Product::None));
        }

        let (line, process) = server_cargo_process("build", bin)?;

        match wait_interruptible("Cargo", process, Interrupt::subscribe_any()).await? {
            CommandResult::Success(_) => {
                log::info!("Cargo finished {}", GRAY.paint(line));

                let changed = proj
                    .site
                    .did_external_file_change(&bin.exe_file)
                    .await
                    .dot()?;
                if changed {
                    log::debug!("Cargo server bin changed");
                    Ok(Outcome::Success(Product::Server))
                } else {
                    log::debug!("Cargo server bin unchanged");
                    Ok(Outcome::Success(Product::None))
                }
            }
            CommandResult::Interrupted => Ok(Outcome::Stopped),
            CommandResult::Failure(_) => Ok(Outcome::Failed),
        }
    })
}

pub fn server_cargo_process(cmd: &str, bin: &BinPackage) -> Result<(String, Child)> {
    let mut command = Command::new("cargo");
    let line = build_cargo_server_cmd(cmd, bin, &mut command);
    Ok((line, command.spawn()?))
}

pub fn build_cargo_server_cmd(cmd: &str, bin: &BinPackage, command: &mut Command) -> String {
    let mut args = vec![cmd.to_string(), format!("--package={}", bin.name.as_str())];
    if cmd != "test" {
        args.push(format!("--bin={}", bin.target))
    }
    args.push("--target-dir=target/server".to_string());
    if let Some(triple) = &bin.target_triple {
        args.push(format!("--target={triple}"));
    }

    if !bin.default_features {
        args.push("--no-default-features".to_string());
    }

    if !bin.features.is_empty() {
        args.push(format!("--features={}", bin.features.join(",")));
    }

    bin.profile.add_to_args(&mut args);
    command.args(&args);

    format!("cargo {}", args.join(" "))
}
