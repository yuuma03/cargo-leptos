use crate::compile::{front_cargo_process, server_cargo_process};
use crate::config::{Config, Project};
use crate::ext::anyhow::{Context, Result};
use crate::logger::GRAY;

pub async fn test_all(conf: &Config) -> Result<()> {
    for proj in &conf.projects {
        test_proj(proj).await?;
    }
    Ok(())
}

pub async fn test_proj(proj: &Project) -> Result<()> {
    if let Some(bin) = &proj.bin {
        let (line, mut proc) = server_cargo_process("test", bin).dot()?;
        proc.wait().await.dot()?;
        log::info!("Cargo server tests finished {}", GRAY.paint(line));
    }

    if let Some(lib) = &proj.lib {
        let (line, mut proc) = front_cargo_process("test", false, lib).dot()?;
        proc.wait().await.dot()?;
        log::info!("Cargo front tests finished {}", GRAY.paint(line));
    }

    Ok(())
}
