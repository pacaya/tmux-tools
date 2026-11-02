use anyhow::{anyhow, Result};
use clap::Args;
use serde::Serialize;
use std::path::PathBuf;
use tmux_tools_core::{
    agents::Registry,
    format::{display_value, Format},
    layout::{self, AgentRegistration, Landing, Registration, Split},
};

use crate::{
    cmd::launch::{
        concise_landing_fields, destination, invoking_cwd, warn_if_unmeasured, wrap_keep_open,
        WindowJson,
    },
    util::rfc3339_utc_now,
    CommonArgs,
};

#[derive(Args, Debug)]
pub struct SpawnAgentArgs {
    #[arg(value_name = "AGENT")]
    pub(crate) agent: String,
    #[arg(long, value_name = "PROFILE")]
    pub(crate) access: Option<String>,
    /// Select the agent's rendering to launch. Defaults to the agent's default surface.
    #[arg(long, value_name = "NAME")]
    pub(crate) surface: Option<String>,
    /// Resume a session by identifier. Validated against the resolved surface's
    /// declared identifier shape before any pane is created.
    #[arg(long, value_name = "ID")]
    pub(crate) resume: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub(crate) name: Option<String>,
    #[arg(long, value_name = "PATH")]
    pub(crate) cwd: Option<PathBuf>,
    #[arg(last = true)]
    pub(crate) extra_args: Vec<String>,
    /// How to place the pane. Inside tmux the default is tile
    #[arg(long, value_enum, value_name = "tile|h|v|window")]
    pub(crate) split: Option<Split>,
    /// The new pane's percentage of the split pane. Alone it is a horizontal split at
    /// N% (no retile); rejected with --split tile
    #[arg(long, value_name = "N")]
    pub(crate) size: Option<u32>,
    /// Skip the keep-alive shell wrap and run the agent bare. The pane closes
    /// the instant the agent exits (or fails to start). Useful when you want
    /// today's tmux-default behavior.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub(crate) bare: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct SpawnAgentJson<'a> {
    agent: &'a str,
    access: Option<&'a str>,
    surface: &'a str,
    name: &'a str,
    pane_id: &'a str,
    binary: &'a str,
    argv: &'a [String],
    launched_at: &'a str,
    relocated: bool,
    #[serde(flatten)]
    window: Option<WindowJson<'a>>,
}

pub fn run(args: &SpawnAgentArgs) -> Result<()> {
    let (registry, warnings) = Registry::load()?;
    for warning in &warnings {
        eprintln!("warning: agent {}: {}", warning.agent, warning.detail);
    }
    let resolved = registry.resolve_launch(
        &args.agent,
        args.access.as_deref(),
        args.surface.as_deref(),
        args.resume.as_deref(),
    )?;
    let binary = resolved.binary;
    let argv = launch_argv(binary.clone(), resolved.args, &args.extra_args);
    let cmd = launch_command(&argv, args.cwd.as_ref(), args.bare)?;
    let launched_at = rfc3339_utc_now()?;
    let cwd = invoking_cwd();

    let landing = layout::place(
        &cmd,
        &destination(&args.common, args.split, args.size),
        &Registration {
            name: args.name.as_deref(),
            // Derive from the agent's resolved command (its binary), never the keep-alive wrap.
            name_source: &binary,
            launched_at: &launched_at,
            cwd: cwd.as_deref(),
            agent: Some(AgentRegistration {
                agent: &args.agent,
                surface: &resolved.surface,
                access: args.access.as_deref(),
                // Caller-supplied trailing arguments can change the rendering out from
                // under the recorded surface; arguments the surface supplies itself are
                // validated by construction and do not set the mark.
                surface_unvalidated: !args.extra_args.is_empty(),
            }),
        },
        &registry,
    )?;

    render_output(
        args,
        &landing,
        &binary,
        &argv,
        &launched_at,
        &resolved.surface,
    )
}

fn launch_argv(binary: String, profile_args: Vec<String>, extra_args: &[String]) -> Vec<String> {
    std::iter::once(binary)
        .chain(profile_args)
        .chain(extra_args.iter().cloned())
        .collect()
}

fn launch_command(argv: &[String], cwd: Option<&PathBuf>, bare: bool) -> Result<String> {
    let command = argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");

    // `cd --` makes the next argument a positional path even if it starts
    // with `-`, so a relative cwd like `-foo` is not parsed as a flag.
    let cwd_prefix = match cwd {
        Some(cwd) => {
            let cwd = cwd
                .to_str()
                .ok_or_else(|| anyhow!("--cwd path is not valid UTF-8: {}", cwd.display()))?;
            Some(format!("cd -- {} &&", shell_quote(cwd)))
        }
        None => None,
    };

    if bare {
        return Ok(match cwd_prefix {
            Some(prefix) => format!("{prefix} {command}"),
            None => command,
        });
    }

    // Keep the optional `cd` in the outer shell so the fallback shell inherits
    // the cwd if `cd` succeeded. `&&` binds tighter than `;`, so `cd && (cmd);
    // exec <shell>` runs the exec whether `cd` or the inner command fails —
    // output is preserved in every branch.
    let wrapped = wrap_keep_open(&command);
    Ok(match cwd_prefix {
        Some(prefix) => format!("{prefix} {wrapped}"),
        None => wrapped,
    })
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Print the spawn result; the name is the one placement chose (`--name`, else the
/// derived name), which is the registered `@tt-name` whenever registration completed.
fn render_output(
    args: &SpawnAgentArgs,
    landing: &Landing,
    binary: &str,
    argv: &[String],
    launched_at: &str,
    surface: &str,
) -> Result<()> {
    warn_if_unmeasured(landing);
    match args.common.format {
        Format::Concise => println!(
            "agent={} access={} surface={} name={} pane={}{}",
            args.agent,
            display_value(args.access.as_deref()),
            surface,
            landing.name,
            landing.pane_id,
            concise_landing_fields(landing)
        ),
        Format::Json => {
            let output = SpawnAgentJson {
                agent: &args.agent,
                access: args.access.as_deref(),
                surface,
                name: &landing.name,
                pane_id: &landing.pane_id,
                binary,
                argv,
                launched_at,
                relocated: landing.relocated,
                window: WindowJson::of(landing),
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!("{}", landing.pane_id),
    }

    Ok(())
}
