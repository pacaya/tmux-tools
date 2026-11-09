//! The contract suite's live Orca run (PRD-261107-0825-01 P10.2–P10.4).
//!
//! Opt-in: it runs only with `TMUX_TOOLS_LIVE_ORCA=1`, from inside an Orca terminal
//! (which supplies `ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID`) with a reachable
//! runtime. The default `cargo test` run returns from it at once.
//!
//! One `#[test]` drives every scenario in order, so the run can end by asserting that
//! neither `tmux-tools list` nor `orca terminal list` shows a pane it created. Every
//! pane the run creates is recorded in a [`Run`], whose `Drop` closes what is left,
//! also on failure. Behaviour a live Orca does not produce on demand (an ambiguous
//! split, a truncated pane list, a missing capability, `worktree ps` rows, send
//! receipts) comes from `orca_shim.py`, a shim around the real `orca` placed first on
//! the CLI's `PATH` only by the scenarios that need it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::{original_environment, wait_for_child, CmdOutput, BIN, FAKE_TUI_BIN};

/// The opt-in switch of the live Orca run.
const LIVE_SWITCH: &str = "TMUX_TOOLS_LIVE_ORCA";

/// Optional: run only the live scenarios whose label contains this text.
const ONLY_SWITCH: &str = "TMUX_TOOLS_LIVE_ORCA_ONLY";

/// Optional: a directory where the Claude-teams scenario saves its raw observations.
const EVIDENCE_SWITCH: &str = "TMUX_TOOLS_LIVE_ORCA_EVIDENCE";

const SHIM: &str = include_str!("orca_shim.py");

const HEXECHO: &str = r#"
import os, sys, termios, tty
fd = sys.stdin.fileno()
tty.setraw(fd)
os.write(1, b"HEXECHO READY\r\n")
while True:
    data = os.read(fd, 1024)
    if not data:
        break
    os.write(1, ("HEX " + data.hex() + "\r\n").encode())
"#;

fn live() -> bool {
    original_environment()
        .get(LIVE_SWITCH)
        .is_some_and(|value| value == "1")
}

fn original(name: &str) -> Option<String> {
    original_environment().get(name).cloned()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// One live run: its temp directory, registry, agent registry, shim, and every pane
/// it created.
///
/// A pane counts as created by the run only on run-specific evidence (P10.4): its
/// handle is in a Pane record of the run's private registry, the shim saw the real
/// `orca` return it to one of the run's calls, its `paneTitle` is one of the
/// run-unique names from [`Run::n`], or it appeared, after one of the run's `--split
/// h|v` calls, in the tab of that call's target, which is always a run-owned pane (a
/// split pane gets no title). A pane without such evidence is never closed.
struct Run {
    dir: PathBuf,
    real_orca: PathBuf,
    shim_dir: PathBuf,
    /// Run-unique suffix of every pane name the run uses.
    tag: String,
    /// Every name [`Run::n`] gave out.
    names: std::cell::RefCell<HashSet<String>>,
    created: std::cell::RefCell<Vec<String>>,
    /// A creating call ended without a known outcome, so a pane may still open.
    uncertain: std::cell::Cell<bool>,
    /// Per `--split h|v` call of the run: the `tabId` of its run-owned target and the
    /// handles listed in that tab before the call. A pane that appears there later is
    /// the run's.
    split_tabs: std::cell::RefCell<Vec<(String, HashSet<String>)>>,
    /// How long the final reconciliation waits for late arrivals when `uncertain`.
    late_wait: Duration,
    /// Environment set on every CLI and direct `orca` call (the fake-CLI runs).
    extra_env: Vec<(String, String)>,
}

/// Per-call options of a CLI invocation.
#[derive(Default)]
struct Call<'a> {
    /// Put the shim first on `PATH`.
    shim: bool,
    /// Extra environment, set after the run's own.
    env: Vec<(&'a str, String)>,
    /// Variables removed from the run's environment.
    remove: Vec<&'a str>,
    timeout: Option<Duration>,
}

impl Run {
    fn start() -> Run {
        let path = original("PATH").unwrap_or_default();
        let real_orca = path
            .split(':')
            .map(|entry| Path::new(entry).join("orca"))
            .find(|candidate| candidate.exists())
            .expect("the live Orca run needs `orca` on PATH");
        Run::with_orca(real_orca, Vec::new(), Duration::from_secs(30))
    }

    fn with_orca(real_orca: PathBuf, extra_env: Vec<(String, String)>, late_wait: Duration) -> Run {
        let (dir, identity) = unique_dir("tt-orca-live");
        std::fs::create_dir_all(dir.join("xdg-config/tmux-tools")).unwrap();
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let shim_dir = dir.join("shim");
        std::fs::create_dir_all(&shim_dir).unwrap();
        write_executable(&shim_dir.join("orca"), SHIM);
        std::fs::write(dir.join("hexecho.py"), HEXECHO).unwrap();
        let tag = identity;
        let run = Run {
            dir,
            real_orca,
            shim_dir,
            tag,
            names: std::cell::RefCell::new(HashSet::new()),
            created: std::cell::RefCell::new(Vec::new()),
            uncertain: std::cell::Cell::new(false),
            split_tabs: std::cell::RefCell::new(Vec::new()),
            late_wait,
            extra_env,
        };
        run.write_agents();
        run
    }

    /// The run-unique pane name for `base`. Orca shows it as the pane's `paneTitle`,
    /// which marks a pane as the run's own even when its creation outcome is unknown.
    fn n(&self, base: &str) -> String {
        let name = format!("{base}-{}", self.tag);
        self.names.borrow_mut().insert(name.clone());
        name
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn state_dir(&self) -> PathBuf {
        self.dir.join("state")
    }

    fn write_agents(&self) {
        let self_script = self.file("self-agent.sh");
        write_executable(
            &self_script,
            &format!(
                "#!/bin/sh\nXDG_STATE_HOME={state} {BIN} self --format json > {out}\nexec sleep 600\n",
                state = self.state_dir().display(),
                out = self.file("self-out.json").display()
            ),
        );
        let toml = format!(
            r#"
[ttfake]
binary = "{fake}"
[ttfake.surfaces.default]
args = ["--no-bracketed-paste", "--report", "{report}"]
bracketed_paste = true
interrupt_key = "C-u"
quit_when_idle = false
[ttfake.access.default]
args = []

[ttalt]
binary = "{fake}"
[ttalt.surfaces.default]
args = ["--alt-turn", "--report", "{alt_report}"]
[ttalt.access.default]
args = []

[tthex]
binary = "python3"
[tthex.surfaces.default]
args = ["-I", "{hexecho}"]
[tthex.access.default]
args = []

[ttself]
binary = "{self_script}"
[ttself.access.default]
args = []
"#,
            fake = FAKE_TUI_BIN,
            report = self.file("fake-report.bin").display(),
            alt_report = self.file("alt-report.bin").display(),
            hexecho = self.file("hexecho.py").display(),
            self_script = self_script.display(),
        );
        // The Claude-teams scenario starts the `claude` of the caller's `PATH` by its
        // absolute path, so the Orca pane's own `PATH` does not decide which one runs.
        let claude = original("PATH")
            .unwrap_or_default()
            .split(':')
            .map(|entry| Path::new(entry).join("claude"))
            .find(|candidate| candidate.is_file());
        let toml = match claude {
            Some(claude) => format!("{toml}\n[claude]\nbinary = \"{}\"\n", claude.display()),
            None => toml,
        };
        std::fs::write(self.dir.join("xdg-config/tmux-tools/agents.toml"), toml).unwrap();
    }

    /// Run the CLI with the run's environment.
    fn cli(&self, args: &[&str], call: Call<'_>) -> CmdOutput {
        let mut command = Command::new(BIN);
        command.args(args);
        command.env_clear();
        for (key, value) in original_environment() {
            command.env(key, value);
        }
        command
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("TMUX_TOOLS_HOST");
        command.env("XDG_STATE_HOME", self.state_dir());
        command.env("XDG_CONFIG_HOME", self.dir.join("xdg-config"));
        command.env("TT_SHIM_REAL", &self.real_orca);
        command.env("TT_SHIM_CREATED", self.file("shim-created.jsonl"));
        for (key, value) in &self.extra_env {
            command.env(key, value);
        }
        if call.shim {
            command.env(
                "PATH",
                format!(
                    "{}:{}",
                    self.shim_dir.display(),
                    self.extra_env
                        .iter()
                        .find(|(key, _)| key == "PATH")
                        .map(|(_, value)| value.clone())
                        .or_else(|| original("PATH"))
                        .unwrap_or_default()
                ),
            );
        }
        for key in &call.remove {
            command.env_remove(key);
        }
        for (key, value) in &call.env {
            command.env(key, value);
        }
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tmux-tools");
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            call.timeout.unwrap_or(Duration::from_secs(180)),
        )
    }

    fn ok(&self, args: &[&str]) -> CmdOutput {
        let out = self.cli(args, Call::default());
        out.assert_success(&format!("{args:?}"));
        out
    }

    fn json(&self, args: &[&str], call: Call<'_>) -> Value {
        let out = self.cli(args, call);
        out.assert_success(&format!("{args:?}"));
        serde_json::from_str(out.stdout.trim()).unwrap_or_else(|error| {
            panic!("{args:?}: stdout is not JSON ({error}): {}", out.stdout)
        })
    }

    /// `orca <args> --json` directly, reading `result`.
    fn orca(&self, args: &[&str]) -> Value {
        let out = Command::new(&self.real_orca)
            .args(args)
            .arg("--json")
            .envs(self.extra_env.iter().map(|(key, value)| (key, value)))
            .output()
            .expect("run orca");
        let parsed: Value = serde_json::from_slice(&out.stdout).expect("orca answers JSON");
        parsed["result"].clone()
    }

    fn terminals(&self) -> Vec<Value> {
        self.orca(&["terminal", "list"])["terminals"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn orca_handles(&self) -> HashSet<String> {
        self.terminals()
            .iter()
            .filter_map(|terminal| terminal["handle"].as_str().map(str::to_owned))
            .collect()
    }

    fn track(&self, handle: &str) {
        let mut created = self.created.borrow_mut();
        if !created.iter().any(|known| known == handle) {
            created.push(handle.to_owned());
        }
    }

    /// The Pane records of the run's private registry, every runtime.
    fn own_records(&self) -> Vec<Value> {
        let root = self.state_dir().join("tmux-tools/orca");
        let Ok(runtimes) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        runtimes
            .flatten()
            .filter_map(|runtime| std::fs::read_dir(runtime.path()).ok())
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::read(entry.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .collect()
    }

    /// Record every pane with run-specific evidence that it is the run's own: a handle
    /// in the run's registry, a handle the shim saw the real `orca` return to a run
    /// call, or a run-unique `paneTitle` among the listed panes (a late arrival after
    /// an ambiguous creation). A pane that is merely new in the worktree is not recorded.
    fn discover(&self) {
        for record in self.own_records() {
            if let Some(handle) = record["handle"].as_str() {
                self.track(handle);
            }
        }
        for line in std::fs::read_to_string(self.file("shim-created.jsonl"))
            .unwrap_or_default()
            .lines()
        {
            if let Some(handle) = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|value| value["handle"].as_str().map(str::to_owned))
            {
                self.track(&handle);
            }
        }
        let names = self.names.borrow().clone();
        let split_tabs = self.split_tabs.borrow().clone();
        for terminal in self.terminals() {
            let titled = terminal["paneTitle"]
                .as_str()
                .is_some_and(|title| names.contains(title));
            let split_late = terminal["tabId"].as_str().is_some_and(|tab| {
                terminal["handle"].as_str().is_some_and(|handle| {
                    split_tabs
                        .iter()
                        .any(|(owned, before)| owned == tab && !before.contains(handle))
                })
            });
            if titled || split_late {
                if let Some(handle) = terminal["handle"].as_str() {
                    self.track(handle);
                }
            }
        }
    }

    /// The listed handles the run owns.
    fn owned_listed(&self) -> Vec<String> {
        self.discover();
        let listed = self.orca_handles();
        self.created
            .borrow()
            .iter()
            .filter(|handle| listed.contains(*handle))
            .cloned()
            .collect()
    }

    /// Run a CLI call that may create a pane. The run's own panes are recorded before
    /// the call's outcome is checked, so a pane from a failed, refused or timed-out
    /// call is closed too (P10.4); a call without a known outcome makes the final
    /// reconciliation wait for a late arrival.
    ///
    /// A `--split h|v` call must name a run-owned pane with `--target`: the tab of that
    /// pane is then the run's evidence for a split pane that opens late, untitled.
    fn cli_creating(&self, args: &[&str], call: Call<'_>) -> CmdOutput {
        self.anchor_split(args);
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.cli(args, call)));
        match &out {
            Ok(done) if done.stderr.contains("spawn outcome unknown") => self.uncertain.set(true),
            Ok(_) => {}
            Err(_) => self.uncertain.set(true),
        }
        self.discover();
        out.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// Before a `--split h|v` call: remember the tab of its run-owned target and the
    /// handles already in it.
    fn anchor_split(&self, args: &[&str]) {
        let value = |flag: &str| {
            args.iter()
                .position(|arg| *arg == flag)
                .and_then(|index| args.get(index + 1).copied())
        };
        if !matches!(value("--split"), Some("h" | "v")) {
            return;
        }
        let target = value("--target").unwrap_or_else(|| {
            panic!("{args:?}: a run's split must name a run-owned pane with --target")
        });
        self.discover();
        let handle = self
            .own_records()
            .iter()
            .find(|record| record["name"] == target)
            .and_then(|record| record["handle"].as_str().map(str::to_owned))
            .unwrap_or_else(|| target.to_owned());
        assert!(
            self.created.borrow().contains(&handle),
            "{args:?}: split target {target} is not a run-owned pane"
        );
        let terminals = self.terminals();
        let tab = terminals
            .iter()
            .find(|terminal| terminal["handle"] == handle.as_str())
            .and_then(|terminal| terminal["tabId"].as_str().map(str::to_owned))
            .unwrap_or_else(|| panic!("split target {handle} is not listed"));
        let before = terminals
            .iter()
            .filter(|terminal| terminal["tabId"] == tab.as_str())
            .filter_map(|terminal| terminal["handle"].as_str().map(str::to_owned))
            .collect();
        self.split_tabs.borrow_mut().push((tab, before));
    }

    /// Spawn through `args` (`--format json` appended), record the new pane, and return
    /// the JSON result.
    fn spawn(&self, args: &[&str], call: Call<'_>) -> Value {
        let mut full: Vec<&str> = args.to_vec();
        full.extend(["--format", "json"]);
        let out = self.cli_creating(&full, call);
        out.assert_success(&format!("{full:?}"));
        let result: Value = serde_json::from_str(out.stdout.trim()).unwrap();
        self.track(result["pane_id"].as_str().unwrap());
        result
    }

    fn launch(&self, name: &str, cmd: &str) -> String {
        let result = self.spawn(&["launch", "--name", name, "--cmd", cmd], Call::default());
        result["pane_id"].as_str().unwrap().to_owned()
    }

    fn launch_with(&self, name: &str, call: Call<'_>) -> String {
        let result = self.spawn(&["launch", "--name", name, "--cmd", "sleep 30"], call);
        result["pane_id"].as_str().unwrap().to_owned()
    }

    fn kill(&self, target: &str) {
        let out = self.cli(&["kill", "--target", target, "--any"], Call::default());
        out.assert_success(&format!("kill {target}"));
    }

    fn record_path(&self, name: &str) -> PathBuf {
        let runtime = self.orca(&["status"])["runtime"]["runtimeId"]
            .as_str()
            .unwrap()
            .to_owned();
        self.state_dir()
            .join("tmux-tools/orca")
            .join(runtime)
            .join(format!("{name}.json"))
    }

    fn screen(&self, target: &str) -> String {
        self.ok(&["capture", "--target", target]).stdout
    }

    fn wait_for_screen(&self, target: &str, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let screen = self.screen(target);
            if screen.contains(needle) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "{target}: {needle:?} never appeared; screen:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    fn shim_log(&self, log: &Path) -> Vec<Vec<String>> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Run {
    /// Close every pane the run owns that is still listed: through `kill`, and through
    /// `orca terminal close` for one `kill` could not close. When a creation outcome
    /// stayed unknown, or a reservation of the run is left, keep re-scanning for the
    /// run's late arrivals for `late_wait` and close them too. Panes without
    /// run-specific evidence are never closed.
    fn close_created(&self) {
        let names = self.names.borrow().clone();
        let reserved = self.own_records().iter().any(|record| {
            record["handle"].is_null()
                && record["name"]
                    .as_str()
                    .is_some_and(|name| names.contains(name))
        });
        let deadline = Instant::now()
            + if self.uncertain.get() || reserved {
                self.late_wait
            } else {
                Duration::ZERO
            };
        loop {
            for handle in self.owned_listed() {
                let _ = self.cli(
                    &["kill", "--target", &handle, "--any", "--force"],
                    Call::default(),
                );
            }
            for handle in self.owned_listed() {
                let _ = Command::new(&self.real_orca)
                    .args(["terminal", "close", "--terminal", &handle, "--json"])
                    .envs(self.extra_env.iter().map(|(key, value)| (key, value)))
                    .output();
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.close_created();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A new directory `<prefix>-<pid>-<ms>-<seq>` under the temp dir, created exclusively,
/// so no two callers (threads or processes) ever share one, and a run-unique tag
/// derived from that identity.
fn unique_dir(prefix: &str) -> (PathBuf, String) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    loop {
        let stamp = now_ms();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("{prefix}-{pid}-{stamp}-{seq}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                let tag = format!(
                    "{:06x}{seq:x}",
                    ((stamp as u32) ^ pid.rotate_left(13)) & 0xff_ffff
                );
                return (dir, tag);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create {}: {error}", dir.display()),
        }
    }
}

fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = list.iter().map(|key| (*key).to_owned()).collect();
    keys.sort();
    keys
}

/// The tmux key sets of P9.13's verbs (from the tmux run's JSON results).
const TMUX_STATUS_KEYS: &[&str] = &[
    "host",
    "session",
    "window",
    "window_index",
    "pane",
    "pane_id",
    "name",
    "agent",
    "access",
    "launched_at",
    "cwd",
    "min_pane_cols",
    "min_pane_rows",
    "below_floor",
    "host_state",
];
const TMUX_LIST_ROW_KEYS: &[&str] = &[
    "host",
    "name",
    "agent",
    "session",
    "pane",
    "access",
    "launched",
    "status",
    "last_activity",
    "host_state",
    // PRD-261108-0447-01 P8.2.
    "parent",
    "team",
    "shim_id",
    "orphan",
];
const TMUX_LAUNCH_KEYS: &[&str] = &[
    "host",
    "name",
    "pane_id",
    "launched_at",
    "relocated",
    "window_id",
    "min_pane_cols",
    "min_pane_rows",
    "below_floor",
];
const TMUX_SPAWN_KEYS: &[&str] = &[
    "host",
    "agent",
    "access",
    "surface",
    "name",
    "pane_id",
    "binary",
    "argv",
    "launched_at",
    "relocated",
    "window_id",
    "min_pane_cols",
    "min_pane_rows",
    "below_floor",
];

type Scenario = fn(&Run);

#[test]
fn orca_contract_suite() {
    if !live() {
        return;
    }
    assert!(
        original("ORCA_TERMINAL_HANDLE").is_some() && original("ORCA_WORKTREE_ID").is_some(),
        "the live Orca run must run inside an Orca terminal"
    );
    let _serial = super::serial_guard();
    let run = Run::start();
    let before = run.orca_handles();

    let scenarios: &[(&str, Scenario)] = &[
        (
            "required verbs on a shell pane",
            required_verbs_on_a_shell_pane,
        ),
        ("key-name and --literal sends", key_name_and_literal_sends),
        (
            "interrupt sends the surface key",
            interrupt_sends_the_surface_key,
        ),
        (
            "ambiguous split recovered by --retry-split",
            ambiguous_split_is_recovered,
        ),
        (
            "prompt spanning several read pages",
            prompt_spans_several_pages,
        ),
        ("refused multi-line prompt", refused_multi_line_prompt),
        ("full-screen turn", full_screen_turn),
        ("self in a spawned and an unregistered pane", self_reports),
        ("calling pane and ORCA_TERMINAL_HANDLE", calling_pane_rules),
        ("--session and --window exit 3", scope_options_exit_3),
        (
            "optional verbs and options exit 3",
            optional_features_exit_3,
        ),
        ("invalid and reserved names", names_on_orca),
        (
            "registry: stale runtime, dead, orphan, in progress",
            registry_reconciliation,
        ),
        ("registry: incomplete pane list", incomplete_pane_list),
        ("kill ownership", kill_ownership),
        ("host state mapping", host_state_mapping),
        (
            "waits and the settle with Host state",
            waits_follow_host_state,
        ),
        ("submission receipts", submission_receipts),
        (
            "runtime without prompt delivery",
            runtime_without_prompt_delivery,
        ),
        ("TMUX_TOOLS_HOST=orca over $TMUX", override_wins_over_tmux),
        (
            "claude teams: spawn-agent claude through the Launcher (PRD-261108-0447-01 P10.6)",
            claude_teams_through_spawn_agent,
        ),
    ];
    // Development aid: run only the scenarios whose label contains this text.
    let only = original(ONLY_SWITCH);

    let mut failures = Vec::new();
    for (label, scenario) in scenarios {
        if only
            .as_ref()
            .is_some_and(|only| !label.contains(only.as_str()))
        {
            continue;
        }
        eprintln!("orca contract: {label}");
        if let Err(panic) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scenario(&run)))
        {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            failures.push(format!("{label}: {message}"));
        }
    }

    // P10.4: close what is left, then assert no pane this run created is listed. A
    // new pane without run-specific evidence is reported, never closed.
    run.close_created();
    let leaked = run.owned_listed();
    let foreign: Vec<String> = run
        .orca_handles()
        .difference(&before)
        .filter(|handle| !leaked.contains(*handle))
        .cloned()
        .collect();
    if !foreign.is_empty() {
        eprintln!(
            "orca contract: possible leak, not closed (no evidence this run created them): {foreign:?}"
        );
    }
    let tt_list = run.json(&["list", "--format", "json"], Call::default());
    assert!(
        leaked.is_empty(),
        "orca terminal list shows panes this run created: {leaked:?}"
    );
    assert_eq!(
        tt_list,
        json!([]),
        "tmux-tools list shows panes this run created"
    );
    assert!(
        failures.is_empty(),
        "{} scenario(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn required_verbs_on_a_shell_pane(run: &Run) {
    let worktree = original("ORCA_WORKTREE_ID").unwrap();
    let launched = run.spawn(
        &[
            "launch",
            "--name",
            run.n("c-shell").as_str(),
            "--cmd",
            "echo C-SHELL-UP",
        ],
        Call::default(),
    );
    assert_eq!(launched["host"], "orca");
    assert_eq!(keys(&launched), sorted(TMUX_LAUNCH_KEYS), "launch keys");
    for key in [
        "relocated",
        "window_id",
        "min_pane_cols",
        "min_pane_rows",
        "below_floor",
    ] {
        assert!(
            launched[key].is_null(),
            "launch {key} must be null: {launched}"
        );
    }
    let handle = launched["pane_id"].as_str().unwrap().to_owned();
    run.wait_for_screen(run.n("c-shell").as_str(), "C-SHELL-UP");

    let rows = run.json(&["list", "--format", "json"], Call::default());
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == run.n("c-shell").as_str())
        .expect("list shows c-shell")
        .clone();
    assert_eq!(keys(&row), sorted(TMUX_LIST_ROW_KEYS), "list row keys");
    assert_eq!(row["session"], worktree.as_str());
    assert_eq!(row["pane"], handle.as_str());
    assert_eq!(row["status"], "running");
    assert!(row["last_activity"].is_null());
    assert_eq!(row["host"], "orca");

    let status = run.json(
        &[
            "status",
            "--target",
            run.n("c-shell").as_str(),
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(keys(&status), sorted(TMUX_STATUS_KEYS), "status keys");
    for key in [
        "window_index",
        "pane",
        "min_pane_cols",
        "min_pane_rows",
        "below_floor",
    ] {
        assert!(status[key].is_null(), "status {key} must be null: {status}");
    }
    let shown = run.orca(&["terminal", "show", "--terminal", &handle]);
    assert_eq!(status["window"], shown["terminal"]["tabId"]);
    assert_eq!(status["host"], "orca");

    let executed = run.json(
        &[
            "execute",
            "--target",
            run.n("c-shell").as_str(),
            "echo EXEC-$((40+2))",
            "--format",
            "json",
            "--timeout",
            "30",
        ],
        Call::default(),
    );
    assert_eq!(executed["exit_code"], 0, "{executed}");
    assert!(
        executed["output"].as_str().unwrap().contains("EXEC-42"),
        "{executed}"
    );
    assert_eq!(executed["host"], "orca");

    let sent = run.json(
        &[
            "send",
            "--target",
            run.n("c-shell").as_str(),
            "echo SENT-OK",
            "--enter",
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(sent["host"], "orca");
    run.wait_for_screen(run.n("c-shell").as_str(), "SENT-OK");
    let entered = run.json(
        &[
            "send-enter",
            "--target",
            run.n("c-shell").as_str(),
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(entered["host"], "orca");
    let escaped = run.json(
        &[
            "escape",
            "--target",
            run.n("c-shell").as_str(),
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(escaped["host"], "orca");

    let by_handle = run.json(
        &["capture", "--target", &handle, "--format", "json"],
        Call::default(),
    );
    assert_eq!(by_handle["host"], "orca");
    assert!(by_handle["output"].as_str().unwrap().contains("SENT-OK"));
    let all = run.cli(
        &["capture", "--target", run.n("c-shell").as_str(), "--all"],
        Call::default(),
    );
    all.assert_success("capture --all");
    assert!(all.stdout.contains("C-SHELL-UP"));
    assert!(
        !all.stderr.contains("alternate screen"),
        "capture --all on Orca prints no full-screen notice: {}",
        all.stderr
    );

    let waited = run.json(
        &[
            "wait-idle",
            "--target",
            run.n("c-shell").as_str(),
            "--timeout",
            "20",
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(waited["reason"], "idle", "{waited}");
    assert!(waited["host_state"].is_null());
    assert_eq!(waited["host"], "orca");

    let notified = run.cli(
        &[
            "notify",
            "--title",
            "t",
            "--body",
            "b",
            "--target",
            run.n("c-shell").as_str(),
        ],
        Call::default(),
    );
    assert_eq!(notified.status, 0);
    assert!(
        notified.stderr.contains("notify: not delivered on orca"),
        "{}",
        notified.stderr
    );

    let record = run.record_path(run.n("c-shell").as_str());
    assert!(record.exists());
    let killed = run.json(
        &[
            "kill",
            "--target",
            run.n("c-shell").as_str(),
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(killed["host"], "orca");
    assert!(!record.exists(), "kill deletes the Pane record");
    let rows = run.json(&["list", "--format", "json"], Call::default());
    assert!(!rows
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == run.n("c-shell").as_str()));
    assert!(!run.orca_handles().contains(&handle));
}

fn key_name_and_literal_sends(run: &Run) {
    let spawned = run.spawn(
        &["spawn-agent", "tthex", "--name", run.n("c-hex").as_str()],
        Call::default(),
    );
    assert_eq!(keys(&spawned), sorted(TMUX_SPAWN_KEYS), "spawn-agent keys");
    for key in [
        "relocated",
        "window_id",
        "min_pane_cols",
        "min_pane_rows",
        "below_floor",
    ] {
        assert!(spawned[key].is_null(), "spawn-agent {key} must be null");
    }
    run.wait_for_screen(run.n("c-hex").as_str(), "HEXECHO READY");
    for argument in ["M-x", "F1", "Up", "y", "^c", "plainword"] {
        run.ok(&["send", "--target", run.n("c-hex").as_str(), argument]);
        std::thread::sleep(Duration::from_millis(300));
    }
    run.ok(&[
        "send",
        "--target",
        run.n("c-hex").as_str(),
        "--literal",
        "M-x",
    ]);
    let screen = run.wait_for_screen(run.n("c-hex").as_str(), "HEX 4d2d78");
    let hex: Vec<&str> = screen
        .lines()
        .filter_map(|line| line.trim().strip_prefix("HEX "))
        .collect();
    assert_eq!(
        hex,
        [
            "1b78",
            "1b4f50",
            "1b5b41",
            "79",
            "03",
            "706c61696e776f7264",
            "4d2d78"
        ],
        "P9.4 bytes; screen:\n{screen}"
    );
    run.kill(run.n("c-hex").as_str());
}

fn interrupt_sends_the_surface_key(run: &Run) {
    let log = run.file("interrupt-log.jsonl");
    let report = run.file("fake-report.bin");
    let _ = std::fs::remove_file(&report);
    run.spawn(
        &["spawn-agent", "ttfake", "--name", run.n("c-int").as_str()],
        Call::default(),
    );
    std::thread::sleep(Duration::from_secs(2));
    let out = run.cli(
        &["interrupt", "--target", run.n("c-int").as_str()],
        Call {
            shim: true,
            env: vec![("TT_SHIM_LOG", log.display().to_string())],
            ..Call::default()
        },
    );
    out.assert_success("interrupt");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let bytes = std::fs::read(&report).unwrap_or_default();
        if bytes.windows(2).any(|w| w == b"\n\x15") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the fixture never received C-u (0x15): {bytes:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        !run.shim_log(&log)
            .iter()
            .flatten()
            .any(|arg| arg == "--interrupt"),
        "no orca call carries --interrupt"
    );
    run.kill(run.n("c-int").as_str());
}

fn ambiguous_split_is_recovered(run: &Run) {
    let log = run.file("split-log.jsonl");
    let state = run.file("split-state.json");
    let anchor = run.launch(run.n("c-split-anchor").as_str(), "sleep 120");
    let before = run.orca_handles();
    let out = run.cli_creating(
        &[
            "spawn-agent",
            "ttfake",
            "--name",
            run.n("c-split").as_str(),
            "--split",
            "h",
            "--target",
            &anchor,
            "--format",
            "json",
        ],
        Call {
            shim: true,
            env: vec![
                ("TT_SHIM_LOG", log.display().to_string()),
                ("TT_SHIM_SPLIT_TIMEOUT_STATE", state.display().to_string()),
            ],
            ..Call::default()
        },
    );
    let new: Vec<String> = run.orca_handles().difference(&before).cloned().collect();
    out.assert_success("spawn-agent --split h through an ambiguous split");
    assert!(
        state.exists(),
        "the shim must have timed out the first split"
    );
    assert_eq!(new.len(), 1, "exactly one new pane: {new:?}");
    let result: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(result["pane_id"], new[0].as_str());
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.record_path(run.n("c-split").as_str())).unwrap())
            .unwrap();
    assert_eq!(
        record["handle"],
        new[0].as_str(),
        "the record holds that pane's handle"
    );
    assert!(
        run.shim_log(&log)
            .iter()
            .any(|call| call.iter().any(|arg| arg == "--retry-split")),
        "recovered through --retry-split"
    );
    run.kill(run.n("c-split").as_str());
    run.kill(run.n("c-split-anchor").as_str());
}

fn prompt_spans_several_pages(run: &Run) {
    run.launch(run.n("c-pages").as_str(), "echo PAGES-UP");
    run.wait_for_screen(run.n("c-pages").as_str(), "PAGES-UP");
    let result = run.json(
        &[
            "prompt",
            "--target",
            run.n("c-pages").as_str(),
            "seq 1 1500",
            "--timeout",
            "60",
            "--format",
            "json",
        ],
        Call {
            env: vec![("TMUX_TOOLS_TEST_ORCA_PAGE_LINES", "200".to_owned())],
            ..Call::default()
        },
    );
    let output = result["output_since_prompt"].as_str().unwrap();
    let numbers: HashSet<&str> = output.lines().map(str::trim).collect();
    for n in [1, 2, 199, 200, 201, 750, 1499, 1500] {
        assert!(
            numbers.contains(n.to_string().as_str()),
            "line {n} missing from the multi-page result"
        );
    }
    assert_eq!(result["complete"], false);
    assert_eq!(result["host"], "orca");
    run.kill(run.n("c-pages").as_str());
}

fn refused_multi_line_prompt(run: &Run) {
    run.spawn(
        &[
            "spawn-agent",
            "ttfake",
            "--name",
            run.n("c-refuse").as_str(),
        ],
        Call::default(),
    );
    std::thread::sleep(Duration::from_secs(2));
    let handle = run.json(
        &[
            "status",
            "--target",
            run.n("c-refuse").as_str(),
            "--format",
            "json",
        ],
        Call::default(),
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let before = run.orca(&["terminal", "read", "--terminal", &handle, "--limit", "1"])["terminal"]
        ["latestCursor"]
        .clone();
    let out = run.cli(
        &[
            "prompt",
            "--target",
            run.n("c-refuse").as_str(),
            "line one\nline two",
        ],
        Call::default(),
    );
    assert_eq!(
        out.status, 1,
        "a refused multi-line prompt exits 1: {out:?}"
    );
    std::thread::sleep(Duration::from_secs(1));
    let after = run.orca(&["terminal", "read", "--terminal", &handle, "--limit", "1"])["terminal"]
        ["latestCursor"]
        .clone();
    assert_eq!(before, after, "orca terminal read shows no new output");
    run.kill(run.n("c-refuse").as_str());
}

fn full_screen_turn(run: &Run) {
    run.spawn(
        &["spawn-agent", "ttalt", "--name", run.n("c-alt").as_str()],
        Call::default(),
    );
    std::thread::sleep(Duration::from_secs(2));
    let record = run.record_path(run.n("c-alt").as_str());
    let before = std::fs::read(&record).unwrap();
    let result = run.json(
        &[
            "prompt",
            "--target",
            run.n("c-alt").as_str(),
            "hello full screen",
            "--timeout",
            "30",
            "--format",
            "json",
        ],
        Call::default(),
    );
    assert_eq!(result["complete"], false, "{result}");
    assert_eq!(
        std::fs::read(&record).unwrap(),
        before,
        "the Pane record is unchanged"
    );
    run.kill(run.n("c-alt").as_str());
}

fn self_reports(run: &Run) {
    let _ = std::fs::remove_file(run.file("self-out.json"));
    run.spawn(
        &["spawn-agent", "ttself", "--name", run.n("c-self").as_str()],
        Call::default(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let out = loop {
        if let Ok(text) = std::fs::read_to_string(run.file("self-out.json")) {
            if text.ends_with('\n') {
                break text;
            }
        }
        assert!(
            Instant::now() < deadline,
            "self never ran in the spawned pane"
        );
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(
        out.trim(),
        format!(
            r#"{{"host":"orca","driven":true,"name":"{}","agent":"ttself","team":null,"role":null,"parent":null}}"#,
            run.n("c-self")
        )
    );
    run.kill(run.n("c-self").as_str());

    // The suite's own Orca terminal is unregistered.
    let unregistered = run.ok(&["self", "--format", "json"]);
    assert_eq!(
        unregistered.stdout.trim(),
        r#"{"host":"orca","driven":false,"name":null,"agent":null,"team":null,"role":null,"parent":null}"#
    );
}

fn calling_pane_rules(run: &Run) {
    for args in [
        vec!["self", "--format", "json"],
        vec!["notify", "--title", "t", "--body", "b"],
        vec!["capture"],
    ] {
        let out = run.cli(
            &args,
            Call {
                env: vec![("TMUX_TOOLS_HOST", "orca".to_owned())],
                remove: vec!["ORCA_TERMINAL_HANDLE"],
                ..Call::default()
            },
        );
        assert_eq!(
            out.status, 1,
            "{args:?} without ORCA_TERMINAL_HANDLE: {out:?}"
        );
        assert!(
            out.stderr.contains("ORCA_TERMINAL_HANDLE"),
            "{args:?}: {}",
            out.stderr
        );
    }
    let handle = run.launch(run.n("c-calling").as_str(), "echo CALLING-MARK");
    run.wait_for_screen(run.n("c-calling").as_str(), "CALLING-MARK");
    let own = run.cli(
        &["capture"],
        Call {
            env: vec![("ORCA_TERMINAL_HANDLE", handle.clone())],
            ..Call::default()
        },
    );
    own.assert_success("capture without --target");
    assert!(
        own.stdout.contains("CALLING-MARK"),
        "capture reads the calling pane: {}",
        own.stdout
    );
    let by_handle = run.ok(&["capture", "--target", &handle]);
    assert!(by_handle.stdout.contains("CALLING-MARK"));
    run.kill(run.n("c-calling").as_str());
}

fn scope_options_exit_3(run: &Run) {
    for (flag, message) in [
        ("--session", "unsupported: --session on orca"),
        ("--window", "unsupported: --window on orca"),
    ] {
        let out = run.cli(&["capture", flag, "x"], Call::default());
        assert_eq!(out.status, 3, "capture {flag}: {out:?}");
        assert_eq!(out.stderr.trim(), message);
    }
}

fn optional_features_exit_3(run: &Run) {
    let before = run.orca_handles();
    for (args, message) in [
        (vec!["rebalance"], "unsupported: rebalance on orca"),
        (
            vec!["launch", "--cmd", "true", "--split", "tile"],
            "unsupported: --split tile on orca",
        ),
        (
            vec!["launch", "--cmd", "true", "--size", "30"],
            "unsupported: --size on orca",
        ),
    ] {
        let out = run.cli_creating(&args, Call::default());
        assert_eq!(out.status, 3, "{args:?}: {out:?}");
        assert_eq!(out.stderr.trim(), message);
    }
    assert_eq!(
        run.orca_handles(),
        before,
        "no optional verb or option creates a pane"
    );
    let notified = run.cli(&["notify", "--title", "t", "--body", "b"], Call::default());
    assert_eq!(notified.status, 0);
    assert!(notified.stderr.contains("notify: not delivered on orca"));
}

fn names_on_orca(run: &Run) {
    let before = run.orca_handles();
    let out = run.cli_creating(
        &["launch", "--name", "a/b", "--cmd", "true"],
        Call::default(),
    );
    assert_eq!(out.status, 1);
    assert_eq!(out.stderr, "invalid name: a/b\n");
    assert_eq!(
        run.orca_handles(),
        before,
        "an invalid name creates no pane"
    );

    run.launch(run.n("c-taken").as_str(), "echo TAKEN");
    let before = run.orca_handles();
    let again = run.cli_creating(
        &["spawn-agent", "ttfake", "--name", run.n("c-taken").as_str()],
        Call::default(),
    );
    assert_eq!(
        again.status, 1,
        "a reserved name fails the spawn: {again:?}"
    );
    assert_eq!(
        run.orca_handles(),
        before,
        "a reserved name creates no pane"
    );
    run.kill(run.n("c-taken").as_str());
}

fn write_record(run: &Run, name: &str, handle: Option<&str>, cwd: &str) -> PathBuf {
    let path = run.record_path(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let runtime = path
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let record = json!({
        "schema": "tmux-tools.pane.v1", "name": name, "host": "orca", "server_id": runtime,
        "handle": handle, "agent": null, "access": null, "surface": null,
        "surface_unvalidated": false, "cwd": cwd, "launched_at": "2026-01-01T00:00:00Z"
    });
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    path
}

fn age_file(path: &Path) {
    let status = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn registry_reconciliation(run: &Run) {
    let stale = run
        .state_dir()
        .join("tmux-tools/orca/not-the-current-runtime");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("x.json"), "{}").unwrap();
    run.ok(&["list"]);
    assert!(!stale.exists(), "list deletes a stale runtime directory");

    let cwd = std::env::current_dir().unwrap().display().to_string();
    let dead = write_record(
        run,
        "c-dead",
        Some("term_00000000-dead-dead-dead-000000000000"),
        &cwd,
    );
    let rows = run.json(&["list", "--format", "json"], Call::default());
    let dead_rows: Vec<&Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["name"] == "c-dead")
        .collect();
    assert_eq!(dead_rows.len(), 1, "one row for the dead name: {rows}");
    assert_eq!(dead_rows[0]["status"], "dead");
    assert!(!dead.exists(), "the dead record is deleted");
    let rows = run.json(&["list", "--format", "json"], Call::default());
    assert!(!rows
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["name"] == "c-dead"));

    let orphan = write_record(run, "c-orphan", None, &cwd);
    age_file(&orphan);
    run.ok(&["list"]);
    assert!(!orphan.exists(), "an old reservation is deleted");

    let young = write_record(run, "c-young", None, &cwd);
    let out = run.cli(&["capture", "--target", "c-young"], Call::default());
    assert_eq!(out.status, 1);
    assert!(
        out.stderr.contains("spawn in progress: c-young"),
        "{}",
        out.stderr
    );
    let rows = run.json(&["list", "--format", "json"], Call::default());
    assert!(
        !rows
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "c-young"),
        "list omits a spawn in progress"
    );
    assert!(young.exists(), "reconciliation leaves a spawn in progress");
    std::fs::remove_file(&young).unwrap();
}

fn incomplete_pane_list(run: &Run) {
    run.launch(run.n("c-a").as_str(), "echo PANE-A");
    run.wait_for_screen(run.n("c-a").as_str(), "PANE-A");
    let cwd = std::env::current_dir().unwrap().display().to_string();
    let b = write_record(
        run,
        "c-b",
        Some("term_00000000-hidden-0000-0000-000000000000"),
        &cwd,
    );
    let a = run.record_path(run.n("c-a").as_str());
    for env in [("TT_SHIM_LIST_TRUNCATED", "1"), ("TT_SHIM_LIST_OMIT", "1")] {
        let call = || Call {
            shim: true,
            env: vec![(env.0, env.1.to_owned())],
            ..Call::default()
        };
        let out = run.cli(&["capture", "--target", "c-b"], call());
        assert_eq!(out.status, 1, "{env:?}");
        assert!(
            out.stderr.contains("pane list incomplete: c-b"),
            "{env:?}: {}",
            out.stderr
        );
        assert!(b.exists());
        let read_a = run.cli(&["capture", "--target", run.n("c-a").as_str()], call());
        read_a.assert_success("capture a on an incomplete list");
        assert!(read_a.stdout.contains("PANE-A"));
        let listed = run.cli(&["list", "--format", "json"], call());
        listed.assert_success("list on an incomplete list");
        assert!(
            listed.stderr.contains("list incomplete: orca"),
            "{}",
            listed.stderr
        );
        let rows: Value = serde_json::from_str(listed.stdout.trim()).unwrap();
        let status = |name: &str| {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .map(|row| row["status"].clone())
        };
        assert_eq!(status("c-b"), Some(json!("unknown")), "{env:?}: {rows}");
        assert_eq!(
            status(run.n("c-a").as_str()),
            Some(json!("running")),
            "{env:?}: {rows}"
        );
        assert!(a.exists() && b.exists(), "both record files stay");
    }
    std::fs::remove_file(&b).unwrap();
    run.kill(run.n("c-a").as_str());
}

fn kill_ownership(run: &Run) {
    let handle = run.launch(run.n("c-own").as_str(), "echo OWN");
    // A pane with no Pane record.
    let record = run.record_path(run.n("c-own").as_str());
    let saved = std::fs::read(&record).unwrap();
    std::fs::remove_file(&record).unwrap();
    let out = run.cli(&["kill", "--target", &handle], Call::default());
    assert_ne!(out.status, 0, "kill refuses an unregistered pane");
    assert!(run.orca_handles().contains(&handle));
    // A record whose cwd differs.
    let mut other: Value = serde_json::from_slice(&saved).unwrap();
    other["cwd"] = json!("/somewhere/else");
    std::fs::write(&record, serde_json::to_vec(&other).unwrap()).unwrap();
    let out = run.cli(
        &["kill", "--target", run.n("c-own").as_str()],
        Call::default(),
    );
    assert_ne!(out.status, 0, "kill refuses a record of another cwd");
    assert!(run.orca_handles().contains(&handle));
    std::fs::write(&record, &saved).unwrap();
    run.ok(&["kill", "--target", run.n("c-own").as_str()]);
    assert!(!record.exists());
    assert!(!run.orca_handles().contains(&handle));
}

fn ps_row(run: &Run, file: &Path, handle: &str, row: Value) {
    let _ = run;
    std::fs::write(
        file,
        serde_json::to_vec(&json!({"handle": handle, "row": row})).unwrap(),
    )
    .unwrap();
}

fn host_state_mapping(run: &Run) {
    let handle = run.spawn(
        &["spawn-agent", "ttfake", "--name", run.n("c-hs").as_str()],
        Call::default(),
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file = run.file("ps-row.json");
    for (row, expected) in [
        (
            json!({"state": "working", "hasLivePty": true, "stateStartedAt": 1}),
            json!("working"),
        ),
        (
            json!({"state": "blocked", "hasLivePty": true, "stateStartedAt": 1}),
            json!("blocked"),
        ),
        (
            json!({"state": "waiting", "hasLivePty": true, "stateStartedAt": 1}),
            json!("blocked"),
        ),
        (
            json!({"state": "done", "hasLivePty": true, "stateStartedAt": 1}),
            json!("done"),
        ),
        (
            json!({"state": "done", "hasLivePty": false, "stateStartedAt": 1}),
            Value::Null,
        ),
        (
            json!({"state": "done", "hasLivePty": true, "stateStartedAt": 1, "subagents": [{"state": "working"}]}),
            json!("working"),
        ),
    ] {
        ps_row(run, &file, &handle, row.clone());
        let status = run.json(
            &[
                "status",
                "--target",
                run.n("c-hs").as_str(),
                "--format",
                "json",
            ],
            Call {
                shim: true,
                env: vec![("TT_SHIM_PS_ROW", file.display().to_string())],
                ..Call::default()
            },
        );
        assert_eq!(status["host_state"], expected, "row {row}");
        assert_ne!(status["host_state"], "idle");
    }
    run.kill(run.n("c-hs").as_str());
}

fn timed(run: &Run, args: &[&str], call: Call<'_>) -> (CmdOutput, Duration) {
    let started = Instant::now();
    let out = run.cli(args, call);
    (out, started.elapsed())
}

fn waits_follow_host_state(run: &Run) {
    let handle = run.spawn(
        &["spawn-agent", "ttfake", "--name", run.n("c-wait").as_str()],
        Call::default(),
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::thread::sleep(Duration::from_secs(2));
    let file = run.file("ps-wait.json");
    let receipt = run.file("receipt-wait.json");
    std::fs::write(&receipt, serde_json::to_vec(&json!({"ok": true, "result": {"send": {
        "handle": handle, "accepted": true,
        "prompt": {"requestId": "r", "stages": ["input_accepted", "turn_started"], "provider": "fake", "observation": "observed"}}}})).unwrap()).unwrap();
    let call = || Call {
        shim: true,
        env: vec![
            ("TT_SHIM_PS_ROW", file.display().to_string()),
            ("TT_SHIM_SEND_RECEIPT", receipt.display().to_string()),
        ],
        ..Call::default()
    };
    // Working, or done with a working subagent: neither ends before the timeout.
    for row in [
        json!({"state": "working", "hasLivePty": true, "stateStartedAt": 1}),
        json!({"state": "done", "hasLivePty": true, "stateStartedAt": 1, "subagents": [{"state": "working"}]}),
    ] {
        ps_row(run, &file, &handle, row.clone());
        let (out, took) = timed(
            run,
            &[
                "wait-idle",
                "--target",
                run.n("c-wait").as_str(),
                "--timeout",
                "5",
                "--format",
                "json",
            ],
            call(),
        );
        out.assert_success("wait-idle");
        assert!(
            took >= Duration::from_secs(5),
            "wait-idle ended early for {row}: {}",
            out.stdout
        );
        let (out, took) = timed(
            run,
            &[
                "prompt",
                "--target",
                run.n("c-wait").as_str(),
                "x",
                "--timeout",
                "5",
                "--format",
                "json",
            ],
            call(),
        );
        out.assert_success("prompt");
        assert!(
            took >= Duration::from_secs(5),
            "prompt settle ended early for {row}: {}",
            out.stdout
        );
    }
    // Done or blocked ends wait-idle.
    for state in ["done", "blocked"] {
        ps_row(
            run,
            &file,
            &handle,
            json!({"state": state, "hasLivePty": true, "stateStartedAt": 1}),
        );
        let (out, took) = timed(
            run,
            &[
                "wait-idle",
                "--target",
                run.n("c-wait").as_str(),
                "--timeout",
                "20",
                "--format",
                "json",
            ],
            call(),
        );
        out.assert_success("wait-idle");
        assert!(
            took < Duration::from_secs(15),
            "a {state} row ends wait-idle: {}",
            out.stdout
        );
    }
    // The settle ends on done or blocked only when it began after the submission.
    ps_row(
        run,
        &file,
        &handle,
        json!({"state": "done", "hasLivePty": true, "stateStartedAt": now_ms() - 600_000}),
    );
    let (out, took) = timed(
        run,
        &[
            "prompt",
            "--target",
            run.n("c-wait").as_str(),
            "x",
            "--timeout",
            "5",
            "--format",
            "json",
        ],
        call(),
    );
    out.assert_success("prompt");
    assert!(
        took >= Duration::from_secs(5),
        "a done row from before the submission does not end the settle"
    );
    for state in ["done", "blocked"] {
        ps_row(
            run,
            &file,
            &handle,
            json!({"state": state, "hasLivePty": true, "stateStartedAt": now_ms() + 600_000}),
        );
        let (out, took) = timed(
            run,
            &[
                "prompt",
                "--target",
                run.n("c-wait").as_str(),
                "x",
                "--timeout",
                "20",
                "--format",
                "json",
            ],
            call(),
        );
        out.assert_success("prompt");
        assert!(
            took < Duration::from_secs(15),
            "a {state} row after the submission ends the settle"
        );
        let result: Value = serde_json::from_str(out.stdout.trim()).unwrap();
        assert_eq!(result["complete"], false);
    }
    // An explicit --until decides: a done Host state does not end it.
    ps_row(
        run,
        &file,
        &handle,
        json!({"state": "done", "hasLivePty": true, "stateStartedAt": 1}),
    );
    let (out, took) = timed(
        run,
        &[
            "wait-idle",
            "--target",
            run.n("c-wait").as_str(),
            "--until",
            "NEVER-MATCHES-xyz",
            "--timeout",
            "3",
        ],
        call(),
    );
    out.assert_success("wait-idle --until");
    assert!(
        took >= Duration::from_secs(3),
        "--until is not ended by Host state"
    );
    run.kill(run.n("c-wait").as_str());
}

fn submission_receipts(run: &Run) {
    let handle = run.spawn(
        &["spawn-agent", "ttfake", "--name", run.n("c-sub").as_str()],
        Call::default(),
    )["pane_id"]
        .as_str()
        .unwrap()
        .to_owned();
    std::thread::sleep(Duration::from_secs(2));
    let receipt = run.file("receipt.json");
    let log = run.file("receipt-log.jsonl");
    let send = |value: Value| json!({"ok": true, "result": {"send": value}});
    let prompt = |stages: Value, provider: &str, observation: &str| {
        send(json!({"handle": handle, "accepted": true,
            "prompt": {"requestId": "r1", "stages": stages, "provider": provider, "observation": observation}}))
    };
    let call = || Call {
        shim: true,
        env: vec![
            ("TT_SHIM_SEND_RECEIPT", receipt.display().to_string()),
            ("TT_SHIM_LOG", log.display().to_string()),
        ],
        ..Call::default()
    };
    let sends = || {
        run.shim_log(&log)
            .into_iter()
            .filter(|call| call.len() > 1 && call[0] == "terminal" && call[1] == "send")
            .collect::<Vec<_>>()
    };
    for (answer, expected) in [
        (
            prompt(
                json!(["input_accepted", "turn_started"]),
                "claude",
                "observed",
            ),
            "confirmed",
        ),
        (
            prompt(json!(["input_accepted"]), "claude", "timeout"),
            "accepted",
        ),
        (
            prompt(
                json!(["input_accepted", "turn_started"]),
                "unsupported",
                "unsupported",
            ),
            "assumed",
        ),
    ] {
        std::fs::write(&receipt, serde_json::to_vec(&answer).unwrap()).unwrap();
        let result = run.json(
            &[
                "prompt",
                "--target",
                run.n("c-sub").as_str(),
                "x",
                "--timeout",
                "4",
                "--format",
                "json",
            ],
            call(),
        );
        assert_eq!(result["submit"], expected, "{answer}");
    }
    for (answer, needle) in [
        (
            json!({"ok": false, "error": {"code": "agent_prompt_blocked", "message": "menu open", "data": {}}}),
            "agent_prompt_blocked",
        ),
        (
            prompt(json!(["input_accepted"]), "claude", "permission"),
            "permission",
        ),
        (
            send(json!({"handle": handle, "accepted": false, "refusedReason": "terminal-busy"})),
            "terminal-busy",
        ),
        (
            prompt(json!(["input_accepted"]), "claude", "incarnation_replaced"),
            "prompt target restarted",
        ),
    ] {
        let _ = std::fs::remove_file(&log);
        std::fs::write(&receipt, serde_json::to_vec(&answer).unwrap()).unwrap();
        let out = run.cli(
            &[
                "prompt",
                "--target",
                run.n("c-sub").as_str(),
                "x",
                "--timeout",
                "4",
            ],
            call(),
        );
        assert_eq!(out.status, 1, "{answer}: {out:?}");
        assert!(
            out.stderr.contains(needle),
            "{answer}: stderr {}",
            out.stderr
        );
        assert_eq!(sends().len(), 1, "a single terminal send for {answer}");
    }
    std::fs::write(
        &receipt,
        serde_json::to_vec(&prompt(
            json!(["input_accepted", "turn_started"]),
            "claude",
            "observed",
        ))
        .unwrap(),
    )
    .unwrap();
    for (timeout, wait) in [("0.2", "1"), ("5000", "3600")] {
        let _ = std::fs::remove_file(&log);
        let mut short = call();
        short.timeout = Some(Duration::from_secs(60));
        let out = run.cli(
            &[
                "prompt",
                "--target",
                run.n("c-sub").as_str(),
                "x",
                "--timeout",
                timeout,
                "--idle-seconds",
                "0.5",
            ],
            short,
        );
        out.assert_success("prompt");
        let sent = sends();
        let index = sent[0]
            .iter()
            .position(|arg| arg == "--wait-submit")
            .expect("--wait-submit");
        assert_eq!(sent[0][index + 1], wait, "--timeout {timeout}");
    }
    run.kill(run.n("c-sub").as_str());
}

fn runtime_without_prompt_delivery(run: &Run) {
    run.spawn(
        &["spawn-agent", "ttfake", "--name", run.n("c-nopd").as_str()],
        Call::default(),
    );
    run.spawn(
        &[
            "spawn-agent",
            "tthex",
            "--name",
            run.n("c-nopd-hex").as_str(),
        ],
        Call::default(),
    );
    run.wait_for_screen(run.n("c-nopd-hex").as_str(), "HEXECHO READY");
    let log = run.file("nopd-log.jsonl");
    let call = || Call {
        shim: true,
        env: vec![
            (
                "TT_SHIM_STATUS_DROP",
                "terminal.prompt-delivery.v1".to_owned(),
            ),
            ("TT_SHIM_LOG", log.display().to_string()),
        ],
        ..Call::default()
    };
    for text in ["one line", "line one\nline two"] {
        let out = run.cli(
            &["prompt", "--target", run.n("c-nopd").as_str(), text],
            call(),
        );
        assert_eq!(out.status, 1, "{text:?}: {out:?}");
        assert!(
            out.stderr.contains("terminal.prompt-delivery.v1"),
            "{}",
            out.stderr
        );
    }
    assert!(
        !run.shim_log(&log)
            .iter()
            .any(|call| call.len() > 1 && call[0] == "terminal" && call[1] == "send"),
        "no terminal send without prompt delivery"
    );
    run.cli(
        &["send", "--target", run.n("c-nopd-hex").as_str(), "y"],
        call(),
    )
    .assert_success("send y");
    run.wait_for_screen(run.n("c-nopd-hex").as_str(), "HEX 79");
    run.cli(
        &["capture", "--target", run.n("c-nopd-hex").as_str()],
        call(),
    )
    .assert_success("capture");
    run.kill(run.n("c-nopd").as_str());
    run.kill(run.n("c-nopd-hex").as_str());
}

fn override_wins_over_tmux(run: &Run) {
    let out = run.json(
        &["self", "--format", "json"],
        Call {
            env: vec![
                ("TMUX_TOOLS_HOST", "orca".to_owned()),
                ("TMUX", "/nonexistent/tt-decoy,1,0".to_owned()),
            ],
            ..Call::default()
        },
    );
    assert_eq!(out["host"], "orca");
}

// --- Claude teams through spawn-agent (PRD-261108-0447-01 P10.6) ---------------------

/// The call options of the Claude-teams scenario. The Launcher in an Orca pane runs
/// with Orca's environment, not the run's, so its Team and the teammates' Pane records
/// live in the default state directory; the scenario's CLI calls use the same one.
fn real_state<'a>() -> Call<'a> {
    match original("XDG_STATE_HOME") {
        Some(dir) => Call {
            env: vec![("XDG_STATE_HOME", dir)],
            ..Call::default()
        },
        None => Call {
            remove: vec!["XDG_STATE_HOME"],
            ..Call::default()
        },
    }
}

/// Save one raw observation when [`EVIDENCE_SWITCH`] names a directory.
fn evidence(file: &str, body: &str) {
    if let Some(dir) = original(EVIDENCE_SWITCH) {
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(file), body);
    }
}

fn evidence_json(file: &str, value: &Value) {
    evidence(file, &serde_json::to_string_pretty(value).unwrap());
}

/// The `orca terminal list --json` row of `handle`.
fn terminal_row(run: &Run, handle: &str) -> Option<Value> {
    run.terminals()
        .into_iter()
        .find(|terminal| terminal["handle"] == handle)
}

/// `<tabId>:<leafId>` of `handle` (PRD-261107-0825-01 P9.10).
fn pane_key(terminal: &Value) -> String {
    format!(
        "{}:{}",
        terminal["tabId"].as_str().unwrap(),
        terminal["leafId"].as_str().unwrap()
    )
}

/// The `orca worktree ps --json` agent row whose `paneKey` is `key`.
fn agent_row(ps: &Value, key: &str) -> Option<Value> {
    ps["worktrees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|worktree| worktree["agents"].as_array())
        .flatten()
        .find(|row| row["paneKey"] == key)
        .cloned()
}

/// The main-agent state of a row: `mainAgent.state`, else the row's `state`.
fn main_state(row: &Value) -> Option<&str> {
    let main = &row["mainAgent"];
    if main.is_object() {
        main["state"].as_str().or_else(|| row["state"].as_str())
    } else {
        row["state"].as_str()
    }
}

/// The Host state PRD-261107-0825-01 P9.10 gives for an agent row.
fn expected_host_state(row: Option<&Value>) -> Value {
    let Some(row) = row else {
        return Value::Null;
    };
    if row["hasLivePty"] == json!(false) {
        return Value::Null;
    }
    let Some(state) = main_state(row) else {
        return Value::Null;
    };
    let subagent_working = row["subagents"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|subagent| subagent["state"] == "working");
    if subagent_working {
        return json!("working");
    }
    match state {
        "working" => json!("working"),
        "blocked" | "waiting" => json!("blocked"),
        "done" => json!("done"),
        _ => Value::Null,
    }
}

/// Whether PRD-261107-0825-01 P9.6 guarantees delivery to a pane registered with agent
/// `claude`, from `terminal show` and its agent row (the capability is checked once).
fn expected_guaranteed(show: &Value, row: Option<&Value>) -> bool {
    show["agentIdentity"] == "claude"
        && row.is_some_and(|row| {
            row["hasLivePty"] == json!(true) && !expected_host_state(Some(row)).is_null()
        })
}

/// The `mainAgent` of the agent row whose `paneKey` is `key` (`null` without a row or
/// a `mainAgent`). A row's `subagents[]` is not its main agent's status.
fn main_agent(ps: &Value, key: &str) -> Value {
    agent_row(ps, key).map_or(Value::Null, |row| row["mainAgent"].clone())
}

/// P10.6: a transition of the teammate's own main agent shows on the teammate row and
/// not on the Leader row. `before` is a settled `orca worktree ps` answer, `after` the
/// first answer in which the teammate row's `mainAgent` changed.
fn judge_teammate_transition(
    before: &Value,
    after: &Value,
    lead_key: &str,
    tm_key: &str,
) -> Result<(), String> {
    let (tm_before, tm_after) = (main_agent(before, tm_key), main_agent(after, tm_key));
    let (lead_before, lead_after) = (main_agent(before, lead_key), main_agent(after, lead_key));
    let started = |main: &Value| main["stateStartedAt"].as_u64();
    let tm_moved = tm_after.is_object()
        && (started(&tm_after) > started(&tm_before) || tm_after["state"] != tm_before["state"]);
    if !tm_moved {
        return Err(format!(
            "the teammate row's mainAgent did not take the transition: {tm_before} -> {tm_after}"
        ));
    }
    if started(&lead_after) != started(&lead_before) || lead_after["state"] != lead_before["state"]
    {
        return Err(format!(
            "the Leader row's mainAgent took the teammate's transition: {lead_before} -> \
             {lead_after} (teammate {tm_before} -> {tm_after})"
        ));
    }
    Ok(())
}

/// `orca worktree ps --json` through `real_orca`, reading `result` (usable off the
/// test thread, where a [`Run`] is not).
fn worktree_ps(real_orca: &Path, extra_env: &[(String, String)]) -> Option<Value> {
    let out = Command::new(real_orca)
        .args(["worktree", "ps", "--json"])
        .envs(extra_env.iter().map(|(key, value)| (key, value)))
        .output()
        .ok()?;
    let parsed: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(parsed["result"].clone())
}

/// The Orca observation of `handle` that P8.5 compares with: `terminal show` and the
/// pane's agent row.
fn observe(run: &Run, handle: &str) -> (Value, Option<Value>) {
    let show = run.orca(&["terminal", "show", "--terminal", handle])["terminal"].clone();
    let key = pane_key(&show);
    let row = agent_row(&run.orca(&["worktree", "ps"]), &key);
    (show, row)
}

/// Run `check` between two Orca observations of `handle` until both observations give
/// the same expected value, then return that value with the check's result.
fn settled<T>(
    run: &Run,
    handle: &str,
    expected: impl Fn(&Value, Option<&Value>) -> Value,
    mut check: impl FnMut() -> T,
) -> (Value, T, Value) {
    for _ in 0..8 {
        let (show, row) = observe(run, handle);
        let before = expected(&show, row.as_ref());
        let result = check();
        let (show, row) = observe(run, handle);
        let after = expected(&show, row.as_ref());
        if before == after {
            return (before, result, json!({"show": show, "row": row}));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    panic!("{handle}: the Orca observation never held still across a check");
}

fn real_rows(run: &Run) -> Vec<Value> {
    run.json(&["list", "--format", "json"], real_state())
        .as_array()
        .unwrap()
        .clone()
}

fn real_row(run: &Run, name: &str) -> Option<Value> {
    real_rows(run).into_iter().find(|row| row["name"] == name)
}

fn poll<T>(what: &str, timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "{what}: not within {timeout:?}");
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn real_screen(run: &Run, target: &str) -> String {
    let out = run.cli(&["capture", "--target", target], real_state());
    out.assert_success(&format!("capture {target}"));
    out.stdout
}

/// Spawn `claude` as Leader `name` through `spawn-agent` with hooks off (harness
/// P9.6), and wait until its TUI is up. Every pane later opened in the Leader's tab is
/// recorded as the run's (a teammate is a split of the Leader).
fn spawn_leader(run: &Run, name: &str) -> String {
    let args = [
        "spawn-agent",
        "claude",
        "--access",
        "full-access",
        "--name",
        name,
        "--format",
        "json",
        "--",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ];
    let out = run.cli_creating(&args, real_state());
    out.assert_success(&format!("{args:?}"));
    let spawned: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    evidence_json(&format!("{name}-spawn.json"), &spawned);
    let handle = spawned["pane_id"].as_str().unwrap().to_owned();
    run.track(&handle);
    let terminals = run.terminals();
    let tab = terminals
        .iter()
        .find(|terminal| terminal["handle"] == handle.as_str())
        .and_then(|terminal| terminal["tabId"].as_str().map(str::to_owned))
        .expect("the Leader pane is listed");
    let before = terminals
        .iter()
        .filter(|terminal| terminal["tabId"] == tab.as_str())
        .filter_map(|terminal| terminal["handle"].as_str().map(str::to_owned))
        .collect();
    run.split_tabs.borrow_mut().push((tab, before));
    poll(
        &format!("{name}: Claude's TUI"),
        Duration::from_secs(120),
        || {
            let screen = real_screen(run, name);
            screen.contains("bypass permissions").then_some(())
        },
    );
    std::thread::sleep(Duration::from_secs(3));
    handle
}

/// Prompt Leader `name` to start one teammate that runs `task` with its Bash tool,
/// and wait for teammate `<name>.tm1`; returns its `list --format json` row.
fn start_teammate(run: &Run, name: &str, task: &str) -> Value {
    let text = format!(
        "Create an agent team now with exactly one teammate. Give the teammate this task: \
         run the shell command `{task}` with your Bash tool, then reply DONE and wait for \
         further messages. Do not run that command yourself and do nothing else."
    );
    let out = run.cli(
        &[
            "prompt",
            "--target",
            name,
            "--timeout",
            "30",
            "--format",
            "json",
            &text,
        ],
        real_state(),
    );
    evidence(&format!("{name}-spawn-prompt.txt"), &format!("{out:?}"));
    assert!(
        !out.stdout.contains("\"refused\""),
        "a single-line prompt is never refused: {out:?}"
    );
    let teammate = format!("{name}.tm1");
    let row = poll(
        &format!("{teammate} in tmux-tools list"),
        Duration::from_secs(240),
        || real_row(run, &teammate),
    );
    if let Some(handle) = row["pane"].as_str() {
        run.track(handle);
    }
    row
}

/// Send `/exit` to Leader `name` only when its input box is empty.
fn clean_exit(run: &Run, name: &str) {
    let screen = poll(
        &format!("{name}: an empty input box"),
        Duration::from_secs(180),
        || {
            let screen = real_screen(run, name);
            let input = screen
                .lines()
                .rev()
                .find(|line| line.trim_start().starts_with('❯'))?;
            (input.trim_start().trim_start_matches('❯').trim().is_empty()).then_some(screen)
        },
    );
    evidence(&format!("{name}-before-exit.txt"), &screen);
    run.cli(
        &["send", "--target", name, "--literal", "/exit"],
        real_state(),
    )
    .assert_success("type /exit");
    std::thread::sleep(Duration::from_millis(700));
    run.cli(&["send-enter", "--target", name], real_state())
        .assert_success("submit /exit");
}

/// The `team.json` of the Team whose Leader is the pane `handle`, from the default
/// state directory the Launcher uses.
fn leader_team(handle: &str) -> Value {
    let state = original("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(original("HOME").unwrap()).join(".local/state"));
    std::fs::read_dir(state.join("tmux-tools/teams"))
        .expect("the teams directory")
        .flatten()
        .filter_map(|entry| std::fs::read(entry.path().join("team.json")).ok())
        .filter_map(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .find(|team| team["leader"]["handle"] == handle)
        .unwrap_or_else(|| panic!("no team has Leader {handle}"))
}

fn claude_teams_through_spawn_agent(run: &Run) {
    let capabilities = run.orca(&["status"])["runtime"]["capabilities"].clone();
    assert!(
        capabilities
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "terminal.prompt-delivery.v1"),
        "this run needs terminal.prompt-delivery.v1"
    );
    let lead = run.n("lead");
    let lead_handle = spawn_leader(run, &lead);
    let lead_row = real_row(run, &lead).expect("lead is listed");
    assert_eq!(lead_row["agent"], "claude", "{lead_row}");

    // Teammate pane environment (P6.5).
    let env_file = run.file("teammate-env.txt");
    let task = format!(
        "printf '%s\\n' \"$ORCA_PANE_KEY\" \"$ORCA_TERMINAL_HANDLE\" \"$ORCA_TAB_ID\" > {}",
        env_file.display()
    );
    let teammate = format!("{lead}.tm1");
    let tm_row = start_teammate(run, &lead, &task);
    evidence_json("teammate-list-row.json", &tm_row);
    assert!(tm_row["team"].is_string(), "{tm_row}");
    assert_eq!(tm_row["parent"], lead.as_str(), "{tm_row}");
    assert_eq!(tm_row["orphan"], false, "{tm_row}");
    let tm_handle = tm_row["pane"].as_str().unwrap().to_owned();
    assert_ne!(tm_handle, lead_handle);

    let tm_terminal = poll(
        &format!("{teammate}: agentIdentity claude in orca terminal list"),
        Duration::from_secs(120),
        || terminal_row(run, &tm_handle).filter(|terminal| terminal["agentIdentity"] == "claude"),
    );
    evidence_json("teammate-terminal-row.json", &tm_terminal);
    let lead_terminal = terminal_row(run, &lead_handle).expect("lead is listed by orca");
    evidence_json("lead-terminal-row.json", &lead_terminal);

    let values: Vec<String> = poll(
        &format!("{teammate}: its printed environment"),
        Duration::from_secs(240),
        || {
            let text = std::fs::read_to_string(&env_file).ok()?;
            let lines: Vec<String> = text.lines().map(str::to_owned).collect();
            (lines.len() == 3).then_some(lines)
        },
    );
    evidence("teammate-env.txt", &values.join("\n"));
    assert_eq!(values[0], pane_key(&tm_terminal), "ORCA_PANE_KEY");
    assert_eq!(values[1], tm_handle, "ORCA_TERMINAL_HANDLE");
    assert_eq!(
        values[2],
        tm_terminal["tabId"].as_str().unwrap(),
        "ORCA_TAB_ID"
    );
    assert_ne!(
        values[0],
        pane_key(&lead_terminal),
        "ORCA_PANE_KEY is not the Leader's"
    );
    assert_ne!(
        values[1], lead_handle,
        "ORCA_TERMINAL_HANDLE is not the Leader's"
    );

    // Orca shows the teammate's status on the teammate's own pane (P6.5).
    let tm_key = pane_key(&tm_terminal);
    let lead_key = pane_key(&lead_terminal);
    let ps = poll(
        &format!("{teammate}: its own agent row"),
        Duration::from_secs(120),
        || {
            let ps = run.orca(&["worktree", "ps"]);
            agent_row(&ps, &tm_key)
                .filter(|row| !expected_host_state(Some(row)).is_null())
                .map(|_| ps)
        },
    );
    evidence_json("worktree-ps-with-teammate.json", &ps);
    let rows_with_tm_key = ps["worktrees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|worktree| worktree["agents"].as_array())
        .flatten()
        .filter(|row| row["paneKey"] == tm_key.as_str())
        .count();
    assert_eq!(
        rows_with_tm_key, 1,
        "one agent row carries the teammate's pane key"
    );
    // A transition of the teammate's own main agent, driven by a prompt into the
    // teammate pane (P8.4) once both main agents have settled, shows on the teammate
    // row and not on the Leader row (P10.6). The Leader row's `subagents[]` entry for
    // the teammate is not the teammate's status and is not compared.
    let mut stable = 0;
    let mut last: Option<(Value, Value)> = None;
    let mut seen = Vec::new();
    let settled_ps = poll(
        &format!("{lead} and {teammate}: settled main agents"),
        Duration::from_secs(240),
        || {
            let ps = run.orca(&["worktree", "ps"]);
            let mains = (main_agent(&ps, &lead_key), main_agent(&ps, &tm_key));
            seen.push(json!({"at": now_ms(), "lead": mains.0, "teammate": mains.1}));
            evidence_json("teammate-transition-settle.json", &json!(seen));
            // While a teammate lives, Orca can keep the Leader's main agent `working`
            // with an unchanged `stateStartedAt` (live run): settled means unchanged.
            let idle = mains.1["state"].is_string() && mains.1["state"] != "working";
            stable = if idle && last.as_ref() == Some(&mains) {
                stable + 1
            } else {
                0
            };
            last = Some(mains);
            std::thread::sleep(Duration::from_secs(1));
            (stable >= 3).then_some(ps)
        },
    );
    evidence_json("teammate-transition-before.json", &settled_ps);
    let tm_before = main_agent(&settled_ps, &tm_key);
    let watcher = {
        let (real_orca, extra_env, tm_key) =
            (run.real_orca.clone(), run.extra_env.clone(), tm_key.clone());
        std::thread::spawn(move || {
            // The first answer with a changed teammate main agent, and the first one
            // after it with the teammate's turn over (a Leader that stays `working`
            // could hide a `working` taken from the teammate, never its `done`).
            let mut first = None;
            let deadline = Instant::now() + Duration::from_secs(120);
            while Instant::now() < deadline {
                if let Some(ps) = worktree_ps(&real_orca, &extra_env) {
                    let main = main_agent(&ps, &tm_key);
                    if main != tm_before {
                        if main["state"] != "working" {
                            return (first.or_else(|| Some(ps.clone())), Some(ps));
                        }
                        first.get_or_insert(ps);
                    }
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            (first, None)
        })
    };
    let tm_prompt = run.cli(
        &[
            "prompt",
            "--target",
            &teammate,
            "--timeout",
            "60",
            "--format",
            "json",
            "Reply with the single word PING. Do nothing else.",
        ],
        real_state(),
    );
    evidence("teammate-transition-prompt.txt", &format!("{tm_prompt:?}"));
    let (first_ps, over_ps) = watcher.join().unwrap();
    let first_ps = first_ps.expect("the teammate row's mainAgent changed after the prompt");
    evidence_json("teammate-transition-after.json", &first_ps);
    let over_ps = over_ps.expect("the teammate row's mainAgent left `working` after the prompt");
    evidence_json("teammate-transition-over.json", &over_ps);
    for ps in [&first_ps, &over_ps] {
        if let Err(problem) = judge_teammate_transition(&settled_ps, ps, &lead_key, &tm_key) {
            panic!("{problem}");
        }
    }

    // P8.5: host_state of the Leader in list and wait-idle, and a multi-line prompt.
    let (expected, listed, observed) = settled(
        run,
        &lead_handle,
        |_, row| expected_host_state(row),
        || real_row(run, &lead).expect("lead is listed")["host_state"].clone(),
    );
    evidence_json(
        "lead-host-state-list.json",
        &json!({"expected": expected, "list": listed, "orca": observed}),
    );
    assert_eq!(listed, expected, "list host_state of {lead}");
    let (expected, waited, observed) = settled(
        run,
        &lead_handle,
        |_, row| expected_host_state(row),
        || {
            let out = run.cli(
                &[
                    "wait-idle",
                    "--target",
                    &lead,
                    "--timeout",
                    "5",
                    "--format",
                    "json",
                ],
                real_state(),
            );
            serde_json::from_str::<Value>(out.stdout.trim())
                .unwrap_or_else(|_| panic!("wait-idle --format json: {out:?}"))
        },
    );
    evidence_json(
        "lead-host-state-wait-idle.json",
        &json!({"expected": expected, "wait_idle": waited, "orca": observed}),
    );
    assert_eq!(
        waited["host_state"], expected,
        "wait-idle host_state of {lead}"
    );

    // wait-idle on the Leader ends by --until whatever its Host state is (P8.5).
    let until = run.cli(
        &[
            "wait-idle",
            "--target",
            &lead,
            "--until",
            "bypass permissions",
            "--timeout",
            "60",
            "--format",
            "json",
        ],
        real_state(),
    );
    evidence("lead-wait-idle-until.txt", &format!("{until:?}"));
    until.assert_success("wait-idle --until on the Leader");

    // Exactly one multi-line prompt, judged by the P9.6 determination from the Orca
    // observation before it; when the observation after it differs, a result that
    // matches either determination is accepted. Never resent.
    let (show, row) = observe(run, &lead_handle);
    let guaranteed_before = expected_guaranteed(&show, row.as_ref());
    let observed_before = json!({"show": show, "row": row});
    let prompted = run.cli(
        &[
            "prompt",
            "--target",
            &lead,
            "--timeout",
            "20",
            "--format",
            "json",
            "Reply with the single word PONG.\nDo nothing else.",
        ],
        real_state(),
    );
    let (show, row) = observe(run, &lead_handle);
    let guaranteed_after = expected_guaranteed(&show, row.as_ref());
    let observed_after = json!({"show": show, "row": row});
    evidence(
        "lead-multi-line-prompt.txt",
        &format!(
            "guaranteed_before={guaranteed_before}\nguaranteed_after={guaranteed_after}\n\
             orca_before={observed_before}\norca_after={observed_after}\n{prompted:?}"
        ),
    );
    let result: Value = serde_json::from_str(prompted.stdout.trim())
        .unwrap_or_else(|_| panic!("prompt --format json: {prompted:?}"));
    // Guaranteed: sent as one submission. Not guaranteed: the Surface `rich` declares
    // bracketed paste, so it is refused before any transmission.
    let matches = |guaranteed: bool| {
        if guaranteed {
            result["one_submission_delivery"] == "guaranteed"
        } else {
            prompted.status == 1 && result["one_submission_delivery"] == "refused"
        }
    };
    if guaranteed_before == guaranteed_after {
        assert!(
            matches(guaranteed_before),
            "guaranteed={guaranteed_before}: {prompted:?}"
        );
    } else {
        assert!(
            matches(guaranteed_before) || matches(guaranteed_after),
            "guaranteed before={guaranteed_before} after={guaranteed_after}: {prompted:?}"
        );
    }

    // A clean Leader exit closes the teammate (P10.6, Verification item 3).
    clean_exit(run, &lead);
    let mut confirmed = false;
    poll(
        &format!("{teammate}: closed by the clean exit"),
        Duration::from_secs(60),
        || {
            let screen = run.cli(&["capture", "--target", &lead], real_state());
            evidence(&format!("{lead}-after-exit.txt"), &screen.stdout);
            // Claude 2.1.319 asks before it exits with a live teammate; its first
            // choice, selected by default, is "Exit and stop tasks".
            if !confirmed && screen.stdout.contains("1. Exit and stop tasks") {
                evidence(&format!("{lead}-exit-dialog.txt"), &screen.stdout);
                run.cli(&["send-enter", "--target", &lead], real_state())
                    .assert_success("confirm the exit");
                confirmed = true;
            }
            (!run.orca_handles().contains(&tm_handle)).then_some(())
        },
    );
    poll(
        &format!("{teammate}: gone from tmux-tools list"),
        Duration::from_secs(30),
        || real_row(run, &teammate).is_none().then_some(()),
    );
    run.cli(&["kill", "--target", &lead], real_state())
        .assert_success("kill the Leader");
    assert!(!run.orca_handles().contains(&lead_handle));

    // A killed Leader leaves an Orphan that list reports, and kill closes it.
    let crash = run.n("crash");
    let crash_handle = spawn_leader(run, &crash);
    let orphan = format!("{crash}.tm1");
    let orphan_row = start_teammate(run, &crash, "true");
    let orphan_handle = orphan_row["pane"].as_str().unwrap().to_owned();
    // The crash: the Leader's Claude dies by SIGKILL, so it closes nothing, and the
    // Launcher ends with a failed exit.
    let team = leader_team(&crash_handle);
    evidence_json("crash-team.json", &team);
    let launcher = team["launcher_pid"].as_u64().unwrap().to_string();
    let children = Command::new("pgrep")
        .args(["-P", &launcher])
        .output()
        .expect("pgrep");
    let claude_pid = String::from_utf8_lossy(&children.stdout)
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("the Launcher {launcher} has no child"))
        .to_owned();
    let killed = Command::new("kill")
        .args(["-KILL", &claude_pid])
        .status()
        .expect("kill");
    assert!(killed.success(), "kill -KILL {claude_pid}");
    let row = poll(
        &format!("{orphan}: orphan true"),
        Duration::from_secs(30),
        || {
            let rows = real_rows(run);
            evidence_json("orphan-poll-list.json", &json!(rows));
            rows.into_iter()
                .find(|row| row["name"] == orphan.as_str())
                .filter(|row| row["orphan"] == true)
        },
    );
    evidence_json("orphan-list-row.json", &row);
    let table = run.cli(&["list"], real_state());
    evidence("orphan-list-table.txt", &table.stdout);
    assert!(
        table.stdout.contains(&format!("orphan:{crash}")),
        "{}",
        table.stdout
    );
    assert!(
        run.orca_handles().contains(&orphan_handle),
        "the Orphan pane stays"
    );
    run.cli(&["kill", "--target", &orphan], real_state())
        .assert_success("kill the Orphan");
    assert!(!run.orca_handles().contains(&orphan_handle));
    run.cli(&["kill", "--target", &crash], real_state())
        .assert_success("kill the crashed Leader's pane");
    assert!(!run.orca_handles().contains(&crash_handle));

    // No pane the scenario created is left in either list.
    let left: Vec<Value> = real_rows(run)
        .into_iter()
        .filter(|row| {
            row["name"]
                .as_str()
                .is_some_and(|name| name.contains(&run.tag))
        })
        .collect();
    assert!(left.is_empty(), "tmux-tools list shows: {left:?}");
    let listed = run.orca_handles();
    for handle in [&lead_handle, &tm_handle, &crash_handle, &orphan_handle] {
        assert!(
            !listed.contains(handle),
            "orca terminal list shows {handle}"
        );
    }
}

// --- The live run's own cleanup, against the fake `orca` CLI (default run) ----------

/// A [`Run`] whose `orca` is `fake_orca.py`, and the fake's pane state.
struct FakeCli {
    dir: PathBuf,
}

impl FakeCli {
    fn start(label: &str) -> FakeCli {
        let (dir, _) = unique_dir(&format!("tt-orca-fake-{label}"));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        write_executable(&dir.join("bin/orca"), include_str!("fake_orca.py"));
        FakeCli { dir }
    }

    fn run(&self) -> Run {
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Run::with_orca(
            self.dir.join("bin/orca"),
            vec![
                ("PATH".to_owned(), path),
                (
                    "FAKE_ORCA_STATE".to_owned(),
                    self.dir.join("state.json").display().to_string(),
                ),
                ("ORCA_TERMINAL_HANDLE".to_owned(), "term_caller".to_owned()),
                ("ORCA_WORKTREE_ID".to_owned(), "fake-worktree".to_owned()),
            ],
            Duration::from_secs(5),
        )
    }

    fn state(&self) -> Value {
        std::fs::read(self.dir.join("state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(json!({"terminals": []}))
    }

    /// `(handle, paneTitle)` of every open fake pane.
    fn open(&self) -> Vec<(String, String)> {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|terminal| {
                (
                    terminal["handle"].as_str().unwrap().to_owned(),
                    terminal["paneTitle"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                )
            })
            .collect()
    }
}

impl Drop for FakeCli {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// P10.4: a pane another session opens in the run's worktree while the run creates
/// one is not the run's; its cleanup closes only its own pane.
#[test]
fn live_run_cleanup_spares_a_pane_opened_meanwhile_by_someone_else() {
    let fake = FakeCli::start("foreign");
    let run = fake.run();
    let name = run.n("c-shell");
    let handle = run.launch_with(
        &name,
        Call {
            // Same base name, without the run's tag.
            env: vec![("FAKE_ORCA_FOREIGN_ON_CREATE", "c-shell".to_owned())],
            ..Call::default()
        },
    );
    assert_eq!(fake.open().len(), 2, "{:?}", fake.open());

    run.close_created();
    let open = fake.open();
    assert_eq!(open.len(), 1, "only the run's own pane is closed: {open:?}");
    assert_ne!(open[0].0, handle);
    assert_eq!(
        open[0].1, "c-shell",
        "the unrelated pane survives: {open:?}"
    );
}

/// P10.4: a pane that opens only after its creation timed out ambiguously still
/// carries the run's name, and the run's final reconciliation closes it.
#[test]
fn live_run_cleanup_closes_its_pane_that_opens_after_an_ambiguous_timeout() {
    let fake = FakeCli::start("late");
    let run = fake.run();
    let name = run.n("c-late");
    let out = run.cli_creating(
        &["launch", "--name", &name, "--cmd", "sleep 30"],
        Call {
            env: vec![("FAKE_ORCA_CREATE_LATE", "8".to_owned())],
            ..Call::default()
        },
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("spawn outcome unknown"),
        "{}",
        out.stderr
    );
    assert!(fake.open().is_empty(), "the pane has not opened yet");
    assert!(!fake.state()["late"].is_null(), "the pane is still pending");

    run.close_created();
    assert!(fake.state()["late"].is_null(), "the late pane opened");
    assert!(
        fake.open().is_empty(),
        "the run closed its late pane: {:?}",
        fake.open()
    );
}

/// P10.4: a split pane gets no title, so one that opens only after its split timed
/// out ambiguously is the run's by the tab of the run-owned pane it split; the run's
/// final reconciliation closes it and spares an unrelated pane of the worktree.
#[test]
fn live_run_cleanup_closes_its_split_pane_that_opens_after_an_ambiguous_timeout() {
    let fake = FakeCli::start("late-split");
    let run = fake.run();
    let other = run.orca(&[
        "terminal",
        "create",
        "--worktree",
        "id:fake-worktree",
        "--title",
        "someone-else",
    ])["terminal"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let anchor = run.launch(&run.n("c-anchor"), "sleep 30");
    let out = run.cli_creating(
        &[
            "launch",
            "--name",
            &run.n("c-late-split"),
            "--cmd",
            "sleep 30",
            "--split",
            "h",
            "--target",
            &anchor,
        ],
        Call {
            env: vec![("FAKE_ORCA_SPLIT_LATE", "8".to_owned())],
            ..Call::default()
        },
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("spawn outcome unknown"),
        "{}",
        out.stderr
    );
    assert!(
        !fake.state()["late"].is_null(),
        "the split pane is still pending"
    );

    run.close_created();
    assert!(fake.state()["late"].is_null(), "the late split pane opened");
    let open = fake.open();
    assert_eq!(
        open.iter()
            .map(|(handle, _)| handle.clone())
            .collect::<Vec<_>>(),
        vec![other],
        "the run closed its anchor and its late split pane, and only those: {open:?}"
    );
}

/// P10.4: Runs created at the same moment never share a directory or a tag, and one
/// Run's teardown leaves the other's Pane records, shim receipts and panes intact.
#[test]
fn concurrent_runs_never_share_their_private_state() {
    let fake = FakeCli::start("concurrent");
    let barrier = std::sync::Barrier::new(2);
    let (a, b) = std::thread::scope(|scope| {
        let start = || {
            barrier.wait();
            fake.run()
        };
        let a = scope.spawn(start);
        let b = scope.spawn(start);
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(a.dir, b.dir, "two Runs share a directory");
    assert_ne!(a.tag, b.tag, "two Runs share a tag");
    let shimmed = || Call {
        shim: true,
        ..Call::default()
    };
    let a_handle = a.launch_with(&a.n("c-own"), shimmed());
    let b_name = b.n("c-own");
    let b_handle = b.launch_with(&b_name, shimmed());
    let b_record = b.record_path(&b_name);
    let b_receipts = b.file("shim-created.jsonl");
    assert!(b_record.exists());
    assert!(std::fs::read_to_string(&b_receipts)
        .unwrap()
        .contains(&b_handle));

    drop(a);
    assert!(b_record.exists(), "the other Run's Pane record survives");
    assert!(
        std::fs::read_to_string(&b_receipts)
            .unwrap_or_default()
            .contains(&b_handle),
        "the other Run's shim receipt survives"
    );
    let open: Vec<String> = fake.open().into_iter().map(|(handle, _)| handle).collect();
    assert_eq!(
        open,
        vec![b_handle],
        "only the dropped Run's pane is closed"
    );
    assert!(!open.contains(&a_handle));

    drop(b);
    assert!(fake.open().is_empty(), "{:?}", fake.open());
}

/// P10.6: the live check of the teammate's own status fails when the Leader row's
/// main agent takes the teammate's transition (rows shaped as live `worktree ps`).
#[test]
fn teammate_transition_check_fails_when_the_leader_row_takes_it() {
    let (lead, tm) = ("tab:lead-leaf", "tab:tm-leaf");
    let row = |key: &str, state: &str, at: u64| {
        json!({
            "agentType": "claude",
            "hasLivePty": true,
            "paneKey": key,
            "mainAgent": {"state": state, "stateStartedAt": at},
            "state": state,
            "subagents": if key == lead {
                json!([{"agentType": "claude", "state": "working", "startedAt": 50}])
            } else {
                json!(null)
            },
        })
    };
    let ps = |rows: Vec<Value>| json!({"worktrees": [{"agents": rows}]});
    let before = ps(vec![row(lead, "done", 100), row(tm, "done", 200)]);
    let on_teammate = ps(vec![row(lead, "done", 100), row(tm, "working", 300)]);
    assert_eq!(
        judge_teammate_transition(&before, &on_teammate, lead, tm),
        Ok(())
    );
    // The transition attributed to the Leader: its keys swapped with the teammate's.
    assert!(judge_teammate_transition(&before, &on_teammate, tm, lead).is_err());
    let on_both = ps(vec![row(lead, "working", 300), row(tm, "working", 300)]);
    assert!(judge_teammate_transition(&before, &on_both, lead, tm).is_err());
    let on_leader = ps(vec![row(lead, "working", 300), row(tm, "done", 200)]);
    assert!(judge_teammate_transition(&before, &on_leader, lead, tm).is_err());
}
