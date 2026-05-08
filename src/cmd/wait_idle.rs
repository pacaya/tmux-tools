use anyhow::{Context, Result};
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::time::Duration;

use crate::{
    agents,
    format::Format,
    idle::{resolve_timeout, validate_seconds, wait_for_idle, IdleConfig},
    names, target, CommonArgs,
};

#[derive(Args, Debug)]
pub struct WaitIdleArgs {
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
struct WaitIdleJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    reason: &'static str,
    duration_ms: u128,
    final_capture: &'a str,
}

pub fn run(args: &WaitIdleArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    let cfg = IdleConfig {
        idle_seconds: validate_seconds(args.idle_seconds, "idle-seconds")?,
        poll_interval: Duration::from_millis(250),
        timeout: resolve_timeout(args.timeout, "timeout")?,
        ready_regex: ready_regex_for(&pane)?,
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

pub(crate) fn ready_regex_for(pane: &str) -> Result<Option<Regex>> {
    let registered = names::read(pane)?;
    let Some(agent_name) = registered.agent else {
        return Ok(None);
    };

    let registry = agents::Registry::load()?;
    let Some(agent) = registry.get(&agent_name) else {
        return Ok(None);
    };

    let Some(pattern) = agent.ready_regex.as_deref() else {
        return Ok(None);
    };

    let regex = Regex::new(pattern)
        .with_context(|| format!("invalid ready_regex for agent {agent_name}: {pattern}"))?;
    Ok(Some(regex))
}
