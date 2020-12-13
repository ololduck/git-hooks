use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs;
use std::path::Path;
use std::process::ExitStatus;

use log::{debug, error};

use crate::utils;

#[cfg(test)]
mod tests {
    use crate::git::{add, changed_files, checkout, clone, git_command, root};
    use std::env::{current_dir, set_current_dir};
    use std::fs::File;
    use std::path::Path;
    use tempdir::TempDir;

    fn setup() -> TempDir {
        let _ = pretty_env_logger::try_init();
        TempDir::new("git-hooks-tests").expect("could not create temp dir")
    }

    #[test]
    fn test_git_command() {
        let _ = setup();
        let r = git_command(&["--version"], None);
        assert!(r.is_ok());
        let (s, out, _err) = r.unwrap();
        assert!(s.success());
        assert!(out.starts_with("git version "));
    }

    #[test]
    fn test_clone() {
        let dir = setup();
        let r = clone(".", dir.path().display().to_string());
        assert!(r.is_ok());
        let p = r.unwrap();
        assert_eq!(p, dir.path().display().to_string());
    }

    #[test]
    fn test_checkout() {
        let dir = setup();
        let _ = clone(".", dir.path().display().to_string());
        let r = checkout(
            "99586a59496151167dc730c62d5405d7a6401bf6",
            dir.path().display().to_string().as_str(),
        );
        assert!(r.is_ok());
        let r = git_command(
            &["rev-parse", "HEAD"],
            Some(dir.path().display().to_string().as_str()),
        );
        assert!(r.is_ok());
        let (s, out, _err) = r.unwrap();
        assert!(s.success());
        assert_eq!(out.trim(), "99586a59496151167dc730c62d5405d7a6401bf6"); // hash of the v0.3.0 tag
    }

    #[test]
    fn test_adding_files() {
        let dir = setup();
        let _ = clone(".", dir.path().display().to_string());
        let f = File::create(dir.path().join("tests.txt"));
        assert!(f.is_ok());
        let old_dir = current_dir().expect("could not unwrap current_dir");
        set_current_dir(Path::new(dir.path().display().to_string().as_str()))
            .expect("could not cd in temp cloned dir");
        let r = changed_files(false);
        assert!(r.is_ok());
        let files = r.unwrap();
        assert!(files.contains(&"tests.txt".to_string()));
        let r = add(&["tests.txt"]);
        assert!(r.is_ok());
        let r = changed_files(true);
        assert!(r.is_ok());
        let files = r.unwrap();
        assert!(files.contains(&"tests.txt".to_string()));
        set_current_dir(old_dir).expect("could not cd back to old dir");
    }

    #[test]
    fn test_root() {
        let dir = setup();
        let _ = clone(
            "https://github.com/paulollivier/git-hooks",
            dir.path().display().to_string(),
        );
        let old_dir = current_dir().expect("could not get current dir");
        set_current_dir(dir.path().join("src")).expect("could not change current dir");
        let r = root();
        assert!(r.is_ok());
        let d = r.unwrap();
        assert_eq!(dir.path().display().to_string(), d);
        set_current_dir(old_dir).expect("could not cd back to old dir");
    }
}

fn git_command<T: AsRef<str> + AsRef<OsStr> + Debug>(
    args: &[T],
    repo: Option<&str>,
) -> anyhow::Result<(ExitStatus, String, String)> {
    utils::execute_cmd("git", &args, repo, None)
}

#[cfg(test)]
/// inits a new git repo
/// if `dir` is Some, the repo will be initiated in the given directory. Otherwise, in the current directory.
pub fn init(dir: Option<&str>) -> anyhow::Result<()> {
    git_command(&["init"], dir)?;
    Ok(())
}

#[cfg(test)]
/// returns the commit hash designated by the given `reference`
pub fn get_hash(reference: &str) -> anyhow::Result<String> {
    let (s, out, err) = git_command(&["rev-parse", reference], None)?;
    if !s.success() {
        return Err(anyhow::Error::msg(err));
    }
    Ok(out.trim().to_string())
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

pub fn checkout(reference: &str, repo: &str) -> anyhow::Result<()> {
    let (status, _stdout, _stderr) =
        git_command(&["rev-parse", "--verify", reference], Some(repo))?;
    if !status.success() {
        return Err(anyhow::Error::msg(format!(
            "could not find reference {} in {}",
            reference, repo
        )));
    }
    git_command(&["checkout", reference], Some(repo))?;
    Ok(())
}

/// Pulls code on the default git branch, givent a repo
pub fn pull(source: &str, target: &str) -> anyhow::Result<String> {
    debug!("getting a fresh version of {}", source);
    let target_dir = Path::new(&target);
    if !(target_dir.exists() && target_dir.is_dir()) {
        return clone(source, target);
    }
    let (_status, stdout, _stderr) = git_command(&["pull"], Some(target))?;
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
    return if in_index {
        let (_status, stdout, _stderr) = git_command(
            &["diff", "--name-only", "--diff-filter=ACM", "--cached"],
            Some(&root()?),
        )?;
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    } else {
        let (_status, stdout, _stderr) = git_command(
            &["ls-files", "--others", "--exclude-standard"],
            Some(&root()?),
        )?;
        Ok(stdout.lines().map(|s| s.to_string()).collect())
    };
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
