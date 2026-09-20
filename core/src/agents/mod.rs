use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

mod builtin;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct AgentSpec {
    pub name: String,
    pub binary: String,
    pub interaction_patterns: Vec<InteractionPatternSpec>,
    pub access_profiles: BTreeMap<String, AccessProfile>,
    pub capabilities: AgentCapabilities,
    /// The agent's named renderings. Every agent has at least one; `ready_regex` and
    /// `ready_lines` live on the surface because they are valid for one rendering only.
    pub surfaces: BTreeMap<String, Surface>,
    /// Name of the surface matching the agent's **pre-surface rendering** — the rendering
    /// it launched with before surfaces existed. Readiness fields written at the agent
    /// level (the pre-surface `agents.toml` shape) bind here, and pane resolution with no
    /// surface record falls back here. Never the (promotable) default by assumption.
    pub pre_surface_rendering: Option<String>,
    /// Name of the surface `spawn-agent` resolves when no `--surface` is given.
    pub default_surface: Option<String>,
}

/// One named rendering of an agent (P2). Carries the launch arguments that select it and
/// every rendering-dependent behavior this registry currently models.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct Surface {
    /// Launch arguments selecting this rendering, applied before the access profile's.
    pub args: Vec<String>,
    pub ready_regex: Option<String>,
    /// How many of the bottom non-blank lines `ready_regex` is tested against.
    /// `None` means the built-in default (1 = bottom line only). `Some(0)` means
    /// "no limit" — scan every non-blank line.
    pub ready_lines: Option<usize>,
    pub busy_regex: Option<String>,
    /// The agent version the P8 smoke measured this surface's declarations against,
    /// e.g. `claude 2.1.278`. `None` for a surface the smoke has not validated. A flat
    /// surface is promoted to the agent's default only once this and both readiness
    /// patterns are present (P2).
    pub validated_version: Option<String>,
    /// Whether the rendering enables bracketed paste. Declared capability only; the live
    /// flag is read from the pane before pasting.
    pub bracketed_paste: bool,
    /// The key `interrupt` sends for this rendering.
    pub interrupt_key: String,
    /// Whether `interrupt_key` quits the agent when the pane is idle (the quit hazard).
    pub quit_when_idle: bool,
    /// Resume-argument syntax and identifier shape, when this rendering supports resume.
    pub resume: Option<ResumeSpec>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            ready_regex: None,
            ready_lines: None,
            busy_regex: None,
            validated_version: None,
            bracketed_paste: false,
            interrupt_key: "C-c".to_owned(),
            quit_when_idle: false,
            resume: None,
        }
    }
}

impl Surface {
    /// Whether this surface ships a smoke-validated `ready_regex`/`busy_regex` pair:
    /// both patterns present and stamped with the agent version they were measured
    /// against. P2 promotes a flat surface to default only when this holds; idle-only
    /// completion is not an acceptable basis.
    pub fn patterns_validated(&self) -> bool {
        self.ready_regex.is_some() && self.busy_regex.is_some() && self.validated_version.is_some()
    }
}

/// The resume syntax and identifier shape a surface declares (P6).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct ResumeSpec {
    /// Argument template applied to the session identifier, e.g. `--resume {id}`.
    pub syntax: String,
    /// Regex the session identifier must match before launching.
    pub id_regex: String,
}

impl ResumeSpec {
    /// Validate `id` against the declared identifier shape and expand the declared
    /// syntax into launch arguments. A malformed identifier is an error, never a
    /// launch that could land in an interactive session picker (P6).
    pub fn args_for(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let shape = regex::Regex::new(&self.id_regex)
            .map_err(|error| anyhow!("invalid resume id_regex {:?}: {error}", self.id_regex))?;

        if !shape.is_match(id) {
            bail!(
                "resume identifier {id:?} does not match the surface's shape {:?}",
                self.id_regex
            );
        }

        if !self.syntax.contains("{id}") {
            bail!("resume syntax {:?} has no {{id}} placeholder", self.syntax);
        }

        Ok(self
            .syntax
            .split_whitespace()
            .map(|token| token.replace("{id}", id))
            .collect())
    }
}

/// A surface resolved by name, borrowed from its agent.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedSurface<'a> {
    pub name: &'a str,
    pub surface: &'a Surface,
}

impl AgentSpec {
    /// The surface `spawn-agent` picks when no `--surface` is given. A named default is
    /// honored only once it carries a validated pattern pair (P2's promotion bar); every
    /// other agent keeps its former default, the pre-surface rendering.
    pub fn default_surface(&self) -> Option<ResolvedSurface<'_>> {
        match self.named_surface(self.default_surface.as_deref()) {
            Some(resolved) if resolved.surface.patterns_validated() => Some(resolved),
            _ => self.pre_surface(),
        }
    }

    /// The surface matching the agent's pre-surface rendering.
    pub fn pre_surface(&self) -> Option<ResolvedSurface<'_>> {
        self.named_surface(self.pre_surface_rendering.as_deref())
            .or_else(|| self.named_surface(self.default_surface.as_deref()))
            .or_else(|| {
                let (name, surface) = self.surfaces.iter().next()?;
                Some(ResolvedSurface {
                    name: name.as_str(),
                    surface,
                })
            })
    }

    fn named_surface<'a>(&'a self, name: Option<&str>) -> Option<ResolvedSurface<'a>> {
        let name = name?;
        let (key, surface) = self.surfaces.get_key_value(name)?;
        Some(ResolvedSurface {
            name: key.as_str(),
            surface,
        })
    }
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

    /// Resolve a `spawn-agent` launch: the named surface when given, otherwise the
    /// agent's default, plus the surface's resume arguments when an identifier is
    /// given. Errors for an unknown agent, an unknown surface name, a surface that
    /// declares no resume syntax, or an identifier outside the surface's shape.
    pub fn resolve_launch(
        &self,
        agent: &str,
        access: Option<&str>,
        surface: Option<&str>,
        resume: Option<&str>,
    ) -> anyhow::Result<ResolvedLaunch> {
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

        let resolved = match surface {
            Some(name) => agent_spec
                .surfaces
                .get(name)
                .map(|surface| (name, surface))
                .ok_or_else(|| anyhow::anyhow!("agent {agent} has no surface {name}"))?,
            None => {
                let resolved = agent_spec
                    .default_surface()
                    .ok_or_else(|| anyhow::anyhow!("agent {agent} has no default surface"))?;
                (resolved.name, resolved.surface)
            }
        };

        // Surface arguments select the rendering and come first; access-profile arguments
        // then grant authority within it; the surface's resume arguments follow, and
        // caller-supplied trailing arguments are appended by the caller. Resume arguments
        // are validated by construction, so they do not mark the pane surface-unvalidated.
        let mut args: Vec<String> = resolved
            .1
            .args
            .iter()
            .cloned()
            .chain(profile.args.iter().cloned())
            .collect();

        if let Some(id) = resume {
            let Some(spec) = resolved.1.resume.as_ref() else {
                bail!(
                    "agent {agent} surface {} does not support resume",
                    resolved.0
                );
            };
            args.extend(
                spec.args_for(id)
                    .map_err(|error| anyhow!("agent {agent} surface {}: {error:#}", resolved.0))?,
            );
        }

        Ok(ResolvedLaunch {
            binary: agent_spec.binary.clone(),
            args,
            surface: resolved.0.to_owned(),
        })
    }

    /// Total surface resolution from a pane registration record: a present record resolves
    /// to that surface, an absent record to the agent's pre-surface rendering, and a record
    /// naming an unknown surface to no surface (unknown state). `None` also when the pane
    /// names no agent the registry knows.
    pub fn resolve_surface<'a>(
        &'a self,
        agent: &str,
        recorded: Option<&str>,
    ) -> Option<ResolvedSurface<'a>> {
        let agent_spec = self.agents.get(agent)?;
        match recorded {
            // A record is authoritative: the pane was launched on the surface it names. A
            // record naming a surface the registry no longer has must not be
            // reinterpreted as another rendering — that would apply foreign patterns.
            Some(name) => agent_spec
                .surfaces
                .get_key_value(name)
                .map(|(key, surface)| ResolvedSurface {
                    name: key.as_str(),
                    surface,
                }),
            // An absent record can only mean a pane created before surfaces existed,
            // which was launched on the pre-surface rendering.
            None => agent_spec.pre_surface(),
        }
    }
}

/// A resolved `spawn-agent` launch: the binary, the full argument list, and the surface
/// name to record on the pane.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ResolvedLaunch {
    pub binary: String,
    pub args: Vec<String>,
    pub surface: String,
}

#[derive(Debug, Deserialize)]
struct AgentConfig {
    #[serde(default)]
    binary: Option<String>,
    /// Pre-surface readiness shape: these bind to the surface matching the agent's
    /// pre-surface rendering, never to a promotable default.
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
    #[serde(default)]
    surfaces: BTreeMap<String, SurfaceConfig>,
    /// Marks which surface matches the agent's pre-surface rendering.
    #[serde(default)]
    pre_surface_rendering: Option<String>,
    /// Names the surface `spawn-agent` uses when no `--surface` is given.
    #[serde(default)]
    default_surface: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SurfaceConfig {
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    ready_regex: Option<String>,
    #[serde(default)]
    ready_lines: Option<usize>,
    #[serde(default)]
    busy_regex: Option<String>,
    #[serde(default)]
    validated_version: Option<String>,
    #[serde(default)]
    bracketed_paste: Option<bool>,
    #[serde(default)]
    interrupt_key: Option<String>,
    #[serde(default)]
    quit_when_idle: Option<bool>,
    #[serde(default)]
    resume: Option<ResumeConfig>,
}

#[derive(Debug, Deserialize)]
struct ResumeConfig {
    syntax: String,
    id_regex: String,
}

impl From<ResumeConfig> for ResumeSpec {
    fn from(config: ResumeConfig) -> Self {
        ResumeSpec {
            syntax: config.syntax,
            id_regex: config.id_regex,
        }
    }
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

/// Name of the surface synthesized for an agent declared without any.
const SYNTHESIZED_SURFACE_NAME: &str = "default";

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
    let declared_default = user_agent.default_surface.clone();

    if let Some(binary) = user_agent.binary {
        agent.binary = binary;
    }

    // Surfaces merge by name and field, so a built-in rendering survives a partial
    // override and a new rendering can be added.
    for (surface_name, config) in user_agent.surfaces {
        if let Some(existing) = agent.surfaces.get_mut(&surface_name) {
            merge_surface(existing, config);
        } else {
            agent
                .surfaces
                .insert(surface_name, surface_from_config(config));
        }
    }

    if let Some(pre) = user_agent.pre_surface_rendering {
        agent.pre_surface_rendering = Some(pre);
    }
    if let Some(default) = user_agent.default_surface {
        agent.default_surface = Some(default);
    }

    // The pre-surface `agents.toml` shape carries readiness at the agent level; those
    // fields are calibrated against the pre-surface rendering and bind there.
    if user_agent.ready_regex.is_some() || user_agent.ready_lines.is_some() {
        bind_pre_surface_readiness(
            name,
            agent,
            user_agent.ready_regex,
            user_agent.ready_lines,
            warnings,
        );
    }

    if let Some(interaction_patterns) = user_agent.interaction_patterns {
        agent.interaction_patterns =
            interaction_patterns_from_config(name, interaction_patterns, warnings);
    }

    for (profile, access_profile) in user_agent.access {
        agent.access_profiles.insert(profile, access_profile.into());
    }

    agent.capabilities.merge_config(user_agent.capabilities);

    normalize_surfaces(name, agent, warnings);
    warn_if_default_not_promoted(name, agent, declared_default.as_deref(), warnings);
}

fn agent_from_config(
    name: String,
    agent: AgentConfig,
    warnings: &mut Vec<LoadWarning>,
) -> Option<AgentSpec> {
    let declared_default = agent.default_surface.clone();

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

    let mut surfaces: BTreeMap<String, Surface> = agent
        .surfaces
        .into_iter()
        .map(|(surface_name, config)| (surface_name, surface_from_config(config)))
        .collect();
    // An agent declared without surfaces loads as one surface built from its agent-level
    // readiness fields; that surface is both its pre-surface rendering and its default.
    let synthesized = surfaces.is_empty();
    if synthesized {
        surfaces.insert(SYNTHESIZED_SURFACE_NAME.to_owned(), Surface::default());
    }

    let mut spec = AgentSpec {
        name,
        binary,
        interaction_patterns,
        access_profiles: agent
            .access
            .into_iter()
            .map(|(profile_name, profile)| (profile_name, profile.into()))
            .collect(),
        capabilities,
        surfaces,
        pre_surface_rendering: if synthesized {
            Some(SYNTHESIZED_SURFACE_NAME.to_owned())
        } else {
            agent.pre_surface_rendering
        },
        default_surface: if synthesized {
            Some(SYNTHESIZED_SURFACE_NAME.to_owned())
        } else {
            agent.default_surface
        },
    };

    let agent_name = spec.name.clone();
    if agent.ready_regex.is_some() || agent.ready_lines.is_some() {
        bind_pre_surface_readiness(
            &agent_name,
            &mut spec,
            agent.ready_regex,
            agent.ready_lines,
            warnings,
        );
    }

    normalize_surfaces(&agent_name, &mut spec, warnings);
    warn_if_default_not_promoted(&agent_name, &spec, declared_default.as_deref(), warnings);
    Some(spec)
}

/// Warn when an `agents.toml`-declared `default_surface` is not the surface actually
/// used, because it fails P2's promotion bar. The fallback itself is intentional (AC3:
/// an agent whose flat surface lacks a validated pair records its pre-surface
/// rendering), but silently recording a different surface than the file names would be
/// a surprise. A name that resolves to no surface is already reported by
/// `normalize_surfaces`.
fn warn_if_default_not_promoted(
    name: &str,
    agent: &AgentSpec,
    declared_default: Option<&str>,
    warnings: &mut Vec<LoadWarning>,
) {
    let Some(declared) = declared_default else {
        return;
    };
    let Some(surface) = agent.surfaces.get(declared) else {
        return;
    };
    let Some(resolved) = agent.default_surface() else {
        return;
    };
    if resolved.name == declared {
        return;
    }

    let mut missing = Vec::new();
    if surface.ready_regex.is_none() {
        missing.push("ready_regex");
    }
    if surface.busy_regex.is_none() {
        missing.push("busy_regex");
    }
    if surface.validated_version.is_none() {
        missing.push("validated_version");
    }
    let missing = if missing.is_empty() {
        "a validated ready_regex/busy_regex pair".to_owned()
    } else {
        missing.join(", ")
    };

    warnings.push(LoadWarning {
        agent: name.to_owned(),
        detail: format!(
            "default_surface {declared:?} is not smoke-validated (missing {missing}); \
             recording {:?} instead",
            resolved.name
        ),
    });
}

/// Apply agent-level (pre-surface shape) readiness fields to the surface matching the
/// agent's pre-surface rendering.
fn bind_pre_surface_readiness(
    name: &str,
    agent: &mut AgentSpec,
    ready_regex: Option<String>,
    ready_lines: Option<usize>,
    warnings: &mut Vec<LoadWarning>,
) {
    let target = agent
        .pre_surface_rendering
        .clone()
        .or_else(|| agent.default_surface.clone())
        .or_else(|| {
            (agent.surfaces.len() == 1)
                .then(|| agent.surfaces.keys().next().cloned())
                .flatten()
        });

    let Some(target) = target else {
        warnings.push(LoadWarning {
            agent: name.to_owned(),
            detail: "readiness fields were given but no pre-surface-rendering surface is named"
                .to_owned(),
        });
        return;
    };

    match agent.surfaces.get_mut(&target) {
        Some(surface) => {
            if let Some(ready_regex) = ready_regex {
                surface.ready_regex = Some(ready_regex);
            }
            if let Some(ready_lines) = ready_lines {
                surface.ready_lines = Some(ready_lines);
            }
        }
        None => warnings.push(LoadWarning {
            agent: name.to_owned(),
            detail: format!("readiness fields name unknown surface {target:?}"),
        }),
    }
}

fn merge_surface(existing: &mut Surface, config: SurfaceConfig) {
    if let Some(args) = config.args {
        existing.args = args;
    }
    if let Some(ready_regex) = config.ready_regex {
        existing.ready_regex = Some(ready_regex);
    }
    if let Some(ready_lines) = config.ready_lines {
        existing.ready_lines = Some(ready_lines);
    }
    if let Some(busy_regex) = config.busy_regex {
        existing.busy_regex = Some(busy_regex);
    }
    if let Some(validated_version) = config.validated_version {
        existing.validated_version = Some(validated_version);
    }
    if let Some(bracketed_paste) = config.bracketed_paste {
        existing.bracketed_paste = bracketed_paste;
    }
    if let Some(interrupt_key) = config.interrupt_key {
        existing.interrupt_key = interrupt_key;
    }
    if let Some(quit_when_idle) = config.quit_when_idle {
        existing.quit_when_idle = quit_when_idle;
    }
    if let Some(resume) = config.resume {
        existing.resume = Some(resume.into());
    }
}

fn surface_from_config(config: SurfaceConfig) -> Surface {
    let mut surface = Surface::default();
    merge_surface(&mut surface, config);
    surface
}

/// Ensure every agent ends the load with exactly one valid default surface and one valid
/// pre-surface-rendering surface, warning about names that resolve to nothing.
fn normalize_surfaces(name: &str, agent: &mut AgentSpec, warnings: &mut Vec<LoadWarning>) {
    if agent.surfaces.is_empty() {
        agent
            .surfaces
            .insert(SYNTHESIZED_SURFACE_NAME.to_owned(), Surface::default());
    }

    let mut pre = valid_surface_name(&agent.surfaces, &agent.pre_surface_rendering);
    let mut default = valid_surface_name(&agent.surfaces, &agent.default_surface);

    if agent.pre_surface_rendering.is_some() && pre.is_none() {
        warnings.push(LoadWarning {
            agent: name.to_owned(),
            detail: format!(
                "pre_surface_rendering {:?} names no surface",
                agent.pre_surface_rendering.as_deref().unwrap_or_default()
            ),
        });
    }
    if agent.default_surface.is_some() && default.is_none() {
        warnings.push(LoadWarning {
            agent: name.to_owned(),
            detail: format!(
                "default_surface {:?} names no surface",
                agent.default_surface.as_deref().unwrap_or_default()
            ),
        });
    }

    if pre.is_none() {
        pre = default.clone();
    }
    if default.is_none() {
        default = pre.clone();
    }

    if pre.is_none() {
        if agent.surfaces.len() > 1 {
            warnings.push(LoadWarning {
                agent: name.to_owned(),
                detail: "no default surface named; choosing the first".to_owned(),
            });
        }
        let first = agent.surfaces.keys().next().cloned();
        pre = first.clone();
        default = first;
    }

    agent.pre_surface_rendering = pre;
    agent.default_surface = default;
}

fn valid_surface_name(
    surfaces: &BTreeMap<String, Surface>,
    candidate: &Option<String>,
) -> Option<String> {
    candidate
        .as_ref()
        .filter(|name| surfaces.contains_key(*name))
        .cloned()
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
        assert_eq!(
            demo.access_profiles["default"].args,
            vec!["--safe".to_owned()]
        );
        assert_eq!(demo.access_profiles["extra"].args, Vec::<String>::new());
        assert!(demo.interaction_patterns.is_empty());
        // A pre-surface agent loads as one synthesized surface carrying its agent-level
        // readiness fields; that surface is both the pre-surface rendering and the default.
        assert_eq!(demo.surfaces.len(), 1);
        assert_eq!(
            demo.surfaces["default"].ready_regex.as_deref(),
            Some("^ready")
        );
        assert_eq!(demo.surfaces["default"].ready_lines, None);
        assert_eq!(demo.pre_surface_rendering.as_deref(), Some("default"));
        assert_eq!(demo.default_surface.as_deref(), Some("default"));
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
        assert_eq!(cursor.surfaces["default"].ready_lines, Some(3));
        assert_eq!(
            cursor.surfaces["default"].ready_regex.as_deref(),
            Some("^\\s*→")
        );
    }

    #[test]
    fn parses_named_surfaces_with_every_field() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"
default_surface = "flat"
pre_surface_rendering = "rich"

[demo.surfaces.rich]
args = ["--rich"]
ready_regex = "^rich ready"
ready_lines = 0

[demo.surfaces.flat]
args = ["--flat"]
ready_regex = "^flat ready"
busy_regex = "^flat busy"
validated_version = "demo 1.0"
bracketed_paste = true
interrupt_key = "C-u"
quit_when_idle = true

[demo.surfaces.flat.resume]
syntax = "--resume {id}"
id_regex = "^[0-9a-f-]{36}$"

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        assert_eq!(demo.surfaces.len(), 2);
        assert_eq!(demo.default_surface.as_deref(), Some("flat"));
        assert_eq!(demo.pre_surface_rendering.as_deref(), Some("rich"));
        assert_eq!(demo.default_surface().unwrap().name, "flat");
        assert_eq!(demo.pre_surface().unwrap().name, "rich");

        let rich = &demo.surfaces["rich"];
        assert_eq!(rich.args, vec!["--rich".to_owned()]);
        assert_eq!(rich.ready_regex.as_deref(), Some("^rich ready"));
        assert_eq!(rich.ready_lines, Some(0));
        assert!(!rich.bracketed_paste);
        assert_eq!(rich.interrupt_key, "C-c");

        let flat = &demo.surfaces["flat"];
        assert_eq!(flat.args, vec!["--flat".to_owned()]);
        assert_eq!(flat.busy_regex.as_deref(), Some("^flat busy"));
        assert_eq!(flat.validated_version.as_deref(), Some("demo 1.0"));
        assert!(flat.patterns_validated());
        assert!(flat.bracketed_paste);
        assert_eq!(flat.interrupt_key, "C-u");
        assert!(flat.quit_when_idle);
        let resume = flat.resume.as_ref().expect("resume declared");
        assert_eq!(resume.syntax, "--resume {id}");
        assert_eq!(resume.id_regex, "^[0-9a-f-]{36}$");
    }

    #[test]
    fn agent_level_readiness_binds_to_pre_surface_not_default() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"
ready_regex = "^agent ready"
ready_lines = 2
default_surface = "flat"
pre_surface_rendering = "rich"

[demo.surfaces.rich]
args = ["--rich"]

[demo.surfaces.flat]
args = ["--flat"]
ready_regex = "^flat ready"

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        // The agent-level (pre-surface shape) readiness fields bind to `rich`, the
        // pre-surface rendering, and do not touch the default `flat` surface.
        assert_eq!(
            demo.surfaces["rich"].ready_regex.as_deref(),
            Some("^agent ready")
        );
        assert_eq!(demo.surfaces["rich"].ready_lines, Some(2));
        assert_eq!(
            demo.surfaces["flat"].ready_regex.as_deref(),
            Some("^flat ready")
        );
        assert_eq!(demo.surfaces["flat"].ready_lines, None);
    }

    #[test]
    fn normalize_chooses_exactly_one_default_and_pre_surface() {
        let agents = parse_registry_toml(
            r#"
[demo]
binary = "/bin/demo"

[demo.surfaces.alpha]
args = ["--alpha"]

[demo.surfaces.beta]
args = ["--beta"]

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        assert_eq!(demo.default_surface.as_deref(), Some("alpha"));
        assert_eq!(demo.pre_surface_rendering.as_deref(), Some("alpha"));
    }

    #[test]
    fn names_no_surface_warns_and_falls_back() {
        let (agents, warnings) = parse_registry_toml_with_warnings(
            r#"
[demo]
binary = "/bin/demo"
default_surface = "nope"

[demo.surfaces.only]
args = []

[demo.access.default]
args = []
"#,
        )
        .unwrap();

        let demo = agents.get("demo").unwrap();
        assert_eq!(demo.default_surface.as_deref(), Some("only"));
        assert!(warnings
            .iter()
            .any(|warning| warning.detail.contains("default_surface")));
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
    fn user_ready_lines_merges_onto_builtin_pre_surface() {
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
        assert_eq!(codex.surfaces["default"].ready_lines, Some(4));
        // Scalar merge leaves the builtin profiles and shipped patterns intact: the
        // user only widened the scan depth, so the codex builtin's measured
        // ready_regex survives.
        assert_eq!(
            codex.surfaces["default"].ready_regex.as_deref(),
            Some("· Ready · Context")
        );
        assert!(codex.access_profiles.contains_key("read-only"));
    }

    #[test]
    fn user_ready_regex_on_builtin_binds_to_pre_surface_rendering() {
        let path = write_temp_agents_file(
            r#"
[claude]
ready_regex = "^agent ready"
ready_lines = 0
"#,
        );

        let (registry, warnings) = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        assert!(warnings.is_empty());
        let claude = registry.get("claude").unwrap();
        assert_eq!(
            claude.surfaces["rich"].ready_regex.as_deref(),
            Some("^agent ready")
        );
        assert_eq!(claude.surfaces["rich"].ready_lines, Some(0));
        // The flat surface ships no patterns of its own, so the override must not reach it.
        assert_eq!(claude.surfaces["flat"].ready_regex, None);
        assert_eq!(claude.surfaces["flat"].ready_lines, None);
    }

    #[test]
    fn user_default_surface_override_failing_the_bar_warns_and_uses_pre_surface() {
        let path = write_temp_agents_file(
            r#"
[claude]
binary = "/usr/local/bin/claude"
default_surface = "flat"
"#,
        );

        let (registry, warnings) = Registry::load_with_user_path(Some(&path)).unwrap();
        fs::remove_file(&path).unwrap();

        // The declared default fails P2's promotion bar (claude's flat surface has no
        // validated pattern pair), so it is reported through the warning channel rather
        // than silently replaced.
        assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
        let warning = &warnings[0];
        assert_eq!(warning.agent, "claude");
        assert!(
            warning.detail.contains("default_surface \"flat\""),
            "warning must name the declared default: {}",
            warning.detail
        );
        assert!(
            warning.detail.contains("recording \"rich\""),
            "warning must name the surface actually used: {}",
            warning.detail
        );
        assert!(
            warning.detail.contains("ready_regex") && warning.detail.contains("busy_regex"),
            "warning must name the missing validation: {}",
            warning.detail
        );

        let claude = registry.get("claude").unwrap();
        // The field still records what the file declared; resolution uses the
        // pre-surface rendering.
        assert_eq!(claude.default_surface.as_deref(), Some("flat"));
        assert_eq!(claude.default_surface().unwrap().name, "rich");
        assert_eq!(claude.pre_surface().unwrap().name, "rich");
        assert_eq!(claude.surfaces.len(), 2);
    }

    /// The shipped `examples/claude-statusline/agents.toml` must actually take effect:
    /// its `busy_regex` has to land on a surface where it is honored, and every busy
    /// indicator the status line emits must classify as Busy rather than Unknown.
    #[test]
    fn shipped_claude_statusline_example_puts_its_busy_regex_on_the_rich_surface() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/claude-statusline/agents.toml");
        let (registry, warnings) = Registry::load_with_user_path(Some(&path)).unwrap();
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        let rich = &registry.get("claude").unwrap().surfaces["rich"];
        let ready = rich
            .ready_regex
            .as_deref()
            .expect("the example must declare ready_regex on the rich surface");
        let busy = rich
            .busy_regex
            .as_deref()
            .expect("the example must declare busy_regex on the rich surface");

        let patterns = crate::idle::SurfacePatterns {
            ready_regex: Some(regex::Regex::new(ready).unwrap()),
            ready_scan_lines: rich
                .ready_lines
                .unwrap_or(crate::idle::DEFAULT_READY_SCAN_LINES),
            busy_regex: Some(regex::Regex::new(busy).unwrap()),
        };

        // Every busy indicator the status line emits classifies as Busy, not Unknown.
        for line in [
            "⚡ working | 🤖 Opus 5 (1M context) | ⎇ main clean",
            "🔐 permission | 🤖 Opus 5 (1M context) | ⎇ main clean",
            "⚙ 3 bg (1 sub) | 🤖 Opus 5 (1M context) | ⎇ main clean",
            "⚙ 2 bg | 🤖 Opus 5 (1M context) | ⎇ main clean",
        ] {
            assert_eq!(
                crate::idle::classify(line, &patterns),
                crate::idle::PaneState::Busy,
                "busy indicator must classify as Busy: {line:?}"
            );
        }

        // The two ready states classify as Idle.
        for line in [
            "✓ done | 🤖 Opus 5 (1M context) | ⎇ main clean",
            "⏸ waiting | 🤖 Opus 5 (1M context) | ⎇ main clean",
        ] {
            assert_eq!(
                crate::idle::classify(line, &patterns),
                crate::idle::PaneState::Idle,
                "ready indicator must classify as Idle: {line:?}"
            );
        }
    }

    fn validated_flat_agent() -> AgentSpec {
        AgentSpec {
            name: "demo".to_owned(),
            binary: "demo".to_owned(),
            interaction_patterns: Vec::new(),
            access_profiles: BTreeMap::from([(
                "default".to_owned(),
                AccessProfile { args: Vec::new() },
            )]),
            capabilities: AgentCapabilities::default(),
            surfaces: BTreeMap::from([
                ("rich".to_owned(), Surface::default()),
                (
                    "flat".to_owned(),
                    Surface {
                        ready_regex: Some("^ready$".to_owned()),
                        busy_regex: Some("^busy$".to_owned()),
                        validated_version: Some("demo 1.0".to_owned()),
                        ..Surface::default()
                    },
                ),
            ]),
            pre_surface_rendering: Some("rich".to_owned()),
            default_surface: Some("flat".to_owned()),
        }
    }

    #[test]
    fn default_surface_requires_a_validated_pattern_pair() {
        // A validated flat surface named as the default is honored.
        let validated = validated_flat_agent();
        assert!(validated.surfaces["flat"].patterns_validated());
        assert_eq!(validated.default_surface().unwrap().name, "flat");

        // Without the stamp it falls back to the pre-surface rendering: P2 promotes a
        // flat surface only once the smoke validated and stamped it.
        let mut unstamped = validated_flat_agent();
        unstamped
            .surfaces
            .get_mut("flat")
            .unwrap()
            .validated_version = None;
        assert_eq!(unstamped.default_surface().unwrap().name, "rich");

        // Same when the busy pattern is missing.
        let mut no_busy = validated_flat_agent();
        no_busy.surfaces.get_mut("flat").unwrap().busy_regex = None;
        assert_eq!(no_busy.default_surface().unwrap().name, "rich");

        // A surface still reachable by an absent pane record keeps resolving to the
        // fixed pre-surface rendering either way.
        assert_eq!(unstamped.pre_surface().unwrap().name, "rich");
        assert_eq!(no_busy.pre_surface().unwrap().name, "rich");
    }

    #[test]
    fn resolve_surface_is_total() {
        let registry = Registry {
            agents: builtin::all().clone(),
        };

        // A record resolves to that surface.
        let recorded = registry.resolve_surface("claude", Some("flat")).unwrap();
        assert_eq!(recorded.name, "flat");
        assert_eq!(recorded.surface.args, vec!["--ax-screen-reader".to_owned()]);

        // No record resolves to the pre-surface rendering, never the default.
        let absent = registry.resolve_surface("claude", None).unwrap();
        assert_eq!(absent.name, "rich");

        // A stale record (a surface the registry no longer has) resolves to no surface,
        // never the pre-surface rendering, so no foreign patterns are applied.
        assert!(registry.resolve_surface("claude", Some("gone")).is_none());

        // No agent resolves to none.
        assert!(registry.resolve_surface("nope", None).is_none());
    }

    #[test]
    fn resolve_launch_uses_default_surface_and_errors_on_unknown() {
        let registry = Registry {
            agents: builtin::all().clone(),
        };

        let launch = registry
            .resolve_launch("codex", Some("read-only"), None, None)
            .unwrap();
        assert_eq!(launch.binary, "codex");
        assert_eq!(launch.surface, "default");
        assert_eq!(
            launch.args,
            vec!["--sandbox".to_owned(), "read-only".to_owned()]
        );

        // The surface's own arguments come before the access profile's.
        let flat = registry
            .resolve_launch("claude", None, Some("flat"), None)
            .unwrap();
        assert_eq!(flat.surface, "flat");
        assert_eq!(
            flat.args,
            vec![
                "--ax-screen-reader".to_owned(),
                "--permission-mode".to_owned(),
                "plan".to_owned()
            ]
        );

        let error = registry
            .resolve_launch("unknown", None, None, None)
            .unwrap_err();
        assert_eq!(error.to_string(), "unknown agent unknown");

        let error = registry
            .resolve_launch("claude", None, Some("nope"), None)
            .unwrap_err();
        assert_eq!(error.to_string(), "agent claude has no surface nope");
    }

    #[test]
    fn resolve_launch_maps_and_validates_resume() {
        let registry = Registry {
            agents: builtin::all().clone(),
        };

        let uuid = "0123abcd-4567-89ab-cdef-0123456789ab";

        // The resume arguments follow the surface's and the access profile's.
        let launch = registry
            .resolve_launch("claude", None, Some("rich"), Some(uuid))
            .unwrap();
        assert_eq!(launch.surface, "rich");
        assert_eq!(
            launch.args,
            vec![
                "--permission-mode".to_owned(),
                "plan".to_owned(),
                "--resume".to_owned(),
                uuid.to_owned()
            ]
        );

        // A positional subcommand syntax expands to its own tokens.
        let launch = registry
            .resolve_launch("codex", None, None, Some(uuid))
            .unwrap();
        assert_eq!(
            launch.args,
            vec![
                "--sandbox".to_owned(),
                "read-only".to_owned(),
                "resume".to_owned(),
                uuid.to_owned()
            ]
        );

        // An identifier outside the surface's shape fails before any launch.
        let error = registry
            .resolve_launch("claude", None, Some("rich"), Some("nope"))
            .unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");

        // A surface that declares no resume syntax cannot be resumed.
        let error = registry
            .resolve_launch("cursor", None, None, Some(uuid))
            .unwrap_err();
        assert!(
            error.to_string().contains("does not support resume"),
            "{error}"
        );
    }

    #[test]
    fn resolve_launch_errors_when_no_default_and_multiple_profiles() {
        let mut agents = BTreeMap::new();
        agents.insert(
            "custom".to_owned(),
            AgentSpec {
                name: "custom".to_owned(),
                binary: "custom".to_owned(),
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
                surfaces: BTreeMap::from([("default".to_owned(), Surface::default())]),
                pre_surface_rendering: Some("default".to_owned()),
                default_surface: Some("default".to_owned()),
            },
        );
        let registry = Registry { agents };

        let error = registry
            .resolve_launch("custom", None, None, None)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "agent custom has multiple access profiles (alpha, beta); pass --access explicitly"
        );
    }

    #[test]
    fn resolve_launch_errors_when_agent_has_no_access_profiles() {
        let mut agents = BTreeMap::new();
        agents.insert(
            "empty".to_owned(),
            AgentSpec {
                name: "empty".to_owned(),
                binary: "empty".to_owned(),
                interaction_patterns: Vec::new(),
                access_profiles: BTreeMap::new(),
                capabilities: AgentCapabilities::default(),
                surfaces: BTreeMap::from([("default".to_owned(), Surface::default())]),
                pre_surface_rendering: Some("default".to_owned()),
                default_surface: Some("default".to_owned()),
            },
        );
        let registry = Registry { agents };

        let error = registry
            .resolve_launch("empty", None, None, None)
            .unwrap_err();
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
