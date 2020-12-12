use std::collections::HashMap;
use std::env;
use std::fs::{File, Permissions};
use std::io::{stdin, stdout, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use clap::{App, Arg, SubCommand};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use shlex::Shlex;

use crate::utils::{execute_cmd, get_files, get_local_repo_path, matches, prefix_path};

mod git;
mod utils;

#[cfg(test)]
mod tests {
    use crate::{git, ExternalHookRepo, Hook, HookConfig, HookEvent};
    use std::env::{current_dir, set_current_dir};
    use tempdir::TempDir;

    #[test]
    fn test_merge() {
        let mut conf = HookConfig {
            hooks: vec![Hook {
                name: "test1".to_string(),
                on_event: None,
                on_file_regex: None,
                action: Some("exe2".to_string()),
                setup_script: None,
            }],
            repos: vec![ExternalHookRepo {
                url: "dummy".to_string(),
                hooks: vec![Hook {
                    name: "test1".to_string(),
                    on_event: Some(vec![HookEvent::PreCommit]),
                    on_file_regex: Some(vec![".*".to_string()]),
                    action: Some("exe1".to_string()),
                    setup_script: Some("hello.sh".to_string()),
                }],
                version: None,
            }],
        };
        assert_ne!(conf.hooks[0].action, conf.repos[0].hooks[0].action);
        conf.update_repos_config();
        assert_eq!(conf.hooks[0].action, conf.repos[0].hooks[0].action);
    }

    #[test]
    fn test_external_repo_with_version() {
        let dir = TempDir::new("git-hooks-tests").expect("could not create tempdir");
        let old_dir = current_dir().expect("could not get current dir");
        set_current_dir(dir.path());
        git::init(None).expect("could not init repo");
        let mut er = ExternalHookRepo {
            url: "https://github.com/paulollivier/rust-hooks".to_string(),
            version: Some("0e74c2b9c6b1cf4ff36d7eedbee8e8093acacaac".to_string()),
            hooks: vec![],
        };
        let r = er.init();
        assert!(r.is_ok());
        let cloned_dir = dir
            .path()
            .join(".git")
            .join("hook-repos")
            .join("rust-hooks");
        assert!(cloned_dir.join("hooks.yml").exists());
        set_current_dir(cloned_dir);
        let r = git::get_hash("HEAD");
        assert!(r.is_ok());
        assert_eq!(
            "0e74c2b9c6b1cf4ff36d7eedbee8e8093acacaac".to_string(),
            r.unwrap()
        );
        set_current_dir(old_dir).expect("could not revert current dir");
    }
}

/// Represents the possible placeholders to be substituted to actual file values.
/// The singular variants mean that the action is to be executed for each file found.
enum ActionFileToken {
    Files,
    File,
    ChangedFiles,
    ChangedFile,
    Root,
}

impl ActionFileToken {
    /// Returns the variant from a textual representation
    /// ```rust
    /// assert_eq!(ActionFileToken::File, ActionFileToken::from_str("{file}"));
    /// assert_eq!(ActionFileToken::ChangedFiles, ActionFileToken::from_str("{changed_files}"));
    /// ```
    fn from_str(token: &str) -> Option<ActionFileToken> {
        match token {
            "{file}" => Some(ActionFileToken::File),
            "{files}" => Some(ActionFileToken::Files),
            "{changed_files}" => Some(ActionFileToken::ChangedFiles),
            "{changed_file}" => Some(ActionFileToken::ChangedFile),
            "{root}" => Some(ActionFileToken::Root),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Eq, PartialEq, Copy, Clone)]
#[serde(rename_all = "kebab-case")]
enum HookEvent {
    ApplyPatchMsg,
    CommitMsg,
    PostCommit,
    PostUpdate,
    PreApplyPatch,
    PreCommit,
    PreMergeCommit,
    PrePush,
    PreRebase,
    PreReceive,
    PrepareCommitMsg,
    Update,
}

static ALL_HOOK_EVENTS: &[HookEvent] = &[
    HookEvent::ApplyPatchMsg,
    HookEvent::CommitMsg,
    HookEvent::PostCommit,
    HookEvent::PostUpdate,
    HookEvent::PreApplyPatch,
    HookEvent::PreCommit,
    HookEvent::PreMergeCommit,
    HookEvent::PrePush,
    HookEvent::PreRebase,
    HookEvent::PreReceive,
    HookEvent::PrepareCommitMsg,
    HookEvent::Update,
];

impl HookEvent {
    fn to_kebab_case(&self) -> &'static str {
        match self {
            HookEvent::ApplyPatchMsg => "apply-patch-msg",
            HookEvent::CommitMsg => "commit-msg",
            HookEvent::PostCommit => "post-commit",
            HookEvent::PostUpdate => "post-update",
            HookEvent::PreApplyPatch => "pre-apply-patch",
            HookEvent::PreCommit => "pre-commit",
            HookEvent::PreMergeCommit => "pre-merge-commit",
            HookEvent::PrePush => "pre-push",
            HookEvent::PreRebase => "pre-rebase",
            HookEvent::PreReceive => "pre-receive",
            HookEvent::PrepareCommitMsg => "prepare-commit-msg",
            HookEvent::Update => "update",
        }
    }
    fn from_kebab_case(s: &str) -> Option<Self> {
        match s {
            "apply-patch-msg" => Some(HookEvent::ApplyPatchMsg),
            "commit-msg" => Some(HookEvent::CommitMsg),
            "post-commit" => Some(HookEvent::PostCommit),
            "post-update" => Some(HookEvent::PostUpdate),
            "pre-apply-patch" => Some(HookEvent::PreApplyPatch),
            "pre-commit" => Some(HookEvent::PreCommit),
            "pre-merge-commit" => Some(HookEvent::PreMergeCommit),
            "pre-push" => Some(HookEvent::PrePush),
            "pre-rebase" => Some(HookEvent::PreRebase),
            "pre-receive" => Some(HookEvent::PreReceive),
            "prepare-commit-msg" => Some(HookEvent::PrepareCommitMsg),
            "update" => Some(HookEvent::Update),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(default)]
struct Hook {
    name: String,
    on_event: Option<Vec<HookEvent>>,
    on_file_regex: Option<Vec<String>>,
    action: Option<String>,
    setup_script: Option<String>,
}

impl Clone for Hook {
    fn clone(&self) -> Self {
        let mut h = Hook::default();
        h.name = self.name.clone();
        if let Some(self_on_event) = &self.on_event {
            let mut on_event = Vec::new();
            for e in self_on_event {
                on_event.push(*e);
            }
            h.on_event = Some(on_event);
        }
        if let Some(regex) = &self.on_file_regex {
            let mut on_file_regex = Vec::new();
            for r in regex {
                on_file_regex.push(r.clone());
            }
            h.on_file_regex = Some(on_file_regex);
        }
        if let Some(action) = &self.action {
            h.action = Some(action.clone());
        }
        if let Some(setup_script) = &self.setup_script {
            h.setup_script = Some(setup_script.clone());
        }
        h
    }
}

fn run_hook(hook: &Hook, hook_repo_path: &str) -> anyhow::Result<()> {
    let root = git::root().expect("Could not get git root.");
    let mut should_run = true;
    // expand PATH
    let mut bin_path = env::var("PATH").expect("PATH is not set in the env.");
    bin_path.push_str(&format!(":{}", hook_repo_path));
    debug!("New $PATH: {}", &bin_path);
    let mut env = HashMap::new();
    env.insert("PATH".to_string(), bin_path);
    // parse the action cli
    let mut action = Shlex::new(
        hook.action
            .as_ref()
            .expect("None action on hook exec")
            .as_str(),
    );
    let cmd = action.next().unwrap();
    let args: Vec<String> = action.collect();
    let mut final_args: Vec<String> = Vec::new();
    for arg in &args {
        if let Some(token) = ActionFileToken::from_str(&arg) {
            match token {
                ActionFileToken::Files => {
                    let mut files = get_files(
                        &root,
                        &hook
                            .on_file_regex
                            .as_ref()
                            .unwrap_or(&vec![".*".to_string()]),
                    )?;
                    should_run = !files.is_empty();
                    final_args.append(&mut files);
                }
                ActionFileToken::File => {
                    unimplemented!("we should check for the token before, as it changes the whole execution logic");
                }
                ActionFileToken::ChangedFiles => {
                    let mut changed_files: Vec<String> = git::changed_files(true)?
                        .iter()
                        .map(|f| Path::new(f))
                        .filter(|p| {
                            matches(
                                p,
                                &(*hook
                                    .on_file_regex
                                    .as_ref()
                                    .unwrap_or(&vec![".*".to_string()])),
                            )
                        })
                        .map(|p| p.display().to_string())
                        .collect();
                    should_run = !changed_files.is_empty();
                    final_args.append(&mut changed_files);
                }
                ActionFileToken::ChangedFile => {
                    // TODO: implement me
                    unimplemented!();
                }
                ActionFileToken::Root => {
                    final_args.push(root.clone());
                }
            }
        } else if should_run {
            final_args.push(arg.to_string());
        } else {
            info!("Could find any files to run hook on");
        }
    }
    let (s, _, _) = execute_cmd(&cmd, &final_args, Some(&root), Some(&env))?;
    debug!(
        "finished executing {} with exit status {}",
        cmd,
        s.code().unwrap()
    );
    if !s.success() {
        Err(anyhow::Error::msg(format!(
            "{:?} reported execution failure: {:?}",
            hook,
            s.code()
        )))
    } else {
        let index_files = git::changed_files(true)?;
        let changed_files = git::changed_files(false)?;
        let files_to_re_add: Vec<&String> = changed_files
            .iter()
            .filter(|f| index_files.contains(f))
            .collect();
        if !files_to_re_add.is_empty() {
            debug!("we must re-add those files: {:#?}", files_to_re_add);
            git::add(&files_to_re_add)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(default)]
struct ExternalHookRepo {
    hooks: Vec<Hook>,
    url: String,
    version: Option<String>,
}

impl ExternalHookRepo {
    pub fn init(&mut self) -> anyhow::Result<()> {
        let clone_dir = get_local_repo_path(&self.url)?;
        debug!("cloning {} to {}", &self.url, &clone_dir);
        git::pull(&self.url, &clone_dir)?;
        if let Some(v) = &self.version {
            git::checkout(v, &clone_dir);
        }
        let mut repo_config = String::new();
        File::open(format!("{}/{}", clone_dir, "hooks.yml"))?.read_to_string(&mut repo_config)?;
        debug!("Got hooks.yml");
        let hook_repo: ExternalHookRepo = serde_yaml::from_str(&repo_config)?;
        debug!("{:?}", hook_repo);
        self.hooks = hook_repo.hooks;
        self.setup()
    }

    /// runs the optional setup scripts
    fn setup(&self) -> anyhow::Result<()> {
        let mut env = HashMap::new();
        env.insert(
            "PATH".to_string(),
            prefix_path(&get_local_repo_path(&self.url)?),
        );
        for hook in &self.hooks {
            if hook.setup_script.is_some() {
                utils::execute_cmd(
                    hook.setup_script.as_ref().expect("should not happen"),
                    &[] as &[&str],
                    Some(&get_local_repo_path(&self.url)?),
                    Some(&env),
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct HookConfig {
    repos: Vec<ExternalHookRepo>,
    hooks: Vec<Hook>,
}

impl HookConfig {
    fn from_file(filename: Option<&str>) -> anyhow::Result<HookConfig> {
        let mut conf_content = String::new();
        let p = filename.unwrap_or(".hooks.yml");
        match File::open(p) {
            Ok(mut f) => {
                f.read_to_string(&mut conf_content)?;
            }
            Err(e) => {
                error!("could not read config file {}: {}", p, e);
            }
        }
        let mut conf: HookConfig = serde_yaml::from_str(&conf_content)?;
        conf.update_repos_config();
        debug!("{:?}", conf);
        conf.repos
            .iter_mut()
            .map(|repo| {
                debug!("init {:?}", repo.url);
                let r = repo.init();
                if let Err(e) = r {
                    warn!(
                        "Got an error while attempting to initialize repo {}: {}",
                        repo.url, e
                    );
                }
            })
            .for_each(drop); // consume the iterator
        Ok(conf)
    }

    /// Installs itself as a hook
    fn init(self, events: &[HookEvent]) -> anyhow::Result<()> {
        for event in events {
            let mut hook_script = File::create(format!(
                "{}/.git/hooks/{}",
                git::root()?,
                event.to_kebab_case()
            ))?;
            hook_script.set_permissions(Permissions::from_mode(0o755))?;
            hook_script.write_all(
                format!("#!/bin/bash -e\ngit-hooks run {}\n", event.to_kebab_case()).as_bytes(),
            )?;
        }
        //TODO: create .hooks.yml if not existing?
        Ok(())
    }

    /// finds defined values in the hook definitions, and overrides the definitions in repos
    fn update_repos_config(&mut self) {
        // TODO error[E0500]: closure requires unique access to `self` but it is already borrowed
        let hooks = &self.hooks;
        self.repos
            .iter_mut()
            .map(|repo| {
                repo.hooks
                    .iter_mut()
                    .map(|h| {
                        let hooks: Vec<&Hook> =
                            hooks.iter().filter(|hook| hook.name == h.name).collect();
                        if !hooks.is_empty() {
                            let hook = hooks[0];
                            if h.name == hook.name {
                                if let Some(on_event) = &hook.on_event {
                                    h.on_event = Some(on_event.clone());
                                }
                                if let Some(on_file_regex) = &hook.on_file_regex {
                                    h.on_file_regex = Some(on_file_regex.clone());
                                }
                                if let Some(action) = &hook.action {
                                    h.action = Some(action.clone());
                                }
                                if let Some(setup_script) = &hook.setup_script {
                                    h.setup_script = Some(setup_script.clone());
                                }
                            }
                        }
                    })
                    .for_each(drop);
            })
            .for_each(drop);
    }
}

fn ask_for_user_confirmation(prompt: &str) -> anyhow::Result<bool> {
    print!("{}: ", prompt);
    stdout().flush()?;
    let mut input = String::new();
    stdin().read_line(&mut input)?;
    Ok(match input.trim() {
        "Y" | "y" => true,
        "N" | "n" => false,
        _ => {
            println!("Incorrect input. Try again.");
            ask_for_user_confirmation(prompt)?
        }
    })
}

fn update() -> anyhow::Result<()> {
    use self_update::cargo_crate_version;
    let status = self_update::backends::github::Update::configure()
        .repo_owner("paulollivier")
        .repo_name("git-hooks")
        .bin_name("git-hooks-linux-amd64")
        .show_download_progress(true)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;
    if status.updated() {
        println!("Downloaded a new version: `{}`!", status.version());
    } else {
        println!("No available update.");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;
    let app = App::new("git-hooks")
        .author("Paul Ollivier <contact@paulollivier.fr>")
        .about("A git hooks manager\nhttps://github.com/paulollivier/git-hooks")
        .subcommand(SubCommand::with_name("self-update").about("git-hooks will try to update itself."))
        .subcommand(SubCommand::with_name("init").about("Install the git hooks in .git/hooks"))
        .subcommand(
            SubCommand::with_name("run")
                .about("Runs the configured hooks for a given event")
                .arg(Arg::with_name("event")
                    .index(1)
                    .help("Runs the hook for the given event, eg. \"pre-commit\", \"post-commit\"…")
                    .required(true)
                    .possible_values(&ALL_HOOK_EVENTS.iter().map(|e| e.to_kebab_case()).collect::<Vec<&'static str>>())
                ),
        );
    let matches = app.get_matches();
    debug!("{:?}", matches);
    debug!("reading conf");
    let conf = HookConfig::from_file(None)?;
    let active_hooks_names: Vec<String> = conf.hooks.iter().map(|h| h.name.clone()).collect();
    debug!("merged conf: {:#?}", conf);
    match matches.subcommand() {
        ("self-update", _) => {
            update()?;
        }
        ("init", _) => {
            if ask_for_user_confirmation(
                "This will overwrite all the hooks in .git/hooks. Are you sure? [Y/N]",
            )? {
                conf.init(ALL_HOOK_EVENTS)?;
                println!("I have init'd myself successfully! 🚀");
            } else {
                println!("Operation cancelled by user.");
            }
        }
        ("run", args) => {
            if let Some(arg_matches) = args {
                if let Some(event) = arg_matches.value_of("event") {
                    let mut has_executed_hook = false;
                    let mut had_error = false;
                    let event = HookEvent::from_kebab_case(event).expect(
                        "Could not unwrap event, although it should be present, thanks to clap",
                    );
                    conf.repos
                        .iter()
                        .map(|repo| {
                            repo.hooks
                                .iter()
                                // filter hooks with the right event
                                .filter(|&hook| {
                                    (*hook).on_event.as_ref().unwrap_or(&vec![HookEvent::PreCommit]).contains(&event)
                                })
                                // filter hooks with their IDs present.
                                .filter(|&hook| {
                                    active_hooks_names.contains(&hook.name)
                                })
                                .map(|hook| {
                                    debug!("would run hook {:?}", hook);
                                    if let Err(e) = run_hook(&hook,
                                                             &get_local_repo_path(&repo.url)
                                                                 .expect("could not get local root repo when attempting to run hook")) {
                                        warn!(
                                            "An error occurred while executing {}: {}",
                                            hook.name, e
                                        );
                                        had_error = true;
                                    }
                                    has_executed_hook = true;
                                }).for_each(drop);
                        })
                        .for_each(drop);
                    if !has_executed_hook {
                        info!("Nothing to do.");
                    }
                    if had_error {
                        return Err(anyhow::Error::msg("a hook reported malfunction"));
                    }
                }
            }
        }
        _ => {
            // Should not happen, clap handles this
            error!("A subcommand must be set! see help (-h)");
            return Err(anyhow::Error::msg("no command given"));
        }
    };
    Ok(())
}
