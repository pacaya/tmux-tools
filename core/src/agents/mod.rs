use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;

mod builtin;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct AgentSpec {
    pub name: String,
    pub binary: String,
    pub ready_regex: Option<String>,
    /// How many of the bottom non-blank lines `ready_regex` is tested against.
    /// `None` means the built-in default (1 = bottom line only). Raise it for TUIs that
    /// render a status/footer row below the input prompt (e.g. cursor). `Some(0)` means
    /// "no limit" — scan every non-blank line, so a uniquely-anchored pattern matches a
    /// status line no matter how many task/footer rows render below it (e.g. Claude with a
    /// custom status line above a variable-height subagent footer).
    pub ready_lines: Option<usize>,
    pub interaction_patterns: Vec<InteractionPatternSpec>,
    pub access_profiles: BTreeMap<String, AccessProfile>,
    pub capabilities: AgentCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessProfile {
    pub args: Vec<String>,
}

#[derive(Default, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    Permission,
    AutoRespond,
    SubagentActive,
    DestructiveWarning,
    #[default]
    #[serde(other)]
    Unknown,
}

impl InteractionKind {
    pub fn event_type(&self) -> Option<&'static str> {
        match self {
            Self::Permission => Some("permission"),
            Self::AutoRespond => Some("auto_respond"),
            Self::SubagentActive => Some("subagent_active"),
            Self::DestructiveWarning => Some("destructive_warning"),
            Self::Unknown => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct InteractionPatternSpec {
    pub pattern: String,
    pub kind: InteractionKind,
    pub description: String,
    pub response: Option<String>,
    pub send_enter: bool,
}

impl InteractionPatternSpec {
    pub fn new(
        pattern: String,
        kind: InteractionKind,
        description: String,
        response: Option<String>,
        send_enter: bool,
    ) -> Self {
        Self {
            pattern,
            kind,
            description,
            response,
            send_enter,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct AgentCapabilities {
    pub worker_execution: bool,
    pub prompt_refinement: bool,
    pub branch_choice: bool,
    pub loop_verdict: bool,
    pub structured_output: bool,
    pub session_reuse: bool,
    pub native_json_schema: bool,
    pub model_selection: bool,
    pub reasoning_config: bool,
    pub system_prompt: bool,
    pub budget_limit: bool,
    pub turn_limit: bool,
    pub cost_reporting: bool,
    pub tool_allowlist: bool,
    pub web_search: bool,
}

impl Default for AgentCapabilities {
    fn default() -> Self {
        Self {
            worker_execution: true,
            prompt_refinement: false,
            branch_choice: false,
            loop_verdict: false,
            structured_output: false,
            session_reuse: false,
            native_json_schema: false,
            model_selection: false,
            reasoning_config: false,
            system_prompt: false,
            budget_limit: false,
            turn_limit: false,
            cost_reporting: false,
            tool_allowlist: false,
            web_search: false,
        }
    }
}

impl AgentCapabilities {
    fn merge_config(&mut self, config: AgentCapabilitiesConfig) {
        macro_rules! merge_bool {
            ($field:ident) => {
                if let Some(value) = config.$field {
                    self.$field = value;
                }
            };
        }
        merge_bool!(worker_execution);
        merge_bool!(prompt_refinement);
        merge_bool!(branch_choice);
        merge_bool!(loop_verdict);
        merge_bool!(structured_output);
        merge_bool!(session_reuse);
        merge_bool!(native_json_schema);
        merge_bool!(model_selection);
        merge_bool!(reasoning_config);
        merge_bool!(system_prompt);
        merge_bool!(budget_limit);
        merge_bool!(turn_limit);
        merge_bool!(cost_reporting);
        merge_bool!(tool_allowlist);
        merge_bool!(web_search);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Registry {
    agents: BTreeMap<String, AgentSpec>,
}

#[derive(Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct LoadWarning {
    pub agent: String,
    pub detail: String,
}

impl Registry {
    /// Invalid entries are skipped with a warning rather than aborting the load.
    pub fn load() -> anyhow::Result<(Registry, Vec<LoadWarning>)> {
        Self::load_with_user_path(user_config_path().as_deref())
    }

    /// Invalid entries are skipped with a warning rather than aborting the load.
    pub fn load_with_user_path(
        path: Option<&Path>,
    ) -> anyhow::Result<(Registry, Vec<LoadWarning>)> {
        let builtins = builtin::all();

        if let Some(path) = path.filter(|path| path.exists()) {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("failed to read agent registry {}", path.display()))?;
            let user_agents = parse_agent_configs(&contents)
                .with_context(|| format!("failed to parse agent registry {}", path.display()))?;

            // User config is a deep merge: scalar fields replace builtins when present,
            // while access profiles merge by name so builtin profiles survive unless overridden.
            let (agents, warnings) = merge_agent_configs(builtins.clone(), user_agents);
            Ok((Registry { agents }, warnings))
        } else {
            Ok((
                Registry {
                    agents: builtins.clone(),
                },
                Vec::new(),
            ))
        }
    }

    pub fn get(&self, agent: &str) -> Option<&AgentSpec> {
        self.agents.get(agent)
    }

    pub fn agents(&self) -> &BTreeMap<String, AgentSpec> {
        &self.agents
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
    interaction_patterns: Option<Vec<InteractionPatternConfig>>,
    #[serde(default)]
    access: BTreeMap<String, AccessProfileConfig>,
    #[serde(default)]
    capabilities: AgentCapabilitiesConfig,
}

#[derive(Debug, Deserialize)]
struct AccessProfileConfig {
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InteractionPatternConfig {
    pattern: String,
    kind: InteractionKind,
    description: String,
    #[serde(default)]
    response: Option<String>,
    #[serde(default = "default_send_enter")]
    send_enter: bool,
}

#[derive(Debug, Default, Deserialize)]
struct AgentCapabilitiesConfig {
    #[serde(default)]
    worker_execution: Option<bool>,
    #[serde(default)]
    prompt_refinement: Option<bool>,
    #[serde(default)]
    branch_choice: Option<bool>,
    #[serde(default)]
    loop_verdict: Option<bool>,
    #[serde(default)]
    structured_output: Option<bool>,
    #[serde(default)]
    session_reuse: Option<bool>,
    #[serde(default)]
    native_json_schema: Option<bool>,
    #[serde(default)]
    model_selection: Option<bool>,
    #[serde(default)]
    reasoning_config: Option<bool>,
    #[serde(default)]
    system_prompt: Option<bool>,
    #[serde(default)]
    budget_limit: Option<bool>,
    #[serde(default)]
    turn_limit: Option<bool>,
    #[serde(default)]
    cost_reporting: Option<bool>,
    #[serde(default)]
    tool_allowlist: Option<bool>,
    #[serde(default)]
    web_search: Option<bool>,
}

fn default_send_enter() -> bool {
    true
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
) -> (BTreeMap<String, AgentSpec>, Vec<LoadWarning>) {
    let mut warnings = Vec::new();

    for (name, user_agent) in user_agents {
        if let Some(agent) = agents.get_mut(&name) {
            merge_existing_agent(&name, agent, user_agent, &mut warnings);
        } else if let Some(agent) = agent_from_config(name.clone(), user_agent, &mut warnings) {
            agents.insert(name, agent);
        }
    }

    (agents, warnings)
}

fn merge_existing_agent(
    name: &str,
    agent: &mut AgentSpec,
    user_agent: AgentConfig,
    warnings: &mut Vec<LoadWarning>,
) {
    if let Some(binary) = user_agent.binary {
        agent.binary = binary;
    }

    if let Some(ready_regex) = user_agent.ready_regex {
        agent.ready_regex = Some(ready_regex);
    }

    if let Some(ready_lines) = user_agent.ready_lines {
        agent.ready_lines = Some(ready_lines);
    }

    if let Some(interaction_patterns) = user_agent.interaction_patterns {
        agent.interaction_patterns =
            interaction_patterns_from_config(name, interaction_patterns, warnings);
    }

    for (profile, access_profile) in user_agent.access {
        agent.access_profiles.insert(profile, access_profile.into());
    }

    agent.capabilities.merge_config(user_agent.capabilities);
}

fn agent_from_config(
    name: String,
    agent: AgentConfig,
    warnings: &mut Vec<LoadWarning>,
) -> Option<AgentSpec> {
    let binary = match agent.binary {
        Some(binary) => binary,
        None => {
            warnings.push(LoadWarning {
                agent: name,
                detail: "missing binary".to_owned(),
            });
            return None;
        }
    };

    let mut capabilities = AgentCapabilities::default();
    capabilities.merge_config(agent.capabilities);
    let interaction_patterns = interaction_patterns_from_config(
        &name,
        agent.interaction_patterns.unwrap_or_default(),
        warnings,
    );

    Some(AgentSpec {
        name,
        binary,
        ready_regex: agent.ready_regex,
        ready_lines: agent.ready_lines,
        interaction_patterns,
        access_profiles: agent
            .access
            .into_iter()
            .map(|(name, profile)| (name, profile.into()))
            .collect(),
        capabilities,
    })
}

fn interaction_patterns_from_config(
    agent: &str,
    patterns: Vec<InteractionPatternConfig>,
    warnings: &mut Vec<LoadWarning>,
) -> Vec<InteractionPatternSpec> {
    let mut specs = Vec::new();

    for pattern in patterns {
        if pattern.kind == InteractionKind::Unknown {
            warnings.push(LoadWarning {
                agent: agent.to_owned(),
                detail: format!(
                    "skipping interaction pattern {:?}: unknown interaction pattern kind",
                    pattern.description
                ),
            });
            continue;
        }

        if let Err(error) = regex::Regex::new(&pattern.pattern) {
            warnings.push(LoadWarning {
                agent: agent.to_owned(),
                detail: format!(
                    "skipping interaction pattern {:?}: invalid regex {:?}: {error}",
                    pattern.description, pattern.pattern
                ),
            });
            continue;
        }

        specs.push(InteractionPatternSpec {
            pattern: pattern.pattern,
            kind: pattern.kind,
            description: pattern.description,
            response: pattern.response,
            send_enter: pattern.send_enter,
        });
    }

    specs
}

impl From<AccessProfileConfig> for AccessProfile {
    fn from(profile: AccessProfileConfig) -> Self {
        AccessProfile { args: profile.args }
    }
}

#[cfg(test)]
fn parse_registry_toml(contents: &str) -> anyhow::Result<BTreeMap<String, AgentSpec>> {
    Ok(parse_registry_toml_with_warnings(contents)?.0)
}

#[cfg(test)]
fn parse_registry_toml_with_warnings(
    contents: &str,
) -> anyhow::Result<(BTreeMap<String, AgentSpec>, Vec<LoadWarning>)> {
    Ok(merge_agent_configs(
        BTreeMap::new(),
        parse_agent_configs(contents)?,
    ))
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
        assert!(demo.interaction_patterns.is_empty());
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
    fn parses_interaction_patterns() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"

[demo.access.default]
args = ["--safe"]

[[demo.interaction_patterns]]
pattern = "Allow this action\\?"
kind = "permission"
description = "Permission request"

[[demo.interaction_patterns]]
kind = "auto_respond"
pattern = "Press Enter to continue"
description = "Continue prompt"
response = "y"
send_enter = false
"#,
        )
        .unwrap();

        let patterns = &agents.get("demo").unwrap().interaction_patterns;
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].kind, InteractionKind::Permission);
        assert_eq!(patterns[0].pattern, "Allow this action\\?");
        assert_eq!(patterns[0].description, "Permission request");
        assert_eq!(patterns[0].response, None);
        assert!(patterns[0].send_enter);
        assert_eq!(patterns[1].kind, InteractionKind::AutoRespond);
        assert_eq!(patterns[1].response.as_deref(), Some("y"));
        assert!(!patterns[1].send_enter);
    }

    #[test]
    fn skips_invalid_interaction_patterns_with_warnings() {
        let (agents, warnings) = parse_registry_toml_with_warnings(
            r#"
[demo]
binary = "/bin/demo"

[demo.access.default]
args = []

[[demo.interaction_patterns]]
pattern = "valid"
kind = "permission"
description = "Valid"

[[demo.interaction_patterns]]
pattern = "typo"
kind = "permision"
description = "Typo"

[[demo.interaction_patterns]]
pattern = "["
kind = "auto_respond"
description = "Bad regex"
"#,
        )
        .unwrap();

        let patterns = &agents.get("demo").unwrap().interaction_patterns;
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].kind, InteractionKind::Permission);
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().all(|warning| warning.agent == "demo"));
        assert!(warnings
            .iter()
            .any(|warning| warning.detail.contains("unknown interaction pattern kind")));
        assert!(warnings
            .iter()
            .any(|warning| warning.detail.contains("invalid regex")));
    }

    #[test]
    fn skips_new_agents_missing_binary_with_warning() {
        let (agents, warnings) = parse_registry_toml_with_warnings(
            r#"
[demo]

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        assert!(!agents.contains_key("demo"));
        assert_eq!(
            warnings,
            vec![LoadWarning {
                agent: "demo".to_owned(),
                detail: "missing binary".to_owned(),
            }]
        );
    }

    #[test]
    fn parses_agent_capabilities() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"

[demo.capabilities]
structured_output = true
session_reuse = true
model_selection = true
web_search = true

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        assert!(demo.capabilities.worker_execution);
        assert!(demo.capabilities.structured_output);
        assert!(demo.capabilities.session_reuse);
        assert!(demo.capabilities.model_selection);
        assert!(demo.capabilities.web_search);
        assert!(!demo.capabilities.reasoning_config);
    }

    #[test]
    fn user_ready_lines_merges_onto_builtin() {
        let path = write_temp_agents_file(
            r#"
[codex]
ready_lines = 4
"#,
        );

        let (registry, warnings) = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(warnings.is_empty());
        let codex = registry.get("codex").unwrap();
        assert_eq!(codex.ready_lines, Some(4));
        // Scalar merge leaves the builtin profiles intact and does not synthesize a
        // ready_regex: the codex builtin ships none (None), so a user-supplied ready_lines
        // must leave it None.
        assert_eq!(codex.ready_regex.as_deref(), None);
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
                interaction_patterns: Vec::new(),
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
                capabilities: AgentCapabilities::default(),
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
                interaction_patterns: Vec::new(),
                access_profiles: BTreeMap::new(),
                capabilities: AgentCapabilities::default(),
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

        let (registry, warnings) = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(warnings.is_empty());
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
