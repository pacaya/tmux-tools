//! End-to-end integration smoke tests for the tmux-tools binary.
//!
//! These tests shell out to the real `tmux` binary on the user's default
//! server, so they:
//!   - use uniquely-named panes (`shell-<pid>` / `codex-test-<pid>`) per run
//!     so they only ever touch panes they created themselves
//!   - clean up those panes via `tmux-tools kill` (and a best-effort tmux
//!     `kill-pane` fallback) regardless of test outcome
//!
//! NOTE on `--session ttests-<pid>` from the spec: the current `launch`
//! implementation hardcodes the managed-session name (see
//! src/cmd/launch.rs:69-86 — it always targets `target::MANAGED_SESSION`)
//! and ignores `args.common.session` for *creating* panes. To run these
//! tests without modifying production code, we drive every invocation with
//! a unique `--name` instead and rely on name-based targeting. This is
//! documented in the test report. If a future implementation honors
//! `--session` in launch, the per-run isolation can be tightened by
//! routing into a unique session.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-tools");

#[derive(Debug)]
struct CmdOutput {
    status: i32,
    stdout: String,
    stderr: String,
}

impl CmdOutput {
    fn assert_success(&self, ctx: &str) {
        assert_eq!(
            self.status, 0,
            "{ctx}: expected exit 0, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.status, self.stdout, self.stderr
        );
    }
}

fn run_bin(args: &[&str]) -> CmdOutput {
    run_bin_with_timeout(args, Duration::from_secs(60))
}

fn run_bin_with_timeout(args: &[&str], timeout: Duration) -> CmdOutput {
    let mut child = Command::new(BIN)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut stderr);
                }
                return CmdOutput {
                    status: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "tmux-tools {args:?} timed out after {:?}",
                        timeout
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("error waiting for tmux-tools {args:?}: {e}"),
        }
    }
}

fn run_tmux(args: &[&str]) -> CmdOutput {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn tmux {args:?}: {e}"));
    CmdOutput {
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Best-effort cleanup helper: kills tmux-tools panes by name on Drop.
struct PaneGuard {
    names: Vec<String>,
}

impl PaneGuard {
    fn new() -> Self {
        Self { names: Vec::new() }
    }

    fn track(&mut self, name: &str) {
        self.names.push(name.to_owned());
    }
}

impl Drop for PaneGuard {
    fn drop(&mut self) {
        for name in &self.names {
            // Try via tmux-tools first (also clears the registry entries).
            let _ = run_bin(&["kill", "--target", name]);
            // Fallback: if a pane is still around with that registered name,
            // brute-force find and kill it directly via tmux.
            if let Some(pane) = pane_id_by_name(name) {
                let _ = run_tmux(&["kill-pane", "-t", &pane]);
            }
        }
    }
}

/// Resolve a pane id by registered `@tt-name` using tmux directly. Returns
/// `None` if no such pane exists.
fn pane_id_by_name(name: &str) -> Option<String> {
    let out = run_tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{@tt-name}",
    ]);
    if out.status != 0 {
        return None;
    }
    for line in out.stdout.lines() {
        if let Some((pane, tt_name)) = line.split_once('\t') {
            if tt_name == name {
                return Some(pane.to_owned());
            }
        }
    }
    None
}

/// Wait until a child shell prompt is settled enough to accept commands. We
/// simply poll for the named pane to exist *and* render at least one line of
/// output (typically the bash prompt).
fn wait_for_pane_ready(name: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(name) {
            let cap = run_tmux(&["capture-pane", "-t", &pane, "-p"]);
            if cap.status == 0 && !cap.stdout.trim().is_empty() {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("pane {name} did not become ready within {timeout:?}");
}

#[test]
fn full_smoke() {
    // Skip entirely if tmux is missing — there is nothing meaningful to test.
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let pid = std::process::id();
    let shell_name = format!("shell-{pid}");
    let codex_name = format!("codex-test-{pid}");

    let mut guard = PaneGuard::new();
    guard.track(&shell_name);
    guard.track(&codex_name);

    // Deferred failures (so all 8 cases run before we fail the test). We
    // capture per-case failure messages and panic at the end if any survived.
    let mut deferred_failures: Vec<String> = Vec::new();

    // ---- 1. launch ---------------------------------------------------------
    let launched = run_bin(&[
        "launch",
        "--cmd",
        "bash --norc --noprofile",
        "--name",
        &shell_name,
        "--format",
        "json",
    ]);
    launched.assert_success("launch");
    let launch_json: serde_json::Value =
        serde_json::from_str(launched.stdout.trim()).expect("launch JSON should parse");
    let pane_id = launch_json
        .get("pane_id")
        .and_then(|v| v.as_str())
        .expect("launch JSON must include pane_id")
        .to_owned();
    assert!(pane_id.starts_with('%'), "pane_id must look like %N: {pane_id}");

    // Verify @tt-name was set on the pane.
    let tt_name = run_tmux(&["display-message", "-p", "-t", &pane_id, "#{@tt-name}"]);
    tt_name.assert_success("display @tt-name after launch");
    assert_eq!(
        tt_name.stdout.trim(),
        shell_name,
        "@tt-name should match --name"
    );

    // Wait for bash to print its first prompt before sending input.
    wait_for_pane_ready(&shell_name, Duration::from_secs(5));

    // ---- 2. send + capture -------------------------------------------------
    let sent = run_bin(&[
        "send",
        "--target",
        &shell_name,
        "echo hi",
        "--enter",
    ]);
    sent.assert_success("send echo hi");

    thread::sleep(Duration::from_millis(400));

    let cap = run_bin(&[
        "capture",
        "--target",
        &shell_name,
        "--lines",
        "20",
        "--format",
        "raw",
    ]);
    cap.assert_success("capture after echo");
    assert!(
        cap.stdout.contains("hi"),
        "capture should contain the echoed 'hi':\n{}",
        cap.stdout
    );

    // ---- 3. execute (success) ---------------------------------------------
    let exec_ls = run_bin(&[
        "execute",
        "--target",
        &shell_name,
        "ls /tmp",
        "--format",
        "json",
        "--timeout",
        "10",
    ]);
    exec_ls.assert_success("execute ls /tmp");
    let exec_ls_json: serde_json::Value =
        serde_json::from_str(exec_ls.stdout.trim()).expect("execute JSON should parse");
    assert_eq!(
        exec_ls_json.get("exit_code").and_then(|v| v.as_i64()),
        Some(0),
        "execute ls /tmp exit_code should be 0; payload: {}",
        exec_ls.stdout
    );
    assert_eq!(
        exec_ls_json.get("timed_out").and_then(|v| v.as_bool()),
        Some(false),
        "timed_out should be false"
    );
    let output = exec_ls_json
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !output.is_empty(),
        "execute output should be non-empty for ls /tmp"
    );

    // ---- 4. execute (non-zero exit) ---------------------------------------
    // Use `(exit 7)` so the wrapping subshell propagates exit 7 cleanly.
    let exec_fail = run_bin(&[
        "execute",
        "--target",
        &shell_name,
        "(exit 7)",
        "--format",
        "json",
        "--timeout",
        "10",
    ]);
    exec_fail.assert_success("execute (exit 7)");
    let exec_fail_json: serde_json::Value =
        serde_json::from_str(exec_fail.stdout.trim()).expect("execute fail JSON should parse");
    assert_eq!(
        exec_fail_json.get("exit_code").and_then(|v| v.as_i64()),
        Some(7),
        "execute (exit 7) exit_code should be 7; payload: {}",
        exec_fail.stdout
    );

    // ---- 5. capture --lines 0 (regression) --------------------------------
    let cap0 = run_bin(&[
        "capture",
        "--target",
        &shell_name,
        "--lines",
        "0",
        "--format",
        "json",
    ]);
    cap0.assert_success("capture --lines 0");
    let cap0_json: serde_json::Value =
        serde_json::from_str(cap0.stdout.trim()).expect("capture --lines 0 JSON should parse");
    let cap0_output = cap0_json
        .get("output")
        .and_then(|v| v.as_str())
        .expect("capture JSON should have output field");
    let cap0_lines = cap0_json
        .get("lines")
        .and_then(|v| v.as_u64())
        .expect("capture JSON should have lines field");
    if !(cap0_output.is_empty() || cap0_lines == 0) {
        deferred_failures.push(format!(
            "REGRESSION: capture --lines 0 returned non-empty output (lines={cap0_lines}). \
             tmux's `capture-pane -S -0 -E -` semantically captures the visible pane; \
             `--lines 0` should short-circuit to an empty result instead. \
             Likely fix site: src/cmd/capture.rs::build_capture_args."
        ));
    }

    // ---- 6. spawn-agent codex (conditional) -------------------------------
    let codex_available = Command::new("codex")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if codex_available {
        let spawn = run_bin_with_timeout(
            &[
                "spawn-agent",
                "codex",
                "--access",
                "read-only",
                "--name",
                &codex_name,
                "--format",
                "json",
            ],
            Duration::from_secs(15),
        );
        spawn.assert_success("spawn-agent codex");
        let spawn_json: serde_json::Value =
            serde_json::from_str(spawn.stdout.trim()).expect("spawn-agent JSON should parse");
        let codex_pane = spawn_json
            .get("pane_id")
            .and_then(|v| v.as_str())
            .expect("spawn-agent JSON must include pane_id")
            .to_owned();
        let agent_tag = run_tmux(&[
            "display-message",
            "-p",
            "-t",
            &codex_pane,
            "#{@tt-agent}",
        ]);
        agent_tag.assert_success("display @tt-agent for codex pane");
        assert_eq!(
            agent_tag.stdout.trim(),
            "codex",
            "@tt-agent should be 'codex' for spawned codex pane"
        );

        // Best-effort: wait for codex's ready regex up to 10s. We don't fail
        // the test if codex itself fails to start cleanly (it might be
        // unauthenticated etc.), only if wait-idle's CLI surface itself
        // crashes.
        let _wait = run_bin_with_timeout(
            &[
                "wait-idle",
                "--target",
                &codex_name,
                "--until",
                "^▌",
                "--timeout",
                "10",
            ],
            Duration::from_secs(20),
        );
        // We intentionally don't assert on `_wait.status` since codex may not
        // be authenticated in the test environment; the goal of step 6 is
        // mostly to cover spawn-agent + @tt-agent registration.
    } else {
        eprintln!("SKIP step 6: codex not on PATH");
    }

    // ---- 7. list -----------------------------------------------------------
    let list_out = run_bin(&[
        "list",
        "--format",
        "json",
        "--all",
    ]);
    list_out.assert_success("list --format json --all");
    let list_json: serde_json::Value = serde_json::from_str(list_out.stdout.trim())
        .expect("list JSON should parse");
    let arr = list_json
        .as_array()
        .expect("list output should be a JSON array");
    let found_shell = arr
        .iter()
        .any(|row| row.get("name").and_then(|v| v.as_str()) == Some(shell_name.as_str()));
    assert!(
        found_shell,
        "list should include the {shell_name} pane; got: {}",
        list_out.stdout
    );

    // ---- 8. kill -----------------------------------------------------------
    let kill_out = run_bin(&["kill", "--target", &shell_name]);
    kill_out.assert_success("kill shell pane");

    // Allow tmux a moment to remove the pane.
    thread::sleep(Duration::from_millis(200));

    let list_after = run_bin(&[
        "list",
        "--format",
        "json",
        "--all",
    ]);
    list_after.assert_success("list after kill");
    let list_after_json: serde_json::Value = serde_json::from_str(list_after.stdout.trim())
        .expect("post-kill list JSON should parse");
    let arr_after = list_after_json
        .as_array()
        .expect("post-kill list should be JSON array");
    let still_there = arr_after
        .iter()
        .any(|row| row.get("name").and_then(|v| v.as_str()) == Some(shell_name.as_str()));
    assert!(
        !still_there,
        "shell pane should be gone after kill; got: {}",
        list_after.stdout
    );

    // ---- final: surface deferred failures ---------------------------------
    if !deferred_failures.is_empty() {
        let combined = deferred_failures.join("\n");
        panic!(
            "{} deferred failure(s):\n{combined}",
            deferred_failures.len()
        );
    }
}
