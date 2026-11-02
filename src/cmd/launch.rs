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
    relocated: bool,
    #[serde(flatten)]
    window: Option<WindowJson<'a>>,
}

/// The landing window's fields of the spawning verbs' JSON. Absent, keys and all,
/// when the pane was gone before its window was measured.
#[derive(Serialize)]
pub(crate) struct WindowJson<'a> {
    window_id: &'a str,
    min_pane_cols: u32,
    min_pane_rows: u32,
    below_floor: bool,
}

impl<'a> WindowJson<'a> {
    pub(crate) fn of(landing: &'a Landing) -> Option<Self> {
        landing.window.as_ref().map(|window| WindowJson {
            window_id: &window.window_id,
            min_pane_cols: window.min_pane_cols,
            min_pane_rows: window.min_pane_rows,
            below_floor: window.below_floor,
        })
    }
}

/// The landing's fields of the spawning verbs' concise line, each preceded by a space:
/// `relocated`, then the landing window's fields, which are left out when the pane was
/// gone before its window was measured.
pub(crate) fn concise_landing_fields(landing: &Landing) -> String {
    let window = landing
        .window
        .as_ref()
        .map(|window| {
            format!(
                " window_id={} min_pane_cols={} min_pane_rows={} below_floor={}",
                window.window_id, window.min_pane_cols, window.min_pane_rows, window.below_floor
            )
        })
        .unwrap_or_default();
    format!(" relocated={}{window}", landing.relocated)
}

/// Warn on stderr, once, when the pane was gone before its window was measured.
pub(crate) fn warn_if_unmeasured(landing: &Landing) {
    if landing.window.is_none() {
        eprintln!(
            "warning: pane {} no longer exists; window geometry not measured",
            landing.pane_id
        );
    }
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

/// Print the launch result; the name is the one placement chose (`--name`, else the
/// derived name), which is the registered `@tt-name` whenever registration completed.
fn render_output(args: &LaunchArgs, landing: &Landing, launched_at: &str) -> Result<()> {
    warn_if_unmeasured(landing);
    match args.common.format {
        Format::Concise => println!(
            "name={} pane={}{}",
            landing.name,
            landing.pane_id,
            concise_landing_fields(landing)
        ),
        Format::Raw => println!("{}", landing.pane_id),
        Format::Json => {
            let output = LaunchJson {
                name: &landing.name,
                pane_id: &landing.pane_id,
                launched_at,
                relocated: landing.relocated,
                window: WindowJson::of(landing),
            };
            println!("{}", serde_json::to_string(&output)?);
        }
    }

    Ok(())
}
