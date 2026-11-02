use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{Format, MISSING_GLYPH},
    layout, target,
};

use crate::{cmd::safety, CommonArgs};

const VERB: &str = "kill";

#[derive(Args, Debug)]
pub struct KillArgs {
    /// Allow killing the pane that is invoking tmux-tools (i.e. `$TMUX_PANE`).
    /// Without this, `kill` refuses to act on the calling pane.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Allow killing panes that were not created by tmux-tools (no `@tt-name`
    /// or `@tt-agent`) or whose recorded `@tt-cwd` differs from the current
    /// working directory.
    #[arg(long, default_value_t = false)]
    pub(crate) any: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct KillJson<'a> {
    pane_id: &'a str,
    name: Option<&'a str>,
    agent: Option<&'a str>,
}

pub fn run(args: &KillArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;
    let registered = safety::enforce_for_pane(VERB, &pane, args.force, args.any)?;

    // The safety verdict above governs destruction only (`--any` included). Whether the
    // window is retiled is the layout module's window guard, evaluated across the whole
    // window before the pane is destroyed.
    layout::remove_pane(&pane, current_cwd().as_deref())?;

    match args.common.format {
        Format::Concise => {
            println!(
                "killed pane={} name={}",
                pane,
                registered.name.as_deref().unwrap_or(MISSING_GLYPH)
            );
        }
        Format::Json => {
            let output = KillJson {
                pane_id: &pane,
                name: registered.name.as_deref(),
                agent: registered.agent.as_deref(),
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!("{pane}"),
    }

    Ok(())
}

/// The invoking process's cwd, the ownership predicate's `current_cwd`; `None` when it
/// is unreadable.
fn current_cwd() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.to_string_lossy().into_owned())
}
