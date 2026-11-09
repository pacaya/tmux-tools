use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use std::env;
use tmux_tools_core::{
    format::Format,
    host::{self, Host, HostState, PaneSummary as StatusRow},
    layout::{self, WindowReport},
    target,
};

use crate::{util::load_registry_for_floors, CommonArgs};

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct StatusJson {
    host: Host,
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
    /// The smallest pane width in the pane's window.
    min_pane_cols: u32,
    /// The smallest pane height in the pane's window.
    min_pane_rows: u32,
    /// Whether the pane's window is below its effective floor.
    below_floor: bool,
    host_state: Option<HostState>,
}

#[derive(Serialize)]
struct NotInTmuxJson {
    host: Host,
    in_tmux: bool,
    host_state: Option<HostState>,
}

pub fn run(args: &StatusArgs) -> Result<()> {
    if args.common.target.is_none() && env::var_os("TMUX").is_none() {
        return render_not_in_tmux(args.common.format);
    }

    let pane_id = match &args.common.target {
        Some(_) => Some(target::resolve_from_common(&args.common)?),
        None => None,
    };
    let description = host::current().describe_pane(pane_id.as_deref())?;

    match args.common.format {
        Format::Raw => print!("{}", description.raw),
        Format::Concise => {
            let row = description.summary?;
            render_concise(&row, &window_report(&row)?);
        }
        Format::Json => {
            let row = description.summary?;
            render_json(&row, &window_report(&row)?)?;
        }
    }

    Ok(())
}

fn render_not_in_tmux(format: Format) -> Result<()> {
    match format {
        Format::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&NotInTmuxJson {
                    host: host::current().host(),
                    in_tmux: false,
                    // No pane, so no Host state.
                    host_state: None,
                })?
            );
        }
        Format::Concise | Format::Raw => println!("not in tmux"),
    }

    Ok(())
}

/// The measured geometry and floor verdict of the window holding the status pane.
fn window_report(row: &StatusRow) -> Result<WindowReport> {
    let registry = load_registry_for_floors();
    layout::inspect(&row.pane_id, &registry)
}

fn render_concise(row: &StatusRow, window: &WindowReport) {
    println!("session: {}", row.session);
    println!("window: {}.{}", row.window_index, row.window_name);
    println!("pane: {}.{}", row.pane_index, row.pane_id);

    if let Some(name) = &row.name {
        println!("name: {name}");
    }
    if let Some(agent) = &row.agent {
        println!("agent: {agent}");
    }
    println!("min_pane_cols: {}", window.min_pane_cols);
    println!("min_pane_rows: {}", window.min_pane_rows);
    println!("below_floor: {}", window.below_floor);
}

fn render_json(row: &StatusRow, window: &WindowReport) -> Result<()> {
    let host = host::current();
    let json = StatusJson {
        host: host.host(),
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
        min_pane_cols: window.min_pane_cols,
        min_pane_rows: window.min_pane_rows,
        below_floor: window.below_floor,
        host_state: host.host_state(&row.pane_id)?,
    };

    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}
