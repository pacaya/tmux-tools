//! The tmux Host backend: every tmux command `tmux-tools` runs is built here.
//!
//! Rules that are tmux facts live here: the Managed session, the Bracketed paste
//! transport, the Pane registration as `@tt-*` pane options, and Layout intent as the
//! `@tt-tiled` window option.

use anyhow::Result;

use crate::host::{
    CaptureRange, Delivery, HistoryProbe, Host, HostCapabilities, HostPort, HostState, ListScope,
    PaneDescription, PaneListing, RegistrationKey, ScreenCapture, SplitDirection, WindowHome,
};
use crate::layout::WindowGeometry;
use crate::names::Registered;

mod listing;
mod notify;
mod pane_io;
mod registration;
mod runner;
mod session;
mod window;

/// Separates the fields of a tmux format line. Registration values are user strings
/// and may hold tabs; the unit separator does not occur in them.
const FIELD_SEP: char = '\x1f';

/// The tmux Host backend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TmuxHost;

impl HostPort for TmuxHost {
    fn host(&self) -> Host {
        Host::Tmux
    }

    fn capabilities(&self) -> HostCapabilities {
        let capabilities = HostCapabilities {
            native_state: false,
            submit_confirmation: false,
            notify: true,
            tile: true,
            rebalance: true,
            size: true,
        };
        #[cfg(feature = "test-fixtures")]
        let capabilities = crate::host::with_test_override(capabilities);
        capabilities
    }

    fn is_command_failure(&self, error: &anyhow::Error) -> bool {
        error.downcast_ref::<runner::CommandFailed>().is_some()
    }

    fn active_pane(&self, target: &str) -> Result<String> {
        session::active_pane_for_target(target)
    }

    fn scope_target(&self, session: Option<&str>, window: Option<&str>) -> String {
        session::scoped_target(session, window)
    }

    fn calling_pane(&self) -> Result<String> {
        session::current_pane()
    }

    fn managed_pane(&self) -> Result<String> {
        session::managed_pane()
    }

    fn create_split(
        &self,
        target: &str,
        direction: SplitDirection,
        percent: Option<u32>,
        command: &str,
    ) -> Result<String> {
        window::create_split(target, direction, percent, command)
    }

    fn create_window(&self, session: &str, command: &str) -> Result<String> {
        window::create_window(session, command)
    }

    fn new_window_session(&self, home: WindowHome<'_>) -> Result<String> {
        session::new_window_session(home)
    }

    fn ensure_managed_session(&self) -> Result<String> {
        session::ensure_managed_session_atomic()
    }

    fn list_panes(&self, scope: ListScope<'_>) -> Result<Vec<PaneListing>> {
        listing::list_panes(scope)
    }

    fn describe_pane(&self, pane: Option<&str>) -> Result<PaneDescription> {
        listing::describe_pane(pane)
    }

    fn pane_exists(&self, pane: &str) -> Result<bool> {
        window::pane_exists(pane)
    }

    fn capture(&self, pane: &str, range: CaptureRange) -> Result<String> {
        pane_io::capture(pane, range)
    }

    fn capture_ansi(&self, pane: &str, start: Option<i64>, end: Option<i64>) -> Result<String> {
        pane_io::capture_ansi(pane, start, end)
    }

    fn capture_with_screen(&self, pane: &str, range: CaptureRange) -> Result<ScreenCapture> {
        pane_io::capture_with_screen(pane, range)
    }

    fn poll_screen(&self, pane: &str) -> Result<ScreenCapture> {
        pane_io::poll_screen(pane)
    }

    fn probe_history(&self, pane: &str) -> Result<(HistoryProbe, String)> {
        pane_io::probe_history(pane)
    }

    fn start_output_pipe(&self, pane: &str, command: &str) -> Result<()> {
        pane_io::start_output_pipe(pane, command)
    }

    fn stop_output_pipe(&self, pane: &str) -> Result<()> {
        pane_io::stop_output_pipe(pane)
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        pane_io::send_keys(pane, keys)
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        pane_io::send_text(pane, text)
    }

    fn bracketed_paste_enabled(&self, pane: &str) -> Result<bool> {
        pane_io::bracketed_paste_enabled(pane)
    }

    fn paste_text(
        &self,
        pane: &str,
        text: &str,
        confirm: &mut dyn FnMut() -> Result<bool>,
    ) -> Result<()> {
        pane_io::paste_text(pane, text, confirm)
    }

    fn close_pane(&self, pane: &str, retile_window: Option<&str>) -> Result<()> {
        window::close_pane(pane, retile_window)
    }

    fn read_registration(&self, pane: &str) -> Result<Registered> {
        registration::read(pane)
    }

    fn read_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<Option<String>> {
        registration::get(pane, key)
    }

    fn write_registration_value(
        &self,
        pane: &str,
        key: RegistrationKey,
        value: &str,
    ) -> Result<()> {
        registration::set(pane, key, value)
    }

    fn clear_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<()> {
        registration::unset(pane, key)
    }

    fn pane_names(&self) -> Result<Vec<(String, String)>> {
        registration::pane_names()
    }

    fn window_registrations(&self, target: &str) -> Result<Vec<Registered>> {
        registration::window_registrations(target)
    }

    fn host_state(&self, _pane: &str) -> Result<Option<HostState>> {
        // tmux reports no agent state for a pane.
        Ok(None)
    }

    fn notify(&self, pane: &str, title: &str, body: &str) -> Result<Delivery> {
        notify::notify(pane, title, body)
    }

    fn apply_tiled_layout(&self, target: &str) -> Result<()> {
        window::retile(target)
    }

    fn window_geometry(&self, target: &str) -> Result<WindowGeometry> {
        window::measure(target)
    }

    fn window_identity(&self, target: &str) -> Result<(String, u32)> {
        window::window_identity(target)
    }

    fn read_layout_intent(&self, target: &str) -> Result<bool> {
        window::tiled_marker_present(target)
    }

    fn write_layout_intent(&self, target: &str, tiled: bool) -> Result<()> {
        if tiled {
            window::set_tiled_marker(target)
        } else {
            window::clear_tiled_marker(target)
        }
    }

    fn session_of(&self, target: &str) -> Result<String> {
        window::session_of(target)
    }

    fn session_windows(&self, session: &str) -> Result<Vec<String>> {
        window::session_windows(session)
    }

    fn join_pane(&self, pane: &str, window: &str) -> Result<()> {
        window::join_pane(pane, window)
    }

    fn break_pane(&self, pane: &str, session: &str) -> Result<()> {
        window::break_pane(pane, session)
    }
}
