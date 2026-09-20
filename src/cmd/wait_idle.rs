use anyhow::{Context, Result};
use clap::Args;
use regex::Regex;
use serde::Serialize;
use std::time::Duration;
use tmux_tools_core::{
    agents,
    format::Format,
    idle::{
        bottom_non_blank_lines, resolve_timeout, validate_seconds, wait_for_idle, IdleConfig,
        IdleReason, SurfacePatterns, DEFAULT_READY_SCAN_LINES, DEFAULT_READY_STABLE_SECONDS,
    },
    names, target,
};

use crate::CommonArgs;

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
    #[arg(
        long,
        default_value_t = 10,
        value_name = "N",
        help = "Bottom non-blank pane lines included in a concise timeout hint (0 disables)"
    )]
    pub(crate) hint_lines: usize,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct WaitIdleJson<'a> {
    target: &'a str,
    name: Option<&'a str>,
    reason: &'static str,
    duration_ms: u128,
    idle_for: f64,
    final_capture: &'a str,
}

pub fn run(args: &WaitIdleArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;

    let ready = surface_patterns_for(&pane)?;
    let cfg = IdleConfig {
        idle_seconds: validate_seconds(args.idle_seconds, "idle-seconds")?,
        poll_interval: Duration::from_millis(250),
        timeout: resolve_timeout(args.timeout, "timeout")?,
        surface: ready,
        ready_stable_seconds: validate_seconds(args.ready_stable_seconds, "ready-stable-seconds")?,
        until_regex: args.until.as_deref().map(Regex::new).transpose()?,
        // `wait-idle` keeps the historical fallback: a quiet `Unknown` capture settles it.
        idle_on_unknown: true,
    };
    let outcome = wait_for_idle(&pane, &cfg)?;

    match args.common.format {
        Format::Concise if outcome.reason == IdleReason::TimedOut => {
            println!(
                "reason={} duration={:.3} idle_for={:.3}",
                outcome.reason.as_str(),
                outcome.duration.as_secs_f64(),
                outcome.idle_for.as_secs_f64()
            );
            if args.hint_lines > 0 {
                println!(
                    "--- timeout hint: bottom {} non-blank lines ---",
                    args.hint_lines
                );
                for line in bottom_non_blank_lines(&outcome.final_capture, args.hint_lines) {
                    println!("{line}");
                }
            }
        }
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
                idle_for: outcome.idle_for.as_secs_f64(),
                final_capture: &outcome.final_capture,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => print!("{}", outcome.final_capture),
    }

    Ok(())
}

/// The resolved surface facts `prompt`/`wait-idle` need: readiness patterns and the
/// declared bracketed-paste capability. `None` when the pane resolves to no surface
/// (no agent, or a record naming a surface the registry no longer has).
pub(crate) struct SurfaceInfo {
    pub patterns: Option<SurfacePatterns>,
    pub bracketed_paste: bool,
}

/// P5's interrupt facts for the pane's resolved surface, resolved **without**
/// compiling the surface's readiness patterns. A surface declaring no quit hazard
/// needs no patterns, and a pattern-compile failure must not preempt the
/// surface-unvalidated refusal. `None` when the pane resolves to no surface.
pub(crate) struct InterruptSurface {
    pub agent: String,
    pub surface: String,
    pub interrupt_key: String,
    pub quit_when_idle: bool,
    pub ready_regex: Option<String>,
    pub ready_lines: Option<usize>,
    pub busy_regex: Option<String>,
}

/// Resolve a pane's recorded surface to its owned fields. `None` when the pane names
/// no agent the registry knows, or its record names a surface the registry does not
/// have.
fn resolved_surface_owned(pane: &str) -> Result<Option<(String, String, agents::Surface)>> {
    let registered = names::read(pane)?;
    let Some(agent_name) = registered.agent else {
        return Ok(None);
    };

    let (registry, warnings) = agents::Registry::load()?;
    for warning in &warnings {
        eprintln!("warning: agent {}: {}", warning.agent, warning.detail);
    }
    let Some(resolved) = registry.resolve_surface(&agent_name, registered.surface.as_deref())
    else {
        return Ok(None);
    };

    Ok(Some((
        agent_name,
        resolved.name.to_owned(),
        resolved.surface.clone(),
    )))
}

/// The compiled readiness patterns and declared capability of the pane's resolved
/// surface. `None` when the pane resolves to no surface.
pub(crate) fn surface_info_for(pane: &str) -> Result<Option<SurfaceInfo>> {
    let Some((agent_name, surface_name, surface)) = resolved_surface_owned(pane)? else {
        return Ok(None);
    };

    Ok(Some(SurfaceInfo {
        patterns: compile_surface_patterns(&agent_name, &surface_name, &surface)?,
        bracketed_paste: surface.bracketed_paste,
    }))
}

/// The interrupt contract of the pane's resolved surface, resolved without compiling
/// its readiness patterns. `None` when the pane resolves to no surface.
pub(crate) fn interrupt_surface_for(pane: &str) -> Result<Option<InterruptSurface>> {
    let Some((agent, surface_name, surface)) = resolved_surface_owned(pane)? else {
        return Ok(None);
    };

    Ok(Some(InterruptSurface {
        agent,
        surface: surface_name,
        interrupt_key: surface.interrupt_key,
        quit_when_idle: surface.quit_when_idle,
        ready_regex: surface.ready_regex,
        ready_lines: surface.ready_lines,
        busy_regex: surface.busy_regex,
    }))
}

/// The compiled readiness patterns of the pane's resolved surface, or `None` when there
/// is no surface or it supplies no patterns (then the wait falls back to idle detection).
pub(crate) fn surface_patterns_for(pane: &str) -> Result<Option<SurfacePatterns>> {
    Ok(surface_info_for(pane)?.and_then(|info| info.patterns))
}

fn compile_surface_patterns(
    agent_name: &str,
    surface_name: &str,
    surface: &agents::Surface,
) -> Result<Option<SurfacePatterns>> {
    compile_patterns(
        agent_name,
        surface_name,
        surface.ready_regex.as_deref(),
        surface.ready_lines,
        surface.busy_regex.as_deref(),
    )
}

/// Compile the raw readiness patterns a resolved surface supplies. `None` when the
/// surface supplies neither pattern.
pub(crate) fn compile_patterns(
    agent_name: &str,
    surface_name: &str,
    ready_regex: Option<&str>,
    ready_lines: Option<usize>,
    busy_regex: Option<&str>,
) -> Result<Option<SurfacePatterns>> {
    let ready_regex = match ready_regex {
        Some(pattern) => Some(Regex::new(pattern).with_context(|| {
            format!("invalid ready_regex for agent {agent_name} surface {surface_name}: {pattern}")
        })?),
        None => None,
    };
    let busy_regex = match busy_regex {
        Some(pattern) => Some(Regex::new(pattern).with_context(|| {
            format!("invalid busy_regex for agent {agent_name} surface {surface_name}: {pattern}")
        })?),
        None => None,
    };

    if ready_regex.is_none() && busy_regex.is_none() {
        return Ok(None);
    }

    // `ready_lines = 0` is the "scan every non-blank line" (whole-pane) sentinel, so it
    // must survive — don't clamp to 1. `None` falls back to the default (bottom line only).
    Ok(Some(SurfacePatterns {
        ready_regex,
        ready_scan_lines: ready_lines.unwrap_or(DEFAULT_READY_SCAN_LINES),
        busy_regex,
    }))
}
