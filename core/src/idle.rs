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
    pub ready_regex: Option<Regex>,
    /// How many of the bottom non-blank lines the `ready_regex` is tested against.
    /// 1 matches only the bottom non-blank line (the default); higher values let the
    /// regex match a prompt that sits above a TUI status/footer row. `0` means "no limit":
    /// every non-blank line is scanned, so a uniquely-anchored pattern matches a status
    /// line regardless of how many task/footer rows render below it.
    pub ready_scan_lines: usize,
    /// How long a `ready_regex` *or* `until_regex` match must hold continuously before
    /// completing (`ReadyMatched` / `UntilMatched`). Guards against a stale indicator that
    /// lingers for a single poll (e.g. a previous turn's `✓ done` / `· Ready ·` status line
    /// still visible right after a new prompt is submitted) triggering a premature, wrong
    /// completion. `0.0` fires on first match (legacy behavior; use for a one-shot marker
    /// that may scroll off-screen before the window elapses).
    pub ready_stable_seconds: f64,
    pub until_regex: Option<Regex>,
}

pub const DEFAULT_READY_SCAN_LINES: usize = 1;
pub const DEFAULT_READY_STABLE_SECONDS: f64 = 2.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdleOutcome {
    pub reason: IdleReason,
    pub duration: Duration,
    pub final_capture: String,
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
            ready_regex: None,
            ready_scan_lines: DEFAULT_READY_SCAN_LINES,
            ready_stable_seconds: DEFAULT_READY_STABLE_SECONDS,
            until_regex: None,
        }
    }
}

pub fn wait_for_idle(pane_id: &str, cfg: &IdleConfig) -> Result<IdleOutcome> {
    let start = Instant::now();
    let mut last_change = start;
    let mut previous_hash = None;
    let mut capture_count = 0_u64;
    let mut ready_debounce = MatchDebounce::default();
    let mut until_debounce = MatchDebounce::default();

    loop {
        let output = tmux::run(&["capture-pane", "-t", pane_id, "-p"])?;
        if output.exit_code != 0 {
            bail!(
                "tmux capture-pane failed with exit code {}: {}",
                output.exit_code,
                output.stderr.trim_end()
            );
        }

        let stripped = strip_ansi(&output.stdout);
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
                final_capture: stripped,
            });
        }

        // A ready match must hold continuously for `ready_stable_seconds` before
        // completing, so a stale indicator visible for a single poll can't trigger a
        // premature completion.
        let ready_now = ready_matches(&stripped, &cfg.ready_regex, cfg.ready_scan_lines);
        if ready_debounce.observe(ready_now, now, cfg.ready_stable_seconds) {
            return Ok(IdleOutcome {
                reason: IdleReason::ReadyMatched,
                duration: start.elapsed(),
                final_capture: stripped,
            });
        }

        // Suppress `Idle` while a ready or until match is pending its debounce: a static
        // pane that is currently matching is exactly what would trip `Idle` early when
        // `idle_seconds <= ready_stable_seconds`. Suppressing it ensures such a pane is
        // reported as `ReadyMatched`/`UntilMatched` after the debounce, never a premature
        // `Idle`.
        if !ready_now
            && !until_now
            && capture_count >= 2
            && (now - last_change).as_secs_f64() >= cfg.idle_seconds
        {
            return Ok(IdleOutcome {
                reason: IdleReason::Idle,
                duration: start.elapsed(),
                final_capture: stripped,
            });
        }

        if start.elapsed() >= cfg.timeout {
            return Ok(IdleOutcome {
                reason: IdleReason::TimedOut,
                duration: start.elapsed(),
                final_capture: stripped,
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
fn bottom_non_blank_lines(stripped: &str, count: usize) -> Vec<&str> {
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
        assert!(cfg.ready_regex.is_none());
        assert!(cfg.until_regex.is_none());
    }
}
