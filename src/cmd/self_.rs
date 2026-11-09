use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{display_value, Format},
    host::{self, Host},
    target,
};

/// Report whether the calling pane is driven by tmux-tools
///
/// The calling pane is driven when it carries a Pane registration (`@tt-name` or
/// `@tt-agent`). `name` and `agent` are null when unknown. Outside tmux it reports
/// `host: tmux, driven: false`.
#[derive(Args, Debug)]
pub struct SelfArgs {
    #[arg(long, value_enum, default_value_t = Format::Concise)]
    pub(crate) format: Format,
}

#[derive(Serialize)]
struct SelfJson {
    host: Host,
    driven: bool,
    name: Option<String>,
    agent: Option<String>,
}

pub fn run(args: &SelfArgs) -> Result<()> {
    let host = host::current();
    let calling = if target::in_tmux() {
        target::calling_pane_id()
    } else {
        None
    };
    let (name, agent) = match calling {
        Some(pane) => {
            let registration = host.read_registration(&pane)?;
            (registration.name, registration.agent)
        }
        None => (None, None),
    };
    let report = SelfJson {
        host: host.host(),
        driven: name.is_some() || agent.is_some(),
        name,
        agent,
    };

    match args.format {
        Format::Json => println!("{}", serde_json::to_string(&report)?),
        Format::Concise | Format::Raw => println!(
            "host={} driven={} name={} agent={}",
            report.host,
            report.driven,
            display_value(report.name.as_deref()),
            display_value(report.agent.as_deref())
        ),
    }

    Ok(())
}
