use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Args};
use regex::Regex;
use serde::Serialize;
use std::process;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::{
    cmd::spawn_agent::shell_quote,
    format::{strip_ansi, Format},
    idle::resolve_timeout,
    names, target, tmux, CommonArgs,
};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOOKBACKS: [Option<u32>; 4] = [Some(100), Some(500), Some(2000), None];

#[derive(Args, Debug)]
pub struct ExecuteArgs {
    #[arg(value_name = "CMD")]
    pub(crate) cmd: String,
    #[arg(long, value_name = "SEC", help = "Timeout in seconds [default: 120, env: TMUX_TOOLS_TIMEOUT]")]
    pub(crate) timeout: Option<f64>,
    #[arg(long, action = ArgAction::SetTrue)]
    pub(crate) no_wait: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Debug)]
struct ExecuteOutcome {
    exit_code: Option<i32>,
    output: String,
    duration: Duration,
    timed_out: bool,
}

#[derive(Debug)]
struct CompletedOutput {
    exit_code: i32,
    output: String,
}

#[derive(Serialize)]
struct ExecuteStartedJson<'a> {
    target: &'a str,
    started: bool,
    start_marker: &'a str,
    end_marker: &'a str,
}

#[derive(Serialize)]
struct ExecuteJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    exit_code: Option<i32>,
    output: &'a str,
    duration_ms: u128,
    timed_out: bool,
}

pub fn run(args: &ExecuteArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    let timeout = resolve_timeout(args.timeout, "timeout")?;
    let (start_marker, end_marker) = make_markers()?;
    let wrapped = wrap_command(&args.cmd, &start_marker, &end_marker);
    send_wrapped_command(&pane, &wrapped)?;

    if args.no_wait {
        return render_started(args, &pane, &start_marker, &end_marker);
    }

    let outcome = wait_for_completion(&pane, &start_marker, &end_marker, timeout)?;
    render_outcome(args, &pane, &outcome)
}

fn make_markers() -> Result<(String, String)> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let suffix = format!("{}_{}", process::id(), nanos);

    Ok((
        format!("__TT_START_{suffix}__"),
        format!("__TT_END_{suffix}__"),
    ))
}

fn wrap_command(cmd: &str, start_marker: &str, end_marker: &str) -> String {
    // Hide marker values from the user command: emit them via printf with
    // single-quoted literals (no shell variables that the user could read or
    // overwrite). Run the user's command through a fresh `bash -c <quoted>`
    // so `;`, `)`, etc. cannot break out into the wrapping shell. Emit a
    // leading `\n` before the END marker so the terminator always starts at
    // column 0 of a new line, even when the user's output lacks a trailing
    // newline.
    let quoted_cmd = shell_quote(cmd);
    format!(
        "printf '%s\\n' '{start_marker}'; bash -c {quoted_cmd}; rc=$?; printf '\\n%s %s\\n' '{end_marker}' \"$rc\""
    )
}

fn send_wrapped_command(pane: &str, wrapped: &str) -> Result<()> {
    tmux::run_checked(&["send-keys", "-t", pane, wrapped, "Enter"])?;
    Ok(())
}

fn wait_for_completion(
    pane: &str,
    start_marker: &str,
    end_marker: &str,
    timeout: Duration,
) -> Result<ExecuteOutcome> {
    let started_at = Instant::now();
    let end_regex = Regex::new(&format!(
        r"(?m)^{}[ \t]+(-?\d+)\s*$",
        regex::escape(end_marker)
    ))?;
    let mut lookback_index = 0;
    let mut partial = String::new();

    loop {
        let raw = capture_pane(pane, LOOKBACKS[lookback_index])?;
        let stripped = strip_ansi(&raw);

        let start = find_start_line_end(&stripped, start_marker);

        if let Some(completed) = extract_completed_at(&stripped, start, &end_regex)? {
            return Ok(ExecuteOutcome {
                exit_code: Some(completed.exit_code),
                output: completed.output,
                duration: started_at.elapsed(),
                timed_out: false,
            });
        }

        let start_seen = start.is_some();
        let end_seen = end_regex.is_match(&stripped);
        if let Some(current_partial) = extract_partial_at(&stripped, start, &end_regex) {
            partial = current_partial;
        }

        let elapsed = started_at.elapsed();
        if elapsed >= timeout {
            return Ok(ExecuteOutcome {
                exit_code: None,
                output: partial,
                duration: elapsed,
                timed_out: true,
            });
        }

        let next_lookback =
            next_lookback_index(lookback_index, elapsed, timeout, end_seen && !start_seen);
        if next_lookback != lookback_index {
            lookback_index = next_lookback;
            continue;
        }

        thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(elapsed)));
    }
}

fn next_lookback_index(
    mut current: usize,
    elapsed: Duration,
    timeout: Duration,
    force: bool,
) -> usize {
    if current + 1 >= LOOKBACKS.len() {
        return current;
    }

    if force {
        return current + 1;
    }

    if timeout.is_zero() {
        return current;
    }

    let progress = elapsed.as_secs_f64() / timeout.as_secs_f64();
    while current + 1 < LOOKBACKS.len() && progress >= expansion_fraction(current) {
        current += 1;
    }

    current
}

fn expansion_fraction(index: usize) -> f64 {
    match index {
        0 => 0.25,
        1 => 0.50,
        2 => 0.75,
        _ => 1.0,
    }
}

fn capture_pane(pane: &str, lookback: Option<u32>) -> Result<String> {
    let mut args = vec!["capture-pane".to_owned(), "-t".to_owned(), pane.to_owned()];
    args.extend(crate::cmd::capture::build_capture_args(
        lookback,
        lookback.is_none(),
    ));
    tmux::run_checked_owned(&args)
}

fn extract_completed_at(
    capture: &str,
    start: Option<usize>,
    end_regex: &Regex,
) -> Result<Option<CompletedOutput>> {
    let Some(output_start) = start else {
        return Ok(None);
    };
    let after_start = &capture[output_start..];

    // Prefer the LAST match: any earlier coincidental occurrence in the user's
    // output (e.g. a forged echo of the marker) is overridden by the real
    // wrapper-emitted terminator.
    let Some(captures) = end_regex.captures_iter(after_start).last() else {
        return Ok(None);
    };
    let full_match = captures
        .get(0)
        .ok_or_else(|| anyhow!("end marker regex matched without a full match"))?;
    let exit_code = captures
        .get(1)
        .ok_or_else(|| anyhow!("end marker regex matched without an exit code"))?
        .as_str()
        .parse::<i32>()
        .context("failed to parse command exit code")?;

    Ok(Some(CompletedOutput {
        exit_code,
        output: after_start[..full_match.start()].to_owned(),
    }))
}

fn extract_partial_at(capture: &str, start: Option<usize>, end_regex: &Regex) -> Option<String> {
    let output_start = start?;
    let after_start = &capture[output_start..];

    // Mirror extract_completed_at: prefer the LAST match so partial output is
    // truncated at the real terminator (if one is present), not at an earlier
    // coincidental occurrence in the user's output.
    match end_regex.find_iter(after_start).last() {
        Some(end_match) => Some(after_start[..end_match.start()].to_owned()),
        None => Some(after_start.to_owned()),
    }
}

fn find_start_line_end(capture: &str, start_marker: &str) -> Option<usize> {
    let mut offset = 0;

    for line in capture.split_inclusive('\n') {
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        if content.trim_end() == start_marker {
            return Some(offset + line.len());
        }
        offset += line.len();
    }

    None
}

fn render_started(
    args: &ExecuteArgs,
    pane: &str,
    start_marker: &str,
    end_marker: &str,
) -> Result<()> {
    match args.common.format {
        Format::Json => {
            let output = ExecuteStartedJson {
                target: pane,
                started: true,
                start_marker,
                end_marker,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Concise | Format::Raw => println!("started"),
    }

    Ok(())
}

fn render_outcome(args: &ExecuteArgs, pane: &str, outcome: &ExecuteOutcome) -> Result<()> {
    match args.common.format {
        Format::Concise => {
            if outcome.timed_out {
                println!(
                    "timed_out=true duration={:.3}",
                    outcome.duration.as_secs_f64()
                );
            } else {
                let exit_code = outcome
                    .exit_code
                    .ok_or_else(|| anyhow!("completed command is missing an exit code"))?;
                println!(
                    "exit={exit_code} duration={:.3}",
                    outcome.duration.as_secs_f64()
                );
            }
            print!("{}", outcome.output);
        }
        Format::Json => {
            let name = names::get(pane, names::KEY_NAME)?;
            let output = ExecuteJson {
                target: pane,
                name: name.as_deref(),
                exit_code: outcome.exit_code,
                output: &outcome.output,
                duration_ms: outcome.duration.as_millis(),
                timed_out: outcome.timed_out,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => print!("{}", outcome.output),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end_regex_for(end_marker: &str) -> Regex {
        Regex::new(&format!(
            r"(?m)^{}[ \t]+(-?\d+)\s*$",
            regex::escape(end_marker)
        ))
        .expect("end-marker regex must compile")
    }

    #[test]
    fn wrap_command_uses_printf_literals_and_bash_subshell() {
        let wrapped = wrap_command("ls /tmp", "__TT_START_X__", "__TT_END_X__");
        // printf-quoted literal markers; no $START/$END variables in the parent shell.
        assert!(
            wrapped.starts_with("printf '%s\\n' '__TT_START_X__'; "),
            "expected printf-literal start emission, got: {wrapped}"
        );
        // User command runs through `bash -c <single-quoted>` so it cannot
        // break out into the wrapping shell.
        assert!(
            wrapped.contains("bash -c 'ls /tmp'"),
            "expected bash -c with single-quoted user cmd, got: {wrapped}"
        );
        // Exit code is captured into a local `rc` variable, not exposed as $?
        // after the user can intervene.
        assert!(
            wrapped.contains("rc=$?;"),
            "expected rc=$? exit-code capture, got: {wrapped}"
        );
        // END marker is emitted with a leading newline so the terminator
        // always begins at column 0 of a fresh line.
        assert!(
            wrapped.contains("printf '\\n%s %s\\n' '__TT_END_X__' \"$rc\""),
            "expected leading-newline printf for END marker, got: {wrapped}"
        );
        // No `START=`/`END=` variable assignments leak the marker values.
        assert!(
            !wrapped.contains("START=") && !wrapped.contains("END="),
            "wrapper must not expose marker values via shell variables: {wrapped}"
        );
    }

    #[test]
    fn wrap_command_quotes_single_quotes_in_user_command() {
        // Single quotes in the user command must be POSIX-quoted; the user
        // cannot break out of the bash -c invocation.
        let wrapped = wrap_command("echo 'hi'", "__TT_START__", "__TT_END__");
        assert!(
            wrapped.contains(r#"bash -c 'echo '"'"'hi'"'"''"#),
            "expected POSIX-quoted user cmd, got: {wrapped}"
        );
    }

    #[test]
    fn end_regex_matches_only_at_line_start() {
        let end = "__TT_END_ABC__";
        let regex = end_regex_for(end);

        // Real terminator on its own line: matches.
        assert!(regex.is_match(&format!("\n{end} 0\n")));
        // Embedded mid-line (e.g. as part of user output): does NOT match.
        assert!(!regex.is_match(&format!("garbage {end} 99 trailing\n")));
        // Embedded after non-newline prefix on the same line: does NOT match.
        assert!(!regex.is_match(&format!("foo {end} 5\n")));
    }

    #[test]
    fn end_regex_iter_returns_last_match() {
        let end = "__TT_END_ABC__";
        let regex = end_regex_for(end);
        let body = format!("{end} 1\n{end} 2\n{end} 3\n");
        let last = regex
            .captures_iter(&body)
            .last()
            .expect("at least one match");
        assert_eq!(last.get(1).unwrap().as_str(), "3");
    }

    #[test]
    fn extract_completed_ignores_fake_marker_not_at_line_start() {
        let start = "__TT_START_ABC__";
        let end = "__TT_END_ABC__";
        let regex = end_regex_for(end);

        // User output contains a fake "<END> 99" embedded mid-line; the real
        // terminator is on its own line afterward with exit code 0.
        let capture = format!(
            "prompt$ run\n{start}\nuser line with fake {end} 99 inside\nmore output\n{end} 0\n"
        );

        let start_pos = find_start_line_end(&capture, start);
        let completed = extract_completed_at(&capture, start_pos, &regex)
            .expect("extract_completed_at should not error")
            .expect("expected a completed match");
        assert_eq!(
            completed.exit_code, 0,
            "must use the real terminator, not the embedded fake; output was: {:?}",
            completed.output
        );
        // The captured body must contain the fake-marker line (it was part of
        // the user's output) but must NOT contain the real terminator line.
        assert!(completed.output.contains("fake __TT_END_ABC__ 99 inside"));
        assert!(!completed.output.contains(&format!("\n{end} 0")));
    }

    #[test]
    fn extract_completed_picks_last_real_marker() {
        let start = "__TT_START_ABC__";
        let end = "__TT_END_ABC__";
        let regex = end_regex_for(end);

        // Two marker-shaped lines at column 0; extract_completed must pick
        // the LAST one so any earlier coincidental occurrence in user output
        // (e.g. an `echo` of a real-looking marker) does not win.
        let capture = format!(
            "prompt$ run\n{start}\nfirst chunk of output\n{end} 99\nsecond chunk\n{end} 0\n"
        );

        let start_pos = find_start_line_end(&capture, start);
        let completed = extract_completed_at(&capture, start_pos, &regex)
            .expect("extract_completed_at should not error")
            .expect("expected a completed match");
        assert_eq!(
            completed.exit_code, 0,
            "must take the LAST marker, not the first"
        );
        // Output must include the first (forged) marker line as part of the body.
        assert!(completed.output.contains(&format!("{end} 99")));
    }

    #[test]
    fn extract_completed_returns_none_without_start_marker() {
        let regex = end_regex_for("__TT_END_X__");
        let capture = "just some output\n";
        let start_pos = find_start_line_end(capture, "__TT_START_X__");
        let result = extract_completed_at(capture, start_pos, &regex)
            .expect("extract_completed_at should not error");
        assert!(result.is_none());
    }

    #[test]
    fn extract_completed_returns_none_without_end_marker() {
        let start = "__TT_START_X__";
        let regex = end_regex_for("__TT_END_X__");
        let capture = format!("{start}\nrunning, not done yet\n");
        let start_pos = find_start_line_end(&capture, start);
        let result = extract_completed_at(&capture, start_pos, &regex)
            .expect("extract_completed_at should not error");
        assert!(result.is_none());
    }
}
