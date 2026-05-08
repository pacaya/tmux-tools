use anyhow::Result;
use clap::Args;
use serde_json::json;

use crate::{format::Format, names, target, tmux, CommonArgs};

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
        let rendered = crate::format::render_capture("", args.common.format);
        return render_output(args.common.format, &pane, &rendered);
    }

    let tmux_args = build_tmux_capture_args(&pane, args.lines, args.all);
    let raw = tmux::run_checked_owned(&tmux_args)?;
    let to_render = match (args.lines, args.all) {
        (Some(n), false) if n > 0 => tail_lines(&raw, n),
        _ => raw,
    };
    let rendered = crate::format::render_capture(&to_render, args.common.format);

    render_output(args.common.format, &pane, &rendered)
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

fn render_output(format: Format, pane: &str, rendered: &str) -> Result<()> {
    match format {
        Format::Raw | Format::Concise => print!("{rendered}"),
        Format::Json => {
            let name = names::get(pane, names::KEY_NAME)?;
            let output = json!({
                "target": pane,
                "name": name,
                "output": rendered,
                "lines": rendered.lines().count(),
            });
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
