use anyhow::Result;
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::thread;
use std::time::Duration;

use crate::{
    cmd::send::dispatch_enter,
    cmd::wait_idle::ready_regex_for,
    format::{render_capture, strip_ansi, Format},
    idle::{resolve_timeout, validate_seconds, wait_for_idle, IdleConfig},
    names, target, tmux, CommonArgs,
};

#[derive(Args, Debug)]
pub struct PromptArgs {
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,
    #[arg(long, default_value_t = 2.0, value_name = "F")]
    pub(crate) idle_seconds: f64,
    #[arg(long, value_name = "SEC", help = "Timeout in seconds [default: 120, env: TMUX_TOOLS_TIMEOUT]")]
    pub(crate) timeout: Option<f64>,
    #[arg(long, value_name = "REGEX")]
    pub(crate) until: Option<String>,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct PromptJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    prompt_sent: &'a str,
    output_since_prompt: &'a str,
    reason: &'static str,
    duration_ms: u128,
}

pub fn run(args: &PromptArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    let before = capture_visible_stripped(&pane)?;
    send_prompt(&pane, &args.text)?;

    let cfg = IdleConfig {
        idle_seconds: validate_seconds(args.idle_seconds, "idle-seconds")?,
        poll_interval: Duration::from_millis(250),
        timeout: resolve_timeout(args.timeout, "timeout")?,
        ready_regex: ready_regex_for(&pane)?,
        until_regex: args.until.as_deref().map(Regex::new).transpose()?,
    };
    let outcome = wait_for_idle(&pane, &cfg)?;

    let after = capture_visible_stripped(&pane)?;
    let output_since_prompt = extract_after_prompt(&before, &after, &args.text);

    match args.common.format {
        Format::Concise => {
            let rendered = render_capture(&output_since_prompt, Format::Concise);
            print!("{rendered}");
        }
        Format::Json => {
            let name = names::get(&pane, names::KEY_NAME)?;
            let rendered = render_capture(&output_since_prompt, Format::Concise);
            let output = PromptJson {
                target: &pane,
                name: name.as_deref(),
                prompt_sent: &args.text,
                output_since_prompt: &rendered,
                reason: outcome.reason.as_str(),
                duration_ms: outcome.duration.as_millis(),
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => print!("{output_since_prompt}"),
    }

    Ok(())
}

fn capture_visible_stripped(pane: &str) -> Result<String> {
    let output = tmux::run_checked(&["capture-pane", "-t", pane, "-p"])?;
    Ok(strip_ansi(&output))
}

fn send_prompt(pane: &str, text: &str) -> Result<()> {
    tmux::run_checked(&["send-keys", "-t", pane, "-l", text])?;
    dispatch_enter(
        true,
        true,
        || crate::cmd::send::send_enter(pane),
        || crate::cmd::send::capture_last_line(pane),
        |ms| thread::sleep(Duration::from_millis(ms)),
    )?;
    Ok(())
}

fn extract_after_prompt(before: &str, after: &str, prompt_text: &str) -> String {
    // Find the line in `after` where the just-sent prompt was rendered, and
    // return everything below it.
    //
    // For TUI-style panes (codex, REPLs, etc.), `capture-pane -p` is visible-only
    // and always returns exactly pane_height lines — a line-count-based diff
    // between before/after collapses to empty even when new content was rendered.
    // Instead, locate the prompt text within `after`: prefer the FIRST occurrence
    // on a line that did NOT appear in `before` (that's the just-rendered prompt;
    // anything after it is the response). Fall back to the LAST occurrence
    // anywhere in `after` if no strictly-new line matches (e.g., the prompt text
    // happens to overlap a pre-existing pane line).
    let before_lines: std::collections::HashSet<&str> = before.lines().collect();

    let mut first_new_match_end: Option<usize> = None;
    let mut last_match_end: Option<usize> = None;
    let mut offset = 0usize;
    for line in after.split_inclusive('\n') {
        if line.contains(prompt_text) {
            let end = offset + line.len();
            last_match_end = Some(end);
            // A "new" match is a line that wasn't in `before` (compare without
            // the trailing newline, since lines() strips it).
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            if first_new_match_end.is_none() && !before_lines.contains(trimmed) {
                first_new_match_end = Some(end);
            }
        }
        offset += line.len();
    }

    first_new_match_end
        .or(last_match_end)
        .map(|i| after[i..].to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_found_in_after_returns_lines_after_first_match_past_boundary() {
        let before = "ready\n";
        let after = "ready\n> ask\nanswer\n";

        assert_eq!(extract_after_prompt(before, after, "ask"), "answer\n");
    }

    #[test]
    fn prompt_not_found_returns_empty() {
        // If the prompt text never appears in `after`, we have no anchor for
        // where the response begins, so return empty rather than guessing.
        let before = "one\ntwo\n";
        let after = "one\ntwo\nthree\nfour\n";

        assert_eq!(extract_after_prompt(before, after, "missing"), "");
    }

    #[test]
    fn same_line_count_visible_pane_capture_extracts_response() {
        // Regression: tmux `capture-pane -p` returns exactly pane_height lines
        // (visible-only, padded with blanks). Before and after will have the same
        // line count even when new content was rendered. Extraction must still
        // anchor on the prompt's own line.
        let before = "old line one\nold line two\nold line three\n\n\n";
        let after = "old line two\nold line three\n› What's 2+2?\n• 4\n\n";

        assert_eq!(
            extract_after_prompt(before, after, "What's 2+2?"),
            "• 4\n\n"
        );
    }

    #[test]
    fn before_equal_to_after_returns_empty() {
        let before = "one\ntwo\n";

        assert_eq!(extract_after_prompt(before, before, "missing"), "");
    }

    #[test]
    fn multi_line_output_captured_cleanly() {
        let before = "prompt\n";
        let after = "prompt\nline one\nline two\n\n";

        assert_eq!(
            extract_after_prompt(before, after, "prompt"),
            "line one\nline two\n\n"
        );
    }

    #[test]
    fn multiple_occurrences_uses_first_after_before_boundary() {
        // When the agent's response restates the prompt text, the FIRST line at or after
        // the before-snapshot boundary is the line where the prompt was actually sent.
        // Anything after that line — including a later restatement — is genuine output.
        let before = "";
        let after = "prompt\nfirst line\nprompt restated\nmore output\n";

        assert_eq!(
            extract_after_prompt(before, after, "prompt"),
            "first line\nprompt restated\nmore output\n"
        );
    }

    #[test]
    fn prompt_match_before_boundary_is_ignored() {
        // A pre-existing line that contains the prompt text must not be treated as
        // the location of the just-sent prompt — only matches at/after `before_line_count`
        // count as the prompt's own line.
        let before = "prompt history\n";
        let after = "prompt history\nresponse line\n";

        assert_eq!(
            extract_after_prompt(before, after, "prompt"),
            "response line\n"
        );
    }
}
