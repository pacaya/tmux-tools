//! Pane placement for the spawning verbs (`launch`, `spawn-agent`).
//!
//! This module is the single owner of placement. [`place`] resolves where a new
//! pane goes, snapshots the ownership of the window it will split, creates it (split
//! or new window), writes its whole registration, retiles the window when the
//! snapshot made it eligible (or clears [`TILED_MARKER`] for explicit sizing), and
//! then measures the window it landed in. When that tiled window is below its
//! effective floor, the new pane is relocated by trial (see [`place`]). It returns a
//! [`Landing`]: the window the pane finally landed in. When the new pane no longer
//! exists at one of those steps after the split, placement stops there and the
//! [`Landing`] carries no window. The verbs build the command and the registration;
//! they never create a pane themselves.
//! [`rebalance`] retiles a window and returns its measured geometry.
//!
//! It owns the window guard and kill-time reclamation: [`window_guard`] answers
//! whether the tool may rearrange a window (it carries [`TILED_MARKER`] and holds a
//! pane this invocation owns), and [`remove_pane`] destroys a pane and, when that
//! guard held before the destruction, retiles its window in the same tmux command
//! sequence.
//!
//! It also owns floors: [`pane_floor`] resolves a pane's floor from its registration
//! and the agent registry, [`effective_floor`] combines a window's, and
//! [`floor_violations`] / [`below_floor`] give the verdict. Those are pure over measured
//! geometry; [`place`] and [`inspect`] measure and apply them. The floor is advisory:
//! only the pane a tiled placement creates is relocated; nothing here refuses a pane
//! or repairs a window that is already below its floor.

use anyhow::{anyhow, Context, Result};
use std::env;

use crate::agents::Registry;
use crate::format::non_empty;
use crate::{names, target, tmux};

/// The `--split` flag of the spawning verbs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum Split {
    /// Tile (the default inside tmux): split, then retile the window with tmux's
    /// `tiled` layout when it already held a pane this tool owns. The first pane in a
    /// window not yet owned is a horizontal split giving it 70%. Cannot be combined
    /// with --size
    #[value(name = "tile")]
    Tile,
    /// Horizontal split of the target pane, 50% or --size N%; no retile
    #[value(name = "h")]
    H,
    /// Vertical split of the target pane, 50% or --size N%; no retile
    #[value(name = "v")]
    V,
    /// A new window instead of a split
    #[value(name = "window")]
    Window,
}

/// Resolved layout for a placement. Computed up-front so both spawning verbs apply
/// the same default rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    /// Split, then retile the window with tmux's `tiled` layout when the window held
    /// an owned pane before the split ([`place`] decides that from a snapshot). A
    /// window that did not is split horizontally with the new pane at
    /// [`FIRST_PANE_PERCENT`] and left as it is.
    Tile,
    /// Explicit sizing intent: `tmux split-window` with the given direction flag and
    /// optional size percentage for the new pane, no retile, and `@tt-tiled` cleared
    /// on the window.
    Split {
        direction: &'static str,
        size: Option<u32>,
    },
    /// Use `tmux new-window`.
    NewWindow,
}

/// The width a tile placement gives the new pane when the window it splits held no
/// owned pane: the first pane this tool puts into a window is placed as before tiling.
pub const FIRST_PANE_PERCENT: u32 = 70;

/// The `@tt-tiled` window option: present on a window this tool tiled, unset when a
/// placement with explicit sizing intent targets it. It records what the tool did; it
/// never gates placement.
pub const TILED_MARKER: &str = "@tt-tiled";

impl Layout {
    /// Whether this layout creates the pane by splitting a pane (as opposed to a new
    /// window).
    fn splits(self) -> bool {
        !matches!(self, Layout::NewWindow)
    }
}

/// Resolve the effective layout from the user's flags.
///
/// The implicit default inside tmux is [`Layout::Tile`]: split, then retile the
/// window with tmux's `tiled` layout when it already held a pane this tool owns. The
/// first pane put into a window not yet owned is the horizontal split giving the new
/// pane [`FIRST_PANE_PERCENT`]%, without a retile, so tiling engages from the second
/// pane on.
///
/// Explicit sizing intent opts out of tiling and is placed as a plain split:
/// `--split h|v` (tmux's native 50/50 unless `--size` is supplied), and `--size N`
/// alone, which is the horizontal split at N%. `--split window` is a new window.
/// `--split tile --size N` is an error: a tiled layout has no percentage to honour.
///
/// `--session`/`--window` without `--target` indicates window-scope intent
/// (there is no caller pane to split into), so the implicit default there is a new
/// window; an explicit `--split tile` splits into the scoped window, as `h|v` do.
///
/// Outside tmux (`in_tmux` false) there is no current pane to split, so the
/// implicit default is [`Layout::NewWindow`]. (An explicit `--split tile|h|v`
/// without an explicit `--target` is absorbed by placement, which falls back to
/// creating a new window in the managed session — preserving long-standing behavior.)
pub fn resolve_layout(
    split: Option<Split>,
    size: Option<u32>,
    has_target: bool,
    has_session: bool,
    has_window: bool,
    in_tmux: bool,
) -> Result<Layout> {
    Ok(match split {
        Some(Split::Tile) => {
            if let Some(size) = size {
                return Err(anyhow!(
                    "--split tile cannot be combined with --size {size}: a tiled layout has \
                     no percentage to honour; pass --split h or --split v with --size, or \
                     drop --size"
                ));
            }
            Layout::Tile
        }
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
            if !in_tmux || window_scope_implied {
                Layout::NewWindow
            } else if size.is_some() {
                Layout::Split {
                    direction: "-h",
                    size,
                }
            } else {
                Layout::Tile
            }
        }
    })
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
    /// The `@tt-name` chosen for the pane: the `--name` value, else the derived name.
    /// It is the name written, unless the pane was gone before registration wrote it.
    pub name: String,
    /// The window the pane finally landed in, measured, with its floor verdict. `None`
    /// when the pane no longer existed at a placement step after the split
    /// (registration, retile, marker, measurement, or a relocation step), so the window
    /// was not measured.
    pub window: Option<WindowReport>,
    /// Whether a floor breach moved the pane out of the window it was split into: a
    /// `join-pane` or `break-pane` of it succeeded (and, with a window, it landed in a
    /// window other than that one).
    pub relocated: bool,
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

    /// The window's measured minima and its floor verdict, with each pane's floor
    /// resolved from its `@tt-agent` and `@tt-surface` through `registry`.
    pub fn report(&self, registry: &Registry) -> WindowReport {
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

/// Place a new pane running `command`: resolve the layout and destination, create the
/// pane there, write `registration` on it, apply the layout step, then measure the
/// window it landed in and give its floor verdict against `registry`.
///
/// The layout step after a split depends on the resolved [`Layout`]:
/// - [`Layout::Tile`]: before splitting, the target window's panes are snapshotted and
///   the window is eligible when the snapshot holds a pane [`names::pane_is_owned`]
///   reports as ours. An eligible window is split, then retiled with tmux's `tiled`
///   layout and marked with [`TILED_MARKER`]. A window that is not eligible gets the
///   [`FIRST_PANE_PERCENT`]% horizontal split, no retile, and no marker change.
/// - [`Layout::Split`] (explicit sizing intent): the split as asked, no retile, and
///   [`TILED_MARKER`] unset on the window.
///
/// A pane created in a new window gets no layout step: the window holds only it.
///
/// When the window was retiled ([`Layout::Tile`] into an eligible window) and the
/// measurement shows it below its effective floor, the new pane is relocated within
/// the session of that window (the origin); see [`relocate`]. Every other placement —
/// explicit sizing, a window not owned before the split, a new window — stays where it
/// was created, whatever its verdict.
///
/// The pane's name is chosen before anything is written on it. When a tmux call at a
/// step after the split (registration, retile, marker, measurement, or a relocation
/// step) exits non-zero and the new pane no longer exists on the server, the
/// remaining steps are skipped and the [`Landing`] carries that name and no window.
/// Every other failure at those steps — the pane still exists, the call could not run,
/// or its output cannot be parsed — is an error, except the refused `join-pane` and the
/// failed `break-pane` that [`relocate`] tolerates.
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
    )?;
    let launch_target = resolve_launch_target(
        destination.target,
        destination.session,
        destination.window,
        layout.splits(),
    )?;
    let creation = resolve_creation(launch_target, layout.splits(), in_tmux)?;

    // Eligibility comes from the window as it stood before this split, so placing a
    // pane never grants the authority to retile the window it lands in.
    let tile = match (layout, &creation) {
        (Layout::Tile, Creation::Split(target)) => window_has_owned_pane(target, registration.cwd)?,
        _ => false,
    };

    let tmux_args = match &creation {
        Creation::Split(target) => {
            let (direction, size) = match layout {
                Layout::Split { direction, size } => (direction, size),
                Layout::Tile if tile => ("-h", None),
                Layout::Tile | Layout::NewWindow => ("-h", Some(FIRST_PANE_PERCENT)),
            };
            split_window_args(command, direction, size, Some(target))
        }
        Creation::NewWindow(session_id) => new_window_args(command, Some(session_id)),
    };
    let pane_id = tmux::run_checked_owned(&tmux_args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux launch command returned an empty pane id"));
    }

    // Chosen before any write, so the landing reports it even when the pane is gone
    // before registration writes it.
    let name = names::choose_name(&pane_id, registration.name, registration.name_source)?;
    let window = measured_window(&pane_id, &name, registration, &creation, layout, tile)?;
    let window = window.map(|mut geometry| {
        apply_new_pane_registration(&mut geometry.panes, &pane_id, registration.agent.as_ref());
        geometry.report(registry)
    });

    // Only a placement that tiled its window is relocated: explicit sizing and a window
    // that was not owned before the split keep the pane (`tile` is false for both).
    let (window, relocated) = match window {
        Some(report) if tile && report.below_floor => {
            let relocation = relocate(&pane_id, &report.window_id, registration, registry)?;
            (relocation.window, relocation.relocated)
        }
        window => (window, false),
    };

    Ok(Landing {
        pane_id,
        name,
        window,
        relocated,
    })
}

/// Where [`relocate`] left the new pane.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Relocation {
    /// The window the pane finally landed in, measured; `None` when the pane was gone.
    window: Option<WindowReport>,
    /// Whether a `join-pane` or `break-pane` of the pane succeeded.
    relocated: bool,
}

/// Move the new pane `pane_id` out of `origin`, the window it was split into and that
/// it pushed below its effective floor, and retile `origin` after it leaves.
///
/// Candidates are the other windows of the origin's session that pass the window guard
/// ([`overflow_candidates`]), judged before any pane is joined, in ascending window
/// index. Each is tried: `join-pane -d` the pane into it, retile it, and measure it
/// against its effective floor including the new pane. The first that is not below its
/// floor keeps the pane. One that is moves the pane on and is retiled again once the
/// pane has left it. A refused `join-pane` while the pane still exists is a rejected
/// trial: nothing moved, and the search goes on.
///
/// When no candidate keeps the pane, `break-pane -d` moves it into a new window of the
/// origin's session, which is tiled and marked [`TILED_MARKER`]. A failed `break-pane`
/// while the pane still exists leaves it where it is (the origin, or the last rejected
/// candidate); that window is retiled and reported.
///
/// No step selects a window. Pane options travel with the pane, so nothing is
/// re-registered. When a step fails and the pane is gone, the remaining steps are
/// skipped and the relocation carries no window; any other failure is an error.
fn relocate(
    pane_id: &str,
    origin: &str,
    registration: &Registration<'_>,
    registry: &Registry,
) -> Result<Relocation> {
    let gone = |relocated| {
        Ok(Relocation {
            window: None,
            relocated,
        })
    };
    // Measure the window the pane is in now; no window when the pane is gone.
    let landed = |relocated| -> Result<Relocation> {
        Ok(Relocation {
            window: landing_report(pane_id, registration.agent.as_ref(), registry)?,
            relocated,
        })
    };

    let Some(session) = unless_pane_gone(pane_id, session_of(pane_id))? else {
        return gone(false);
    };
    let windows = session_windows(&session)?;
    let candidates = overflow_candidates(&windows, origin, registration.cwd);

    // Whether a move succeeded, and the window the pane is in.
    let mut moved = false;
    let mut current = origin.to_owned();
    for candidate in candidates {
        match move_step(pane_id, join_pane(pane_id, candidate))? {
            MoveStep::Moved => {}
            MoveStep::PaneGone => return gone(moved),
            MoveStep::Refused => continue,
        }
        moved = true;
        let left = std::mem::replace(&mut current, candidate.to_owned());
        if unless_pane_gone(pane_id, retile(pane_id))?.is_none()
            || unless_pane_gone(pane_id, retile(&left))?.is_none()
        {
            return gone(moved);
        }
        let trial = landed(moved)?;
        match &trial.window {
            Some(report) if report.below_floor => {}
            _ => return Ok(trial),
        }
    }

    match move_step(pane_id, break_pane(pane_id, &session))? {
        MoveStep::Moved => {
            moved = true;
            if unless_pane_gone(pane_id, retile(pane_id))?.is_none()
                || unless_pane_gone(pane_id, set_tiled_marker(pane_id))?.is_none()
                || unless_pane_gone(pane_id, retile(&current))?.is_none()
            {
                return gone(moved);
            }
        }
        MoveStep::PaneGone => return gone(moved),
        MoveStep::Refused => {
            if unless_pane_gone(pane_id, retile(pane_id))?.is_none() {
                return gone(moved);
            }
        }
    }
    landed(moved)
}

/// Measure the window `pane_id` (the new pane) is in and give its floor verdict, with
/// the new pane's floor taken from `agent`. `None` when the pane is gone.
fn landing_report(
    pane_id: &str,
    agent: Option<&AgentRegistration<'_>>,
    registry: &Registry,
) -> Result<Option<WindowReport>> {
    let Some(output) = unless_pane_gone(pane_id, read_measurement(pane_id))? else {
        return Ok(None);
    };
    let mut geometry = parse_measurement(pane_id, &output)?;
    apply_new_pane_registration(&mut geometry.panes, pane_id, agent);
    Ok(Some(geometry.report(registry)))
}

/// The outcome of a `join-pane` or `break-pane` of the new pane.
enum MoveStep {
    Moved,
    /// The call exited non-zero and the pane is no longer on the server.
    PaneGone,
    /// The call exited non-zero and the pane still exists: it did not move.
    Refused,
}

/// Classify a move of `pane_id` as [`unless_pane_gone`] classifies a step, except that
/// a failed call while the pane still exists is [`MoveStep::Refused`] rather than an
/// error. A failure that is not a tmux exit, or whose pane existence cannot be read,
/// is the move's own error.
fn move_step(pane_id: &str, step: Result<()>) -> Result<MoveStep> {
    match step {
        Ok(()) => Ok(MoveStep::Moved),
        Err(error) if error.downcast_ref::<tmux::CommandFailed>().is_some() => {
            match pane_exists(pane_id) {
                Ok(true) => Ok(MoveStep::Refused),
                Ok(false) => Ok(MoveStep::PaneGone),
                Err(_) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

/// The `#{session_id}` of the session `target` is in.
fn session_of(target: &str) -> Result<String> {
    let session = tmux::run_checked(&["display-message", "-p", "-t", target, "#{session_id}"])
        .with_context(|| format!("cannot read the session of {target}"))?
        .trim()
        .to_owned();
    if session.is_empty() {
        return Err(anyhow!("tmux reported no session id for {target}"));
    }
    Ok(session)
}

/// What the window guard reads about every window of `session` (a session id), each by
/// [`read_window_state`].
fn session_windows(session: &str) -> Result<Vec<WindowState>> {
    let output = tmux::run_checked(&["list-windows", "-t", session, "-F", "#{window_id}"])
        .with_context(|| format!("cannot list the windows of session {session}"))?;
    output
        .lines()
        .map(str::trim)
        .filter(|window_id| !window_id.is_empty())
        .map(read_window_state)
        .collect()
}

/// The overflow candidates among `windows` (one session's): every window other than
/// `origin` on which the window guard holds against `current_cwd` — it carries
/// [`TILED_MARKER`] and holds a pane this invocation owns — in ascending window index.
fn overflow_candidates<'a>(
    windows: &'a [WindowState],
    origin: &str,
    current_cwd: Option<&str>,
) -> Vec<&'a str> {
    let mut candidates: Vec<&WindowState> = windows
        .iter()
        .filter(|window| window.window_id != origin && window.guard(current_cwd).holds())
        .collect();
    candidates.sort_by_key(|window| window.index);
    candidates
        .into_iter()
        .map(|window| window.window_id.as_str())
        .collect()
}

/// `join-pane -d` `pane_id` into `window_id`, splitting its active pane; the window is
/// not selected.
fn join_pane(pane_id: &str, window_id: &str) -> Result<()> {
    tmux::run_checked(&["join-pane", "-d", "-s", pane_id, "-t", window_id])
        .with_context(|| format!("cannot join pane {pane_id} into window {window_id}"))?;
    Ok(())
}

/// `break-pane -d` `pane_id` into a new window of `session` (a session id); the window
/// is not selected.
fn break_pane(pane_id: &str, session: &str) -> Result<()> {
    let destination = format!("{session}:");
    tmux::run_checked(&["break-pane", "-d", "-s", pane_id, "-t", &destination])
        .with_context(|| format!("cannot break pane {pane_id} into a new window of {session}"))?;
    Ok(())
}

/// The placement steps after the split: write the registration on `pane_id`, apply
/// the layout step, and measure the window it landed in. `None` when a step found the
/// pane gone (see [`unless_pane_gone`]); the remaining steps are then skipped.
fn measured_window(
    pane_id: &str,
    name: &str,
    registration: &Registration<'_>,
    creation: &Creation,
    layout: Layout,
    tile: bool,
) -> Result<Option<WindowGeometry>> {
    if unless_pane_gone(pane_id, register(pane_id, name, registration))?.is_none() {
        return Ok(None);
    }
    if matches!(creation, Creation::Split(_)) {
        let layout_step = match layout {
            Layout::Tile if tile => {
                if unless_pane_gone(pane_id, retile(pane_id))?.is_none() {
                    return Ok(None);
                }
                set_tiled_marker(pane_id)
            }
            Layout::Split { .. } => clear_tiled_marker(pane_id),
            Layout::Tile | Layout::NewWindow => Ok(()),
        };
        if unless_pane_gone(pane_id, layout_step)?.is_none() {
            return Ok(None);
        }
    }
    let Some(output) = unless_pane_gone(pane_id, read_measurement(pane_id))? else {
        return Ok(None);
    };
    parse_measurement(pane_id, &output).map(Some)
}

/// `Some` of a placement step's value; `None` when the step failed on a tmux call that
/// exited non-zero ([`tmux::CommandFailed`]) and `pane_id` is no longer on the server.
/// Any other failure — including one whose pane still exists, or whose existence
/// cannot be read — is the step's own error.
///
/// "Gone" is judged by the server's pane list rather than by tmux's error text, which
/// differs per command (`no such pane`, `no such window`, `can't find pane`).
fn unless_pane_gone<T>(pane_id: &str, step: Result<T>) -> Result<Option<T>> {
    match step {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.downcast_ref::<tmux::CommandFailed>().is_some() => {
            match pane_exists(pane_id) {
                Ok(false) => Ok(None),
                Ok(true) | Err(_) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

/// Whether the server lists `pane_id` among all its panes.
fn pane_exists(pane_id: &str) -> Result<bool> {
    let output = tmux::run_checked(&["list-panes", "-a", "-F", "#{pane_id}"])?;
    Ok(output.lines().any(|line| line.trim() == pane_id))
}

/// Write the whole registration on `pane_id`, with `name` (from
/// [`names::choose_name`]) as its `@tt-name`.
fn register(pane_id: &str, name: &str, registration: &Registration<'_>) -> Result<()> {
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
    names::set(pane_id, names::KEY_NAME, name)?;
    names::set(pane_id, names::KEY_LAUNCHED_AT, registration.launched_at)?;
    if let Some(cwd) = registration.cwd {
        names::set(pane_id, names::KEY_CWD, cwd)?;
    }
    Ok(())
}

/// Retile the window that `target` (a window or pane target) is in with tmux's
/// `tiled` layout, then measure it.
///
/// This applies the layout only: it neither checks ownership nor reads or writes
/// [`TILED_MARKER`]; callers apply their own guards.
pub fn rebalance(target: &str) -> Result<WindowGeometry> {
    retile(target)?;
    measure(target)
}

/// tmux's layout name for the tiled arrangement this tool applies.
const TILED_LAYOUT: &str = "tiled";

/// Apply tmux's `tiled` layout to the window that `target` is in.
fn retile(target: &str) -> Result<()> {
    tmux::run_checked(&["select-layout", "-t", target, TILED_LAYOUT])
        .with_context(|| format!("cannot retile the window of {target}"))?;
    Ok(())
}

/// The window guard's answer: whether the tool may rearrange a window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowGuard {
    /// The window carries [`TILED_MARKER`] and holds a pane [`names::pane_is_owned`]
    /// reports as ours.
    Holds,
    /// The window does not carry [`TILED_MARKER`]: the tool did not arrange it. This is
    /// the answer whenever the marker is absent, whatever the window's ownership.
    MarkerAbsent,
    /// The window carries [`TILED_MARKER`] but holds no pane this invocation owns.
    NoOwnedPane,
}

impl WindowGuard {
    /// Whether the guard holds.
    pub fn holds(self) -> bool {
        self == WindowGuard::Holds
    }
}

/// Evaluate the window guard on the window that `target` (a pane or window target)
/// is in: read its [`TILED_MARKER`] and apply [`names::pane_is_owned`], judged
/// against `current_cwd`, to every pane of the window.
pub fn window_guard(target: &str, current_cwd: Option<&str>) -> Result<WindowGuard> {
    Ok(read_window_state(target)?.guard(current_cwd))
}

/// What [`remove_pane`] did besides destroying the pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Removal {
    /// The window guard, evaluated on the pane's window before the pane was destroyed.
    pub guard: WindowGuard,
    /// Whether the window was retiled: the guard held and the pane was not the
    /// window's last.
    pub retiled: bool,
}

/// Destroy `pane_id` and, when the window guard holds on its window, retile the
/// panes that remain.
///
/// The guard is read before the destruction: pane options vanish with the pane, so a
/// pane holding the window's only ownership evidence still counts. The removal and
/// the retile reach tmux as one command sequence, so the retile completes even when
/// the destroyed pane is the caller's own and the calling process is hung up with it.
/// A pane that is its window's last takes the window with it; no retile is sent.
pub fn remove_pane(pane_id: &str, current_cwd: Option<&str>) -> Result<Removal> {
    let state = read_window_state(pane_id)?;
    let guard = state.guard(current_cwd);
    let retiled = guard.holds() && state.panes.len() > 1;
    tmux::run_checked_owned(&removal_args(pane_id, &state.window_id, retiled))?;
    Ok(Removal { guard, retiled })
}

/// The tmux arguments that destroy `pane_id` and, when `retile` is set, retile
/// `window_id` in the same command sequence.
fn removal_args(pane_id: &str, window_id: &str, retile: bool) -> Vec<String> {
    let mut args = vec!["kill-pane", "-t", pane_id];
    if retile {
        args.extend([";", "select-layout", "-t", window_id, TILED_LAYOUT]);
    }
    args.into_iter().map(str::to_owned).collect()
}

/// What the window guard reads about one window.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WindowState {
    window_id: String,
    /// The window's index in its session, the order of the overflow search.
    index: u32,
    /// Whether the window carries [`TILED_MARKER`].
    marked: bool,
    /// The registration of every pane in the window, as the ownership predicate reads it.
    panes: Vec<names::Registered>,
}

impl WindowState {
    fn guard(&self, current_cwd: Option<&str>) -> WindowGuard {
        guard_verdict(self.marked, &self.panes, current_cwd)
    }
}

/// The window guard over what was read: the marker first, then ownership.
fn guard_verdict(
    marked: bool,
    panes: &[names::Registered],
    current_cwd: Option<&str>,
) -> WindowGuard {
    if !marked {
        WindowGuard::MarkerAbsent
    } else if panes
        .iter()
        .any(|registered| names::pane_is_owned(registered, current_cwd))
    {
        WindowGuard::Holds
    } else {
        WindowGuard::NoOwnedPane
    }
}

/// Read the window that `target` is in: its id and index, then its marker and its
/// panes' registrations by that id.
fn read_window_state(target: &str) -> Result<WindowState> {
    let format = format!("#{{window_id}}{FIELD_SEP}#{{window_index}}");
    let output = tmux::run_checked(&["display-message", "-p", "-t", target, &format])
        .with_context(|| format!("cannot read the window of {target}"))?;
    let (window_id, index) = parse_window_identity(&output)
        .with_context(|| format!("cannot read the window of {target}"))?;
    let marked = tiled_marker_present(&window_id)?;
    let panes = read_ownership_snapshot(&window_id)?;
    Ok(WindowState {
        window_id,
        index,
        marked,
        panes,
    })
}

/// Parse a window's id and index, separated by [`FIELD_SEP`].
fn parse_window_identity(output: &str) -> Result<(String, u32)> {
    let line = output.trim_end_matches(['\r', '\n']);
    let Some((window_id, index)) = line.split_once(FIELD_SEP) else {
        return Err(anyhow!("unexpected tmux window line {line:?}"));
    };
    let window_id = window_id.trim();
    if window_id.is_empty() {
        return Err(anyhow!("tmux reported no window id in {line:?}"));
    }
    let index = index
        .trim()
        .parse()
        .map_err(|_| anyhow!("unexpected window index {index:?}"))?;
    Ok((window_id.to_owned(), index))
}

/// Whether the window that `target` is in carries [`TILED_MARKER`] itself (a global
/// value is not the window's record). Presence is the test, whatever the value.
fn tiled_marker_present(target: &str) -> Result<bool> {
    let output = tmux::run_checked(&["show-options", "-w", "-q", "-t", target, TILED_MARKER])
        .with_context(|| format!("cannot read the tiled marker of the window of {target}"))?;
    Ok(output
        .lines()
        .any(|line| line.split_whitespace().next() == Some(TILED_MARKER)))
}

/// Set [`TILED_MARKER`] on the window that `target` is in.
fn set_tiled_marker(target: &str) -> Result<()> {
    tmux::run_checked(&["set-option", "-w", "-t", target, TILED_MARKER, "1"])
        .with_context(|| format!("cannot mark the window of {target} tiled"))?;
    Ok(())
}

/// Unset [`TILED_MARKER`] on the window that `target` is in. Unsetting an absent
/// option succeeds, so "cleared" and "never set" are one state.
fn clear_tiled_marker(target: &str) -> Result<()> {
    tmux::run_checked(&["set-option", "-w", "-u", "-t", target, TILED_MARKER])
        .with_context(|| format!("cannot clear the tiled marker of the window of {target}"))?;
    Ok(())
}

/// Whether the window that `target` is in holds a pane [`names::pane_is_owned`]
/// reports as ours, judged against `current_cwd`.
fn window_has_owned_pane(target: &str, current_cwd: Option<&str>) -> Result<bool> {
    Ok(read_ownership_snapshot(target)?
        .iter()
        .any(|registered| names::pane_is_owned(registered, current_cwd)))
}

/// The registration of every pane in the window that `target` is in, as the
/// ownership predicate reads it.
fn read_ownership_snapshot(target: &str) -> Result<Vec<names::Registered>> {
    let format = [names::KEY_NAME, names::KEY_AGENT, names::KEY_CWD]
        .map(|key| format!("#{{{key}}}"))
        .join(&FIELD_SEP.to_string());
    let output = tmux::run_checked(&["list-panes", "-t", target, "-F", &format])
        .with_context(|| format!("cannot read the panes of the window of {target}"))?;
    parse_ownership_snapshot(&output)
}

/// Parse ownership snapshot lines — `@tt-name`, `@tt-agent` and `@tt-cwd`, separated
/// by [`FIELD_SEP`] — one per pane, into the registrations the ownership predicate
/// reads. Every other registration field is left unset.
fn parse_ownership_snapshot(output: &str) -> Result<Vec<names::Registered>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.trim_end_matches('\r').split(FIELD_SEP).collect();
            let [name, agent, cwd] = fields[..] else {
                return Err(anyhow!("unexpected tmux pane registration line {line:?}"));
            };
            // Trimmed as `names::read` trims, so both callers of the predicate see the
            // same registration.
            Ok(names::Registered {
                name: non_empty(name.trim()),
                agent: non_empty(agent.trim()),
                access: None,
                launched_at: None,
                cwd: non_empty(cwd.trim()),
                surface: None,
                surface_unvalidated: false,
            })
        })
        .collect()
}

/// Separates the fields of a measurement line. Registration values are user strings
/// and may hold tabs; the unit separator does not occur in them.
const FIELD_SEP: char = '\x1f';

/// Every pane of the window `pane_id` (a pane or window target) is in, with its size
/// and registration, as tmux reports them now.
fn measure(pane_id: &str) -> Result<WindowGeometry> {
    parse_measurement(pane_id, &read_measurement(pane_id)?)
}

/// The measurement's tmux call: the raw lines [`parse_measurement`] reads.
fn read_measurement(pane_id: &str) -> Result<String> {
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
    tmux::run_checked(&["list-panes", "-t", pane_id, "-F", &format])
        .with_context(|| format!("cannot measure the window of pane {pane_id}"))
}

/// Parse the output of [`read_measurement`] for `pane_id`.
fn parse_measurement(pane_id: &str, output: &str) -> Result<WindowGeometry> {
    parse_window_geometry(output)
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

/// How the pane is created: every destination path of the spawning verbs resolves to
/// one of these.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Creation {
    /// `split-window -t <target>`: a pane id or a `session[:window]` scope.
    Split(String),
    /// `new-window -t <session id>`.
    NewWindow(String),
}

/// Resolve how the pane is created from the launch target and whether the layout
/// splits. A split with no explicit destination splits the calling pane inside tmux;
/// outside tmux it falls back to a new window in the managed session.
fn resolve_creation(target: Option<LaunchTarget>, split: bool, in_tmux: bool) -> Result<Creation> {
    match target {
        Some(LaunchTarget::SplitPane(pane)) => {
            if !split {
                return Err(anyhow!(
                    "internal: split target supplied without a split layout"
                ));
            }
            Ok(Creation::Split(pane))
        }
        Some(LaunchTarget::WindowScope(scope)) => Ok(if split {
            Creation::Split(scope)
        } else {
            Creation::NewWindow(scope)
        }),
        None if in_tmux => {
            // Pin the target to the calling pane via $TMUX_PANE. Without
            // an explicit `-t`, tmux operates on the most-recently-active
            // pane on the server, which races when other sessions are
            // opened concurrently — the new pane lands in the wrong
            // session.
            let calling = target::calling_pane_id()
                .ok_or_else(|| anyhow!("$TMUX is set but no calling pane id is available"))?;
            if split {
                Ok(Creation::Split(calling))
            } else {
                let source = format!("calling pane {calling}");
                Ok(Creation::NewWindow(destination_session_id(
                    &calling, &source,
                )?))
            }
        }
        // Outside tmux: ensure the managed session exists, then add a fresh
        // window for our command. A session this call creates gets the deep
        // history limit before the command's window exists, so the command's
        // first line survives scrolling. The window is created by the session's
        // stable id, not by name, so it cannot land in a replacement session.
        None => Ok(Creation::NewWindow(ensure_managed_session_atomic()?)),
    }
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

    fn resolve(
        split: Option<Split>,
        size: Option<u32>,
        has_target: bool,
        has_session: bool,
        has_window: bool,
        in_tmux: bool,
    ) -> Layout {
        resolve_layout(split, size, has_target, has_session, has_window, in_tmux).unwrap()
    }

    #[test]
    fn flagless_inside_tmux_is_tile() {
        assert_eq!(resolve(None, None, false, false, false, true), Layout::Tile);
        assert_eq!(resolve(None, None, true, false, false, true), Layout::Tile);
        // `--target` with a scope still splits the resolved pane.
        assert_eq!(resolve(None, None, true, true, true, true), Layout::Tile);
    }

    #[test]
    fn size_alone_is_the_horizontal_split_at_that_percentage() {
        let h40 = Layout::Split {
            direction: "-h",
            size: Some(40),
        };
        assert_eq!(resolve(None, Some(40), false, false, false, true), h40);
        assert_eq!(resolve(None, Some(40), true, false, false, true), h40);
    }

    #[test]
    fn flagless_outside_tmux_or_with_a_scope_is_a_new_window() {
        for size in [None, Some(40)] {
            assert_eq!(
                resolve(None, size, false, false, false, false),
                Layout::NewWindow
            );
            assert_eq!(
                resolve(None, size, false, true, false, true),
                Layout::NewWindow
            );
            assert_eq!(
                resolve(None, size, false, false, true, true),
                Layout::NewWindow
            );
        }
    }

    #[test]
    fn explicit_split_flags_resolve_regardless_of_context() {
        for in_tmux in [true, false] {
            assert_eq!(
                resolve(Some(Split::H), None, false, true, false, in_tmux),
                Layout::Split {
                    direction: "-h",
                    size: None
                }
            );
            assert_eq!(
                resolve(Some(Split::V), Some(30), false, false, false, in_tmux),
                Layout::Split {
                    direction: "-v",
                    size: Some(30)
                }
            );
            assert_eq!(
                resolve(Some(Split::Window), Some(30), true, false, false, in_tmux),
                Layout::NewWindow
            );
            // An explicit tile chooses its destination as `h|v` do, scope included.
            for (target, session, window) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (false, true, true),
            ] {
                assert_eq!(
                    resolve(Some(Split::Tile), None, target, session, window, in_tmux),
                    Layout::Tile
                );
            }
        }
    }

    #[test]
    fn split_tile_with_size_is_rejected_naming_both_flags() {
        for in_tmux in [true, false] {
            let error = resolve_layout(Some(Split::Tile), Some(50), false, false, false, in_tmux)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("--split") && error.contains("--size"),
                "the diagnostic must name both flags: {error}"
            );
        }
    }

    /// One ownership snapshot line: `@tt-name`, `@tt-agent`, `@tt-cwd`.
    fn snapshot_line(name: &str, agent: &str, cwd: &str) -> String {
        [name, agent, cwd].join(&FIELD_SEP.to_string()) + "\n"
    }

    #[test]
    fn ownership_snapshot_reads_name_agent_and_cwd_per_pane() {
        let output = [
            snapshot_line("", "", ""),
            snapshot_line("npm-a3f2", "", "/work"),
            snapshot_line("", "codex", ""),
        ]
        .concat();
        let panes = parse_ownership_snapshot(&output).unwrap();
        assert_eq!(panes.len(), 3);
        assert_eq!(
            (panes[0].name.as_deref(), panes[0].agent.as_deref()),
            (None, None)
        );
        assert_eq!(panes[1].name.as_deref(), Some("npm-a3f2"));
        assert_eq!(panes[1].cwd.as_deref(), Some("/work"));
        assert_eq!(panes[2].agent.as_deref(), Some("codex"));
        assert_eq!(panes[2].cwd, None);
    }

    #[test]
    fn ownership_snapshot_rejects_malformed_lines() {
        let sep = FIELD_SEP.to_string();
        assert!(parse_ownership_snapshot(&["a", "b"].join(&sep)).is_err());
        assert!(parse_ownership_snapshot(&["a", "b", "c", "d"].join(&sep)).is_err());
        assert!(parse_ownership_snapshot("").unwrap().is_empty());
    }

    // -- Window guard and kill-time removal (ISSUE-260801-1310-06) ---------------------

    fn registered(name: Option<&str>, cwd: Option<&str>) -> names::Registered {
        names::Registered {
            name: name.map(str::to_owned),
            agent: None,
            access: None,
            launched_at: None,
            cwd: cwd.map(str::to_owned),
            surface: None,
            surface_unvalidated: false,
        }
    }

    #[test]
    fn guard_holds_on_a_marked_window_with_one_owned_pane_among_others() {
        let panes = [
            registered(None, None),
            registered(Some("ours"), Some("/work")),
            registered(Some("theirs"), Some("/other")),
        ];
        assert_eq!(
            guard_verdict(true, &panes, Some("/work")),
            WindowGuard::Holds
        );
        assert!(WindowGuard::Holds.holds());
    }

    #[test]
    fn guard_names_the_absent_marker_whatever_the_ownership() {
        let owned = [registered(Some("ours"), Some("/work"))];
        let foreign = [registered(Some("theirs"), Some("/other"))];
        for panes in [&owned[..], &foreign[..]] {
            let verdict = guard_verdict(false, panes, Some("/work"));
            assert_eq!(verdict, WindowGuard::MarkerAbsent);
            assert!(!verdict.holds());
        }
    }

    #[test]
    fn guard_names_missing_ownership_on_a_marked_window() {
        let panes = [
            registered(None, None),
            registered(Some("theirs"), Some("/other")),
        ];
        let verdict = guard_verdict(true, &panes, Some("/work"));
        assert_eq!(verdict, WindowGuard::NoOwnedPane);
        assert!(!verdict.holds());
    }

    #[test]
    fn guard_ownership_fails_open_as_the_predicate_does() {
        // No recorded cwd, or no current cwd: the predicate passes on registration.
        assert!(guard_verdict(true, &[registered(Some("old"), None)], Some("/work")).holds());
        assert!(guard_verdict(true, &[registered(Some("x"), Some("/other"))], None).holds());
    }

    // -- Overflow candidates (ISSUE-260801-1310-07) -----------------------------------

    fn window(id: &str, index: u32, marked: bool, panes: Vec<names::Registered>) -> WindowState {
        WindowState {
            window_id: id.to_owned(),
            index,
            marked,
            panes,
        }
    }

    fn owned() -> names::Registered {
        registered(Some("ours"), Some("/work"))
    }

    #[test]
    fn overflow_candidates_exclude_unmarked_unowned_and_origin_windows() {
        let windows = [
            // The origin, marked and owned: never its own destination.
            window("@1", 1, true, vec![owned()]),
            // Unmarked, though owned.
            window("@2", 2, false, vec![owned()]),
            // Marked, but its only panes are unregistered or from another cwd.
            window(
                "@3",
                3,
                true,
                vec![
                    registered(None, None),
                    registered(Some("x"), Some("/other")),
                ],
            ),
            // Marked and owned: the one candidate.
            window("@4", 4, true, vec![registered(None, None), owned()]),
        ];
        assert_eq!(overflow_candidates(&windows, "@1", Some("/work")), ["@4"]);
    }

    #[test]
    fn overflow_candidates_are_in_ascending_window_index() {
        // Listed out of index order, with ids whose order differs from the indexes.
        let windows = [
            window("@7", 5, true, vec![owned()]),
            window("@2", 0, true, vec![owned()]),
            window("@9", 9, true, vec![owned()]),
            window("@3", 2, true, vec![owned()]),
            window("@1", 1, false, vec![owned()]),
        ];
        assert_eq!(
            overflow_candidates(&windows, "@3", Some("/work")),
            ["@2", "@7", "@9"]
        );
    }

    #[test]
    fn overflow_candidates_judge_ownership_as_the_window_guard_does() {
        // No recorded cwd passes, as the ownership predicate fails open.
        let windows = [
            window("@1", 1, true, vec![registered(Some("old"), None)]),
            window("@2", 2, true, vec![registered(Some("x"), Some("/other"))]),
        ];
        assert_eq!(overflow_candidates(&windows, "@0", Some("/work")), ["@1"]);
        // An unreadable current cwd passes every registered pane.
        assert_eq!(overflow_candidates(&windows, "@0", None), ["@1", "@2"]);
        assert!(overflow_candidates(&[], "@0", Some("/work")).is_empty());
    }

    #[test]
    fn window_identity_parses_id_and_index() {
        let sep = FIELD_SEP.to_string();
        assert_eq!(
            parse_window_identity(&format!("@4{sep}12\n")).unwrap(),
            ("@4".to_owned(), 12)
        );
        assert!(parse_window_identity("@4\n").is_err());
        assert!(parse_window_identity(&format!("{sep}1\n")).is_err());
        assert!(parse_window_identity(&format!("@4{sep}x\n")).is_err());
    }

    #[test]
    fn removal_with_a_retile_is_one_command_sequence() {
        assert_eq!(
            removal_args("%5", "@2", true),
            [
                "kill-pane",
                "-t",
                "%5",
                ";",
                "select-layout",
                "-t",
                "@2",
                "tiled"
            ]
        );
    }

    #[test]
    fn removal_without_a_retile_is_the_bare_kill() {
        assert_eq!(removal_args("%5", "@2", false), ["kill-pane", "-t", "%5"]);
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
