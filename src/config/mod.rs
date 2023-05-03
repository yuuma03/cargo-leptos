#[cfg(test)]
mod tests;

mod assets;
mod bin_package;
mod cli;
mod dotenvs;
mod end2end;
mod lib_package;
mod profile;
mod project;
mod style;
mod tailwind;

use std::{fmt::Debug, sync::Arc};

pub use self::cli::{Cli, Commands, Log, Opts};
use crate::ext::{
    anyhow::{Context, Result},
    MetadataExt,
};
use anyhow::bail;
pub use bin_package::BinPackage;
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::Metadata;
pub use lib_package::LibPackage;
pub use profile::Profile;
pub use project::{Project, ProjectConfig};
pub use style::StyleConfig;
pub use tailwind::TailwindConfig;

pub struct Config {
    /// absolute path to the working dir
    pub working_dir: Utf8PathBuf,
    pub projects: Vec<Arc<Project>>,
    pub default_run: Option<Arc<Project>>,
    pub cli: Opts,
    pub watch: bool,
}

impl Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("projects", &self.projects)
            .field("cli", &self.cli)
            .field("watch", &self.watch)
            .finish_non_exhaustive()
    }
}

impl Config {
    pub fn load(cli: Opts, cwd: &Utf8Path, manifest_path: &Utf8Path, watch: bool) -> Result<Self> {
        let metadata = Metadata::load_cleaned(manifest_path)?;
        let root_package = metadata.root_package();

        let mut projects = Project::resolve(&cli, cwd, &metadata, watch).dot()?;
        let mut default_run = None;

        if projects.is_empty() {
            bail!("Please define leptos projects in the workspace Cargo.toml sections [[workspace.metadata.leptos]]")
        }

        if let Some(proj_name) = &cli.project {
            if let Some(proj) = projects.iter().find(|p| p.name == *proj_name) {
                default_run = Some(proj.clone());
                projects = vec![proj.clone()];
            } else {
                bail!(
                    r#"The specified project "{proj_name}" not found. Available projects: {}"#,
                    names(&projects)
                )
            }
        } else {
            if let Some(proj_name) = root_package
                .and_then(|package| package.default_run.clone())
                .or_else(|| root_package.and_then(|package| Some(package.name.clone())))
            {
                if let Some(proj) = projects.iter().find(|p| p.name == *proj_name) {
                    default_run = Some(proj.clone());
                } else {
                    bail!(
                        r#"The specified project "{proj_name}" not found. Available projects: {}"#,
                        names(&projects)
                    )
                }
            }
        }

        Ok(Self {
            working_dir: metadata.workspace_root.clone(),
            projects,
            default_run,
            cli,
            watch,
        })
    }

    #[cfg(test)]
    pub fn test_load(cli: Opts, cwd: &str, manifest_path: &str, watch: bool) -> Self {
        use crate::ext::PathBufExt;

        let manifest_path = Utf8PathBuf::from(manifest_path)
            .canonicalize_utf8()
            .unwrap();
        let mut cwd = Utf8PathBuf::from(cwd).canonicalize_utf8().unwrap();
        cwd.clean_windows_path();
        Self::load(cli, &cwd, &manifest_path, watch).unwrap()
    }

    pub fn current_project(&self) -> Result<Arc<Project>> {
        if let Some(default_run) = &self.default_run {
            Ok(default_run.clone())
        } else {
            bail!("There are several projects available ({}). Please select one of them with the command line parameter --project", names(&self.projects));
        }
    }
}

fn names(projects: &[Arc<Project>]) -> String {
    projects
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}
