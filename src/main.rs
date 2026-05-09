use anyhow::Result;
use clap::{Args, Parser, Subcommand};

mod agents;
mod cmd;
pub mod format;
mod idle;
mod names;
mod target;
mod tmux;
mod util;

use crate::cmd::{
    capture::CaptureArgs, escape::EscapeArgs, execute::ExecuteArgs, interrupt::InterruptArgs,
    kill::KillArgs, launch::LaunchArgs, list::ListArgs, prompt::PromptArgs, send::SendArgs,
    send_enter::SendEnterArgs, spawn_agent::SpawnAgentArgs, status::StatusArgs,
    wait_idle::WaitIdleArgs,
};
pub use crate::format::Format;

#[derive(Debug, Parser)]
#[command(name = "tmux-tools")]
#[command(about = "A Rust CLI for controlling tmux sessions and panes")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Launch(LaunchArgs),
    Send(SendArgs),
    #[command(name = "send-enter")]
    SendEnter(SendEnterArgs),
    Capture(CaptureArgs),
    Execute(ExecuteArgs),
    #[command(name = "wait-idle")]
    WaitIdle(WaitIdleArgs),
    Prompt(PromptArgs),
    #[command(name = "spawn-agent")]
    SpawnAgent(SpawnAgentArgs),
    Kill(KillArgs),
    Interrupt(InterruptArgs),
    Escape(EscapeArgs),
    List(ListArgs),
    Status(StatusArgs),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Target {
    value: String,
}

impl Target {
    pub(crate) fn as_str(&self) -> &str {
        &self.value
    }
}

pub(crate) fn parse_target(value: &str) -> std::result::Result<Target, String> {
    if value.is_empty() {
        return Err("target cannot be empty".to_owned());
    }

    Ok(Target {
        value: value.to_owned(),
    })
}

#[derive(Args, Debug)]
pub(crate) struct CommonArgs {
    #[arg(long, value_name = "name|id", value_parser = parse_target)]
    pub(crate) target: Option<Target>,
    #[arg(long, value_enum, default_value_t = Format::Concise)]
    pub(crate) format: Format,
    #[arg(long, value_name = "NAME")]
    pub(crate) session: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub(crate) window: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Launch(args) => crate::cmd::launch::run(&args)?,
        Commands::Send(args) => crate::cmd::send::run(&args)?,
        Commands::SendEnter(args) => crate::cmd::send_enter::run(&args)?,
        Commands::Capture(args) => crate::cmd::capture::run(&args)?,
        Commands::Execute(args) => crate::cmd::execute::run(&args)?,
        Commands::WaitIdle(args) => crate::cmd::wait_idle::run(&args)?,
        Commands::Prompt(args) => crate::cmd::prompt::run(&args)?,
        Commands::SpawnAgent(args) => crate::cmd::spawn_agent::run(&args)?,
        Commands::Kill(args) => crate::cmd::kill::run(&args)?,
        Commands::Interrupt(args) => crate::cmd::interrupt::run(&args)?,
        Commands::Escape(args) => crate::cmd::escape::run(&args)?,
        Commands::List(args) => crate::cmd::list::run(&args)?,
        Commands::Status(args) => crate::cmd::status::run(&args)?,
    }

    Ok(())
}
