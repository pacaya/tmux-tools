use anyhow::{bail, Result};
use clap::Args;
use std::env;
use tmux_tools_core::{
    host::{self, Delivery},
    target,
};

use crate::{parse_target, Target};

/// Show a desktop notification for a pane
///
/// Notifies for the --target pane, or for the calling pane when --target is absent.
/// When the host cannot deliver it, the verb still exits 0 and prints
/// `notify: not delivered on <host>` on stderr.
#[derive(Args, Debug)]
pub struct NotifyArgs {
    #[arg(long, value_name = "T")]
    pub(crate) title: String,
    #[arg(long, value_name = "B")]
    pub(crate) body: String,
    #[arg(long, value_name = "name|id", value_parser = parse_target)]
    pub(crate) target: Option<Target>,
}

pub fn run(args: &NotifyArgs) -> Result<()> {
    let pane = match &args.target {
        Some(name) => target::resolve(&target::parse(name.as_str()), None, None)?,
        None => calling_pane()?,
    };

    let host = host::current();
    let delivery = if host.capabilities().notify {
        host.notify(&pane, &args.title, &args.body)?
    } else {
        Delivery::NotDelivered
    };
    if delivery == Delivery::NotDelivered {
        eprintln!("notify: not delivered on {}", host.host());
    }

    Ok(())
}

/// The calling pane on tmux: `TMUX_PANE`, with no fallback, so a notification never
/// goes to a pane the caller did not name.
fn calling_pane() -> Result<String> {
    match env::var("TMUX_PANE") {
        Ok(pane) if !pane.trim().is_empty() => Ok(pane.trim().to_owned()),
        _ => bail!(
            "notify without --target needs TMUX_PANE to name the calling pane; TMUX_PANE is unset"
        ),
    }
}
