//! `list` and `status` on tmux: every pane in a scope, and one pane described.

use anyhow::{anyhow, Context, Result};
use std::collections::BTreeSet;
use std::env;

use super::registration::option_format;
use super::{runner, session, FIELD_SEP};
use crate::format::non_empty;
use crate::host::{ListScope, PaneDescription, PaneListing, PaneSummary, RegistrationKey};

const STATUS_FIELD_COUNT: usize = 8;

/// Every pane in `scope`. The scope is resolved before the panes are listed.
pub(super) fn list_panes(scope: ListScope<'_>) -> Result<Vec<PaneListing>> {
    let scope = session_scope(scope)?;
    let mut rows = list_all_panes()?;

    if let Some(scope) = &scope {
        rows.retain(|row| scope.contains(&row.session));
    }

    Ok(rows)
}

fn session_scope(scope: ListScope<'_>) -> Result<Option<BTreeSet<String>>> {
    match scope {
        ListScope::Session(session) => Ok(Some(BTreeSet::from([session.to_owned()]))),
        ListScope::All => Ok(None),
        ListScope::Default => {
            let mut scope = BTreeSet::from([session::MANAGED_SESSION.to_owned()]);
            if env::var_os("TMUX").is_some() {
                scope.insert(session::current_session()?);
            }

            Ok(Some(scope))
        }
    }
}

fn list_all_panes() -> Result<Vec<PaneListing>> {
    let list_format = list_format();
    let output = runner::run(&["list-panes", "-a", "-F", &list_format])
        .context("failed to run tmux command: tmux list-panes -a")?;

    if output.exit_code != 0 {
        if stderr_indicates_no_server(&output.stderr) {
            return Ok(Vec::new());
        }

        return Err(anyhow!(
            "tmux command failed (args: {:?}, exit code {}): {}",
            ["list-panes", "-a", "-F", list_format.as_str()],
            output.exit_code,
            output.stderr.trim()
        ));
    }

    output.stdout.lines().map(parse_pane_row).collect()
}

fn list_format() -> String {
    format!(
        "#{{session_name}}\x1f#{{window_index}}\x1f#{{window_name}}\x1f#{{pane_index}}\x1f#{{pane_id}}\x1f{}\x1f{}\x1f{}\x1f{}\x1f#{{pane_dead}}\x1f#{{pane_dead_status}}\x1f#{{window_activity}}",
        option_format(RegistrationKey::Name),
        option_format(RegistrationKey::Agent),
        option_format(RegistrationKey::Access),
        option_format(RegistrationKey::LaunchedAt),
    )
}

fn stderr_indicates_no_server(stderr: &str) -> bool {
    const NO_SERVER_MARKERS: &[&str] = &[
        "no server running",
        "No such file or directory",
        "Connection refused",
        "error connecting",
    ];
    NO_SERVER_MARKERS
        .iter()
        .any(|marker| stderr.contains(marker))
}

fn parse_pane_row(line: &str) -> Result<PaneListing> {
    let fields: Vec<&str> = line.split(FIELD_SEP).collect();
    if fields.len() != 12 {
        return Err(anyhow!("unexpected tmux list-panes output: {line:?}"));
    }

    let window_index_sort = fields[1].parse().with_context(|| {
        format!(
            "failed to parse tmux window index {:?} for pane {}",
            fields[1], fields[4]
        )
    })?;
    let pane_index_sort = fields[3].parse().with_context(|| {
        format!(
            "failed to parse tmux pane index {:?} for pane {}",
            fields[3], fields[4]
        )
    })?;

    let last_activity = match fields[11] {
        "" => None,
        raw => match raw.parse::<i64>() {
            Ok(0) => None,
            Ok(value) => Some(value),
            Err(_) => None,
        },
    };

    Ok(PaneListing {
        raw: line.to_owned(),
        session: fields[0].to_owned(),
        window_index_sort,
        window_name: fields[2].to_owned(),
        pane_index_sort,
        pane_id: fields[4].to_owned(),
        name: non_empty(fields[5]),
        agent: non_empty(fields[6]),
        access: non_empty(fields[7]),
        launched_at: non_empty(fields[8]),
        pane_dead: fields[9] == "1",
        pane_dead_status: non_empty(fields[10]),
        last_activity,
        label: None,
        unknown: false,
    })
}

pub(super) fn describe_pane(pane_id: Option<&str>) -> Result<PaneDescription> {
    let status_format = status_format();
    let args = match pane_id {
        Some(pane_id) => vec!["display-message", "-p", "-t", pane_id, &status_format],
        None => vec!["display-message", "-p", &status_format],
    };

    let raw = runner::run_checked(&args)?;
    let summary = parse_status(raw.trim_end_matches('\n'));
    Ok(PaneDescription { raw, summary })
}

fn status_format() -> String {
    format!(
        "#{{session_name}}\x1f#{{window_index}}.#{{window_name}}\x1f#{{pane_index}}.#{{pane_id}}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        option_format(RegistrationKey::Name),
        option_format(RegistrationKey::Agent),
        option_format(RegistrationKey::Access),
        option_format(RegistrationKey::LaunchedAt),
        option_format(RegistrationKey::Cwd),
    )
}

fn parse_status(line: &str) -> Result<PaneSummary> {
    let fields: Vec<&str> = line.split(FIELD_SEP).collect();
    if fields.len() != STATUS_FIELD_COUNT {
        return Err(anyhow!("unexpected tmux status output: {line:?}"));
    }

    let (window_index, window_name) = fields[1]
        .split_once('.')
        .ok_or_else(|| anyhow!("unexpected tmux window status output: {:?}", fields[1]))?;
    let (pane_index, pane_id) = fields[2]
        .split_once('.')
        .ok_or_else(|| anyhow!("unexpected tmux pane status output: {:?}", fields[2]))?;

    Ok(PaneSummary {
        session: fields[0].to_owned(),
        window_index: window_index.to_owned(),
        window_name: window_name.to_owned(),
        pane_index: pane_index.to_owned(),
        pane_id: pane_id.to_owned(),
        name: non_empty(fields[3]),
        agent: non_empty(fields[4]),
        access: non_empty(fields[5]),
        launched_at: non_empty(fields[6]),
        cwd: non_empty(fields[7]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_line(fields: &[&str]) -> String {
        fields.join(&FIELD_SEP.to_string())
    }

    #[test]
    fn parses_pane_row_with_unit_separator() {
        let line = build_line(&[
            "managed",
            "0",
            "main",
            "1",
            "%42",
            "my-pane",
            "claude",
            "alice",
            "2024-01-01T00:00:00Z",
            "0",
            "",
            "1700000000",
        ]);

        let row = parse_pane_row(&line).expect("row parses");
        assert_eq!(row.session, "managed");
        assert_eq!(row.window_index_sort, 0);
        assert_eq!(row.window_name, "main");
        assert_eq!(row.pane_index_sort, 1);
        assert_eq!(row.pane_id, "%42");
        assert_eq!(row.name.as_deref(), Some("my-pane"));
        assert_eq!(row.agent.as_deref(), Some("claude"));
        assert_eq!(row.access.as_deref(), Some("alice"));
        assert_eq!(row.launched_at.as_deref(), Some("2024-01-01T00:00:00Z"));
        assert!(!row.pane_dead);
        assert_eq!(row.pane_dead_status, None);
        assert_eq!(row.last_activity, Some(1_700_000_000));
    }

    #[test]
    fn parses_pane_row_with_tab_in_name() {
        // A tab inside a user-supplied @tt-name should not break parsing
        // because the field separator is now \x1f.
        let line = build_line(&[
            "managed",
            "0",
            "main",
            "1",
            "%42",
            "my\tname\twith\ttabs",
            "",
            "",
            "",
            "0",
            "",
            "0",
        ]);

        let row = parse_pane_row(&line).expect("row parses with tab in name");
        assert_eq!(row.name.as_deref(), Some("my\tname\twith\ttabs"));
        assert_eq!(row.last_activity, None);
    }

    #[test]
    fn parses_pane_row_rejects_wrong_field_count() {
        let line = build_line(&["only", "three", "fields"]);
        assert!(parse_pane_row(&line).is_err());
    }

    #[test]
    fn last_activity_zero_is_none() {
        let line = build_line(&["s", "0", "w", "0", "%1", "", "", "", "", "0", "", "0"]);
        let row = parse_pane_row(&line).expect("row parses");
        assert_eq!(row.last_activity, None);
    }

    #[test]
    fn stderr_no_server_detection() {
        assert!(stderr_indicates_no_server("no server running on /tmp/tmux"));
        assert!(stderr_indicates_no_server(
            "error connecting to /tmp/tmux-501/default (No such file or directory)"
        ));
        assert!(stderr_indicates_no_server("Connection refused"));
        assert!(stderr_indicates_no_server("error connecting to socket"));
        assert!(!stderr_indicates_no_server("some other tmux error"));
    }

    #[test]
    fn parses_status_with_unit_separator() {
        let line = build_line(&[
            "managed",
            "0.main",
            "1.%42",
            "my-pane",
            "claude",
            "workspace-write",
            "1700000000",
            "/work",
        ]);

        let row = parse_status(&line).expect("status parses");
        assert_eq!(row.session, "managed");
        assert_eq!(row.window_index, "0");
        assert_eq!(row.window_name, "main");
        assert_eq!(row.pane_index, "1");
        assert_eq!(row.pane_id, "%42");
        assert_eq!(row.name.as_deref(), Some("my-pane"));
        assert_eq!(row.agent.as_deref(), Some("claude"));
        assert_eq!(row.access.as_deref(), Some("workspace-write"));
        assert_eq!(row.launched_at.as_deref(), Some("1700000000"));
        assert_eq!(row.cwd.as_deref(), Some("/work"));
    }

    #[test]
    fn parses_status_with_tab_in_name() {
        // Tab inside the user-supplied name must not break parsing.
        let line = build_line(&[
            "managed",
            "0.main",
            "1.%42",
            "name\twith\ttab",
            "",
            "",
            "",
            "",
        ]);

        let row = parse_status(&line).expect("status parses with tab");
        assert_eq!(row.name.as_deref(), Some("name\twith\ttab"));
        assert_eq!(row.agent, None);
        assert_eq!(row.access, None);
        assert_eq!(row.launched_at, None);
        assert_eq!(row.cwd, None);
    }

    #[test]
    fn parses_status_rejects_wrong_field_count() {
        let line = build_line(&["only", "three", "fields"]);
        assert!(parse_status(&line).is_err());
    }
}
