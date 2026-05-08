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
    pub until_regex: Option<Regex>,
}

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
            until_regex: None,
        }
    }
}

pub fn wait_for_idle(pane_id: &str, cfg: &IdleConfig) -> Result<IdleOutcome> {
    let start = Instant::now();
    let mut last_change = start;
    let mut previous_hash = None;
    let mut capture_count = 0_u64;

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

        if let Some(reason) = classify(&stripped, &cfg.ready_regex, &cfg.until_regex) {
            return Ok(IdleOutcome {
                reason,
                duration: start.elapsed(),
                final_capture: stripped,
            });
        }

        if capture_count >= 2 && (now - last_change).as_secs_f64() >= cfg.idle_seconds {
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

fn classify(stripped: &str, ready: &Option<Regex>, until: &Option<Regex>) -> Option<IdleReason> {
    if ready
        .as_ref()
        .is_some_and(|regex| regex.is_match(bottom_non_blank_line(stripped)))
    {
        return Some(IdleReason::ReadyMatched);
    }

    if until.as_ref().is_some_and(|regex| regex.is_match(stripped)) {
        return Some(IdleReason::UntilMatched);
    }

    None
}

fn bottom_non_blank_line(stripped: &str) -> &str {
    stripped
        .split('\n')
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
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
    fn classify_returns_ready_when_ready_matches_bottom_line() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));
        let until = Some(Regex::new(r"earlier").expect("test regex compiles"));

        assert_eq!(
            classify("earlier\nready>\n\n", &ready, &until),
            Some(IdleReason::ReadyMatched)
        );
    }

    #[test]
    fn classify_returns_until_when_ready_does_not_match_bottom_line() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));
        let until = Some(Regex::new(r"match earlier").expect("test regex compiles"));

        assert_eq!(
            classify("match earlier\nnot ready\n", &ready, &until),
            Some(IdleReason::UntilMatched)
        );
    }

    #[test]
    fn classify_returns_none_when_neither_regex_matches() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));
        let until = Some(Regex::new(r"done").expect("test regex compiles"));

        assert_eq!(classify("working\nstill working\n", &ready, &until), None);
    }

    #[test]
    fn classify_returns_none_when_ready_regex_sees_only_blank_capture() {
        let ready = Some(Regex::new(r"^ready>$").expect("test regex compiles"));
        let until = None;

        assert_eq!(classify("\n  \n\t\n", &ready, &until), None);
    }

    #[test]
    fn default_config_uses_expected_values() {
        let cfg = IdleConfig::default();

        assert_eq!(cfg.idle_seconds, 2.0);
        assert_eq!(cfg.poll_interval, Duration::from_millis(250));
        assert_eq!(cfg.timeout, Duration::from_secs(120));
        assert!(cfg.ready_regex.is_none());
        assert!(cfg.until_regex.is_none());
    }
}
