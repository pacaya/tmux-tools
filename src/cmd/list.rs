use anyhow::{anyhow, Context, Result};
use clap::Args;
use serde::Serialize;
use std::collections::BTreeSet;
use std::env;

use crate::{
    format::{non_empty, Format, MISSING_GLYPH},
    names, target, tmux,
};

const LIST_FORMAT: &str = concat!(
    "#{session_name}\x1f#{window_index}\x1f#{window_name}\x1f#{pane_index}\x1f#{pane_id}\x1f#{",
    names::key_name!(),
    "}\x1f#{",
    names::key_agent!(),
    "}\x1f#{",
    names::key_access!(),
    "}\x1f#{",
    names::key_launched_at!(),
    "}\x1f#{pane_dead}\x1f#{pane_dead_status}\x1f#{window_activity}",
);
const FIELD_SEP: char = '\x1f';

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(long, value_name = "NAME")]
    pub session: Option<String>,
    #[arg(long)]
    pub all: bool,
    #[arg(long, value_enum, default_value_t = Format::Concise)]
    pub format: Format,
}

#[derive(Debug)]
struct PaneRow {
    raw: String,
    session: String,
    window_index_sort: u64,
    window_name: String,
    pane_index_sort: u64,
    pane_id: String,
    name: Option<String>,
    agent: Option<String>,
    access: Option<String>,
    launched_at: Option<String>,
    pane_dead: bool,
    pane_dead_status: Option<String>,
    last_activity: Option<i64>,
}

#[derive(Serialize)]
struct ListJsonRow {
    name: Option<String>,
    agent: Option<String>,
    session: String,
    pane: String,
    access: Option<String>,
    launched: Option<String>,
    status: String,
    last_activity: Option<i64>,
}

pub fn run(args: &ListArgs) -> Result<()> {
    let scope = session_scope(args)?;
    let mut rows = list_panes()?;

    if let Some(scope) = &scope {
        rows.retain(|row| scope.contains(&row.session));
    }

    rows.sort_by(|left, right| {
        left.session
            .cmp(&right.session)
            .then(left.window_index_sort.cmp(&right.window_index_sort))
            .then(left.pane_index_sort.cmp(&right.pane_index_sort))
    });

    match args.format {
        Format::Raw => render_raw(&rows),
        Format::Concise => render_concise(&rows),
        Format::Json => render_json(&rows)?,
    }

    Ok(())
}

fn session_scope(args: &ListArgs) -> Result<Option<BTreeSet<String>>> {
    if let Some(session) = &args.session {
        return Ok(Some(BTreeSet::from([session.clone()])));
    }

    if args.all {
        return Ok(None);
    }

    let mut scope = BTreeSet::from([target::MANAGED_SESSION.to_owned()]);
    if env::var_os("TMUX").is_some() {
        scope.insert(current_session()?);
    }

    Ok(Some(scope))
}

fn current_session() -> Result<String> {
    let session = tmux::run_checked(&["display-message", "-p", "#{session_name}"])?
        .trim()
        .to_owned();
    if session.is_empty() {
        return Err(anyhow!("tmux returned an empty current session name"));
    }

    Ok(session)
}

fn list_panes() -> Result<Vec<PaneRow>> {
    let output = tmux::run(&["list-panes", "-a", "-F", LIST_FORMAT])
        .context("failed to run tmux command: tmux list-panes -a")?;

    if output.exit_code != 0 {
        if stderr_indicates_no_server(&output.stderr) {
            return Ok(Vec::new());
        }

        return Err(anyhow!(
            "tmux command failed (args: {:?}, exit code {}): {}",
            ["list-panes", "-a", "-F", LIST_FORMAT],
            output.exit_code,
            output.stderr.trim()
        ));
    }

    output.stdout.lines().map(parse_pane_row).collect()
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

fn parse_pane_row(line: &str) -> Result<PaneRow> {
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

    Ok(PaneRow {
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
    })
}

fn render_raw(rows: &[PaneRow]) {
    for row in rows {
        println!("{}", row.raw);
    }
}

fn render_concise(rows: &[PaneRow]) {
    render_concise_at(rows, current_unix_time())
}

fn render_concise_at(rows: &[PaneRow], now: i64) {
    let table: Vec<[String; 8]> = rows
        .iter()
        .map(|row| {
            [
                display_value(row.name.as_deref()),
                display_value(row.agent.as_deref()),
                row.session.clone(),
                row.pane_label(),
                display_value(row.access.as_deref()),
                display_value(row.launched_at.as_deref()),
                row.status(),
                format_activity(row.last_activity, now),
            ]
        })
        .collect();

    let headers = [
        "NAME", "AGENT", "SESSION", "PANE", "ACCESS", "LAUNCHED", "STATUS", "ACTIVITY",
    ];
    let widths = column_widths(&headers, &table);

    println!(
        "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}  {:<w6$}  {:<w7$}",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        headers[5],
        headers[6],
        headers[7],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
        w5 = widths[5],
        w6 = widths[6],
        w7 = widths[7],
    );

    for row in table {
        println!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {:<w5$}  {:<w6$}  {:<w7$}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            row[6],
            row[7],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
            w5 = widths[5],
            w6 = widths[6],
            w7 = widths[7],
        );
    }
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

fn render_json(rows: &[PaneRow]) -> Result<()> {
    let json_rows: Vec<ListJsonRow> = rows
        .iter()
        .map(|row| ListJsonRow {
            name: row.name.clone(),
            agent: row.agent.clone(),
            session: row.session.clone(),
            pane: row.pane_label(),
            access: row.access.clone(),
            launched: row.launched_at.clone(),
            status: row.status(),
            last_activity: row.last_activity,
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&json_rows)?);
    Ok(())
}

fn column_widths(headers: &[&str; 8], rows: &[[String; 8]]) -> [usize; 8] {
    let mut widths = headers.map(str::len);
    for row in rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.chars().count());
        }
    }
    widths
}

fn display_value(value: Option<&str>) -> String {
    crate::format::display_value(value).to_owned()
}

impl PaneRow {
    fn pane_label(&self) -> String {
        format!(
            "{}.{}:{}.{}",
            self.window_index_sort, self.window_name, self.pane_index_sort, self.pane_id
        )
    }

    fn status(&self) -> String {
        if !self.pane_dead {
            return "running".to_owned();
        }

        self.pane_dead_status
            .as_ref()
            .map(|status| format!("dead+{status}"))
            .unwrap_or_else(|| "dead".to_owned())
    }
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
        let line = build_line(&[
            "s", "0", "w", "0", "%1", "", "", "", "", "0", "", "0",
        ]);
        let row = parse_pane_row(&line).expect("row parses");
        assert_eq!(row.last_activity, None);
    }

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
        };

        let json_row = ListJsonRow {
            name: row.name.clone(),
            agent: row.agent.clone(),
            session: row.session.clone(),
            pane: row.pane_label(),
            access: row.access.clone(),
            launched: row.launched_at.clone(),
            status: row.status(),
            last_activity: row.last_activity,
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
            name: None,
            agent: None,
            session: "s".into(),
            pane: "0.w:0.%1".into(),
            access: None,
            launched: None,
            status: "running".into(),
            last_activity: None,
        };

        let json = serde_json::to_string(&json_row).unwrap();
        assert!(
            json.contains("\"last_activity\":null"),
            "expected null last_activity: {json}"
        );
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
