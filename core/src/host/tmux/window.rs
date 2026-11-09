//! Pane creation, closing, and the window operations of the tmux backend: the tiled
//! layout, window and pane geometry, Layout intent as the `@tt-tiled` window option,
//! and moving a pane between windows.

use anyhow::{anyhow, Context, Result};

use super::{registration::option_format, runner, FIELD_SEP};
use crate::format::non_empty;
use crate::host::{RegistrationKey, SplitDirection};
use crate::layout::{PaneGeometry, WindowGeometry};

/// The `@tt-tiled` window option: Layout intent on tmux. Present on a window this tool
/// tiled, unset when a placement with explicit sizing intent targets it.
pub(super) const TILED_MARKER: &str = "@tt-tiled";

/// tmux's layout name for the tiled arrangement this tool applies.
const TILED_LAYOUT: &str = "tiled";

pub(super) fn create_split(
    target: &str,
    direction: SplitDirection,
    percent: Option<u32>,
    command: &str,
) -> Result<String> {
    created_pane_id(&split_window_args(
        command,
        direction_flag(direction),
        percent,
        Some(target),
    ))
}

pub(super) fn create_window(session: &str, command: &str) -> Result<String> {
    created_pane_id(&new_window_args(command, Some(session)))
}

fn created_pane_id(args: &[String]) -> Result<String> {
    let pane_id = runner::run_checked_owned(args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!("tmux launch command returned an empty pane id"));
    }
    Ok(pane_id)
}

fn direction_flag(direction: SplitDirection) -> &'static str {
    match direction {
        SplitDirection::Horizontal => "-h",
        SplitDirection::Vertical => "-v",
    }
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

/// Destroy `pane_id` and, with `retile_window`, retile that window in the same tmux
/// command sequence.
pub(super) fn close_pane(pane_id: &str, retile_window: Option<&str>) -> Result<()> {
    runner::run_checked_owned(&removal_args(pane_id, retile_window))?;
    Ok(())
}

/// The tmux arguments that destroy `pane_id` and, with `retile_window`, retile that
/// window in the same command sequence.
fn removal_args(pane_id: &str, retile_window: Option<&str>) -> Vec<String> {
    let mut args = vec!["kill-pane", "-t", pane_id];
    if let Some(window_id) = retile_window {
        args.extend([";", "select-layout", "-t", window_id, TILED_LAYOUT]);
    }
    args.into_iter().map(str::to_owned).collect()
}

/// Whether the server lists `pane_id` among all its panes.
pub(super) fn pane_exists(pane_id: &str) -> Result<bool> {
    let output = runner::run_checked(&["list-panes", "-a", "-F", "#{pane_id}"])?;
    Ok(output.lines().any(|line| line.trim() == pane_id))
}

/// Apply tmux's `tiled` layout to the window that `target` is in.
pub(super) fn retile(target: &str) -> Result<()> {
    runner::run_checked(&["select-layout", "-t", target, TILED_LAYOUT])
        .with_context(|| format!("cannot retile the window of {target}"))?;
    Ok(())
}

/// The `#{session_id}` of the session `target` is in.
pub(super) fn session_of(target: &str) -> Result<String> {
    let session = runner::run_checked(&["display-message", "-p", "-t", target, "#{session_id}"])
        .with_context(|| format!("cannot read the session of {target}"))?
        .trim()
        .to_owned();
    if session.is_empty() {
        return Err(anyhow!("tmux reported no session id for {target}"));
    }
    Ok(session)
}

/// The window ids of `session` (a session id).
pub(super) fn session_windows(session: &str) -> Result<Vec<String>> {
    let output = runner::run_checked(&["list-windows", "-t", session, "-F", "#{window_id}"])
        .with_context(|| format!("cannot list the windows of session {session}"))?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|window_id| !window_id.is_empty())
        .map(str::to_owned)
        .collect())
}

/// `join-pane -d` `pane_id` into `window_id`, splitting its active pane; the window is
/// not selected.
pub(super) fn join_pane(pane_id: &str, window_id: &str) -> Result<()> {
    runner::run_checked(&["join-pane", "-d", "-s", pane_id, "-t", window_id])
        .with_context(|| format!("cannot join pane {pane_id} into window {window_id}"))?;
    Ok(())
}

/// `break-pane -d` `pane_id` into a new window of `session` (a session id); the window
/// is not selected.
pub(super) fn break_pane(pane_id: &str, session: &str) -> Result<()> {
    let destination = format!("{session}:");
    runner::run_checked(&["break-pane", "-d", "-s", pane_id, "-t", &destination])
        .with_context(|| format!("cannot break pane {pane_id} into a new window of {session}"))?;
    Ok(())
}

/// The id and index of the window that `target` is in.
pub(super) fn window_identity(target: &str) -> Result<(String, u32)> {
    let format = format!("#{{window_id}}{FIELD_SEP}#{{window_index}}");
    let output = runner::run_checked(&["display-message", "-p", "-t", target, &format])
        .with_context(|| format!("cannot read the window of {target}"))?;
    parse_window_identity(&output).with_context(|| format!("cannot read the window of {target}"))
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
pub(super) fn tiled_marker_present(target: &str) -> Result<bool> {
    let output = runner::run_checked(&["show-options", "-w", "-q", "-t", target, TILED_MARKER])
        .with_context(|| format!("cannot read the tiled marker of the window of {target}"))?;
    Ok(output
        .lines()
        .any(|line| line.split_whitespace().next() == Some(TILED_MARKER)))
}

/// Set [`TILED_MARKER`] on the window that `target` is in.
pub(super) fn set_tiled_marker(target: &str) -> Result<()> {
    runner::run_checked(&["set-option", "-w", "-t", target, TILED_MARKER, "1"])
        .with_context(|| format!("cannot mark the window of {target} tiled"))?;
    Ok(())
}

/// Unset [`TILED_MARKER`] on the window that `target` is in. Unsetting an absent
/// option succeeds, so "cleared" and "never set" are one state.
pub(super) fn clear_tiled_marker(target: &str) -> Result<()> {
    runner::run_checked(&["set-option", "-w", "-u", "-t", target, TILED_MARKER])
        .with_context(|| format!("cannot clear the tiled marker of the window of {target}"))?;
    Ok(())
}

/// Every pane of the window `pane_id` (a pane or window target) is in, with its size
/// and registration, as tmux reports them now.
pub(super) fn measure(pane_id: &str) -> Result<WindowGeometry> {
    parse_measurement(pane_id, &read_measurement(pane_id)?)
}

/// The measurement's tmux call: the raw lines [`parse_measurement`] reads.
fn read_measurement(pane_id: &str) -> Result<String> {
    let format = [
        "#{window_id}".to_owned(),
        "#{pane_id}".to_owned(),
        "#{pane_width}".to_owned(),
        "#{pane_height}".to_owned(),
        option_format(RegistrationKey::Agent),
        option_format(RegistrationKey::Surface),
    ]
    .join(&FIELD_SEP.to_string());
    // `list-panes -t <pane>` lists the panes of that pane's window.
    runner::run_checked(&["list-panes", "-t", pane_id, "-F", &format])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_names_its_direction_target_size_and_command() {
        assert_eq!(
            split_window_args(
                "sleep 600",
                direction_flag(SplitDirection::Horizontal),
                Some(70),
                Some("%3")
            ),
            [
                "split-window",
                "-h",
                "-t",
                "%3",
                "-p",
                "70",
                "-P",
                "-F",
                "#{pane_id}",
                "sleep 600"
            ]
        );
        assert_eq!(
            split_window_args(
                "sleep 600",
                direction_flag(SplitDirection::Vertical),
                None,
                Some("%3")
            ),
            [
                "split-window",
                "-v",
                "-t",
                "%3",
                "-P",
                "-F",
                "#{pane_id}",
                "sleep 600"
            ]
        );
    }

    #[test]
    fn a_new_window_targets_the_session_id() {
        assert_eq!(
            new_window_args("sleep 600", Some("$2")),
            [
                "new-window",
                "-t",
                "$2",
                "-P",
                "-F",
                "#{pane_id}",
                "sleep 600"
            ]
        );
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
            removal_args("%5", Some("@2")),
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
        assert_eq!(removal_args("%5", None), ["kill-pane", "-t", "%5"]);
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
}
