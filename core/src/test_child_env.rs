//! The child-process environment of every test executable of the workspace (P10).
//!
//! Compiled only into test builds: the core's unit tests, the `tmux-tools` binary's
//! unit tests, and the integration suite (each includes this file). When the test
//! executable loads, before any test runs, it removes every variable that
//! [`is_caller_var`] names from its own environment and drops every `PATH` directory that holds
//! an `orca`, `hc` or `rex` executable or is a Team Shim directory. Every child process a test
//! starts inherits that environment, so no child selects a host by accident or reaches
//! a real Orca runtime, herdr-core or Rex server, also when the suite runs inside an
//! Orca, Herdr or Rex terminal. A test that needs one of them sets it on its own command.
//! [`original_environment`] keeps the environment as the executable started, for the
//! opt-in live Orca run, the opt-in private-core Herdr run and the opt-in
//! private-server Rex run.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

/// The host-selection variables no child process of a test sees unless the test
/// sets them.
pub const HOST_SELECTION_VARS: [&str; 8] = [
    "TMUX_TOOLS_HOST",
    "ORCA_TERMINAL_HANDLE",
    "ORCA_WORKTREE_ID",
    "HC_TERMINAL_ID",
    "TMUX_TOOLS_HERDR_SPACE",
    "TMUX_TOOLS_HERDR_TAB",
    "REX_BLOCK",
    "TMUX_TOOLS_REX_SESSION",
];

/// The Rex variables that point `rex` at a server and a session; a child never inherits
/// them, so no child reaches the default Rex server (PRD-261112-2108-01 P8.5).
pub const REX_SERVER_VARS: [&str; 2] = ["REX_SERVER", "REX_SESSION"];

/// The herdr-core variables that point `hc` at a core; a child never inherits them,
/// so no child reaches the default core (PRD-261109-2223-01 P8.3).
pub const HERDR_CORE_VARS: [&str; 3] = ["HC_SESSION", "HC_TERMINAL_SOCKET", "HC_SCOPE"];

/// The Team variables a `claude-teams` Leader passes to Claude (PRD-261108-0447-01
/// P2.3). A suite run from inside a Team must not inherit them.
pub const TEAM_VARS: [&str; 2] = ["TT_TEAM", "TT_HOST"];

/// Whether `name` is a variable of the calling host or Team, or an opt-in switch of
/// the suite, that no child process of a test sees unless the test sets it
/// (ISSUE-261118-0414-02): every `TT_*`, `TMUX`, `TMUX_PANE`, every `REX_*`,
/// `HC_TERMINAL_ID`, every `ORCA_*`, every `TMUX_TOOLS_LIVE_*` switch, and the
/// variables of [`HOST_SELECTION_VARS`], [`HERDR_CORE_VARS`], [`REX_SERVER_VARS`]
/// and [`TEAM_VARS`].
pub fn is_caller_var(name: &str) -> bool {
    ["TT_", "REX_", "ORCA_", "TMUX_TOOLS_LIVE_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["TMUX", "TMUX_PANE", "HC_TERMINAL_ID"].contains(&name)
        || HOST_SELECTION_VARS
            .into_iter()
            .chain(HERDR_CORE_VARS)
            .chain(REX_SERVER_VARS)
            .chain(TEAM_VARS)
            .any(|var| var == name)
}

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

/// Remove every variable that [`is_caller_var`] names, every `PATH` directory holding an `orca`, `hc` or `rex` executable, and every Team Shim directory from
/// this process's environment,
/// so the suite also runs from inside a `claude-teams` Leader. Runs once per process.
pub fn sanitize() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        for var in original_environment().keys() {
            if is_caller_var(var) {
                std::env::remove_var(var);
            }
        }
        let path = std::env::var("PATH").unwrap_or_default();
        let kept: Vec<&str> = path
            .split(':')
            .filter(|dir| {
                !dir.is_empty()
                    && !std::path::Path::new(dir).join("orca").exists()
                    && !std::path::Path::new(dir).join("hc").exists()
                    && !std::path::Path::new(dir).join("rex").exists()
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
            "env | grep -E '^(TMUX_TOOLS_HOST|ORCA_TERMINAL_HANDLE|ORCA_WORKTREE_ID|HC_TERMINAL_ID|TMUX_TOOLS_HERDR_SPACE|TMUX_TOOLS_HERDR_TAB|HC_SESSION|HC_TERMINAL_SOCKET|HC_SCOPE|REX_BLOCK|REX_SERVER|REX_SESSION|TMUX_TOOLS_REX_SESSION|TT_TEAM|TT_HOST)=' ; command -v orca; command -v hc; command -v rex",
        ])
        .output()
        .expect("run sh");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "",
        "a child process must see no host-selection variable, no orca, no hc and no rex"
    );
}
