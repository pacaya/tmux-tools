use anyhow::{bail, Result};
use clap::Args;
use tmux_tools_core::{
    format::Format,
    layout::{self, WindowGuard},
    target,
};

use crate::{
    cmd::launch::{concise_window_fields, invoking_cwd, WindowJson},
    util::load_registry_for_floors,
    CommonArgs,
};

/// Retile a tool-tiled window and report its geometry
///
/// Retiles the target pane's window with tmux's `tiled` layout, when tmux-tools tiled
/// it (`@tt-tiled`) and it holds a pane tmux-tools created from this cwd, then reports
/// its measured geometry and floor verdict. Outside tmux, --target, --session or
/// --window is required.
#[derive(Args, Debug)]
pub struct RebalanceArgs {
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

pub fn run(args: &RebalanceArgs) -> Result<()> {
    // Outside tmux a selector-less resolution would pick (and create) the managed
    // session; refuse before target resolution runs.
    if !has_selector(&args.common) && !target::in_tmux() {
        bail!("rebalance outside tmux needs --target, --session or --window to name the window");
    }

    let pane = target::resolve_from_common(&args.common)?;
    match layout::window_guard(&pane, invoking_cwd().as_deref())? {
        WindowGuard::Holds => {}
        WindowGuard::MarkerAbsent => bail!(
            "refusing to rebalance the window of pane {pane}: layout intent: the window does not carry @tt-tiled, so tmux-tools did not tile it"
        ),
        WindowGuard::NoOwnedPane => bail!(
            "refusing to rebalance the window of pane {pane}: ownership: no pane in the window was created by tmux-tools (@tt-name or @tt-agent) from this cwd"
        ),
    }

    let registry = load_registry_for_floors();
    let window = layout::rebalance(&pane)?.report(&registry);

    match args.common.format {
        Format::Concise => println!("rebalanced {}", concise_window_fields(&window)),
        Format::Json => println!(
            "{}",
            serde_json::to_string(&WindowJson::from_report(&window))?
        ),
        Format::Raw => println!("{}", window.window_id),
    }

    Ok(())
}

/// Whether the invocation names its window: `--target`, `--session` or `--window`.
fn has_selector(common: &CommonArgs) -> bool {
    common.target.is_some() || common.session.is_some() || common.window.is_some()
}
