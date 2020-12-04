use std::collections::HashMap;
use std::env;
use std::fs::{File, Permissions};
use std::io::{Read, Write};
use std::mem::zeroed;
use std::os::unix::fs::PermissionsExt;

use clap::{App, Arg, ArgMatches, SubCommand};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use shlex::Shlex;

use crate::utils::{execute_cmd, get_files, get_local_repo_path};

mod git;
mod utils;

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

#[derive(Deserialize, Serialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum HookEvent {
    PreCommit,
    PostCommit,
}

impl HookEvent {
    fn to_kebab_case(&self) -> &'static str {
        match self {
            HookEvent::PreCommit => "pre-commit",
            HookEvent::PostCommit => "post-commit",
        }
    }
    fn from_kebab_case(s: &str) -> Option<Self> {
        match s {
            "pre-commit" => Some(HookEvent::PreCommit),
            "post-commit" => Some(HookEvent::PostCommit),
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

fn run_hook(hook: &Hook, hook_repo_path: &str) -> anyhow::Result<()> {
    let root = git::root().expect("Could not get git root.");
    // expand PATH
    let mut bin_path = env::var("PATH").expect("PATH is not set in the env.");
    bin_path.push_str(&format!(";{}", hook_repo_path));
    debug!("New $PATH: {}", &bin_path);
    let mut env = HashMap::new();
    env.insert("PATH".to_string(), bin_path);
    // parse the action cli
    let mut action = Shlex::new(hook.action.as_ref().expect("None action on hook exec").as_str());
    if let (_consumed, Some(len)) = action.size_hint() {
        let cmd = action.next().unwrap();
        let args: Vec<String> = action.collect();
        let mut final_args: Vec<String> = Vec::new();
        if len > 1 {
            for arg in &args {
                if let Some(token) = ActionFileToken::from_str(&arg) {
                    match token {
                        ActionFileToken::Files => {
                            let mut files = get_files(
                                &root,
                                &hook.on_file_regex.as_ref().unwrap_or(&vec!["*".to_string()]),
                            )?;
                            final_args.append(&mut files);
                        }
                        ActionFileToken::File => {
                            unimplemented!("we should check for the token before, as it changes the whole execution logic");
                        }
                        ActionFileToken::ChangedFiles => {
                            // TODO: implement me
                            unimplemented!();
                        }
                        ActionFileToken::ChangedFile => {
                            // TODO: implement me
                            unimplemented!();
                        }
                        ActionFileToken::Root => {
                            final_args.push(root.clone());
                        }
                    }
                } else {
                    final_args.push(arg.to_string());
                }
            }
        }
        execute_cmd(&cmd, &args, Some(&root), Some(env))?;
    }
    Ok(())
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(default)]
struct ExternalHookRepo {
    hooks: Vec<Hook>,
    url: String,
}

impl ExternalHookRepo {
    pub fn init(&mut self) -> anyhow::Result<()> {
        let clone_dir = get_local_repo_path(&self.url)?;
        debug!("cloning {} to {}", &self.url, &clone_dir);
        git::pull(&self.url, &clone_dir)?;
        let mut repo_config = String::new();
        File::open(format!("{}/{}", clone_dir, "hooks.yml"))?.read_to_string(&mut repo_config)?;
        debug!("Got hooks.yml");
        let hook_repo: ExternalHookRepo = serde_yaml::from_str(&repo_config)?;
        debug!("{:?}", hook_repo);
        self.hooks = hook_repo.hooks;
        self.setup()
    }

    /// runs the eventual setup script
    fn setup(&self) -> anyhow::Result<()> {
        for hook in &self.hooks {
            if hook.setup_script.is_some() {
                utils::execute_cmd(
                    hook.setup_script.as_ref().expect("should not happen"),
                    &[] as &[&str],
                    Some(&get_local_repo_path(&self.url)?),
                    None,
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
        // TODO: return meaningful error message on config file absence.
        File::open(filename.unwrap_or(".hooks.yml"))?.read_to_string(&mut conf_content)?;
        let mut conf: HookConfig = serde_yaml::from_str(&conf_content)?;
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
        // TODO: ask for user confirmation
        for event in events {
            let mut hook_script = File::create(format!(
                "{}/.git/hooks/{}",
                git::root()?,
                event.to_kebab_case()
            ))?;
            hook_script.set_permissions(Permissions::from_mode(0o755))?;
            hook_script.write_all(
                format!("#!/bin/bash\ngit-hooks run --{}\n", event.to_kebab_case()).as_bytes(),
            )?;
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;
    debug!("reading conf");
    let conf = HookConfig::from_file(None)?;
    debug!("merged conf: {:?}", conf);
    let app = App::new("git-hooks")
        .author("Paul Ollivier <contact@paulollivier.fr>")
        .about("A git hooks manager")
        .subcommand(SubCommand::with_name("init").help("install the git hooks"))
        .subcommand(
            SubCommand::with_name("run")
                .help("Runs the configured hooks for a given event")
                .arg(Arg::with_name("event")
                    .index(1)
                    .help("Runs the hook for the given event, eg. \"pre-commit\", \"post-commit\"…")
                    .required(true)
                    .possible_values(&[HookEvent::PreCommit.to_kebab_case(),
                        HookEvent::PostCommit.to_kebab_case()])
                ),
        );
    let matches = app.get_matches();
    debug!("{:?}", matches);
    match matches.subcommand() {
        ("init", _) => {
            // todo: implement me properly
            conf.init(&[HookEvent::PreCommit, HookEvent::PostCommit])?;
        }
        ("run", args) => {
            if let Some(arg_matches) = args {
                if let Some(event) = arg_matches.value_of("event") {
                    let mut has_executed_hook = false;
                    let event = HookEvent::from_kebab_case(event).expect(
                        "Could not unwrap event, although it should be present, thanks to clap",
                    );
                    conf.repos
                        .iter()
                        .map(|repo| {
                            repo.hooks
                                .iter()
                                .filter(|&hook| {
                                    if let Some(events) = &(*hook).on_event {
                                        return events.contains(&event);
                                    }
                                    false
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
                                    }
                                    has_executed_hook = true;
                                }).for_each(drop);
                        })
                        .for_each(drop);
                    if has_executed_hook {
                        info!("Nothing to do.");
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
