#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::env;

use crate::host::{self, Host, Unsupported};
use crate::names;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSpec {
    PaneId(String),
    WindowId(String),
    Name(String),
    SmartDefault,
}

pub trait TargetArgs {
    fn target(&self) -> Option<&str>;
    fn session(&self) -> Option<&str>;
    fn window(&self) -> Option<&str>;
}

pub fn parse(s: &str) -> TargetSpec {
    if is_tmux_id(s, '%') {
        TargetSpec::PaneId(s.to_owned())
    } else if is_tmux_id(s, '@') {
        TargetSpec::WindowId(s.to_owned())
    } else {
        TargetSpec::Name(s.to_owned())
    }
}

pub fn resolve(spec: &TargetSpec, session: Option<&str>, window: Option<&str>) -> Result<String> {
    if host::current().host() == Host::Orca {
        return resolve_on_orca(spec, session, window);
    }
    match spec {
        TargetSpec::PaneId(pane_id) => Ok(pane_id.to_owned()),
        TargetSpec::WindowId(window_id) => host::current().active_pane(window_id),
        TargetSpec::Name(name) => names::find_pane_by_name(name)?
            .ok_or_else(|| anyhow!("no pane registered with name {name:?}")),
        TargetSpec::SmartDefault => resolve_smart_default(session, window),
    }
}

pub fn resolve_from_common(common: &impl TargetArgs) -> Result<String> {
    let spec = match common.target() {
        Some(target) => parse(target),
        None => TargetSpec::SmartDefault,
    };
    resolve(&spec, common.session(), common.window())
}

/// P9.1: on Orca a Target is a pane name or a `term_…` handle, and a verb without
/// `--target` acts on the calling pane. `--session` and `--window` are unsupported.
fn resolve_on_orca(
    spec: &TargetSpec,
    session: Option<&str>,
    window: Option<&str>,
) -> Result<String> {
    require_no_scope(session, window)?;
    let name = match spec {
        TargetSpec::SmartDefault => return host::current().calling_pane(),
        TargetSpec::PaneId(value) | TargetSpec::WindowId(value) | TargetSpec::Name(value) => value,
    };
    if is_orca_handle(name) {
        return Ok(name.to_owned());
    }
    names::find_pane_by_name(name)?.ok_or_else(|| anyhow!("no pane registered with name {name:?}"))
}

/// Exit 3 for `--session` or `--window` on a host without sessions and windows (P9.1).
pub fn require_no_scope(session: Option<&str>, window: Option<&str>) -> Result<()> {
    let host = host::current().host();
    if host == Host::Orca {
        if session.is_some() {
            return Err(Unsupported {
                feature: "--session",
                host,
            }
            .into());
        }
        if window.is_some() {
            return Err(Unsupported {
                feature: "--window",
                host,
            }
            .into());
        }
    }
    Ok(())
}

/// Whether `value` is an Orca terminal handle (`term_…`).
pub fn is_orca_handle(value: &str) -> bool {
    value
        .strip_prefix("term_")
        .is_some_and(|rest| !rest.is_empty())
}

fn resolve_smart_default(session: Option<&str>, window: Option<&str>) -> Result<String> {
    let host = host::current();
    if session.is_some() || window.is_some() {
        let target = host.scope_target(session, window);
        return host.active_pane(&target);
    }

    if in_tmux() {
        return host.calling_pane();
    }

    host.managed_pane()
}

/// Whether the invoking process runs inside tmux: `TMUX` is set and non-empty. Outside
/// tmux, a smart-default resolution without `--session` or `--window` targets the
/// managed session, creating it when absent.
pub fn in_tmux() -> bool {
    env::var("TMUX").map(|s| !s.is_empty()).unwrap_or(false)
}

/// Return the pane id of the pane that invoked `tmux-tools`, when running
/// inside a tmux session. Reads `$TMUX_PANE` first (set automatically by
/// tmux); falls back to the Host port's calling pane when `$TMUX_PANE` is
/// unset but `$TMUX` is set. Returns `None` outside tmux.
pub fn calling_pane_id() -> Option<String> {
    // PRD-261108-0447-01 P3.3: inside a team the calling pane is the Leader.
    if let Some(leader) = crate::team::leader_handle() {
        return Some(leader);
    }
    if host::current().host() == Host::Orca {
        return env::var(host::ORCA_TERMINAL_HANDLE)
            .ok()
            .filter(|handle| !handle.trim().is_empty());
    }
    if let Ok(pane) = env::var("TMUX_PANE") {
        let trimmed = pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    if in_tmux() {
        if let Ok(pane) = host::current().calling_pane() {
            return Some(pane);
        }
    }
    None
}

fn is_tmux_id(value: &str, prefix: char) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|char| char.is_ascii_digit()))
}
