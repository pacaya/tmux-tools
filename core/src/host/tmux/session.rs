//! Target resolution, the Managed session, and new-window destinations on tmux.

use anyhow::{anyhow, Context, Result};
use std::env;

use super::runner;
use crate::host::WindowHome;

/// The name of the Managed session: the `tmux-tools` session this tool creates and
/// reuses when invoked from outside tmux.
pub(super) const MANAGED_SESSION: &str = "tmux-tools";

/// The history depth the spawning verbs give a session they create, so `prompt`
/// can read a completed turn back out of scrollback even after it scrolls off screen.
/// tmux's default is 2000. Cost is roughly 70 bytes per stored line (see README).
pub(super) const DEFAULT_HISTORY_LIMIT: u32 = 50_000;

/// Exact-match session target for the managed session. tmux resolves a bare session name
/// by prefix when no exact match exists, so a stray `tmux-tools-*` session would
/// otherwise be mistaken for the managed one. The `=` prefix requires an exact name.
pub(super) fn managed_session_target() -> String {
    format!("={MANAGED_SESSION}")
}

/// Exact-match pane target for the managed session's current window, for commands that
/// take a pane/window target (`display-message` needs the trailing `:`).
pub(super) fn managed_session_pane_target() -> String {
    format!("={MANAGED_SESSION}:")
}

pub(super) fn scoped_target(session: Option<&str>, window: Option<&str>) -> String {
    match (session, window) {
        (Some(session), Some(window)) => format!("{session}:{window}"),
        (Some(session), None) => session.to_owned(),
        // The managed session is always resolved by exact name, never by a bare name that
        // tmux could prefix-match to a differently named session.
        (None, Some(window)) => format!("={MANAGED_SESSION}:{window}"),
        (None, None) => format!("={MANAGED_SESSION}:"),
    }
}

pub(super) fn current_pane() -> Result<String> {
    // Prefer $TMUX_PANE: tmux sets it per-pane and child processes inherit it,
    // so it identifies the *calling* pane unambiguously. `tmux display -p` with
    // no `-t` falls back to the most-recently-active pane on the server, which
    // races when other tmux sessions/clients are opened concurrently.
    if let Ok(pane) = env::var("TMUX_PANE") {
        let trimmed = pane.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    // Fallback: parse $TMUX (`<socket>,<server_pid>,<session_id_numeric>`) to
    // recover at least the caller's *session*, and resolve that session's
    // active pane. Without this, a bare `tmux display -p` would return the
    // most-recently-active pane across all sessions.
    if let Some(session_target) = caller_session_from_tmux_env() {
        if let Ok(pane) = display_pane_id(&["display", "-p", "-t", &session_target, "#{pane_id}"]) {
            return Ok(pane);
        }
    }
    display_pane_id(&["display", "-p", "#{pane_id}"])
}

/// Extract the caller's session-id target (e.g. `$3`) from the `$TMUX` env
/// var. The format is `<socket>,<server_pid>,<session_id_numeric>`; the last
/// comma-separated field is the numeric session id (no leading `$`).
fn caller_session_from_tmux_env() -> Option<String> {
    let tmux = env::var("TMUX").ok()?;
    if tmux.is_empty() {
        return None;
    }
    let session_id_num = tmux.rsplit(',').next()?.trim();
    if session_id_num.is_empty() || !session_id_num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("${session_id_num}"))
}

pub(super) fn active_pane_for_target(target: &str) -> Result<String> {
    display_pane_id(&["display", "-p", "-t", target, "#{pane_id}"])
}

fn display_pane_id(args: &[&str]) -> Result<String> {
    let pane_id = runner::run_checked(args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!(
            "tmux command returned an empty pane id: {:?}",
            args
        ));
    }

    Ok(pane_id)
}

/// The most recently active pane of the managed session, created when absent.
pub(super) fn managed_pane() -> Result<String> {
    ensure_managed_session()?;
    most_recent_pane_in_session(&managed_session_target())
}

fn ensure_managed_session() -> Result<()> {
    let target = managed_session_target();
    let args = ["has-session", "-t", &target];
    let output = runner::run_clean(&args)
        .with_context(|| format!("failed to run tmux command: tmux {}", args.join(" ")))?;

    if output.exit_code == 0 {
        return Ok(());
    }

    runner::run_checked_clean(&["new-session", "-d", "-s", MANAGED_SESSION])?;
    Ok(())
}

fn most_recent_pane_in_session(session: &str) -> Result<String> {
    let format = "#{window_activity}\t#{window_active_pane_id}";
    let args = ["list-windows", "-t", session, "-F", format];
    let windows = runner::run_checked_clean(&args)?;
    let mut most_recent: Option<(i64, String)> = None;

    for line in windows.lines() {
        let (activity, pane_id) = line.split_once('\t').ok_or_else(|| {
            anyhow!("unexpected tmux list-windows output for session {session:?}: {line:?}")
        })?;
        let activity = activity.parse::<i64>().with_context(|| {
            format!("failed to parse window activity {activity:?} in session {session:?}")
        })?;

        match &most_recent {
            Some((best_activity, _)) if *best_activity >= activity => {}
            _ => most_recent = Some((activity, pane_id.to_owned())),
        }
    }

    let (_, pane_id) =
        most_recent.ok_or_else(|| anyhow!("tmux session {session:?} has no windows to target"))?;
    if pane_id.is_empty() {
        return Err(anyhow!(
            "tmux list-windows returned an empty pane id for session {session:?}"
        ));
    }

    Ok(pane_id)
}

/// The session id where a new window for `home` goes, and the description of `home`
/// that a failure names.
pub(super) fn new_window_session(home: WindowHome<'_>) -> Result<String> {
    let (scope, source) = match home {
        WindowHome::TargetPane(pane) => (pane.to_owned(), format!("target pane {pane}")),
        WindowHome::CallingPane(pane) => (pane.to_owned(), format!("calling pane {pane}")),
        // A new window is placed in the scoped session; `--window` names no index for it.
        WindowHome::Session(session) => (format!("{session}:"), format!("session {session:?}")),
        WindowHome::ManagedSession => (
            managed_session_pane_target(),
            format!("managed session {MANAGED_SESSION:?}"),
        ),
    };
    destination_session_id(&scope, &source)
}

/// The stable `#{session_id}` of the session that `pane_target` resolves to: the
/// destination of a new-window placement. `new-window -t` accepts neither a pane id
/// (tmux: `can't specify pane here`) nor a bare session name (tmux reads a numeric
/// name such as `0` as a window index), so every new-window placement targets the
/// session id, as the outside-tmux path does. `source` describes `pane_target` for
/// the error.
fn destination_session_id(pane_target: &str, source: &str) -> Result<String> {
    let failed = || {
        format!("new-window placement failed: cannot resolve the destination session of {source}")
    };
    let output = runner::run(&["display-message", "-p", "-t", pane_target, "#{session_id}"])
        .with_context(failed)?;
    let session_id = output.stdout.trim();
    if output.exit_code != 0 || session_id.is_empty() {
        let reason = match output.stderr.trim() {
            "" => "tmux reported no session id",
            stderr => stderr,
        };
        return Err(anyhow!("{}: {reason}", failed()));
    }
    Ok(session_id.to_owned())
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
pub(super) fn ensure_managed_session_atomic() -> Result<String> {
    if let Some(id) = managed_session_id()? {
        return Ok(id);
    }

    let private = private_session_name();
    let created = runner::run(&[
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
        let _ = runner::run(&["kill-session", "-t", &private]);
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
    let set = runner::run(&["set-option", "-t", pending.id(), "history-limit", &limit])?;
    if set.exit_code != 0 {
        return Err(anyhow!(
            "tmux set-option -t {} history-limit failed (exit {}): {}",
            pending.id(),
            set.exit_code,
            set.stderr.trim()
        ));
    }

    let renamed = runner::run(&["rename-session", "-t", pending.id(), MANAGED_SESSION])?;
    if renamed.exit_code == 0 {
        // Publish: the id now names the managed session, so the guard must not kill it.
        let id = pending.id().to_owned();
        pending.disarm();
        return Ok(id);
    }

    // The rename lost a race with another publisher, or the managed name is otherwise
    // unavailable: remove the private session and adopt the session now present.
    let private_id = pending.id().to_owned();
    let _ = runner::run(&["kill-session", "-t", &private_id]);
    pending.disarm();
    if let Some(id) = managed_session_id()? {
        return Ok(id);
    }
    Err(anyhow!(
        "tmux rename-session -t {private_id} {} failed (exit {}): {}",
        MANAGED_SESSION,
        renamed.exit_code,
        renamed.stderr.trim()
    ))
}

/// The managed session's stable `#{session_id}`, resolved by exact name, or `None` when no
/// session carries that exact name. A bare name would prefix-match a `tmux-tools-*`
/// session, so every lookup uses `=tmux-tools` (pane-targeting commands need the `:`).
fn managed_session_id() -> Result<Option<String>> {
    let session_target = managed_session_target();
    if runner::run(&["has-session", "-t", &session_target])?.exit_code != 0 {
        return Ok(None);
    }
    let pane_target = managed_session_pane_target();
    let id = runner::run_checked(&["display-message", "-p", "-t", &pane_target, "#{session_id}"])?;
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
            let _ = runner::run(&["kill-session", "-t", &id]);
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

/// The name of the session the tmux client of this invocation is in.
pub(super) fn current_session() -> Result<String> {
    let session = runner::run_checked(&["display-message", "-p", "#{session_name}"])?
        .trim()
        .to_owned();
    if session.is_empty() {
        return Err(anyhow!("tmux returned an empty current session name"));
    }

    Ok(session)
}
