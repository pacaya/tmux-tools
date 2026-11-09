#![allow(dead_code)]

use anyhow::{anyhow, Result};
use std::env;

use crate::{host, names};

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
