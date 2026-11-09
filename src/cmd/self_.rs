use anyhow::Result;
use clap::Args;
use serde::Serialize;
use tmux_tools_core::{
    format::{display_value, Format},
    host::{self, Host},
    names::Registered,
    target,
    team::{self, TeamRole},
};

/// Report whether the calling pane is driven by tmux-tools
///
/// The calling pane is driven when it carries a Pane registration (`@tt-name` or
/// `@tt-agent`). `name` and `agent` are null when unknown. Outside tmux it reports
/// `host: tmux, driven: false`. Inside a team it also reports the team id and the
/// pane's role.
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
    team: Option<String>,
    role: Option<TeamRole>,
    parent: Option<String>,
}

/// The calling pane and its Pane registration, when the calling pane is known and
/// (on a registry host) has a live Pane record. Inside a team the calling pane is the
/// Leader (PRD-261108-0447-01 P3.3).
fn calling_registration() -> Result<Option<Registered>> {
    let host = host::current();
    let calling = if host.uses_pane_records() {
        // On Orca the calling pane is `ORCA_TERMINAL_HANDLE`, required (P9.1); it is
        // driven when a live Pane record holds that handle (P8).
        let handle = host.calling_pane()?;
        host.pane_names()?
            .iter()
            .any(|(pane, _)| *pane == handle)
            .then_some(handle)
    } else if target::in_tmux() {
        target::calling_pane_id()
    } else {
        None
    };
    calling
        .map(|pane| host.read_registration(&pane))
        .transpose()
}

/// The name of the calling pane when it is a tmux-tools pane (`self` reports
/// `driven: true` and a name, PRD-261108-0447-01 P7.2); else `None`. The `parent` of
/// a pane that `spawn-agent` or `launch` creates.
pub(crate) fn calling_pane_name() -> Result<Option<String>> {
    let host = host::current();
    if host.uses_pane_records() && host.calling_pane().is_err() {
        // No calling Orca pane is known, so it is no tmux-tools pane.
        return Ok(None);
    }
    match calling_registration() {
        Ok(registration) => Ok(registration.and_then(|registration| registration.name)),
        // A calling pane the host cannot read (a closed pane, or one on another server)
        // is no tmux-tools pane: `parent` is null and the create goes on (P7.1).
        Err(error) if host.is_command_failure(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn run(args: &SelfArgs) -> Result<()> {
    let registration = calling_registration()?.unwrap_or_default();
    let in_team = team::current_id();
    // P8.1: `leader` when `TT_TEAM` is set; else `teammate` when the registration has a
    // team; else null.
    let role = if in_team.is_some() {
        Some(TeamRole::Leader)
    } else if registration.team.is_some() {
        Some(TeamRole::Teammate)
    } else {
        None
    };
    let report = SelfJson {
        host: host::current().host(),
        driven: registration.name.is_some() || registration.agent.is_some(),
        name: registration.name,
        agent: registration.agent,
        team: in_team.or(registration.team),
        role,
        parent: registration.parent,
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
