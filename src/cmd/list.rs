use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{self, Format, MISSING_GLYPH},
    host::{self, Host, HostState, ListScope, PaneListing as PaneRow},
    team,
};

use crate::util::{instant_later, parse_rfc3339_utc};

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long, value_name = "NAME")]
    pub session: Option<String>,
    #[arg(long)]
    pub all: bool,
    #[arg(long, value_enum, default_value_t = Format::Concise)]
    pub format: Format,
}

#[derive(Serialize)]
struct ListJsonRow {
    host: Host,
    name: Option<String>,
    agent: Option<String>,
    /// The pane's session; on Herdr its `herdr.space` label or `null`
    /// (PRD-261109-2223-01 P7.3).
    session: Option<String>,
    pane: String,
    access: Option<String>,
    launched: Option<String>,
    status: String,
    last_activity: Option<i64>,
    host_state: Option<HostState>,
    parent: Option<String>,
    team: Option<String>,
    shim_id: Option<String>,
    orphan: bool,
}

pub fn run(args: &ListArgs) -> Result<()> {
    let mut rows = host::current().list_panes(list_scope(args))?;

    rows.sort_by(|left, right| {
        left.session
            .cmp(&right.session)
            .then(left.window_index_sort.cmp(&right.window_index_sort))
            .then(left.pane_index_sort.cmp(&right.pane_index_sort))
    });

    match args.format {
        Format::Raw => render_raw(&rows),
        Format::Concise => {
            let orphans = orphans(&rows, args)?;
            render_concise(&rows, &orphans)
        }
        Format::Json => {
            let orphans = orphans(&rows, args)?;
            render_json(&rows, &orphans)?
        }
    }

    Ok(())
}

/// Whether each row is an Orphan (PRD-261108-0447-01 P8.3): its `parent` does not
/// resolve to a live pane, or resolves to a pane launched later than the row; or it has
/// a `team` whose team directory does not exist or whose `leader.lock` is not held.
fn orphans(rows: &[PaneRow], args: &ListArgs) -> Result<Vec<bool>> {
    let live: Vec<PaneRow>;
    let live_rows = if rows.iter().all(|row| row.parent.is_none()) {
        &[][..]
    } else if args.all || host::current().creates_registered() {
        rows
    } else {
        // A parent can live outside the listed scope.
        live = host::current().list_panes(ListScope::All)?;
        &live[..]
    };
    Ok(rows
        .iter()
        .map(|row| {
            let parent_gone = row.parent.as_deref().is_some_and(|parent| {
                !live_rows.iter().any(|candidate| {
                    candidate.name.as_deref() == Some(parent)
                        && !candidate.pane_dead
                        && !launched_later(candidate, row)
                })
            });
            let team_gone = row.team.as_deref().is_some_and(|id| !team::is_running(id));
            parent_gone || team_gone
        })
        .collect())
}

/// Whether `candidate` was launched after `row`, compared as parsed instants (whole-
/// second and sub-second records both read). An unknown time is never later.
fn launched_later(candidate: &PaneRow, row: &PaneRow) -> bool {
    let instant = |row: &PaneRow| row.launched_at.as_deref().and_then(parse_rfc3339_utc);
    match (instant(candidate), instant(row)) {
        (Some(candidate), Some(row)) => instant_later(candidate, row),
        _ => false,
    }
}

/// The `PARENT` cell: `orphan:<parent>` or `orphan` for an Orphan, else the parent.
fn parent_cell(row: &PaneRow, orphan: bool) -> String {
    match (orphan, row.parent.as_deref()) {
        (true, Some(parent)) => format!("orphan:{parent}"),
        (true, None) => "orphan".to_owned(),
        (false, parent) => display_value(parent),
    }
}

/// The panes `list` reads: `--session`, `--all`, else the host's default scope.
fn list_scope(args: &ListArgs) -> ListScope<'_> {
    if let Some(session) = &args.session {
        ListScope::Session(session)
    } else if args.all {
        ListScope::All
    } else {
        ListScope::Default
    }
}

fn render_raw(rows: &[PaneRow]) {
    for row in rows {
        println!("{}", row.raw);
    }
}

fn render_concise(rows: &[PaneRow], orphans: &[bool]) {
    render_concise_at(rows, orphans, current_unix_time())
}

fn render_concise_at(rows: &[PaneRow], orphans: &[bool], now: i64) {
    let table: Vec<[String; 9]> = rows
        .iter()
        .zip(orphans)
        .map(|(row, orphan)| {
            [
                display_value(row.name.as_deref()),
                display_value(row.agent.as_deref()),
                parent_cell(row, *orphan),
                row.session.clone(),
                pane_label(row),
                display_value(row.access.as_deref()),
                display_value(row.launched_at.as_deref()),
                pane_status(row),
                format_activity(row.last_activity, now),
            ]
        })
        .collect();

    let headers = [
        "NAME", "AGENT", "PARENT", "SESSION", "PANE", "ACCESS", "LAUNCHED", "STATUS", "ACTIVITY",
    ];
    let widths = column_widths(&headers, &table);
    println!("{}", table_line(&headers.map(str::to_owned), &widths));
    for row in table {
        println!("{}", table_line(&row, &widths));
    }
}

/// One table line: every cell left-aligned to its column width, two spaces apart, as
/// the earlier fixed format printed it (trailing padding included).
fn table_line(cells: &[String; 9], widths: &[usize; 9]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join("  ")
}

fn current_unix_time() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn format_activity(activity: Option<i64>, now: i64) -> String {
    let Some(value) = activity else {
        return MISSING_GLYPH.to_owned();
    };
    if value <= 0 {
        return MISSING_GLYPH.to_owned();
    }

    let delta = now.saturating_sub(value);
    if delta < 0 {
        return MISSING_GLYPH.to_owned();
    }

    if delta < 60 {
        format!("{delta}s ago")
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

fn render_json(rows: &[PaneRow], orphans: &[bool]) -> Result<()> {
    let host = host::current();
    let pane_ids: Vec<String> = rows.iter().map(|row| row.pane_id.clone()).collect();
    let host_states = host.host_states(&pane_ids)?;
    let json_rows = rows
        .iter()
        .zip(host_states)
        .zip(orphans)
        .map(|((row, host_state), orphan)| {
            Ok(ListJsonRow {
                host: host.host(),
                name: row.name.clone(),
                agent: row.agent.clone(),
                session: (!matches!(host.host(), Host::Herdr | Host::Rex)
                    || !row.session.is_empty())
                .then(|| row.session.clone()),
                pane: pane_label(row),
                access: row.access.clone(),
                launched: row.launched_at.clone(),
                status: pane_status(row),
                last_activity: row.last_activity,
                host_state,
                parent: row.parent.clone(),
                team: row.team.clone(),
                shim_id: row.shim_id.clone(),
                orphan: *orphan,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    println!("{}", serde_json::to_string_pretty(&json_rows)?);
    Ok(())
}

fn column_widths(headers: &[&str; 9], rows: &[[String; 9]]) -> [usize; 9] {
    let mut widths = headers.map(str::len);
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.chars().count());
        }
    }
    widths
}

fn display_value(value: Option<&str>) -> String {
    format::display_value(value).to_owned()
}

fn pane_label(row: &PaneRow) -> String {
    if let Some(label) = &row.label {
        return label.clone();
    }
    format!(
        "{}.{}:{}.{}",
        row.window_index_sort, row.window_name, row.pane_index_sort, row.pane_id
    )
}

fn pane_status(row: &PaneRow) -> String {
    if row.unknown {
        return "unknown".to_owned();
    }
    if !row.pane_dead {
        return "running".to_owned();
    }

    row.pane_dead_status
        .as_ref()
        .map(|status| format!("dead+{status}"))
        .unwrap_or_else(|| "dead".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_row_includes_last_activity() {
        let row = PaneRow {
            raw: String::new(),
            session: "s".into(),
            window_index_sort: 0,
            window_name: "w".into(),
            pane_index_sort: 0,
            pane_id: "%1".into(),
            name: None,
            agent: None,
            access: None,
            launched_at: None,
            pane_dead: false,
            pane_dead_status: None,
            last_activity: Some(1_700_000_000),
            label: None,
            unknown: false,
            parent: None,
            team: None,
            shim_id: None,
        };

        let json_row = ListJsonRow {
            host: Host::Tmux,
            name: row.name.clone(),
            agent: row.agent.clone(),
            session: Some(row.session.clone()),
            pane: pane_label(&row),
            access: row.access.clone(),
            launched: row.launched_at.clone(),
            status: pane_status(&row),
            last_activity: row.last_activity,
            host_state: None,
            parent: None,
            team: None,
            shim_id: None,
            orphan: false,
        };

        let json = serde_json::to_string(&json_row).unwrap();
        assert!(
            json.contains("\"last_activity\":1700000000"),
            "expected last_activity in JSON: {json}"
        );
    }

    #[test]
    fn json_row_last_activity_null_when_missing() {
        let json_row = ListJsonRow {
            host: Host::Tmux,
            name: None,
            agent: None,
            session: Some("s".into()),
            pane: "0.w:0.%1".into(),
            access: None,
            launched: None,
            status: "running".into(),
            last_activity: None,
            host_state: None,
            parent: None,
            team: None,
            shim_id: None,
            orphan: false,
        };

        let json = serde_json::to_string(&json_row).unwrap();
        assert!(
            json.contains("\"last_activity\":null"),
            "expected null last_activity: {json}"
        );
    }

    #[test]
    fn format_activity_relative() {
        let now = 1_000_000;
        assert_eq!(format_activity(None, now), MISSING_GLYPH);
        assert_eq!(format_activity(Some(0), now), MISSING_GLYPH);
        assert_eq!(format_activity(Some(now - 5), now), "5s ago");
        assert_eq!(format_activity(Some(now - 120), now), "2m ago");
        assert_eq!(format_activity(Some(now - 7_200), now), "2h ago");
        assert_eq!(format_activity(Some(now - 172_800), now), "2d ago");
    }
}
