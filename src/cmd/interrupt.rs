use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::{cmd::safety, format::Format, target, tmux, CommonArgs};

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
    key: &'static str,
}

pub fn run(args: &InterruptArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;
    safety::enforce_for_pane(VERB, &pane, args.force, args.any)?;

    tmux::run_checked(&["send-keys", "-t", &pane, "C-c"])?;

    match args.common.format {
        Format::Concise => println!("sent C-c to {pane}"),
        Format::Json => {
            let output = InterruptJson {
                target: &pane,
                key: "C-c",
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!(),
    }

    Ok(())
}
