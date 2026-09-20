use anyhow::{anyhow, Result};
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{strip_ansi, Format},
    idle::{classify, PaneState, SurfacePatterns},
    target, tmux,
};

use crate::{
    cmd::{
        safety,
        wait_idle::{compile_patterns, interrupt_surface_for, InterruptSurface},
    },
    CommonArgs,
};

const VERB: &str = "interrupt";

#[derive(Args, Debug)]
pub struct InterruptArgs {
    /// Allow interrupting the pane that is invoking tmux-tools.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Allow interrupting panes that were not created by tmux-tools or whose
    /// recorded `@tt-cwd` differs from the current working directory.
    #[arg(long, default_value_t = false)]
    pub(crate) any: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct InterruptJson<'a> {
    target: &'a str,
    key: &'a str,
}

/// The structured refusal reported for JSON output. The command still exits
/// non-zero; the field is how a programmatic caller sees *why* nothing was sent.
#[derive(Serialize)]
struct InterruptRefusalJson<'a> {
    target: &'a str,
    key: &'a str,
    error: &'a str,
}

pub fn run(args: &InterruptArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;
    let registered = safety::enforce_for_pane(VERB, &pane, args.force, args.any)?;

    // Resolve the surface's key and hazard without compiling its patterns: a surface
    // declaring no quit hazard needs none, and a compile failure must not preempt the
    // surface-unvalidated refusal below.
    let surface = interrupt_surface_for(&pane)?;
    let key = surface
        .as_ref()
        .map(|info| info.interrupt_key.as_str())
        .unwrap_or("C-c");

    if let Some(info) = &surface {
        if info.quit_when_idle {
            // A surface-unvalidated pane is refused outright, whatever it renders:
            // its rendering is not the one whose quit hazard was declared.
            if registered.surface_unvalidated {
                return refuse(
                    args.common.format,
                    &pane,
                    key,
                    unvalidated_message(&pane, key, info),
                );
            }

            // Compile and classify only for a validated hazardous pane. A malformed
            // pattern means the pane cannot be confirmed positively busy, so nothing
            // is sent.
            let patterns = match compile_patterns(
                &info.agent,
                &info.surface,
                info.ready_regex.as_deref(),
                info.ready_lines,
                info.busy_regex.as_deref(),
            ) {
                Ok(patterns) => patterns,
                Err(error) => {
                    return refuse(
                        args.common.format,
                        &pane,
                        key,
                        malformed_pattern_message(&pane, key, info, &error),
                    );
                }
            };

            // Without a busy pattern the classifier can never return `Busy`, so
            // "wait and retry" would be guidance that cannot succeed.
            let Some(patterns) = patterns.filter(|patterns| patterns.busy_regex.is_some()) else {
                return refuse(
                    args.common.format,
                    &pane,
                    key,
                    no_busy_pattern_message(&pane, key, info),
                );
            };

            let state = classify_live(&pane, &patterns)?;
            if state != PaneState::Busy {
                return refuse(
                    args.common.format,
                    &pane,
                    key,
                    not_busy_message(&pane, key, state),
                );
            }
        }
    }

    tmux::run_checked(&["send-keys", "-t", &pane, key])?;

    match args.common.format {
        Format::Concise => println!("sent {key} to {pane}"),
        Format::Json => {
            let output = InterruptJson { target: &pane, key };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!(),
    }

    Ok(())
}

/// P3's classifier applied once to the pane's live visible capture.
fn classify_live(pane: &str, patterns: &SurfacePatterns) -> Result<PaneState> {
    let output = tmux::run_checked(&["capture-pane", "-t", pane, "-p"])?;
    Ok(classify(&strip_ansi(&output), patterns))
}

fn state_name(state: PaneState) -> &'static str {
    match state {
        PaneState::Idle => "idle",
        PaneState::Busy => "busy",
        PaneState::Unknown => "unknown",
    }
}

fn not_busy_message(pane: &str, key: &str, state: PaneState) -> String {
    format!(
        "refusing to interrupt pane {pane}: the resolved surface declares that {key} quits the \
         agent while idle, and the pane reads {} (not positively busy); wait until the agent is \
         generating and retry",
        state_name(state)
    )
}

fn no_busy_pattern_message(pane: &str, key: &str, info: &InterruptSurface) -> String {
    format!(
        "refusing to interrupt pane {pane}: surface {} supplies no busy pattern, so the pane can \
         never be confirmed positively busy and {key} could quit it while idle; configure a \
         busy_regex for the surface and retry",
        info.surface
    )
}

fn malformed_pattern_message(
    pane: &str,
    key: &str,
    info: &InterruptSurface,
    error: &anyhow::Error,
) -> String {
    format!(
        "refusing to interrupt pane {pane}: surface {} declares that {key} quits the agent while \
         idle, but its readiness pattern is malformed ({error:#}), so the pane cannot be confirmed \
         positively busy; fix the surface's ready_regex or busy_regex and retry",
        info.surface
    )
}

fn unvalidated_message(pane: &str, key: &str, info: &InterruptSurface) -> String {
    format!(
        "refusing to interrupt pane {pane}: surface {} declares that {key} quits the agent while \
         idle, and this pane was spawned with caller-supplied arguments so its rendering is \
         surface-unvalidated; respawn the agent without trailing arguments before interrupting",
        info.surface
    )
}

/// Report the refusal (as JSON when asked) and return the same message as the
/// non-zero exit. Raw and concise carry it on stderr via the process error.
fn refuse(format: Format, pane: &str, key: &str, message: String) -> Result<()> {
    if format == Format::Json {
        let output = InterruptRefusalJson {
            target: pane,
            key,
            error: &message,
        };
        println!("{}", serde_json::to_string(&output)?);
    }
    Err(anyhow!(message))
}
