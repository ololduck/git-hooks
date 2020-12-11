use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs;
use std::path::Path;
use std::process::ExitStatus;

use log::{debug, error};

use crate::utils;

fn git_command<T: AsRef<str> + AsRef<OsStr> + Debug>(
    args: &[T],
    repo: Option<&str>,
) -> anyhow::Result<(ExitStatus, String, String)> {
    utils::execute_cmd("git", &args, repo, None)
}

/// Clones a git depot & returns the path to the cloned instance
/// TODO:
///     - clone a shallow copy
///     - clone specific revision
pub fn clone<T: AsRef<str>, U: AsRef<str>>(source: T, target: U) -> anyhow::Result<String> {
    let target_dir = Path::new(target.as_ref());
    if !(target_dir.exists() && target_dir.is_dir()) {
        if let Err(e) = fs::create_dir_all(target_dir) {
            error!(
                "Could not create clone destination directory: {:?}",
                e.kind()
            );
            return Err(anyhow::Error::new(e));
        }
    }
    let (_status, _stdout, _stderr) = git_command(
        &["clone", source.as_ref(), target.as_ref()] as &[&str],
        None,
    )?;
    Ok(String::from(target.as_ref()))
}

/// Pulls code on the default git branch, givent a repo
pub fn pull(source: &str, target: &str) -> anyhow::Result<String> {
    debug!("getting a fresh version of {}", source);
    let target_dir = Path::new(&target);
    if !(target_dir.exists() && target_dir.is_dir()) {
        return clone(source, target);
    }
    let (_status, stdout, _stderr) = git_command(&["pull"], Some(target.as_ref()))?;
    Ok(stdout)
}

pub fn add<T: AsRef<str>>(files: &[T]) -> anyhow::Result<()> {
    let mut args = vec!["add"];
    for x in files {
        args.push(x.as_ref());
    }
    let (_status, _stdout, _stderr) = git_command(&args, Some(&root()?))?;
    Ok(())
}

pub fn changed_files(in_index: bool) -> anyhow::Result<Vec<String>> {
    let mut args = vec!["diff", "--name-only", "--diff-filter=ACM"];
    if in_index {
        args.push("--cached");
    }
    //git diff --cached --name-only --diff-filter=ACM
    let (_status, stdout, _stderr) = git_command(&args, Some(&root()?))?;
    Ok(stdout.lines().map(|s| String::from(s)).collect())
}

/// Returns the root of the repository.
/// If executed in /tmp/my-repo/src, returns /tmp/my-repo
pub fn root() -> anyhow::Result<String> {
    let (_status, stdout, _stderr) =
        git_command(&["rev-parse", "--show-toplevel"] as &[&str], None)?;
    let stdout = stdout
        .strip_suffix("\n")
        .expect("Could not strip git root output string. weird")
        .to_string();
    Ok(stdout)
}
