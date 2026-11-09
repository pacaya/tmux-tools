use anyhow::{anyhow, Result};
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::thread;
use std::time::Duration;
use tmux_tools_core::{
    format::{render_capture, strip_ansi, Format},
    host::{self, CaptureRange, HistoryProbe as PaneProbe, HostState, Submission},
    idle::{
        resolve_timeout, validate_seconds, wait_for_idle, IdleConfig, DEFAULT_READY_STABLE_SECONDS,
    },
    names, target,
};

use crate::{
    cmd::{hosted, send::dispatch_enter, wait_idle::surface_info_for},
    CommonArgs,
};

#[derive(Args, Debug)]
pub struct PromptArgs {
    #[arg(value_name = "TEXT")]
    pub(crate) text: String,
    #[arg(long, default_value_t = 2.0, value_name = "F")]
    pub(crate) idle_seconds: f64,
    #[arg(long, default_value_t = DEFAULT_READY_STABLE_SECONDS, value_name = "F")]
    pub(crate) ready_stable_seconds: f64,
    #[arg(
        long,
        value_name = "SEC",
        help = "Timeout in seconds [default: 120, env: TMUX_TOOLS_TIMEOUT]"
    )]
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
    one_submission_delivery: &'static str,
    /// P7 turn observation: whether `busy_regex` was seen between submission and settle.
    turn_observed: bool,
    /// P7 completeness: whether everything after the history mark could be supplied.
    complete: bool,
    host_state: Option<HostState>,
    submit: Submission,
}

/// The structured refusal reported for JSON output. The command still exits
/// non-zero; the field is how a programmatic caller sees *why* the refusal was
/// made instead of an undifferentiated transport error.
#[derive(Serialize)]
struct PromptRefusalJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    prompt_sent: &'a str,
    one_submission_delivery: &'static str,
    error: &'a str,
    /// A refusal submits nothing, so it carries no `submit`; the host state is not read.
    host_state: Option<HostState>,
}

/// P1's three reported outcomes. `Refused` carries the message so the caller can
/// report it before returning the non-zero exit.
#[derive(Debug)]
enum DeliveryOutcome {
    /// The pane's live `bracket_paste_flag` is set: the whole prompt is one paste.
    Guaranteed,
    /// The live flag is clear and the surface declares no capability, or the text is
    /// single-line: deliver, but do not claim one-submission delivery.
    Unavailable,
    /// The live flag is clear while the surface declares the capability, and the text
    /// is multi-line. Delivering would submit fragments, so nothing is sent.
    Refused(String),
}

impl DeliveryOutcome {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Guaranteed => "guaranteed",
            Self::Unavailable => "unavailable",
            Self::Refused(_) => "refused",
        }
    }
}

pub fn run(args: &PromptArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    // Resolve and compile the surface patterns and declared capability, and build the
    // full wait configuration, *before* capturing or sending anything. An invalid
    // `ready_regex`/`busy_regex` or a registry error must fail without delivering the
    // prompt, so a retry cannot submit it twice.
    let surface = surface_info_for(&pane)?;
    let declared_bracketed_paste = surface.as_ref().is_some_and(|info| info.bracketed_paste);
    let patterns = surface.and_then(|info| info.patterns);
    // `prompt` settles by idle detection only when the classifier cannot be used at all —
    // no surface, or a surface that supplies no patterns. With patterns present, an
    // `Unknown` capture (neither or both matching) is never treated as idle.
    let idle_on_unknown = patterns.is_none();
    let cfg = IdleConfig {
        idle_seconds: validate_seconds(args.idle_seconds, "idle-seconds")?,
        poll_interval: Duration::from_millis(250),
        timeout: resolve_timeout(args.timeout, "timeout")?,
        surface: patterns,
        ready_stable_seconds: validate_seconds(args.ready_stable_seconds, "ready-stable-seconds")?,
        until_regex: args.until.as_deref().map(Regex::new).transpose()?,
        idle_on_unknown,
        host_state: None,
        until_decides_alone: crate::cmd::wait_idle::until_decides_alone(args.until.is_some()),
        // Turn observation counts Host state `working` (P9.3).
        observe_host_state: host::current().capabilities().native_state,
    };

    // A host that observes submission (P6.4) delivers through its own transport.
    if host::current().capabilities().submit_confirmation {
        return run_with_submission(args, &pane, declared_bracketed_paste, cfg);
    }

    // P7's history mark is fixed at the submission point (the cursor row) before anything
    // is submitted, and the pane is captured at the same instant so pre-submission output
    // can be told apart from the turn's own. The probe is taken before the live flag read,
    // so the flag is read as close to the paste as the refusal-before-load ordering allows.
    let (before_probe, before_capture) = read_probe_and_capture(&pane)?;
    let mark = before_probe.mark();

    // P1's refusal clause: an expectation/reality mismatch that is already visible
    // refuses before anything is loaded into tmux. Empty text cannot be multi-line,
    // so it skips this and is handled by `send_prompt` below.
    if !args.text.is_empty() {
        if let DeliveryOutcome::Refused(message) =
            determine_delivery(&pane, declared_bracketed_paste, &args.text)?
        {
            report_refusal(args.common.format, &pane, &args.text, &message)?;
            return Err(anyhow!("{message}"));
        }
    }

    // The authoritative outcome comes from the flag read immediately before the
    // paste (or, for empty text, before the Enter); `send_prompt` returns it along
    // with every frame its verified-Enter delivery captured, so a busy footer that
    // was visible only during delivery still counts as the turn being observed.
    let (delivery, delivery_frames) = send_prompt(&pane, &args.text, declared_bracketed_paste)?;
    if let DeliveryOutcome::Refused(message) = &delivery {
        report_refusal(args.common.format, &pane, &args.text, message)?;
        return Err(anyhow!("{message}"));
    }
    let delivery_str = delivery.as_str();
    if matches!(args.common.format, Format::Raw | Format::Concise) {
        emit_delivery_notice(&pane, delivery_str);
    }

    let outcome = wait_for_idle(&pane, &cfg)?;

    // The mark is never advanced: the result begins there and always includes the
    // agent's echo and the turn's output. A redraw can place either above the mark, so
    // the pre-submission capture is used to detect content newer than the mark.
    let (after_probe, after_capture) = read_probe_and_capture(&pane)?;
    let evicted = history_evicted(&before_probe, &after_probe);
    let width_changed = before_probe.pane_width != after_probe.pane_width;
    // A pane that was on the alternate screen at the mark, at any idle-loop poll, or at
    // settle has no scrollback for that output, even if it left the screen before settling.
    let alternate_seen =
        before_probe.alternate_on || outcome.alternate_seen || after_probe.alternate_on;
    let complete = !alternate_seen && !evicted && !width_changed;

    let before_lines: Vec<&str> = before_capture.lines().collect();
    let after_lines: Vec<&str> = after_capture.lines().collect();
    let shift = if evicted {
        eviction_shift(&before_probe, &after_probe, &before_lines, &after_lines)
    } else {
        0
    };
    let output_since_prompt = extract_result(
        &before_lines,
        &after_lines,
        mark,
        after_probe.alternate_on,
        after_probe.history_size as usize,
        shift,
    );
    let delivery_busy = delivery_frames.iter().any(|frame| {
        cfg.surface
            .as_ref()
            .and_then(|patterns| patterns.busy_regex.as_ref())
            .is_some_and(|regex| regex.is_match(frame))
    });
    let turn_observed = delivery_busy || outcome.busy_seen;

    // Raw and concise have no structured fields, so both dimensions go to stderr; JSON
    // reports them as `turn_observed` and `complete`. They are stated on every run.
    if matches!(args.common.format, Format::Raw | Format::Concise) {
        emit_turn_observation(turn_observed);
        emit_completeness(complete, alternate_seen, evicted, width_changed);
    }

    match args.common.format {
        Format::Concise => {
            let rendered = render_capture(&output_since_prompt, Format::Concise);
            print!("{rendered}");
        }
        Format::Json => {
            let name = names::get(&pane, names::KEY_NAME)?;
            let rendered = render_capture(&output_since_prompt, Format::Concise);
            let host = host::current();
            let output = PromptJson {
                target: &pane,
                name: name.as_deref(),
                prompt_sent: &args.text,
                output_since_prompt: &rendered,
                reason: outcome.reason.as_str(),
                duration_ms: outcome.duration.as_millis(),
                one_submission_delivery: delivery_str,
                turn_observed,
                complete,
                host_state: host.host_state(&pane)?,
                // Without `submit_confirmation` the host gives no observation.
                submit: Submission::Assumed,
            };
            println!("{}", serde_json::to_string(&hosted(&output))?);
        }
        Format::Raw => print!("{output_since_prompt}"),
    }

    Ok(())
}

/// `prompt` on a host with `submit_confirmation` (Orca, P9.6). Delivery is determined
/// before any transmission; the prompt is one host send that observes its submission;
/// the History mark is the host's read cursor; Host state takes part in the settle; the
/// host reports no width or alternate-screen state, so Completeness is false (P9.3).
fn run_with_submission(
    args: &PromptArgs,
    pane: &str,
    declared_bracketed_paste: bool,
    mut cfg: IdleConfig,
) -> Result<()> {
    let host = host::current();
    let registered = names::read(pane)?;
    let guaranteed = match host.prompt_delivery_guaranteed(pane, registered.agent.as_deref()) {
        Ok(guaranteed) => guaranteed,
        Err(error) => {
            report_refusal(args.common.format, pane, &args.text, &format!("{error:#}"))?;
            return Err(error);
        }
    };
    let delivery = if guaranteed {
        DeliveryOutcome::Guaranteed
    } else if declared_bracketed_paste && args.text.contains('\n') {
        DeliveryOutcome::Refused(format!(
            "refusing to send a multi-line prompt to pane {pane}: the surface declares \
             bracketed-paste support but the host cannot guarantee one-submission delivery \
             (the agent is not reported live in the pane), so delivering could submit \
             fragments; wait for the agent to come up and retry"
        ))
    } else {
        DeliveryOutcome::Unavailable
    };
    if let DeliveryOutcome::Refused(message) = &delivery {
        report_refusal(args.common.format, pane, &args.text, message)?;
        return Err(anyhow!("{message}"));
    }

    let mark = host.history_cursor(pane)?;
    let submitted_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default();
    let submit = host.submit_prompt(pane, &args.text, cfg.timeout)?;
    let delivery_str = delivery.as_str();
    if matches!(args.common.format, Format::Raw | Format::Concise) {
        match delivery {
            DeliveryOutcome::Guaranteed => eprintln!(
                "tmux-tools: prompt: one-submission delivery guaranteed: the host reports the \
                 agent live in pane {pane}"
            ),
            _ => eprintln!(
                "tmux-tools: prompt: one-submission delivery unavailable: the host does not \
                 report the agent live in pane {pane}"
            ),
        }
    }

    cfg.host_state =
        crate::cmd::wait_idle::host_state_gate(args.until.is_some(), Some(submitted_ms));
    let outcome = wait_for_idle(pane, &cfg)?;
    let output_since_prompt = strip_ansi(&host.read_history_since(pane, &mark)?);
    let turn_observed = outcome.busy_seen;
    let complete = false;

    if matches!(args.common.format, Format::Raw | Format::Concise) {
        emit_turn_observation(turn_observed);
        eprintln!(
            "tmux-tools: prompt: completeness: incomplete: {} reports no pane width or \
             full-screen state",
            host.host()
        );
    }

    match args.common.format {
        Format::Concise => print!("{}", render_capture(&output_since_prompt, Format::Concise)),
        Format::Json => {
            let rendered = render_capture(&output_since_prompt, Format::Concise);
            let output = PromptJson {
                target: pane,
                name: registered.name.as_deref(),
                prompt_sent: &args.text,
                output_since_prompt: &rendered,
                reason: outcome.reason.as_str(),
                duration_ms: outcome.duration.as_millis(),
                one_submission_delivery: delivery_str,
                turn_observed,
                complete,
                host_state: host.host_state(pane)?,
                submit,
            };
            println!("{}", serde_json::to_string(&hosted(&output))?);
        }
        Format::Raw => print!("{output_since_prompt}"),
    }
    Ok(())
}

/// The live bracketed-paste state, read from the pane immediately before the
/// delivery decision. tmux only brackets a `paste-buffer -p` when the target
/// application has *currently* enabled bracketed paste, so this — not the
/// surface's declared capability — is what the one-submission guarantee keys on.
fn determine_delivery(
    pane: &str,
    declared_bracketed_paste: bool,
    text: &str,
) -> Result<DeliveryOutcome> {
    if read_bracket_paste_flag(pane)? {
        return Ok(DeliveryOutcome::Guaranteed);
    }

    if declared_bracketed_paste && text.contains('\n') {
        return Ok(DeliveryOutcome::Refused(refusal_message(pane)));
    }

    Ok(DeliveryOutcome::Unavailable)
}

fn read_bracket_paste_flag(pane: &str) -> Result<bool> {
    host::current().bracketed_paste_enabled(pane)
}

fn refusal_message(pane: &str) -> String {
    format!(
        "refusing to send a multi-line prompt to pane {pane}: the surface declares bracketed-paste \
         support but the live bracket_paste_flag is clear (the agent may still be starting, or the \
         pane has fallen back to its shell), so delivering would submit fragments; wait for \
         bracketed paste to come up and retry"
    )
}

/// Report the refusal on stderr (raw and concise) or as a JSON field, ahead of
/// the non-zero exit.
fn report_refusal(format: Format, pane: &str, text: &str, message: &str) -> Result<()> {
    if format == Format::Json {
        let name = names::get(pane, names::KEY_NAME)?;
        let output = PromptRefusalJson {
            target: pane,
            name: name.as_deref(),
            prompt_sent: text,
            one_submission_delivery: "refused",
            error: message,
            host_state: None,
        };
        println!("{}", serde_json::to_string(&hosted(&output))?);
    }
    Ok(())
}

/// Raw and concise output have no structured field, so the delivery outcome goes
/// to stderr; JSON reports it as `one_submission_delivery` instead.
fn emit_delivery_notice(pane: &str, delivery: &str) {
    match delivery {
        "guaranteed" => eprintln!(
            "tmux-tools: prompt: one-submission delivery guaranteed: pane {pane} has bracketed \
             paste enabled"
        ),
        _ => eprintln!(
            "tmux-tools: prompt: one-submission delivery unavailable: pane {pane} does not have \
             bracketed paste enabled"
        ),
    }
}

/// P7 turn observation for raw and concise, which have no structured field. JSON
/// reports the same fact as `turn_observed`.
fn emit_turn_observation(observed: bool) {
    if observed {
        eprintln!(
            "tmux-tools: prompt: turn observed: busy_regex seen between submission and settle"
        );
    } else {
        eprintln!(
            "tmux-tools: prompt: turn unobserved: busy_regex not seen between submission and settle"
        );
    }
}

/// P7 completeness for raw and concise, which have no structured field. JSON
/// reports the same fact as `complete`. The message names every condition that held.
fn emit_completeness(complete: bool, alternate_seen: bool, evicted: bool, width_changed: bool) {
    if complete {
        eprintln!("tmux-tools: prompt: completeness: complete");
        return;
    }

    let mut reasons: Vec<&str> = Vec::new();
    if alternate_seen {
        reasons.push("the pane has no scrollback (alternate screen)");
    }
    if evicted {
        reasons.push("history has evicted lines");
    }
    if width_changed {
        reasons.push("the pane's width changed between the mark and settle");
    }
    eprintln!(
        "tmux-tools: prompt: completeness: incomplete: {}",
        reasons.join(" and ")
    );
}

/// Sample the live pane state and the full history at one instant, so the probe's
/// numbers and the captured bytes describe the same instant. Returns the probe and the
/// stripped capture.
fn read_probe_and_capture(pane: &str) -> Result<(PaneProbe, String)> {
    let (probe, capture) = host::current().probe_history(pane)?;
    Ok((probe, strip_ansi(&capture)))
}

/// P7 eviction. When the running tmux provides `history_collected`, any increase between
/// the mark and settle is eviction. Otherwise the pane counts as evicted once
/// `history_size` reaches the floor tmux frees to when it trims history: it drops the
/// oldest tenth in one step, so an evicting pane never sits below
/// `history_limit - history_limit / 10`.
fn history_evicted(before: &PaneProbe, after: &PaneProbe) -> bool {
    match (before.history_collected, after.history_collected) {
        (Some(mark), Some(settle)) => settle > mark,
        _ => after.history_size >= after.history_limit - after.history_limit / 10,
    }
}

/// The number of lines eviction removed from the front of the before capture, used to
/// shift the mark. With `history_collected` it is the mark-to-settle delta; otherwise the
/// captures are aligned by their shared content. `0` when no shift can be found.
fn eviction_shift(
    before: &PaneProbe,
    after: &PaneProbe,
    before_lines: &[&str],
    after_lines: &[&str],
) -> usize {
    match (before.history_collected, after.history_collected) {
        (Some(marked), Some(settled)) => settled.saturating_sub(marked) as usize,
        _ => align_offset(before_lines, after_lines).unwrap_or(0),
    }
}

/// Where the after capture's retained history begins inside the before capture, found by
/// matching their shared content after eviction dropped the oldest lines. `None` when the
/// captures share too little to align (the caller then keeps the widest range).
fn align_offset(before: &[&str], after: &[&str]) -> Option<usize> {
    const WINDOW: usize = 4;
    if before.len() < WINDOW || after.len() < WINDOW {
        return None;
    }

    let mut best: Option<(usize, usize)> = None;
    for offset in 0..=before.len() - WINDOW {
        if before[offset..offset + WINDOW] != after[..WINDOW] {
            continue;
        }
        let mut matched = WINDOW;
        while offset + matched < before.len()
            && matched < after.len()
            && before[offset + matched] == after[matched]
        {
            matched += 1;
        }
        if best.map_or(true, |(_, best_matched)| matched > best_matched) {
            best = Some((offset, matched));
        }
    }

    best.map(|(offset, _)| offset)
}

/// P7's result: the content at and after the pre-submission mark. `mark` is the cursor
/// row's absolute line at submission, so the result begins at the agent's echo. When a
/// redraw puts the echo or turn output above the mark, the pre-submission capture reveals
/// the first changed line and the result starts there instead, keeping the newer content
/// even if unchanged pre-submission output comes with it. `eviction_shift` re-aligns the
/// before capture after eviction dropped the oldest lines. The alternate screen keeps no
/// tmux scrollback, so its result starts at the settle probe's `history_size` (the first
/// visible row) rather than the main-screen history `capture-pane -S -` still carries.
fn extract_result(
    before_lines: &[&str],
    after_lines: &[&str],
    mark: usize,
    alternate_on: bool,
    visible_start: usize,
    eviction_shift: usize,
) -> String {
    if alternate_on {
        let start = visible_start.min(after_lines.len());
        return join_lines(&after_lines[start..]);
    }

    let shifted_before = before_lines.get(eviction_shift..).unwrap_or(&[]);
    let common = common_prefix_lines(shifted_before, after_lines);
    let shifted_mark = mark.saturating_sub(eviction_shift);
    let start = shifted_mark.min(common).min(after_lines.len());
    join_lines(&after_lines[start..])
}

/// Join lines, dropping the visible pane's trailing blank rows so an empty turn reports
/// an empty result.
fn join_lines(lines: &[&str]) -> String {
    let mut end = lines.len();
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end == 0 {
        return String::new();
    }
    let mut result = lines[..end].join("\n");
    result.push('\n');
    result
}

/// How many leading lines `before` and `after` share, comparing line by line.
fn common_prefix_lines(before: &[&str], after: &[&str]) -> usize {
    let mut index = 0;
    while index < before.len() && index < after.len() && before[index] == after[index] {
        index += 1;
    }
    index
}

/// Deliver `text` as one paste, then submit it with Enter, returning the outcome
/// decided by the flag read immediately before the paste and every frame the verified
/// Enter delivery captured.
///
/// The text goes to tmux on stdin (`load-buffer -`), never to a temporary file,
/// and `paste-buffer -p` brackets it so embedded newlines reach the composer as
/// newlines rather than submissions. Empty text has nothing to load or paste, so
/// it keeps the historical behavior of submitting with Enter alone.
fn send_prompt(
    pane: &str,
    text: &str,
    declared_bracketed_paste: bool,
) -> Result<(DeliveryOutcome, Vec<String>)> {
    let outcome = if text.is_empty() {
        determine_delivery(pane, declared_bracketed_paste, text)?
    } else {
        deliver_prompt(pane, text, declared_bracketed_paste)?
    };

    if let DeliveryOutcome::Refused(_) = &outcome {
        return Ok((outcome, Vec::new()));
    }

    let delivery = dispatch_enter(
        true,
        true,
        || crate::cmd::send::send_enter(pane),
        // The whole visible pane, not just the bottom line: a busy footer that sits above
        // the cursor row (as the fixture's does) must still be observable during delivery.
        || capture_delivery_frame(pane),
        |ms| thread::sleep(Duration::from_millis(ms)),
    )?;
    Ok((outcome, delivery.captures))
}

/// The ANSI-stripped visible pane, used as an Enter-verification frame. `busy_regex` is
/// matched against the same stripped form the idle loop uses.
fn capture_delivery_frame(pane: &str) -> Result<String> {
    let output = host::current().capture(pane, CaptureRange::Visible)?;
    Ok(strip_ansi(&output))
}

/// Stage `text` for one bracketed paste, read the live bracketed-paste state again
/// immediately before pasting, and paste only when that final read allows it. A
/// refusal from the second read discards the staged text and returns without pasting.
fn deliver_prompt(
    pane: &str,
    text: &str,
    declared_bracketed_paste: bool,
) -> Result<DeliveryOutcome> {
    let mut decided = None;
    host::current().paste_text(pane, text, &mut || {
        let outcome = determine_delivery(pane, declared_bracketed_paste, text)?;
        let paste = !matches!(outcome, DeliveryOutcome::Refused(_));
        decided = Some(outcome);
        Ok(paste)
    })?;
    decided.ok_or_else(|| anyhow!("internal: the paste ran without a delivery decision"))
}

#[cfg(test)]
mod tests {

    use super::*;

    fn probe(
        history_size: u64,
        history_limit: u64,
        cursor_y: u64,
        alternate_on: bool,
        history_collected: Option<u64>,
    ) -> PaneProbe {
        PaneProbe {
            history_size,
            history_limit,
            cursor_y,
            alternate_on,
            pane_width: 80,
            history_collected,
        }
    }

    #[test]
    fn mark_is_history_size_plus_cursor_row() {
        assert_eq!(probe(100, 50000, 5, false, None).mark(), 105);
        assert_eq!(probe(0, 2000, 23, false, None).mark(), 23);
    }

    fn lines(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    #[test]
    fn extract_result_starts_at_the_mark_and_keeps_the_echo() {
        let before = lines("h0\nh1\nh2\n\n");
        let after = lines("h0\nh1\nh2\n$ prompt\necho\nresponse\n");

        assert_eq!(
            extract_result(&before, &after, 3, false, 0, 0),
            "$ prompt\necho\nresponse\n"
        );
    }

    #[test]
    fn extract_result_excludes_unchanged_pre_submission_output() {
        let before = lines("DISTINCTIVE\nh0\nh1\n\n");
        let after = lines("DISTINCTIVE\nh0\nh1\n$ prompt\nresponse\n");

        let result = extract_result(&before, &after, 3, false, 0, 0);
        assert_eq!(result, "$ prompt\nresponse\n");
        assert!(
            !result.contains("DISTINCTIVE"),
            "pre-submission output above the mark must be excluded; got {result:?}"
        );
    }

    #[test]
    fn extract_result_keeps_content_redrawn_above_the_mark() {
        // The composer row (index 1) is redrawn in place with the echo, so the echo
        // starts above the cursor-row mark at index 2.
        let before = lines("banner\n\n\n");
        let after = lines("banner\necho line one\necho line two\nresponse\n");

        assert_eq!(
            extract_result(&before, &after, 2, false, 0, 0),
            "echo line one\necho line two\nresponse\n"
        );
    }

    #[test]
    fn extract_result_alternate_starts_at_the_visible_screen() {
        // `capture-pane -S -` on the alternate screen still carries the main-screen
        // history (indices 0..2); the alternate rows begin at `history_size` (2).
        let before = lines("main one\nmain two\n");
        let after = lines("main one\nmain two\necho\nturn output\n");

        assert_eq!(
            extract_result(&before, &after, 999, true, 2, 0),
            "echo\nturn output\n"
        );
    }

    #[test]
    fn extract_result_is_empty_when_nothing_changed_after_the_mark() {
        let before = lines("banner\n\n\n");
        let after = lines("banner\n\n\n");

        assert_eq!(extract_result(&before, &after, 2, false, 0, 0), "");
    }

    #[test]
    fn extract_result_shifts_the_mark_past_evicted_lines() {
        // Eviction dropped PRE-1..PRE-3, so the after capture begins at PRE-4 and the
        // cursor-row mark at 6 shifts to 3; the echo then starts at 3.
        let before = lines("PRE-1\nPRE-2\nPRE-3\nPRE-4\nPRE-5\nPRE-6\n\n");
        let after = lines("PRE-4\nPRE-5\nPRE-6\nECHO\nturn\n");

        assert_eq!(
            extract_result(&before, &after, 6, false, 0, 3),
            "ECHO\nturn\n"
        );
    }

    #[test]
    fn align_offset_finds_the_evicted_prefix() {
        let before = lines("a\nb\nc\nd\ne\nf\ng\nh\n");
        let after = lines("d\ne\nf\ng\nh\nNEW\n");

        assert_eq!(align_offset(&before, &after), Some(3));
    }

    #[test]
    fn align_offset_returns_none_when_there_is_nothing_to_align() {
        let before = lines("a\nb\nc\nd\n");
        let after = lines("w\nx\ny\nz\n");

        assert_eq!(align_offset(&before, &after), None);
    }

    #[test]
    fn eviction_shift_uses_the_counter_delta_when_available() {
        let marked = probe(100, 1000, 5, false, Some(40));
        let settled = probe(100, 1000, 5, false, Some(47));

        assert_eq!(eviction_shift(&marked, &settled, &[], &[]), 7);
    }

    #[test]
    fn eviction_shift_aligns_captures_without_the_counter() {
        let before = lines("a\nb\nc\nd\ne\nf\n");
        let after = lines("c\nd\ne\nf\nNEW\n");
        let marked = probe(0, 1000, 0, false, None);
        let settled = probe(0, 1000, 0, false, None);

        assert_eq!(eviction_shift(&marked, &settled, &before, &after), 2);
    }

    #[test]
    fn history_evicted_uses_the_counter_when_the_tmux_provides_it() {
        let marked = probe(100, 1000, 5, false, Some(40));
        let grown = probe(100, 1000, 5, false, Some(41));
        let unchanged = probe(100, 1000, 5, false, Some(40));

        assert!(history_evicted(&marked, &grown));
        assert!(!history_evicted(&marked, &unchanged));
    }

    #[test]
    fn history_evicted_falls_back_to_the_ninety_percent_floor() {
        // history_limit 1000 frees at 900; below it is merely near the limit.
        let before = probe(0, 1000, 5, false, None);
        assert!(history_evicted(&before, &probe(900, 1000, 5, false, None)));
        assert!(history_evicted(&before, &probe(950, 1000, 5, false, None)));
        assert!(!history_evicted(&before, &probe(899, 1000, 5, false, None)));
    }
}
