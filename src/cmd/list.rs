use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{self, Format, MISSING_GLYPH},
    host::{self, ListScope, PaneListing as PaneRow},
};

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
    let mut rows = host::current().list_panes(list_scope(args))?;

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
                pane_label(row),
                display_value(row.access.as_deref()),
                display_value(row.launched_at.as_deref()),
                pane_status(row),
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
            pane: pane_label(row),
            access: row.access.clone(),
            launched: row.launched_at.clone(),
            status: pane_status(row),
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
    format::display_value(value).to_owned()
}

fn pane_label(row: &PaneRow) -> String {
    format!(
        "{}.{}:{}.{}",
        row.window_index_sort, row.window_name, row.pane_index_sort, row.pane_id
    )
}

fn pane_status(row: &PaneRow) -> String {
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
        };

        let json_row = ListJsonRow {
            name: row.name.clone(),
            agent: row.agent.clone(),
            session: row.session.clone(),
            pane: pane_label(&row),
            access: row.access.clone(),
            launched: row.launched_at.clone(),
            status: pane_status(&row),
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
