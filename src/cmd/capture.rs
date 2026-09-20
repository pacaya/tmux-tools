use anyhow::Result;
use clap::Args;
use serde_json::json;
use tmux_tools_core::{format, format::Format, names, target, tmux};

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

    // tmux interprets `-S -0 -E -` as the visible-pane shorthand rather than a
    // zero-length range, so honour the explicit zero intent here.
    if args.lines == Some(0) && !args.all {
        let rendered = format::render_capture("", args.common.format);
        return render_output(args.common.format, &pane, &rendered, None);
    }

    // `--all` claims full history, but a pane on the alternate screen has no
    // tmux scrollback. The determination reads live pane state (`alternate_on`)
    // rather than any recorded surface, so a pane that entered or left the
    // alternate screen after launch is reported as it currently is. The flag
    // and the capture come from one tmux invocation, so a pane that switches
    // screens mid-request cannot report one state's stdout beside the other
    // state's ceiling.
    let (raw, scrollback_available) = if args.all {
        let combined = build_tmux_alternate_capture_args(&pane);
        let combined_out = tmux::run_checked_owned(&combined)?;
        let (flag, body) = split_alternate_and_capture(&combined_out);
        (body.to_owned(), Some(!parse_alternate_on(flag)))
    } else {
        let tmux_args = build_tmux_capture_args(&pane, args.lines, args.all);
        (tmux::run_checked_owned(&tmux_args)?, None)
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

/// True when the pane is currently on the alternate screen, where tmux keeps
/// no scrollback. Parses the live `alternate_on` pane variable. Shared with
/// `prompt`, whose completeness check reuses this live no-scrollback test.
pub(crate) fn parse_alternate_on(raw: &str) -> bool {
    raw.trim() == "1"
}

/// Split one combined `display-message`/`capture-pane` invocation's stdout into
/// the leading `alternate_on` flag and the capture body. The flag is the text
/// before the first newline, the body is everything after it.
fn split_alternate_and_capture(raw: &str) -> (&str, &str) {
    raw.split_once('\n').unwrap_or((raw, ""))
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

fn build_tmux_capture_args(pane: &str, lines: Option<u32>, all: bool) -> Vec<String> {
    let mut args = vec!["capture-pane".to_owned(), "-t".to_owned(), pane.to_owned()];
    args.extend(build_capture_args(lines, all));
    args
}

/// Build one tmux invocation that reads the live `alternate_on` flag and runs
/// the full-history capture, so both describe the same instant. tmux treats the
/// standalone `;` argument as its command separator.
fn build_tmux_alternate_capture_args(pane: &str) -> Vec<String> {
    let mut args = vec![
        "display-message".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        pane.to_owned(),
        "#{alternate_on}".to_owned(),
        ";".to_owned(),
    ];
    args.extend(build_tmux_capture_args(pane, None, true));
    args
}

pub(crate) fn build_capture_args(lines: Option<u32>, all: bool) -> Vec<String> {
    let mut args = vec!["-p".to_owned()];

    if all {
        args.push("-S".to_owned());
        args.push("-".to_owned());
    } else if let Some(lines) = lines {
        args.push("-S".to_owned());
        args.push(format!("-{lines}"));
        args.push("-E".to_owned());
        args.push("-".to_owned());
    }

    args
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
        assert_eq!(build_capture_args(None, false), vec!["-p"]);
    }

    #[test]
    fn zero_lines_is_not_treated_as_unset() {
        assert_eq!(
            build_capture_args(Some(0), false),
            vec!["-p", "-S", "-0", "-E", "-"]
        );
    }

    #[test]
    fn lines_captures_tail_range() {
        assert_eq!(
            build_capture_args(Some(5), false),
            vec!["-p", "-S", "-5", "-E", "-"]
        );
    }

    #[test]
    fn all_captures_from_start_of_history() {
        assert_eq!(build_capture_args(None, true), vec!["-p", "-S", "-"]);
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
            build_tmux_alternate_capture_args("%1"),
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
    fn split_alternate_and_capture_splits_at_first_newline() {
        let (flag, body) = split_alternate_and_capture("1\nalpha\nbeta\n");
        assert_eq!(flag, "1");
        assert_eq!(body, "alpha\nbeta\n");
    }

    #[test]
    fn split_alternate_and_capture_keeps_flag_like_body_lines() {
        let (flag, body) = split_alternate_and_capture("0\n1\n2\n");
        assert_eq!(flag, "0");
        assert_eq!(body, "1\n2\n");
    }

    #[test]
    fn split_alternate_and_capture_handles_a_missing_body() {
        let (flag, body) = split_alternate_and_capture("0\n");
        assert_eq!(flag, "0");
        assert_eq!(body, "");
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
