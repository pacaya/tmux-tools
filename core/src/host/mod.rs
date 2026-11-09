//! The Host port: the one pane-level interface through which `tmux-tools` drives the
//! panes of a Host.
//!
//! A [`Host`] is the terminal program whose panes the tool drives. A Host backend is the
//! implementation of [`HostPort`] for one host; [`TmuxHost`] is the only backend. Every
//! module outside a backend (the verbs, [`crate::layout`], [`crate::target`],
//! [`crate::names`] and [`crate::idle`]) reaches a host only through this port, and no
//! module outside a backend builds host commands. [`HostCapabilities`] names the
//! optional features a backend supports.
//!
//! The operations are pane-level: create (a split or a new window), list, capture
//! (screen and history from a mark), send text, send keys, close, read and write the
//! Pane registration, capabilities, and the layout operations gated by
//! [`HostCapabilities::tile`]: apply the tiled layout, read window and pane geometry,
//! read and write Layout intent, and move a pane to another or a new window.

use anyhow::Result;

use crate::layout::WindowGeometry;
use crate::names::Registered;

mod tmux;

pub use self::tmux::TmuxHost;

/// The terminal program whose panes the tool drives.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Host {
    Tmux,
}

/// The optional features a Host backend supports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostCapabilities {
    /// The host reports Host state for a pane.
    pub native_state: bool,
    /// The host observes a prompt's submission.
    pub submit_confirmation: bool,
    /// The host shows desktop notifications.
    pub notify: bool,
    /// The host applies a tiled layout and supports the other layout operations.
    pub tile: bool,
    /// The host supports the `rebalance` verb.
    pub rebalance: bool,
    /// The host honours a size for a split (`--size`).
    pub size: bool,
}

/// The backend the current invocation drives. Only the tmux backend exists.
pub fn current() -> &'static dyn HostPort {
    &TmuxHost
}

/// One field of a Pane registration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RegistrationKey {
    Name,
    Agent,
    Access,
    LaunchedAt,
    Cwd,
    Surface,
    SurfaceUnvalidated,
}

/// The direction of a split: the new pane goes right of (`Horizontal`) or below
/// (`Vertical`) the pane it splits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Which lines of a pane a capture reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureRange {
    /// The visible pane only.
    Visible,
    /// The last `n` lines of history and screen, ending at the bottom of the pane.
    Tail(u32),
    /// The whole history and the visible pane.
    All,
    /// The lines from `start` to `end`, numbered as the host numbers them (negative =
    /// history).
    Rows { start: i64, end: i64 },
}

/// A capture with the pane's live alternate-screen state, both read at one instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCapture {
    /// Whether the pane is on the alternate screen, where it keeps no scrollback.
    pub alternate_on: bool,
    /// The captured text.
    pub text: String,
}

/// The live pane state sampled at the same instant as a full-history capture, from
/// which `prompt` fixes its History mark and judges Completeness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryProbe {
    pub history_size: u64,
    pub history_limit: u64,
    pub cursor_y: u64,
    pub alternate_on: bool,
    /// The pane's width in columns. A change between the mark and settle rewraps
    /// history, so the row-counted mark no longer locates the submission point.
    pub pane_width: u64,
    /// The pane's count of lines evicted from history, when the running host provides
    /// it. Support is detected at runtime, never from a version.
    pub history_collected: Option<u64>,
}

impl HistoryProbe {
    /// The History mark: the absolute line of the cursor — the submission point — in a
    /// full-history capture of the same instant. History lines precede the visible
    /// pane, so the cursor's absolute index is `history_size + cursor_y`.
    pub fn mark(&self) -> usize {
        (self.history_size + self.cursor_y) as usize
    }
}

/// Which panes `list` reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListScope<'a> {
    /// The panes of the named session.
    Session(&'a str),
    /// Every pane.
    All,
    /// The host's default scope: on tmux, the Managed session and, inside tmux, the
    /// caller's session.
    Default,
}

/// One pane as `list` reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneListing {
    /// The host's own line for the pane, printed unchanged by `list --format raw`.
    pub raw: String,
    pub session: String,
    pub window_index_sort: u64,
    pub window_name: String,
    pub pane_index_sort: u64,
    pub pane_id: String,
    pub name: Option<String>,
    pub agent: Option<String>,
    pub access: Option<String>,
    pub launched_at: Option<String>,
    pub pane_dead: bool,
    pub pane_dead_status: Option<String>,
    pub last_activity: Option<i64>,
}

/// One pane as `status` reports it.
#[derive(Debug)]
pub struct PaneDescription {
    /// The host's own description of the pane, printed unchanged by
    /// `status --format raw`.
    pub raw: String,
    /// The description parsed; an error when the host's description is malformed.
    pub summary: Result<PaneSummary>,
}

/// The parsed fields of a [`PaneDescription`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneSummary {
    pub session: String,
    pub window_index: String,
    pub window_name: String,
    pub pane_index: String,
    pub pane_id: String,
    pub name: Option<String>,
    pub agent: Option<String>,
    pub access: Option<String>,
    pub launched_at: Option<String>,
    pub cwd: Option<String>,
}

/// Where a new window is created: the session of a pane, a named session, or the
/// Managed session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowHome<'a> {
    /// The session of the pane the caller named with `--target`.
    TargetPane(&'a str),
    /// The session of the calling pane.
    CallingPane(&'a str),
    /// The session the caller named with `--session`.
    Session(&'a str),
    /// The Managed session.
    ManagedSession,
}

/// The pane-level operations of one host.
///
/// Targets are host-native strings: a pane id, a window id, or a scope that a backend
/// operation returned. A host command that ran and failed is an error that
/// [`HostPort::is_command_failure`] recognises through any added context.
pub trait HostPort: Sync {
    /// Which host this backend drives.
    fn host(&self) -> Host;

    /// The optional features this backend supports.
    fn capabilities(&self) -> HostCapabilities;

    /// Whether `error` (or an error it wraps) is a host command that ran and failed, as
    /// opposed to one that could not run.
    fn is_command_failure(&self, error: &anyhow::Error) -> bool;

    // --- Targets -------------------------------------------------------------------

    /// The active pane of the window or scope `target`.
    fn active_pane(&self, target: &str) -> Result<String>;

    /// The scope target of `--session` and `--window` (at least one is given; the
    /// Managed session stands in for an absent session).
    fn scope_target(&self, session: Option<&str>, window: Option<&str>) -> String;

    /// The pane that invoked the tool, inside the host.
    fn calling_pane(&self) -> Result<String>;

    /// The most recently active pane of the Managed session, which is created when
    /// absent.
    fn managed_pane(&self) -> Result<String>;

    // --- Create --------------------------------------------------------------------

    /// Split `target` and run `command` in the new pane; returns its pane id.
    fn create_split(
        &self,
        target: &str,
        direction: SplitDirection,
        percent: Option<u32>,
        command: &str,
    ) -> Result<String>;

    /// Create a window in `session` (a session id) running `command`; returns the pane
    /// id of its pane.
    fn create_window(&self, session: &str, command: &str) -> Result<String>;

    /// The session id where a new window for `home` goes.
    fn new_window_session(&self, home: WindowHome<'_>) -> Result<String>;

    /// Ensure the Managed session exists, tolerating concurrent callers, and return its
    /// session id.
    fn ensure_managed_session(&self) -> Result<String>;

    // --- List ----------------------------------------------------------------------

    /// Every pane in `scope`, unsorted. No running host is an empty list.
    fn list_panes(&self, scope: ListScope<'_>) -> Result<Vec<PaneListing>>;

    /// Describe `pane`, or the host's current pane when `None`.
    fn describe_pane(&self, pane: Option<&str>) -> Result<PaneDescription>;

    /// Whether the host lists `pane` among all its panes.
    fn pane_exists(&self, pane: &str) -> Result<bool>;

    // --- Capture -------------------------------------------------------------------

    /// Capture `range` of `pane` as plain text.
    fn capture(&self, pane: &str, range: CaptureRange) -> Result<String>;

    /// Capture lines `start` to `end` of `pane` (both `None`: the visible pane) with its
    /// ANSI escape sequences.
    fn capture_ansi(&self, pane: &str, start: Option<i64>, end: Option<i64>) -> Result<String>;

    /// Capture `range` of `pane` with its live alternate-screen state.
    fn capture_with_screen(&self, pane: &str, range: CaptureRange) -> Result<ScreenCapture>;

    /// One idle-loop poll: the visible pane with its live alternate-screen state.
    fn poll_screen(&self, pane: &str) -> Result<ScreenCapture>;

    /// The live [`HistoryProbe`] of `pane` and its full-history capture, both read at
    /// one instant.
    fn probe_history(&self, pane: &str) -> Result<(HistoryProbe, String)>;

    /// Start copying the output of `pane` to the shell command `command`.
    fn start_output_pipe(&self, pane: &str, command: &str) -> Result<()>;

    /// Stop copying the output of `pane`.
    fn stop_output_pipe(&self, pane: &str) -> Result<()>;

    // --- Send ----------------------------------------------------------------------

    /// Send `keys` to `pane`; each is a key name or text the host reads as key names.
    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()>;

    /// Send `text` to `pane` literally.
    fn send_text(&self, pane: &str, text: &str) -> Result<()>;

    /// Whether the application in `pane` has bracketed paste enabled now.
    fn bracketed_paste_enabled(&self, pane: &str) -> Result<bool>;

    /// Deliver `text` to `pane` as one bracketed paste. `confirm` runs after the text is
    /// staged and immediately before the paste; when it returns `false` or an error,
    /// nothing is pasted and that error is returned.
    fn paste_text(
        &self,
        pane: &str,
        text: &str,
        confirm: &mut dyn FnMut() -> Result<bool>,
    ) -> Result<()>;

    // --- Close ---------------------------------------------------------------------

    /// Close `pane`. With `retile_window`, apply the tiled layout to that window in the
    /// same host request, so the retile completes even when the closed pane is the
    /// caller's own.
    fn close_pane(&self, pane: &str, retile_window: Option<&str>) -> Result<()>;

    // --- Registration --------------------------------------------------------------

    /// The whole Pane registration of `pane`.
    fn read_registration(&self, pane: &str) -> Result<Registered>;

    /// One registration field of `pane`; `None` when absent or empty.
    fn read_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<Option<String>>;

    /// Write one registration field of `pane`.
    fn write_registration_value(&self, pane: &str, key: RegistrationKey, value: &str)
        -> Result<()>;

    /// Remove one registration field of `pane`.
    fn clear_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<()>;

    /// Every pane id with its registered name (empty when the pane has none).
    fn pane_names(&self) -> Result<Vec<(String, String)>>;

    /// The name, agent and cwd registration of every pane in the window of `target`,
    /// as the ownership predicate reads them. Every other field is unset.
    fn window_registrations(&self, target: &str) -> Result<Vec<Registered>>;

    // --- Layout (gated by `tile`) --------------------------------------------------

    /// Apply the tiled layout to the window of `target`.
    fn apply_tiled_layout(&self, target: &str) -> Result<()>;

    /// Every pane of the window of `target` with its size and floor registration.
    fn window_geometry(&self, target: &str) -> Result<WindowGeometry>;

    /// The id and index of the window of `target`.
    fn window_identity(&self, target: &str) -> Result<(String, u32)>;

    /// Whether the window of `target` records Layout intent "tiled".
    fn read_layout_intent(&self, target: &str) -> Result<bool>;

    /// Record (`true`) or clear (`false`) Layout intent "tiled" on the window of
    /// `target`. Clearing an absent record succeeds.
    fn write_layout_intent(&self, target: &str, tiled: bool) -> Result<()>;

    /// The session id of the session `target` is in.
    fn session_of(&self, target: &str) -> Result<String>;

    /// The window ids of `session` (a session id).
    fn session_windows(&self, session: &str) -> Result<Vec<String>>;

    /// Move `pane` into `window`, splitting its active pane; the window is not selected.
    fn join_pane(&self, pane: &str, window: &str) -> Result<()>;

    /// Move `pane` into a new window of `session` (a session id); the window is not
    /// selected.
    fn break_pane(&self, pane: &str, session: &str) -> Result<()>;
}
