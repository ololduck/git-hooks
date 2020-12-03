use std::{env, fs};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Debug;
use std::io::Read;
use std::iter::Map;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use log::{debug, error};
use regex::Regex;
use serde::export::fmt::Display;
use walkdir::WalkDir;

use crate::git;

const HOOK_REPOS_SAVE_LOCATION: &str = ".git/hook-repos";

pub fn execute_cmd<T: AsRef<str> + AsRef<OsStr> + Debug>(
    bin: &str,
    args: &[T],
    cwd: Option<&str>,
    env: Option<HashMap<String, String>>,
) -> anyhow::Result<(ExitStatus, String, String)> {
    let env = env.unwrap_or(HashMap::new());
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
    let res = cmd.wait();
    let (mut stderr, mut stdout) = (String::new(), String::new());
    if let Some(mut output) = cmd.stderr.take() {
        output.read_to_string(&mut stderr)?;
    }
    if let Some(mut output) = cmd.stdout.take() {
        output.read_to_string(&mut stdout)?;
    }
    if res.is_err() {
        error!(
            "Error on \"{} {:?}\" invocation, here's the output:\nstdout: {}\nstderr: {}",
            bin, args, stdout, stderr
        );
        return Err(anyhow::Error::new(res.unwrap_err()));
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
        url.split("/").last().expect("incomplete repo URL?")
    ))
}

pub fn get_files<T: AsRef<str> + Display>(
    base_dir: &str,
    regexps: &[T],
) -> anyhow::Result<Vec<String>> {
    let dot_git_re = Regex::new(".git/*")?;
    let mut flist: Vec<String> = Vec::new();
    for entry in WalkDir::new(base_dir).into_iter().filter_entry(|e| {
        for regex in regexps {
            let r = Regex::new(regex.as_ref()).expect(&format!("invalid regex: {}", regex));
            if r.is_match(&e.path().display().to_string())
                && !(dot_git_re.is_match(&e.path().display().to_string()) || e.path().is_dir())
            {
                debug!("Found matching file {}", e.path().display());
                return true;
            }
        }
        false
    }) {
        flist.push(entry?.path().display().to_string());
    }
    Ok(flist)
}

pub fn is_program_in_path(program: &str) -> bool {
    if let Ok(path) = env::var("PATH") {
        for p in path.split(":") {
            let p_str = format!("{}/{}", p, program);
            if fs::metadata(p_str).is_ok() {
                return true;
            }
        }
    }
    false
}
