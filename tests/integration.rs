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
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tmux-tools");

/// Serializes the integration suite. Each test calls `tmux-tools launch ...`
/// against the test runner's pane via `tmux split-window`; running multiple
/// tests in parallel races on the shared tmux server (concurrent splits
/// against the same pane hit tmux's size constraints and pane-id lookups
/// see partially-registered panes). Hold this guard for the lifetime of a
/// test to keep things deterministic under default `cargo test`.
fn serial_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

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
    let child = Command::new(BIN)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));

    wait_for_child(child, &format!("{BIN} {args:?}"), timeout)
}

/// Wait for a spawned child, then collect its captured stdout/stderr. Shared by
/// the plain and environment-injected runners so their timeout and panic
/// behavior cannot drift.
fn wait_for_child(mut child: std::process::Child, label: &str, timeout: Duration) -> CmdOutput {
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
                    panic!("{label} timed out after {timeout:?}");
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("error waiting for {label}: {e}"),
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
    let out = run_tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@tt-name}"]);
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
    let _serial = serial_guard();

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
    // `--bare` so the launched bash is exactly what the test inspects below;
    // the default keep-open wrap would replace bash with `$SHELL` if that bash
    // ever exited. Bash never exits during this test (we only send commands to
    // it), but `--bare` makes the intent explicit and removes a future
    // debugger's surprise.
    let launched = run_bin(&[
        "launch",
        "--cmd",
        "bash --norc --noprofile",
        "--name",
        &shell_name,
        "--bare",
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
    assert!(
        pane_id.starts_with('%'),
        "pane_id must look like %N: {pane_id}"
    );

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
    let sent = run_bin(&["send", "--target", &shell_name, "echo hi", "--enter"]);
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
        let agent_tag = run_tmux(&["display-message", "-p", "-t", &codex_pane, "#{@tt-agent}"]);
        agent_tag.assert_success("display @tt-agent for codex pane");
        assert_eq!(
            agent_tag.stdout.trim(),
            "codex",
            "@tt-agent should be 'codex' for spawned codex pane"
        );

        // Best-effort: wait for codex's status line up to 10s. We don't fail
        // the test if codex itself fails to start cleanly (it might be
        // unauthenticated etc.), only if wait-idle's CLI surface itself
        // crashes. `· Ready · Context` is codex's idle status line; with the
        // default `--ready-stable-seconds` debounce it must hold ~2s before
        // firing `until_matched`.
        let _wait = run_bin_with_timeout(
            &[
                "wait-idle",
                "--target",
                &codex_name,
                "--until",
                "· Ready · Context",
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
    let list_out = run_bin(&["list", "--format", "json", "--all"]);
    list_out.assert_success("list --format json --all");
    let list_json: serde_json::Value =
        serde_json::from_str(list_out.stdout.trim()).expect("list JSON should parse");
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

    let list_after = run_bin(&["list", "--format", "json", "--all"]);
    list_after.assert_success("list after kill");
    let list_after_json: serde_json::Value =
        serde_json::from_str(list_after.stdout.trim()).expect("post-kill list JSON should parse");
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

#[test]
fn wait_idle_timeout_concise_first_line_includes_idle_for() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("wait-hint-line-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "bash --norc --noprofile",
        "--name",
        &name,
        "--bare",
    ]);
    launched.assert_success("launch wait-idle hint pane");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let changing_name = name.clone();
    let change = thread::spawn(move || {
        thread::sleep(Duration::from_millis(750));
        let sent = run_bin(&[
            "send",
            "--target",
            &changing_name,
            "printf 'controlled-change-marker\\n'",
            "--enter",
        ]);
        sent.assert_success("emit controlled output during wait-idle");
    });

    let waited = run_bin(&[
        "wait-idle",
        "--target",
        &name,
        "--idle-seconds",
        "60",
        "--timeout",
        "2",
    ]);
    change
        .join()
        .expect("controlled pane update should complete");
    waited.assert_success("wait-idle timeout");

    let first_line = waited.stdout.lines().next().unwrap_or_default();
    let expected_shape =
        regex::Regex::new(r"^reason=timed_out duration=(\d+\.\d{3}) idle_for=(\d+\.\d{3})$")
            .expect("test regex compiles");
    let captures = expected_shape.captures(first_line).unwrap_or_else(|| {
        panic!("timeout first line should expose duration and idle age; got: {first_line:?}")
    });
    let duration = captures[1]
        .parse::<f64>()
        .expect("duration should be numeric");
    let idle_for = captures[2]
        .parse::<f64>()
        .expect("idle_for should be numeric");
    assert!(
        idle_for >= 0.75,
        "idle_for should measure a stable interval after the controlled pane update; got {idle_for:.3}s"
    );
    assert!(
        duration - idle_for >= 0.5,
        "idle_for should reset after output changes and be materially less than total duration; duration={duration:.3}s idle_for={idle_for:.3}s"
    );
}

#[test]
fn wait_idle_timeout_concise_includes_delimited_bottom_ten_non_blank_lines() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("wait-hint-tail-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "printf 'line-01\nline-02\n\nline-03\nline-04\nline-05\nline-06\nline-07\nline-08\nline-09\nline-10\nline-11\nline-12\n'; sleep 30",
        "--name",
        &name,
        "--bare",
    ]);
    launched.assert_success("launch wait-idle tail pane");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let waited = run_bin(&[
        "wait-idle",
        "--target",
        &name,
        "--idle-seconds",
        "60",
        "--timeout",
        "0.1",
    ]);
    waited.assert_success("wait-idle timeout with tail");

    let lines: Vec<&str> = waited.stdout.lines().collect();
    assert_eq!(
        lines.get(1).copied(),
        Some("--- timeout hint: bottom 10 non-blank lines ---"),
        "timeout tail should be separated by a greppable provenance marker"
    );
    assert_eq!(
        lines.get(2..),
        Some(
            [
                "line-03", "line-04", "line-05", "line-06", "line-07", "line-08", "line-09",
                "line-10", "line-11", "line-12",
            ]
            .as_slice()
        ),
        "timeout tail should contain exactly the bottom ten non-blank pane lines"
    );
}

#[test]
fn wait_idle_hint_lines_positive_value_controls_timeout_tail_length() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("wait-hint-three-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "printf 'line-01\nline-02\nline-03\nline-04\nline-05\n'; sleep 30",
        "--name",
        &name,
        "--bare",
    ]);
    launched.assert_success("launch wait-idle custom tail pane");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let waited = run_bin(&[
        "wait-idle",
        "--target",
        &name,
        "--idle-seconds",
        "60",
        "--timeout",
        "0.1",
        "--hint-lines",
        "3",
    ]);
    waited.assert_success("wait-idle timeout with custom tail length");

    let lines: Vec<&str> = waited.stdout.lines().collect();
    assert_eq!(
        lines.get(1).copied(),
        Some("--- timeout hint: bottom 3 non-blank lines ---"),
        "timeout delimiter should report the configured positive tail length"
    );
    assert_eq!(
        lines.get(2..),
        Some(["line-03", "line-04", "line-05"].as_slice()),
        "--hint-lines 3 should contain exactly the bottom three non-blank pane lines"
    );
}

#[test]
fn wait_idle_hint_lines_zero_suppresses_timeout_tail() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("wait-hint-off-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "printf 'pane output\n'; sleep 30",
        "--name",
        &name,
        "--bare",
    ]);
    launched.assert_success("launch wait-idle no-tail pane");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let waited = run_bin(&[
        "wait-idle",
        "--target",
        &name,
        "--idle-seconds",
        "60",
        "--timeout",
        "0.1",
        "--hint-lines",
        "0",
    ]);
    waited.assert_success("wait-idle timeout with hint disabled");

    assert_eq!(
        waited.stdout.lines().count(),
        1,
        "--hint-lines 0 should leave only the timeout reason line; got:\n{}",
        waited.stdout
    );
}

#[test]
fn wait_idle_timeout_json_includes_numeric_idle_for() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("wait-hint-json-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "printf 'json pane output\n'; sleep 30",
        "--name",
        &name,
        "--bare",
    ]);
    launched.assert_success("launch wait-idle JSON pane");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let waited = run_bin(&[
        "wait-idle",
        "--target",
        &name,
        "--idle-seconds",
        "60",
        "--timeout",
        "0.1",
        "--format",
        "json",
    ]);
    waited.assert_success("wait-idle timeout JSON");

    let payload: serde_json::Value =
        serde_json::from_str(waited.stdout.trim()).expect("wait-idle JSON should parse");
    assert_eq!(
        payload.get("reason").and_then(|value| value.as_str()),
        Some("timed_out"),
        "controlled wait should exercise the timeout JSON path"
    );
    assert!(
        payload
            .get("idle_for")
            .and_then(|value| value.as_f64())
            .is_some(),
        "timeout JSON should expose idle_for as numeric seconds; payload: {}",
        waited.stdout
    );
    assert!(
        payload
            .get("final_capture")
            .and_then(|value| value.as_str())
            .is_some_and(|capture| capture.contains("json pane output")),
        "timeout JSON final_capture should retain the known pane text; payload: {}",
        waited.stdout
    );
}

/// Default `launch` wraps the command so the pane survives its exit. A
/// non-zero-exit command should leave its output visible in scrollback.
#[test]
fn launch_keeps_pane_alive_after_cmd_exit() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let pid = std::process::id();
    let name = format!("keepopen-{pid}");
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Command prints "hello" then exits non-zero. Without the keep-open wrap,
    // tmux would reap the pane and "hello" would be lost. `echo` (not `printf`)
    // puts it on its own line, so a keep-alive shell prompt that clears the
    // current line cannot overwrite it before the capture.
    let launched = run_bin(&[
        "launch",
        "--cmd",
        "echo hello; exit 1",
        "--name",
        &name,
        "--format",
        "json",
    ]);
    launched.assert_success("launch with default keep-open wrap");

    wait_for_pane_ready(&name, Duration::from_secs(5));

    // Give the wrap shell a moment to take over after `exit 1`.
    thread::sleep(Duration::from_millis(500));

    let cap = run_bin(&[
        "capture", "--target", &name, "--lines", "20", "--format", "raw",
    ]);
    cap.assert_success("capture after wrapped cmd exit");
    assert!(
        cap.stdout.contains("hello"),
        "capture should retain output from the exited command:\n{}",
        cap.stdout
    );

    assert!(
        pane_id_by_name(&name).is_some(),
        "pane should still be alive after the wrapped command exited"
    );
}

/// `capture --lines N` is documented as a tail. Regression for the bug where
/// it was wired as `tmux capture-pane -S -N -E -` (a *lookback* — include N
/// rows of scrollback before the visible pane) so on a fresh pane all small
/// N values returned the same full visible buffer.
#[test]
fn capture_lines_tails_visible_pane() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let pid = std::process::id();
    let name = format!("tail-{pid}");
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let launched = run_bin(&[
        "launch",
        "--cmd",
        "bash --norc --noprofile",
        "--name",
        &name,
        "--bare",
        "--format",
        "json",
    ]);
    launched.assert_success("launch tail pane");

    wait_for_pane_ready(&name, Duration::from_secs(5));

    let sent = run_bin(&[
        "send",
        "--target",
        &name,
        "for i in $(seq 1 30); do echo line-$i; done",
        "--enter",
    ]);
    sent.assert_success("send seq 1..30 burst");

    thread::sleep(Duration::from_millis(500));

    let cap = run_bin(&[
        "capture", "--target", &name, "--lines", "5", "--format", "raw",
    ]);
    cap.assert_success("capture --lines 5 after 30-line burst");

    // The tail also includes the trailing shell prompt, so `--lines 5` yields
    // ~4 numeric lines + the prompt. Assert the latest output is present and
    // that earlier lines are absent — the latter is the regression signal:
    // the buggy lookback wiring would have included the entire visible pane,
    // which contains every `line-1` … `line-30` row.
    for tail_line in ["line-29", "line-30"] {
        assert!(
            cap.stdout.contains(tail_line),
            "capture --lines 5 should include {tail_line}; got:\n{}",
            cap.stdout
        );
    }
    for early_line in ["line-1\n", "line-10", "line-20"] {
        assert!(
            !cap.stdout.contains(early_line),
            "capture --lines 5 must not include {early_line:?} \
             (proves we tailed rather than captured the whole visible pane); got:\n{}",
            cap.stdout
        );
    }
}

/// Regression: when several tmux sessions are opened in a short window by
/// other means, `tmux-tools launch` (no target flags) used to split whichever
/// pane was most-recently-active on the tmux server — i.e. the decoy session
/// — instead of the calling pane. Fix passes `$TMUX_PANE` as `-t` explicitly.
///
/// The test simulates the bug by:
///   1. creating a `harness` session whose pane id we pretend is the caller,
///   2. creating a `decoy` session afterward (so tmux considers it the
///      most-recently-active pane),
///   3. invoking `tmux-tools launch` with `TMUX` and `TMUX_PANE` set to point
///      at the harness pane,
///   4. asserting the new pane's `#{session_name}` is the harness session,
///      not the decoy.
#[test]
fn launch_targets_calling_pane_not_most_recent_client() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let pid = std::process::id();
    let harness_session = format!("tt-harness-{pid}");
    let decoy_session = format!("tt-decoy-{pid}");
    let name = format!("calling-{pid}");

    struct SessionGuard(String);
    impl Drop for SessionGuard {
        fn drop(&mut self) {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", &self.0])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _harness_guard = SessionGuard(harness_session.clone());
    let _decoy_guard = SessionGuard(decoy_session.clone());
    let mut pane_guard = PaneGuard::new();
    pane_guard.track(&name);

    // Best-effort cleanup of any lingering sessions from a previous run with
    // the same pid (highly unlikely but cheap).
    let _ = run_tmux(&["kill-session", "-t", &harness_session]);
    let _ = run_tmux(&["kill-session", "-t", &decoy_session]);

    // 1. Create the harness session first, then capture its pane id.
    run_tmux(&["new-session", "-d", "-s", &harness_session])
        .assert_success("create harness session");
    let harness_pane = run_tmux(&[
        "display-message",
        "-p",
        "-t",
        &harness_session,
        "#{pane_id}",
    ]);
    harness_pane.assert_success("query harness pane id");
    let harness_pane_id = harness_pane.stdout.trim().to_owned();
    assert!(
        harness_pane_id.starts_with('%'),
        "harness pane id must look like %N: {harness_pane_id}"
    );

    // 2. Create the decoy session AFTER, so it becomes the
    //    most-recently-active session on the server. With TMUX_PANE absent
    //    (next step) and no `-t`, `tmux split-window` would land here —
    //    that's the bug.
    run_tmux(&["new-session", "-d", "-s", &decoy_session]).assert_success("create decoy session");

    // 3. Build a TMUX env var pointing at the *harness* session, but
    //    deliberately leave TMUX_PANE *unset*. Format:
    //    "<socket>,<server_pid>,<session_id_numeric>". This is the most
    //    realistic reproduction of the user-reported bug: TMUX is set
    //    correctly to the caller's session but TMUX_PANE was lost somewhere
    //    in process spawning (e.g. when a long-lived agent inherits TMUX
    //    from a wrapper). Without our fix, tmux's implicit "current pane"
    //    falls back to most-recently-active across the whole server — i.e.
    //    the decoy.
    let socket_q = run_tmux(&["display-message", "-p", "#{socket_path}"]);
    socket_q.assert_success("query socket_path");
    let socket = socket_q.stdout.trim().to_owned();
    let server_pid_q = run_tmux(&["display-message", "-p", "#{pid}"]);
    server_pid_q.assert_success("query server pid");
    let server_pid = server_pid_q.stdout.trim().to_owned();
    let harness_session_id_q = run_tmux(&[
        "display-message",
        "-p",
        "-t",
        &harness_session,
        "#{session_id}",
    ]);
    harness_session_id_q.assert_success("query harness session_id");
    let harness_session_id_num = harness_session_id_q
        .stdout
        .trim()
        .trim_start_matches('$')
        .to_owned();
    let tmux_env = format!("{socket},{server_pid},{harness_session_id_num}");

    // Avoid using `harness_pane_id` directly via TMUX_PANE: we want this
    // test to simulate the case where TMUX_PANE is missing. Suppress the
    // unused-binding warning explicitly.
    let _ = &harness_pane_id;

    // Invoke `tmux-tools launch` with TMUX set but TMUX_PANE explicitly
    // *unset*. The fix must still resolve the calling session via TMUX and
    // pass `-t` so the split lands in the harness, not in the decoy.
    let mut child = Command::new(BIN)
        .args([
            "launch", "--cmd", "sleep 30", "--name", &name, "--bare", "--format", "json",
        ])
        .env("TMUX", &tmux_env)
        .env_remove("TMUX_PANE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN} launch: {e}"));

    let started = Instant::now();
    let timeout = Duration::from_secs(15);
    let (status, stdout, stderr) = loop {
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
                break (status.code().unwrap_or(-1), stdout, stderr);
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("tmux-tools launch timed out after {timeout:?}");
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("error waiting for tmux-tools launch: {e}"),
        }
    };

    assert_eq!(
        status, 0,
        "launch should succeed with TMUX_PANE override; stdout={stdout} stderr={stderr}"
    );
    let launch_json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("launch JSON should parse");
    let new_pane = launch_json
        .get("pane_id")
        .and_then(|v| v.as_str())
        .expect("launch JSON must include pane_id")
        .to_owned();

    // 4. The new pane must live in the harness session, not the decoy.
    let session_q = run_tmux(&["display-message", "-p", "-t", &new_pane, "#{session_name}"]);
    session_q.assert_success("query session_name for new pane");
    let actual_session = session_q.stdout.trim();
    assert_eq!(
        actual_session, harness_session,
        "new pane should land in harness session ({harness_session}), not {actual_session} \
         (decoy was {decoy_session}) — regression for $TMUX_PANE-based targeting"
    );
}

// ---------------------------------------------------------------------------
// P4 — `capture --all` reports its own ceiling.
//
// These tests drive the feature-gated `tt-fake-tui` fixture, so they compile
// and run only under the documented `--features test-fixtures` command. A bare
// `cargo test` skips them (the fixture binary is not built).
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
const FAKE_TUI_BIN: &str = env!("CARGO_BIN_EXE_tt-fake-tui");

/// Launch `tt-fake-tui` in a uniquely-named bare pane and wait until it
/// renders its ready banner.
#[cfg(feature = "test-fixtures")]
fn launch_fake_tui(name: &str) {
    launch_fake_tui_with(name, &[]);
}

/// Launch `tt-fake-tui` with extra fixture arguments (`--report`, `--no-bracketed-paste`,
/// `--ready`, ...) in a uniquely-named bare pane and wait until it renders.
#[cfg(feature = "test-fixtures")]
fn launch_fake_tui_with(name: &str, extra_args: &[&str]) {
    let mut cmd = shell_quote(FAKE_TUI_BIN);
    for arg in extra_args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(arg));
    }
    let launched = run_bin(&[
        "launch", "--cmd", &cmd, "--name", name, "--bare", "--format", "json",
    ]);
    launched.assert_success("launch tt-fake-tui");
    wait_for_pane_ready(name, Duration::from_secs(5));
}

/// Single-quote a value for the shell command `launch --cmd` runs.
#[cfg(feature = "test-fixtures")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Drive the fixture's stdin control channel (`alt` / `normal` / `quit`).
#[cfg(feature = "test-fixtures")]
fn fake_tui_command(name: &str, command: &str) {
    let sent = run_bin(&["send", "--target", name, command, "--literal", "--enter"]);
    sent.assert_success("send tt-fake-tui command");
}

/// Poll until the named pane's live `alternate_on` matches `expected`.
#[cfg(feature = "test-fixtures")]
fn wait_for_alternate_on(name: &str, expected: bool, timeout: Duration) {
    let expected_flag = if expected { "1" } else { "0" };
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(name) {
            let out = run_tmux(&["display-message", "-p", "-t", &pane, "#{alternate_on}"]);
            if out.status == 0 && out.stdout.trim() == expected_flag {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("pane {name} did not reach alternate_on={expected_flag} within {timeout:?}");
}

/// Poll the visible pane until it contains `needle`. Used to wait out the
/// fixture's post-switch report so later captures race nothing.
#[cfg(feature = "test-fixtures")]
fn wait_for_pane_text(name: &str, needle: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(name) {
            let cap = run_tmux(&["capture-pane", "-t", &pane, "-p"]);
            if cap.status == 0 && cap.stdout.contains(needle) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("pane {name} did not render {needle:?} within {timeout:?}");
}

/// Poll the visible pane until it no longer contains `needle`.
#[cfg(feature = "test-fixtures")]
fn wait_for_pane_text_absent(name: &str, needle: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(name) {
            let cap = run_tmux(&["capture-pane", "-t", &pane, "-p"]);
            if cap.status == 0 && !cap.stdout.contains(needle) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("pane {name} still rendered {needle:?} after {timeout:?}");
}

/// The current visible text of a registered pane.
#[cfg(feature = "test-fixtures")]
fn capture_pane_text(name: &str) -> String {
    let pane = pane_id_by_name(name).expect("pane should exist");
    let cap = run_tmux(&["capture-pane", "-t", &pane, "-p"]);
    cap.assert_success("capture pane");
    cap.stdout
}

/// Switch the fixture's live screen, then wait until both tmux's `alternate_on`
/// and the fixture's rendered state reflect the switch.
#[cfg(feature = "test-fixtures")]
fn set_fake_tui_alternate(name: &str, on: bool) {
    let (command, expected_flag, rendered) = if on {
        ("alt", true, "tt-fake-tui: on the alternate screen")
    } else {
        ("normal", false, "tt-fake-tui: on the normal screen")
    };
    fake_tui_command(name, command);
    wait_for_alternate_on(name, expected_flag, Duration::from_secs(5));
    wait_for_pane_text(name, rendered, Duration::from_secs(5));
}

/// The documented concise rendering of a raw capture: `capture-pane -p` emits
/// no ANSI, so this trims each line's trailing whitespace and drops trailing
/// blank lines. Used as the independent pre-change expected value for concise
/// stdout (the fixture emits no idle-prompt-only lines, so the concise
/// duplicate collapse does not apply).
#[cfg(feature = "test-fixtures")]
fn expected_concise(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().map(str::trim_end).collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Poll the named pane's live `bracket_paste_flag` until it matches `expected`.
/// A fixture that enables bracketed paste flips this when tmux processes its
/// `\x1b[?2004h`, which may trail the ready banner by one poll.
#[cfg(feature = "test-fixtures")]
fn wait_for_bracket_paste_flag(name: &str, expected: bool, timeout: Duration) {
    let expected_flag = if expected { "1" } else { "0" };
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(name) {
            let out = run_tmux(&[
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{bracket_paste_flag}",
            ]);
            if out.status == 0 && out.stdout.trim() == expected_flag {
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("pane {name} did not reach bracket_paste_flag={expected_flag} within {timeout:?}");
}

/// One framed record from the fixture's `--report` file.
#[cfg(feature = "test-fixtures")]
#[derive(Debug)]
struct ReportEvent {
    kind: String,
    bytes: Vec<u8>,
}

/// Parse the fixture's length-framed report: a `"<kind> <len>\n"` header followed
/// by exactly `len` bytes. A trailing partial frame is ignored so a poll can read
/// the file while the fixture is still writing.
#[cfg(feature = "test-fixtures")]
fn read_report(path: &std::path::Path) -> Vec<ReportEvent> {
    let data = std::fs::read(path).unwrap_or_default();
    let mut events = Vec::new();
    let mut rest: &[u8] = &data;
    while let Some(newline) = rest.iter().position(|&byte| byte == b'\n') {
        let Ok(header) = std::str::from_utf8(&rest[..newline]) else {
            break;
        };
        let mut parts = header.split(' ');
        let Some(kind) = parts.next() else { break };
        let Some(len) = parts.next().and_then(|len| len.parse::<usize>().ok()) else {
            break;
        };
        let body_start = newline + 1;
        if rest.len() < body_start + len {
            break;
        }
        events.push(ReportEvent {
            kind: kind.to_owned(),
            bytes: rest[body_start..body_start + len].to_vec(),
        });
        rest = &rest[body_start + len..];
    }
    events
}

/// Poll the report until `predicate` holds, returning the events seen. Used
/// because the fixture writes the report asynchronously with respect to the
/// `prompt` process exiting.
#[cfg(feature = "test-fixtures")]
fn wait_for_report<F>(path: &std::path::Path, timeout: Duration, predicate: F) -> Vec<ReportEvent>
where
    F: Fn(&[ReportEvent]) -> bool,
{
    let started = Instant::now();
    loop {
        let events = read_report(path);
        if predicate(&events) {
            return events;
        }
        if started.elapsed() > timeout {
            let summary: Vec<String> = events
                .iter()
                .map(|event| format!("{}:{}", event.kind, event.bytes.len()))
                .collect();
            panic!(
                "report at {path:?} did not satisfy the predicate within {timeout:?}; events: \
                 {summary:?}"
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Assert tmux shows no buffer carrying the prompt transport's name.
#[cfg(feature = "test-fixtures")]
fn assert_no_prompt_buffer() {
    let out = run_tmux(&["list-buffers"]);
    assert_eq!(
        out.status, 0,
        "tmux list-buffers failed: stdout={:?} stderr={:?}",
        out.stdout, out.stderr
    );
    assert!(
        !out.stdout.contains("tmux-tools-prompt"),
        "a prompt buffer leaked into the paste ring: {:?}",
        out.stdout
    );
}

/// A payload larger than tmux's `send-keys` argument ceiling (measured between
/// 16000 accepted and 20000 rejected) and containing embedded newlines.
#[cfg(feature = "test-fixtures")]
fn large_multiline_prompt() -> String {
    let mut text = String::from("PROMPT-BEGIN\n");
    let mut index = 0;
    while text.len() < 24_000 {
        text.push_str(&format!(
            "payload-line-{index:05} abcdefghijklmnopqrstuvwxyz\n"
        ));
        index += 1;
    }
    text.push_str("PROMPT-END");
    text
}

/// The absolute path of the real tmux, so an interposed fake `tmux` can delegate
/// every subcommand except the one under test.
#[cfg(feature = "test-fixtures")]
fn real_tmux_path() -> String {
    let output = Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .expect("failed to resolve tmux");
    assert!(output.status.success(), "tmux is not on PATH");
    String::from_utf8(output.stdout)
        .expect("tmux path is UTF-8")
        .trim()
        .to_owned()
}

/// A fake `tmux` that fails one subcommand and delegates everything else to the
/// real binary, so a test can inject a transport failure after `load-buffer`.
#[cfg(feature = "test-fixtures")]
fn tmux_wrapper_failing(subcommand: &str) -> String {
    format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"{subcommand}\" ]; then\n\
         \techo \"injected {subcommand} failure\" >&2\n\
         \texit 1\n\
         fi\n\
         exec '{}' \"$@\"\n",
        real_tmux_path()
    )
}

/// A fake `tmux` that, after a successful `load-buffer`, turns the target pane's
/// live bracketed-paste flag off (by telling the fixture to emit
/// `\x1b[?2004l`) and waits until tmux reflects it. This makes the flag read
/// that happens after `load-buffer` see a clear flag deterministically,
/// exercising the refusal that must happen between load and paste.
#[cfg(feature = "test-fixtures")]
fn tmux_wrapper_flipping_bracket_paste(pane_id: &str) -> String {
    let real = real_tmux_path();
    format!(
        "#!/bin/sh\n\
         REAL='{real}'\n\
         if [ \"$1\" = \"load-buffer\" ]; then\n\
         \t\"$REAL\" \"$@\" || exit $?\n\
         \t\"$REAL\" send-keys -t '{pane_id}' -l 'bracket off'\n\
         \t\"$REAL\" send-keys -t '{pane_id}' Enter\n\
         \ti=0\n\
         \twhile [ $i -lt 100 ]; do\n\
         \t\tflag=$(\"$REAL\" display-message -p -t '{pane_id}' '#{{bracket_paste_flag}}')\n\
         \t\t[ \"$flag\" = \"0\" ] && break\n\
         \t\ti=$((i+1))\n\
         \t\tsleep 0.02\n\
         \tdone\n\
         \texit 0\n\
         fi\n\
         exec \"$REAL\" \"$@\"\n"
    )
}

/// A fake `tmux` that records a successful `load-buffer` and then fails the next
/// `display-message`, so a test can make the post-load `bracket_paste_flag` read
/// fail while leaving the earlier reads intact.
#[cfg(feature = "test-fixtures")]
fn tmux_wrapper_failing_post_load_flag_read(state_path: &std::path::Path) -> String {
    let real = real_tmux_path();
    let state = state_path.display();
    format!(
        "#!/bin/sh\n\
         REAL='{real}'\n\
         STATE='{state}'\n\
         if [ \"$1\" = \"load-buffer\" ]; then\n\
         \t\"$REAL\" \"$@\" || exit $?\n\
         \t: > \"$STATE\"\n\
         \texit 0\n\
         fi\n\
         if [ \"$1\" = \"display-message\" ] && [ -f \"$STATE\" ]; then\n\
         \techo \"injected post-load flag read failure\" >&2\n\
         \texit 1\n\
         fi\n\
         exec \"$REAL\" \"$@\"\n"
    )
}

/// P4: `--all` against a pane on the alternate screen leaves raw and concise
/// stdout byte-identical and reports the ceiling on stderr.
#[cfg(feature = "test-fixtures")]
#[test]
fn capture_all_on_alternate_screen_keeps_stdout_and_warns_on_stderr() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("capture-alt-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui(&name);
    set_fake_tui_alternate(&name, true);

    let pane = pane_id_by_name(&name).expect("fixture pane should exist");
    // Independent oracle: tmux's own full-history capture is exactly what
    // `capture --all --format raw` printed before this change.
    let expected_raw = run_tmux(&["capture-pane", "-p", "-S", "-", "-t", &pane]);
    expected_raw.assert_success("tmux capture-pane -p -S -");

    let raw = run_bin(&["capture", "--target", &name, "--all", "--format", "raw"]);
    raw.assert_success("capture --all raw");
    assert_eq!(
        raw.stdout, expected_raw.stdout,
        "raw stdout must stay byte-identical to the pre-change full-history capture"
    );
    assert!(
        raw.stderr.contains("full history is unavailable")
            && raw.stderr.to_lowercase().contains("alternate screen"),
        "raw stderr should carry the alternate-screen ceiling notice; got: {:?}",
        raw.stderr
    );
    assert!(
        !raw.stdout.contains("full history"),
        "the notice must not leak into raw stdout; got: {:?}",
        raw.stdout
    );

    let concise = run_bin(&["capture", "--target", &name, "--all", "--format", "concise"]);
    concise.assert_success("capture --all concise");
    assert_eq!(
        concise.stdout,
        expected_concise(&expected_raw.stdout),
        "concise stdout must stay byte-identical to the pre-change concise rendering"
    );
    assert!(
        concise.stderr.contains("full history is unavailable")
            && concise.stderr.to_lowercase().contains("alternate screen"),
        "concise stderr should carry the ceiling notice; got: {:?}",
        concise.stderr
    );
    assert!(
        !concise.stdout.contains("full history"),
        "the notice must not leak into concise stdout; got: {:?}",
        concise.stdout
    );
}

/// P4: JSON gains a structured field saying scrollback is unavailable.
#[cfg(feature = "test-fixtures")]
#[test]
fn capture_all_on_alternate_screen_reports_scrollback_unavailable_in_json() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("capture-alt-json-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui(&name);
    set_fake_tui_alternate(&name, true);

    let out = run_bin(&["capture", "--target", &name, "--all", "--format", "json"]);
    out.assert_success("capture --all json on alternate screen");
    let payload: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("capture JSON should parse");
    assert_eq!(
        payload
            .get("scrollback_available")
            .and_then(|v| v.as_bool()),
        Some(false),
        "alternate-screen JSON must say scrollback is unavailable; payload: {}",
        out.stdout
    );
    assert!(
        payload
            .get("lines")
            .and_then(|v| v.as_u64())
            .is_some_and(|lines| lines > 0),
        "capture JSON must still report its actual line count; payload: {}",
        out.stdout
    );
    assert!(
        out.stderr.is_empty(),
        "JSON reports the ceiling through its field, not stderr; got: {:?}",
        out.stderr
    );
}

/// P4: the determination reads live pane state, so entering the alternate
/// screen after launch turns the notice on and leaving it turns the notice off.
#[cfg(feature = "test-fixtures")]
#[test]
fn capture_all_tracks_live_alternate_screen_state() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("capture-alt-live-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui(&name);

    // Each observation runs the human format (whose notice is on stderr) and
    // the JSON format (whose fact is the `scrollback_available` field).
    let observe = |ctx: &str| -> (String, serde_json::Value) {
        let human = run_bin(&["capture", "--target", &name, "--all"]);
        human.assert_success(ctx);
        let json = run_bin(&["capture", "--target", &name, "--all", "--format", "json"]);
        json.assert_success(ctx);
        assert!(
            json.stderr.is_empty(),
            "{ctx}: JSON reports through its field, not stderr; got: {:?}",
            json.stderr
        );
        let payload = serde_json::from_str(json.stdout.trim()).expect("capture JSON should parse");
        (human.stderr, payload)
    };

    let (before_stderr, before) = observe("capture --all on the normal screen");
    assert!(
        before_stderr.is_empty(),
        "a normal-screen pane must not warn; got: {before_stderr:?}"
    );
    assert_eq!(
        before.get("scrollback_available").and_then(|v| v.as_bool()),
        Some(true),
        "a normal-screen pane can supply scrollback; payload: {before}"
    );

    set_fake_tui_alternate(&name, true);

    let (entered_stderr, entered) = observe("capture --all after entering the alternate screen");
    assert!(
        entered_stderr.contains("full history is unavailable"),
        "entering the alternate screen after launch must produce the notice; got: {entered_stderr:?}"
    );
    assert_eq!(
        entered
            .get("scrollback_available")
            .and_then(|v| v.as_bool()),
        Some(false),
        "entering the alternate screen must flag scrollback unavailable; payload: {entered}"
    );

    set_fake_tui_alternate(&name, false);

    let (left_stderr, left) = observe("capture --all after leaving the alternate screen");
    assert!(
        left_stderr.is_empty(),
        "leaving the alternate screen must silence the notice; got: {left_stderr:?}"
    );
    assert_eq!(
        left.get("scrollback_available").and_then(|v| v.as_bool()),
        Some(true),
        "leaving the alternate screen must not say scrollback is unavailable; payload: {left}"
    );
}

/// P4: a `--lines N` shortfall is not a structural ceiling and stays silent;
/// the actual `lines` count still reports what was captured.
#[cfg(feature = "test-fixtures")]
#[test]
fn capture_lines_shortfall_stays_silent() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let name = format!("capture-lines-short-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui(&name);

    let requested = 200_u64;
    let out = run_bin(&[
        "capture",
        "--target",
        &name,
        "--lines",
        &requested.to_string(),
        "--format",
        "json",
    ]);
    out.assert_success("capture --lines on a short pane");
    assert!(
        out.stderr.is_empty(),
        "a --lines shortfall must stay silent; got: {:?}",
        out.stderr
    );

    let payload: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("capture JSON should parse");
    let actual = payload
        .get("lines")
        .and_then(|v| v.as_u64())
        .expect("capture JSON should report a numeric lines count");
    assert!(
        actual < requested,
        "the fixture pane should hold fewer than {requested} lines; got {actual}"
    );
    let output = payload
        .get("output")
        .and_then(|v| v.as_str())
        .expect("capture JSON should carry output");
    assert!(
        !output.trim().is_empty(),
        "the shortfall capture must not be empty; payload: {}",
        out.stdout
    );

    // Independent oracle: read the same pane's content directly from tmux
    // rather than trusting the response's own `output`/`lines` pair.
    let pane = pane_id_by_name(&name).expect("fixture pane should exist");
    let range = format!("-{requested}");
    let oracle = run_tmux(&["capture-pane", "-t", &pane, "-p", "-S", &range, "-E", "-"]);
    oracle.assert_success("tmux capture-pane -p -S -200 -E -");
    let oracle_lines = oracle.stdout.trim_end().lines().count() as u64;
    assert!(
        oracle_lines > 0,
        "the independent tmux capture should be non-empty; got: {:?}",
        oracle.stdout
    );
    assert_eq!(
        actual, oracle_lines,
        "reported lines must match an independent tmux capture of the fixture pane"
    );
    assert!(
        payload.get("scrollback_available").is_none(),
        "a --lines capture makes no scrollback claim; payload: {}",
        out.stdout
    );

    let raw = run_bin(&[
        "capture",
        "--target",
        &name,
        "--lines",
        &requested.to_string(),
        "--format",
        "raw",
    ]);
    raw.assert_success("capture --lines raw on a short pane");
    assert!(
        raw.stderr.is_empty(),
        "a --lines shortfall must stay silent in raw format too; got: {:?}",
        raw.stderr
    );
}

/// `--bare` opts out of the wrap. The pane should disappear once the launched
/// command exits.
#[test]
fn launch_bare_pane_closes_when_cmd_exits() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let pid = std::process::id();
    let name = format!("bare-{pid}");

    // `sleep 0.3` keeps the pane alive long enough for `launch` to register
    // `@tt-name` before tmux reaps it.
    let launched = run_bin(&[
        "launch",
        "--cmd",
        "sleep 0.3",
        "--bare",
        "--name",
        &name,
        "--format",
        "json",
    ]);
    launched.assert_success("launch --bare");

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if pane_id_by_name(&name).is_none() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Best-effort cleanup if the pane unexpectedly outlived the sleep.
    let _ = run_bin(&["kill", "--target", &name]);
    panic!(
        "bare pane {name} should have closed after the command exited; \
         it was still listed after 3s"
    );
}

// ---------------------------------------------------------------------------
// P2/P3 — surface model, selection, recording, and the pane-state classifier.
//
// These tests drive the feature-gated `tt-fake-tui` fixture and inject a
// throwaway `agents.toml` through `XDG_CONFIG_HOME`, so they compile and run
// only under the documented `--features test-fixtures` command.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
struct AgentsConfig {
    dir: std::path::PathBuf,
}

#[cfg(feature = "test-fixtures")]
impl AgentsConfig {
    fn new() -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tt-agents-{}-{suffix}", std::process::id()));
        std::fs::create_dir_all(dir.join("tmux-tools")).unwrap();
        Self { dir }
    }

    fn xdg(&self) -> &std::path::Path {
        &self.dir
    }

    fn write_agents_toml(&self, contents: &str) {
        std::fs::write(self.dir.join("tmux-tools").join("agents.toml"), contents).unwrap();
    }

    fn write_executable(&self, name: &str, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = self.dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
        path
    }
}

#[cfg(feature = "test-fixtures")]
impl Drop for AgentsConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(feature = "test-fixtures")]
fn run_bin_env(args: &[&str], xdg: &std::path::Path) -> CmdOutput {
    run_bin_env_with_timeout(args, xdg, Duration::from_secs(60))
}

#[cfg(feature = "test-fixtures")]
fn run_bin_env_with_timeout(args: &[&str], xdg: &std::path::Path, timeout: Duration) -> CmdOutput {
    let child = Command::new(BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", xdg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));

    wait_for_child(child, &format!("{BIN} {args:?}"), timeout)
}

/// Run the CLI with `dir` prepended to `PATH`, so a test can interpose a fake
/// `tmux` on the command's tmux calls.
#[cfg(feature = "test-fixtures")]
fn run_bin_env_path(args: &[&str], xdg: &std::path::Path, dir: &std::path::Path) -> CmdOutput {
    let path = std::env::var("PATH").unwrap_or_default();
    let child = Command::new(BIN)
        .args(args)
        .env("XDG_CONFIG_HOME", xdg)
        .env("PATH", format!("{}:{path}", dir.display()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));

    wait_for_child(child, &format!("{BIN} {args:?}"), Duration::from_secs(60))
}

#[cfg(feature = "test-fixtures")]
fn pane_option(name: &str, key: &str) -> String {
    let pane = pane_id_by_name(name).expect("pane should exist");
    let out = run_tmux(&["display-message", "-p", "-t", &pane, &format!("#{{{key}}}")]);
    out.assert_success("display pane option");
    out.stdout.trim().to_owned()
}

/// Wait with the injected registry and return `(reason, final_capture)`.
#[cfg(feature = "test-fixtures")]
fn wait_idle_json(name: &str, xdg: &std::path::Path, extra: &[&str]) -> (String, String) {
    let mut args = vec![
        "wait-idle",
        "--target",
        name,
        "--format",
        "json",
        "--timeout",
        "10",
    ];
    args.extend_from_slice(extra);
    let out = run_bin_env(&args, xdg);
    out.assert_success("wait-idle");
    let payload: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("wait-idle JSON should parse");
    (
        payload
            .get("reason")
            .and_then(|v| v.as_str())
            .expect("wait-idle JSON should carry reason")
            .to_owned(),
        payload
            .get("final_capture")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned(),
    )
}

/// The custom agent used by the default-resolution acceptance items: its default
/// surface (`flat`) is not its pre-surface rendering (`rich`), each surface renders a
/// distinguishable ready/busy footer through its own launch arguments, and `flat`
/// carries a validated pattern pair so it is actually promoted.
#[cfg(feature = "test-fixtures")]
fn surface_agent_toml() -> String {
    format!(
        r#"
[surfagent]
binary = "{fake}"
default_surface = "flat"
pre_surface_rendering = "rich"

[surfagent.surfaces.rich]
args = ["--ready", "RICH-SURFACE-READY", "--busy", "RICH-SURFACE-BUSY"]
ready_regex = "^tt-fake-tui: ready: RICH-SURFACE-READY$"

[surfagent.surfaces.flat]
args = ["--ready", "FLAT-SURFACE-READY", "--busy", "FLAT-SURFACE-BUSY"]
ready_regex = "^tt-fake-tui: ready: FLAT-SURFACE-READY$"
busy_regex = "^tt-fake-tui: busy: FLAT-SURFACE-BUSY$"
validated_version = "surfagent 1.0"

[surfagent.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    )
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_no_surface_settles_on_default_not_pre_surface() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&surface_agent_toml());

    let name = format!("surface-default-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "surfagent",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent surfagent on default surface");
    assert_eq!(pane_option(&name, "@tt-surface"), "flat");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "the default surface's ready footer must match; capture: {capture:?}"
    );
    assert!(
        capture.contains("FLAT-SURFACE-READY"),
        "the default (flat) surface's footer must be on screen; capture: {capture:?}"
    );
    assert!(
        !capture.contains("RICH-SURFACE-READY"),
        "the pre-surface footer must not have been rendered; capture: {capture:?}"
    );
}

/// P2's promotion bar: a flat surface named as the default is only honored once it
/// carries a validated `ready_regex`/`busy_regex` pair stamped with the agent version.
/// Otherwise `spawn-agent` keeps the agent's former (pre-surface) default.
#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_falls_back_to_pre_surface_when_flat_default_is_not_validated() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[unstamped]
binary = "{fake}"
default_surface = "flat"
pre_surface_rendering = "rich"

[unstamped.surfaces.rich]
args = ["--ready", "UNSTAMPED-RICH-READY", "--busy", "UNSTAMPED-RICH-BUSY"]
ready_regex = "^tt-fake-tui: ready: UNSTAMPED-RICH-READY$"

# Both patterns are present, but the surface carries no validated_version, so it is
# not promoted and does not become the default.
[unstamped.surfaces.flat]
args = ["--ready", "UNSTAMPED-FLAT-READY", "--busy", "UNSTAMPED-FLAT-BUSY"]
ready_regex = "^tt-fake-tui: ready: UNSTAMPED-FLAT-READY$"
busy_regex = "^tt-fake-tui: busy: UNSTAMPED-FLAT-BUSY$"

[unstamped.access.default]
args = []

[nobusy]
binary = "{fake}"
default_surface = "flat"
pre_surface_rendering = "rich"

[nobusy.surfaces.rich]
args = ["--ready", "NOBUSY-RICH-READY", "--busy", "NOBUSY-RICH-BUSY"]
ready_regex = "^tt-fake-tui: ready: NOBUSY-RICH-READY$"

# No busy_regex at all: the pair is incomplete, so the flat surface is not promoted.
[nobusy.surfaces.flat]
args = ["--ready", "NOBUSY-FLAT-READY", "--busy", "NOBUSY-FLAT-BUSY"]
ready_regex = "^tt-fake-tui: ready: NOBUSY-FLAT-READY$"
validated_version = "nobusy 1.0"

[nobusy.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let mut guard = PaneGuard::new();
    for (agent, marker, missing) in [
        ("unstamped", "UNSTAMPED", "validated_version"),
        ("nobusy", "NOBUSY", "busy_regex"),
    ] {
        let name = format!("surface-fallback-{agent}-{}", std::process::id());
        guard.track(&name);

        let spawn = run_bin_env(
            &[
                "spawn-agent",
                agent,
                "--split",
                "v",
                "--bare",
                "--name",
                &name,
                "--format",
                "json",
            ],
            config.xdg(),
        );
        spawn.assert_success("spawn-agent on an unvalidated flat default");
        assert_eq!(
            pane_option(&name, "@tt-surface"),
            "rich",
            "{agent}: an unvalidated flat default must fall back to the pre-surface rendering"
        );
        assert!(
            spawn.stderr.contains("warning:")
                && spawn.stderr.contains("default_surface \"flat\"")
                && spawn.stderr.contains("recording \"rich\""),
            "{agent}: the declared default must be reported as replaced; stderr: {:?}",
            spawn.stderr
        );
        assert!(
            spawn.stderr.contains(missing),
            "{agent}: the warning must name the missing {missing}; stderr: {:?}",
            spawn.stderr
        );

        wait_for_pane_ready(&name, Duration::from_secs(5));
        let (reason, capture) =
            wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
        assert_eq!(
            reason, "ready_matched",
            "{agent}: the pre-surface rendering's own pattern must match; capture: {capture:?}"
        );
        assert!(
            capture.contains(&format!("{marker}-RICH-READY")),
            "{agent}: the pre-surface footer must be rendered; capture: {capture:?}"
        );
        assert!(
            !capture.contains(&format!("{marker}-FLAT-READY")),
            "{agent}: the unpromoted flat rendering must not have launched; capture: {capture:?}"
        );
    }
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_records_surface_and_marks_caller_args_unvalidated() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&surface_agent_toml());

    // A named surface that supplies its own launch arguments is still validated.
    let named = format!("surface-named-{}", std::process::id());
    let caller = format!("surface-caller-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&named);
    guard.track(&caller);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "surfagent",
            "--surface",
            "rich",
            "--bare",
            "--name",
            &named,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent with --surface rich");
    assert_eq!(pane_option(&named, "@tt-surface"), "rich");
    assert_eq!(
        pane_option(&named, "@tt-surface-unvalidated"),
        "",
        "surface-supplied arguments must not set the unvalidated mark"
    );

    // Caller-supplied trailing arguments mark the pane surface-unvalidated.
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "surfagent",
            "--bare",
            "--name",
            &caller,
            "--",
            "--caller-supplied",
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent with trailing caller arguments");
    assert_eq!(pane_option(&caller, "@tt-surface"), "flat");
    assert_eq!(
        pane_option(&caller, "@tt-surface-unvalidated"),
        "1",
        "caller-supplied arguments must mark the pane surface-unvalidated"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn pane_without_surface_record_resolves_to_pre_surface_rendering() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&surface_agent_toml());

    let name = format!("surface-absent-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Launch on the pre-surface rendering, then strip the record: only a pane created
    // before surfaces existed looks like this.
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "surfagent",
            "--surface",
            "rich",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent on rich surface");
    let pane = pane_id_by_name(&name).expect("spawned pane should exist");
    let unset = run_tmux(&["set-option", "-p", "-u", "-t", &pane, "@tt-surface"]);
    unset.assert_success("unset @tt-surface");

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "an absent record must resolve to the pre-surface rendering; capture: {capture:?}"
    );
    assert!(
        capture.contains("RICH-SURFACE-READY"),
        "the pre-surface footer must be on screen; capture: {capture:?}"
    );
    assert!(
        !capture.contains("FLAT-SURFACE-READY"),
        "the default surface's footer must not be rendered; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn pre_surface_agents_toml_binds_readiness_to_pre_surface_after_flat_promotion() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // A promoted custom agent: its validated `flat` surface is the default, its
    // pre-surface rendering is `rich`, and an agent-level readiness override targets
    // the rich rendering. Built-in claude cannot play this role — its flat rendering
    // has no native patterns and stays unpromoted (see `builtin.rs`).
    let stub = config.write_executable(
        "binding-stub.sh",
        &format!(
            "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = \"--flat-surface\" ]; then\n\
                 exec '{fake}' --ready FLAT-SHIPPED-READY --busy FLAT-SHIPPED-BUSY\n\
               fi\n\
             done\n\
             exec '{fake}' --ready RICH-OVERRIDE-READY --busy RICH-OVERRIDE-BUSY\n",
            fake = FAKE_TUI_BIN
        ),
    );
    // Pre-surface shape for the override: agent-level readiness fields, which bind to
    // `rich`, the pre-surface rendering — not to the promoted `flat` default.
    config.write_agents_toml(&format!(
        r#"
[bindingagent]
binary = "{stub}"
default_surface = "flat"
pre_surface_rendering = "rich"
ready_regex = "^tt-fake-tui: ready: RICH-OVERRIDE-READY$"
ready_lines = 1

[bindingagent.surfaces.rich]
args = []

[bindingagent.surfaces.flat]
args = ["--flat-surface"]
ready_regex = "^tt-fake-tui: ready: FLAT-SHIPPED-READY$"
busy_regex = "^tt-fake-tui: busy: FLAT-SHIPPED-BUSY$"
validated_version = "bindingagent 1.0"

[bindingagent.access.default]
args = []
"#,
        stub = stub.display()
    ));

    let flat = format!("promoted-flat-{}", std::process::id());
    let rich = format!("pre-surface-rich-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&flat);
    guard.track(&rich);

    // No surface named: the promoted flat default is recorded, and its shipped pattern
    // — not the agent-level override — governs its readiness.
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "bindingagent",
            "--split",
            "v",
            "--bare",
            "--name",
            &flat,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent bindingagent (promoted flat default)");
    assert_eq!(pane_option(&flat, "@tt-surface"), "flat");
    assert!(
        !spawn.stderr.contains("warning:"),
        "a validated default must not warn; stderr: {:?}",
        spawn.stderr
    );
    let (reason, capture) = wait_idle_json(&flat, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "the promoted flat surface must keep its shipped pattern; capture: {capture:?}"
    );
    assert!(
        capture.contains("FLAT-SHIPPED-READY"),
        "the flat stub rendering must be on screen; capture: {capture:?}"
    );

    // The pre-surface rendering is still reachable and the agent-level override
    // governs it.
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "bindingagent",
            "--surface",
            "rich",
            "--split",
            "v",
            "--bare",
            "--name",
            &rich,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent bindingagent --surface rich");
    assert_eq!(pane_option(&rich, "@tt-surface"), "rich");
    let (reason, capture) = wait_idle_json(&rich, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "the pre-surface override must govern the pre-surface rendering; capture: {capture:?}"
    );
    assert!(
        capture.contains("RICH-OVERRIDE-READY"),
        "the rich stub rendering must be on screen; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn pre_surface_custom_agent_without_surfaces_loads_and_governs_readiness() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[precustom]
binary = "{fake}"
ready_regex = "^tt-fake-tui: ready on the normal screen$"
ready_lines = 1

[precustom.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("precustom-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "precustom",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent precustom");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "a synthesized surface must carry the agent-level readiness fields; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn agents_toml_unvalidated_default_surface_warns_and_records_pre_surface() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // Naming built-in claude's flat surface as the default fails P2's promotion bar —
    // `--ax-screen-reader` ships no validated ready/busy pair — so `spawn-agent` must
    // warn and record the pre-surface rendering (`rich`) instead.
    config.write_agents_toml(&format!(
        r#"
[claude]
binary = "{fake}"
default_surface = "flat"

[claude.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("builtin-default-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "claude",
            "--split",
            "v",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent claude with an overridden default");
    assert_eq!(
        pane_option(&name, "@tt-surface"),
        "rich",
        "an unvalidated declared default must fall back to the pre-surface rendering"
    );
    assert!(
        spawn.stderr.contains("warning:"),
        "the silent replacement must become a warning; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        spawn.stderr.contains("default_surface \"flat\"")
            && spawn.stderr.contains("recording \"rich\""),
        "the warning must name the declared default and the surface actually used; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        spawn.stderr.contains("ready_regex") && spawn.stderr.contains("busy_regex"),
        "the warning must name the missing validation; stderr: {:?}",
        spawn.stderr
    );
}

/// H1: built-in claude's flat surface ships no native ready/busy chrome, so it is not
/// promoted and claude resolves and records its pre-surface rendering (`rich`) with no
/// surface named and no warning.
#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_claude_no_surface_records_rich_pre_surface() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // Only the binary is overridden; the built-in surfaces and default are unchanged.
    config.write_agents_toml(&format!("[claude]\nbinary = \"{FAKE_TUI_BIN}\"\n"));

    let name = format!("builtin-claude-rich-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "claude",
            "--split",
            "v",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent claude with no surface named");
    assert_eq!(
        pane_option(&name, "@tt-surface"),
        "rich",
        "an unpromoted flat surface must not become claude's default"
    );
    assert!(
        !spawn.stderr.contains("warning:"),
        "the shipped default needs no warning; stderr: {:?}",
        spawn.stderr
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn wait_idle_falls_back_to_idle_detection_without_patterns() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[nopattern]
binary = "{fake}"

[nopattern.surfaces.default]
args = []

[nopattern.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let agent_pane = format!("nopattern-{}", std::process::id());
    let launch_pane = format!("nopattern-launch-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&agent_pane);
    guard.track(&launch_pane);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "nopattern",
            "--bare",
            "--name",
            &agent_pane,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent nopattern");
    wait_for_pane_ready(&agent_pane, Duration::from_secs(5));
    let (reason, _) = wait_idle_json(&agent_pane, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a surface that supplies no patterns falls back to idle detection"
    );

    // A `launch` pane has no agent and therefore no surface: idle detection as well.
    let quoted = format!("'{}'", FAKE_TUI_BIN.replace('\'', "'\\''"));
    let launched = run_bin(&["launch", "--cmd", &quoted, "--name", &launch_pane, "--bare"]);
    launched.assert_success("launch fixture");
    wait_for_pane_ready(&launch_pane, Duration::from_secs(5));
    let (reason, _) = wait_idle_json(&launch_pane, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a launch pane with no surface falls back to idle detection"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_single_surface_reads_resolved_readiness_fields() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // The built-in cursor surface is replaced with fixture-renderable patterns so the
    // resolved single surface's readiness fields are exercised end to end.
    config.write_agents_toml(&format!(
        r#"
[cursor]
binary = "{fake}"

[cursor.surfaces.default]
args = ["--ready", "CURSOR-SURFACE-READY", "--busy", "CURSOR-SURFACE-BUSY"]
ready_regex = "^tt-fake-tui: ready: CURSOR-SURFACE-READY$"

[cursor.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("builtin-cursor-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "cursor",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent cursor");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    let (reason, _) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "cursor's single surface must carry the resolved readiness fields"
    );
}

// ---------------------------------------------------------------------------
// P6 — resume is surface-owned and fails loudly.
//
// `spawn-agent --resume <ID>` maps the identifier through the resolved
// surface's declared syntax and validates it against that surface's declared
// identifier shape before any pane is created. The resume argument the surface
// supplies does not mark the pane surface-unvalidated.
// ---------------------------------------------------------------------------

/// The `agents.toml` block README documents for `dsh`, extracted verbatim so the
/// test exercises exactly the surfaces the documentation declares.
#[cfg(feature = "test-fixtures")]
fn readme_dsh_surfaces_toml() -> String {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README should be readable");
    let heading = readme
        .find("#### `dsh` surfaces")
        .expect("README should document the dsh surfaces");
    let after_heading = &readme[heading..];
    let fence_open = after_heading
        .find("```toml")
        .expect("the dsh section should open a toml fence")
        + "```toml".len();
    let body = &after_heading[fence_open..];
    let fence_close = body.find("```").expect("the dsh toml fence should close");
    body[..fence_close].to_owned()
}

/// The line the stub agent writes after every argument, so a reader can tell a
/// complete argv from a prefix of one still being appended.
#[cfg(feature = "test-fixtures")]
const ARGV_STUB_SENTINEL: &str = "__argv-stub-argv-complete__";

/// A stub agent that appends each received argument to `log`, one per line,
/// then a sentinel line marking the argv complete, then blocks so the pane
/// stays alive for registration assertions. It stands in for a real agent
/// binary, which the suite never launches.
#[cfg(feature = "test-fixtures")]
fn write_argv_stub(config: &AgentsConfig, log: &std::path::Path) -> std::path::PathBuf {
    config.write_executable(
        "argv-stub.sh",
        &format!(
            "#!/bin/sh\n\
             for arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{log}'; done\n\
             printf '%s\\n' '{sentinel}' >> '{log}'\n\
             exec sleep 300\n",
            log = log.display(),
            sentinel = ARGV_STUB_SENTINEL
        ),
    )
}

/// The complete arguments the stub agent reported, or `None` until it has
/// finished publishing them. A read that catches the log mid-append (the log
/// has no sentinel yet) is treated as not ready, so the returned arguments are
/// never a prefix of the real argv; the sentinel itself is excluded.
#[cfg(feature = "test-fixtures")]
fn argv_stub_args(log: &std::path::Path) -> Option<Vec<String>> {
    let contents = std::fs::read_to_string(log).ok()?;
    let mut args: Vec<String> = contents.lines().map(str::to_owned).collect();
    if args.last().map(String::as_str) != Some(ARGV_STUB_SENTINEL) {
        return None;
    }
    args.pop();
    Some(args)
}

/// Wait until the stub agent has reported its complete arguments.
#[cfg(feature = "test-fixtures")]
fn wait_for_argv_stub(log: &std::path::Path, timeout: Duration) -> Vec<String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(args) = argv_stub_args(log) {
            return args;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "stub agent never reported its arguments at {}",
        log.display()
    );
}

/// The position of `flag` in `args` and its immediate value.
#[cfg(feature = "test-fixtures")]
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.get(index + 1).map(String::as_str)
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_rejects_malformed_resume_identifier_without_a_pane() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let log = config.xdg().join("argv.log");
    let stub = write_argv_stub(&config, &log);
    config.write_agents_toml(&format!("[claude]\nbinary = \"{}\"\n", stub.display()));

    let name = format!("resume-bad-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "claude",
            "--resume",
            "not-a-session-id",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    assert_ne!(
        spawn.status, 0,
        "a malformed resume identifier must fail; stdout: {:?}",
        spawn.stdout
    );
    assert!(
        spawn.stderr.contains("not-a-session-id"),
        "the failure must name the offending identifier; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        !spawn.stderr.contains("unexpected argument"),
        "the --resume flag must be a real option; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        pane_id_by_name(&name).is_none(),
        "a malformed identifier must create no pane"
    );
    assert!(
        !log.exists(),
        "the agent binary must not be launched for a malformed identifier"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_maps_resume_through_surface_syntax_without_unvalidated_mark() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let log = config.xdg().join("argv.log");
    let stub = write_argv_stub(&config, &log);
    config.write_agents_toml(&format!("[claude]\nbinary = \"{}\"\n", stub.display()));

    let uuid = "0123abcd-4567-89ab-cdef-0123456789ab";
    let name = format!("resume-good-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "claude",
            "--surface",
            "rich",
            "--resume",
            uuid,
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent claude --resume <uuid>");

    let args = wait_for_argv_stub(&log, Duration::from_secs(5));
    assert_eq!(
        flag_value(&args, "--resume"),
        Some(uuid),
        "the launch command must carry the surface's resume syntax; argv: {args:?}"
    );
    assert_eq!(pane_option(&name, "@tt-surface"), "rich");
    assert_eq!(
        pane_option(&name, "@tt-surface-unvalidated"),
        "",
        "the surface-supplied resume argument must not mark the pane surface-unvalidated"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_maps_resume_through_a_subcommand_syntax() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let log = config.xdg().join("argv.log");
    let stub = write_argv_stub(&config, &log);
    config.write_agents_toml(&format!("[codex]\nbinary = \"{}\"\n", stub.display()));

    let uuid = "0123abcd-4567-89ab-cdef-0123456789ab";
    let name = format!("resume-codex-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "codex",
            "--resume",
            uuid,
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent codex --resume <uuid>");

    let args = wait_for_argv_stub(&log, Duration::from_secs(5));
    assert_eq!(
        flag_value(&args, "resume"),
        Some(uuid),
        "the launch command must carry codex's positional resume syntax; argv: {args:?}"
    );
    assert_eq!(
        pane_option(&name, "@tt-surface-unvalidated"),
        "",
        "the surface-supplied resume argument must not mark the pane surface-unvalidated"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_rejects_resume_when_the_resolved_surface_declares_none() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let log = config.xdg().join("argv.log");
    let stub = write_argv_stub(&config, &log);
    config.write_agents_toml(&format!(
        r#"
[noresume]
binary = "{stub}"

[noresume.surfaces.default]
args = []

[noresume.access.default]
args = []
"#,
        stub = stub.display()
    ));

    let name = format!("resume-none-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "noresume",
            "--resume",
            "0123abcd-4567-89ab-cdef-0123456789ab",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    assert_ne!(
        spawn.status, 0,
        "resuming a surface that declares no resume syntax must fail"
    );
    assert!(
        spawn.stderr.contains("does not support resume"),
        "the failure must say the surface has no resume syntax; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        !spawn.stderr.contains("unexpected argument"),
        "the --resume flag must be a real option; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        pane_id_by_name(&name).is_none(),
        "an unsupported resume must create no pane"
    );
    assert!(
        !log.exists(),
        "the agent binary must not be launched for an unsupported resume"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn readme_dsh_rich_surface_rejects_identifier_outside_its_settled_shape() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&readme_dsh_surfaces_toml());

    // A `dsh` stub earlier on PATH than any real binary: if validation were bypassed,
    // it would record that a live `dsh` had been launched.
    let dsh_log = config.xdg().join("dsh.log");
    config.write_executable(
        "dsh",
        &format!(
            "#!/bin/sh\nprintf 'launched\\n' >> '{log}'\nexec sleep 300\n",
            log = dsh_log.display()
        ),
    );

    let name = format!("resume-dsh-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // `dshline-<uuid>` is the flat surface's shape, not the rich surface's; the rich
    // surface is selected explicitly (the README promotes the flat surface to default).
    // It must fail before any pane exists, so no live `dsh` binary is needed.
    let flat_shaped = "dshline-0123abcd-4567-89ab-cdef-0123456789ab";
    let spawn = run_bin_env_path(
        &[
            "spawn-agent",
            "dsh",
            "--surface",
            "rich",
            "--resume",
            flat_shaped,
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
        config.xdg(),
    );
    assert_ne!(
        spawn.status, 0,
        "an identifier outside the rich surface's shape must fail"
    );
    assert!(
        spawn.stderr.contains(flat_shaped),
        "the failure must name the identifier outside the rich surface's shape; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        !spawn.stderr.contains("unexpected argument"),
        "the --resume flag must be a real option; stderr: {:?}",
        spawn.stderr
    );
    assert!(
        pane_id_by_name(&name).is_none(),
        "no pane may be created when the identifier does not match the shape"
    );
    assert!(
        !dsh_log.exists(),
        "a rejected identifier must not launch the dsh binary"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn readme_dsh_surfaces_accept_their_settled_resume_shapes() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    // A stub stands in for the real binary so the README-declared shapes can be
    // exercised end to end without a live `dsh`.
    for (surface, id) in [
        ("rich", "session-0123abcd-4567-89ab-cdef-0123456789ab"),
        ("rich", "0123abcd-4567-89ab-cdef-0123456789ab"),
        ("flat", "dshline-0123abcd-4567-89ab-cdef-0123456789ab"),
    ] {
        let config = AgentsConfig::new();
        let log = config.xdg().join("argv.log");
        let stub = write_argv_stub(&config, &log);
        let toml = readme_dsh_surfaces_toml().replace(
            "binary = \"dsh\"",
            &format!("binary = \"{}\"", stub.display()),
        );
        config.write_agents_toml(&toml);

        let name = format!("resume-dsh-ok-{surface}-{}", std::process::id());
        let mut guard = PaneGuard::new();
        guard.track(&name);

        let spawn = run_bin_env(
            &[
                "spawn-agent",
                "dsh",
                "--surface",
                surface,
                "--resume",
                id,
                "--bare",
                "--name",
                &name,
                "--format",
                "json",
            ],
            config.xdg(),
        );
        spawn.assert_success(&format!(
            "spawn-agent dsh --surface {surface} --resume {id}"
        ));

        let args = wait_for_argv_stub(&log, Duration::from_secs(5));
        assert_eq!(
            flag_value(&args, "--resume"),
            Some(id),
            "the README-documented {surface} surface must accept {id}; argv: {args:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// H1 — idle detection is the completion fallback whenever the classifier cannot
// decide (no patterns, no surface, or `unknown`), never overrides `busy`.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn pattern_surface_without_ready_footer_settles_by_idle_detection() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // The surface declares a pattern, but the fixture never renders a matching footer.
    // The classifier returns `unknown`, so idle detection must still complete the wait.
    config.write_agents_toml(&format!(
        r#"
[patternidle]
binary = "{fake}"

[patternidle.surfaces.default]
args = []
ready_regex = "^NEVER-MATCHES$"

[patternidle.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("pattern-idle-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "patternidle",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent patternidle");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a pattern-bearing surface whose ready footer never matches must fall back to idle \
         detection; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn busy_only_surface_settles_by_idle_detection_once_busy_footer_gone() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[busyonly]
binary = "{fake}"

[busyonly.surfaces.default]
args = ["--busy", "BUSY-ONLY"]
busy_regex = "tt-fake-tui: busy: BUSY-ONLY"

[busyonly.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("busy-only-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "busyonly",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent busyonly");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    // Show the busy footer, then clear it: the fixture redraws a single footer row, so a
    // state report removes the active busy footer.
    fake_tui_command(&name, "busy");
    wait_for_pane_text(&name, "busy: BUSY-ONLY", Duration::from_secs(5));
    fake_tui_command(&name, "normal");
    wait_for_pane_text_absent(&name, "busy: BUSY-ONLY", Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a busy-only surface with no busy footer on screen classifies unknown and must settle \
         by idle detection; capture: {capture:?}"
    );
    assert!(
        !capture.contains("BUSY-ONLY"),
        "the busy footer must be gone before idle completes; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn static_busy_footer_does_not_settle_as_idle() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[busystatic]
binary = "{fake}"

[busystatic.surfaces.default]
args = ["--busy", "STATIC-BUSY"]
busy_regex = "tt-fake-tui: busy: STATIC-BUSY"

[busystatic.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("busy-static-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "busystatic",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent busystatic");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    fake_tui_command(&name, "busy");
    wait_for_pane_text(&name, "busy: STATIC-BUSY", Duration::from_secs(5));

    // `wait_idle_json` pins `--timeout 10`, so build the shorter-timeout invocation here.
    let out = run_bin_env(
        &[
            "wait-idle",
            "--target",
            &name,
            "--format",
            "json",
            "--idle-seconds",
            "1",
            "--timeout",
            "2",
        ],
        config.xdg(),
    );
    out.assert_success("wait-idle static busy");
    let payload: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("wait-idle JSON should parse");
    let reason = payload
        .get("reason")
        .and_then(|v| v.as_str())
        .expect("wait-idle JSON should carry reason");
    assert_eq!(
        reason, "timed_out",
        "a static busy footer classifies busy and must suppress idle detection until timeout; \
         stdout: {}",
        out.stdout
    );
}

// ---------------------------------------------------------------------------
// M1 — a present record that names an unknown surface resolves to no surface.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn stale_recorded_surface_resolves_to_no_surface() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&surface_agent_toml());

    let name = format!("surface-stale-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Launch on the rich rendering, whose ready footer is on screen, then point the record at
    // a surface the registry does not have. The record must not be reinterpreted as the
    // pre-surface rendering (which would apply the rich patterns).
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "surfagent",
            "--surface",
            "rich",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent surfagent on rich surface");
    wait_for_pane_ready(&name, Duration::from_secs(5));
    let pane = pane_id_by_name(&name).expect("spawned pane should exist");
    let stale = run_tmux(&["set-option", "-p", "-t", &pane, "@tt-surface", "gone"]);
    stale.assert_success("point @tt-surface at a missing surface");

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a record naming an unknown surface must resolve to no surface and use idle detection, \
         never another rendering's patterns; capture: {capture:?}"
    );
    assert!(
        capture.contains("RICH-SURFACE-READY"),
        "the rich footer stays on screen but must be ignored; capture: {capture:?}"
    );
}

// ---------------------------------------------------------------------------
// M2 — `prompt` validates surface patterns before it sends anything.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_invalid_surface_regex_sends_nothing() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[badsurf]
binary = "{fake}"

[badsurf.surfaces.default]
args = ["--ready", "BAD-READY"]
busy_regex = "(unclosed"

[badsurf.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("prompt-badregex-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "badsurf",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent badsurf");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    // The surface's `busy_regex` is invalid. `prompt` must fail while resolving patterns,
    // before it sends the text, so a retry cannot submit the prompt twice.
    let text = "SHOULD-NOT-BE-SENT";
    let out = run_bin_env(&["prompt", "--target", &name, text], config.xdg());
    assert_ne!(
        out.status, 0,
        "prompt with an invalid surface regex must fail; stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    let capture = capture_pane_text(&name);
    assert!(
        !capture.contains(text),
        "prompt must not deliver text when surface patterns fail to compile; capture: {capture:?}"
    );
}

// ---------------------------------------------------------------------------
// M3 — `tt-fake-tui` redraws one footer row, so ready→busy→ready leaves no stale
// footer behind.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn fake_tui_redraws_single_footer_through_ready_busy_ready() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&format!(
        r#"
[bothfoot]
binary = "{fake}"

[bothfoot.surfaces.default]
args = ["--ready", "BOTH-READY", "--busy", "BOTH-BUSY"]
ready_regex = "^tt-fake-tui: ready: BOTH-READY$"
busy_regex = "tt-fake-tui: busy: BOTH-BUSY"

[bothfoot.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("both-footer-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "bothfoot",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent bothfoot");
    wait_for_pane_ready(&name, Duration::from_secs(5));
    wait_for_pane_text(&name, "ready: BOTH-READY", Duration::from_secs(5));

    fake_tui_command(&name, "busy");
    wait_for_pane_text(&name, "busy: BOTH-BUSY", Duration::from_secs(5));
    let busy_capture = capture_pane_text(&name);
    assert!(
        !busy_capture.contains("ready: BOTH-READY"),
        "the busy footer must replace the ready footer, not append below it; capture: \
         {busy_capture:?}"
    );

    fake_tui_command(&name, "ready");
    wait_for_pane_text(&name, "ready: BOTH-READY", Duration::from_secs(5));
    let ready_capture = capture_pane_text(&name);
    assert!(
        !ready_capture.contains("busy: BOTH-BUSY"),
        "the ready footer must replace the busy footer, not append below it; capture: \
         {ready_capture:?}"
    );

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "after ready→busy→ready the pane must classify idle on the ready footer; capture: \
         {capture:?}"
    );
}

// ---------------------------------------------------------------------------
// M4 — acceptance item 8 at the CLI for the built-in codex, cursor and agy
// surfaces, keeping each surface's shipped readiness fields.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_codex_settles_by_idle_detection() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // Only the launched binary is swapped; codex's built-in surface ships no readiness
    // patterns, so idle detection is the only completion signal.
    config.write_agents_toml(&format!(
        r#"
[codex]
binary = "{fake}"
"#,
        fake = FAKE_TUI_BIN
    ));

    let name = format!("builtin-codex-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "codex",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent codex");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "codex ships no readiness patterns and must settle by idle detection; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_cursor_shipped_ready_regex_matches_fixture_footer() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // Only the binary is swapped: cursor's shipped ready_regex keys off its input-prompt
    // placeholder, which this stub renders verbatim.
    let cursor_stub = config.write_executable(
        "cursor-stub.sh",
        "#!/bin/sh\nprintf '  → Plan, search, build anything\\n'\nexec cat\n",
    );
    config.write_agents_toml(&format!(
        r#"
[cursor]
binary = "{stub}"
"#,
        stub = cursor_stub.display()
    ));

    let name = format!("builtin-cursor-shipped-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "cursor",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent cursor");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "cursor's shipped ready_regex must match the fixture footer; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_agy_shipped_ready_regex_matches_fixture_footer() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // Only the binary is swapped: agy's shipped ready_regex keys off its bottom-line footer.
    let agy_stub = config.write_executable(
        "agy-stub.sh",
        "#!/bin/sh\nprintf '? for shortcuts\\n'\nexec cat\n",
    );
    config.write_agents_toml(&format!(
        r#"
[agy]
binary = "{stub}"
"#,
        stub = agy_stub.display()
    ));

    let name = format!("builtin-agy-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "agy",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent agy");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--ready-stable-seconds", "0"]);
    assert_eq!(
        reason, "ready_matched",
        "agy's shipped ready_regex must match the fixture footer; capture: {capture:?}"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn builtin_cursor_quiet_pane_falls_back_to_idle() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    // cursor's shipped ready_regex never matches this quiet pane, so the classifier returns
    // unknown and idle detection must complete the wait (acceptance item 8 fallback).
    let quiet_stub = config.write_executable(
        "cursor-quiet.sh",
        "#!/bin/sh\nprintf 'working on it...\\n'\nexec cat\n",
    );
    config.write_agents_toml(&format!(
        r#"
[cursor]
binary = "{stub}"
"#,
        stub = quiet_stub.display()
    ));

    let name = format!("builtin-cursor-quiet-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "cursor",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent cursor");
    wait_for_pane_ready(&name, Duration::from_secs(5));

    let (reason, capture) = wait_idle_json(&name, config.xdg(), &["--idle-seconds", "1"]);
    assert_eq!(
        reason, "idle",
        "a quiet pane whose shipped ready_regex does not match must fall back to idle detection; \
         capture: {capture:?}"
    );
}

// ---------------------------------------------------------------------------
// P1 — bracketed-paste transport: one prompt is one submission.
//
// `prompt` loads the text into a uniquely-named tmux buffer from stdin and
// pastes it with `paste-buffer -p -d`. The one-submission guarantee is keyed to
// the pane's live `bracket_paste_flag`, read immediately before pasting, and
// every outcome is reported on stderr (raw/concise) and as a JSON field.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_bracketed_paste_delivers_one_submission_and_reports_guaranteed() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("bracketed-report.bin");
    let name = format!("prompt-bracketed-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--report", report.to_str().unwrap()]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    let payload = large_multiline_prompt();
    assert!(
        payload.len() > 20_000,
        "the payload must exceed tmux's send-keys ceiling; got {} bytes",
        payload.len()
    );

    // JSON reports the structured outcome; the fixture's report is the oracle for
    // the text, never the pane's rendered bytes (tmux renders pastes itself).
    let json_out = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            &payload,
            "--format",
            "json",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    json_out.assert_success("prompt bracketed json");
    let parsed: serde_json::Value =
        serde_json::from_str(json_out.stdout.trim()).expect("prompt JSON should parse");
    assert_eq!(
        parsed["one_submission_delivery"].as_str(),
        Some("guaranteed"),
        "live bracketed paste must report a guaranteed one-submission delivery; payload: {}",
        json_out.stdout
    );

    let events = wait_for_report(&report, Duration::from_secs(5), |events| {
        events
            .iter()
            .filter(|event| event.kind == "submission")
            .count()
            == 1
    });
    let submissions: Vec<&ReportEvent> = events
        .iter()
        .filter(|event| event.kind == "submission")
        .collect();
    assert_eq!(
        submissions.len(),
        1,
        "one prompt must be exactly one submission; events: {events:?}"
    );
    assert_eq!(
        submissions[0].bytes.as_slice(),
        payload.as_bytes(),
        "the fixture's reported submission must match what was sent"
    );

    assert_no_prompt_buffer();

    // Raw and concise carry the guarantee on stderr, where they have no field.
    let concise = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            &payload,
            "--format",
            "concise",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    concise.assert_success("prompt bracketed concise");
    assert!(
        concise
            .stderr
            .contains("one-submission delivery guaranteed"),
        "concise stderr should report the guarantee; got: {:?}",
        concise.stderr
    );

    let raw = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            &payload,
            "--format",
            "raw",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    raw.assert_success("prompt bracketed raw");
    assert!(
        raw.stderr.contains("one-submission delivery guaranteed"),
        "raw stderr should report the guarantee; got: {:?}",
        raw.stderr
    );

    // Three runs, three one-submission deliveries.
    wait_for_report(&report, Duration::from_secs(5), |events| {
        events
            .iter()
            .filter(|event| event.kind == "submission")
            .count()
            == 3
    });
    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_transport_failure_after_load_buffer_leaves_no_prompt_buffer() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("transport-report.bin");
    let name = format!("prompt-transport-failure-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--report", report.to_str().unwrap()]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    // Interpose a fake `tmux` that fails `paste-buffer`; `load-buffer` and the
    // buffer cleanup still reach the real server.
    config.write_executable("tmux", &tmux_wrapper_failing("paste-buffer"));

    let text = "transport failure payload\nsecond line\n";
    let out = run_bin_env_path(
        &[
            "prompt",
            "--target",
            &name,
            text,
            "--format",
            "json",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
        config.xdg(),
    );

    assert_ne!(
        out.status, 0,
        "a transport failure must exit non-zero; stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr.contains("injected paste-buffer failure"),
        "the original transport error must be reported unchanged; stderr: {:?}",
        out.stderr
    );

    let events = read_report(&report);
    assert!(
        !events.iter().any(|event| event.kind == "submission"),
        "paste-buffer failed, so no submission should have reached the fixture; events: {events:?}"
    );
    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_refuses_multiline_when_live_flag_clear_but_surface_declares_capability() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("refusal-report.bin");
    config.write_agents_toml(&format!(
        r#"
[refuser]
binary = "{fake}"

[refuser.surfaces.default]
args = ["--no-bracketed-paste", "--report", "{report}"]
bracketed_paste = true

[refuser.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN,
        report = report.display()
    ));

    let name = format!("prompt-refusal-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "refuser",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent refuser");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    wait_for_pane_ready(&name, Duration::from_secs(5));
    wait_for_bracket_paste_flag(&name, false, Duration::from_secs(5));

    let text = "first line\nsecond line\nthird line";

    // JSON reports the refusal field; the command still exits non-zero.
    let json_out = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            text,
            "--format",
            "json",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    assert_ne!(
        json_out.status, 0,
        "the refusal must exit non-zero; stdout: {}\nstderr: {}",
        json_out.stdout, json_out.stderr
    );
    let refusal: serde_json::Value =
        serde_json::from_str(json_out.stdout.trim()).expect("refusal JSON should parse");
    assert_eq!(
        refusal["one_submission_delivery"].as_str(),
        Some("refused"),
        "the JSON field must report the refusal; payload: {}",
        json_out.stdout
    );
    assert!(
        json_out.stderr.to_lowercase().contains("refus"),
        "the refusal must be reported on stderr; got: {:?}",
        json_out.stderr
    );

    for format in ["concise", "raw"] {
        let out = run_bin_env(
            &[
                "prompt",
                "--target",
                &name,
                text,
                "--format",
                format,
                "--timeout",
                "15",
            ],
            config.xdg(),
        );
        assert_ne!(out.status, 0, "{format} refusal must exit non-zero");
        assert!(
            out.stderr.to_lowercase().contains("refus"),
            "{format} stderr must report the refusal; got: {:?}",
            out.stderr
        );
    }

    assert!(
        read_report(&report).is_empty(),
        "a refusal must not deliver anything to the fixture"
    );
    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_without_surface_and_no_bracketed_paste_delivers_and_reports_unavailable() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("unavailable-report.bin");
    let name = format!("prompt-unavailable-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(
        &name,
        &["--no-bracketed-paste", "--report", report.to_str().unwrap()],
    );
    wait_for_bracket_paste_flag(&name, false, Duration::from_secs(5));

    let text = "alpha line one\nbeta line two\ngamma line three";

    let json_out = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            text,
            "--format",
            "json",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    json_out.assert_success("prompt unavailable json");
    let parsed: serde_json::Value =
        serde_json::from_str(json_out.stdout.trim()).expect("prompt JSON should parse");
    assert_eq!(
        parsed["one_submission_delivery"].as_str(),
        Some("unavailable"),
        "a pane with no surface and no bracketed paste must report delivery unavailable; payload: {}",
        json_out.stdout
    );

    // Bracketed paste is off, so the payload arrives as ordinary lines (tmux
    // translates LF to CR). Each CR submits the line it terminates, so the
    // fixture still reports receiving every line.
    let events = wait_for_report(&report, Duration::from_secs(5), |events| {
        events
            .iter()
            .filter(|event| event.kind == "submission")
            .count()
            >= 3
    });
    let submissions: Vec<String> = events
        .iter()
        .filter(|event| event.kind == "submission")
        .map(|event| String::from_utf8_lossy(&event.bytes).into_owned())
        .collect();
    for line in ["alpha line one", "beta line two", "gamma line three"] {
        assert!(
            submissions.iter().any(|received| received == line),
            "the fixture should report receiving {line:?}; got: {submissions:?}"
        );
    }

    for format in ["concise", "raw"] {
        let out = run_bin_env(
            &[
                "prompt",
                "--target",
                &name,
                text,
                "--format",
                format,
                "--idle-seconds",
                "0.5",
                "--timeout",
                "15",
            ],
            config.xdg(),
        );
        out.assert_success(&format!("prompt unavailable {format}"));
        assert!(
            out.stderr.contains("one-submission delivery unavailable"),
            "{format} stderr must report delivery unavailable; got: {:?}",
            out.stderr
        );
    }

    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_empty_text_still_submits_with_enter_and_leaves_no_buffer() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("empty-report.bin");
    let name = format!("prompt-empty-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--report", report.to_str().unwrap()]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    // Empty text has no buffer to load; it must still submit with Enter and
    // report the live outcome instead of failing in paste-buffer.
    let out = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            "",
            "--format",
            "json",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "15",
        ],
        config.xdg(),
    );
    out.assert_success("prompt empty json");
    let parsed: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("prompt JSON should parse");
    assert_eq!(
        parsed["prompt_sent"].as_str(),
        Some(""),
        "the JSON must report the empty prompt; payload: {}",
        out.stdout
    );
    assert_eq!(
        parsed["one_submission_delivery"].as_str(),
        Some("guaranteed"),
        "the live bracketed paste flag must still be reported; payload: {}",
        out.stdout
    );

    // The fixture observes the Enter even though the composer was empty; nothing
    // is composed, so the report carries no submission.
    wait_for_pane_text(&name, "tt-fake-tui: enter", Duration::from_secs(5));
    assert!(
        read_report(&report).is_empty(),
        "an empty prompt must not create a submission record"
    );

    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_refuses_when_flag_clears_between_load_and_paste() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("flip-report.bin");
    config.write_agents_toml(&format!(
        r#"
[flipper]
binary = "{fake}"

[flipper.surfaces.default]
args = ["--report", "{report}"]
bracketed_paste = true

[flipper.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN,
        report = report.display()
    ));

    let name = format!("prompt-flip-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "flipper",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent flipper");
    assert_eq!(pane_option(&name, "@tt-surface"), "default");
    wait_for_pane_ready(&name, Duration::from_secs(5));
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    let pane = pane_id_by_name(&name).expect("fixture pane should exist");
    config.write_executable("tmux", &tmux_wrapper_flipping_bracket_paste(&pane));

    // The pre-load read sees the flag set, so the prompt loads. The interposed
    // wrapper clears the flag during `load-buffer`; the read immediately before
    // pasting must now refuse and discard the loaded buffer.
    let text = "first line\nsecond line\n";
    let out = run_bin_env_path(
        &[
            "prompt",
            "--target",
            &name,
            text,
            "--format",
            "json",
            "--timeout",
            "15",
        ],
        config.xdg(),
        config.xdg(),
    );

    assert_ne!(
        out.status, 0,
        "the post-load refusal must exit non-zero; stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    let refusal: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("refusal JSON should parse");
    assert_eq!(
        refusal["one_submission_delivery"].as_str(),
        Some("refused"),
        "the refusal decided before pasting must be reported; payload: {}",
        out.stdout
    );
    assert!(
        out.stderr.to_lowercase().contains("refus"),
        "the refusal must be reported on stderr; got: {:?}",
        out.stderr
    );

    assert!(
        read_report(&report).is_empty(),
        "a refusal before pasting must deliver nothing to the fixture"
    );
    assert_no_prompt_buffer();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_post_load_flag_read_failure_leaves_no_prompt_buffer() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("flag-read-report.bin");
    let name = format!("prompt-flag-read-failure-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--report", report.to_str().unwrap()]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    // Make only the `display-message` that follows `load-buffer` fail, so the
    // already-loaded buffer must be cleaned up on the error path.
    let state = config.xdg().join("flag-read-load-marker");
    config.write_executable("tmux", &tmux_wrapper_failing_post_load_flag_read(&state));

    let text = "post-load read failure payload\nsecond line\n";
    let out = run_bin_env_path(
        &[
            "prompt",
            "--target",
            &name,
            text,
            "--format",
            "json",
            "--timeout",
            "15",
        ],
        config.xdg(),
        config.xdg(),
    );

    assert_ne!(
        out.status, 0,
        "the failed post-load read must exit non-zero; stdout: {}\nstderr: {}",
        out.stdout, out.stderr
    );
    assert!(
        out.stderr.contains("injected post-load flag read failure"),
        "the original read error must be reported unchanged; stderr: {:?}",
        out.stderr
    );

    // Nothing was pasted, and the buffer loaded before the failed read is gone.
    let events = read_report(&report);
    assert!(
        !events.iter().any(|event| event.kind == "submission"),
        "no paste should have reached the fixture; events: {events:?}"
    );
    assert_no_prompt_buffer();
}

// ---------------------------------------------------------------------------
// P7 — `prompt` returns from a pre-submission history mark and reports turn
// observation and completeness independently. Driven at the CLI seam against
// `tt-fake-tui`; session retention is observed on a test-owned tmux server that
// never touches the user's default or current one.
// ---------------------------------------------------------------------------

/// The `prompt --format json` argument vector, with caller-supplied `--idle-seconds`
/// and `--timeout` honoured instead of the defaults.
#[cfg(feature = "test-fixtures")]
fn prompt_json_args<'a>(name: &'a str, text: &'a str, extra: &'a [&'a str]) -> Vec<&'a str> {
    let mut args = vec!["prompt", "--target", name, text, "--format", "json"];
    if !extra.contains(&"--idle-seconds") {
        args.extend_from_slice(&["--idle-seconds", "0.5"]);
    }
    if !extra.contains(&"--timeout") {
        args.extend_from_slice(&["--timeout", "10"]);
    }
    args.extend_from_slice(extra);
    args
}

/// Run `prompt` with the injected registry and return the parsed JSON payload.
#[cfg(feature = "test-fixtures")]
fn prompt_json(name: &str, text: &str, xdg: &std::path::Path, extra: &[&str]) -> serde_json::Value {
    let args = prompt_json_args(name, text, extra);
    let out = run_bin_env(&args, xdg);
    out.assert_success("prompt json");
    serde_json::from_str(out.stdout.trim()).expect("prompt JSON should parse")
}

/// Like [`prompt_json`], but with `dir` prepended to `PATH` so a fake `tmux` can
/// rewrite the P7 probe.
#[cfg(feature = "test-fixtures")]
fn prompt_json_path(
    name: &str,
    text: &str,
    xdg: &std::path::Path,
    extra: &[&str],
    dir: &std::path::Path,
) -> serde_json::Value {
    let args = prompt_json_args(name, text, extra);
    let out = run_bin_env_path(&args, xdg, dir);
    out.assert_success("prompt json");
    serde_json::from_str(out.stdout.trim()).expect("prompt JSON should parse")
}

/// Run `prompt` at a chosen format, returning the raw command output so a test can
/// inspect the stderr dimension notices.
#[cfg(feature = "test-fixtures")]
fn prompt_at_format(name: &str, text: &str, format: &str, xdg: &std::path::Path) -> CmdOutput {
    run_bin_env(
        &[
            "prompt",
            "--target",
            name,
            text,
            "--format",
            format,
            "--idle-seconds",
            "0.5",
            "--timeout",
            "10",
        ],
        xdg,
    )
}

/// Like [`prompt_at_format`], but with `dir` prepended to `PATH` so a fake `tmux`
/// can rewrite the P7 probe.
#[cfg(feature = "test-fixtures")]
fn prompt_at_format_path(
    name: &str,
    text: &str,
    format: &str,
    xdg: &std::path::Path,
    dir: &std::path::Path,
) -> CmdOutput {
    run_bin_env_path(
        &[
            "prompt",
            "--target",
            name,
            text,
            "--format",
            format,
            "--idle-seconds",
            "0.5",
            "--timeout",
            "10",
        ],
        xdg,
        dir,
    )
}

/// A registry with one `turn` agent whose surface carries distinguishable ready and
/// busy footers. `extra` appends fixture flags such as `--busy-on-submit`.
#[cfg(feature = "test-fixtures")]
fn observation_agents_toml(report: &std::path::Path, extra: &[&str]) -> String {
    let mut args = vec![
        "--ready".to_owned(),
        "TURN-READY".to_owned(),
        "--busy".to_owned(),
        "TURN-BUSY".to_owned(),
        "--report".to_owned(),
        report.display().to_string(),
    ];
    args.extend(extra.iter().map(|arg| (*arg).to_owned()));
    let args = args
        .iter()
        .map(|arg| format!("\"{arg}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
[turn]
binary = "{fake}"

[turn.surfaces.default]
args = [{args}]
ready_regex = "^tt-fake-tui: ready: TURN-READY$"
busy_regex = "tt-fake-tui: busy: TURN-BUSY"

[turn.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN,
    )
}

/// Spawn the `turn` observation agent bare and wait for its ready footer.
#[cfg(feature = "test-fixtures")]
fn spawn_turn_agent(
    name: &str,
    config: &AgentsConfig,
    report: &std::path::Path,
    busy_on_submit: bool,
) {
    let extra: &[&str] = if busy_on_submit {
        &["--busy-on-submit"]
    } else {
        &[]
    };
    spawn_turn_agent_with(name, config, report, extra);
}

/// Spawn the `turn` observation agent bare with extra fixture flags.
#[cfg(feature = "test-fixtures")]
fn spawn_turn_agent_with(
    name: &str,
    config: &AgentsConfig,
    report: &std::path::Path,
    extra: &[&str],
) {
    config.write_agents_toml(&observation_agents_toml(report, extra));
    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "turn",
            "--bare",
            "--name",
            name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent turn");
    wait_for_pane_text(name, "ready: TURN-READY", Duration::from_secs(5));
}

/// A registry with one `gate` agent whose readiness patterns the caller controls, so a
/// fixture capture can be made to match neither or both.
#[cfg(feature = "test-fixtures")]
fn gating_agents_toml(ready: &str, busy: &str) -> String {
    format!(
        r#"
[gate]
binary = "{fake}"

[gate.surfaces.default]
args = ["--ready", "GATE-FOOTER"]
ready_regex = "{ready}"
busy_regex = "{busy}"

[gate.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN,
    )
}

/// How a fake `tmux` answers the P7 probe's `#{history_collected}` field.
#[cfg(feature = "test-fixtures")]
enum ProbeCounter<'a> {
    /// Strip the field and force `history_size`/`history_limit` to a floor-crossing
    /// `950`/`1000`, so the CLI takes the 90% fallback and reports incomplete.
    Absent,
    /// Strip the field and force `history_size`/`history_limit` below the 90% floor, so
    /// the CLI takes the fallback and reports complete.
    AbsentBelowFloor,
    /// Report a supported counter that never moves.
    Constant(u64),
    /// Report a strictly increasing counter persisted in the state file.
    Increment(&'a std::path::Path),
}

/// A fake `tmux` that rewrites the P7 probe's `#{history_collected}` field while
/// preserving the real `cursor_y` and `alternate_on`, so both eviction paths are
/// exercised regardless of whether the host tmux provides the counter.
#[cfg(feature = "test-fixtures")]
fn tmux_wrapper_probe(counter: ProbeCounter<'_>) -> String {
    let real = real_tmux_path();
    let probe_body = match counter {
        ProbeCounter::Absent => "printf '%s\\n' \"950 1000 $3 $4\"\n".to_owned(),
        ProbeCounter::AbsentBelowFloor => "printf '%s\\n' \"100 1000 $3 $4\"\n".to_owned(),
        ProbeCounter::Constant(value) => format!("printf '%s\\n' \"$1 $2 $3 $4 {value}\"\n"),
        ProbeCounter::Increment(state) => format!(
            "n=0\n[ -f '{state}' ] && n=$(cat '{state}')\nn=$((n+1))\nprintf '%s' \"$n\" > \
             '{state}'\nprintf '%s\\n' \"$1 $2 $3 $4 $n\"\n",
            state = state.display(),
        ),
    };
    format!(
        "#!/bin/sh\n\
         REAL='{real}'\n\
         if [ \"$1\" = \"display-message\" ]; then\n\
         case \"$5\" in\n\
         *'#{{history_collected}}'*)\n\
         base=$(\"$REAL\" \"$1\" \"$2\" \"$3\" \"$4\" '#{{history_size}} #{{history_limit}} \
         #{{cursor_y}} #{{alternate_on}}')\n\
         set -- $base\n\
         {probe_body}\
         exit 0 ;;\n\
         esac\n\
         fi\n\
         exec \"$REAL\" \"$@\"\n",
        real = real,
        probe_body = probe_body,
    )
}

/// A test-owned, config-isolated tmux server. Every command runs with `TMUX` and
/// `TMUX_PANE` removed and `TMUX_TMPDIR` pointed at a test-owned temp dir; the server
/// starts with `-f /dev/null`, and teardown kills only that server via its explicit
/// `-L` label and removes the temp dir. The label (and therefore the endpoint) is fixed
/// before the server starts, and the temp dir exists before the first fallible step, so
/// a failed startup still tears both down. It never touches the default or current server.
#[cfg(feature = "test-fixtures")]
struct IsolatedTmuxServer {
    tmpdir: std::path::PathBuf,
    label: String,
    bin_dir: std::path::PathBuf,
}

#[cfg(feature = "test-fixtures")]
impl IsolatedTmuxServer {
    fn start() -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        // Use a short base so the derived unix socket path stays under the OS limit
        // (macOS's per-user temp dir plus `tmux-<uid>/<label>` can exceed it).
        let base = std::path::Path::new("/tmp");
        let base = if base.is_dir() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let tmpdir = base.join(format!("tt-iso-{:x}-{:x}", std::process::id(), suffix));
        let label = format!("iso{suffix:x}");
        let bin_dir = tmpdir.join("bin");

        // Build the guard *before* the first fallible operation: if creating the temp
        // dir or starting the server fails, `Drop` still kills the endpoint and removes
        // the temp dir.
        let server = Self {
            tmpdir,
            label,
            bin_dir,
        };
        server.install_wrapper();
        server.start_server();
        server
    }

    /// A `tmux` wrapper on the CLI's `PATH` that pins every call to this endpoint, so the
    /// CLI under test uses the same explicit `-L` label as setup and teardown.
    fn install_wrapper(&self) {
        self.write_wrapper(None);
    }

    /// Rewrite the wrapper to inject a caller `TMUX`/`TMUX_PANE` context into every tmux
    /// subprocess (the CLI itself keeps them removed). This reproduces the inside-tmux
    /// caller environment for the session-creation path without making the CLI take the
    /// inside-tmux branch.
    fn install_wrapper_with_context(&self, tmux: &str, tmux_pane: &str) {
        self.write_wrapper(Some((tmux, tmux_pane)));
    }

    fn write_wrapper(&self, context: Option<(&str, &str)>) {
        let wrapper = match context {
            Some((tmux, tmux_pane)) => format!(
                "#!/bin/sh\nTMUX='{tmux}' TMUX_PANE='{tmux_pane}' exec '{}' -L '{}' \"$@\"\n",
                real_tmux_path(),
                self.label
            ),
            None => format!(
                "#!/bin/sh\nexec '{}' -L '{}' \"$@\"\n",
                real_tmux_path(),
                self.label
            ),
        };
        self.install_wrapper_script(&wrapper);
    }

    /// Replace the wrapper with an arbitrary script, for a test that needs to inject a
    /// specific tmux failure while still pinning to this endpoint.
    fn install_wrapper_script(&self, script: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::create_dir_all(&self.bin_dir).unwrap();
        let path = self.bin_dir.join("tmux");
        std::fs::write(&path, script).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    fn start_server(&self) {
        let started = self.tmux(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "isolated-root",
            "sleep",
            "600",
        ]);
        started.assert_success("start isolated tmux server");
    }

    fn tmux(&self, args: &[&str]) -> CmdOutput {
        isolated_tmux(&self.tmpdir, &self.label, args)
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        self.run_cli(args, None)
    }

    /// Run the CLI against this server with an injected agent registry.
    fn cli_env(&self, args: &[&str], xdg: &std::path::Path) -> CmdOutput {
        self.run_cli(args, Some(xdg))
    }

    fn run_cli(&self, args: &[&str], xdg: Option<&std::path::Path>) -> CmdOutput {
        let path = format!(
            "{}:{}",
            self.bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(BIN);
        command
            .args(args)
            .env("PATH", path)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TMUX_TMPDIR", &self.tmpdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(xdg) = xdg {
            command.env("XDG_CONFIG_HOME", xdg);
        }
        let child = command
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));
        wait_for_child(child, &format!("{BIN} {args:?}"), Duration::from_secs(30))
    }
}

#[cfg(feature = "test-fixtures")]
impl Drop for IsolatedTmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.label, "kill-server"])
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TMUX_TMPDIR", &self.tmpdir)
            .output();
        let _ = std::fs::remove_dir_all(&self.tmpdir);
    }
}

/// Run one tmux command against a test-owned `TMUX_TMPDIR` and explicit label, with the
/// ambient tmux environment removed.
#[cfg(feature = "test-fixtures")]
fn isolated_tmux(tmpdir: &std::path::Path, label: &str, args: &[&str]) -> CmdOutput {
    let out = Command::new("tmux")
        .args(["-L", label])
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("TMUX_TMPDIR", tmpdir)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn tmux {args:?}: {e}"));
    CmdOutput {
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Resolve a registered pane id on an isolated server.
#[cfg(feature = "test-fixtures")]
fn isolated_pane_by_name(server: &IsolatedTmuxServer, name: &str) -> Option<String> {
    let out = server.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@tt-name}"]);
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

/// Poll an isolated server's pane until its history contains `needle`.
#[cfg(feature = "test-fixtures")]
fn wait_for_isolated_pane_text(
    server: &IsolatedTmuxServer,
    name: &str,
    needle: &str,
    timeout: Duration,
) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = isolated_pane_by_name(server, name) {
            let cap = server.tmux(&["capture-pane", "-p", "-S", "-", "-t", &pane]);
            if cap.status == 0 && cap.stdout.contains(needle) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("isolated pane {name} did not render {needle:?} within {timeout:?}");
}

/// Acceptance: a multi-line prompt wider than the pane still returns the fixture's
/// response and a recognizable echo of the prompt. The pre-submission history mark
/// makes the anchor independent of the prompt's rendering, which the removed
/// text-anchored extractor could not do.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_returns_the_fixture_response_and_echo_for_a_wide_multiline_prompt() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-wide-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "WIDE-READY", "--echo-submit"]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    let payload = large_multiline_prompt();
    let out = prompt_at_format(&name, &payload, "raw", config.xdg());
    out.assert_success("prompt wide multiline raw");

    assert!(
        out.stdout.contains("PROMPT-BEGIN"),
        "the result must contain a recognizable echo of the prompt; stdout: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("submission #1"),
        "the result must contain the fixture's response; stdout: {:?}",
        out.stdout
    );
}

/// Acceptance: distinctive normal-screen output above the submission point that the
/// turn does not redraw is absent from the result, even though it remains in history.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_excludes_unchanged_output_above_the_submission_point() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-preexisting-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "PRE-READY", "--echo-submit"]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    // Fill the pane with history above the submission point; line 1 is far above it.
    fake_tui_command(&name, "lines 60");
    wait_for_pane_text(&name, "tt-fake-tui: line 60", Duration::from_secs(5));

    let out = prompt_at_format(&name, "DISTINCT-PROMPT-ECHO", "raw", config.xdg());
    out.assert_success("prompt pre-existing raw");

    assert!(
        out.stdout.contains("DISTINCT-PROMPT-ECHO"),
        "the fingerprint echo must be present; stdout: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("submission #1"),
        "the fixture's response must be present; stdout: {:?}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("tt-fake-tui: line 1\n"),
        "output from before submission that the turn did not redraw must be absent; stdout: {:?}",
        out.stdout
    );
}

/// Acceptance: a normal-screen fixture whose composer row is redrawn in place has its
/// echo start above the submission point, and the result still contains it and the
/// turn's output.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_keeps_an_echo_redrawn_above_the_submission_point() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-redraw-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The ready footer is the composer row, directly above the cursor mark. Echoing
    // through it redraws the row that the pre-submission capture saw.
    launch_fake_tui_with(&name, &["--ready", "REDRAW-READY", "--echo-submit"]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));
    wait_for_pane_text(&name, "ready: REDRAW-READY", Duration::from_secs(5));

    let parsed = prompt_json(&name, "REDRAW-ECHO-CONTENT", config.xdg(), &[]);
    let output = parsed["output_since_prompt"].as_str().unwrap_or_default();
    assert!(
        output.contains("REDRAW-ECHO-CONTENT"),
        "the redrawn echo must be kept; output: {output:?}"
    );
    assert!(
        output.contains("submission #1"),
        "the turn's output must be kept; output: {output:?}"
    );
}

/// Acceptance: a fixture on the alternate screen still returns the recognizable echo
/// of the prompt.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_on_the_alternate_screen_still_contains_the_echo() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-alt-echo-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "ALT-ECHO-READY", "--echo-submit"]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));
    set_fake_tui_alternate(&name, true);

    let out = prompt_at_format(&name, "ALT-SCREEN-ECHO", "raw", config.xdg());
    out.assert_success("prompt alternate echo raw");
    assert!(
        out.stdout.contains("ALT-SCREEN-ECHO"),
        "the alternate screen must still return a recognizable echo; stdout: {:?}",
        out.stdout
    );
    assert!(
        out.stdout.contains("submission #1"),
        "the turn's output must be returned; stdout: {:?}",
        out.stdout
    );
}

/// Acceptance: a pane that emits the turn on the alternate screen and leaves it before
/// settling is still reported incomplete, because that output had no scrollback.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_incomplete_when_the_alternate_screen_is_left_before_settle() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-alt-left-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "ALT-LEFT-READY", "--alt-turn"]);

    // `--until` settles only after the fixture has left the alternate screen and printed
    // its normal-screen marker, so the alternate screen was observed only mid-wait.
    let parsed = prompt_json(
        &name,
        "alternate turn",
        config.xdg(),
        &[
            "--until",
            "alt-turn done",
            "--ready-stable-seconds",
            "0",
            "--idle-seconds",
            "5",
            "--timeout",
            "8",
        ],
    );
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(false),
        "a turn emitted on the alternate screen must be reported incomplete; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = run_bin_env(
            &[
                "prompt",
                "--target",
                &name,
                "alternate turn again",
                "--format",
                format,
                "--until",
                "alt-turn done",
                "--ready-stable-seconds",
                "0",
                "--idle-seconds",
                "5",
                "--timeout",
                "8",
            ],
            config.xdg(),
        );
        out.assert_success(&format!("prompt alternate-left {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: incomplete:"),
            "{format} stderr must report incompleteness; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr.contains("scrollback"),
            "{format} stderr must name the alternate-screen ceiling; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: complete"),
            "{format} stderr must not report complete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: on the alternate screen the result starts at the visible screen, excluding
/// the main-screen history `capture-pane -S -` still carries.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_excludes_pre_alternate_history_from_the_result() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-pre-alt-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "PRE-ALT-READY", "--echo-submit"]);
    wait_for_bracket_paste_flag(&name, true, Duration::from_secs(5));

    // Build main-screen history that the alternate screen keeps but must not return.
    fake_tui_command(&name, "lines 40");
    wait_for_pane_text(&name, "tt-fake-tui: line 40", Duration::from_secs(5));
    set_fake_tui_alternate(&name, true);

    let out = prompt_at_format(&name, "PRE-ALT-ECHO", "raw", config.xdg());
    out.assert_success("prompt pre-alternate raw");
    assert!(
        out.stdout.contains("PRE-ALT-ECHO"),
        "the alternate-screen echo must be returned; stdout: {:?}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("tt-fake-tui: line "),
        "main-screen history must not be returned on the alternate screen; stdout: {:?}",
        out.stdout
    );
}

/// Acceptance: a busy footer visible only during the Enter delivery is still reported as
/// the turn being observed.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_observes_a_busy_footer_seen_only_during_delivery() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("p7-busy-paste-report.bin");
    let name = format!("p7-busy-paste-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The busy footer is rendered as soon as the prompt is pasted and restored to ready
    // on submit, so only the Enter delivery's own frames see it.
    spawn_turn_agent_with(&name, &config, &report, &["--busy-on-paste"]);

    let parsed = prompt_json(&name, "briefly busy", config.xdg(), &["--timeout", "5"]);
    assert_eq!(
        parsed["turn_observed"].as_bool(),
        Some(true),
        "a busy footer seen during delivery must report the turn observed; payload: {parsed}"
    );
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "payload: {parsed}"
    );
}

/// Acceptance: output emitted before the busy footer is returned, and the turn is
/// reported observed. The busy footer persists, so the wait ends at the timeout.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_returns_output_before_the_busy_footer_and_reports_the_turn_observed() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("p7-observed-report.bin");
    let name = format!("p7-observed-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    spawn_turn_agent(&name, &config, &report, true);

    let out = run_bin_env(
        &[
            "prompt",
            "--target",
            &name,
            "observed payload",
            "--format",
            "json",
            "--idle-seconds",
            "0.5",
            "--timeout",
            "2",
        ],
        config.xdg(),
    );
    out.assert_success("prompt observed json");
    let parsed: serde_json::Value =
        serde_json::from_str(out.stdout.trim()).expect("prompt JSON should parse");
    assert_eq!(
        parsed["turn_observed"].as_bool(),
        Some(true),
        "the busy footer seen between submission and settle must report the turn observed; \
         payload: {}",
        out.stdout
    );
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "payload: {}",
        out.stdout
    );
    let output = parsed["output_since_prompt"].as_str().unwrap_or_default();
    assert!(
        output.contains("submission #1"),
        "output emitted before the busy footer must be returned; output: {output:?}"
    );

    // Raw and concise state the observed polarity; the opposite notice must be absent.
    for format in ["raw", "concise"] {
        let out = run_bin_env(
            &[
                "prompt",
                "--target",
                &name,
                "observed again",
                "--format",
                format,
                "--idle-seconds",
                "0.5",
                "--timeout",
                "2",
            ],
            config.xdg(),
        );
        out.assert_success(&format!("prompt observed {format}"));
        assert!(
            out.stderr.contains("tmux-tools: prompt: turn observed:"),
            "{format} stderr must state the observed polarity; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("turn unobserved:"),
            "{format} stderr must not state the opposite polarity; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: complete"),
            "{format} stderr must state the exact complete notice; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: incomplete"),
            "{format} stderr must not report incomplete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: with the busy footer suppressed the same content is returned and the
/// turn is reported unobserved.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_returns_the_same_output_and_reports_the_turn_unobserved_when_busy_is_suppressed() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("p7-unobserved-report.bin");
    let name = format!("p7-unobserved-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    spawn_turn_agent(&name, &config, &report, false);

    let parsed = prompt_json(
        &name,
        "unobserved payload",
        config.xdg(),
        &["--ready-stable-seconds", "0"],
    );
    assert_eq!(
        parsed["turn_observed"].as_bool(),
        Some(false),
        "a suppressed busy footer must report the turn unobserved; payload: {parsed}"
    );
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "payload: {parsed}"
    );
    let output = parsed["output_since_prompt"].as_str().unwrap_or_default();
    assert!(
        output.contains("submission #1"),
        "the same fixture response must be returned; output: {output:?}"
    );

    // Raw and concise state the unobserved polarity; the opposite notice must be absent.
    for format in ["raw", "concise"] {
        let out = run_bin_env(
            &[
                "prompt",
                "--target",
                &name,
                "unobserved again",
                "--format",
                format,
                "--idle-seconds",
                "0.5",
                "--ready-stable-seconds",
                "0",
                "--timeout",
                "10",
            ],
            config.xdg(),
        );
        out.assert_success(&format!("prompt unobserved {format}"));
        assert!(
            out.stderr.contains("tmux-tools: prompt: turn unobserved:"),
            "{format} stderr must state the unobserved polarity; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("turn observed:"),
            "{format} stderr must not state the opposite polarity; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: with both patterns present, a capture matching neither never settles as
/// idle — the wait must end at the timeout instead.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_does_not_treat_neither_match_as_idle_with_patterns_present() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&gating_agents_toml("^GATE-NEVER-READY$", "GATE-NEVER-BUSY"));
    let name = format!("p7-neither-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "gate",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent gate");
    wait_for_pane_text(&name, "ready: GATE-FOOTER", Duration::from_secs(5));

    let parsed = prompt_json(&name, "neither match", config.xdg(), &["--timeout", "2"]);
    assert_ne!(
        parsed["reason"].as_str(),
        Some("idle"),
        "a neither-matching capture must not settle as idle; payload: {parsed}"
    );
}

/// Acceptance: with both patterns present, a capture matching both never settles as
/// idle — the wait must end at the timeout instead.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_does_not_treat_both_match_as_idle_with_patterns_present() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_agents_toml(&gating_agents_toml("tt-fake-tui", "tt-fake-tui"));
    let name = format!("p7-both-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    let spawn = run_bin_env(
        &[
            "spawn-agent",
            "gate",
            "--bare",
            "--name",
            &name,
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawn.assert_success("spawn-agent gate");
    wait_for_pane_text(&name, "ready: GATE-FOOTER", Duration::from_secs(5));

    let parsed = prompt_json(&name, "both match", config.xdg(), &["--timeout", "2"]);
    assert_ne!(
        parsed["reason"].as_str(),
        Some("idle"),
        "a both-matching capture must not settle as idle; payload: {parsed}"
    );
}

/// Acceptance: a normal-screen fixture whose output fits in history is reported
/// complete, on stderr and in JSON.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_complete_when_output_fits_in_history() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-complete-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "FITS-READY"]);

    let parsed = prompt_json(&name, "output that fits", config.xdg(), &[]);
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format(&name, "output that fits again", format, config.xdg());
        out.assert_success(&format!("prompt complete {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: complete"),
            "{format} stderr must state the complete dimension; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: incomplete"),
            "{format} stderr must not call a fitting result incomplete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: an alternate-screen fixture is reported incomplete on stderr and in JSON.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_incomplete_on_the_alternate_screen() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-alt-incomplete-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "ALT-P7-READY"]);
    set_fake_tui_alternate(&name, true);

    let parsed = prompt_json(&name, "alternate prompt", config.xdg(), &[]);
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(false),
        "the alternate screen has no scrollback; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format(&name, "alternate prompt again", format, config.xdg());
        out.assert_success(&format!("prompt alternate {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: incomplete:"),
            "{format} stderr must report the alternate-screen ceiling; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr.contains("scrollback"),
            "{format} stderr must name the missing scrollback; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: complete"),
            "{format} stderr must not report complete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: when the running tmux provides `history_collected` and it increases
/// between mark and settle, the result is incomplete, on stderr and in JSON.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_incomplete_when_the_history_counter_increases() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let state = config.xdg().join("p7-counter-state");
    config.write_executable("tmux", &tmux_wrapper_probe(ProbeCounter::Increment(&state)));
    let name = format!("p7-counter-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "COUNTER-READY"]);

    let parsed = prompt_json_path(&name, "counter eviction", config.xdg(), &[], config.xdg());
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(false),
        "an increasing history counter must report eviction; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format_path(
            &name,
            "counter eviction again",
            format,
            config.xdg(),
            config.xdg(),
        );
        out.assert_success(&format!("prompt counter {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: incomplete:"),
            "{format} stderr must report eviction; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr.contains("evicted"),
            "{format} stderr must name eviction; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: complete"),
            "{format} stderr must not report complete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: a pane without the counter whose `history_size` is at or above the 90%
/// floor is reported incomplete, on stderr and in JSON.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_incomplete_at_the_ninety_percent_floor_without_the_counter() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_executable("tmux", &tmux_wrapper_probe(ProbeCounter::Absent));
    let name = format!("p7-floor-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "FLOOR-READY"]);

    let parsed = prompt_json_path(&name, "floor eviction", config.xdg(), &[], config.xdg());
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(false),
        "history at the 90% floor must report eviction; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format_path(
            &name,
            "floor eviction again",
            format,
            config.xdg(),
            config.xdg(),
        );
        out.assert_success(&format!("prompt floor {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: incomplete:"),
            "{format} stderr must report eviction; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr.contains("evicted"),
            "{format} stderr must name eviction; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: complete"),
            "{format} stderr must not report complete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: an alternate screen together with eviction is reported incomplete, with
/// both conditions named on stderr.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_incomplete_when_alternate_and_evicted_together() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_executable("tmux", &tmux_wrapper_probe(ProbeCounter::Absent));
    let name = format!("p7-both-incomplete-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "BOTH-P7-READY"]);
    set_fake_tui_alternate(&name, true);

    let parsed = prompt_json_path(
        &name,
        "alternate and evicted",
        config.xdg(),
        &[],
        config.xdg(),
    );
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(false),
        "both conditions at once must be reported incomplete; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format_path(
            &name,
            "alternate and evicted again",
            format,
            config.xdg(),
            config.xdg(),
        );
        out.assert_success(&format!("prompt alternate and evicted {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: incomplete:"),
            "{format} stderr must report incompleteness; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr.contains("scrollback") && out.stderr.contains("evicted"),
            "{format} stderr must name both conditions; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: complete"),
            "{format} stderr must not report complete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: a supported counter that never moves, on a pane below the floor, is
/// reported complete.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_complete_when_the_history_counter_is_unchanged() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_executable("tmux", &tmux_wrapper_probe(ProbeCounter::Constant(0)));
    let name = format!("p7-counter-clean-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "COUNTER-CLEAN-READY"]);

    let parsed = prompt_json_path(&name, "counter unchanged", config.xdg(), &[], config.xdg());
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "an unchanged counter below the floor must report complete; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format_path(
            &name,
            "counter unchanged again",
            format,
            config.xdg(),
            config.xdg(),
        );
        out.assert_success(&format!("prompt counter-clean {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: complete"),
            "{format} stderr must state the exact complete notice; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: incomplete"),
            "{format} stderr must not report incomplete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: a pane without the counter whose `history_size` is below the 90% floor is
/// reported complete, on stderr and in JSON, regardless of whether the host tmux provides
/// the counter.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_complete_below_the_floor_without_the_counter() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    config.write_executable("tmux", &tmux_wrapper_probe(ProbeCounter::AbsentBelowFloor));
    let name = format!("p7-below-floor-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--ready", "BELOW-FLOOR-READY"]);

    let parsed = prompt_json_path(&name, "below the floor", config.xdg(), &[], config.xdg());
    assert_eq!(
        parsed["complete"].as_bool(),
        Some(true),
        "history below the 90% floor must report complete; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format_path(
            &name,
            "below the floor again",
            format,
            config.xdg(),
            config.xdg(),
        );
        out.assert_success(&format!("prompt below-floor {format}"));
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: complete"),
            "{format} stderr must state the exact complete notice; got: {:?}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("completeness: incomplete"),
            "{format} stderr must not report incomplete; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: a fixture that prints nothing after the mark returns an empty result in
/// raw, concise and JSON, while both dimensions are still stated.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_reports_an_empty_result_with_both_dimensions_on_every_format() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let name = format!("p7-empty-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    launch_fake_tui_with(&name, &["--quiet-submit"]);

    let parsed = prompt_json(&name, "", config.xdg(), &[]);
    assert_eq!(
        parsed["output_since_prompt"].as_str(),
        Some(""),
        "a fixture that prints nothing must yield an empty JSON result; payload: {parsed}"
    );
    assert!(
        parsed
            .get("turn_observed")
            .is_some_and(|value| value.is_boolean()),
        "JSON must always carry the turn-observation field; payload: {parsed}"
    );
    assert!(
        parsed
            .get("complete")
            .is_some_and(|value| value.is_boolean()),
        "JSON must always carry the completeness field; payload: {parsed}"
    );

    for format in ["raw", "concise"] {
        let out = prompt_at_format(&name, "", format, config.xdg());
        out.assert_success(&format!("prompt empty {format}"));
        assert!(
            out.stdout.is_empty(),
            "{format} stdout must be empty for a silent turn; got: {:?}",
            out.stdout
        );
        assert!(
            out.stderr.contains("tmux-tools: prompt: turn unobserved:"),
            "{format} stderr must state the turn-observation dimension; got: {:?}",
            out.stderr
        );
        assert!(
            out.stderr
                .contains("tmux-tools: prompt: completeness: complete"),
            "{format} stderr must state the completeness dimension; got: {:?}",
            out.stderr
        );
    }
}

/// Acceptance: after real eviction, the result still starts at the mark (the echo), not at
/// the oldest retained pre-submission line. Runs on a test-owned tmux server.
#[cfg(feature = "test-fixtures")]
#[test]
fn prompt_excludes_pre_submission_output_after_real_eviction() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let name = format!("p7-evict-{}", std::process::id());
    let cmd = format!("{} --echo-submit", shell_quote(FAKE_TUI_BIN));
    let launched = server.cli(&[
        "launch", "--cmd", &cmd, "--name", &name, "--bare", "--format", "json",
    ]);
    launched.assert_success("launch eviction fixture");
    wait_for_isolated_pane_text(
        &server,
        &name,
        "tt-fake-tui: ready on the normal screen",
        Duration::from_secs(5),
    );

    // Shrink the session the fixture lives in so the turn overruns history.
    let shrink = server.tmux(&["set-option", "-t", "tmux-tools", "history-limit", "100"]);
    shrink.assert_success("shrink managed session history-limit");

    let sent = server.cli(&[
        "send",
        "--target",
        &name,
        "lines 200",
        "--literal",
        "--enter",
    ]);
    sent.assert_success("emit pre-submission history");
    wait_for_isolated_pane_text(
        &server,
        &name,
        "tt-fake-tui: line 200",
        Duration::from_secs(10),
    );

    let payload = (1..=80)
        .map(|index| format!("TURN-ECHO-{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let out = server.cli(&[
        "prompt",
        "--target",
        &name,
        &payload,
        "--format",
        "raw",
        "--idle-seconds",
        "0.5",
        "--timeout",
        "10",
    ]);
    out.assert_success("prompt after eviction");

    let pane = isolated_pane_by_name(&server, &name).expect("eviction pane should exist");
    let full = server.cli(&["capture", "--all", "--target", &pane, "--format", "raw"]);
    full.assert_success("capture retained history");
    assert!(
        full.stdout.contains("tt-fake-tui: line "),
        "the setup must retain pre-submission history; capture: {:?}",
        full.stdout
    );
    assert!(
        out.stdout.contains("TURN-ECHO-01"),
        "the echo must survive eviction; stdout: {:?}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("tt-fake-tui: line "),
        "retained pre-submission output must be excluded; stdout: {:?}",
        out.stdout
    );
}

/// Acceptance: `launch` creating a session raises its `history-limit` before the
/// command's window exists, so a command that overruns the server's global limit
/// keeps its first line. Runs on a test-owned, config-isolated tmux server.
#[cfg(feature = "test-fixtures")]
#[test]
fn launch_created_session_retains_more_history_than_the_global_limit() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let global = server.tmux(&["set-option", "-g", "history-limit", "1000"]);
    global.assert_success("set isolated global history-limit");

    let launched = server.cli(&[
        "launch",
        "--cmd",
        "seq 1 5000; sleep 600",
        "--name",
        "retained",
        "--bare",
        "--format",
        "json",
    ]);
    launched.assert_success("launch retained pane");
    let pane = serde_json::from_str::<serde_json::Value>(launched.stdout.trim())
        .expect("launch JSON should parse")
        .get("pane_id")
        .and_then(|value| value.as_str())
        .expect("launch JSON should carry pane_id")
        .to_owned();

    // The managed session the CLI created carries the raised limit.
    let limit = server.tmux(&["show-options", "-t", "tmux-tools", "history-limit"]);
    limit.assert_success("show managed session history-limit");
    assert!(
        limit.stdout.contains("50000"),
        "the created session must carry history-limit 50000; got: {:?}",
        limit.stdout
    );

    // Wait for the command to finish printing, then read the full history back.
    let started = Instant::now();
    loop {
        let cap = server.tmux(&["capture-pane", "-p", "-S", "-", "-t", &pane]);
        if cap.status == 0 && cap.stdout.contains("5000") {
            break;
        }
        if started.elapsed() > Duration::from_secs(15) {
            panic!(
                "seq output did not reach the pane; last capture: {:?}",
                cap.stdout
            );
        }
        thread::sleep(Duration::from_millis(100));
    }

    let captured = server.cli(&["capture", "--all", "--target", &pane, "--format", "raw"]);
    captured.assert_success("capture --all on the retained pane");
    assert!(
        captured.stdout.lines().any(|line| line.trim() == "1"),
        "the command's first line must survive in scrollback; stdout head: {:?}",
        captured.stdout.lines().take(5).collect::<Vec<_>>()
    );
}

/// Acceptance: even when every tmux subprocess carries a `TMUX`/`TMUX_PANE` caller
/// context, the created session gets `history-limit` 50000 and the caller session is
/// untouched. Runs on an isolated server whose `tmux` wrapper injects that caller context
/// (the CLI itself keeps `TMUX` removed, so it takes the session-creation path).
#[cfg(feature = "test-fixtures")]
#[test]
fn launch_created_session_ignores_a_caller_tmux_context() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let global = server.tmux(&["set-option", "-g", "history-limit", "1000"]);
    global.assert_success("set isolated global history-limit");

    // A session tmux-tools did not create, with a distinctive history limit.
    let caller = server.tmux(&["new-session", "-d", "-s", "caller", "sleep", "600"]);
    caller.assert_success("create caller session");
    let caller_limit = server.tmux(&["set-option", "-t", "caller", "history-limit", "777"]);
    caller_limit.assert_success("set caller history-limit");
    let caller_pane = server
        .tmux(&["display-message", "-p", "-t", "caller", "#{pane_id}"])
        .stdout
        .trim()
        .to_owned();

    // Build the caller context the tmux subprocesses will see. The wrapper injects it for
    // every subprocess; the CLI's own environment never carries it, so `launch` still
    // creates the managed session.
    let socket = server.tmux(&["display-message", "-p", "#{socket_path}"]);
    socket.assert_success("query socket path");
    let server_pid = server.tmux(&["display-message", "-p", "#{pid}"]);
    server_pid.assert_success("query server pid");
    let session_id = server.tmux(&["display-message", "-p", "-t", "caller", "#{session_id}"]);
    session_id.assert_success("query caller session id");
    let tmux_env = format!(
        "{},{},{}",
        socket.stdout.trim(),
        server_pid.stdout.trim(),
        session_id.stdout.trim().trim_start_matches('$')
    );
    server.install_wrapper_with_context(&tmux_env, &caller_pane);

    let launched = server.cli(&[
        "launch",
        "--cmd",
        "seq 1 5000; sleep 600",
        "--name",
        "retained",
        "--bare",
        "--format",
        "json",
    ]);
    launched.assert_success("launch with a caller context");
    let pane = serde_json::from_str::<serde_json::Value>(launched.stdout.trim())
        .expect("launch JSON should parse")
        .get("pane_id")
        .and_then(|value| value.as_str())
        .expect("launch JSON should carry pane_id")
        .to_owned();

    // The caller session (and its pane) keep their limit...
    let caller_after = server.tmux(&["show-options", "-t", "caller", "history-limit"]);
    caller_after.assert_success("show caller history-limit");
    assert!(
        caller_after.stdout.contains("777"),
        "the caller session must keep its history-limit; got: {:?}",
        caller_after.stdout
    );
    let caller_pane_limit = server.tmux(&[
        "display-message",
        "-p",
        "-t",
        &caller_pane,
        "#{history_limit}",
    ]);
    assert_eq!(
        caller_pane_limit.stdout.trim(),
        "777",
        "the caller pane must keep its effective history-limit"
    );

    // ...while the created session and the launched command's pane get 50000.
    let managed = server.tmux(&["show-options", "-t", "tmux-tools", "history-limit"]);
    managed.assert_success("show managed session history-limit");
    assert!(
        managed.stdout.contains("50000"),
        "the created session must carry history-limit 50000; got: {:?}",
        managed.stdout
    );
    let pane_limit = server.tmux(&["display-message", "-p", "-t", &pane, "#{history_limit}"]);
    pane_limit.assert_success("show launched pane history-limit");
    assert_eq!(
        pane_limit.stdout.trim(),
        "50000",
        "the launched command's pane must carry the raised effective limit"
    );

    // The first line survives the overrun of the server's global limit.
    let started = Instant::now();
    loop {
        let cap = server.tmux(&["capture-pane", "-p", "-S", "-", "-t", &pane]);
        if cap.status == 0 && cap.stdout.contains("5000") {
            break;
        }
        if started.elapsed() > Duration::from_secs(15) {
            panic!(
                "seq output did not reach the pane; last capture: {:?}",
                cap.stdout
            );
        }
        thread::sleep(Duration::from_millis(100));
    }

    let captured = server.cli(&["capture", "--all", "--target", &pane, "--format", "raw"]);
    captured.assert_success("capture --all on the retained pane");
    assert!(
        captured.stdout.lines().any(|line| line.trim() == "1"),
        "the command's first line must survive in scrollback; stdout head: {:?}",
        captured.stdout.lines().take(5).collect::<Vec<_>>()
    );
}

/// Acceptance: a stray `tmux-tools-*` session (as batch 2 could leak) is never mistaken
/// for the managed session, because the managed session is resolved by exact name.
/// `launch` creates the exact `tmux-tools` session and its pane gets `history-limit` 50000.
#[cfg(feature = "test-fixtures")]
#[test]
fn launch_creates_managed_session_despite_a_stray_pending_session() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let global = server.tmux(&["set-option", "-g", "history-limit", "1000"]);
    global.assert_success("set isolated global history-limit");

    // A leaked pending session whose name starts with the managed name: a bare
    // `-t tmux-tools` lookup would prefix-match it.
    let stray = server.tmux(&[
        "new-session",
        "-d",
        "-s",
        "tmux-tools-pending-999-999",
        "sleep",
        "600",
    ]);
    stray.assert_success("create stray pending session");

    let launched = server.cli(&[
        "launch",
        "--cmd",
        "seq 1 5000; sleep 600",
        "--name",
        "retained",
        "--bare",
        "--format",
        "json",
    ]);
    launched.assert_success("launch with a stray pending session present");
    let pane = serde_json::from_str::<serde_json::Value>(launched.stdout.trim())
        .expect("launch JSON should parse")
        .get("pane_id")
        .and_then(|value| value.as_str())
        .expect("launch JSON should carry pane_id")
        .to_owned();

    // The exact managed session exists, the stray is untouched, and the launched pane
    // lives in the managed session with the raised limit.
    let managed = server.tmux(&["has-session", "-t", "=tmux-tools"]);
    managed.assert_success("the exact managed session must exist");
    let stray_limit = server.tmux(&[
        "display-message",
        "-p",
        "-t",
        "tmux-tools-pending-999-999:",
        "#{history_limit}",
    ]);
    assert_eq!(
        stray_limit.stdout.trim(),
        "1000",
        "the stray session must keep the global limit"
    );
    let pane_session = server.tmux(&["display-message", "-p", "-t", &pane, "#{session_name}"]);
    assert_eq!(
        pane_session.stdout.trim(),
        "tmux-tools",
        "the launched pane must be in the created managed session"
    );
    let pane_limit = server.tmux(&["display-message", "-p", "-t", &pane, "#{history_limit}"]);
    assert_eq!(
        pane_limit.stdout.trim(),
        "50000",
        "the launched pane must carry the raised effective limit"
    );

    // The first line survives the overrun of the server's global limit.
    let started = Instant::now();
    loop {
        let cap = server.tmux(&["capture-pane", "-p", "-S", "-", "-t", &pane]);
        if cap.status == 0 && cap.stdout.contains("5000") {
            break;
        }
        if started.elapsed() > Duration::from_secs(15) {
            panic!(
                "seq output did not reach the pane; last capture: {:?}",
                cap.stdout
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
    let captured = server.cli(&["capture", "--all", "--target", &pane, "--format", "raw"]);
    captured.assert_success("capture --all on the retained pane");
    assert!(
        captured.stdout.lines().any(|line| line.trim() == "1"),
        "the command's first line must survive in scrollback; stdout head: {:?}",
        captured.stdout.lines().take(5).collect::<Vec<_>>()
    );
}

/// Acceptance: if a post-creation tmux call times out, its `Err` propagates through `?`
/// but the created private session is still removed rather than leaked.
#[cfg(feature = "test-fixtures")]
#[test]
fn launch_removes_its_pending_session_when_a_post_creation_step_times_out() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    // Hang the `set-option history-limit` invocation past tmux-tools' command timeout, so
    // `tmux::run` returns an `Err` through `?` after the private session was created.
    server.install_wrapper_script(&format!(
        "#!/bin/sh\nfor arg in \"$@\"; do\n\tif [ \"$arg\" = \"history-limit\" ];\n\t\tthen exec sleep 30\n\tfi\ndone\nexec '{}' -L '{}' \"$@\"\n",
        real_tmux_path(),
        server.label
    ));

    let launched = server.cli(&[
        "launch", "--cmd", "sleep 60", "--name", "leaky", "--bare", "--format", "raw",
    ]);
    assert_ne!(
        launched.status, 0,
        "launch must fail when a post-creation step times out"
    );

    let sessions = server.tmux(&["list-sessions", "-F", "#{session_name}"]);
    sessions.assert_success("list sessions after the failed launch");
    assert!(
        !sessions
            .stdout
            .lines()
            .any(|line| line.starts_with("tt-pending-")),
        "a failed launch must not leak its private session; sessions: {:?}",
        sessions.stdout
    );
    assert!(
        !sessions.stdout.lines().any(|line| line == "tmux-tools"),
        "a failed launch must not publish the managed session; sessions: {:?}",
        sessions.stdout
    );
}

/// Acceptance: `launch` placing a pane in a session tmux-tools did not create leaves
/// that session's `history-limit` unchanged. Runs on a test-owned tmux server.
#[cfg(feature = "test-fixtures")]
#[test]
fn launch_into_a_foreign_session_leaves_its_history_limit_unchanged() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let created = server.tmux(&["new-session", "-d", "-s", "foreign", "sleep", "600"]);
    created.assert_success("create foreign session");
    let set = server.tmux(&["set-option", "-t", "foreign", "history-limit", "1234"]);
    set.assert_success("set foreign session history-limit");

    let launched = server.cli(&[
        "launch",
        "--session",
        "foreign",
        "--cmd",
        "sleep 600",
        "--name",
        "foreign-pane",
        "--bare",
        "--format",
        "raw",
    ]);
    launched.assert_success("launch into the foreign session");

    let after = server.tmux(&["show-options", "-t", "foreign", "history-limit"]);
    after.assert_success("show foreign session history-limit");
    assert!(
        after.stdout.contains("1234"),
        "a session tmux-tools did not create must keep its history-limit; got: {:?}",
        after.stdout
    );
}

/// Acceptance: `spawn-agent` creating a session also raises the session's
/// `history-limit`. Runs on a test-owned tmux server.
#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_created_session_gets_the_raised_limit() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let config = AgentsConfig::new();
    let report = config.xdg().join("foreign-report.bin");
    config.write_agents_toml(&observation_agents_toml(&report, &[]));

    let spawned = server.cli_env(
        &[
            "spawn-agent",
            "turn",
            "--bare",
            "--name",
            "isolated-turn",
            "--format",
            "json",
        ],
        config.xdg(),
    );
    spawned.assert_success("spawn-agent turn on the isolated server");

    let limit = server.tmux(&["show-options", "-t", "tmux-tools", "history-limit"]);
    limit.assert_success("show managed session history-limit");
    assert!(
        limit.stdout.contains("50000"),
        "a spawn-agent-created session must carry history-limit 50000; got: {:?}",
        limit.stdout
    );
}

/// Acceptance: `spawn-agent` placing a pane in a session tmux-tools did not create
/// leaves that session's `history-limit` unchanged. Runs on a test-owned tmux server.
#[cfg(feature = "test-fixtures")]
#[test]
fn spawn_agent_into_a_foreign_session_leaves_its_history_limit_unchanged() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let server = IsolatedTmuxServer::start();
    let created = server.tmux(&["new-session", "-d", "-s", "foreign", "sleep", "600"]);
    created.assert_success("create foreign session");
    let set = server.tmux(&["set-option", "-t", "foreign", "history-limit", "4321"]);
    set.assert_success("set foreign session history-limit");

    let config = AgentsConfig::new();
    let report = config.xdg().join("foreign-spawn-report.bin");
    config.write_agents_toml(&observation_agents_toml(&report, &[]));

    let spawned = server.cli_env(
        &[
            "spawn-agent",
            "turn",
            "--session",
            "foreign",
            "--bare",
            "--name",
            "foreign-turn",
            "--format",
            "raw",
        ],
        config.xdg(),
    );
    spawned.assert_success("spawn-agent into the foreign session");

    let after = server.tmux(&["show-options", "-t", "foreign", "history-limit"]);
    after.assert_success("show foreign session history-limit");
    assert!(
        after.stdout.contains("4321"),
        "a session tmux-tools did not create must keep its history-limit; got: {:?}",
        after.stdout
    );
}

// ---------------------------------------------------------------------------
// P5 — `interrupt` sends the surface's declared key and refuses whenever that
// key could quit the agent. Driven at the CLI seam against `tt-fake-tui`, whose
// `--report` file is the oracle for which key (if any) reached the fixture.
// ---------------------------------------------------------------------------

#[cfg(feature = "test-fixtures")]
const KEY_C_C: u8 = 0x03;

#[cfg(feature = "test-fixtures")]
const KEY_C_U: u8 = 0x15;

/// The registry used by the interrupt tests. `hazard` declares `C-u` as its
/// interrupt key and that the key quits while idle; `calm` declares the same key
/// with no quit hazard. Each captures a `watched` surface whose patterns match the
/// fixture's footers and an `unknown` surface that supplies no patterns. The
/// `malformed*` agents carry a `busy_regex` that does not compile: one with no quit
/// hazard, one hazardous.
#[cfg(feature = "test-fixtures")]
fn interrupt_agents_toml(report: &std::path::Path) -> String {
    format!(
        r#"
[hazard]
binary = "{fake}"
default_surface = "watched"
pre_surface_rendering = "watched"

[hazard.surfaces.watched]
args = ["--ready", "HAZ-READY", "--busy", "HAZ-BUSY", "--report", "{report}"]
ready_regex = "^tt-fake-tui: ready: HAZ-READY$"
busy_regex = "tt-fake-tui: busy: HAZ-BUSY"
interrupt_key = "C-u"
quit_when_idle = true

[hazard.surfaces.unknown]
args = ["--ready", "HAZ-READY", "--busy", "HAZ-BUSY", "--report", "{report}"]
interrupt_key = "C-u"
quit_when_idle = true

[hazard.access.default]
args = []

[calm]
binary = "{fake}"
default_surface = "watched"
pre_surface_rendering = "watched"

[calm.surfaces.watched]
args = ["--ready", "CALM-READY", "--busy", "CALM-BUSY", "--report", "{report}"]
ready_regex = "^tt-fake-tui: ready: CALM-READY$"
busy_regex = "tt-fake-tui: busy: CALM-BUSY"
interrupt_key = "C-u"
quit_when_idle = false

[calm.surfaces.unknown]
args = ["--ready", "CALM-READY", "--busy", "CALM-BUSY", "--report", "{report}"]
interrupt_key = "C-u"
quit_when_idle = false

[calm.access.default]
args = []

[malformed]
binary = "{fake}"
default_surface = "only"
pre_surface_rendering = "only"

[malformed.surfaces.only]
args = ["--ready", "MAL-READY", "--busy", "MAL-BUSY", "--report", "{report}"]
ready_regex = "^tt-fake-tui: ready: MAL-READY$"
busy_regex = "(unclosed"
interrupt_key = "C-u"
quit_when_idle = false

[malformed.access.default]
args = []

[malformed_hazard]
binary = "{fake}"
default_surface = "only"
pre_surface_rendering = "only"

[malformed_hazard.surfaces.only]
args = ["--ready", "MALH-READY", "--busy", "MALH-BUSY", "--report", "{report}"]
ready_regex = "^tt-fake-tui: ready: MALH-READY$"
busy_regex = "(unclosed"
interrupt_key = "C-u"
quit_when_idle = true

[malformed_hazard.access.default]
args = []
"#,
        fake = FAKE_TUI_BIN,
        report = report.display()
    )
}

/// Spawn an interrupt-test pane and wait until its fixture is rendering.
#[cfg(feature = "test-fixtures")]
fn spawn_interrupt_pane(
    config: &AgentsConfig,
    agent: &str,
    surface: Option<&str>,
    name: &str,
    caller_args: &[&str],
) {
    let mut args = vec![
        "spawn-agent",
        agent,
        "--bare",
        "--name",
        name,
        "--format",
        "json",
    ];
    if let Some(surface) = surface {
        args.push("--surface");
        args.push(surface);
    }
    if !caller_args.is_empty() {
        args.push("--");
        args.extend_from_slice(caller_args);
    }
    let out = run_bin_env(&args, config.xdg());
    out.assert_success("spawn-agent for interrupt");
    wait_for_pane_ready(name, Duration::from_secs(5));
}

/// A control key that is never an interrupt target: `C-g` (0x07). Sending it through
/// the pane after an interrupt and waiting for it to appear in the report guarantees
/// the report is readable and that every key sent earlier has been written and
/// processed, so the observed key sequence is complete. A missing report never
/// satisfies this wait, so the test fails instead of reading as "no key".
#[cfg(feature = "test-fixtures")]
const KEY_SENTINEL: u8 = 0x07;

/// How many sentinels the fixture has reported so far.
#[cfg(feature = "test-fixtures")]
fn sentinel_count(events: &[ReportEvent]) -> usize {
    events
        .iter()
        .filter(|event| event.kind == "key" && event.bytes == [KEY_SENTINEL])
        .count()
}

/// Send the sentinel through the pane's input path and block until the fixture reports
/// this new sentinel, not one left by an earlier barrier.
#[cfg(feature = "test-fixtures")]
fn send_sentinel_and_wait(name: &str, report: &std::path::Path) {
    let before = sentinel_count(&read_report(report));
    let pane = pane_id_by_name(name).expect("pane should exist");
    let sent = run_tmux(&["send-keys", "-t", &pane, "C-g"]);
    sent.assert_success("send sentinel key");
    wait_for_report(report, Duration::from_secs(5), |events| {
        sentinel_count(events) > before
    });
}

/// The interrupt-test keys the fixture reported before the most recent sentinel, in
/// order. Sentinels themselves are excluded.
#[cfg(feature = "test-fixtures")]
fn keys_before_last_sentinel(report: &std::path::Path) -> Vec<u8> {
    let events = read_report(report);
    let last = events
        .iter()
        .rposition(|event| event.kind == "key" && event.bytes == [KEY_SENTINEL])
        .expect("the sentinel must be present in the report");
    events[..last]
        .iter()
        .filter(|event| event.kind == "key" && event.bytes != [KEY_SENTINEL])
        .flat_map(|event| event.bytes.iter().copied())
        .collect()
}

/// Assert `text` carries each required lowercase substring.
#[cfg(feature = "test-fixtures")]
fn assert_guidance(text: &str, required: &[&str], context: &str) {
    let lowered = text.to_lowercase();
    for needle in required {
        assert!(
            lowered.contains(needle),
            "{context}: expected guidance {needle:?}; got: {text:?}"
        );
    }
}

/// Run `interrupt` in concise and JSON form, asserting both are refused with the
/// expected guidance, and settle each with a sentinel.
#[cfg(feature = "test-fixtures")]
fn assert_interrupt_refused(
    config: &AgentsConfig,
    name: &str,
    report: &std::path::Path,
    guidance: &[&str],
) {
    let concise = run_bin_env(&["interrupt", "--target", name], config.xdg());
    assert_ne!(
        concise.status, 0,
        "interrupt must be refused; stdout: {}\nstderr: {}",
        concise.stdout, concise.stderr
    );
    assert_guidance(&concise.stderr, guidance, "concise refusal");
    send_sentinel_and_wait(name, report);

    let json = run_bin_env(
        &["interrupt", "--target", name, "--format", "json"],
        config.xdg(),
    );
    assert_ne!(
        json.status, 0,
        "the JSON refusal must exit non-zero; stdout: {}",
        json.stdout
    );
    let refusal: serde_json::Value =
        serde_json::from_str(json.stdout.trim()).expect("refusal JSON should parse");
    assert_guidance(
        refusal["error"].as_str().unwrap_or_default(),
        guidance,
        "JSON refusal error",
    );
    send_sentinel_and_wait(name, report);
}

/// Run `interrupt`, assert success and its concise key naming, then settle with a
/// sentinel and assert the fixture received exactly `expected`.
#[cfg(feature = "test-fixtures")]
fn assert_interrupt_sends(
    config: &AgentsConfig,
    name: &str,
    report: &std::path::Path,
    expected: &[u8],
    concise_fragment: &str,
) {
    let out = run_bin_env(&["interrupt", "--target", name], config.xdg());
    out.assert_success("interrupt must send");
    assert!(
        out.stdout.contains(concise_fragment),
        "the concise output must name the sent key; got: {:?}",
        out.stdout
    );
    send_sentinel_and_wait(name, report);
    assert_eq!(
        keys_before_last_sentinel(report),
        expected,
        "the fixture must receive exactly the declared key sequence"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_quit_hazard_surface_refuses_while_idle() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-hazard-idle-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-hazard-idle-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The fixture's ready footer is on screen and matches the surface.
    spawn_interrupt_pane(&config, "hazard", None, &name, &[]);
    wait_for_pane_text(&name, "ready: HAZ-READY", Duration::from_secs(5));

    assert_interrupt_refused(&config, &name, &report, &["refus", "idle", "wait", "retry"]);
    assert!(
        keys_before_last_sentinel(&report).is_empty(),
        "no key may reach the fixture on an idle quit-hazard refusal"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_quit_hazard_surface_refuses_while_unknown() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-hazard-unknown-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-hazard-unknown-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The surface supplies no patterns, so P3 cannot decide and the refusal must name
    // that cause and a workable action rather than "wait and retry".
    spawn_interrupt_pane(&config, "hazard", Some("unknown"), &name, &[]);
    wait_for_pane_text(&name, "ready: HAZ-READY", Duration::from_secs(5));

    assert_interrupt_refused(&config, &name, &report, &["refus", "busy", "configure"]);
    assert!(
        keys_before_last_sentinel(&report).is_empty(),
        "no key may reach the fixture on an unknown-state quit-hazard refusal"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_quit_hazard_surface_sends_declared_key_when_busy() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-busy-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-hazard-busy-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    spawn_interrupt_pane(&config, "hazard", None, &name, &[]);
    wait_for_pane_text(&name, "ready: HAZ-READY", Duration::from_secs(5));

    fake_tui_command(&name, "busy");
    wait_for_pane_text(&name, "busy: HAZ-BUSY", Duration::from_secs(5));

    assert_interrupt_sends(&config, &name, &report, &[KEY_C_U], "sent C-u");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_refuses_surface_unvalidated_pane_even_when_busy() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-unvalidated-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-unvalidated-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Caller-supplied trailing arguments mark the pane `@tt-surface-unvalidated`.
    spawn_interrupt_pane(&config, "hazard", None, &name, &["--caller-supplied"]);
    assert_eq!(
        pane_option(&name, "@tt-surface-unvalidated"),
        "1",
        "caller-supplied arguments must mark the pane surface-unvalidated"
    );
    wait_for_pane_text(&name, "ready: HAZ-READY", Duration::from_secs(5));

    // Even a positively busy rendering is refused: it is not the rendering whose
    // quit hazard was declared.
    fake_tui_command(&name, "busy");
    wait_for_pane_text(&name, "busy: HAZ-BUSY", Duration::from_secs(5));

    assert_interrupt_refused(
        &config,
        &name,
        &report,
        &["refus", "caller-supplied", "respawn"],
    );
    assert!(
        keys_before_last_sentinel(&report).is_empty(),
        "no key may reach a surface-unvalidated pane"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_no_hazard_surface_sends_declared_key_while_idle() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-calm-idle-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-calm-idle-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The ready footer matches, but a no-hazard surface sends regardless.
    spawn_interrupt_pane(&config, "calm", None, &name, &[]);
    wait_for_pane_text(&name, "ready: CALM-READY", Duration::from_secs(5));
    assert_interrupt_sends(&config, &name, &report, &[KEY_C_U], "sent C-u");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_no_hazard_surface_sends_declared_key_while_unknown() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-calm-unknown-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-calm-unknown-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Neither pattern matches, but there is no hazard to guard against.
    spawn_interrupt_pane(&config, "calm", Some("unknown"), &name, &[]);
    wait_for_pane_text(&name, "ready: CALM-READY", Duration::from_secs(5));
    assert_interrupt_sends(&config, &name, &report, &[KEY_C_U], "sent C-u");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_no_hazard_surface_sends_declared_key_on_unvalidated_pane() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-calm-unvalidated-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-calm-unvalidated-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // Surface-unvalidated: still no hazard, so the key is sent.
    spawn_interrupt_pane(&config, "calm", None, &name, &["--caller-supplied"]);
    assert_eq!(
        pane_option(&name, "@tt-surface-unvalidated"),
        "1",
        "caller-supplied arguments must mark the pane surface-unvalidated"
    );
    wait_for_pane_text(&name, "ready: CALM-READY", Duration::from_secs(5));
    assert_interrupt_sends(&config, &name, &report, &[KEY_C_U], "sent C-u");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_launched_pane_sends_c_c() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-launch-report.bin");
    let name = format!("interrupt-launch-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // A `launch`ed pane has no agent and therefore no surface: `interrupt` keeps
    // today's unconditional `C-c`.
    launch_fake_tui_with(&name, &["--report", report.to_str().unwrap()]);
    let out = run_bin(&["interrupt", "--target", &name]);
    out.assert_success("interrupt a launched pane");
    assert!(
        out.stdout.contains("sent C-c"),
        "a launched pane must be sent C-c; got: {:?}",
        out.stdout
    );
    send_sentinel_and_wait(&name, &report);
    assert_eq!(
        keys_before_last_sentinel(&report),
        vec![KEY_C_C],
        "a launched (no-surface) pane must receive C-c"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_no_hazard_surface_with_malformed_regex_still_sends() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-malformed-calm-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-malformed-calm-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // The surface declares no quit hazard, so its malformed `busy_regex` must not be
    // compiled and must not stop the declared key from being sent.
    spawn_interrupt_pane(&config, "malformed", None, &name, &[]);
    wait_for_pane_text(&name, "ready: MAL-READY", Duration::from_secs(5));
    assert_interrupt_sends(&config, &name, &report, &[KEY_C_U], "sent C-u");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_malformed_hazardous_unvalidated_pane_uses_unvalidated_refusal() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config
        .xdg()
        .join("interrupt-malformed-unvalidated-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-malformed-unvalidated-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // A hazardous surface-unvalidated pane is refused before its malformed pattern is
    // ever compiled, so the refusal names the unvalidated rendering, not the regex.
    spawn_interrupt_pane(
        &config,
        "malformed_hazard",
        None,
        &name,
        &["--caller-supplied"],
    );
    assert_eq!(
        pane_option(&name, "@tt-surface-unvalidated"),
        "1",
        "caller-supplied arguments must mark the pane surface-unvalidated"
    );
    wait_for_pane_text(&name, "ready: MALH-READY", Duration::from_secs(5));

    assert_interrupt_refused(
        &config,
        &name,
        &report,
        &["refus", "caller-supplied", "respawn"],
    );
    assert!(
        keys_before_last_sentinel(&report).is_empty(),
        "no key may reach a malformed surface-unvalidated pane"
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupt_on_validated_hazardous_surface_with_malformed_regex_refuses() {
    let _serial = serial_guard();

    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("SKIP: tmux not on PATH");
        return;
    }

    let config = AgentsConfig::new();
    let report = config.xdg().join("interrupt-malformed-hazard-report.bin");
    config.write_agents_toml(&interrupt_agents_toml(&report));

    let name = format!("interrupt-malformed-hazard-{}", std::process::id());
    let mut guard = PaneGuard::new();
    guard.track(&name);

    // A validated hazardous surface whose pattern cannot compile is refused: the pane
    // cannot be confirmed positively busy, so nothing is sent.
    spawn_interrupt_pane(&config, "malformed_hazard", None, &name, &[]);
    wait_for_pane_text(&name, "ready: MALH-READY", Duration::from_secs(5));

    assert_interrupt_refused(&config, &name, &report, &["refus", "malformed", "fix"]);
    assert!(
        keys_before_last_sentinel(&report).is_empty(),
        "a malformed hazardous surface must send no key"
    );
}
