//! Pane placement for the spawning verbs (`launch`, `spawn-agent`).
//!
//! This module is the single owner of placement. [`place`] resolves where a new
//! pane goes, creates it (split or new window), writes its whole registration, and
//! then measures the window it landed in, returning a [`Landing`]. The verbs build
//! the command and the registration; they never create a pane themselves.
//!
//! It also owns floors: [`pane_floor`] resolves a pane's floor from its registration
//! and the agent registry, [`effective_floor`] combines a window's, and
//! [`floor_violations`] / [`below_floor`] give the verdict. Those are pure over measured
//! geometry; [`place`] and [`inspect`] measure and apply them. The floor is advisory:
//! nothing here relocates or refuses a pane.

use anyhow::{anyhow, Context, Result};
use std::env;

use crate::agents::Registry;
use crate::format::non_empty;
use crate::{names, target, tmux};

/// The `--split` flag of the spawning verbs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
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

/// Resolved layout for a placement. Computed up-front so both spawning verbs apply
/// the same default rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    /// Use `tmux split-window` with the given direction flag and optional size
    /// percentage for the new pane.
    Split {
        direction: &'static str,
        size: Option<u32>,
    },
    /// Use `tmux new-window`.
    NewWindow,
}

impl Layout {
    /// Project a layout into the `(split_flag, size)` pair pane creation expects.
    fn split_args(self) -> (Option<&'static str>, Option<u32>) {
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
/// Outside tmux (`in_tmux` false) there is no current pane to split, so the
/// implicit default is [`Layout::NewWindow`]. (Explicit `--split h|v` without an
/// explicit `--target` is absorbed by placement, which falls back to creating a
/// new window in the managed session — preserving long-standing behavior.)
pub fn resolve_layout(
    split: Option<Split>,
    size: Option<u32>,
    has_target: bool,
    has_session: bool,
    has_window: bool,
    in_tmux: bool,
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

/// Where the caller asked the new pane to go: the spawning verbs' layout and
/// target flags, as given.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Destination<'a> {
    pub split: Option<Split>,
    pub size: Option<u32>,
    pub target: Option<&'a str>,
    pub session: Option<&'a str>,
    pub window: Option<&'a str>,
}

/// The whole registration [`place`] writes on the new pane. A `None` field is never
/// written, not even as an empty value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registration<'a> {
    /// The `--name` value. Without it, `@tt-name` is derived from `name_source`.
    pub name: Option<&'a str>,
    /// The launched command's first word (never the keep-alive wrap), the stem of a
    /// derived `@tt-name`.
    pub name_source: &'a str,
    /// `@tt-launched-at`.
    pub launched_at: &'a str,
    /// `@tt-cwd`: the invoking process's cwd, `None` when it is unreadable.
    pub cwd: Option<&'a str>,
    /// The agent fields; `None` for `launch`.
    pub agent: Option<AgentRegistration<'a>>,
}

/// The `spawn-agent` part of a pane's registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentRegistration<'a> {
    /// `@tt-agent`.
    pub agent: &'a str,
    /// `@tt-surface`: the resolved surface, recorded unconditionally.
    pub surface: &'a str,
    /// `@tt-access`: the `--access` value, `None` without the flag.
    pub access: Option<&'a str>,
    /// `@tt-surface-unvalidated`: set when caller-supplied trailing arguments follow.
    pub surface_unvalidated: bool,
}

/// Where a placed pane landed, measured after its registration was written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Landing {
    pub pane_id: String,
    /// The `@tt-name` actually written.
    pub name: String,
    /// The stable tmux id of the window the pane landed in.
    pub window_id: String,
    /// The smallest pane width measured across every pane in that window.
    pub min_pane_cols: u32,
    /// The smallest pane height measured across every pane in that window.
    pub min_pane_rows: u32,
    /// Whether that window is below its effective floor (see [`below_floor`]).
    pub below_floor: bool,
}

/// A window's measured geometry and floor verdict, as [`inspect`] reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowReport {
    /// The stable tmux id of the window.
    pub window_id: String,
    /// The smallest pane width measured across every pane in the window.
    pub min_pane_cols: u32,
    /// The smallest pane height measured across every pane in the window.
    pub min_pane_rows: u32,
    /// Whether the window is below its effective floor (see [`below_floor`]).
    pub below_floor: bool,
}

/// A minimum pane geometry: a pane's floor, or a window's effective floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Floor {
    pub cols: u32,
    pub rows: u32,
}

/// The floor of every axis a resolved surface does not declare, and of a pane whose
/// agent or recorded surface the registry does not have. An explicit assumption — the
/// componentwise maximum of the measured seeds — not a measurement of any agent.
pub const DEFAULT_FLOOR: Floor = Floor { cols: 58, rows: 16 };

/// One pane of a window, as measured, with the registration its floor resolves from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneGeometry {
    pub pane_id: String,
    pub cols: u32,
    pub rows: u32,
    /// `@tt-agent`; `None` when unset.
    pub agent: Option<String>,
    /// `@tt-surface`; `None` when unset.
    pub surface: Option<String>,
}

/// Every pane of one window, as measured.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowGeometry {
    pub window_id: String,
    /// Never empty.
    pub panes: Vec<PaneGeometry>,
}

impl WindowGeometry {
    /// The smallest pane width in the window.
    pub fn min_pane_cols(&self) -> u32 {
        self.panes.iter().map(|pane| pane.cols).min().unwrap_or(0)
    }

    /// The smallest pane height in the window.
    pub fn min_pane_rows(&self) -> u32 {
        self.panes.iter().map(|pane| pane.rows).min().unwrap_or(0)
    }

    fn report(&self, registry: &Registry) -> WindowReport {
        WindowReport {
            window_id: self.window_id.clone(),
            min_pane_cols: self.min_pane_cols(),
            min_pane_rows: self.min_pane_rows(),
            below_floor: below_floor(registry, &self.panes),
        }
    }
}

/// The floor a pane contributes, from its `@tt-agent` and `@tt-surface`.
///
/// - No agent: `None` — the pane contributes no floor (it is still measured).
/// - The surface resolves as readiness resolves it ([`Registry::resolve_surface`]): a
///   recorded surface is that surface, an absent record the agent's pre-surface
///   rendering. Each axis the resolved surface declares keeps its value; an undeclared
///   axis takes [`DEFAULT_FLOOR`]'s.
/// - An agent the registry does not know, or a recorded surface it does not have:
///   [`DEFAULT_FLOOR`].
pub fn pane_floor(
    registry: &Registry,
    agent: Option<&str>,
    surface: Option<&str>,
) -> Option<Floor> {
    let agent = agent?;
    Some(match registry.resolve_surface(agent, surface) {
        Some(resolved) => Floor {
            cols: resolved.surface.min_cols.unwrap_or(DEFAULT_FLOOR.cols),
            rows: resolved.surface.min_rows.unwrap_or(DEFAULT_FLOOR.rows),
        },
        None => DEFAULT_FLOOR,
    })
}

/// A window's effective floor: the componentwise maximum over the floors its panes
/// contribute. `None` when no pane contributes one (no pane has an agent).
pub fn effective_floor(registry: &Registry, panes: &[PaneGeometry]) -> Option<Floor> {
    panes
        .iter()
        .filter_map(|pane| pane_floor(registry, pane.agent.as_deref(), pane.surface.as_deref()))
        .reduce(|a, b| Floor {
            cols: a.cols.max(b.cols),
            rows: a.rows.max(b.rows),
        })
}

/// The panes narrower or shorter than `floor` — every pane, with or without an agent.
pub fn floor_violations(panes: &[PaneGeometry], floor: Floor) -> Vec<&PaneGeometry> {
    panes
        .iter()
        .filter(|pane| pane.cols < floor.cols || pane.rows < floor.rows)
        .collect()
}

/// Whether the window these panes make up is below its effective floor: some pane (so
/// the window's measured minimum on that axis) is below it. A window with no
/// floor-contributing pane is never below floor.
pub fn below_floor(registry: &Registry, panes: &[PaneGeometry]) -> bool {
    effective_floor(registry, panes).is_some_and(|floor| !floor_violations(panes, floor).is_empty())
}

/// Measure the window that holds `pane_id` and give its floor verdict, reading every
/// pane's registration from tmux.
pub fn inspect(pane_id: &str, registry: &Registry) -> Result<WindowReport> {
    Ok(measure(pane_id)?.report(registry))
}

/// Make the new pane's floor inputs the ones placement was given rather than what tmux
/// reads back: its agent and resolved surface for `spawn-agent`, none for `launch`.
fn apply_new_pane_registration(
    panes: &mut [PaneGeometry],
    pane_id: &str,
    agent: Option<&AgentRegistration<'_>>,
) {
    for pane in panes.iter_mut().filter(|pane| pane.pane_id == pane_id) {
        pane.agent = agent.map(|agent| agent.agent.to_owned());
        pane.surface = agent.map(|agent| agent.surface.to_owned());
    }
}

/// Place a new pane running `command`: resolve the destination, create the pane
/// there, write `registration` on it, then measure the window it landed in and give
/// its floor verdict against `registry`.
pub fn place(
    command: &str,
    destination: &Destination<'_>,
    registration: &Registration<'_>,
    registry: &Registry,
) -> Result<Landing> {
    let in_tmux = env::var_os("TMUX").is_some();
    let layout = resolve_layout(
        destination.split,
        destination.size,
        destination.target.is_some(),
        destination.session.is_some(),
        destination.window.is_some(),
        in_tmux,
    );
    let (split_arg, size_arg) = layout.split_args();
    let launch_target = resolve_launch_target(
        destination.target,
        destination.session,
        destination.window,
        matches!(layout, Layout::Split { .. }),
    )?;

    let pane_id = create_pane(
        command,
        split_arg,
        size_arg,
        launch_target.as_ref(),
        in_tmux,
    )?;
    let name = register(&pane_id, registration)?;
    let mut geometry = measure(&pane_id)?;
    apply_new_pane_registration(&mut geometry.panes, &pane_id, registration.agent.as_ref());
    let report = geometry.report(registry);

    Ok(Landing {
        pane_id,
        name,
        window_id: report.window_id,
        min_pane_cols: report.min_pane_cols,
        min_pane_rows: report.min_pane_rows,
        below_floor: report.below_floor,
    })
}

/// Write the whole registration on `pane_id` and return the `@tt-name` written.
fn register(pane_id: &str, registration: &Registration<'_>) -> Result<String> {
    if let Some(agent) = &registration.agent {
        names::set(pane_id, names::KEY_AGENT, agent.agent)?;
        // Record the resolved surface unconditionally, whether or not one was named: an
        // absent record can then only mean a pane created before surfaces existed.
        names::set(pane_id, names::KEY_SURFACE, agent.surface)?;
        // Caller-supplied trailing arguments can change the rendering out from under the
        // recorded surface. Arguments the surface supplies itself are validated by
        // construction and do not set the mark.
        if agent.surface_unvalidated {
            names::set(pane_id, names::KEY_SURFACE_UNVALIDATED, "1")?;
        }
        if let Some(access) = agent.access {
            names::set(pane_id, names::KEY_ACCESS, access)?;
        }
    }
    let name = names::set_name_or_derive(pane_id, registration.name, registration.name_source)?;
    names::set(pane_id, names::KEY_LAUNCHED_AT, registration.launched_at)?;
    if let Some(cwd) = registration.cwd {
        names::set(pane_id, names::KEY_CWD, cwd)?;
    }
    Ok(name)
}

/// Separates the fields of a measurement line. Registration values are user strings
/// and may hold tabs; the unit separator does not occur in them.
const FIELD_SEP: char = '\x1f';

/// Every pane of the window `pane_id` is in, with its size and registration, as tmux
/// reports them now.
fn measure(pane_id: &str) -> Result<WindowGeometry> {
    let format = [
        "#{window_id}".to_owned(),
        "#{pane_id}".to_owned(),
        "#{pane_width}".to_owned(),
        "#{pane_height}".to_owned(),
        format!("#{{{}}}", names::KEY_AGENT),
        format!("#{{{}}}", names::KEY_SURFACE),
    ]
    .join(&FIELD_SEP.to_string());
    // `list-panes -t <pane>` lists the panes of that pane's window.
    let output = tmux::run_checked(&["list-panes", "-t", pane_id, "-F", &format])
        .with_context(|| format!("cannot measure the window of pane {pane_id}"))?;
    parse_window_geometry(&output)
        .with_context(|| format!("cannot measure the window of pane {pane_id}"))
}

/// Parse measurement lines — window id, pane id, width, height, `@tt-agent` and
/// `@tt-surface`, separated by [`FIELD_SEP`] — one per pane of one window.
fn parse_window_geometry(output: &str) -> Result<WindowGeometry> {
    let mut window_id: Option<&str> = None;
    let mut panes = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.trim_end_matches('\r').split(FIELD_SEP).collect();
        let [window, pane, cols, rows, agent, surface] = fields[..] else {
            return Err(anyhow!("unexpected tmux pane geometry line {line:?}"));
        };
        if window.is_empty() {
            return Err(anyhow!("tmux reported no window id in {line:?}"));
        }
        if pane.is_empty() {
            return Err(anyhow!("tmux reported no pane id in {line:?}"));
        }
        match window_id {
            None => window_id = Some(window),
            Some(seen) if seen != window => {
                return Err(anyhow!(
                    "tmux listed panes of two windows ({seen}, {window})"
                ))
            }
            Some(_) => {}
        }
        let cols: u32 = cols
            .parse()
            .map_err(|_| anyhow!("unexpected pane width {cols:?}"))?;
        let rows: u32 = rows
            .parse()
            .map_err(|_| anyhow!("unexpected pane height {rows:?}"))?;
        panes.push(PaneGeometry {
            pane_id: pane.to_owned(),
            cols,
            rows,
            agent: non_empty(agent),
            surface: non_empty(surface),
        });
    }
    let window_id = window_id.ok_or_else(|| anyhow!("tmux listed no panes"))?;
    Ok(WindowGeometry {
        window_id: window_id.to_owned(),
        panes,
    })
}

/// Resolved tmux target for pane creation.
#[derive(Clone, Debug, Eq, PartialEq)]
enum LaunchTarget {
    /// Pane id to split (used when `split` is also supplied).
    SplitPane(String),
    /// The destination session id for new-window placement, or the
    /// `session[:window]` scope to split when a split direction is also supplied.
    WindowScope(String),
}

/// Resolve the caller's `--target`/`--session`/`--window` flags into a
/// [`LaunchTarget`].
///
/// `target::resolve` always yields a pane id (`%N`). For split that's
/// exactly what we want; for new-window we widen the scope to the
/// containing session's id so tmux creates the window in the right place.
fn resolve_launch_target(
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
        let session_id = destination_session_id(&pane_id, &format!("target pane {pane_id}"))?;
        return Ok(Some(LaunchTarget::WindowScope(session_id)));
    }

    if session.is_some() || window.is_some() {
        if split {
            return Ok(Some(LaunchTarget::WindowScope(target::scoped_target(
                session, window,
            ))));
        }
        // A new window is placed in the scoped session; `--window` names no index for it.
        let (scope, source) = match session {
            Some(session) => (format!("{session}:"), format!("session {session:?}")),
            None => (
                target::managed_session_pane_target(),
                format!("managed session {:?}", target::MANAGED_SESSION),
            ),
        };
        let session_id = destination_session_id(&scope, &source)?;
        return Ok(Some(LaunchTarget::WindowScope(session_id)));
    }

    Ok(None)
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
    let output = tmux::run(&["display-message", "-p", "-t", pane_target, "#{session_id}"])
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

/// Create the pane and return its id. Every destination path of the spawning verbs
/// goes through here.
fn create_pane(
    cmd: &str,
    split: Option<&str>,
    size: Option<u32>,
    target: Option<&LaunchTarget>,
    in_tmux: bool,
) -> Result<String> {
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
            if in_tmux {
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
                    let source = format!("calling pane {calling}");
                    new_window_args(cmd, Some(&destination_session_id(&calling, &source)?))
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

/// The history depth the spawning verbs give a session they create, so `prompt`
/// can read a completed turn back out of scrollback even after it scrolls off screen.
/// tmux's default is 2000. Cost is roughly 70 bytes per stored line (see README).
pub const DEFAULT_HISTORY_LIMIT: u32 = 50_000;

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

#[cfg(test)]
mod tests {
    use super::*;

    const H70: Layout = Layout::Split {
        direction: "-h",
        size: Some(70),
    };

    #[test]
    fn flagless_inside_tmux_is_the_seventy_percent_horizontal_split() {
        assert_eq!(resolve_layout(None, None, false, false, false, true), H70);
        assert_eq!(resolve_layout(None, None, true, false, false, true), H70);
        // `--target` with a scope still splits the resolved pane.
        assert_eq!(resolve_layout(None, None, true, true, true, true), H70);
    }

    #[test]
    fn flagless_size_overrides_the_seventy_percent_default() {
        assert_eq!(
            resolve_layout(None, Some(40), false, false, false, true),
            Layout::Split {
                direction: "-h",
                size: Some(40)
            }
        );
    }

    #[test]
    fn flagless_outside_tmux_or_with_a_scope_is_a_new_window() {
        assert_eq!(
            resolve_layout(None, None, false, false, false, false),
            Layout::NewWindow
        );
        assert_eq!(
            resolve_layout(None, None, false, true, false, true),
            Layout::NewWindow
        );
        assert_eq!(
            resolve_layout(None, None, false, false, true, true),
            Layout::NewWindow
        );
    }

    #[test]
    fn explicit_split_flags_resolve_regardless_of_context() {
        for in_tmux in [true, false] {
            assert_eq!(
                resolve_layout(Some(Split::H), None, false, true, false, in_tmux),
                Layout::Split {
                    direction: "-h",
                    size: None
                }
            );
            assert_eq!(
                resolve_layout(Some(Split::V), Some(30), false, false, false, in_tmux),
                Layout::Split {
                    direction: "-v",
                    size: Some(30)
                }
            );
            assert_eq!(
                resolve_layout(Some(Split::Window), Some(30), true, false, false, in_tmux),
                Layout::NewWindow
            );
        }
    }

    /// One `list-panes` geometry line in the measurement format.
    fn line(window: &str, pane: &str, cols: u32, rows: u32, agent: &str, surface: &str) -> String {
        [
            window.to_owned(),
            pane.to_owned(),
            cols.to_string(),
            rows.to_string(),
            agent.to_owned(),
            surface.to_owned(),
        ]
        .join(&FIELD_SEP.to_string())
            + "\n"
    }

    #[test]
    fn window_geometry_is_the_componentwise_minimum() {
        let output = [
            line("@3", "%1", 83, 71, "", ""),
            line("@3", "%2", 196, 71, "codex", "default"),
            line("@3", "%3", 280, 10, "", ""),
        ]
        .concat();
        let geometry = parse_window_geometry(&output).unwrap();
        assert_eq!(geometry.window_id, "@3");
        assert_eq!(
            (geometry.min_pane_cols(), geometry.min_pane_rows()),
            (83, 10)
        );
        assert_eq!(geometry.panes.len(), 3);
        assert_eq!(geometry.panes[1].agent.as_deref(), Some("codex"));
        assert_eq!(geometry.panes[1].surface.as_deref(), Some("default"));
        // An unset user option reads back empty and is no registration.
        assert_eq!(geometry.panes[0].agent, None);
        assert_eq!(geometry.panes[0].surface, None);
    }

    #[test]
    fn window_geometry_of_a_single_pane_is_that_pane() {
        let geometry = parse_window_geometry(&line("@0", "%0", 280, 82, "", "")).unwrap();
        assert_eq!(geometry.window_id, "@0");
        assert_eq!(
            (geometry.min_pane_cols(), geometry.min_pane_rows()),
            (280, 82)
        );
    }

    #[test]
    fn window_geometry_keeps_tabs_inside_registration_values() {
        let geometry = parse_window_geometry(&line("@0", "%0", 280, 82, "odd\tagent", "")).unwrap();
        assert_eq!(geometry.panes[0].agent.as_deref(), Some("odd\tagent"));
    }

    #[test]
    fn window_geometry_rejects_empty_and_malformed_output() {
        let sep = FIELD_SEP.to_string();
        let join = |fields: &[&str]| fields.join(&sep) + "\n";
        assert!(parse_window_geometry("").is_err());
        assert!(parse_window_geometry("\n").is_err());
        assert!(parse_window_geometry(&join(&["@0", "%0", "280", "82", ""])).is_err());
        assert!(parse_window_geometry(&join(&["@0", "%0", "wide", "82", "", ""])).is_err());
        assert!(parse_window_geometry(&join(&["", "%0", "280", "82", "", ""])).is_err());
        assert!(parse_window_geometry(&join(&["@0", "", "280", "82", "", ""])).is_err());
        assert!(parse_window_geometry(&join(&["@0", "%0", "280", "82", "", "", "extra"])).is_err());
    }

    #[test]
    fn window_geometry_rejects_panes_of_two_windows() {
        let output = [
            line("@0", "%0", 280, 82, "", ""),
            line("@1", "%1", 100, 20, "", ""),
        ]
        .concat();
        assert!(parse_window_geometry(&output).is_err());
    }

    // -- Floors (ISSUE-260801-1310-05) -------------------------------------------------

    fn floor(cols: u32, rows: u32) -> Floor {
        Floor { cols, rows }
    }

    /// The registry an `agents.toml` with `contents` produces over the built-ins.
    fn registry_from(contents: &str) -> Registry {
        let (registry, _warnings) = Registry::from_user_toml(contents).unwrap();
        registry
    }

    fn pane(
        id: &str,
        cols: u32,
        rows: u32,
        agent: Option<&str>,
        surface: Option<&str>,
    ) -> PaneGeometry {
        PaneGeometry {
            pane_id: id.to_owned(),
            cols,
            rows,
            agent: agent.map(str::to_owned),
            surface: surface.map(str::to_owned),
        }
    }

    #[test]
    fn surface_floor_declared_in_agents_toml_is_the_resolved_floor() {
        let registry = registry_from(
            r#"
[codex.surfaces.default]
min_cols = 70
min_rows = 12

[demo]
binary = "/bin/demo"

[demo.surfaces.main]
min_cols = 40
min_rows = 30
"#,
        );
        assert_eq!(
            pane_floor(&registry, Some("codex"), Some("default")),
            Some(floor(70, 12))
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("main")),
            Some(floor(40, 30))
        );
    }

    #[test]
    fn one_axis_override_of_a_seeded_surface_keeps_the_shipped_other_axis() {
        let cols_only = registry_from("[codex.surfaces.default]\nmin_cols = 50\n");
        assert_eq!(
            pane_floor(&cols_only, Some("codex"), Some("default")),
            Some(floor(50, 9)),
            "only min_cols was set, so codex keeps its shipped 9 rows, not the default 16"
        );

        let rows_only = registry_from("[codex.surfaces.default]\nmin_rows = 20\n");
        assert_eq!(
            pane_floor(&rows_only, Some("codex"), Some("default")),
            Some(floor(58, 20)),
            "only min_rows was set, so codex keeps its shipped 58 columns"
        );
    }

    #[test]
    fn unseeded_surface_declaring_one_axis_takes_the_global_default_for_the_other() {
        let registry = registry_from(
            r#"
[demo]
binary = "/bin/demo"

[demo.surfaces.wide]
min_cols = 40

[demo.surfaces.tall]
min_rows = 30
"#,
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("wide")),
            Some(floor(40, DEFAULT_FLOOR.rows))
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("tall")),
            Some(floor(DEFAULT_FLOOR.cols, 30))
        );
    }

    #[test]
    fn agent_level_floor_binds_to_the_pre_surface_rendering_only() {
        // A new agent whose (validated) default `flat` differs from its pre-surface `rich`.
        let registry = registry_from(
            r#"
[demo]
binary = "/bin/demo"
min_cols = 40
min_rows = 20
default_surface = "flat"
pre_surface_rendering = "rich"

[demo.surfaces.rich]
args = ["--rich"]

[demo.surfaces.flat]
args = ["--flat"]
ready_regex = "^ready$"
busy_regex = "^busy$"
validated_version = "demo 1.0"
"#,
        );
        let demo = registry.get("demo").unwrap();
        assert_eq!(demo.default_surface().unwrap().name, "flat");
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("rich")),
            Some(floor(40, 20))
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("flat")),
            Some(DEFAULT_FLOOR),
            "the default surface's floor must not move"
        );

        // A built-in merged over: `claude` names `flat` as its default, and the agent-level
        // floor still lands on `rich`, the pre-surface rendering.
        let claude = registry_from(
            r#"
[claude]
min_cols = 30
default_surface = "flat"
"#,
        );
        assert_eq!(
            pane_floor(&claude, Some("claude"), Some("rich")),
            Some(floor(30, 16))
        );
        assert_eq!(
            pane_floor(&claude, Some("claude"), Some("flat")),
            Some(DEFAULT_FLOOR)
        );
    }

    #[test]
    fn new_agent_without_surfaces_binds_an_agent_level_floor_to_its_one_surface() {
        let registry = registry_from(
            r#"
[demo]
binary = "/bin/demo"
min_cols = 44
"#,
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), None),
            Some(floor(44, DEFAULT_FLOOR.rows))
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), Some("default")),
            Some(floor(44, DEFAULT_FLOOR.rows))
        );
    }

    #[test]
    fn pane_without_surface_record_takes_the_pre_surface_floor_not_the_default() {
        let registry = registry_from(
            r#"
[demo]
binary = "/bin/demo"
default_surface = "flat"
pre_surface_rendering = "rich"

[demo.surfaces.rich]
min_cols = 30
min_rows = 20

[demo.surfaces.flat]
ready_regex = "^ready$"
busy_regex = "^busy$"
validated_version = "demo 1.0"
min_cols = 70
min_rows = 10
"#,
        );
        assert_eq!(
            registry
                .get("demo")
                .unwrap()
                .default_surface()
                .unwrap()
                .name,
            "flat"
        );
        assert_eq!(
            pane_floor(&registry, Some("demo"), None),
            Some(floor(30, 20))
        );
    }

    #[test]
    fn claude_floors_resolve_per_recorded_surface() {
        let registry = Registry::builtin();
        assert_eq!(
            pane_floor(&registry, Some("claude"), Some("flat")),
            Some(floor(58, 16))
        );
        assert_eq!(
            pane_floor(&registry, Some("claude"), Some("rich")),
            Some(floor(26, 16))
        );
        assert_eq!(
            pane_floor(&registry, Some("claude"), None),
            Some(floor(26, 16)),
            "no @tt-surface resolves to the rich pre-surface rendering"
        );
    }

    #[test]
    fn unknown_agent_or_surface_takes_the_default_pair() {
        let registry = Registry::builtin();
        assert_eq!(
            pane_floor(&registry, Some("nope"), None),
            Some(DEFAULT_FLOOR)
        );
        assert_eq!(
            pane_floor(&registry, Some("nope"), Some("rich")),
            Some(DEFAULT_FLOOR)
        );
        assert_eq!(
            pane_floor(&registry, Some("codex"), Some("gone")),
            Some(DEFAULT_FLOOR)
        );
        assert_eq!(DEFAULT_FLOOR, floor(58, 16));
    }

    #[test]
    fn a_pane_without_an_agent_contributes_no_floor() {
        let registry = Registry::builtin();
        assert_eq!(pane_floor(&registry, None, None), None);
        assert_eq!(pane_floor(&registry, None, Some("rich")), None);
    }

    #[test]
    fn builtin_floors_are_the_measured_seeds() {
        let registry = Registry::builtin();
        assert_eq!(
            pane_floor(&registry, Some("codex"), Some("default")),
            Some(floor(58, 9))
        );
        assert_eq!(
            pane_floor(&registry, Some("cursor"), Some("default")),
            Some(floor(36, 9))
        );
        assert_eq!(
            pane_floor(&registry, Some("claude"), Some("rich")),
            Some(floor(26, 16))
        );
        assert_eq!(
            pane_floor(&registry, Some("claude"), Some("flat")),
            Some(floor(58, 16))
        );
        assert_eq!(
            pane_floor(&registry, Some("agy"), Some("default")),
            Some(DEFAULT_FLOOR)
        );
    }

    #[test]
    fn effective_floor_is_the_componentwise_maximum_of_agent_panes_only() {
        let registry = Registry::builtin();
        let panes = [
            // codex 58x9 and cursor 36x9 and claude rich 26x16 → 58x16.
            pane("%1", 100, 40, Some("codex"), Some("default")),
            pane("%2", 100, 40, Some("cursor"), Some("default")),
            pane("%3", 100, 40, Some("claude"), Some("rich")),
            // An agentless pane contributes nothing, however small it is.
            pane("%4", 5, 3, None, None),
        ];
        assert_eq!(effective_floor(&registry, &panes), Some(floor(58, 16)));

        let narrow = [
            pane("%1", 100, 40, Some("cursor"), Some("default")),
            pane("%2", 100, 40, None, None),
        ];
        assert_eq!(effective_floor(&registry, &narrow), Some(floor(36, 9)));
    }

    #[test]
    fn an_agentless_pane_below_the_effective_floor_puts_the_window_below_floor() {
        let registry = Registry::builtin();
        let panes = [
            pane("%1", 120, 40, Some("cursor"), Some("default")),
            // The window's smallest pane has no agent and is narrower than cursor's 36.
            pane("%2", 30, 40, None, None),
        ];
        let effective = effective_floor(&registry, &panes).unwrap();
        let violations = floor_violations(&panes, effective);
        assert_eq!(
            violations
                .iter()
                .map(|p| p.pane_id.as_str())
                .collect::<Vec<_>>(),
            ["%2"]
        );
        assert!(below_floor(&registry, &panes));
    }

    #[test]
    fn floor_comparison_is_per_axis_and_strict() {
        let registry = Registry::builtin();
        // Exactly at the floor is not below it.
        let at = [pane("%1", 58, 9, Some("codex"), Some("default"))];
        assert!(!below_floor(&registry, &at));
        assert!(floor_violations(&at, floor(58, 9)).is_empty());
        // One axis short is below it.
        let short = [pane("%1", 200, 8, Some("codex"), Some("default"))];
        assert!(below_floor(&registry, &short));
        let narrow = [pane("%1", 57, 80, Some("codex"), Some("default"))];
        assert!(below_floor(&registry, &narrow));
    }

    #[test]
    fn a_window_without_agent_panes_is_never_below_floor() {
        let registry = Registry::builtin();
        let panes = [pane("%1", 10, 3, None, None), pane("%2", 20, 5, None, None)];
        assert_eq!(effective_floor(&registry, &panes), None);
        assert!(!below_floor(&registry, &panes));
    }

    #[test]
    fn the_new_panes_registration_overrides_what_tmux_reads_back() {
        let registry = Registry::builtin();
        // tmux reports the new pane with no agent options (for example, read before the
        // write is visible); placement's own inputs are authoritative for it.
        let mut panes = vec![
            pane("%1", 100, 40, None, None),
            pane("%2", 30, 40, None, None),
        ];
        assert!(!below_floor(&registry, &panes));
        apply_new_pane_registration(
            &mut panes,
            "%2",
            Some(&AgentRegistration {
                agent: "codex",
                surface: "default",
                access: None,
                surface_unvalidated: false,
            }),
        );
        assert_eq!(panes[1].agent.as_deref(), Some("codex"));
        assert_eq!(panes[1].surface.as_deref(), Some("default"));
        assert!(below_floor(&registry, &panes));

        // A `launch` pane carries no agent, whatever tmux reads back for it.
        let mut panes = vec![pane("%2", 30, 40, Some("codex"), Some("default"))];
        apply_new_pane_registration(&mut panes, "%2", None);
        assert_eq!(panes[0].agent, None);
        assert_eq!(panes[0].surface, None);
    }
}
