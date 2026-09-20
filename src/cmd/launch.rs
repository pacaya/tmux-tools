use anyhow::{anyhow, Result};
use clap::{Args, ValueEnum};
use serde::Serialize;
use std::env;
use tmux_tools_core::{format::Format, names, target, tmux};

use crate::{util::rfc3339_utc_now, CommonArgs};

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
    /// Skip the keep-alive shell wrap and run the command bare. The pane closes
    /// the instant the command exits, just like raw `tmux split-window <cmd>`.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub(crate) bare: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Split {
    #[value(name = "h")]
    H,
    #[value(name = "v")]
    V,
    /// Force the legacy "new window" behavior even though the implicit default
    /// inside tmux is now a horizontal split. Outside tmux this is the only
    /// behavior available regardless.
    #[value(name = "window")]
    Window,
}

/// Resolved layout for a launch invocation. Computed up-front so both
/// `launch` and `spawn-agent` apply the same default rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Layout {
    /// Use `tmux split-window` with the given direction flag and optional size
    /// percentage for the new pane.
    Split {
        direction: &'static str,
        size: Option<u32>,
    },
    /// Use `tmux new-window`.
    NewWindow,
}

#[derive(Serialize)]
struct LaunchJson<'a> {
    name: Option<&'a str>,
    pane_id: &'a str,
    launched_at: &'a str,
}

pub fn run(args: &LaunchArgs) -> Result<()> {
    let layout = resolve_layout(
        args.split,
        args.size.map(u32::from),
        args.common.target.is_some(),
        args.common.session.is_some(),
        args.common.window.is_some(),
    );
    let (split_arg, size_arg) = layout.split_args();
    let launch_target = resolve_launch_target(
        args.common.target.as_ref().map(|target| target.as_str()),
        args.common.session.as_deref(),
        args.common.window.as_deref(),
        matches!(layout, Layout::Split { .. }),
    )?;

    let cmd_for_tmux = if args.bare {
        args.cmd.clone()
    } else {
        wrap_keep_open(&args.cmd)
    };

    let pane_id = launch_pane(
        &cmd_for_tmux,
        args.name.as_deref(),
        split_arg,
        size_arg,
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
                // Pin the target to the calling pane via $TMUX_PANE. Without
                // an explicit `-t`, tmux operates on the most-recently-active
                // pane on the server, which races when other sessions are
                // opened concurrently — the new pane lands in the wrong
                // session.
                let calling = target::calling_pane_id()
                    .ok_or_else(|| anyhow!("$TMUX is set but no calling pane id is available"))?;
                if let Some(split) = split {
                    split_window_args(cmd, split, size, Some(&calling))
                } else {
                    new_window_args(cmd, Some(&calling))
                }
            } else {
                // Outside tmux: ensure the managed session exists, then add a fresh
                // window for our command. A session this call creates gets the deep
                // history limit before the command's window exists, so the command's
                // first line survives scrolling. The window is created by the session's
                // stable id, not by name, so it cannot land in a replacement session.
                let managed_session = ensure_managed_session_atomic()?;
                new_window_args(cmd, Some(&managed_session))
            }
        }
    };

    let pane_id = tmux::run_checked_owned(&tmux_args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux launch command returned an empty pane id"));
    }

    Ok(pane_id)
}

/// The history depth `launch`/`spawn-agent` give a session they create, so `prompt`
/// can read a completed turn back out of scrollback even after it scrolls off screen.
/// tmux's default is 2000. Cost is roughly 70 bytes per stored line (see README).
pub(crate) const DEFAULT_HISTORY_LIMIT: u32 = 50_000;

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

/// Ensure the managed `tmux-tools` session exists, tolerating concurrent callers, and
/// return its stable `#{session_id}`.
///
/// We do *not* use `tmux new-session -d -A`: when the session already exists,
/// `-A` falls through to `attach-session`, which requires a controlling
/// terminal and fails with "open terminal failed: not a terminal" for any
/// caller without a TTY (e.g. cargo test spawning the binary with piped
/// stdio).
///
/// The managed session is always resolved by **exact** name (`=tmux-tools`), so a stray
/// `tmux-tools-*` session can never be mistaken for it. A created session is built under
/// a private, collision-proof name that does not begin with the managed name, and its
/// stable id is used to raise `history-limit` and then to rename it to `tmux-tools`. Two
/// things follow. The option is targeted by id, so it can never land on a same-named
/// session another client created, and never on the caller's session (an untargeted
/// `set-option` resolves against the caller's `TMUX` context). And the session is
/// unreachable by its managed name until the limit is set, so no concurrent client can
/// open a pane in it under the old limit. If another caller publishes first, this private
/// session is a redundant orphan: it is killed and the present session is adopted.
fn ensure_managed_session_atomic() -> Result<String> {
    if let Some(id) = managed_session_id()? {
        return Ok(id);
    }

    let private = private_session_name();
    let created = tmux::run(&[
        "new-session",
        "-d",
        "-s",
        &private,
        "-P",
        "-F",
        "#{session_id}",
    ])?;
    if created.exit_code != 0 {
        // Another caller may have created and published the managed session first.
        if let Some(id) = managed_session_id()? {
            return Ok(id);
        }
        return Err(anyhow!(
            "tmux new-session -d -s {private} failed (exit {}): {}",
            created.exit_code,
            created.stderr.trim()
        ));
    }

    let session_id = created.stdout.trim().to_owned();
    if session_id.is_empty() {
        let _ = tmux::run(&["kill-session", "-t", &private]);
        return Err(anyhow!(
            "tmux new-session -d -s {private} returned no session id"
        ));
    }

    // From here on, any exit that does not publish the session must remove it: this guard
    // kills the private session on `?` (spawn failure or timeout) as well as on the
    // explicit error paths below.
    let pending = PendingSession::new(session_id);

    // `history-limit` is a session option (it cannot be set per pane) and new windows
    // inherit it, so it must be set before the command's window exists.
    let limit = DEFAULT_HISTORY_LIMIT.to_string();
    let set = tmux::run(&["set-option", "-t", pending.id(), "history-limit", &limit])?;
    if set.exit_code != 0 {
        return Err(anyhow!(
            "tmux set-option -t {} history-limit failed (exit {}): {}",
            pending.id(),
            set.exit_code,
            set.stderr.trim()
        ));
    }

    let renamed = tmux::run(&[
        "rename-session",
        "-t",
        pending.id(),
        target::MANAGED_SESSION,
    ])?;
    if renamed.exit_code == 0 {
        // Publish: the id now names the managed session, so the guard must not kill it.
        let id = pending.id().to_owned();
        pending.disarm();
        return Ok(id);
    }

    // The rename lost a race with another publisher, or the managed name is otherwise
    // unavailable: remove the private session and adopt the session now present.
    let private_id = pending.id().to_owned();
    let _ = tmux::run(&["kill-session", "-t", &private_id]);
    pending.disarm();
    if let Some(id) = managed_session_id()? {
        return Ok(id);
    }
    Err(anyhow!(
        "tmux rename-session -t {private_id} {} failed (exit {}): {}",
        target::MANAGED_SESSION,
        renamed.exit_code,
        renamed.stderr.trim()
    ))
}

/// The managed session's stable `#{session_id}`, resolved by exact name, or `None` when no
/// session carries that exact name. A bare name would prefix-match a `tmux-tools-*`
/// session, so every lookup uses `=tmux-tools` (pane-targeting commands need the `:`).
fn managed_session_id() -> Result<Option<String>> {
    let session_target = target::managed_session_target();
    if tmux::run(&["has-session", "-t", &session_target])?.exit_code != 0 {
        return Ok(None);
    }
    let pane_target = target::managed_session_pane_target();
    let id = tmux::run_checked(&["display-message", "-p", "-t", &pane_target, "#{session_id}"])?;
    let id = id.trim().to_owned();
    if id.is_empty() {
        return Err(anyhow!(
            "tmux returned an empty session id for the managed session"
        ));
    }
    Ok(Some(id))
}

/// A created-but-unpublished managed session that removes itself on drop. It is disarmed
/// only once the session has been published under the managed name or already removed.
struct PendingSession {
    id: Option<String>,
}

impl PendingSession {
    fn new(id: String) -> Self {
        Self { id: Some(id) }
    }

    fn id(&self) -> &str {
        self.id.as_deref().unwrap_or_default()
    }

    fn disarm(mut self) {
        self.id = None;
    }
}

impl Drop for PendingSession {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _ = tmux::run(&["kill-session", "-t", &id]);
        }
    }
}

/// A private session name no other client is expected to use. It deliberately does not
/// begin with the managed name, so no prefix lookup can resolve it as the managed session.
fn private_session_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("tt-pending-{}-{nanos}", std::process::id())
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

impl Layout {
    /// Project a layout into the `(split_flag, size)` pair that `launch_pane`
    /// expects.
    pub(crate) fn split_args(self) -> (Option<&'static str>, Option<u32>) {
        match self {
            Layout::Split { direction, size } => (Some(direction), size),
            Layout::NewWindow => (None, None),
        }
    }
}

/// Resolve the effective layout from the user's flags. The implicit default
/// inside tmux is a horizontal split with the new pane occupying 70% of the
/// width — so that the caller (compressed to 30%) and the callee remain
/// visible side-by-side. `--split window` opts back into the legacy
/// new-window behavior. `--split h|v` keeps tmux's native 50/50 default
/// unless `--size` is supplied.
///
/// `--session`/`--window` without `--target` indicates window-scope intent
/// (there is no caller pane to split into), so we fall back to a new window
/// even under the implicit default.
///
/// Outside tmux there is no current pane to split, so the result is always
/// [`Layout::NewWindow`] regardless of the flags. (Explicit `--split h|v`
/// without an explicit `--target` is silently absorbed by the launch
/// dispatcher, which falls back to creating a new window in the managed
/// session — preserving long-standing behavior.)
pub(crate) fn resolve_layout(
    split: Option<Split>,
    size: Option<u32>,
    has_target: bool,
    has_session: bool,
    has_window: bool,
) -> Layout {
    match split {
        Some(Split::H) => Layout::Split {
            direction: "-h",
            size,
        },
        Some(Split::V) => Layout::Split {
            direction: "-v",
            size,
        },
        Some(Split::Window) => Layout::NewWindow,
        None => {
            let in_tmux = env::var_os("TMUX").is_some();
            let window_scope_implied = !has_target && (has_session || has_window);
            if in_tmux && !window_scope_implied {
                Layout::Split {
                    direction: "-h",
                    size: size.or(Some(70)),
                }
            } else {
                Layout::NewWindow
            }
        }
    }
}
