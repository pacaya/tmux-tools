use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::Format,
    layout::{self, Destination, Landing, Registration, Split},
    names,
};

use crate::{
    util::{load_registry_for_floors, rfc3339_utc_now},
    CommonArgs,
};

#[derive(Args, Debug)]
pub struct LaunchArgs {
    #[arg(long, value_name = "SHELL")]
    pub(crate) cmd: String,
    #[arg(long, value_name = "NAME")]
    pub(crate) name: Option<String>,
    /// How to place the pane. Inside tmux the default is tile
    #[arg(long, value_enum, value_name = "tile|h|v|window")]
    pub(crate) split: Option<Split>,
    /// The new pane's percentage of the split pane. Alone it is a horizontal split at
    /// N% (no retile); rejected with --split tile
    #[arg(long, value_name = "N")]
    pub(crate) size: Option<u8>,
    /// Skip the keep-alive shell wrap and run the command bare. The pane closes
    /// the instant the command exits, just like raw `tmux split-window <cmd>`.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub(crate) bare: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct LaunchJson<'a> {
    name: &'a str,
    pane_id: &'a str,
    launched_at: &'a str,
    window_id: &'a str,
    min_pane_cols: u32,
    min_pane_rows: u32,
    below_floor: bool,
}

pub fn run(args: &LaunchArgs) -> Result<()> {
    let cmd_for_tmux = if args.bare {
        args.cmd.clone()
    } else {
        wrap_keep_open(&args.cmd)
    };
    let launched_at = rfc3339_utc_now()?;
    let cwd = invoking_cwd();
    // Loaded before the pane exists: the verdict needs it, and its warnings belong to
    // this invocation whether or not placement succeeds.
    let registry = load_registry_for_floors();

    let landing = layout::place(
        &cmd_for_tmux,
        &destination(&args.common, args.split, args.size.map(u32::from)),
        &Registration {
            name: args.name.as_deref(),
            // Derive from the user's command, never from the keep-alive wrap.
            name_source: names::first_command_word(&args.cmd),
            launched_at: &launched_at,
            cwd: cwd.as_deref(),
            agent: None,
        },
        &registry,
    )?;

    render_output(args, &landing, &launched_at)
}

/// The placement destination the spawning verbs' shared flags ask for.
pub(crate) fn destination(
    common: &CommonArgs,
    split: Option<Split>,
    size: Option<u32>,
) -> Destination<'_> {
    Destination {
        split,
        size,
        target: common.target.as_ref().map(|target| target.as_str()),
        session: common.session.as_deref(),
        window: common.window.as_deref(),
    }
}

/// The invoking process's cwd for `@tt-cwd`, `None` when it is unreadable.
pub(crate) fn invoking_cwd() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.to_string_lossy().into_owned())
}

/// Wrap a command so the pane survives its exit by `exec`-ing into a fallback
/// shell. Without this, `tmux split-window <cmd>` reaps the pane the instant
/// `<cmd>` exits — taking error output with it.
///
/// The user's command runs in a `(...)` subshell so an embedded `exit` only
/// terminates that subshell, not the wrapping shell that performs the `exec`.
/// The fallback shell is taken from `$SHELL`, falling back to `/bin/sh`. We
/// resolve it at tmux-tools launch time and embed the literal path so the
/// wrapper string only relies on POSIX `(...)`, `;`, and `exec`, which every
/// shell tmux might dispatch via `default-shell -c` understands.
pub(crate) fn wrap_keep_open(cmd: &str) -> String {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned());
    format!("({cmd}); exec {shell}")
}

/// Print the launch result; the name is the registered `@tt-name`, not the flag value.
fn render_output(args: &LaunchArgs, landing: &Landing, launched_at: &str) -> Result<()> {
    match args.common.format {
        Format::Concise => println!(
            "name={} pane={} window_id={} min_pane_cols={} min_pane_rows={} below_floor={}",
            landing.name,
            landing.pane_id,
            landing.window_id,
            landing.min_pane_cols,
            landing.min_pane_rows,
            landing.below_floor
        ),
        Format::Raw => println!("{}", landing.pane_id),
        Format::Json => {
            let output = LaunchJson {
                name: &landing.name,
                pane_id: &landing.pane_id,
                launched_at,
                window_id: &landing.window_id,
                min_pane_cols: landing.min_pane_cols,
                min_pane_rows: landing.min_pane_rows,
                below_floor: landing.below_floor,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
    }

    Ok(())
}
