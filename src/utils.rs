use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::{Debug, Display};
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::{env, fs};

use log::{debug, error};
use regex::Regex;
use walkdir::WalkDir;

use crate::git;

const HOOK_REPOS_SAVE_LOCATION: &str = ".git/hook-repos";

pub fn execute_cmd<T: AsRef<str> + AsRef<OsStr> + Debug>(
    bin: &str,
    args: &[T],
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> anyhow::Result<(ExitStatus, String, String)> {
    debug!(
        "called \"{} {:?}\" in {:?} with env expanded with {:?}",
        bin, args, cwd, env
    );
    let empty_map = HashMap::new();
    let env = env.unwrap_or(&empty_map);
    let mut cmd = match cwd {
        Some(path) => Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(path)
            .envs(env)
            .spawn()?,
        None => Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(env)
            .spawn()?,
    };
    let (mut stderr, mut stdout) = (String::new(), String::new());
    if let Some(mut output) = cmd.stderr.take() {
        output.read_to_string(&mut stderr)?;
    }
    debug!("cmd stdout: {}", stdout);
    if let Some(mut output) = cmd.stdout.take() {
        output.read_to_string(&mut stdout)?;
    }
    debug!("cmd stderr: {}", stdout);
    let res = cmd.wait();
    if let Err(r) = res {
        error!(
            "Error on \"{} {:?}\" invocation, here's the output:\nstdout: {}\nstderr: {}",
            bin, args, stdout, stderr
        );
        return Err(anyhow::Error::new(r));
    }
    let status = res.unwrap();
    if !status.success() {
        error!(
            "Error on \"{} {:?}\" invocation, here's the output:\nstdout: {}\nstderr: {}",
            bin, args, stdout, stderr
        );
        return Err(anyhow::Error::msg("command invoked with errors"));
    }
    Ok((status, stdout, stderr))
}

pub fn get_local_repo_path(url: &str) -> anyhow::Result<String> {
    Ok(format!(
        "{}/{}/{}",
        git::root()?,
        HOOK_REPOS_SAVE_LOCATION,
        url.split('/').last().expect("incomplete repo URL?")
    ))
}

pub fn matches<T: AsRef<str> + Display>(e: &Path, regexps: &[T]) -> bool {
    let dot_git_re =
        Regex::new("\\.git/*").unwrap_or_else(|regex| panic!("invalid regex: {}", regex));
    if e.is_dir() {
        debug!("skipping dir {}", e.display());
        return false;
    }
    if dot_git_re.is_match(&e.display().to_string()) {
        debug!("skipping git file {}", e.display());
        return false;
    }
    for regex in regexps {
        let r = Regex::new(regex.as_ref()).expect(&format!("invalid regex: {}", regex));
        if r.is_match(&e.display().to_string()) {
            debug!("Found matching file {}", e.display());
            return true;
        }
        debug!("File {} didn't match re {}", e.display(), regex);
    }
    debug!("File {} didn't match", e.display());
    false
}

pub fn get_files<T: AsRef<str> + Display>(
    base_dir: &str,
    regexps: &[T],
) -> anyhow::Result<Vec<String>> {
    let final_list = WalkDir::new(base_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            debug!("trying file {}", e.path().display());
            matches(e.path(), regexps)
        })
        .map(|e| {
            debug!("Adding file {:?}", e);
            e.path().display().to_string()
        })
        .collect();
    debug!("final list: {:?}", final_list);
    Ok(final_list)
}

/// Returns true if the given program name can be found in $PATH
pub fn _is_program_in_path(program: &str) -> bool {
    if let Ok(path) = env::var("PATH") {
        for p in path.split(':') {
            let p_str = format!("{}/{}", p, program);
            if fs::metadata(p_str).is_ok() {
                return true;
            }
        }
    }
    false
}

pub fn prefix_path(p: &str) -> String {
    // expand PATH
    let mut bin_path = env::var("PATH").expect("PATH is not set in the env.");
    bin_path.insert_str(0, &format!("{}:", p));
    debug!("New $PATH: {}", &bin_path);
    bin_path
}
