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
    let output = tmux::run_checked_owned(&tmux_args)?;
    let rendered = crate::format::render_capture(&output, args.common.format);

    render_output(args.common.format, &pane, &rendered)
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
}
