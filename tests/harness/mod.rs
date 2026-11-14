//! The isolated tmux test harness shared by the integration test crates
//! (`tests/integration.rs`, `tests/claude_teams.rs`).
//!
//! Every test that talks to tmux holds its own [`IsolatedTmuxServer`]: a private
//! server on socket `-L tt-tests-<pid>-<n>`, started with `-f /dev/null` at a
//! fixed 280x82, whose `TMUX_TMPDIR` is `<tmp>/tt-tests-<pid>-<n>`. All `tmux`
//! and `tmux-tools` calls a test makes against a server go through it, so the
//! suite never reaches the developer's tmux server or loads their tmux
//! configuration. Dropping the value kills the server, which is the only
//! cleanup a test needs.
//!
//! The CLI runs in one of two modes (see [`CliMode`]): inside tmux, with `TMUX`
//! and `TMUX_PANE` naming the private server and a pane the harness owns, or
//! outside tmux, with both removed.

#![allow(dead_code)]
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const BIN: &str = env!("CARGO_BIN_EXE_tmux-tools");

/// The child-process environment of the suite (P10): applied when the executable
/// loads, before any test runs.
#[path = "../../core/src/test_child_env.rs"]
pub mod test_child_env;

pub use test_child_env::original_environment;

pub fn sanitize_child_environment() {
    test_child_env::sanitize();
}

/// Serializes the integration suite. Hold this guard for the lifetime of a
/// test to keep things deterministic under default `cargo test`.
pub fn serial_guard() -> MutexGuard<'static, ()> {
    sanitize_child_environment();
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[derive(Debug)]
pub struct CmdOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn assert_success(&self, ctx: &str) {
        assert_eq!(
            self.status, 0,
            "{ctx}: expected exit 0, got {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.status, self.stdout, self.stderr
        );
    }
}

/// Wait for a spawned child, then collect its captured stdout/stderr.
pub fn wait_for_child(mut child: std::process::Child, label: &str, timeout: Duration) -> CmdOutput {
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

/// Width and height of every harness server's first window.
pub const SERVER_WIDTH: &str = "280";
pub const SERVER_HEIGHT: &str = "82";

/// The session every harness server starts with. Its single pane is the pane the
/// harness owns: in inside-tmux mode the CLI runs with `TMUX_PANE` pointing at it.
pub const HARNESS_SESSION: &str = "isolated-root";

/// How the CLI under test sees tmux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliMode {
    /// `TMUX` is the private server's socket triple and `TMUX_PANE` is the harness
    /// pane, so the CLI takes its inside-tmux branch against the private server.
    InsideTmux,
    /// `TMUX` and `TMUX_PANE` are removed, so the CLI takes its outside-tmux branch.
    /// A `tmux` wrapper on the CLI's `PATH` pins every tmux call to the private
    /// server's `-L` label.
    OutsideTmux,
}

/// Per-invocation additions to the environment the harness gives the CLI.
#[derive(Default)]
pub struct CliOptions<'a> {
    /// `XDG_CONFIG_HOME`, to inject an agent registry.
    pub xdg: Option<&'a std::path::Path>,
    /// A directory prepended to `PATH`, to interpose a fake `tmux`.
    pub path_prefix: Option<&'a std::path::Path>,
    /// Wait limit for the CLI process (default 60s).
    pub timeout: Option<Duration>,
    /// Inside-tmux mode only: remove `TMUX_PANE` and point `TMUX` at this session
    /// of the private server, for a test whose subject is resolution without
    /// `TMUX_PANE`.
    pub caller_session_without_pane: Option<&'a str>,
    /// The CLI's working directory (default: the test process's own).
    pub cwd: Option<&'a std::path::Path>,
    /// Inside-tmux mode only: point `TMUX_PANE` at this pane of the private server
    /// and `TMUX` at its session, for a test whose calling pane is not the harness
    /// pane.
    pub caller_pane: Option<&'a str>,
    /// Extra environment variables for the CLI, set after the mode's own.
    pub env: &'a [(&'a str, &'a str)],
}

/// Per-process counter for `tt-tests-<pid>-<n>` server labels.
pub static NEXT_SERVER: AtomicUsize = AtomicUsize::new(0);

/// A test-owned, config-isolated tmux server. Every integration test that talks to
/// tmux holds exactly one, and every `tmux` or CLI call the test makes against a
/// server goes through it.
///
/// - The server runs on its own socket, `-L tt-tests-<pid>-<n>`, started with
///   `-f /dev/null` (no user configuration) and a first window of 280x82.
/// - Its `TMUX_TMPDIR` is `<tmp>/tt-tests-<pid>-<n>` (`<tmp>` is `/tmp`, or the OS
///   temp dir when `/tmp` is absent), and every tmux and CLI call runs with it.
/// - Direct `tmux` calls run with `TMUX` and `TMUX_PANE` removed and the explicit
///   label; CLI calls get the environment of the server's [`CliMode`].
/// - `Drop` kills the server and removes its `TMUX_TMPDIR` only once the server no
///   longer answers `has-session`. A `<tmp>/tt-tests-*` directory left after a run
///   therefore names a server that may have leaked (socket label and `TMUX_TMPDIR`).
///
/// The struct is built before the first fallible step, so a failed startup still
/// tears the server and directory down. It never touches the default or current
/// server.
pub struct IsolatedTmuxServer {
    pub label: String,
    pub tmpdir: std::path::PathBuf,
    pub bin_dir: std::path::PathBuf,
    pub mode: CliMode,
    pub socket_path: String,
    pub server_pid: String,
    pub session_id: String,
    pub pane: String,
}

#[cfg_attr(not(feature = "test-fixtures"), allow(dead_code))]
impl IsolatedTmuxServer {
    /// A private server whose CLI calls run inside tmux: `TMUX` names this server
    /// and `TMUX_PANE` the harness pane.
    pub fn start_inside() -> Self {
        Self::start(CliMode::InsideTmux)
    }

    /// A private server whose CLI calls run outside tmux: `TMUX` and `TMUX_PANE` are
    /// removed and a `PATH` wrapper pins tmux to this server.
    pub fn start_outside() -> Self {
        Self::start(CliMode::OutsideTmux)
    }

    pub fn start(mode: CliMode) -> Self {
        sanitize_child_environment();
        let n = NEXT_SERVER.fetch_add(1, Ordering::SeqCst);
        let label = format!("tt-tests-{}-{n}", std::process::id());
        // A short base keeps the derived unix socket path under the OS limit (macOS's
        // per-user temp dir plus `tmux-<uid>/<label>` can exceed it).
        let base = std::path::Path::new("/tmp");
        let base = if base.is_dir() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let tmpdir = base.join(&label);
        let bin_dir = tmpdir.join("bin");

        // Build the guard *before* the first fallible operation: if creating the temp
        // dir or starting the server fails, `Drop` still kills the endpoint and removes
        // the temp dir.
        let mut server = Self {
            label,
            tmpdir,
            bin_dir,
            mode,
            socket_path: String::new(),
            server_pid: String::new(),
            session_id: String::new(),
            pane: String::new(),
        };
        std::fs::create_dir_all(&server.tmpdir).unwrap();
        if mode == CliMode::OutsideTmux {
            server.install_wrapper();
        }
        server.start_server();
        server.read_context();
        server
    }

    pub fn start_server(&self) {
        let started = self.tmux(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            HARNESS_SESSION,
            "-x",
            SERVER_WIDTH,
            "-y",
            SERVER_HEIGHT,
            "sleep",
            "86400",
        ]);
        started.assert_success("start isolated tmux server");
    }

    pub fn read_context(&mut self) {
        let target = format!("={HARNESS_SESSION}:");
        let out = self.tmux(&[
            "display-message",
            "-p",
            "-t",
            &target,
            "#{socket_path}\t#{pid}\t#{session_id}\t#{pane_id}",
        ]);
        out.assert_success("read isolated server context");
        let fields: Vec<&str> = out.stdout.trim_end().split('\t').collect();
        assert_eq!(
            fields.len(),
            4,
            "unexpected server context: {:?}",
            out.stdout
        );
        self.socket_path = fields[0].to_owned();
        self.server_pid = fields[1].to_owned();
        self.session_id = fields[2].trim_start_matches('$').to_owned();
        self.pane = fields[3].to_owned();
    }

    /// The `-L` socket label, `tt-tests-<pid>-<n>`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The server's `TMUX_TMPDIR`, `<tmp>/tt-tests-<pid>-<n>`.
    pub fn tmpdir(&self) -> &std::path::Path {
        &self.tmpdir
    }

    /// The server process id.
    pub fn server_pid(&self) -> &str {
        &self.server_pid
    }

    /// The pane the harness owns; the CLI's `TMUX_PANE` in inside-tmux mode.
    pub fn pane(&self) -> &str {
        &self.pane
    }

    /// The `TMUX` value naming this server and the harness session.
    pub fn tmux_env(&self) -> String {
        format!(
            "{},{},{}",
            self.socket_path, self.server_pid, self.session_id
        )
    }

    /// The `TMUX` value naming this server and `session` (a session or pane target).
    pub fn tmux_env_for_session(&self, session: &str) -> String {
        let out = self.tmux(&["display-message", "-p", "-t", session, "#{session_id}"]);
        out.assert_success("query session id");
        format!(
            "{},{},{}",
            self.socket_path,
            self.server_pid,
            out.stdout.trim().trim_start_matches('$')
        )
    }

    /// A `tmux` wrapper on the CLI's `PATH` that pins every call to this endpoint, so the
    /// CLI under test uses the same explicit `-L` label as setup and teardown.
    pub fn install_wrapper(&self) {
        self.write_wrapper(None);
    }

    /// Rewrite the wrapper to inject a caller `TMUX`/`TMUX_PANE` context into every tmux
    /// subprocess (the CLI itself keeps them removed). This reproduces the inside-tmux
    /// caller environment for the session-creation path without making the CLI take the
    /// inside-tmux branch.
    pub fn install_wrapper_with_context(&self, tmux: &str, tmux_pane: &str) {
        self.write_wrapper(Some((tmux, tmux_pane)));
    }

    pub fn write_wrapper(&self, context: Option<(&str, &str)>) {
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
    /// specific tmux failure while still pinning to this endpoint. Outside-tmux mode
    /// only: inside-tmux mode puts no wrapper on the CLI's `PATH`.
    pub fn install_wrapper_script(&self, script: &str) {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            self.mode,
            CliMode::OutsideTmux,
            "the PATH tmux wrapper exists only in outside-tmux mode"
        );
        std::fs::create_dir_all(&self.bin_dir).unwrap();
        let path = self.bin_dir.join("tmux");
        std::fs::write(&path, script).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    /// A `tmux` command addressed to this server by label, with its `TMUX_TMPDIR` and
    /// the ambient `TMUX`/`TMUX_PANE` removed.
    pub fn tmux_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("tmux");
        command
            .args(["-L", &self.label])
            .args(args)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("TMUX_TMPDIR", &self.tmpdir);
        command
    }

    /// Run one tmux command against this server.
    pub fn tmux(&self, args: &[&str]) -> CmdOutput {
        let out = self
            .tmux_command(args)
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn tmux {args:?}: {e}"));
        CmdOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `program` with the environment the harness gives the CLI: this server's
    /// `TMUX_TMPDIR`, and `TMUX`/`TMUX_PANE` set (inside-tmux mode) or removed
    /// (outside-tmux mode, with the pinning wrapper first on `PATH`).
    pub fn command_with_cli_env(&self, program: &str, options: &CliOptions<'_>) -> Command {
        let mut path_dirs: Vec<String> = Vec::new();
        if let Some(prefix) = options.path_prefix {
            path_dirs.push(prefix.display().to_string());
        }
        let mut command = Command::new(program);
        command.env("TMUX_TMPDIR", &self.tmpdir);
        match self.mode {
            CliMode::InsideTmux => match (options.caller_session_without_pane, options.caller_pane)
            {
                (Some(_), Some(_)) => {
                    panic!("caller_session_without_pane and caller_pane are exclusive")
                }
                (Some(session), None) => {
                    command
                        .env("TMUX", self.tmux_env_for_session(session))
                        .env_remove("TMUX_PANE");
                }
                (None, Some(pane)) => {
                    command
                        .env("TMUX", self.tmux_env_for_session(pane))
                        .env("TMUX_PANE", pane);
                }
                (None, None) => {
                    command
                        .env("TMUX", self.tmux_env())
                        .env("TMUX_PANE", &self.pane);
                }
            },
            CliMode::OutsideTmux => {
                assert!(
                    options.caller_session_without_pane.is_none(),
                    "resolution without TMUX_PANE needs inside-tmux mode"
                );
                assert!(
                    options.caller_pane.is_none(),
                    "a calling pane needs inside-tmux mode"
                );
                path_dirs.push(self.bin_dir.display().to_string());
                command.env_remove("TMUX").env_remove("TMUX_PANE");
            }
        }
        path_dirs.push(std::env::var("PATH").unwrap_or_default());
        command.env("PATH", path_dirs.join(":"));
        if let Some(xdg) = options.xdg {
            command.env("XDG_CONFIG_HOME", xdg);
        }
        if let Some(cwd) = options.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in options.env {
            command.env(key, value);
        }
        command
    }

    /// Run `program` with the environment the harness gives the CLI.
    pub fn run_with_cli_env(&self, program: &str, args: &[&str]) -> CmdOutput {
        let out = self
            .command_with_cli_env(program, &CliOptions::default())
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("failed to spawn {program} {args:?}: {e}"));
        CmdOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    pub fn run_cli(&self, args: &[&str], options: CliOptions<'_>) -> CmdOutput {
        let child = self
            .command_with_cli_env(BIN, &options)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {BIN} {args:?}: {e}"));
        wait_for_child(
            child,
            &format!("{BIN} {args:?}"),
            options.timeout.unwrap_or(Duration::from_secs(60)),
        )
    }

    pub fn cli(&self, args: &[&str]) -> CmdOutput {
        self.run_cli(args, CliOptions::default())
    }

    pub fn cli_with_timeout(&self, args: &[&str], timeout: Duration) -> CmdOutput {
        self.run_cli(
            args,
            CliOptions {
                timeout: Some(timeout),
                ..CliOptions::default()
            },
        )
    }

    /// Run the CLI against this server with an injected agent registry.
    pub fn cli_env(&self, args: &[&str], xdg: &std::path::Path) -> CmdOutput {
        self.run_cli(
            args,
            CliOptions {
                xdg: Some(xdg),
                ..CliOptions::default()
            },
        )
    }

    /// Run the CLI with `dir` prepended to `PATH`, so a test can interpose a fake
    /// `tmux` on the command's tmux calls.
    pub fn cli_env_path(
        &self,
        args: &[&str],
        xdg: &std::path::Path,
        dir: &std::path::Path,
    ) -> CmdOutput {
        self.run_cli(
            args,
            CliOptions {
                xdg: Some(xdg),
                path_prefix: Some(dir),
                ..CliOptions::default()
            },
        )
    }

    /// Run the CLI without `TMUX_PANE`, with `TMUX` naming `session` of this server.
    pub fn cli_without_pane(&self, args: &[&str], session: &str, timeout: Duration) -> CmdOutput {
        self.run_cli(
            args,
            CliOptions {
                timeout: Some(timeout),
                caller_session_without_pane: Some(session),
                ..CliOptions::default()
            },
        )
    }

    /// Run the CLI with `pane` as the calling pane: `TMUX_PANE` is `pane` and `TMUX`
    /// names its session. `xdg`, when given, injects an agent registry.
    pub fn cli_from_pane(
        &self,
        args: &[&str],
        pane: &str,
        xdg: Option<&std::path::Path>,
    ) -> CmdOutput {
        self.run_cli(
            args,
            CliOptions {
                xdg,
                caller_pane: Some(pane),
                ..CliOptions::default()
            },
        )
    }

    /// Whether the server still answers `has-session`.
    pub fn answers(&self) -> bool {
        self.tmux_command(&["has-session"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

/// Whether a process with this pid exists.
pub fn process_exists(pid: &str) -> bool {
    !pid.is_empty()
        && Command::new("kill")
            .args(["-0", pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
}

impl Drop for IsolatedTmuxServer {
    fn drop(&mut self) {
        let _ = self
            .tmux_command(&["kill-server"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        // `kill-server` needs the socket. A server whose socket is gone, or whose pid
        // was never read (a failed startup), still runs `-L <label>`: end it by its
        // pid and by its label so no test server outlives the suite.
        let deadline = Instant::now() + Duration::from_secs(2);
        while (self.answers() || process_exists(&self.server_pid)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if process_exists(&self.server_pid) {
            let _ = Command::new("kill")
                .args(["-KILL", &self.server_pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = Command::new("pkill")
            .args(["-KILL", "-f", &format!("tmux -L {} ", self.label)])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        // Remove the directory only once the server is gone, so a server that
        // survives teardown keeps its `<tmp>/tt-tests-*` directory as a leak marker.
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.answers() || process_exists(&self.server_pid) {
            if Instant::now() > deadline {
                eprintln!(
                    "WARNING: tmux test server -L {} (pid {}) survived teardown; leaving \
                     TMUX_TMPDIR {} in place",
                    self.label,
                    self.server_pid,
                    self.tmpdir.display()
                );
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&self.tmpdir);
    }
}

/// The absolute path of the real tmux, so an interposed fake `tmux` can delegate
/// every subcommand except the one under test.
pub fn real_tmux_path() -> String {
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

/// Resolve a pane id by registered `@tt-name` using tmux directly. Returns
/// `None` if no such pane exists.
pub fn pane_id_by_name(server: &IsolatedTmuxServer, name: &str) -> Option<String> {
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

/// Wait until a child shell prompt is settled enough to accept commands. We
/// simply poll for the named pane to exist *and* render at least one line of
/// output (typically the bash prompt).
pub fn wait_for_pane_ready(server: &IsolatedTmuxServer, name: &str, timeout: Duration) {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Some(pane) = pane_id_by_name(server, name) {
            let cap = server.tmux(&["capture-pane", "-t", &pane, "-p"]);
            if cap.status == 0 && !cap.stdout.trim().is_empty() {
                return;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("pane {name} did not become ready within {timeout:?}");
}

// ---------------------------------------------------------------------------
// The private-server harness itself.
// ---------------------------------------------------------------------------

/// `<tmp>` for harness servers: `/tmp`, or the OS temp dir when `/tmp` is absent.
pub fn harness_tmp_base() -> std::path::PathBuf {
    let tmp = std::path::Path::new("/tmp");
    if tmp.is_dir() {
        tmp.to_path_buf()
    } else {
        std::env::temp_dir()
    }
}
