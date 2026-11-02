#![allow(dead_code)]

use anyhow::{anyhow, Result};

use crate::tmux;

// Keys are exposed both as `pub const &str` (for normal use) and via
// macro_rules! aliases (for use inside `concat!`, which only accepts literals).
// Keep both arms in sync.
macro_rules! key_name {
    () => {
        "@tt-name"
    };
}
macro_rules! key_agent {
    () => {
        "@tt-agent"
    };
}
macro_rules! key_access {
    () => {
        "@tt-access"
    };
}
macro_rules! key_launched_at {
    () => {
        "@tt-launched-at"
    };
}
macro_rules! key_cwd {
    () => {
        "@tt-cwd"
    };
}
macro_rules! key_surface {
    () => {
        "@tt-surface"
    };
}
macro_rules! key_surface_unvalidated {
    () => {
        "@tt-surface-unvalidated"
    };
}
pub const KEY_NAME: &str = key_name!();
pub const KEY_AGENT: &str = key_agent!();
pub const KEY_ACCESS: &str = key_access!();
pub const KEY_LAUNCHED_AT: &str = key_launched_at!();
pub const KEY_CWD: &str = key_cwd!();
pub const KEY_SURFACE: &str = key_surface!();
pub const KEY_SURFACE_UNVALIDATED: &str = key_surface_unvalidated!();

#[derive(Clone, Debug, Eq, PartialEq)]
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

pub fn get(pane_id: &str, key: &str) -> Result<Option<String>> {
    let format = format!("#{{{key}}}");
    let args = ["display", "-p", "-t", pane_id, format.as_str()];
    let value = tmux::run_checked(&args)?.trim().to_owned();

    Ok((!value.is_empty()).then_some(value))
}

pub fn set(pane_id: &str, key: &str, value: &str) -> Result<()> {
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

    let args = ["set-option", "-p", "-t", pane_id, key, value];
    tmux::run_checked(&args)?;

    Ok(())
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

pub fn unset(pane_id: &str, key: &str) -> Result<()> {
    let args = ["set-option", "-p", "-u", "-t", pane_id, key];
    tmux::run_checked(&args)?;

    Ok(())
}

pub fn find_pane_by_name(name: &str) -> Result<Option<String>> {
    let pane_ids = list_panes_with_name(name)?;

    match pane_ids.len() {
        0 => Ok(None),
        1 => Ok(Some(pane_ids.into_iter().next().unwrap())),
        _ => Err(anyhow!(
            "name '{}' resolves to multiple panes: {}; use a pane id (%N) explicitly",
            name,
            pane_ids.join(", ")
        )),
    }
}

fn list_panes_with_name(name: &str) -> Result<Vec<String>> {
    let format = format!("#{{pane_id}}\t#{{{}}}", KEY_NAME);
    let args = ["list-panes", "-a", "-F", format.as_str()];
    let panes = tmux::run_checked(&args)?;

    Ok(parse_panes_with_name(&panes, name))
}

fn parse_panes_with_name(output: &str, name: &str) -> Vec<String> {
    let mut matches = Vec::new();
    for line in output.lines() {
        if let Some((pane_id, pane_name)) = line.split_once('\t') {
            if pane_name == name {
                matches.push(pane_id.to_owned());
            }
        }
    }
    matches
}

pub fn read(pane_id: &str) -> Result<Registered> {
    const FIELD_SEP: char = '\x1f';
    const FORMAT: &str = concat!(
        "#{",
        key_name!(),
        "}\x1f",
        "#{",
        key_agent!(),
        "}\x1f",
        "#{",
        key_access!(),
        "}\x1f",
        "#{",
        key_launched_at!(),
        "}\x1f",
        "#{",
        key_cwd!(),
        "}\x1f",
        "#{",
        key_surface!(),
        "}\x1f",
        "#{",
        key_surface_unvalidated!(),
        "}",
    );
    let raw = tmux::run_checked(&["display-message", "-p", "-t", pane_id, FORMAT])?;
    let line = raw.trim_end_matches('\n');
    let mut parts = line.splitn(7, FIELD_SEP);
    let name = parts.next().and_then(non_empty_owned);
    let agent = parts.next().and_then(non_empty_owned);
    let access = parts.next().and_then(non_empty_owned);
    let launched_at = parts.next().and_then(non_empty_owned);
    let cwd = parts.next().and_then(non_empty_owned);
    let surface = parts.next().and_then(non_empty_owned);
    let surface_unvalidated = parts.next().is_some_and(truthy);
    Ok(Registered {
        name,
        agent,
        access,
        launched_at,
        cwd,
        surface,
        surface_unvalidated,
    })
}

fn truthy(value: &str) -> bool {
    let value = value.trim();
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn non_empty_owned(s: &str) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_returns_empty_when_no_matches() {
        let output = "%1\tfoo\n%2\tbar\n";
        let matches = parse_panes_with_name(output, "baz");
        assert!(matches.is_empty());
    }

    #[test]
    fn parse_returns_single_match() {
        let output = "%1\tfoo\n%2\tbar\n%3\tbaz\n";
        let matches = parse_panes_with_name(output, "bar");
        assert_eq!(matches, vec!["%2".to_owned()]);
    }

    #[test]
    fn parse_returns_all_matches_for_duplicates() {
        let output = "%1\tdup\n%2\tother\n%3\tdup\n%4\tdup\n";
        let matches = parse_panes_with_name(output, "dup");
        assert_eq!(
            matches,
            vec!["%1".to_owned(), "%3".to_owned(), "%4".to_owned()]
        );
    }

    #[test]
    fn parse_skips_lines_without_tab() {
        let output = "no-tab-line\n%1\tname\n";
        let matches = parse_panes_with_name(output, "name");
        assert_eq!(matches, vec!["%1".to_owned()]);
    }

    #[test]
    fn parse_skips_panes_with_empty_names_when_searching_non_empty() {
        let output = "%1\t\n%2\tname\n%3\t\n";
        let matches = parse_panes_with_name(output, "name");
        assert_eq!(matches, vec!["%2".to_owned()]);
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

    #[test]
    fn parse_handles_empty_output() {
        let matches = parse_panes_with_name("", "anything");
        assert!(matches.is_empty());
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
