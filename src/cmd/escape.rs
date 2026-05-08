use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::{cmd::safety, format::Format, target, tmux, CommonArgs};

const VERB: &str = "escape";

#[derive(Args, Debug)]
pub struct EscapeArgs {
    /// Allow sending Escape to the pane that is invoking tmux-tools.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Allow sending Escape to panes that were not created by tmux-tools or
    /// whose recorded `@tt-cwd` differs from the current working directory.
    #[arg(long, default_value_t = false)]
    pub(crate) any: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct EscapeJson<'a> {
    target: &'a str,
    key: &'static str,
}

pub fn run(args: &EscapeArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;
    safety::enforce_for_pane(VERB, &pane, args.force, args.any)?;

    tmux::run_checked(&["send-keys", "-t", &pane, "Escape"])?;

    match args.common.format {
        Format::Concise => println!("sent Escape to {pane}"),
        Format::Json => {
            let output = EscapeJson {
                target: &pane,
                key: "Escape",
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!(),
    }

    Ok(())
}
