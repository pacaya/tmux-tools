use anyhow::{Context, Result};
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::time::Duration;
use tmux_tools_core::{
    agents,
    format::Format,
    idle::{
        resolve_timeout, validate_seconds, wait_for_idle, IdleConfig, DEFAULT_READY_SCAN_LINES,
        DEFAULT_READY_STABLE_SECONDS,
    },
    names, target,
};

use crate::CommonArgs;

/// The readiness signal resolved from the pane's registered agent profile: the compiled
/// `ready_regex` (if any) and how many bottom non-blank lines it should be tested against.
pub(crate) struct ReadySignal {
    pub(crate) regex: Option<Regex>,
    pub(crate) scan_lines: usize,
}

#[derive(Args, Debug)]
pub struct WaitIdleArgs {
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
struct WaitIdleJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    reason: &'static str,
    duration_ms: u128,
    final_capture: &'a str,
}

pub fn run(args: &WaitIdleArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    let ready = ready_signal_for(&pane)?;
    let cfg = IdleConfig {
        idle_seconds: validate_seconds(args.idle_seconds, "idle-seconds")?,
        poll_interval: Duration::from_millis(250),
        timeout: resolve_timeout(args.timeout, "timeout")?,
        ready_regex: ready.regex,
        ready_scan_lines: ready.scan_lines,
        ready_stable_seconds: validate_seconds(args.ready_stable_seconds, "ready-stable-seconds")?,
        until_regex: args.until.as_deref().map(Regex::new).transpose()?,
    };
    let outcome = wait_for_idle(&pane, &cfg)?;

    match args.common.format {
        Format::Concise => println!(
            "reason={} duration={:.3}",
            outcome.reason.as_str(),
            outcome.duration.as_secs_f64()
        ),
        Format::Json => {
            let name = names::get(&pane, names::KEY_NAME)?;
            let output = WaitIdleJson {
                target: &pane,
                name: name.as_deref(),
                reason: outcome.reason.as_str(),
                duration_ms: outcome.duration.as_millis(),
                final_capture: &outcome.final_capture,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => print!("{}", outcome.final_capture),
    }

    Ok(())
}

pub(crate) fn ready_signal_for(pane: &str) -> Result<ReadySignal> {
    let none = ReadySignal {
        regex: None,
        scan_lines: DEFAULT_READY_SCAN_LINES,
    };

    let registered = names::read(pane)?;
    let Some(agent_name) = registered.agent else {
        return Ok(none);
    };

    let (registry, _warnings) = agents::Registry::load()?;
    let Some(agent) = registry.get(&agent_name) else {
        return Ok(none);
    };

    // `ready_lines = 0` is the "scan every non-blank line" (whole-pane) sentinel, so it
    // must survive — don't clamp to 1. `None` falls back to the default (bottom line only).
    let scan_lines = agent.ready_lines.unwrap_or(DEFAULT_READY_SCAN_LINES);

    let Some(pattern) = agent.ready_regex.as_deref() else {
        return Ok(ReadySignal {
            regex: None,
            scan_lines,
        });
    };

    let regex = Regex::new(pattern)
        .with_context(|| format!("invalid ready_regex for agent {agent_name}: {pattern}"))?;
    Ok(ReadySignal {
        regex: Some(regex),
        scan_lines,
    })
}
