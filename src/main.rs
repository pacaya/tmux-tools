use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tmux_tools_core::{
    host::{self, Unsupported},
    names::InvalidName,
    team::TeamNotFound,
    Format,
};

mod cmd;
mod shim;
#[cfg(test)]
#[path = "../core/src/test_child_env.rs"]
mod test_child_env;
mod util;

use crate::cmd::{
    capture::CaptureArgs, claude_teams::ClaudeTeamsArgs, escape::EscapeArgs, execute::ExecuteArgs,
    interrupt::InterruptArgs, kill::KillArgs, launch::LaunchArgs, list::ListArgs,
    notify::NotifyArgs, prompt::PromptArgs, rebalance::RebalanceArgs, self_::SelfArgs,
    send::SendArgs, send_enter::SendEnterArgs, spawn_agent::SpawnAgentArgs, status::StatusArgs,
    wait_idle::WaitIdleArgs,
};

#[derive(Debug, Parser)]
#[command(name = "tmux-tools")]
#[command(version)]
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
    Rebalance(RebalanceArgs),
    Notify(NotifyArgs),
    #[command(name = "self")]
    SelfPane(SelfArgs),
    #[command(name = "claude-teams")]
    ClaudeTeams(ClaudeTeamsArgs),
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

impl tmux_tools_core::target::TargetArgs for CommonArgs {
    fn target(&self) -> Option<&str> {
        self.target.as_ref().map(Target::as_str)
    }

    fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }

    fn window(&self) -> Option<&str> {
        self.window.as_deref()
    }
}

/// The exit code of an optional verb or option the host lacks.
const EXIT_UNSUPPORTED: i32 = 3;

fn main() -> Result<()> {
    // PRD-261108-0447-01 P5.1: started as `tmux`, the binary is the Shim.
    let mut argv = std::env::args();
    let argv0 = argv.next().unwrap_or_default();
    if std::path::Path::new(&argv0)
        .file_name()
        .is_some_and(|name| name == "tmux")
    {
        std::process::exit(shim::run(argv.collect()));
    }

    let cli = Cli::parse();

    // The Launcher selects its host itself and fails open (PRD-261108-0447-01 P2.7).
    if let Commands::ClaudeTeams(args) = &cli.command {
        std::process::exit(crate::cmd::claude_teams::run(args));
    }

    // P3.1: an invalid `TMUX_TOOLS_HOST` fails every verb before it runs.
    match host::selection().and_then(|_| run(cli.command)) {
        Err(error) => {
            if let Some(unsupported) = error.downcast_ref::<Unsupported>() {
                eprintln!("{unsupported}");
                std::process::exit(EXIT_UNSUPPORTED);
            }
            if let Some(not_found) = error.downcast_ref::<TeamNotFound>() {
                // PRD-261108-0447-01 P3.2: stderr is `team not found: <id>`.
                eprintln!("{not_found}");
                std::process::exit(1);
            }
            if let Some(invalid) = error.downcast_ref::<InvalidName>() {
                // P4.1: stderr is exactly `invalid name: <name>`.
                eprintln!("{invalid}");
                std::process::exit(1);
            }
            Err(error)
        }
        Ok(()) => Ok(()),
    }
}

fn run(command: Commands) -> Result<()> {
    match command {
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
        Commands::Rebalance(args) => crate::cmd::rebalance::run(&args)?,
        Commands::Notify(args) => crate::cmd::notify::run(&args)?,
        Commands::SelfPane(args) => crate::cmd::self_::run(&args)?,
        Commands::ClaudeTeams(_) => unreachable!("claude-teams runs before host selection"),
    }

    Ok(())
}
