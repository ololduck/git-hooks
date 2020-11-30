use std::fs;
use std::path::Path;
use std::process::ExitStatus;

use log::{debug, error};

use crate::utils;

fn git_command(args: &[&str], repo: Option<&str>) -> anyhow::Result<(ExitStatus, String, String)> {
    utils::execute_cmd("git", args, repo)
}

/// Clones a git depot & returns the path to the cloned instance
/// TODO:
///     - clone a shallow copy
///     - clone specific revision
pub fn clone(source: &str, target: &str) -> anyhow::Result<String> {
    let target_dir = Path::new(target);
    if !(target_dir.exists() && target_dir.is_dir()) {
        if let Err(e) = fs::create_dir_all(target_dir) {
            error!(
                "Could not create clone destination directory: {:?}",
                e.kind()
            );
            return Err(anyhow::Error::new(e));
        }
    }
    let (_status, _stdout, _stderr) = git_command(&["clone", source, target], None)?;
    Ok(String::from(target))
}

pub fn pull(source: &str, target: &str) -> anyhow::Result<String> {
    debug!("getting a fresh version of {}", source);
    let target_dir = Path::new(target);
    if !(target_dir.exists() && target_dir.is_dir()) {
        return clone(source, target);
    }
    let (_status, stdout, _stderr) = git_command(&["pull"], Some(target))?;
    Ok(stdout)
}

pub fn root() -> anyhow::Result<String> {
    let (_status, stdout, _stderr) = git_command(&["rev-parse", "--show-toplevel"], None)?;
    let stdout = stdout
        .strip_suffix("\n")
        .expect("Could not strip git root output string. weird")
        .to_string();
    Ok(stdout)
}
