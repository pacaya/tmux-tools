use anyhow::{anyhow, Result};
use clap::{Args, ValueEnum};
use serde::Serialize;
use std::env;

use crate::{format::Format, names, target, tmux, util::rfc3339_utc_now, CommonArgs};

/// Resolved tmux target for `launch_pane`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchTarget {
    /// Pane id to split (used when `split` is also supplied).
    SplitPane(String),
    /// `session[:window]` (or session id) for new-window placement.
    WindowScope(String),
}

#[derive(Args, Debug)]
pub struct LaunchArgs {
    #[arg(long, value_name = "SHELL")]
    pub(crate) cmd: String,
    #[arg(long, value_name = "NAME")]
    pub(crate) name: Option<String>,
    #[arg(long, value_enum, value_name = "h|v")]
    pub(crate) split: Option<Split>,
    #[arg(long, value_name = "N")]
    pub(crate) size: Option<u8>,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Split {
    #[value(name = "h")]
    H,
    #[value(name = "v")]
    V,
}

#[derive(Serialize)]
struct LaunchJson<'a> {
    name: Option<&'a str>,
    pane_id: &'a str,
    launched_at: &'a str,
}

pub fn run(args: &LaunchArgs) -> Result<()> {
    let split_arg = args.split.map(Split::tmux_flag);
    let launch_target = resolve_launch_target(
        args.common.target.as_ref().map(|target| target.as_str()),
        args.common.session.as_deref(),
        args.common.window.as_deref(),
        split_arg.is_some(),
    )?;

    let pane_id = launch_pane(
        &args.cmd,
        args.name.as_deref(),
        split_arg,
        args.size.map(u32::from),
        launch_target.as_ref(),
    )?;
    let launched_at = rfc3339_utc_now()?;

    if let Some(name) = &args.name {
        names::set(&pane_id, names::KEY_NAME, name)?;
    }
    names::set(&pane_id, names::KEY_LAUNCHED_AT, &launched_at)?;
    if let Ok(cwd) = std::env::current_dir() {
        names::set(&pane_id, names::KEY_CWD, &cwd.to_string_lossy())?;
    }

    render_output(args, &pane_id, &launched_at)
}

/// Resolve the caller's `--target`/`--session`/`--window` flags into a
/// [`LaunchTarget`].
///
/// `target::resolve` always yields a pane id (`%N`). For split that's
/// exactly what we want; for new-window we widen the scope to the
/// containing session so tmux creates the window in the right place.
pub(crate) fn resolve_launch_target(
    target: Option<&str>,
    session: Option<&str>,
    window: Option<&str>,
    split: bool,
) -> Result<Option<LaunchTarget>> {
    if let Some(raw) = target {
        let spec = target::parse(raw);
        let pane_id = target::resolve(&spec, session, window)?;
        if split {
            return Ok(Some(LaunchTarget::SplitPane(pane_id)));
        }
        let session = session_for_pane(&pane_id)?;
        return Ok(Some(LaunchTarget::WindowScope(session)));
    }

    if session.is_some() || window.is_some() {
        return Ok(Some(LaunchTarget::WindowScope(target::scoped_target(
            session, window,
        ))));
    }

    Ok(None)
}

fn session_for_pane(pane_id: &str) -> Result<String> {
    let args = ["display", "-p", "-t", pane_id, "#{session_name}"];
    let session = tmux::run_checked(&args)?.trim().to_owned();
    if session.is_empty() {
        return Err(anyhow!(
            "tmux failed to report session for pane {pane_id:?}"
        ));
    }
    Ok(session)
}

pub fn launch_pane(
    cmd: &str,
    name: Option<&str>,
    split: Option<&str>,
    size: Option<u32>,
    target: Option<&LaunchTarget>,
) -> Result<String> {
    let _ = name;

    let tmux_args = match target {
        Some(LaunchTarget::SplitPane(pane)) => {
            let split = split.ok_or_else(|| {
                anyhow!("internal: split target supplied without a split direction")
            })?;
            split_window_args(cmd, split, size, Some(pane))
        }
        Some(LaunchTarget::WindowScope(scope)) => {
            if let Some(split) = split {
                split_window_args(cmd, split, size, Some(scope))
            } else {
                new_window_args(cmd, Some(scope))
            }
        }
        None => {
            if env::var_os("TMUX").is_some() {
                if let Some(split) = split {
                    split_window_args(cmd, split, size, None)
                } else {
                    new_window_args(cmd, None)
                }
            } else {
                // Outside tmux: ensure the managed session exists, then add a
                // fresh window for our command.
                ensure_managed_session_atomic()?;
                new_window_args(cmd, Some(&format!("{}:", target::MANAGED_SESSION)))
            }
        }
    };

    let pane_id = tmux::run_checked_owned(&tmux_args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux launch command returned an empty pane id"));
    }

    Ok(pane_id)
}

fn split_window_args(
    cmd: &str,
    split: &str,
    size: Option<u32>,
    target: Option<&str>,
) -> Vec<String> {
    let mut tmux_args = vec!["split-window".to_owned(), split.to_owned()];

    if let Some(target) = target {
        tmux_args.push("-t".to_owned());
        tmux_args.push(target.to_owned());
    }

    if let Some(size) = size {
        tmux_args.push("-p".to_owned());
        tmux_args.push(size.to_string());
    }

    tmux_args.push("-P".to_owned());
    tmux_args.push("-F".to_owned());
    tmux_args.push("#{pane_id}".to_owned());
    tmux_args.push(cmd.to_owned());
    tmux_args
}

fn new_window_args(cmd: &str, target: Option<&str>) -> Vec<String> {
    let mut tmux_args = vec!["new-window".to_owned()];
    if let Some(target) = target {
        tmux_args.push("-t".to_owned());
        tmux_args.push(target.to_owned());
    }
    tmux_args.push("-P".to_owned());
    tmux_args.push("-F".to_owned());
    tmux_args.push("#{pane_id}".to_owned());
    tmux_args.push(cmd.to_owned());
    tmux_args
}

/// Ensure the managed `tmux-tools` session exists, tolerating concurrent
/// callers.
///
/// We do *not* use `tmux new-session -d -A`: when the session already exists,
/// `-A` falls through to `attach-session`, which requires a controlling
/// terminal and fails with "open terminal failed: not a terminal" for any
/// caller without a TTY (e.g. cargo test spawning the binary with piped
/// stdio).
///
/// Instead, check first with `has-session` and only call `new-session -d`
/// when missing. If two callers race and both try to create, the loser's
/// `new-session` errors with "duplicate session"; we recover by re-checking
/// and treating a present session as success.
fn ensure_managed_session_atomic() -> Result<()> {
    if tmux::run(&["has-session", "-t", target::MANAGED_SESSION])?.exit_code == 0 {
        return Ok(());
    }

    let create = tmux::run(&["new-session", "-d", "-s", target::MANAGED_SESSION])?;
    if create.exit_code == 0 {
        return Ok(());
    }
    if tmux::run(&["has-session", "-t", target::MANAGED_SESSION])?.exit_code == 0 {
        return Ok(());
    }
    Err(anyhow!(
        "tmux new-session -d -s {} failed (exit {}): {}",
        target::MANAGED_SESSION,
        create.exit_code,
        create.stderr.trim()
    ))
}

fn render_output(args: &LaunchArgs, pane_id: &str, launched_at: &str) -> Result<()> {
    match args.common.format {
        Format::Concise => {
            if let Some(name) = &args.name {
                println!("name={name} pane={pane_id}");
            } else {
                println!("pane={pane_id}");
            }
        }
        Format::Raw => println!("{pane_id}"),
        Format::Json => {
            let output = LaunchJson {
                name: args.name.as_deref(),
                pane_id,
                launched_at,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
    }

    Ok(())
}

impl Split {
    pub(crate) fn tmux_flag(self) -> &'static str {
        match self {
            Self::H => "-h",
            Self::V => "-v",
        }
    }
}
