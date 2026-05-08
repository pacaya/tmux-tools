use anyhow::{anyhow, Context, Result};
use clap::Args;
use serde::Serialize;
use std::env;

use crate::{
    format::{non_empty, Format},
    target, tmux, CommonArgs,
};

const STATUS_FORMAT: &str = concat!(
    "#{session_name}\x1f#{window_index}.#{window_name}\x1f#{pane_index}.#{pane_id}\x1f#{",
    crate::names::key_name!(),
    "}\x1f#{",
    crate::names::key_agent!(),
    "}\x1f#{",
    crate::names::key_access!(),
    "}\x1f#{",
    crate::names::key_launched_at!(),
    "}\x1f#{",
    crate::names::key_cwd!(),
    "}",
);
const FIELD_SEP: char = '\x1f';
const STATUS_FIELD_COUNT: usize = 8;

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Debug)]
struct StatusRow {
    session: String,
    window_index: String,
    window_name: String,
    pane_index: String,
    pane_id: String,
    name: Option<String>,
    agent: Option<String>,
    access: Option<String>,
    launched_at: Option<String>,
    cwd: Option<String>,
}

#[derive(Serialize)]
struct StatusJson {
    session: String,
    window: String,
    window_index: u64,
    pane: u64,
    pane_id: String,
    name: Option<String>,
    agent: Option<String>,
    access: Option<String>,
    launched_at: Option<String>,
    cwd: Option<String>,
}

#[derive(Serialize)]
struct NotInTmuxJson {
    in_tmux: bool,
}

pub fn run(args: &StatusArgs) -> Result<()> {
    if args.common.target.is_none() && env::var_os("TMUX").is_none() {
        return render_not_in_tmux(args.common.format);
    }

    let pane_id = match &args.common.target {
        Some(_) => Some(target::resolve_from_common(&args.common)?),
        None => None,
    };
    let raw = display_status(pane_id.as_deref())?;

    match args.common.format {
        Format::Raw => print!("{raw}"),
        Format::Concise => render_concise(&parse_status(raw.trim_end_matches('\n'))?),
        Format::Json => render_json(&parse_status(raw.trim_end_matches('\n'))?)?,
    }

    Ok(())
}

fn render_not_in_tmux(format: Format) -> Result<()> {
    match format {
        Format::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&NotInTmuxJson { in_tmux: false })?
            );
        }
        Format::Concise | Format::Raw => println!("not in tmux"),
    }

    Ok(())
}

fn display_status(pane_id: Option<&str>) -> Result<String> {
    let args = match pane_id {
        Some(pane_id) => vec!["display-message", "-p", "-t", pane_id, STATUS_FORMAT],
        None => vec!["display-message", "-p", STATUS_FORMAT],
    };

    tmux::run_checked(&args)
}

fn parse_status(line: &str) -> Result<StatusRow> {
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

    Ok(StatusRow {
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

fn render_concise(row: &StatusRow) {
    println!("session: {}", row.session);
    println!("window: {}.{}", row.window_index, row.window_name);
    println!("pane: {}.{}", row.pane_index, row.pane_id);

    if let Some(name) = &row.name {
        println!("name: {name}");
    }
    if let Some(agent) = &row.agent {
        println!("agent: {agent}");
    }
}

fn render_json(row: &StatusRow) -> Result<()> {
    let json = StatusJson {
        session: row.session.clone(),
        window: row.window_name.clone(),
        window_index: row.window_index.parse().with_context(|| {
            format!(
                "failed to parse tmux window index {:?} for pane {}",
                row.window_index, row.pane_id
            )
        })?,
        pane: row.pane_index.parse().with_context(|| {
            format!(
                "failed to parse tmux pane index {:?} for pane {}",
                row.pane_index, row.pane_id
            )
        })?,
        pane_id: row.pane_id.clone(),
        name: row.name.clone(),
        agent: row.agent.clone(),
        access: row.access.clone(),
        launched_at: row.launched_at.clone(),
        cwd: row.cwd.clone(),
    };

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_line(fields: &[&str]) -> String {
        fields.join(&FIELD_SEP.to_string())
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
