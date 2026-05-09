use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::{cmd::safety, format::Format, target, tmux, CommonArgs};

const VERB: &str = "send-enter";

#[derive(Args, Debug)]
pub struct SendEnterArgs {
    /// Allow sending Enter to the pane that is invoking tmux-tools.
    #[arg(long, default_value_t = false)]
    pub(crate) force: bool,
    /// Allow sending Enter to panes that were not created by tmux-tools or
    /// whose recorded `@tt-cwd` differs from the current working directory.
    #[arg(long, default_value_t = false)]
    pub(crate) any: bool,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(Serialize)]
struct SendEnterJson<'a> {
    target: &'a str,
    key: &'static str,
}

pub fn run(args: &SendEnterArgs) -> Result<()> {
    let pane = target::resolve_from_common(&args.common)?;
    safety::enforce_for_pane(VERB, &pane, args.force, args.any)?;

    tmux::run_checked(&["send-keys", "-t", &pane, "Enter"])?;

    match args.common.format {
        Format::Concise => println!("sent Enter to {pane}"),
        Format::Json => {
            let output = SendEnterJson {
                target: &pane,
                key: "Enter",
            };
            println!("{}", serde_json::to_string(&output)?);
        }
        Format::Raw => println!(),
    }

    Ok(())
}
