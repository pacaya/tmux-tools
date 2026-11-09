use anyhow::Result;
use clap::Args;
use serde_json::json;
use tmux_tools_core::{
    format,
    format::Format,
    host::{self, CaptureRange},
    names, target,
};

use crate::CommonArgs;

#[derive(Args, Debug)]
pub struct CaptureArgs {
    #[arg(long, conflicts_with = "all", value_name = "N")]
    pub(crate) lines: Option<u32>,
    #[arg(long)]
    pub(crate) all: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

pub fn run(args: &CaptureArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    // A zero-line tail reads as the visible pane on tmux rather than a zero-length
    // range, so honour the explicit zero intent here.
    if args.lines == Some(0) && !args.all {
        let rendered = format::render_capture("", args.common.format);
        return render_output(args.common.format, &pane, &rendered, None);
    }

    // `--all` claims full history, but a pane on the alternate screen has no
    // scrollback. The determination reads live pane state rather than any
    // recorded surface, so a pane that entered or left the alternate screen
    // after launch is reported as it currently is. The flag and the capture
    // are read at one instant, so a pane that switches screens mid-request
    // cannot report one state's stdout beside the other state's ceiling.
    let host = host::current();
    let range = capture_range(args.lines, args.all);
    let (raw, scrollback_available) = if args.all {
        let screen = host.capture_with_screen(&pane, range)?;
        (screen.text, Some(!screen.alternate_on))
    } else {
        (host.capture(&pane, range)?, None)
    };
    let to_render = match (args.lines, args.all) {
        (Some(n), false) if n > 0 => tail_lines(&raw, n),
        _ => raw,
    };
    let rendered = format::render_capture(&to_render, args.common.format);

    // Raw and concise report the ceiling on stderr; JSON reports it through the
    // `scrollback_available` field instead.
    if scrollback_available == Some(false)
        && matches!(args.common.format, Format::Raw | Format::Concise)
    {
        emit_scrollback_notice(&pane);
    }

    render_output(args.common.format, &pane, &rendered, scrollback_available)
}

/// The structural ceiling notice for raw and concise output, where there is no
/// structured field to carry it. JSON reports the same fact as
/// `scrollback_available` and does not print this.
fn emit_scrollback_notice(pane: &str) {
    eprintln!(
        "tmux-tools: capture --all: full history is unavailable: pane {pane} is on the alternate screen and has no scrollback"
    );
}

/// Trim trailing blank/whitespace-only rows from a raw tmux capture, then
/// keep only the last `n` lines. Anchors the tail at the bottom of pane
/// content rather than the literal bottom of the visible buffer, so a sparse
/// pane with the cursor near the top doesn't return `n` blank rows.
fn tail_lines(raw: &str, n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    let mut lines: Vec<&str> = raw.lines().collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let start = lines.len().saturating_sub(n as usize);
    let mut out = lines[start..].join("\n");
    out.push('\n');
    out
}

/// The lines a capture reads: the whole history with `--all`, the last `--lines N`,
/// else the visible pane.
pub(crate) fn capture_range(lines: Option<u32>, all: bool) -> CaptureRange {
    if all {
        CaptureRange::All
    } else if let Some(lines) = lines {
        CaptureRange::Tail(lines)
    } else {
        CaptureRange::Visible
    }
}

fn render_output(
    format: Format,
    pane: &str,
    rendered: &str,
    scrollback_available: Option<bool>,
) -> Result<()> {
    match format {
        Format::Raw | Format::Concise => print!("{rendered}"),
        Format::Json => {
            let name = names::get(pane, names::KEY_NAME)?;
            let mut output = json!({
                "host": host::current().host(),
                "target": pane,
                "name": name,
                "output": rendered,
                "lines": rendered.lines().count(),
            });
            // Only `--all` makes a scrollback claim; the field is absent for a
            // visible-pane or `--lines N` capture.
            if let Some(available) = scrollback_available {
                output["scrollback_available"] = json!(available);
            }
            println!("{output}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lines_captures_visible_pane_only() {
        assert_eq!(capture_range(None, false), CaptureRange::Visible);
    }

    #[test]
    fn zero_lines_is_not_treated_as_unset() {
        assert_eq!(capture_range(Some(0), false), CaptureRange::Tail(0));
    }

    #[test]
    fn lines_captures_tail_range() {
        assert_eq!(capture_range(Some(5), false), CaptureRange::Tail(5));
    }

    #[test]
    fn all_captures_from_start_of_history() {
        assert_eq!(capture_range(None, true), CaptureRange::All);
    }

    #[test]
    fn tail_lines_takes_last_n_when_buffer_has_more() {
        assert_eq!(tail_lines("a\nb\nc\nd\ne\n", 3), "c\nd\ne\n");
    }

    #[test]
    fn tail_lines_returns_all_when_n_exceeds_buffer() {
        assert_eq!(tail_lines("a\nb\nc\nd\ne\n", 10), "a\nb\nc\nd\ne\n");
    }

    #[test]
    fn tail_lines_strips_trailing_blank_lines_before_tailing() {
        assert_eq!(
            tail_lines("alpha\nbeta\ngamma\n   \n\n\n", 2),
            "beta\ngamma\n"
        );
    }

    #[test]
    fn tail_lines_zero_returns_empty() {
        assert_eq!(tail_lines("a\nb\nc\n", 0), "");
    }

    #[test]
    fn tail_lines_all_blank_input_returns_empty() {
        assert_eq!(tail_lines("\n\n   \n", 5), "");
    }
}
