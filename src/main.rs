use std::fs::File;
use std::io::Read;

use log::{debug, error, warn};
use serde::{Deserialize, Serialize};

use crate::utils::get_local_repo_path;

mod git;
mod utils;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
enum HookEvent {
    PreCommit,
    PostCommit,
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
    fn setup(&self) -> anyhow::Result<()> {
        for hook in &self.hooks {
            if hook.setup_script.is_some() {
                utils::execute_cmd(
                    hook.setup_script.as_ref().expect("should not happen"),
                    &[],
                    Some(&get_local_repo_path(&self.url)?),
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
            .for_each(drop);
        Ok(conf)
    }
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;
    debug!("reading conf");
    let conf = HookConfig::from_file(None)?;
    debug!("merged conf: {:?}", conf);
    Ok(())
}
