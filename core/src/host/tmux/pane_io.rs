//! Capture, send, and the Bracketed paste transport on tmux.

use anyhow::{anyhow, bail, Result};
use std::time::{SystemTime, UNIX_EPOCH};

use super::runner;
use crate::host::{CaptureRange, HistoryProbe, ScreenCapture};

/// `capture-pane -t <pane> -p` with the arguments that select `range`.
fn capture_args(pane: &str, range: CaptureRange) -> Vec<String> {
    let mut args = vec![
        "capture-pane".to_owned(),
        "-t".to_owned(),
        pane.to_owned(),
        "-p".to_owned(),
    ];
    match range {
        CaptureRange::Visible => {}
        CaptureRange::Tail(lines) => {
            args.push("-S".to_owned());
            args.push(format!("-{lines}"));
            args.push("-E".to_owned());
            args.push("-".to_owned());
        }
        CaptureRange::All => {
            args.push("-S".to_owned());
            args.push("-".to_owned());
        }
        CaptureRange::Rows { start, end } => {
            args.push("-S".to_owned());
            args.push(start.to_string());
            args.push("-E".to_owned());
            args.push(end.to_string());
        }
    }
    args
}

/// One tmux invocation that reads the live `alternate_on` flag and runs the capture,
/// so both describe the same instant. tmux treats the standalone `;` argument as its
/// command separator.
fn screen_capture_args(pane: &str, range: CaptureRange) -> Vec<String> {
    let mut args = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        pane.to_owned(),
        "#{alternate_on}".to_owned(),
        ";".to_owned(),
    ];
    args.extend(capture_args(pane, range));
    args
}

/// Split one combined `display-message`/`capture-pane` invocation's stdout into the
/// leading `alternate_on` flag and the capture body. The flag is the text before the
/// first newline, the body is everything after it.
fn split_screen_capture(raw: &str) -> ScreenCapture {
    let (flag, body) = raw.split_once('\n').unwrap_or((raw, ""));
    ScreenCapture {
        alternate_on: parse_alternate_on(flag),
        text: body.to_owned(),
    }
}

/// True when the pane is on the alternate screen, where tmux keeps no scrollback.
fn parse_alternate_on(raw: &str) -> bool {
    raw.trim() == "1"
}

pub(super) fn capture(pane: &str, range: CaptureRange) -> Result<String> {
    runner::run_checked_owned(&capture_args(pane, range))
}

pub(super) fn capture_with_screen(pane: &str, range: CaptureRange) -> Result<ScreenCapture> {
    let raw = runner::run_checked_owned(&screen_capture_args(pane, range))?;
    Ok(split_screen_capture(&raw))
}

pub(super) fn poll_screen(pane: &str) -> Result<ScreenCapture> {
    let args = screen_capture_args(pane, CaptureRange::Visible);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = runner::run(&refs)?;
    if output.exit_code != 0 {
        bail!(
            "tmux capture-pane failed with exit code {}: {}",
            output.exit_code,
            output.stderr.trim_end()
        );
    }
    Ok(split_screen_capture(&output.stdout))
}

pub(super) fn capture_ansi(pane: &str, start: Option<i64>, end: Option<i64>) -> Result<String> {
    runner::run_checked_owned(&capture_ansi_args(pane, start, end))
}

fn capture_ansi_args(pane_id: &str, start: Option<i64>, end: Option<i64>) -> Vec<String> {
    let mut args = vec![
        "capture-pane".to_owned(),
        "-e".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        pane_id.to_owned(),
    ];

    if let Some(start) = start {
        args.push("-S".to_owned());
        args.push(start.to_string());
    }
    if let Some(end) = end {
        args.push("-E".to_owned());
        args.push(end.to_string());
    }

    args
}

/// The format string that samples the History probe. `#{history_collected}` expands to
/// an empty string on a tmux that does not provide the counter, which is how its
/// support is detected at runtime (a numeric sixth field means supported), never from
/// a version. It stays last so its absence cannot shift the fields before it.
const PROBE_FORMAT: &str = "#{history_size} #{history_limit} #{cursor_y} #{alternate_on} \
                            #{pane_width} #{history_collected}";

/// Sample the live pane state and the full `capture-pane -S -` history in one tmux
/// invocation, so the probe's numbers and the captured bytes describe the same instant.
/// The first line is the probe; the rest is the capture.
pub(super) fn probe_history(pane: &str) -> Result<(HistoryProbe, String)> {
    let combined = runner::run_checked(&[
        "display-message",
        "-p",
        "-t",
        pane,
        PROBE_FORMAT,
        ";",
        "capture-pane",
        "-t",
        pane,
        "-p",
        "-S",
        "-",
    ])?;
    let (summary, capture) = combined.split_once('\n').unwrap_or((combined.as_str(), ""));
    let probe = parse_probe(summary).ok_or_else(|| {
        anyhow!("tmux returned an unparseable pane probe for pane {pane}: {summary:?}")
    })?;
    Ok((probe, capture.to_owned()))
}

/// Parse the `history_size history_limit cursor_y alternate_on pane_width
/// [history_collected]` summary line. The trailing counter is absent on a tmux that does
/// not provide it.
fn parse_probe(summary: &str) -> Option<HistoryProbe> {
    let mut fields = summary.split_whitespace();
    let history_size = fields.next()?.parse().ok()?;
    let history_limit = fields.next()?.parse().ok()?;
    let cursor_y = fields.next()?.parse().ok()?;
    let alternate_on = parse_alternate_on(fields.next()?);
    let pane_width = fields.next()?.parse().ok()?;
    let history_collected = fields.next().and_then(|value| value.parse().ok());
    Some(HistoryProbe {
        history_size,
        history_limit,
        cursor_y,
        alternate_on,
        pane_width,
        history_collected,
    })
}

pub(super) fn start_output_pipe(pane: &str, command: &str) -> Result<()> {
    runner::run_checked(&["pipe-pane", "-t", pane, command])?;
    Ok(())
}

pub(super) fn stop_output_pipe(pane: &str) -> Result<()> {
    runner::run(&["pipe-pane", "-t", pane])?;
    Ok(())
}

pub(super) fn send_keys(pane: &str, keys: &[&str]) -> Result<()> {
    let mut args = vec!["send-keys", "-t", pane];
    args.extend_from_slice(keys);
    runner::run_checked(&args)?;
    Ok(())
}

pub(super) fn send_text(pane: &str, text: &str) -> Result<()> {
    runner::run_checked(&["send-keys", "-t", pane, "-l", text])?;
    Ok(())
}

/// The live bracketed-paste state of the application in `pane`. tmux only brackets a
/// `paste-buffer -p` when the application has *currently* enabled bracketed paste.
pub(super) fn bracketed_paste_enabled(pane: &str) -> Result<bool> {
    let value =
        runner::run_checked(&["display-message", "-p", "-t", pane, "#{bracket_paste_flag}"])?;
    Ok(value.trim() == "1")
}

/// The Bracketed paste transport: load `text` into a uniquely-named buffer from stdin
/// (`load-buffer -`, never a temporary file), run `confirm` immediately before pasting,
/// and paste with `paste-buffer -p -d` only when it allows it. The buffer is deleted on
/// every path after a successful load; cleanup never replaces the transport error.
pub(super) fn paste_text(
    pane: &str,
    text: &str,
    confirm: &mut dyn FnMut() -> Result<bool>,
) -> Result<()> {
    let buffer = unique_prompt_buffer_name();
    runner::run_checked_with_stdin(&["load-buffer", "-b", &buffer, "-"], text.as_bytes())?;

    match confirm() {
        // The buffer is already loaded, so cleanup must run even when the
        // confirmation fails. The error is returned unchanged.
        Err(error) => {
            let _ = runner::run_checked(&["delete-buffer", "-b", &buffer]);
            Err(error)
        }
        Ok(false) => {
            let _ = runner::run_checked(&["delete-buffer", "-b", &buffer]);
            Ok(())
        }
        Ok(true) => {
            let pasted =
                runner::run_checked(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", pane]);

            // Deletion on every path after a successful load: `-d` removed the buffer
            // on a successful paste, and this removes it when the paste failed.
            // Cleanup never replaces the transport error.
            let _ = runner::run_checked(&["delete-buffer", "-b", &buffer]);

            pasted?;
            Ok(())
        }
    }
}

/// A buffer name that cannot collide with another prompt's, or with a user's own
/// buffers, while naming the transport so a leaked buffer is recognizable.
fn unique_prompt_buffer_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("tmux-tools-prompt-{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_capture_reads_the_visible_pane_only() {
        assert_eq!(
            capture_args("%1", CaptureRange::Visible),
            ["capture-pane", "-t", "%1", "-p"]
        );
    }

    #[test]
    fn tail_capture_is_the_last_lines_to_the_bottom() {
        assert_eq!(
            capture_args("%1", CaptureRange::Tail(5)),
            ["capture-pane", "-t", "%1", "-p", "-S", "-5", "-E", "-"]
        );
        assert_eq!(
            capture_args("%1", CaptureRange::Tail(0)),
            ["capture-pane", "-t", "%1", "-p", "-S", "-0", "-E", "-"]
        );
    }

    #[test]
    fn all_captures_from_start_of_history() {
        assert_eq!(
            capture_args("%1", CaptureRange::All),
            ["capture-pane", "-t", "%1", "-p", "-S", "-"]
        );
    }

    #[test]
    fn rows_capture_names_both_bounds() {
        assert_eq!(
            capture_args("%1", CaptureRange::Rows { start: -1, end: -1 }),
            ["capture-pane", "-t", "%1", "-p", "-S", "-1", "-E", "-1"]
        );
    }

    #[test]
    fn parse_alternate_on_recognizes_the_live_flag() {
        assert!(parse_alternate_on("1\n"));
        assert!(!parse_alternate_on("0\n"));
        assert!(!parse_alternate_on(""));
    }

    #[test]
    fn all_reads_flag_and_capture_in_one_invocation() {
        assert_eq!(
            screen_capture_args("%1", CaptureRange::All),
            vec![
                "display-message",
                "-p",
                "-t",
                "%1",
                "#{alternate_on}",
                ";",
                "capture-pane",
                "-t",
                "%1",
                "-p",
                "-S",
                "-",
            ]
        );
    }

    #[test]
    fn split_screen_capture_splits_at_first_newline() {
        let capture = split_screen_capture("1\nalpha\nbeta\n");
        assert!(capture.alternate_on);
        assert_eq!(capture.text, "alpha\nbeta\n");
    }

    #[test]
    fn split_screen_capture_keeps_flag_like_body_lines() {
        let capture = split_screen_capture("0\n1\n2\n");
        assert!(!capture.alternate_on);
        assert_eq!(capture.text, "1\n2\n");
    }

    #[test]
    fn split_screen_capture_handles_a_missing_body() {
        let capture = split_screen_capture("0\n");
        assert!(!capture.alternate_on);
        assert_eq!(capture.text, "");
    }

    #[test]
    fn capture_ansi_default_builds_visible_pane_args() {
        assert_eq!(
            capture_ansi_args("%7", None, None),
            vec!["capture-pane", "-e", "-p", "-t", "%7"]
        );
    }

    #[test]
    fn capture_ansi_range_builds_history_args() {
        assert_eq!(
            capture_ansi_args("%7", Some(-100), Some(-1)),
            vec![
                "capture-pane",
                "-e",
                "-p",
                "-t",
                "%7",
                "-S",
                "-100",
                "-E",
                "-1",
            ]
        );
    }

    #[test]
    fn parse_probe_reads_the_five_fields_when_the_counter_is_absent() {
        let parsed = parse_probe("12 50000 8 1 80").expect("probe parses");

        assert_eq!(parsed.history_size, 12);
        assert_eq!(parsed.history_limit, 50000);
        assert_eq!(parsed.cursor_y, 8);
        assert!(parsed.alternate_on);
        assert_eq!(parsed.history_collected, None);
    }

    #[test]
    fn parse_probe_reads_the_counter_when_the_tmux_provides_it() {
        let parsed = parse_probe("12 50000 8 0 80 34").expect("probe parses");

        assert!(!parsed.alternate_on);
        assert_eq!(parsed.history_collected, Some(34));
    }

    #[test]
    fn parse_probe_rejects_a_malformed_summary() {
        assert!(parse_probe("").is_none());
        assert!(parse_probe("12 50000 8").is_none());
        assert!(parse_probe("x 50000 8 0").is_none());
    }

    #[test]
    fn parse_probe_reads_the_pane_width_and_requires_it() {
        let parsed = parse_probe("12 50000 8 0 137 34").expect("probe parses");

        assert_eq!(parsed.pane_width, 137);
        assert_eq!(parsed.history_collected, Some(34));
        assert!(parse_probe("12 50000 8 0").is_none());
        assert!(parse_probe("12 50000 8 0 wide").is_none());
    }
}
