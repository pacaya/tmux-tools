//! The Host port: the one pane-level interface through which `tmux-tools` drives the
//! panes of a Host.
//!
//! A [`Host`] is the terminal program whose panes the tool drives. A Host backend is the
//! implementation of [`HostPort`] for one host: [`TmuxHost`], [`OrcaHost`] and
//! [`HerdrHost`]. The
//! [`HostSelection`] of an invocation picks one (P3); [`current`] returns it. Every
//! module outside a backend (the verbs, [`crate::layout`], [`crate::target`],
//! [`crate::names`] and [`crate::idle`]) reaches a host only through this port, and no
//! module outside a backend builds host commands. [`HostCapabilities`] names the
//! optional features a backend supports.
//!
//! The operations are pane-level: create (a split or a new window), list, capture
//! (screen and history from a mark), send text, send keys, close, read and write the
//! Pane registration, Host state, notify, capabilities, and the layout operations
//! gated by [`HostCapabilities::tile`]: apply the tiled layout, read window and pane
//! geometry, read and write Layout intent, and move a pane to another or a new window.

use std::fmt;

use anyhow::Result;
use serde::Serialize;

use crate::layout::{Registration, WindowGeometry};
use crate::names::Registered;

mod herdr;
mod orca;
mod tmux;

pub use self::herdr::{
    hc_name, HerdrHost, HC_TERMINAL_ID, TMUX_TOOLS_HERDR_SPACE, TMUX_TOOLS_HERDR_TAB,
};
pub use self::orca::{OrcaHost, ORCA_TERMINAL_HANDLE, ORCA_WORKTREE_ID};
pub use self::tmux::TmuxHost;
use crate::team::{TT_HOST, TT_TEAM};

/// The terminal program whose panes the tool drives.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Host {
    Tmux,
    Orca,
    Herdr,
}

impl Host {
    /// The host's name as the CLI reports it: `tmux`, `orca` or `herdr`.
    pub fn as_str(self) -> &'static str {
        match self {
            Host::Tmux => "tmux",
            Host::Orca => "orca",
            Host::Herdr => "herdr",
        }
    }
}

/// The environment variable that selects the host explicitly (P3.1).
pub const TMUX_TOOLS_HOST: &str = "TMUX_TOOLS_HOST";

/// Where a [`HostSelection`] came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionSource {
    /// `TMUX_TOOLS_HOST` named the host.
    Override,
    /// The named environment variable detected the host (P3.2).
    Detected(String),
    /// Nothing matched; the host is tmux (P3.3).
    Default,
    /// Inside a team, `TT_HOST` named the host (PRD-261108-0447-01 P3.1).
    Team(String),
}

impl fmt::Display for SelectionSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectionSource::Override => formatter.write_str(TMUX_TOOLS_HOST),
            SelectionSource::Detected(env_var) => formatter.write_str(env_var),
            SelectionSource::Default => formatter.write_str("default"),
            SelectionSource::Team(team_id) => write!(formatter, "TT_HOST (team {team_id})"),
        }
    }
}

/// The host chosen for one invocation and the source of that choice (P3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostSelection {
    pub host: Host,
    pub source: SelectionSource,
}

impl HostSelection {
    /// Select the host from the process environment: `TMUX_TOOLS_HOST` when set
    /// (`tmux`, `orca` or `herdr`; anything else, the empty value included, is an
    /// error); otherwise `$TMUX` set selects tmux, `ORCA_TERMINAL_HANDLE` set selects
    /// orca, and `HC_TERMINAL_ID` set selects herdr, in that order; otherwise tmux.
    pub fn from_env() -> Result<HostSelection> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// [`HostSelection::from_env`] over an arbitrary variable lookup.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<HostSelection> {
        // PRD-261108-0447-01 P3.1: inside a team, `TT_HOST` selects first.
        if let Some(team_id) = lookup(TT_TEAM).filter(|value| !value.trim().is_empty()) {
            let host = match lookup(TT_HOST).as_deref() {
                None => anyhow::bail!("{TT_TEAM} is set but {TT_HOST} is unset"),
                Some("tmux") => Host::Tmux,
                Some("orca") => Host::Orca,
                Some(other) => anyhow::bail!(
                    "{TT_HOST}={other:?} is not a host; valid values are tmux and orca"
                ),
            };
            return Ok(HostSelection {
                host,
                source: SelectionSource::Team(team_id.trim().to_owned()),
            });
        }
        if let Some(value) = lookup(TMUX_TOOLS_HOST) {
            let host = match value.as_str() {
                "tmux" => Host::Tmux,
                "orca" => Host::Orca,
                "herdr" => Host::Herdr,
                other => anyhow::bail!(
                    "{TMUX_TOOLS_HOST}={other:?} is not a host; valid values are tmux, orca and herdr"
                ),
            };
            return Ok(HostSelection {
                host,
                source: SelectionSource::Override,
            });
        }
        let set = |name: &str| lookup(name).is_some_and(|value| !value.is_empty());
        for (env_var, host) in [
            ("TMUX", Host::Tmux),
            (ORCA_TERMINAL_HANDLE, Host::Orca),
            (HC_TERMINAL_ID, Host::Herdr),
        ] {
            if set(env_var) {
                return Ok(HostSelection {
                    host,
                    source: SelectionSource::Detected(env_var.to_owned()),
                });
            }
        }
        Ok(HostSelection {
            host: Host::Tmux,
            source: SelectionSource::Default,
        })
    }
}

/// The selection of this invocation, made once from the environment.
fn selection_cell() -> &'static std::result::Result<HostSelection, String> {
    static SELECTION: std::sync::OnceLock<std::result::Result<HostSelection, String>> =
        std::sync::OnceLock::new();
    SELECTION.get_or_init(|| HostSelection::from_env().map_err(|error| format!("{error:#}")))
}

/// The [`HostSelection`] of this invocation; an error when `TMUX_TOOLS_HOST` names no
/// host, or, inside a team, when `TT_HOST` is missing or invalid or the Team record
/// does not exist ([`crate::team::TeamNotFound`], PRD-261108-0447-01 P3.2). The CLI
/// checks it before any verb runs.
pub fn selection() -> Result<HostSelection> {
    let selection = selection_cell()
        .clone()
        .map_err(|message| anyhow::anyhow!(message))?;
    if let SelectionSource::Team(_) = selection.source {
        crate::team::current()?;
    }
    // PRD-261109-2223-01 P2.3: a selected Herdr whose `hc` is missing or whose
    // `hc server status` fails ends the invocation before any verb runs.
    if selection.host == Host::Herdr {
        current().server_identity()?;
    }
    Ok(selection)
}

impl fmt::Display for Host {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The agent state a host itself reports for a pane. A host that reports none gives
/// `None` (`null` in JSON). Separate from the Pane state `idle | busy | unknown`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostState {
    Working,
    Blocked,
    Done,
    Idle,
}

/// What `prompt` knows of its submission (JSON `submit`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Submission {
    /// The host observed the submission or the turn start.
    Confirmed,
    /// The host accepted the input but observed no submission within the wait.
    Accepted,
    /// The host gives no observation; every host without `submit_confirmation`.
    Assumed,
}

/// Whether a desktop notification reached the host's client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    /// The notification was handed to the host for its client.
    Delivered,
    /// The host has no client to show it (on tmux: the pane's session has no attached
    /// client).
    NotDelivered,
}

/// An optional verb or option invoked on a host whose capabilities lack it. The CLI
/// prints it as `unsupported: <feature> on <host>` and exits 3.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unsupported {
    /// The verb, or the option as written (`--split tile`, `--size`).
    pub feature: &'static str,
    pub host: Host,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unsupported: {} on {}", self.feature, self.host)
    }
}

impl std::error::Error for Unsupported {}

/// `Ok` when `supported`; otherwise the [`Unsupported`] error for `feature` on the
/// current host.
pub fn require(supported: bool, feature: &'static str) -> Result<()> {
    if supported {
        Ok(())
    } else {
        Err(Unsupported {
            feature,
            host: current().host(),
        }
        .into())
    }
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

/// The backend the current invocation drives, from its [`HostSelection`]. An invalid
/// selection falls back to tmux here; the CLI rejects it through [`selection`] before
/// any verb runs.
pub fn current() -> &'static dyn HostPort {
    static ORCA: std::sync::OnceLock<OrcaHost> = std::sync::OnceLock::new();
    static HERDR: std::sync::OnceLock<HerdrHost> = std::sync::OnceLock::new();
    match selection_cell() {
        Ok(HostSelection {
            host: Host::Orca,
            source,
        }) => ORCA.get_or_init(|| OrcaHost::new(source.clone())),
        Ok(HostSelection {
            host: Host::Herdr,
            source,
        }) => HERDR.get_or_init(|| HerdrHost::new(source.clone())),
        _ => &TmuxHost,
    }
}

/// The environment variable of the test-only capability override: a comma-separated
/// list of capability names (`native_state`, `submit_confirmation`, `notify`, `tile`,
/// `rebalance`, `size`) that are false for this invocation. Compiled only with the
/// `test-fixtures` feature; a build without it ignores the variable.
#[cfg(feature = "test-fixtures")]
pub const TEST_CAPABILITIES_OFF: &str = "TMUX_TOOLS_TEST_CAPABILITIES_OFF";

/// `capabilities` with every capability named in [`TEST_CAPABILITIES_OFF`] set false.
/// An unknown name panics, so a mistyped test fails instead of passing vacuously.
#[cfg(feature = "test-fixtures")]
pub(crate) fn with_test_override(mut capabilities: HostCapabilities) -> HostCapabilities {
    let Ok(names) = std::env::var(TEST_CAPABILITIES_OFF) else {
        return capabilities;
    };
    for name in names
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let flag = match name {
            "native_state" => &mut capabilities.native_state,
            "submit_confirmation" => &mut capabilities.submit_confirmation,
            "notify" => &mut capabilities.notify,
            "tile" => &mut capabilities.tile,
            "rebalance" => &mut capabilities.rebalance,
            "size" => &mut capabilities.size,
            other => panic!("{TEST_CAPABILITIES_OFF}: unknown capability {other:?}"),
        };
        *flag = false;
    }
    capabilities
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
    /// PRD-261108-0447-01 P7.
    Parent,
    /// PRD-261108-0447-01 P6.4: the team id of a Teammate pane.
    Team,
    ShimId,
    Title,
    ClaudeAgentName,
    ClaudeTeamName,
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
    /// The `pane` value `list` prints for this row, when the host names its panes by
    /// one handle (Orca); `None` for the tmux `<window>.<name>:<pane>.<id>` label.
    pub label: Option<String>,
    /// Whether the host's pane list was incomplete and did not list this row's pane, so
    /// its liveness is unknown (P4.5).
    pub unknown: bool,
    /// The registration's `parent`, `team` and `shim_id` (PRD-261108-0447-01 P8.2).
    pub parent: Option<String>,
    pub team: Option<String>,
    pub shim_id: Option<String>,
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

/// The `prompt` result read from a History mark, with its Completeness
/// (PRD-260918-0323-01 P7).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySince {
    /// Every history line after the mark.
    pub text: String,
    /// Whether everything after the mark could be supplied.
    pub complete: bool,
    /// Why the result is incomplete, for the raw and concise stderr notice.
    pub incomplete_reason: String,
}

/// The pane-level operations of one host.
///
/// Targets are host-native strings: a pane id, a window id, or a scope that a backend
/// operation returned. A host command that ran and failed is an error that
/// [`HostPort::is_command_failure`] recognises through any added context.
pub trait HostPort: Sync {
    /// Which host this backend drives.
    fn host(&self) -> Host;

    /// The optional features this backend supports, read on each call.
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

    /// The identity of the host server the panes live on: the tmux server socket path,
    /// or the Orca `runtimeId` (a Team record's `server_id`, PRD-261108-0447-01 P4.3).
    fn server_identity(&self) -> Result<String>;

    /// The most recently active pane of the Managed session, which is created when
    /// absent.
    fn managed_pane(&self) -> Result<String>;

    // --- Create --------------------------------------------------------------------

    /// Split `target` and run `command` in the new pane; returns its pane id. With
    /// `detached`, the new pane does not become the active pane (tmux `split-window -d`);
    /// a host without such an option ignores it.
    fn create_split(
        &self,
        target: &str,
        direction: SplitDirection,
        percent: Option<u32>,
        command: &str,
        detached: bool,
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

    // --- Host state and notify -----------------------------------------------------

    /// The Host state of `pane`; `None` when the host reports none.
    fn host_state(&self, pane: &str) -> Result<Option<HostState>>;

    /// The Host state of each of `panes`, in order, read with one host query where the
    /// host allows it (`list`).
    fn host_states(&self, panes: &[String]) -> Result<Vec<Option<HostState>>> {
        panes.iter().map(|pane| self.host_state(pane)).collect()
    }

    /// Show a desktop notification with `title` and `body` for `pane`. Called only on a
    /// host with [`HostCapabilities::notify`].
    fn notify(&self, pane: &str, title: &str, body: &str) -> Result<Delivery>;

    // --- Pane records, prompts, and Host state with its start ----------------------
    //
    // The defaults serve a host that keeps the Pane registration on the pane itself
    // (tmux); a registry host (Orca) overrides them.

    /// Whether this host keeps the Pane registration in Pane records (P4.3).
    fn uses_pane_records(&self) -> bool {
        false
    }

    /// Whether this host creates every pane through [`HostPort::create_registered`],
    /// with its registration written at creation: a host with no tmux sessions or
    /// windows, which measures no geometry. A registry host does; Herdr does with no
    /// Pane records (PRD-261109-2223-01 P3.4).
    fn creates_registered(&self) -> bool {
        self.uses_pane_records()
    }

    /// Whether the program that runs a pane command keeps a shell after the command
    /// exits, so the command itself is the Keep-alive wrap (Orca, P9.9).
    fn shell_stays_after_command(&self) -> bool {
        false
    }

    /// Whether a new pane starts in the caller's directory; when it does not (Orca),
    /// the pane command `cd`s there.
    fn pane_starts_in_caller_dir(&self) -> bool {
        true
    }

    /// Whether `value` has the form of this host's own pane address, which a Target
    /// may name directly (Orca `term_…`, Herdr `t<n>`).
    fn is_pane_address(&self, _value: &str) -> bool {
        false
    }

    /// The host's exact status word for `pane` (`status` JSON `host_word`); `None`
    /// when the host reports none.
    fn host_word(&self, _pane: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// Whether `prompt` refuses a multi-line prompt whose one-submission delivery is
    /// not guaranteed, whatever the Surface declares (Herdr, PRD-261109-2223-01 P6.6).
    fn refuses_unguaranteed_multiline(&self) -> bool {
        false
    }

    /// The pane registered with `name`; `None` when no pane is. An error when the name
    /// is ambiguous, or the host cannot tell whether it is live.
    fn find_registered(&self, name: &str) -> Result<Option<String>> {
        let panes: Vec<String> = self
            .pane_names()?
            .into_iter()
            .filter(|(_, pane_name)| pane_name == name)
            .map(|(pane_id, _)| pane_id)
            .collect();
        match panes.len() {
            0 => Ok(None),
            1 => Ok(panes.into_iter().next()),
            _ => anyhow::bail!(
                "name '{}' resolves to multiple panes: {}; use a pane id (%N) explicitly",
                name,
                panes.join(", ")
            ),
        }
    }

    /// On a registry host: reserve the name, create a pane running `command` (a new tab
    /// when `split` is `None`, else a split of `target` or the calling pane), and
    /// register it (P4.4). Returns the new pane's handle and its name.
    fn create_registered(
        &self,
        _command: &str,
        _split: Option<SplitDirection>,
        _target: Option<&str>,
        _registration: &Registration<'_>,
    ) -> Result<(String, String)> {
        anyhow::bail!("{} keeps no Pane records", self.host())
    }

    /// The History mark of a host whose history is a read cursor (P9.3).
    fn history_cursor(&self, _pane: &str) -> Result<String> {
        anyhow::bail!("{} has no history read cursor", self.host())
    }

    /// Every history line of `pane` after `cursor`, paged to its end.
    fn read_history_since(&self, _pane: &str, _cursor: &str) -> Result<String> {
        anyhow::bail!("{} has no history read cursor", self.host())
    }

    /// [`HostPort::read_history_since`] with its Completeness. The default reports
    /// incomplete: a host whose history is a read cursor reports no pane width and no
    /// full-screen state (P9.3).
    fn read_history_since_complete(&self, pane: &str, cursor: &str) -> Result<HistorySince> {
        Ok(HistorySince {
            text: self.read_history_since(pane, cursor)?,
            complete: false,
            incomplete_reason: format!(
                "{} reports no pane width or full-screen state",
                self.host()
            ),
        })
    }

    /// On a host with `submit_confirmation`: whether one-submission delivery of a prompt
    /// to `pane`, registered with `agent`, is guaranteed (P9.6). An error when the host
    /// cannot deliver a prompt at all.
    fn prompt_delivery_guaranteed(&self, _pane: &str, _agent: Option<&str>) -> Result<bool> {
        anyhow::bail!("{} does not observe prompt submission", self.host())
    }

    /// On a host with `submit_confirmation`: send `text` plus Enter as one prompt and
    /// observe its submission for up to `wait`.
    fn submit_prompt(
        &self,
        _pane: &str,
        _text: &str,
        _wait: std::time::Duration,
    ) -> Result<Submission> {
        anyhow::bail!("{} does not observe prompt submission", self.host())
    }

    /// The final `submit` of a prompt whose [`HostPort::submit_prompt`] answered
    /// `submit`, judged after the settle step. The default keeps `submit`.
    fn settle_submission(&self, _pane: &str, submit: Submission) -> Result<Submission> {
        Ok(submit)
    }

    /// The Host state of `pane` with the time it began, in milliseconds since the Unix
    /// epoch when the host reports it.
    fn host_state_since(&self, pane: &str) -> Result<Option<(HostState, Option<u64>)>> {
        Ok(self.host_state(pane)?.map(|state| (state, None)))
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn select(vars: &[(&str, &str)]) -> Result<HostSelection> {
        let vars: std::collections::HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        HostSelection::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn selection_follows_p3() {
        let both = select(&[("TMUX", "/s,1,0"), (ORCA_TERMINAL_HANDLE, "term_x")]).unwrap();
        assert_eq!(both.host, Host::Tmux);
        assert_eq!(both.source, SelectionSource::Detected("TMUX".to_owned()));

        let orca = select(&[(ORCA_TERMINAL_HANDLE, "term_x")]).unwrap();
        assert_eq!(orca.host, Host::Orca);
        assert_eq!(orca.source.to_string(), ORCA_TERMINAL_HANDLE);

        let overridden = select(&[(TMUX_TOOLS_HOST, "orca"), ("TMUX", "/s,1,0")]).unwrap();
        assert_eq!(
            (overridden.host, overridden.source),
            (Host::Orca, SelectionSource::Override)
        );

        let none = select(&[]).unwrap();
        assert_eq!(
            (none.host, none.source),
            (Host::Tmux, SelectionSource::Default)
        );

        let error = select(&[(TMUX_TOOLS_HOST, "foo")]).unwrap_err().to_string();
        assert!(error.contains("tmux") && error.contains("orca"), "{error}");

        // P3.1: a present but empty value is an invalid value, not an unset one.
        // PRD-261108-0447-01 P3.1: inside a team `TT_HOST` selects first.
        let team = select(&[
            (TT_TEAM, "0a1b2c3d"),
            (TT_HOST, "orca"),
            ("TMUX", "/s,1,0"),
            (TMUX_TOOLS_HOST, "tmux"),
        ])
        .unwrap();
        assert_eq!(
            (team.host, team.source),
            (Host::Orca, SelectionSource::Team("0a1b2c3d".to_owned()))
        );
        let missing = select(&[(TT_TEAM, "0a1b2c3d")]).unwrap_err().to_string();
        assert!(missing.contains("TT_HOST"), "{missing}");
        let invalid = select(&[(TT_TEAM, "0a1b2c3d"), (TT_HOST, "rex")])
            .unwrap_err()
            .to_string();
        assert!(
            invalid.contains("tmux") && invalid.contains("orca"),
            "{invalid}"
        );

        // PRD-261109-2223-01 P2.1–P2.2: `herdr` is a valid value, and
        // `HC_TERMINAL_ID` selects it after `$TMUX` and `ORCA_TERMINAL_HANDLE`.
        let herdr = select(&[(HC_TERMINAL_ID, "t1")]).unwrap();
        assert_eq!(
            (herdr.host, herdr.source),
            (
                Host::Herdr,
                SelectionSource::Detected(HC_TERMINAL_ID.to_owned())
            )
        );
        let tmux_first = select(&[("TMUX", "/s,1,0"), (HC_TERMINAL_ID, "t1")]).unwrap();
        assert_eq!(tmux_first.host, Host::Tmux);
        let orca_first =
            select(&[(ORCA_TERMINAL_HANDLE, "term_x"), (HC_TERMINAL_ID, "t1")]).unwrap();
        assert_eq!(orca_first.host, Host::Orca);
        let override_herdr = select(&[(TMUX_TOOLS_HOST, "herdr"), ("TMUX", "/s,1,0")]).unwrap();
        assert_eq!(override_herdr.host, Host::Herdr);
        let error = select(&[(TMUX_TOOLS_HOST, "foo")]).unwrap_err().to_string();
        assert!(error.contains("herdr"), "{error}");

        let empty = select(&[(TMUX_TOOLS_HOST, ""), ("TMUX", "/s,1,0")])
            .unwrap_err()
            .to_string();
        assert!(empty.contains("tmux") && empty.contains("orca"), "{empty}");
    }
}
