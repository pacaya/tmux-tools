use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use regex::Regex;
use xxhash_rust::xxh3::xxh3_64;

use crate::format::strip_ansi;
use crate::tmux;

#[derive(Clone, Debug)]
pub struct IdleConfig {
    pub idle_seconds: f64,
    pub poll_interval: Duration,
    pub timeout: Duration,
    /// The compiled readiness patterns of the pane's resolved surface. `None` when the
    /// pane has no surface, or its surface supplies no `ready_regex`/`busy_regex`; the
    /// loop then falls back to idle detection. When present, P3's classifier is the only
    /// source of pane state, and idle detection is the completion fallback only while the
    /// classifier returns `Unknown` — it is suppressed while the classifier returns
    /// `Busy` and is never a parallel definition of idleness.
    pub surface: Option<SurfacePatterns>,
    /// How long a ready match *or* `until_regex` match must hold continuously before
    /// completing (`ReadyMatched` / `UntilMatched`). Guards against a stale indicator that
    /// lingers for a single poll (e.g. a previous turn's `✓ done` / `· Ready ·` status line
    /// still visible right after a new prompt is submitted) triggering a premature, wrong
    /// completion. `0.0` fires on first match (legacy behavior; use for a one-shot marker
    /// that may scroll off-screen before the window elapses).
    pub ready_stable_seconds: f64,
    pub until_regex: Option<Regex>,
    /// Whether idle detection (a quiet capture) may settle the wait when P3's classifier
    /// returns `Unknown`. `wait-idle` keeps the historical fallback (`true`); `prompt` sets
    /// it `false` whenever the resolved surface supplies patterns, so a capture matching
    /// neither or both patterns never settles as idle. When the surface supplies no patterns
    /// (or the pane has no surface) `prompt` leaves it `true`, because idle detection is then
    /// the only completion signal.
    pub idle_on_unknown: bool,
}

pub const DEFAULT_READY_SCAN_LINES: usize = 1;
pub const DEFAULT_READY_STABLE_SECONDS: f64 = 2.0;

/// P3: the three-valued pane state. `Unknown` is a distinct outcome, never a synonym
/// for `Busy`; consumers that need certainty must branch on it explicitly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneState {
    Idle,
    Busy,
    Unknown,
}

/// The compiled readiness patterns a resolved surface supplies. Compiling happens once
/// when the surface is resolved, so the classifier stays pure and cheap to call per poll.
#[derive(Clone, Debug, Default)]
pub struct SurfacePatterns {
    pub ready_regex: Option<Regex>,
    /// How many of the bottom non-blank lines `ready_regex` is tested against. `1` is the
    /// default (bottom line only); `0` means "no limit" — scan every non-blank line.
    pub ready_scan_lines: usize,
    pub busy_regex: Option<Regex>,
}

impl SurfacePatterns {
    /// Whether this surface supplies no patterns at all, and so cannot classify a pane
    /// beyond `Unknown`.
    pub fn is_empty(&self) -> bool {
        self.ready_regex.is_none() && self.busy_regex.is_none()
    }
}

/// P3: the one place pane state is derived. Total over the readiness-pattern truth table:
///
/// | patterns       | capture matches | result  |
/// |----------------|-----------------|---------|
/// | both present   | ready only      | idle    |
/// | both present   | busy only       | busy    |
/// | both present   | neither         | unknown |
/// | both present   | both            | unknown |
/// | busy absent    | ready           | idle    |
/// | busy absent    | not ready       | unknown |
/// | ready absent   | busy            | busy    |
/// | ready absent   | not busy        | unknown |
/// | neither present| any             | unknown |
pub fn classify(stripped: &str, patterns: &SurfacePatterns) -> PaneState {
    let ready = patterns
        .ready_regex
        .as_ref()
        .map(|_| ready_matches(stripped, &patterns.ready_regex, patterns.ready_scan_lines));
    let busy = patterns
        .busy_regex
        .as_ref()
        .map(|regex| regex.is_match(stripped));

    match (ready, busy) {
        (Some(true), Some(true)) => PaneState::Unknown,
        (Some(true), Some(false)) => PaneState::Idle,
        (Some(false), Some(true)) => PaneState::Busy,
        (Some(false), Some(false)) => PaneState::Unknown,
        (Some(true), None) => PaneState::Idle,
        (Some(false), None) => PaneState::Unknown,
        (None, Some(true)) => PaneState::Busy,
        (None, Some(false)) => PaneState::Unknown,
        (None, None) => PaneState::Unknown,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdleOutcome {
    pub reason: IdleReason,
    pub duration: Duration,
    pub idle_for: Duration,
    pub final_capture: String,
    /// P7 turn observation: whether the resolved surface's `busy_regex` was seen at any
    /// poll between submission and settle. Independent of `reason`: a busy pane that only
    /// ends at `--timeout` still reports the turn as observed. Always `false` when the
    /// surface supplies no `busy_regex`.
    pub busy_seen: bool,
    /// P7 completeness support: whether the pane was on the alternate screen at any poll.
    /// The alternate screen keeps no tmux scrollback, so a pane that was on it at any
    /// observed point cannot supply everything after the mark. Sampled alongside each
    /// capture, so it is seen even if the pane leaves the alternate screen before settle.
    pub alternate_seen: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdleReason {
    Idle,
    ReadyMatched,
    UntilMatched,
    TimedOut,
}

impl IdleReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IdleReason::Idle => "idle",
            IdleReason::ReadyMatched => "ready_matched",
            IdleReason::UntilMatched => "until_matched",
            IdleReason::TimedOut => "timed_out",
        }
    }
}

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

pub fn resolve_timeout(cli_value: Option<f64>, name: &str) -> Result<Duration> {
    if let Some(seconds) = cli_value {
        return validate_duration(seconds, name);
    }
    if let Some(timeout) = read_timeout_env() {
        return Ok(timeout);
    }
    Ok(DEFAULT_TIMEOUT)
}

pub fn validate_seconds(seconds: f64, name: &str) -> Result<f64> {
    if !seconds.is_finite() || seconds < 0.0 {
        bail!("{name} must be a finite non-negative number of seconds");
    }
    Ok(seconds)
}

pub fn validate_duration(seconds: f64, name: &str) -> Result<Duration> {
    validate_seconds(seconds, name).map(Duration::from_secs_f64)
}

impl Default for IdleConfig {
    fn default() -> Self {
        Self {
            idle_seconds: 2.0,
            poll_interval: Duration::from_millis(250),
            timeout: Duration::from_secs(120),
            surface: None,
            ready_stable_seconds: DEFAULT_READY_STABLE_SECONDS,
            until_regex: None,
            idle_on_unknown: true,
        }
    }
}

pub fn wait_for_idle(pane_id: &str, cfg: &IdleConfig) -> Result<IdleOutcome> {
    let start = Instant::now();
    let mut last_change = start;
    let mut previous_hash = None;
    let mut capture_count = 0_u64;
    let mut busy_seen = false;
    let mut alternate_seen = false;
    let mut ready_debounce = MatchDebounce::default();
    let mut until_debounce = MatchDebounce::default();

    loop {
        // Sample the live `alternate_on` flag in the same tmux invocation as the capture,
        // so the flag and the bytes describe one instant. A pane on the alternate screen
        // has no scrollback, and it can leave the screen before settle; P7 completeness
        // must still know it was there, so the flag is tracked on every poll.
        let output = tmux::run(&[
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{alternate_on}",
            ";",
            "capture-pane",
            "-t",
            pane_id,
            "-p",
        ])?;
        if output.exit_code != 0 {
            bail!(
                "tmux capture-pane failed with exit code {}: {}",
                output.exit_code,
                output.stderr.trim_end()
            );
        }

        let (alternate_flag, capture) = output
            .stdout
            .split_once('\n')
            .unwrap_or((output.stdout.as_str(), ""));
        if alternate_flag.trim() == "1" {
            alternate_seen = true;
        }
        let stripped = strip_ansi(capture);
        // P7 turn observation: whether `busy_regex` was seen at any poll between
        // submission and settle. It is read from the pattern directly, not from the
        // classifier's outcome, so a busy match is still observed when both patterns
        // match (classifier `Unknown`).
        if cfg
            .surface
            .as_ref()
            .and_then(|patterns| patterns.busy_regex.as_ref())
            .is_some_and(|regex| regex.is_match(&stripped))
        {
            busy_seen = true;
        }
        let new_hash = xxh3_64(stripped.as_bytes());
        let now = Instant::now();

        if previous_hash != Some(new_hash) {
            last_change = now;
            previous_hash = Some(new_hash);
        }
        capture_count += 1;

        // `--until` is an explicit user-supplied terminator. Like `ready_regex` it must
        // hold continuously for `ready_stable_seconds` before firing, so a pattern that is
        // only transiently on screen — e.g. a status line still reading the previous turn's
        // `· Ready ·` for one poll right after a prompt is submitted — can't trigger a
        // premature completion. Set `--ready-stable-seconds 0` to fire on the first match
        // (for a one-shot marker that may scroll off-screen before the window elapses).
        let until_now = cfg
            .until_regex
            .as_ref()
            .is_some_and(|regex| regex.is_match(&stripped));
        if until_debounce.observe(until_now, now, cfg.ready_stable_seconds) {
            return Ok(IdleOutcome {
                reason: IdleReason::UntilMatched,
                duration: start.elapsed(),
                idle_for: now - last_change,
                final_capture: stripped,
                busy_seen,
                alternate_seen,
            });
        }

        // P3's classifier is the only place pane state is derived. When the resolved
        // surface supplies no patterns (or there is no surface) the state is `Unknown`,
        // which falls through to idle detection below.
        let state = cfg
            .surface
            .as_ref()
            .filter(|patterns| !patterns.is_empty())
            .map(|patterns| classify(&stripped, patterns))
            .unwrap_or(PaneState::Unknown);
        let ready_now = state == PaneState::Idle;

        // A ready match must hold continuously for `ready_stable_seconds` before
        // completing, so a stale indicator visible for a single poll can't trigger a
        // premature completion.
        if ready_debounce.observe(ready_now, now, cfg.ready_stable_seconds) {
            return Ok(IdleOutcome {
                reason: IdleReason::ReadyMatched,
                duration: start.elapsed(),
                idle_for: now - last_change,
                final_capture: stripped,
                busy_seen,
                alternate_seen,
            });
        }

        // Idle detection is the fallback for when the classifier cannot decide. `Unknown`
        // covers no patterns, no surface, and a capture that matches neither or both
        // patterns. A `Busy` classification suppresses it, so a static busy footer can only
        // end at `--timeout`; an `Idle` classification is completed by the ready debounce
        // above. `idle_on_unknown` gates the fallback: `wait-idle` allows it, while `prompt`
        // disallows it whenever the surface supplies patterns, so an unknown capture there
        // waits for a definite state or the timeout. Suppress it while an `--until` match is
        // pending its debounce: a static pane that is currently matching is exactly what
        // would trip `Idle` early when `idle_seconds <= ready_stable_seconds`.
        if state == PaneState::Unknown
            && cfg.idle_on_unknown
            && !until_now
            && capture_count >= 2
            && (now - last_change).as_secs_f64() >= cfg.idle_seconds
        {
            return Ok(IdleOutcome {
                reason: IdleReason::Idle,
                duration: start.elapsed(),
                idle_for: now - last_change,
                final_capture: stripped,
                busy_seen,
                alternate_seen,
            });
        }

        if start.elapsed() >= cfg.timeout {
            return Ok(IdleOutcome {
                reason: IdleReason::TimedOut,
                duration: start.elapsed(),
                idle_for: now - last_change,
                final_capture: stripped,
                busy_seen,
                alternate_seen,
            });
        }

        sleep(cfg.poll_interval);
    }
}

pub fn read_timeout_env() -> Option<Duration> {
    std::env::var("TMUX_TOOLS_TIMEOUT")
        .ok()
        .and_then(|s| parse_timeout(&s))
}

fn parse_timeout(s: &str) -> Option<Duration> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(Duration::from_secs_f64)
}

/// Whether the `ready_regex` matches any of the bottom `ready_scan_lines` non-blank
/// lines of the capture. Returns `false` when no `ready_regex` is configured.
fn ready_matches(stripped: &str, ready: &Option<Regex>, ready_scan_lines: usize) -> bool {
    ready.as_ref().is_some_and(|regex| {
        bottom_non_blank_lines(stripped, ready_scan_lines)
            .iter()
            .any(|line| regex.is_match(line))
    })
}

/// Tracks how long a match (a `ready_regex` or `--until` pattern) has held continuously
/// so completion can be debounced.
#[derive(Default)]
struct MatchDebounce {
    /// When the current uninterrupted run of matches began. `None` when the most
    /// recent observation was not a match.
    since: Option<Instant>,
}

impl MatchDebounce {
    /// Records one observation and returns whether a match has now held continuously for
    /// at least `stable_seconds`. A `false` observation resets the run.
    /// `stable_seconds == 0.0` fires on the first match (legacy behavior).
    fn observe(&mut self, matched: bool, now: Instant, stable_seconds: f64) -> bool {
        if !matched {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        (now - since).as_secs_f64() >= stable_seconds
    }
}

/// The bottom `count` non-blank lines, ordered top-to-bottom. Blank lines are skipped
/// so a trailing TUI status row's padding doesn't consume the budget.
///
/// `count == 0` means "no limit": every non-blank line of the capture is returned. With a
/// uniquely-anchored `ready_regex` this lets readiness match a status line no matter how
/// many task/footer rows render below it (the opt-in Claude status-line profile sets
/// `ready_lines = 0`). Prefer a small fixed window for patterns that aren't unique, since
/// an unbounded scan also reaches conversation text above the input box.
pub fn bottom_non_blank_lines(stripped: &str, count: usize) -> Vec<&str> {
    let limit = if count == 0 { usize::MAX } else { count };
    let mut lines: Vec<&str> = stripped
        .split('\n')
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(limit)
        .collect();
    lines.reverse();
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timeout_accepts_positive_finite_seconds() {
        assert_eq!(parse_timeout("5"), Some(Duration::from_secs(5)));
        assert_eq!(parse_timeout("3.5"), Some(Duration::from_secs_f64(3.5)));
        assert_eq!(parse_timeout("not a number"), None);
        assert_eq!(parse_timeout("-1"), None);
    }

    #[test]
    fn ready_matches_when_ready_regex_matches_bottom_line() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));

        assert!(ready_matches("earlier\nready>\n\n", &ready, 1));
    }

    #[test]
    fn ready_matches_within_scan_window_above_status_row() {
        let ready = Some(Regex::new(r"^\s*→").expect("test regex compiles"));

        // The `→` prompt sits two lines above the bottom status/footer rows.
        let pane = "  → Plan, search, build anything\n\n  Auto  /tmp\n";

        // Default depth of 1 only sees the status row and misses the prompt.
        assert!(!ready_matches(pane, &ready, 1));

        // A wider window reaches the prompt line.
        assert!(ready_matches(pane, &ready, 3));
    }

    #[test]
    fn ready_does_not_match_when_regex_absent_from_bottom_lines() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));

        assert!(!ready_matches("working\nstill working\n", &ready, 1));
    }

    #[test]
    fn ready_does_not_match_when_no_regex_configured() {
        assert!(!ready_matches("ready>\n", &None, 1));
    }

    #[test]
    fn ready_does_not_match_blank_capture() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));

        assert!(!ready_matches("\n  \n\t\n", &ready, 1));
    }

    /// The status-line `ready_regex` documented for a customized Claude Code install
    /// (see README "Detecting readiness from a custom Claude status line"). Exercised
    /// here as a literal pattern to lock the recommended config's semantics.
    const CLAUDE_STATUS_READY: &str = r"(✓ done|⏸ waiting) \| 🤖";

    #[test]
    fn claude_status_regex_matches_done_and_waiting_states() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        assert!(ready_matches(
            "✓ done | 🤖 opus-4.8 | ctx 42%\n",
            &ready,
            15
        ));
        assert!(ready_matches(
            "⏸ waiting | 🤖 opus-4.8 | ctx 42%\n",
            &ready,
            15
        ));
    }

    #[test]
    fn claude_status_regex_does_not_match_busy_states() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        assert!(!ready_matches(
            "⚡ working | 🤖 opus-4.8 | ctx 42%\n",
            &ready,
            15
        ));
        assert!(!ready_matches(
            "🔐 permission | 🤖 opus-4.8 | ctx 42%\n",
            &ready,
            15
        ));
        assert!(!ready_matches(
            "⚙ 3 bg (1 sub) | 🤖 opus-4.8 | ctx 42%\n",
            &ready,
            15
        ));
    }

    #[test]
    fn claude_status_regex_ignores_bare_done_in_conversation_text() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        // Conversation text mentioning "✓ done" without the ` | 🤖` status segment
        // must not false-match.
        assert!(!ready_matches(
            "The build is ✓ done now, all green.\n",
            &ready,
            15
        ));
    }

    #[test]
    fn claude_status_regex_matches_above_trailing_task_rows() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        // The status line sits above several task/subagent rows that render below it.
        let pane = "✓ done | 🤖 opus-4.8 | ctx 42%\n\
                    ⎿ task one running\n\
                    ⎿ task two running\n\
                    ⎿ task three running\n";

        // A narrow window only sees the trailing task rows and misses the status line.
        assert!(!ready_matches(pane, &ready, 2));

        // The documented `ready_lines = 15` window reaches the status line.
        assert!(ready_matches(pane, &ready, 15));
    }

    #[test]
    fn claude_status_regex_scan_lines_zero_matches_above_any_number_of_task_rows() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        // A status line followed by more trailing subagent rows than any fixed window
        // would cover — the case a variable-height footer produces in practice.
        let mut pane = String::from("✓ done | 🤖 opus-4.8 | ctx 42%\n");
        for i in 0..30 {
            pane.push_str(&format!("⎿ task {i} running\n"));
        }

        // Even a generous fixed window misses the status line once rows exceed it.
        assert!(!ready_matches(&pane, &ready, 15));
        // `ready_lines = 0` scans every non-blank line, so it still matches.
        assert!(ready_matches(&pane, &ready, 0));
    }

    #[test]
    fn claude_status_regex_scan_lines_zero_still_requires_status_anchor() {
        let ready = Some(Regex::new(CLAUDE_STATUS_READY).expect("test regex compiles"));

        // Whole-pane scan (0) over a busy pane whose conversation merely mentions
        // "✓ done" without the ` | 🤖` status segment must not false-match.
        let pane = "The build is ✓ done now, all green.\n\
                    ⚡ working | 🤖 opus-4.8 | ctx 42%\n\
                    ⎿ task running\n";
        assert!(!ready_matches(pane, &ready, 0));
    }

    #[test]
    fn ready_debounce_fires_only_after_stable_window() {
        let mut debounce = MatchDebounce::default();
        let t0 = Instant::now();

        // First match starts the run but does not fire yet.
        assert!(!debounce.observe(true, t0, 2.0));
        // Still within the window.
        assert!(!debounce.observe(true, t0 + Duration::from_millis(1_500), 2.0));
        // Held continuously for >= 2s: fires.
        assert!(debounce.observe(true, t0 + Duration::from_millis(2_000), 2.0));
    }

    #[test]
    fn ready_debounce_resets_on_non_match() {
        let mut debounce = MatchDebounce::default();
        let t0 = Instant::now();

        assert!(!debounce.observe(true, t0, 2.0));
        // A non-match resets the run.
        assert!(!debounce.observe(false, t0 + Duration::from_millis(1_500), 2.0));
        // The clock restarts from the next match, so 2.5s after t0 is still < 2s in.
        assert!(!debounce.observe(true, t0 + Duration::from_millis(2_500), 2.0));
        // Only after a fresh continuous 2s does it fire.
        assert!(debounce.observe(true, t0 + Duration::from_millis(4_600), 2.0));
    }

    #[test]
    fn ready_debounce_zero_seconds_fires_immediately() {
        let mut debounce = MatchDebounce::default();
        let t0 = Instant::now();

        assert!(debounce.observe(true, t0, 0.0));
    }

    #[test]
    fn default_config_uses_expected_values() {
        let cfg = IdleConfig::default();

        assert_eq!(cfg.idle_seconds, 2.0);
        assert_eq!(cfg.poll_interval, Duration::from_millis(250));
        assert_eq!(cfg.timeout, Duration::from_secs(120));
        assert_eq!(cfg.ready_stable_seconds, 2.0);
        assert!(cfg.surface.is_none());
        assert!(cfg.until_regex.is_none());
        assert!(cfg.idle_on_unknown);
    }

    fn patterns(
        ready: Option<&str>,
        ready_scan_lines: usize,
        busy: Option<&str>,
    ) -> SurfacePatterns {
        SurfacePatterns {
            ready_regex: ready.map(|pattern| Regex::new(pattern).expect("test regex compiles")),
            ready_scan_lines,
            busy_regex: busy.map(|pattern| Regex::new(pattern).expect("test regex compiles")),
        }
    }

    #[test]
    fn classify_both_patterns_ready_only_is_idle() {
        let patterns = patterns(Some("^READY$"), 1, Some("^BUSY$"));
        assert_eq!(classify("READY\n", &patterns), PaneState::Idle);
    }

    #[test]
    fn classify_both_patterns_busy_only_is_busy() {
        let patterns = patterns(Some("^READY$"), 1, Some("BUSY"));
        assert_eq!(classify("BUSY\n", &patterns), PaneState::Busy);
    }

    #[test]
    fn classify_both_patterns_neither_is_unknown() {
        let patterns = patterns(Some("^READY$"), 1, Some("^BUSY$"));
        assert_eq!(classify("something else\n", &patterns), PaneState::Unknown);
    }

    #[test]
    fn classify_both_patterns_both_is_unknown() {
        let patterns = patterns(Some("READY"), 1, Some("BUSY"));
        assert_eq!(classify("READY and BUSY\n", &patterns), PaneState::Unknown);
    }

    #[test]
    fn classify_busy_absent_ready_is_idle() {
        let patterns = patterns(Some("^READY$"), 1, None);
        assert_eq!(classify("READY\n", &patterns), PaneState::Idle);
    }

    #[test]
    fn classify_busy_absent_not_ready_is_unknown() {
        let patterns = patterns(Some("^READY$"), 1, None);
        assert_eq!(classify("waiting\n", &patterns), PaneState::Unknown);
    }

    #[test]
    fn classify_ready_absent_busy_is_busy() {
        let patterns = patterns(None, 1, Some("BUSY"));
        assert_eq!(classify("BUSY\n", &patterns), PaneState::Busy);
    }

    #[test]
    fn classify_ready_absent_not_busy_is_unknown() {
        let patterns = patterns(None, 1, Some("BUSY"));
        assert_eq!(classify("waiting\n", &patterns), PaneState::Unknown);
    }

    #[test]
    fn classify_neither_pattern_present_is_unknown() {
        let patterns = patterns(None, 1, None);
        assert!(patterns.is_empty());
        assert_eq!(classify("READY\n", &patterns), PaneState::Unknown);
    }
}
