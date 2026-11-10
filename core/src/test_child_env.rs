//! The child-process environment of every test executable of the workspace (P10).
//!
//! Compiled only into test builds: the core's unit tests, the `tmux-tools` binary's
//! unit tests, and the integration suite (each includes this file). When the test
//! executable loads, before any test runs, it removes [`HOST_SELECTION_VARS`] and
//! [`TEAM_VARS`] from its own environment and drops every `PATH` directory that holds
//! an `orca` or `hc` executable or is a Team Shim directory. Every child process a test
//! starts inherits that environment, so no child selects a host by accident or reaches
//! a real Orca runtime or herdr-core, also when the suite runs inside an Orca or Herdr
//! terminal. A test that needs one of them sets it on its own command.
//! [`original_environment`] keeps the environment as the executable started, for the
//! opt-in live Orca run and the opt-in private-core Herdr run.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

/// The host-selection variables no child process of a test sees unless the test
/// sets them.
pub const HOST_SELECTION_VARS: [&str; 6] = [
    "TMUX_TOOLS_HOST",
    "ORCA_TERMINAL_HANDLE",
    "ORCA_WORKTREE_ID",
    "HC_TERMINAL_ID",
    "TMUX_TOOLS_HERDR_SPACE",
    "TMUX_TOOLS_HERDR_TAB",
];

/// The herdr-core variables that point `hc` at a core; a child never inherits them,
/// so no child reaches the default core (PRD-261109-2223-01 P8.3).
pub const HERDR_CORE_VARS: [&str; 3] = ["HC_SESSION", "HC_TERMINAL_SOCKET", "HC_SCOPE"];

/// The Team variables a `claude-teams` Leader passes to Claude (PRD-261108-0447-01
/// P2.3). A suite run from inside a Team must not inherit them.
pub const TEAM_VARS: [&str; 2] = ["TT_TEAM", "TT_HOST"];

/// Whether `dir` is a Team's Shim directory, `<team dir>/bin` beside `team.lock`.
fn is_shim_dir(dir: &str) -> bool {
    let dir = std::path::Path::new(dir);
    dir.file_name().is_some_and(|name| name == "bin")
        && dir
            .parent()
            .is_some_and(|team| team.join("team.lock").exists())
}

/// The process environment as the test executable started, before [`sanitize`].
pub fn original_environment() -> &'static HashMap<String, String> {
    static ORIGINAL: OnceLock<HashMap<String, String>> = OnceLock::new();
    ORIGINAL.get_or_init(|| std::env::vars().collect())
}

/// Remove [`HOST_SELECTION_VARS`], [`HERDR_CORE_VARS`], [`TEAM_VARS`], every `PATH`
/// directory holding an `orca` or `hc` executable, and every Team Shim directory from
/// this process's environment,
/// so the suite also runs from inside a `claude-teams` Leader. Runs once per process.
pub fn sanitize() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        original_environment();
        for var in HOST_SELECTION_VARS
            .into_iter()
            .chain(HERDR_CORE_VARS)
            .chain(TEAM_VARS)
        {
            std::env::remove_var(var);
        }
        let path = std::env::var("PATH").unwrap_or_default();
        let kept: Vec<&str> = path
            .split(':')
            .filter(|dir| {
                !dir.is_empty()
                    && !std::path::Path::new(dir).join("orca").exists()
                    && !std::path::Path::new(dir).join("hc").exists()
                    && !is_shim_dir(dir)
            })
            .collect();
        std::env::set_var("PATH", kept.join(":"));
    });
}

/// Runs [`sanitize`] when the executable loads, before the test harness starts any
/// test thread.
#[used]
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link_section = "__DATA,__mod_init_func"
)]
#[cfg_attr(
    any(target_os = "linux", target_os = "android", target_os = "freebsd"),
    link_section = ".init_array"
)]
static SANITIZE_AT_LOAD: extern "C" fn() = {
    extern "C" fn sanitize_at_load() {
        sanitize();
    }
    sanitize_at_load
};

#[test]
fn child_processes_see_no_host_selection_variables_and_no_orca() {
    let out = std::process::Command::new("sh")
        .args([
            "-c",
            "env | grep -E '^(TMUX_TOOLS_HOST|ORCA_TERMINAL_HANDLE|ORCA_WORKTREE_ID|HC_TERMINAL_ID|TMUX_TOOLS_HERDR_SPACE|TMUX_TOOLS_HERDR_TAB|HC_SESSION|HC_TERMINAL_SOCKET|HC_SCOPE|TT_TEAM|TT_HOST)=' ; command -v orca; command -v hc",
        ])
        .output()
        .expect("run sh");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "",
        "a child process must see no host-selection variable, no orca and no hc"
    );
}
