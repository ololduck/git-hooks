use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};

use log::error;

use crate::git;

const HOOK_REPOS_SAVE_LOCATION: &str = ".git/hook-repos";

pub fn execute_cmd(
    bin: &str,
    args: &[&str],
    cwd: Option<&str>,
) -> anyhow::Result<(ExitStatus, String, String)> {
    let mut cmd = match cwd {
        Some(path) => Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(path)
            .spawn()?,
        None => Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
