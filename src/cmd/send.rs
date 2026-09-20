use anyhow::Result;
use clap::{ArgAction, Args};
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tmux_tools_core::{format::Format, target, tmux};

use crate::CommonArgs;

#[derive(Args, Debug)]
pub struct SendArgs {
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,
    #[arg(long, action = ArgAction::SetTrue)]
    pub(crate) enter: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    pub(crate) literal: bool,
    /// Verify Enter delivery by capturing the bottom line and retry up to 3
    /// times if the line is unchanged. Off by default to avoid duplicate
    /// submissions to non-echoing programs (passwords, full-screen TUIs).
    #[arg(long, action = ArgAction::SetTrue)]
    pub(crate) verify: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct SendJson<'a> {
    target: &'a str,
    sent: &'a str,
    verified: bool,
    attempts: usize,
}

pub fn run(args: &SendArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    send_text(args, &pane)?;

    let delivery = dispatch_enter(
        args.enter,
        args.verify,
        || send_enter(&pane),
        || capture_last_line(&pane),
        |ms| thread::sleep(Duration::from_millis(ms)),
    )?;

    render_output(args, &pane, delivery.attempts, delivery.verified)
}

/// What one verified-Enter delivery did. `captures` are every pane frame the delivery
/// took — the pre-Enter frame plus each post-Enter frame — so a caller can observe state
/// that was visible only during delivery (e.g. a brief busy footer).
pub(crate) struct EnterDelivery {
    pub attempts: usize,
    pub verified: bool,
    pub captures: Vec<String>,
}

/// Pure dispatch logic with side effects injected as closures so the
/// verify-vs-no-verify behaviour can be unit-tested without tmux.
pub(crate) fn dispatch_enter<S, C, P>(
    enter: bool,
    verify: bool,
    mut send_enter_fn: S,
    mut capture_fn: C,
    mut sleep_fn: P,
) -> Result<EnterDelivery>
where
    S: FnMut() -> Result<()>,
    C: FnMut() -> Result<String>,
    P: FnMut(u64),
{
    if !enter {
        return Ok(EnterDelivery {
            attempts: 1,
            verified: true,
            captures: Vec::new(),
        });
    }

    if !verify {
        send_enter_fn()?;
        return Ok(EnterDelivery {
            attempts: 1,
            verified: true,
            captures: Vec::new(),
        });
    }

    sleep_fn(100);
    let before = capture_fn()?;
    let mut captures = vec![before.clone()];
    let mut attempts = 0;
    let mut verified = false;

    for attempt in 0..3 {
        attempts += 1;
        send_enter_fn()?;
        sleep_fn(200);

        let after = capture_fn()?;
        let changed = after != before;
        captures.push(after);
        if changed {
            verified = true;
            break;
        }

        if attempt < 2 {
            sleep_fn(200);
        }
    }

    Ok(EnterDelivery {
        attempts,
        verified,
        captures,
    })
}

fn send_text(args: &SendArgs, pane: &str) -> Result<()> {
    let mut tmux_args = vec!["send-keys", "-t", pane];
    if args.literal {
        tmux_args.push("-l");
    }
    tmux_args.push(args.text.as_str());

    tmux::run_checked(&tmux_args)?;
    Ok(())
}

pub(crate) fn send_enter(pane: &str) -> Result<()> {
    tmux::run_checked(&["send-keys", "-t", pane, "Enter"])?;
    Ok(())
}

pub(crate) fn capture_last_line(pane: &str) -> Result<String> {
    tmux::run_checked(&["capture-pane", "-t", pane, "-p", "-S", "-1", "-E", "-1"])
}

fn render_output(args: &SendArgs, pane: &str, attempts: usize, verified: bool) -> Result<()> {
    match args.common.format {
        Format::Json => {
            let output = SendJson {
                target: pane,
                sent: &args.text,
                verified,
                attempts,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Concise | Format::Raw => {
            println!("sent attempts={attempts} verified={verified}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Run `dispatch_enter` against an unchanging captured line and count how
    /// many times Enter is sent. The capture closure always returns the same
    /// string, simulating a non-echoing program (password prompt, TUI).
    fn count_enter_sends(enter: bool, verify: bool) -> (usize, bool, usize) {
        let send_count = RefCell::new(0_usize);
        let result = dispatch_enter(
            enter,
            verify,
            || {
                *send_count.borrow_mut() += 1;
                Ok(())
            },
            || Ok::<String, anyhow::Error>("static-bottom-line".to_owned()),
            |_ms| {},
        )
        .expect("dispatch_enter should not error");
        let count = *send_count.borrow();
        (result.attempts, result.verified, count)
    }

    #[test]
    fn no_enter_flag_sends_zero_enters_and_reports_verified_one_attempt() {
        // When --enter is absent, the Enter pathway is skipped entirely;
        // attempts is reported as 1 (the text send itself) and verified=true.
        let (attempts, verified, sends) = count_enter_sends(false, false);
        assert_eq!(sends, 0, "no Enter should be sent without --enter");
        assert_eq!(attempts, 1);
        assert!(verified);
    }

    #[test]
    fn enter_without_verify_sends_exactly_one_enter_even_when_line_unchanged() {
        // Without --verify, the verify-and-retry loop must NOT run. Even
        // when the captured bottom line never changes (non-echoing
        // program), Enter is sent exactly once.
        let (attempts, verified, sends) = count_enter_sends(true, false);
        assert_eq!(
            sends, 1,
            "--enter without --verify must send Enter exactly once, got {sends}"
        );
        assert_eq!(attempts, 1);
        assert!(verified);
    }

    #[test]
    fn enter_with_verify_retries_up_to_three_times_when_line_unchanged() {
        // With --verify, the legacy retry loop runs and may send Enter up
        // to 3 times when verification keeps failing.
        let (attempts, verified, sends) = count_enter_sends(true, true);
        assert_eq!(
            sends, 3,
            "--enter --verify with unchanged line should retry to 3 sends"
        );
        assert_eq!(attempts, 3);
        assert!(
            !verified,
            "verification must report failure when line never changes"
        );
    }

    #[test]
    fn enter_with_verify_stops_after_first_observed_change() {
        // When the captured line changes after the first Enter, the loop
        // must short-circuit: 1 send, verified=true.
        let send_count = RefCell::new(0_usize);
        let captures = RefCell::new(vec!["before".to_owned(), "after".to_owned()]);
        let mut next_capture_index = 0_usize;
        let delivery = dispatch_enter(
            true,
            true,
            || {
                *send_count.borrow_mut() += 1;
                Ok(())
            },
            || {
                let captured = captures.borrow();
                let value = captured
                    .get(next_capture_index)
                    .cloned()
                    .unwrap_or_else(|| "after".to_owned());
                next_capture_index += 1;
                Ok::<String, anyhow::Error>(value)
            },
            |_ms| {},
        )
        .expect("dispatch_enter should not error");
        assert_eq!(*send_count.borrow(), 1);
        assert_eq!(delivery.attempts, 1);
        assert!(delivery.verified);
    }
}
