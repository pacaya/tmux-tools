#![allow(dead_code)]

use anyhow::{anyhow, Result};

use crate::host::{self, RegistrationKey};

// The Pane registration fields, as the Host port names them. Each backend stores them
// its own way (on tmux, as `@tt-*` pane options).
pub const KEY_NAME: RegistrationKey = RegistrationKey::Name;
pub const KEY_AGENT: RegistrationKey = RegistrationKey::Agent;
pub const KEY_ACCESS: RegistrationKey = RegistrationKey::Access;
pub const KEY_LAUNCHED_AT: RegistrationKey = RegistrationKey::LaunchedAt;
pub const KEY_CWD: RegistrationKey = RegistrationKey::Cwd;
pub const KEY_SURFACE: RegistrationKey = RegistrationKey::Surface;
pub const KEY_SURFACE_UNVALIDATED: RegistrationKey = RegistrationKey::SurfaceUnvalidated;
pub const KEY_PARENT: RegistrationKey = RegistrationKey::Parent;
pub const KEY_TEAM: RegistrationKey = RegistrationKey::Team;
pub const KEY_SHIM_ID: RegistrationKey = RegistrationKey::ShimId;
pub const KEY_TITLE: RegistrationKey = RegistrationKey::Title;
pub const KEY_CLAUDE_AGENT_NAME: RegistrationKey = RegistrationKey::ClaudeAgentName;
pub const KEY_CLAUDE_TEAM_NAME: RegistrationKey = RegistrationKey::ClaudeTeamName;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Registered {
    pub name: Option<String>,
    pub agent: Option<String>,
    pub access: Option<String>,
    pub launched_at: Option<String>,
    pub cwd: Option<String>,
    /// The surface recorded at spawn time. `None` for a pane created before surfaces
    /// existed (or by `launch`), which resolves to the agent's pre-surface rendering.
    pub surface: Option<String>,
    /// Whether caller-supplied trailing arguments left the recorded surface unvalidated.
    pub surface_unvalidated: bool,
    /// The name of the tmux-tools pane this pane was created from (PRD-261108-0447-01 P7).
    pub parent: Option<String>,
    /// The team id of a Teammate pane (P6.4).
    pub team: Option<String>,
    /// The Shim id of a Teammate pane (P6.4).
    pub shim_id: Option<String>,
    /// Display data (P6.4); never resolves a Target.
    pub title: Option<String>,
    /// Display data (P6.4); never resolves a Target.
    pub claude_agent_name: Option<String>,
    /// Display data (P6.4); never resolves a Target.
    pub claude_team_name: Option<String>,
}

/// The single ownership rule: is this pane ours?
///
/// A pane is owned when `@tt-name` or `@tt-agent` is set and, when both the
/// recorded `@tt-cwd` and `current_cwd` are known, they match. An absent cwd on
/// either side passes (fail-open for panes registered before `@tt-cwd`, and
/// for callers that cannot read their own cwd). No other registration field
/// affects the answer.
///
/// Every "is this pane ours" decision goes through here — the destructive
/// verbs' safety evaluator and pane layout alike.
pub fn pane_is_owned(registered: &Registered, current_cwd: Option<&str>) -> bool {
    let registered_by_us = registered.name.is_some() || registered.agent.is_some();
    let cwd_matches = match (registered.cwd.as_deref(), current_cwd) {
        (Some(recorded), Some(current)) => recorded == current,
        _ => true,
    };
    registered_by_us && cwd_matches
}

pub fn get(pane_id: &str, key: RegistrationKey) -> Result<Option<String>> {
    host::current().read_registration_value(pane_id, key)
}

pub fn set(pane_id: &str, key: RegistrationKey, value: &str) -> Result<()> {
    if key == KEY_NAME {
        let existing = list_panes_with_name(value)?;
        for other in &existing {
            if other != pane_id {
                return Err(anyhow!(
                    "name '{value}' is already used by pane {other}; pick a different name or kill the existing pane"
                ));
            }
        }
    }

    host::current().write_registration_value(pane_id, key, value)
}

/// The stem of a derived `@tt-name` when the command yields no usable word.
pub const DERIVED_NAME_FALLBACK_STEM: &str = "pane";

/// Hex digits in a derived name's random suffix (`npm-a3f2`).
pub const DERIVED_NAME_SUFFIX_LEN: usize = 4;

/// How many fresh derivations [`choose_name`] tries when a derived name is already
/// taken by another pane.
const DERIVED_NAME_ATTEMPTS: usize = 16;

/// The `@tt-name` to register on `pane_id`, chosen without writing it and without a
/// tmux call that targets `pane_id`, so the name is known even when the pane is gone
/// before it is written.
///
/// With `explicit` (the `--name` value) that is the name, unchanged; writing it with
/// [`set`] applies the usual collision check. Without it, the name is derived from
/// `program` — the launched command's first word, never the keep-alive wrap — as
/// `<stem>-<random hex>` (see [`derive_name`]). A derived name that another pane
/// already carries is re-derived; the random suffix alone keeps concurrent
/// same-command collisions negligible, since [`set`]'s uniqueness check and write
/// are not atomic.
pub fn choose_name(pane_id: &str, explicit: Option<&str>, program: &str) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name.to_owned());
    }
    for _ in 0..DERIVED_NAME_ATTEMPTS {
        let candidate = derive_name(program);
        if list_panes_with_name(&candidate)?
            .iter()
            .any(|other| other != pane_id)
        {
            continue;
        }
        return Ok(candidate);
    }
    Err(anyhow!(
        "could not derive an unused name for pane {pane_id} from {program:?}; pass --name"
    ))
}

/// A name derived from `program`: [`derived_name_stem`] plus `-` and a random
/// lowercase hex suffix of [`DERIVED_NAME_SUFFIX_LEN`] digits.
pub fn derive_name(program: &str) -> String {
    format!("{}-{}", derived_name_stem(program), random_suffix())
}

/// The first whitespace-separated word of a shell command line.
pub fn first_command_word(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or_default()
}

/// The stem of a derived name: the basename of `program`, reduced to ASCII letters,
/// digits, `-`, `_` and `.` (valid in a tmux option value and a `--target` name),
/// with leading `-`/`_`/`.` dropped so `--target <name>` never parses as a flag.
/// A word that keeps no letter or digit gets [`DERIVED_NAME_FALLBACK_STEM`].
pub fn derived_name_stem(program: &str) -> String {
    let basename = program.rsplit('/').next().unwrap_or_default();
    let reduced: String = basename
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let stem = reduced.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    if stem.is_empty() {
        DERIVED_NAME_FALLBACK_STEM.to_owned()
    } else {
        stem.to_owned()
    }
}

/// Random lowercase hex. `RandomState` is seeded from the OS per process, and the
/// clock and pid are mixed in, so concurrent invocations draw independent values
/// without a new dependency.
fn random_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    hasher.write_u32(std::process::id());
    let hex = format!("{:016x}", hasher.finish());
    hex[..DERIVED_NAME_SUFFIX_LEN].to_owned()
}

pub fn unset(pane_id: &str, key: RegistrationKey) -> Result<()> {
    host::current().clear_registration_value(pane_id, key)
}

pub fn find_pane_by_name(name: &str) -> Result<Option<String>> {
    host::current().find_registered(name)
}

/// A `--name` value that P4.1 rejects. The CLI prints it as `invalid name: <name>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidName(pub String);

impl std::fmt::Display for InvalidName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid name: {}", self.0)
    }
}

impl std::error::Error for InvalidName {}

/// P4.1: a pane name matches `[A-Za-z0-9._-]+` and is neither `.` nor `..`. Checked
/// on every host before any pane or record is created.
pub fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(InvalidName(name.to_owned()).into())
    }
}

fn list_panes_with_name(name: &str) -> Result<Vec<String>> {
    Ok(host::current()
        .pane_names()?
        .into_iter()
        .filter(|(_, pane_name)| pane_name == name)
        .map(|(pane_id, _)| pane_id)
        .collect())
}

pub fn read(pane_id: &str) -> Result<Registered> {
    host::current().read_registration(pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_applies_the_p4_1_character_set() {
        for bad in ["a/b", "..", ".", "a b", "", "é", "a:b"] {
            let error = validate_name(bad).unwrap_err();
            assert_eq!(error.to_string(), format!("invalid name: {bad}"));
        }
        for good in ["A.b_c-9", "x", ".hidden", "a..b"] {
            validate_name(good).unwrap();
        }
    }

    #[test]
    fn derived_stem_is_the_basename_of_the_first_word() {
        assert_eq!(derived_name_stem("cargo"), "cargo");
        assert_eq!(derived_name_stem("/usr/local/bin/npm"), "npm");
        assert_eq!(derived_name_stem("./scripts/run.sh"), "run.sh");
    }

    #[test]
    fn derived_stem_drops_characters_invalid_in_a_name() {
        assert_eq!(derived_name_stem("(cd"), "cd");
        assert_eq!(derived_name_stem("'my#prog'"), "myprog");
        assert_eq!(derived_name_stem("FOO=1"), "FOO1");
        assert_eq!(derived_name_stem("tt-fake-tui"), "tt-fake-tui");
    }

    #[test]
    fn derived_stem_never_starts_with_a_separator() {
        // A leading `-` would make `--target <name>` parse as a flag.
        assert_eq!(derived_name_stem("--weird"), "weird");
        assert_eq!(derived_name_stem(".hidden"), "hidden");
    }

    #[test]
    fn derived_stem_falls_back_when_no_letter_or_digit_remains() {
        for word in ["", ":", "./", "%", "@", "-._", "é"] {
            assert_eq!(
                derived_name_stem(word),
                DERIVED_NAME_FALLBACK_STEM,
                "{word:?}"
            );
        }
    }

    #[test]
    fn first_command_word_splits_on_whitespace() {
        assert_eq!(first_command_word("cargo --version"), "cargo");
        assert_eq!(first_command_word("  npm\trun dev"), "npm");
        assert_eq!(first_command_word(""), "");
    }

    #[test]
    fn derived_name_is_stem_dash_four_hex() {
        let name = derive_name("cargo");
        let suffix = name.strip_prefix("cargo-").expect("stem prefix");
        assert_eq!(suffix.len(), DERIVED_NAME_SUFFIX_LEN);
        assert!(suffix
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn derived_names_resolve_as_names_not_tmux_ids() {
        for word in ["", "%", "@", "1", "cargo"] {
            let name = derive_name(word);
            assert_eq!(
                crate::target::parse(&name),
                crate::target::TargetSpec::Name(name.clone())
            );
        }
    }

    #[test]
    fn derived_suffixes_vary() {
        let suffixes: std::collections::HashSet<String> =
            (0..64).map(|_| derive_name("x")).collect();
        assert!(suffixes.len() > 32, "suffixes must be random: {suffixes:?}");
    }

    fn registration(name: Option<&str>, agent: Option<&str>, cwd: Option<&str>) -> Registered {
        Registered {
            name: name.map(str::to_owned),
            agent: agent.map(str::to_owned),
            access: None,
            launched_at: None,
            cwd: cwd.map(str::to_owned),
            surface: None,
            surface_unvalidated: false,
            ..Registered::default()
        }
    }

    #[test]
    fn pane_is_owned_via_agent_alone_without_recorded_cwd() {
        let reg = registration(None, Some("codex"), None);
        assert!(pane_is_owned(&reg, Some("/work")));
    }

    #[test]
    fn pane_is_owned_via_name_alone_without_recorded_cwd() {
        let reg = registration(Some("shell"), None, None);
        assert!(pane_is_owned(&reg, Some("/work")));
    }

    #[test]
    fn pane_is_owned_when_recorded_cwd_matches_current() {
        let reg = registration(Some("shell"), None, Some("/work"));
        assert!(pane_is_owned(&reg, Some("/work")));
    }

    #[test]
    fn pane_is_owned_when_current_cwd_unknown() {
        let reg = registration(Some("shell"), None, Some("/work"));
        assert!(pane_is_owned(&reg, None));
    }

    #[test]
    fn pane_is_not_owned_without_name_or_agent_even_when_cwd_matches() {
        let reg = registration(None, None, Some("/work"));
        assert!(!pane_is_owned(&reg, Some("/work")));
    }

    #[test]
    fn pane_is_not_owned_when_recorded_cwd_differs_from_current() {
        let reg = registration(Some("shell"), None, Some("/other"));
        assert!(!pane_is_owned(&reg, Some("/work")));
    }
}
