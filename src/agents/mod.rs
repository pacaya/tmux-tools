use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;

mod builtin;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSpec {
    pub name: String,
    pub binary: String,
    pub ready_regex: Option<String>,
    /// How many of the bottom non-blank lines `ready_regex` is tested against.
    /// `None` means the built-in default (1 = bottom line only). Raise it for TUIs that
    /// render a status/footer row below the input prompt (e.g. cursor).
    pub ready_lines: Option<usize>,
    pub access_profiles: BTreeMap<String, AccessProfile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessProfile {
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registry {
    agents: BTreeMap<String, AgentSpec>,
}

impl Registry {
    pub fn load() -> anyhow::Result<Registry> {
        Self::load_with_user_path(user_config_path().as_deref())
    }

    pub fn load_with_user_path(path: Option<&Path>) -> anyhow::Result<Registry> {
        let builtins = builtin::all();

        if let Some(path) = path.filter(|path| path.exists()) {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("failed to read agent registry {}", path.display()))?;
            let user_agents = parse_agent_configs(&contents)
                .with_context(|| format!("failed to parse agent registry {}", path.display()))?;

            // User config is a deep merge: scalar fields replace builtins when present,
            // while access profiles merge by name so builtin profiles survive unless overridden.
            let agents = merge_agent_configs(builtins.clone(), user_agents)?;
            Ok(Registry { agents })
        } else {
            Ok(Registry {
                agents: builtins.clone(),
            })
        }
    }

    pub fn get(&self, agent: &str) -> Option<&AgentSpec> {
        self.agents.get(agent)
    }

    pub fn launch_argv(
        &self,
        agent: &str,
        access: Option<&str>,
    ) -> anyhow::Result<(String, Vec<String>)> {
        let Some(agent_spec) = self.agents.get(agent) else {
            bail!("unknown agent {agent}");
        };

        let profile = match access {
            Some(access) => {
                let Some(profile) = agent_spec.access_profiles.get(access) else {
                    bail!("agent {agent} has no access profile {access}");
                };

                profile
            }
            None => match agent_spec.access_profiles.get("default") {
                Some(profile) => profile,
                None => {
                    if agent_spec.access_profiles.is_empty() {
                        bail!("agent {agent} has no access profiles");
                    }

                    let mut names: Vec<&str> = agent_spec
                        .access_profiles
                        .keys()
                        .map(String::as_str)
                        .collect();
                    names.sort_unstable();
                    let list = names.join(", ");
                    bail!(
                        "agent {agent} has multiple access profiles ({list}); pass --access explicitly"
                    );
                }
            },
        };

        Ok((agent_spec.binary.clone(), profile.args.clone()))
    }
}

#[derive(Debug, Deserialize)]
struct AgentConfig {
    #[serde(default)]
    binary: Option<String>,
    #[serde(default)]
    ready_regex: Option<String>,
    #[serde(default)]
    ready_lines: Option<usize>,
    #[serde(default)]
    access: BTreeMap<String, AccessProfileConfig>,
}

#[derive(Debug, Deserialize)]
struct AccessProfileConfig {
    #[serde(default)]
    args: Vec<String>,
}

/// The documented user config path: `$XDG_CONFIG_HOME/tmux-tools/agents.toml`, falling
/// back to `~/.config/tmux-tools/agents.toml`. We resolve XDG explicitly rather than via
/// `dirs::config_dir()` because on macOS the latter points at
/// `~/Library/Application Support`, which doesn't match the README or where users (and our
/// own examples) actually place the file.
fn user_config_path() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))?;

    Some(config_home.join("tmux-tools").join("agents.toml"))
}

fn parse_agent_configs(contents: &str) -> anyhow::Result<BTreeMap<String, AgentConfig>> {
    toml::from_str(contents).context("invalid agents TOML")
}

fn merge_agent_configs(
    mut agents: BTreeMap<String, AgentSpec>,
    user_agents: BTreeMap<String, AgentConfig>,
) -> anyhow::Result<BTreeMap<String, AgentSpec>> {
    for (name, user_agent) in user_agents {
        if let Some(agent) = agents.get_mut(&name) {
            merge_existing_agent(agent, user_agent);
        } else {
            let agent = agent_from_config(name.clone(), user_agent)?;
            agents.insert(name, agent);
        }
    }

    Ok(agents)
}

fn merge_existing_agent(agent: &mut AgentSpec, user_agent: AgentConfig) {
    if let Some(binary) = user_agent.binary {
        agent.binary = binary;
    }

    if let Some(ready_regex) = user_agent.ready_regex {
        agent.ready_regex = Some(ready_regex);
    }

    if let Some(ready_lines) = user_agent.ready_lines {
        agent.ready_lines = Some(ready_lines);
    }

    for (profile, access_profile) in user_agent.access {
        agent.access_profiles.insert(profile, access_profile.into());
    }
}

fn agent_from_config(name: String, agent: AgentConfig) -> anyhow::Result<AgentSpec> {
    let binary = match agent.binary {
        Some(binary) => binary,
        None => bail!("agent {name} is missing binary"),
    };

    Ok(AgentSpec {
        name,
        binary,
        ready_regex: agent.ready_regex,
        ready_lines: agent.ready_lines,
        access_profiles: agent
            .access
            .into_iter()
            .map(|(name, profile)| (name, profile.into()))
            .collect(),
    })
}

impl From<AccessProfileConfig> for AccessProfile {
    fn from(profile: AccessProfileConfig) -> Self {
        AccessProfile { args: profile.args }
    }
}

#[cfg(test)]
fn parse_registry_toml(contents: &str) -> anyhow::Result<BTreeMap<String, AgentSpec>> {
    parse_agent_configs(contents)?
        .into_iter()
        .map(|(name, agent)| {
            let agent = agent_from_config(name.clone(), agent)?;
            Ok((name, agent))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn parses_inline_toml_registry_doc() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"
ready_regex = "^ready"

[demo.access.default]
args = ["--safe"]

[demo.access.extra]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        assert_eq!(demo.name, "demo");
        assert_eq!(demo.binary, "/bin/demo");
        assert_eq!(demo.ready_regex.as_deref(), Some("^ready"));
        assert_eq!(
            demo.access_profiles["default"].args,
            vec!["--safe".to_owned()]
        );
        assert_eq!(demo.access_profiles["extra"].args, Vec::<String>::new());
        assert_eq!(demo.ready_lines, None);
    }

    #[test]
    fn parses_ready_lines_scan_depth() {
        let agents = parse_registry_toml(
            r#"
[cursor]
binary = "cursor-agent"
ready_regex = "^\\s*→"
ready_lines = 3

[cursor.access.default]
args = []
"#,
        )
        .unwrap();

        let cursor = agents.get("cursor").unwrap();
        assert_eq!(cursor.binary, "cursor-agent");
        assert_eq!(cursor.ready_lines, Some(3));
    }

    #[test]
    fn user_ready_lines_merges_onto_builtin() {
        let path = write_temp_agents_file(
            r#"
[codex]
ready_lines = 4
"#,
        );

        let registry = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        let codex = registry.get("codex").unwrap();
        assert_eq!(codex.ready_lines, Some(4));
        // Scalar merge leaves the builtin ready_regex and profiles intact.
        assert_eq!(codex.ready_regex.as_deref(), Some("^▌"));
        assert!(codex.access_profiles.contains_key("read-only"));
    }

    #[test]
    fn launch_argv_returns_builtin_profile_args_and_errors_on_unknown_agent() {
        let registry = Registry {
            agents: builtin::all().clone(),
        };

        let (binary, args) = registry.launch_argv("codex", Some("read-only")).unwrap();

        assert_eq!(binary, "codex");
        assert_eq!(args, vec!["--sandbox".to_owned(), "read-only".to_owned()]);

        let error = registry.launch_argv("unknown", None).unwrap_err();
        assert_eq!(error.to_string(), "unknown agent unknown");
    }

    #[test]
    fn launch_argv_without_access_uses_safe_default_for_builtins() {
        let registry = Registry {
            agents: builtin::all().clone(),
        };

        let (binary, args) = registry.launch_argv("codex", None).unwrap();
        assert_eq!(binary, "codex");
        assert_eq!(args, vec!["--sandbox".to_owned(), "read-only".to_owned()]);

        let (binary, args) = registry.launch_argv("claude", None).unwrap();
        assert_eq!(binary, "claude");
        assert_eq!(
            args,
            vec!["--permission-mode".to_owned(), "plan".to_owned()]
        );
    }

    #[test]
    fn launch_argv_errors_when_no_default_and_multiple_profiles() {
        let mut agents = BTreeMap::new();
        agents.insert(
            "custom".to_owned(),
            AgentSpec {
                name: "custom".to_owned(),
                binary: "custom".to_owned(),
                ready_regex: None,
                ready_lines: None,
                access_profiles: BTreeMap::from([
                    (
                        "alpha".to_owned(),
                        AccessProfile {
                            args: vec!["--alpha".to_owned()],
                        },
                    ),
                    (
                        "beta".to_owned(),
                        AccessProfile {
                            args: vec!["--beta".to_owned()],
                        },
                    ),
                ]),
            },
        );
        let registry = Registry { agents };

        let error = registry.launch_argv("custom", None).unwrap_err();
        assert_eq!(
            error.to_string(),
            "agent custom has multiple access profiles (alpha, beta); pass --access explicitly"
        );
    }

    #[test]
    fn launch_argv_errors_when_agent_has_no_access_profiles() {
        let mut agents = BTreeMap::new();
        agents.insert(
            "empty".to_owned(),
            AgentSpec {
                name: "empty".to_owned(),
                binary: "empty".to_owned(),
                ready_regex: None,
                ready_lines: None,
                access_profiles: BTreeMap::new(),
            },
        );
        let registry = Registry { agents };

        let error = registry.launch_argv("empty", None).unwrap_err();
        assert_eq!(error.to_string(), "agent empty has no access profiles");
    }

    #[test]
    fn load_with_user_path_deep_merges_user_agent_over_builtin() {
        let path = write_temp_agents_file(
            r#"
[codex]
binary = "/usr/local/bin/codex"

[codex.access.full-access]
args = ["--unsafe"]
"#,
        );

        let registry = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        let codex = registry.get("codex").unwrap();
        assert_eq!(codex.binary, "/usr/local/bin/codex");
        assert!(codex.access_profiles.contains_key("read-only"));
        assert!(codex.access_profiles.contains_key("workspace-write"));
        assert_eq!(
            codex.access_profiles["full-access"].args,
            vec!["--unsafe".to_owned()]
        );
    }

    fn write_temp_agents_file(contents: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tmux-tools-agents-{}-{suffix}.toml",
            std::process::id()
        ));

        fs::write(&path, contents).unwrap();
        path
    }
}
