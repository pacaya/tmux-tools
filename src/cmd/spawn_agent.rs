use anyhow::{anyhow, Result};
use clap::Args;
use serde::Serialize;
use std::path::PathBuf;
use tmux_tools_core::{
    agents::Registry,
    format::{display_value, Format},
    names,
};

use crate::{
    cmd::launch::{
        launch_pane, resolve_launch_target, resolve_layout, wrap_keep_open, Layout, Split,
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
    #[arg(long, value_enum, value_name = "h|v")]
    pub(crate) split: Option<Split>,
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
    name: Option<&'a str>,
    pane_id: &'a str,
    binary: &'a str,
    argv: &'a [String],
    launched_at: &'a str,
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
    let layout = resolve_layout(
        args.split,
        args.size,
        args.common.target.is_some(),
        args.common.session.is_some(),
        args.common.window.is_some(),
    );
    let (split_arg, size_arg) = layout.split_args();
    let launch_target = resolve_launch_target(
        args.common.target.as_ref().map(|target| target.as_str()),
        args.common.session.as_deref(),
        args.common.window.as_deref(),
        matches!(layout, Layout::Split { .. }),
    )?;
    let pane_id = launch_pane(
        &cmd,
        args.name.as_deref(),
        split_arg,
        size_arg,
        launch_target.as_ref(),
    )?;
    let launched_at = rfc3339_utc_now()?;

    names::set(&pane_id, names::KEY_AGENT, &args.agent)?;
    // Record the resolved surface unconditionally, whether or not one was named: an
    // absent record can then only mean a pane created before surfaces existed.
    names::set(&pane_id, names::KEY_SURFACE, &resolved.surface)?;
    // Caller-supplied trailing arguments can change the rendering out from under the
    // recorded surface. Arguments the surface supplies itself are validated by
    // construction and do not set the mark.
    if !args.extra_args.is_empty() {
        names::set(&pane_id, names::KEY_SURFACE_UNVALIDATED, "1")?;
    }
    if let Some(access) = &args.access {
        names::set(&pane_id, names::KEY_ACCESS, access)?;
    }
    if let Some(name) = &args.name {
        names::set(&pane_id, names::KEY_NAME, name)?;
    }
    names::set(&pane_id, names::KEY_LAUNCHED_AT, &launched_at)?;
    if let Ok(cwd) = std::env::current_dir() {
        names::set(&pane_id, names::KEY_CWD, &cwd.to_string_lossy())?;
    }

    render_output(
        args,
        &pane_id,
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

fn render_output(
    args: &SpawnAgentArgs,
    pane_id: &str,
    binary: &str,
    argv: &[String],
    launched_at: &str,
    surface: &str,
) -> Result<()> {
    match args.common.format {
        Format::Concise => println!(
            "agent={} access={} surface={} name={} pane={}",
            args.agent,
            display_value(args.access.as_deref()),
            surface,
            display_value(args.name.as_deref()),
            pane_id
        ),
        Format::Json => {
            let output = SpawnAgentJson {
                agent: &args.agent,
                access: args.access.as_deref(),
                surface,
                name: args.name.as_deref(),
                pane_id,
                binary,
                argv,
                launched_at,
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!("{pane_id}"),
    }

    Ok(())
}
