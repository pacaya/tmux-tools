//! Shared safety checks for destructive verbs (`kill`, `interrupt`, `escape`).
//!
//! Two independent guards are applied before the verb acts:
//!
//! 1. **Self-target guard.** If the resolved target equals the calling
//!    pane (`$TMUX_PANE`), the verb refuses unless `--force` is passed. This
//!    prevents accidental self-kill / self-interrupt of the user's own pane.
//!
//! 2. **Ownership + cwd guard.** Destructive verbs only act on panes
//!    created by tmux-tools (those with `@tt-name` or `@tt-agent` set) and,
//!    when `@tt-cwd` is recorded, only on panes whose recorded cwd matches
//!    the current cwd. `--any` overrides both checks.
//!
//! Both flags default to off and are independent: `--force` lets you act on
//! the calling pane; `--any` lets you act on cross-cwd or non-owned panes.

use anyhow::{anyhow, Result};

use crate::{names, names::Registered, target};

/// Verdict produced by [`evaluate`]: either `Allow` (proceed) or `Deny`
/// with a human-readable reason. Pure function — easy to unit-test without
/// a tmux server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Allow,
    Deny(String),
}

/// Inputs for [`evaluate`]. Constructed by callers from CLI args, the
/// resolved target pane id, the pane's `@tt-*` registration, the calling
/// pane id (`$TMUX_PANE`), and the current working directory.
#[derive(Clone, Debug)]
pub struct SafetyInput<'a> {
    pub verb: &'a str,
    pub target_pane: &'a str,
    pub calling_pane: Option<&'a str>,
    pub registered: &'a Registered,
    pub current_cwd: Option<&'a str>,
    pub force: bool,
    pub any: bool,
}

/// Apply self-target, ownership, and cwd-scope checks in that order. The
/// first failing check produces a `Deny`; all checks pass -> `Allow`.
pub fn evaluate(input: &SafetyInput<'_>) -> Verdict {
    // Self-target check.
    if let Some(calling) = input.calling_pane {
        if calling == input.target_pane && !input.force {
            return Verdict::Deny(format!(
                "refusing to {} the calling pane {}; pass --force to override",
                input.verb, input.target_pane
            ));
        }
    }

    // Ownership check (must have @tt-name or @tt-agent).
    let owned = input.registered.name.is_some() || input.registered.agent.is_some();
    if !owned && !input.any {
        return Verdict::Deny(format!(
            "refusing to {} pane {} not created by tmux-tools (no @tt-name or @tt-agent set); pass --any to override",
            input.verb, input.target_pane
        ));
    }

    // Cwd-scope check (only when @tt-cwd was recorded).
    if owned {
        if let (Some(pane_cwd), Some(current)) = (input.registered.cwd.as_deref(), input.current_cwd) {
            if pane_cwd != current && !input.any {
                return Verdict::Deny(format!(
                    "refusing to {} pane {} owned by another cwd ({}); pass --any to override",
                    input.verb, input.target_pane, pane_cwd
                ));
            }
        }
    }

    Verdict::Allow
}

/// Convenience wrapper: turns a `Deny` verdict into an `anyhow::Error`.
pub fn enforce(input: &SafetyInput<'_>) -> Result<()> {
    match evaluate(input) {
        Verdict::Allow => Ok(()),
        Verdict::Deny(msg) => Err(anyhow!(msg)),
    }
}

pub fn enforce_for_pane(verb: &'static str, pane: &str, force: bool, any: bool) -> Result<Registered> {
    let registered = names::read(pane)?;
    let calling = target::calling_pane_id();
    let current_cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    enforce(&SafetyInput {
        verb,
        target_pane: pane,
        calling_pane: calling.as_deref(),
        registered: &registered,
        current_cwd: current_cwd.as_deref(),
        force,
        any,
    })?;
    Ok(registered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_reg() -> Registered {
        Registered {
            name: None,
            agent: None,
            access: None,
            launched_at: None,
            cwd: None,
        }
    }

    fn owned_reg(cwd: Option<&str>) -> Registered {
        Registered {
            name: Some("shell".to_owned()),
            agent: None,
            access: None,
            launched_at: None,
            cwd: cwd.map(str::to_owned),
        }
    }

    #[test]
    fn allow_when_target_owned_and_cwd_matches() {
        let reg = owned_reg(Some("/work"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn deny_when_target_is_calling_pane_and_no_force() {
        let reg = owned_reg(Some("/work"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%1",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        match evaluate(&input) {
            Verdict::Deny(msg) => {
                assert!(msg.contains("calling pane"), "msg: {msg}");
                assert!(msg.contains("--force"), "msg: {msg}");
                assert!(msg.contains("kill"), "msg: {msg}");
                assert!(msg.contains("%1"), "msg: {msg}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn allow_self_target_when_force_set() {
        let reg = owned_reg(Some("/work"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%1",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: true,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn deny_when_pane_not_owned_by_tmux_tools() {
        let reg = empty_reg();
        let input = SafetyInput {
            verb: "interrupt",
            target_pane: "%9",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        match evaluate(&input) {
            Verdict::Deny(msg) => {
                assert!(msg.contains("not created by tmux-tools"), "msg: {msg}");
                assert!(msg.contains("--any"), "msg: {msg}");
                assert!(msg.contains("interrupt"), "msg: {msg}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn allow_unowned_pane_when_any_set() {
        let reg = empty_reg();
        let input = SafetyInput {
            verb: "interrupt",
            target_pane: "%9",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: true,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn deny_when_pane_cwd_differs_and_no_any() {
        let reg = owned_reg(Some("/other"));
        let input = SafetyInput {
            verb: "escape",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        match evaluate(&input) {
            Verdict::Deny(msg) => {
                assert!(msg.contains("another cwd"), "msg: {msg}");
                assert!(msg.contains("/other"), "msg: {msg}");
                assert!(msg.contains("--any"), "msg: {msg}");
                assert!(msg.contains("escape"), "msg: {msg}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn allow_cross_cwd_when_any_set() {
        let reg = owned_reg(Some("/other"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: true,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn allow_owned_pane_with_no_recorded_cwd() {
        // Older panes that pre-date @tt-cwd should still be killable without
        // `--any` as long as they're owned (have @tt-name or @tt-agent).
        let reg = owned_reg(None);
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn allow_when_calling_pane_unknown_outside_tmux() {
        let reg = owned_reg(Some("/work"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: None,
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn allow_when_current_cwd_unknown() {
        // If we can't read the current cwd, skip the cwd check rather than
        // fail closed (matches the "no-op" semantics for missing context).
        let reg = owned_reg(Some("/other"));
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: None,
            force: false,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn ownership_recognised_via_agent_only() {
        let reg = Registered {
            name: None,
            agent: Some("codex".to_owned()),
            access: None,
            launched_at: None,
            cwd: Some("/work".to_owned()),
        };
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%5",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }

    #[test]
    fn self_kill_check_runs_before_ownership_check() {
        // Even if the pane is unowned, --force on self-target means we
        // should fall through to the ownership check; without --any that
        // still denies. Order matters: with neither flag, the self-target
        // message should fire first.
        let reg = empty_reg();
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%1",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: false,
            any: false,
        };
        match evaluate(&input) {
            Verdict::Deny(msg) => {
                assert!(msg.contains("calling pane"), "msg: {msg}");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn force_and_any_together_allow_self_unowned_pane() {
        let reg = empty_reg();
        let input = SafetyInput {
            verb: "kill",
            target_pane: "%1",
            calling_pane: Some("%1"),
            registered: &reg,
            current_cwd: Some("/work"),
            force: true,
            any: true,
        };
        assert_eq!(evaluate(&input), Verdict::Allow);
    }
}
