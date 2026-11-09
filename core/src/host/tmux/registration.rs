//! The Pane registration on tmux: the `@tt-*` pane user-options.

use anyhow::{anyhow, Context, Result};

use super::{runner, FIELD_SEP};
use crate::format::non_empty;
use crate::host::RegistrationKey;
use crate::names::Registered;

/// The tmux pane option that holds `key`.
pub(super) fn option_name(key: RegistrationKey) -> &'static str {
    match key {
        RegistrationKey::Name => "@tt-name",
        RegistrationKey::Agent => "@tt-agent",
        RegistrationKey::Access => "@tt-access",
        RegistrationKey::LaunchedAt => "@tt-launched-at",
        RegistrationKey::Cwd => "@tt-cwd",
        RegistrationKey::Surface => "@tt-surface",
        RegistrationKey::SurfaceUnvalidated => "@tt-surface-unvalidated",
    }
}

/// The tmux format that expands to the value of `key`.
pub(super) fn option_format(key: RegistrationKey) -> String {
    format!("#{{{}}}", option_name(key))
}

pub(super) fn get(pane_id: &str, key: RegistrationKey) -> Result<Option<String>> {
    let format = option_format(key);
    let args = ["display", "-p", "-t", pane_id, format.as_str()];
    let value = runner::run_checked(&args)?.trim().to_owned();

    Ok((!value.is_empty()).then_some(value))
}

pub(super) fn set(pane_id: &str, key: RegistrationKey, value: &str) -> Result<()> {
    let args = ["set-option", "-p", "-t", pane_id, option_name(key), value];
    runner::run_checked(&args)?;

    Ok(())
}

pub(super) fn unset(pane_id: &str, key: RegistrationKey) -> Result<()> {
    let args = ["set-option", "-p", "-u", "-t", pane_id, option_name(key)];
    runner::run_checked(&args)?;

    Ok(())
}

pub(super) fn pane_names() -> Result<Vec<(String, String)>> {
    let format = format!("#{{pane_id}}\t{}", option_format(RegistrationKey::Name));
    let args = ["list-panes", "-a", "-F", format.as_str()];
    let panes = runner::run_checked(&args)?;

    Ok(parse_pane_names(&panes))
}

/// Parse `pane id <TAB> name` lines; a line without a tab is skipped.
fn parse_pane_names(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(pane_id, name)| (pane_id.to_owned(), name.to_owned()))
        .collect()
}

pub(super) fn read(pane_id: &str) -> Result<Registered> {
    const FIELD_SEP: char = '\x1f';
    const FORMAT: &str = "#{@tt-name}\x1f#{@tt-agent}\x1f#{@tt-access}\x1f#{@tt-launched-at}\x1f\
                          #{@tt-cwd}\x1f#{@tt-surface}\x1f#{@tt-surface-unvalidated}";
    let raw = runner::run_checked(&["display-message", "-p", "-t", pane_id, FORMAT])?;
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

/// The registration of every pane in the window that `target` is in, as the
/// ownership predicate reads it.
pub(super) fn window_registrations(target: &str) -> Result<Vec<Registered>> {
    let format = [
        RegistrationKey::Name,
        RegistrationKey::Agent,
        RegistrationKey::Cwd,
    ]
    .map(option_format)
    .join(&FIELD_SEP.to_string());
    let output = runner::run_checked(&["list-panes", "-t", target, "-F", &format])
        .with_context(|| format!("cannot read the panes of the window of {target}"))?;
    parse_ownership_snapshot(&output)
}

/// Parse ownership snapshot lines — `@tt-name`, `@tt-agent` and `@tt-cwd`, separated
/// by [`FIELD_SEP`] — one per pane, into the registrations the ownership predicate
/// reads. Every other registration field is left unset.
fn parse_ownership_snapshot(output: &str) -> Result<Vec<Registered>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.trim_end_matches('\r').split(FIELD_SEP).collect();
            let [name, agent, cwd] = fields[..] else {
                return Err(anyhow!("unexpected tmux pane registration line {line:?}"));
            };
            // Trimmed as `read` trims, so both callers of the predicate see the
            // same registration.
            Ok(Registered {
                name: non_empty(name.trim()),
                agent: non_empty(agent.trim()),
                access: None,
                launched_at: None,
                cwd: non_empty(cwd.trim()),
                surface: None,
                surface_unvalidated: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_matching(output: &str, name: &str) -> Vec<String> {
        parse_pane_names(output)
            .into_iter()
            .filter(|(_, pane_name)| pane_name == name)
            .map(|(pane_id, _)| pane_id)
            .collect()
    }

    #[test]
    fn parse_returns_empty_when_no_matches() {
        let output = "%1\tfoo\n%2\tbar\n";
        let matches = names_matching(output, "baz");
        assert!(matches.is_empty());
    }

    #[test]
    fn parse_returns_single_match() {
        let output = "%1\tfoo\n%2\tbar\n%3\tbaz\n";
        let matches = names_matching(output, "bar");
        assert_eq!(matches, vec!["%2".to_owned()]);
    }

    #[test]
    fn parse_returns_all_matches_for_duplicates() {
        let output = "%1\tdup\n%2\tother\n%3\tdup\n%4\tdup\n";
        let matches = names_matching(output, "dup");
        assert_eq!(
            matches,
            vec!["%1".to_owned(), "%3".to_owned(), "%4".to_owned()]
        );
    }

    #[test]
    fn parse_skips_lines_without_tab() {
        let output = "no-tab-line\n%1\tname\n";
        let matches = names_matching(output, "name");
        assert_eq!(matches, vec!["%1".to_owned()]);
    }

    #[test]
    fn parse_skips_panes_with_empty_names_when_searching_non_empty() {
        let output = "%1\t\n%2\tname\n%3\t\n";
        let matches = names_matching(output, "name");
        assert_eq!(matches, vec!["%2".to_owned()]);
    }

    #[test]
    fn parse_handles_empty_output() {
        let matches = names_matching("", "anything");
        assert!(matches.is_empty());
    }

    #[test]
    fn registration_options_are_the_tt_pane_options() {
        assert_eq!(option_name(RegistrationKey::Name), "@tt-name");
        assert_eq!(option_name(RegistrationKey::Agent), "@tt-agent");
        assert_eq!(option_name(RegistrationKey::Access), "@tt-access");
        assert_eq!(option_name(RegistrationKey::LaunchedAt), "@tt-launched-at");
        assert_eq!(option_name(RegistrationKey::Cwd), "@tt-cwd");
        assert_eq!(option_name(RegistrationKey::Surface), "@tt-surface");
        assert_eq!(
            option_name(RegistrationKey::SurfaceUnvalidated),
            "@tt-surface-unvalidated"
        );
    }

    /// One ownership snapshot line: name, agent, cwd.
    fn snapshot_line(name: &str, agent: &str, cwd: &str) -> String {
        [name, agent, cwd].join(&FIELD_SEP.to_string()) + "\n"
    }

    #[test]
    fn ownership_snapshot_reads_name_agent_and_cwd_per_pane() {
        let output = [
            snapshot_line("", "", ""),
            snapshot_line("npm-a3f2", "", "/work"),
            snapshot_line("", "codex", ""),
        ]
        .concat();
        let panes = parse_ownership_snapshot(&output).unwrap();
        assert_eq!(panes.len(), 3);
        assert_eq!(
            (panes[0].name.as_deref(), panes[0].agent.as_deref()),
            (None, None)
        );
        assert_eq!(panes[1].name.as_deref(), Some("npm-a3f2"));
        assert_eq!(panes[1].cwd.as_deref(), Some("/work"));
        assert_eq!(panes[2].agent.as_deref(), Some("codex"));
        assert_eq!(panes[2].cwd, None);
    }

    #[test]
    fn ownership_snapshot_rejects_malformed_lines() {
        let sep = FIELD_SEP.to_string();
        assert!(parse_ownership_snapshot(&["a", "b"].join(&sep)).is_err());
        assert!(parse_ownership_snapshot(&["a", "b", "c", "d"].join(&sep)).is_err());
        assert!(parse_ownership_snapshot("").unwrap().is_empty());
    }
}
