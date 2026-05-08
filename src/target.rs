#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use std::env;

use crate::{names, tmux};

pub const MANAGED_SESSION: &str = "tmux-tools";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSpec {
    PaneId(String),
    WindowId(String),
    Name(String),
    SmartDefault,
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
        TargetSpec::WindowId(window_id) => active_pane_for_target(window_id),
        TargetSpec::Name(name) => names::find_pane_by_name(name)?
            .ok_or_else(|| anyhow!("no pane registered with name {name:?}")),
        TargetSpec::SmartDefault => resolve_smart_default(session, window),
    }
}

pub(crate) fn resolve_from_common(common: &crate::CommonArgs) -> Result<String> {
    let spec = match &common.target {
        Some(target) => parse(target.as_str()),
        None => TargetSpec::SmartDefault,
    };
    resolve(&spec, common.session.as_deref(), common.window.as_deref())
}

fn resolve_smart_default(session: Option<&str>, window: Option<&str>) -> Result<String> {
    if session.is_some() || window.is_some() {
        let target = scoped_target(session, window);
        return active_pane_for_target(&target);
    }

    if env::var("TMUX").map(|s| !s.is_empty()).unwrap_or(false) {
        return current_pane();
    }

    ensure_managed_session()?;
    most_recent_pane_in_session(MANAGED_SESSION)
}

pub(crate) fn scoped_target(session: Option<&str>, window: Option<&str>) -> String {
    match (session, window) {
        (Some(session), Some(window)) => format!("{session}:{window}"),
        (Some(session), None) => session.to_owned(),
        (None, Some(window)) => format!("{MANAGED_SESSION}:{window}"),
        (None, None) => MANAGED_SESSION.to_owned(),
    }
}

fn current_pane() -> Result<String> {
    display_pane_id(&["display", "-p", "#{pane_id}"])
}

/// Return the pane id of the pane that invoked `tmux-tools`, when running
/// inside a tmux session. Reads `$TMUX_PANE` first (set automatically by
/// tmux); falls back to `tmux display-message -p '#{pane_id}'` when
/// `$TMUX_PANE` is unset but `$TMUX` is set. Returns `None` outside tmux.
pub fn calling_pane_id() -> Option<String> {
    if let Ok(pane) = env::var("TMUX_PANE") {
        let trimmed = pane.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    if env::var("TMUX").map(|s| !s.is_empty()).unwrap_or(false) {
        if let Ok(pane) = current_pane() {
            return Some(pane);
        }
    }
    None
}

fn active_pane_for_target(target: &str) -> Result<String> {
    display_pane_id(&["display", "-p", "-t", target, "#{pane_id}"])
}

fn display_pane_id(args: &[&str]) -> Result<String> {
    let pane_id = tmux::run_checked(args)?.trim().to_owned();
    if pane_id.is_empty() {
        return Err(anyhow!(
            "tmux command returned an empty pane id: {:?}",
            args
        ));
    }

    Ok(pane_id)
}

fn ensure_managed_session() -> Result<()> {
    let args = ["has-session", "-t", MANAGED_SESSION];
    let output = tmux::run_clean(&args)
        .with_context(|| format!("failed to run tmux command: tmux {}", args.join(" ")))?;

    if output.exit_code == 0 {
        return Ok(());
    }

    tmux::run_checked_clean(&["new-session", "-d", "-s", MANAGED_SESSION])?;
    Ok(())
}

fn most_recent_pane_in_session(session: &str) -> Result<String> {
    let format = "#{window_activity}\t#{window_active_pane_id}";
    let args = ["list-windows", "-t", session, "-F", format];
    let windows = tmux::run_checked_clean(&args)?;
    let mut most_recent: Option<(i64, String)> = None;

    for line in windows.lines() {
        let (activity, pane_id) = line.split_once('\t').ok_or_else(|| {
            anyhow!("unexpected tmux list-windows output for session {session:?}: {line:?}")
        })?;
        let activity = activity.parse::<i64>().with_context(|| {
            format!("failed to parse window activity {activity:?} in session {session:?}")
        })?;

        match &most_recent {
            Some((best_activity, _)) if *best_activity >= activity => {}
            _ => most_recent = Some((activity, pane_id.to_owned())),
        }
    }

    let (_, pane_id) =
        most_recent.ok_or_else(|| anyhow!("tmux session {session:?} has no windows to target"))?;
    if pane_id.is_empty() {
        return Err(anyhow!(
            "tmux list-windows returned an empty pane id for session {session:?}"
        ));
    }

    Ok(pane_id)
}

fn is_tmux_id(value: &str, prefix: char) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|char| char.is_ascii_digit()))
}

