//! The child-process environment of every test executable of the workspace (P10).
//!
//! Compiled only into test builds: the core's unit tests, the `tmux-tools` binary's
//! unit tests, and the integration suite (each includes this file). When the test
//! executable loads, before any test runs, it removes [`HOST_SELECTION_VARS`] from
//! its own environment and drops every `PATH` directory that holds an `orca`
//! executable. Every child process a test starts inherits that environment, so no
//! child selects a host by accident or reaches a real Orca runtime, also when the
//! suite runs inside an Orca terminal. A test that needs one of them sets it on its
//! own command. [`original_environment`] keeps the environment as the executable
//! started, for the opt-in live Orca run.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

/// The host-selection variables no child process of a test sees unless the test
/// sets them.
pub const HOST_SELECTION_VARS: [&str; 3] = [
    "TMUX_TOOLS_HOST",
    "ORCA_TERMINAL_HANDLE",
    "ORCA_WORKTREE_ID",
];

/// The process environment as the test executable started, before [`sanitize`].
pub fn original_environment() -> &'static HashMap<String, String> {
    static ORIGINAL: OnceLock<HashMap<String, String>> = OnceLock::new();
    ORIGINAL.get_or_init(|| std::env::vars().collect())
}

/// Remove [`HOST_SELECTION_VARS`] and every `PATH` directory holding an `orca`
/// executable from this process's environment. Runs once per process.
pub fn sanitize() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        original_environment();
        for var in HOST_SELECTION_VARS {
            std::env::remove_var(var);
        }
        let path = std::env::var("PATH").unwrap_or_default();
        let kept: Vec<&str> = path
            .split(':')
            .filter(|dir| !dir.is_empty() && !std::path::Path::new(dir).join("orca").exists())
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
            "env | grep -E '^(TMUX_TOOLS_HOST|ORCA_TERMINAL_HANDLE|ORCA_WORKTREE_ID)=' ; command -v orca",
        ])
        .output()
        .expect("run sh");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "",
        "a child process must see no host-selection variable and no orca"
    );
}
