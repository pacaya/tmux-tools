use anyhow::{anyhow, Result};
use clap::Args;
use serde::Serialize;
use std::path::PathBuf;

use crate::{
    agents::Registry,
    cmd::launch::{launch_pane, resolve_launch_target, Split},
    format::{display_value, Format},
    names,
    util::rfc3339_utc_now,
    CommonArgs,
};

#[derive(Args, Debug)]
pub struct SpawnAgentArgs {
    #[arg(value_name = "AGENT")]
    pub(crate) agent: String,
    #[arg(long, value_name = "PROFILE")]
    pub(crate) access: Option<String>,
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
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct SpawnAgentJson<'a> {
    agent: &'a str,
    access: Option<&'a str>,
    name: Option<&'a str>,
    pane_id: &'a str,
    binary: &'a str,
    argv: &'a [String],
    launched_at: &'a str,
}

pub fn run(args: &SpawnAgentArgs) -> Result<()> {
    let registry = Registry::load()?;
    let (binary, profile_args) = registry.launch_argv(&args.agent, args.access.as_deref())?;
    let argv = launch_argv(binary.clone(), profile_args, &args.extra_args);
    let cmd = launch_command(&argv, args.cwd.as_ref())?;
    let split_arg = args.split.map(Split::tmux_flag);
    let launch_target = resolve_launch_target(
        args.common.target.as_ref().map(|target| target.as_str()),
        args.common.session.as_deref(),
        args.common.window.as_deref(),
        split_arg.is_some(),
    )?;
    let pane_id = launch_pane(
        &cmd,
        args.name.as_deref(),
        split_arg,
        args.size,
        launch_target.as_ref(),
    )?;
    let launched_at = rfc3339_utc_now()?;

    names::set(&pane_id, names::KEY_AGENT, &args.agent)?;
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

    render_output(args, &pane_id, &binary, &argv, &launched_at)
}

fn launch_argv(binary: String, profile_args: Vec<String>, extra_args: &[String]) -> Vec<String> {
    std::iter::once(binary)
        .chain(profile_args)
        .chain(extra_args.iter().cloned())
        .collect()
}

fn launch_command(argv: &[String], cwd: Option<&PathBuf>) -> Result<String> {
    let command = argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");

    if let Some(cwd) = cwd {
        let cwd = cwd
            .to_str()
            .ok_or_else(|| anyhow!("--cwd path is not valid UTF-8: {}", cwd.display()))?;
        // `cd --` makes the next argument a positional path even if it starts
        // with `-`, so a relative cwd like `-foo` is not parsed as a flag.
        Ok(format!("cd -- {} && {command}", shell_quote(cwd)))
    } else {
        Ok(command)
    }
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
) -> Result<()> {
    match args.common.format {
        Format::Concise => println!(
            "agent={} access={} name={} pane={}",
            args.agent,
            display_value(args.access.as_deref()),
            display_value(args.name.as_deref()),
            pane_id
        ),
        Format::Json => {
            let output = SpawnAgentJson {
                agent: &args.agent,
                access: args.access.as_deref(),
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

