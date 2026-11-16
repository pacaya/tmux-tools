//! `tmux-tools claude-teams`, the `tmux` Shim, and the `parent`/`team`/`orphan` output
//! (PRD-261108-0447-01), through the CLI on the isolated tmux test server and with a
//! fake `orca` CLI on `PATH`.
//!
//! A fake `claude` script stands in for Claude: started by the Launcher, it records
//! its arguments and environment, replays a trace fixture through the Shim
//! (`tests/fixtures/claude-teams/<version key>.jsonl`, P10.2) or runs a scripted
//! sequence, can pause until the test lets it go, and exits with a chosen status.
//!
//! The opt-in live test (P10.3) runs the real `claude` with `TMUX_TOOLS_LIVE_CLAUDE=1`.

#[path = "harness/mod.rs"]
mod harness;

use harness::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/claude-teams");
const FAKE_ORCA: &str = include_str!("contract/fake_orca.py");

/// The fake `claude`. Modes (environment):
/// - `--version` prints `FAKE_CLAUDE_VERSION` (default `2.1.319 (Claude Code)`).
/// - With `--agent-id` or `--agent-name` it is a teammate: it sleeps.
/// - Started without the Launcher (no `TT_TEAM`) with `FAKE_CLAUDE_NO_TEAM_SLEEP=1`,
///   it sleeps.
/// - Else it is the Leader: it waits until the file `FAKE_CLAUDE_WAIT_FOR` exists
///   (when set), writes `<report>.args`/`.env`, runs `TT_BIN self`
///   into `<report>.self` when `FAKE_CLAUDE_SELF=1`, replays `FAKE_CLAUDE_FIXTURE`
///   (skipping `kill-pane` with `FAKE_CLAUDE_SKIP_KILL=1`, or pausing before the first
///   `kill-pane` with `FAKE_CLAUDE_PAUSE_BEFORE_KILL=1`) or runs
///   `FAKE_CLAUDE_SCRIPT=nospace`, pauses (`<report>.paused`, until `<report>.go`)
///   when `FAKE_CLAUDE_PAUSE=1`, writes `<report>` and exits `FAKE_CLAUDE_EXIT`.
const FAKE_CLAUDE: &str = r##"#!/usr/bin/env python3
import json, os, subprocess, sys, time
args = sys.argv[1:]
if args[:1] == ["--version"]:
    print(os.environ.get("FAKE_CLAUDE_VERSION", "2.1.319 (Claude Code)"))
    sys.exit(0)
if "--agent-id" in args or "--agent-name" in args:
    time.sleep(3600)
    sys.exit(0)
if "TT_TEAM" not in os.environ and os.environ.get("FAKE_CLAUDE_NO_TEAM_SLEEP") == "1":
    time.sleep(3600)
    sys.exit(0)
wait_for = os.environ.get("FAKE_CLAUDE_WAIT_FOR")
if wait_for:
    deadline = time.time() + 300
    while not os.path.exists(wait_for) and time.time() < deadline:
        time.sleep(0.05)
report = os.environ["FAKE_CLAUDE_REPORT"]
json.dump(args, open(report + ".args", "w"))
json.dump({k: os.environ.get(k) for k in
           ["TT_TEAM", "TT_HOST", "TMUX", "TMUX_PANE", "PATH"]}, open(report + ".env", "w"))
if os.environ.get("FAKE_CLAUDE_SELF") == "1":
    out = subprocess.run([os.environ["TT_BIN"], "self", "--format", "json"],
                         capture_output=True, text=True)
    open(report + ".self", "w").write(out.stdout + out.stderr)

def pause():
    open(report + ".paused", "w").write("1")
    deadline = time.time() + 300
    while not os.path.exists(report + ".go") and time.time() < deadline:
        time.sleep(0.05)

def tmux(argv):
    p = subprocess.run(["tmux"] + argv, capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr

result = {"calls": 0, "mismatches": []}
fixture = os.environ.get("FAKE_CLAUDE_FIXTURE")
if fixture:
    team, cwd, claude = os.environ["TT_TEAM"], os.getcwd(), os.path.abspath(sys.argv[0])
    def fill(s):
        return s.replace("{{team_id}}", team).replace("{{cwd}}", cwd).replace("{{claude}}", claude)
    for line in open(fixture):
        if not line.strip():
            continue
        entry = json.loads(line)
        argv = [fill(a) for a in entry["argv"]]
        if "kill-pane" in argv and os.environ.get("FAKE_CLAUDE_SKIP_KILL") == "1":
            continue
        if ("kill-pane" in argv and os.environ.get("FAKE_CLAUDE_PAUSE_BEFORE_KILL") == "1"
                and not os.path.exists(report + ".paused")):
            json.dump(result, open(report + ".partial", "w"))
            pause()
        code, out, err = tmux(argv)
        result["calls"] += 1
        expected = fill(entry["stdout"])
        if code != entry["exit"] or out != expected:
            result["mismatches"].append({"argv": argv, "exit": code, "stdout": out,
                                         "stderr": err, "expected_exit": entry["exit"],
                                         "expected_stdout": expected})
if os.environ.get("FAKE_CLAUDE_SCRIPT") == "nospace":
    started = []
    failure = None
    for _ in range(200):
        code, out, err = tmux(["split-window", "-d", "-t", "%1", "-h", "-P", "-F",
                               "#{pane_id}", "--", "cat"])
        if code != 0:
            failure = {"exit": code, "stderr": err, "stdout": out}
            break
        started.append(out.strip())
    result["started"] = started
    result["failure"] = failure
    code, out, err = tmux(["respawn-pane", "-k", "-t", "%2", "--", "sleep 3600"])
    result["respawn"] = {"exit": code, "stdout": out, "stderr": err}
if os.environ.get("FAKE_CLAUDE_PAUSE") == "1":
    json.dump(result, open(report + ".partial", "w"))
    pause()
json.dump(result, open(report, "w"))
sys.exit(int(os.environ.get("FAKE_CLAUDE_EXIT", "0")))
"##;

/// A fresh empty directory under the harness temp base.
fn temp_dir(label: &str) -> PathBuf {
    let dir = harness_tmp_base().join(format!(
        "tt-ct-{label}-{}-{}",
        std::process::id(),
        NEXT_SERVER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// One test's state: its `XDG_STATE_HOME`, the fake `claude`, its report, and a work
/// directory (the Launcher's cwd).
struct Scene {
    dir: PathBuf,
}

impl Scene {
    fn new(label: &str) -> Scene {
        let dir = temp_dir(label);
        for sub in ["bin", "state", "work", "config"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/claude"), FAKE_CLAUDE);
        Scene { dir }
    }

    fn bin(&self) -> PathBuf {
        self.dir.join("bin")
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    fn work(&self) -> PathBuf {
        std::fs::canonicalize(self.dir.join("work")).unwrap()
    }

    fn report(&self) -> PathBuf {
        self.dir.join("report.json")
    }

    fn teams(&self) -> PathBuf {
        self.state().join("tmux-tools/teams")
    }

    /// The team directories that exist.
    fn team_dirs(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.teams())
            .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default()
    }

    fn base_env(&self) -> Vec<(String, String)> {
        vec![
            ("XDG_STATE_HOME".into(), self.state().display().to_string()),
            (
                "XDG_CONFIG_HOME".into(),
                self.dir.join("config").display().to_string(),
            ),
            (
                "FAKE_CLAUDE_REPORT".into(),
                self.report().display().to_string(),
            ),
            ("TT_BIN".into(), BIN.to_owned()),
        ]
    }

    /// Wait until the fake Leader paused.
    fn wait_paused(&self, child: &mut Child) {
        let marker = PathBuf::from(format!("{}.paused", self.report().display()));
        let deadline = Instant::now() + Duration::from_secs(120);
        while !marker.exists() {
            if let Ok(Some(status)) = child.try_wait() {
                panic!("claude-teams exited {status} before the fake claude paused");
            }
            assert!(Instant::now() < deadline, "the fake claude never paused");
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn go(&self) {
        std::fs::write(format!("{}.go", self.report().display()), "1").unwrap();
    }

    fn read_report(&self, suffix: &str) -> Value {
        let path = format!("{}{suffix}", self.report().display());
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!("read {path}: {e}");
        }))
        .unwrap()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Start `claude-teams` in the background from `caller_pane` of `server`.
fn start_launcher(
    server: &IsolatedTmuxServer,
    scene: &Scene,
    caller_pane: &str,
    env: &[(&str, &str)],
    args: &[&str],
) -> Child {
    let mut all: Vec<(String, String)> = scene.base_env();
    all.extend(env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    let pairs: Vec<(&str, &str)> = all.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let work = scene.work();
    let bin = scene.bin();
    let options = CliOptions {
        path_prefix: Some(&bin),
        caller_pane: Some(caller_pane),
        cwd: Some(&work),
        env: &pairs,
        ..CliOptions::default()
    };
    server
        .command_with_cli_env(BIN, &options)
        .arg("claude-teams")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn claude-teams")
}

/// Run the CLI on `server` with the scene's state directory.
fn cli(server: &IsolatedTmuxServer, scene: &Scene, args: &[&str]) -> CmdOutput {
    let state = scene.state().display().to_string();
    server.run_cli(
        args,
        CliOptions {
            env: &[("XDG_STATE_HOME", &state)],
            ..CliOptions::default()
        },
    )
}

/// Run the CLI from `pane` of `server` with the scene's state directory.
fn cli_from(server: &IsolatedTmuxServer, scene: &Scene, pane: &str, args: &[&str]) -> CmdOutput {
    let state = scene.state().display().to_string();
    let config = scene.dir.join("config").display().to_string();
    server.run_cli(
        args,
        CliOptions {
            env: &[("XDG_STATE_HOME", &state), ("XDG_CONFIG_HOME", &config)],
            caller_pane: Some(pane),
            ..CliOptions::default()
        },
    )
}

fn list_json(server: &IsolatedTmuxServer, scene: &Scene) -> Vec<Value> {
    let out = cli(server, scene, &["list", "--all", "--format", "json"]);
    out.assert_success("list --all --format json");
    serde_json::from_str::<Value>(&out.stdout)
        .unwrap_or_else(|e| panic!("list JSON: {e}: {}", out.stdout))
        .as_array()
        .unwrap()
        .clone()
}

fn row<'a>(rows: &'a [Value], name: &str) -> Option<&'a Value> {
    rows.iter().find(|row| row["name"] == name)
}

/// A tmux-tools pane `name` running `sleep`, made by `launch`; returns its pane id.
fn launch_pane(server: &IsolatedTmuxServer, name: &str) -> String {
    launch_pane_running(server, name, "sleep 3600")
}

fn launch_pane_running(server: &IsolatedTmuxServer, name: &str, command: &str) -> String {
    server
        .cli(&["launch", "--name", name, "--split", "v", "--cmd", command])
        .assert_success(&format!("launch {name}"));
    pane_id_by_name(server, name).unwrap_or_else(|| panic!("pane {name} exists"))
}

fn finish(child: Child) -> CmdOutput {
    wait_for_child(child, "claude-teams", Duration::from_secs(120))
}

/// Run the Shim (the binary started as `tmux`) inside the Team `team_dir` with `args`.
fn shim(
    team_dir: &Path,
    scene: &Scene,
    host: &str,
    extra: &[(&str, String)],
    args: &[&str],
) -> CmdOutput {
    let id = team_dir.file_name().unwrap().to_str().unwrap();
    let mut command = Command::new(team_dir.join("bin/tmux"));
    command
        .args(args)
        .env("TT_TEAM", id)
        .env("TT_HOST", host)
        .env("XDG_STATE_HOME", scene.state())
        .env_remove("TT_SHIM_DEBUG");
    for (key, value) in extra {
        command.env(key, value);
    }
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the Shim");
    wait_for_child(child, "shim", Duration::from_secs(60))
}

fn fixture(version_key: &str) -> PathBuf {
    Path::new(FIXTURE_DIR).join(format!("{version_key}.jsonl"))
}

/// The version key of every trace fixture in `FIXTURE_DIR`, read at test time and
/// sorted, so a new fixture file is replayed with no test change (P10.2).
fn fixture_keys() -> Vec<String> {
    let mut keys: Vec<String> = std::fs::read_dir(FIXTURE_DIR)
        .expect("read the fixture directory")
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    keys.sort();
    assert!(!keys.is_empty(), "no trace fixture in {FIXTURE_DIR}");
    keys
}

/// Run `replay` once per trace fixture; a failure names the fixture.
fn for_every_fixture(replay: fn(&str)) {
    for key in fixture_keys() {
        eprintln!("P10.2 replay of fixture {key}");
        if let Err(panic) = std::panic::catch_unwind(|| replay(&key)) {
            eprintln!("P10.2 replay of fixture {key} failed");
            std::panic::resume_unwind(panic);
        }
    }
}

/// The fake `claude --version` output for a fixture's version key.
fn fake_version(key: &str) -> String {
    format!("{key} (Claude Code)")
}

// --- P10.2: replay of the trace fixture ---------------------------------------------

/// Acceptance: replaying each trace fixture through the Shim inside a Team from pane
/// `lead`, every call's stdout and exit equal the filled fixture line, and `list`
/// shows `lead.tm1` with the team id, `parent: "lead"`, `shim_id: "%2"`,
/// `orphan: false` before the fixture's final `kill-pane`. Every fixture line, that
/// `kill-pane` included, is replayed and compared; after the clean exit no Teammate
/// pane is left and the team directory is gone (P2.5).
#[test]
fn the_fixture_replays_and_a_clean_exit_closes_the_team() {
    for_every_fixture(the_fixture_replays_and_a_clean_exit_closes_the_team_with);
}

fn the_fixture_replays_and_a_clean_exit_closes_the_team_with(key: &str) {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("replay");
    let lead = launch_pane(&server, "lead");
    let fixture = fixture(key);
    let fixture_path = fixture.display().to_string();

    let mut child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
            ("FAKE_CLAUDE_VERSION", &fake_version(key)),
            ("FAKE_CLAUDE_PAUSE_BEFORE_KILL", "1"),
            ("FAKE_CLAUDE_SELF", "1"),
        ],
        &[],
    );
    scene.wait_paused(&mut child);
    let partial = scene.read_report(".partial");
    assert!(partial["calls"].as_u64().unwrap() > 0, "{partial}");
    assert_eq!(partial["mismatches"], serde_json::json!([]), "{partial:#}");

    let teams = scene.team_dirs();
    assert_eq!(teams.len(), 1, "one team directory: {teams:?}");
    let team_id = teams[0].file_name().unwrap().to_str().unwrap().to_owned();

    let rows = list_json(&server, &scene);
    let tm1 = row(&rows, "lead.tm1").unwrap_or_else(|| panic!("lead.tm1 listed: {rows:#?}"));
    assert_eq!(tm1["team"], team_id.as_str(), "{tm1}");
    assert_eq!(tm1["parent"], "lead", "{tm1}");
    assert_eq!(tm1["shim_id"], "%2", "{tm1}");
    assert_eq!(tm1["orphan"], false, "{tm1}");
    assert_eq!(
        tm1["agent"], "claude",
        "the respawned teammate runs claude: {tm1}"
    );

    // P8.1: `self` in the Leader and in a Teammate pane.
    let leader_self: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}.self", scene.report().display())).unwrap(),
    )
    .unwrap();
    assert_eq!(leader_self["role"], "leader", "{leader_self}");
    assert_eq!(leader_self["team"], team_id.as_str(), "{leader_self}");
    assert_eq!(leader_self["name"], "lead", "{leader_self}");
    let tm1_pane = pane_id_by_name(&server, "lead.tm1").unwrap();
    let teammate_self = cli_from(&server, &scene, &tm1_pane, &["self", "--format", "json"]);
    teammate_self.assert_success("self in lead.tm1");
    let teammate_self: Value = serde_json::from_str(&teammate_self.stdout).unwrap();
    assert_eq!(teammate_self["role"], "teammate", "{teammate_self}");
    assert_eq!(teammate_self["team"], team_id.as_str(), "{teammate_self}");
    assert_eq!(teammate_self["parent"], "lead", "{teammate_self}");

    // P6.5: the teammate does not get the Leader's team environment.
    let env = server.tmux(&["show-environment", "-t", &tm1_pane, "TT_TEAM"]);
    assert_ne!(
        env.status, 0,
        "TT_TEAM is not in the teammate's environment: {env:?}"
    );

    scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "claude-teams exits 0: {out:?}");
    assert_eq!(out.stdout, "", "the Launcher adds no output");
    let report = scene.read_report("");
    let lines = std::fs::read_to_string(&fixture)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(
        report["calls"], lines as u64,
        "every fixture line replayed: {report}"
    );
    assert_eq!(report["mismatches"], serde_json::json!([]), "{report:#}");
    let rows = list_json(&server, &scene);
    assert!(
        rows.iter().all(|row| row["team"] != team_id.as_str()),
        "no Teammate pane of the Team is listed: {rows:#?}"
    );
    assert!(pane_id_by_name(&server, "lead.tm1").is_none());
    assert!(!teams[0].exists(), "the team directory is gone");
    assert!(
        pane_id_by_name(&server, "lead").is_some(),
        "the Leader pane stays"
    );
}

/// Acceptance (P2.5): when Claude exits 0 and leaves a Teammate pane open (the fixture
/// replayed without its `kill-pane`), the Launcher closes it and deletes the team
/// directory.
#[test]
fn a_clean_exit_closes_teammates_that_claude_left_open() {
    for_every_fixture(a_clean_exit_closes_teammates_that_claude_left_open_with);
}

fn a_clean_exit_closes_teammates_that_claude_left_open_with(key: &str) {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("leftopen");
    let lead = launch_pane(&server, "lead");
    let fixture_path = fixture(key).display().to_string();
    let child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
            ("FAKE_CLAUDE_VERSION", &fake_version(key)),
            ("FAKE_CLAUDE_SKIP_KILL", "1"),
        ],
        &[],
    );
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
    assert_eq!(scene.read_report("")["mismatches"], serde_json::json!([]));
    assert!(
        pane_id_by_name(&server, "lead.tm1").is_none(),
        "the Launcher closed lead.tm1"
    );
    assert!(scene.team_dirs().is_empty(), "the team directory is gone");
}

/// Acceptance: when Claude exits 1, the Teammate panes and the team directory stay,
/// `list` reports them as Orphans once the Launcher ended, and `kill --target lead.tm1`
/// closes the pane.
#[test]
fn a_failed_exit_keeps_teammates_as_orphans_that_kill_closes() {
    for_every_fixture(a_failed_exit_keeps_teammates_as_orphans_that_kill_closes_with);
}

fn a_failed_exit_keeps_teammates_as_orphans_that_kill_closes_with(key: &str) {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("crash");
    let lead = launch_pane(&server, "lead");
    let fixture_path = fixture(key).display().to_string();

    let child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
            ("FAKE_CLAUDE_VERSION", &fake_version(key)),
            ("FAKE_CLAUDE_SKIP_KILL", "1"),
            ("FAKE_CLAUDE_EXIT", "1"),
        ],
        &[],
    );
    let out = finish(child);
    assert_eq!(
        out.status, 1,
        "claude-teams exits with Claude's status: {out:?}"
    );
    assert_eq!(scene.read_report("")["mismatches"], serde_json::json!([]));
    assert_eq!(scene.team_dirs().len(), 1, "the team directory stays");

    let rows = list_json(&server, &scene);
    let tm1 = row(&rows, "lead.tm1").unwrap_or_else(|| panic!("lead.tm1 stays: {rows:#?}"));
    assert_eq!(tm1["orphan"], true, "{tm1}");
    assert_eq!(tm1["parent"], "lead", "{tm1}");

    // Ownership applies as on any pane (P8.4): the Teammate pane's cwd is the Leader's.
    let state = scene.state().display().to_string();
    let work = scene.work();
    server
        .run_cli(
            &["kill", "--target", "lead.tm1"],
            CliOptions {
                env: &[("XDG_STATE_HOME", &state)],
                cwd: Some(&work),
                ..CliOptions::default()
            },
        )
        .assert_success("kill lead.tm1");
    assert!(
        pane_id_by_name(&server, "lead.tm1").is_none(),
        "kill closed lead.tm1"
    );
}

/// Acceptance (P10.2): in a 120x24 window, teammates start until a `split-window`
/// exits 1 with `tmux: no space for a new pane`; that failed start creates no pane and
/// no registration. The respawn of `tm1` then exits 0, or exits 1 with the same
/// message while the old pane and its registration stay.
#[test]
fn a_full_window_refuses_a_teammate_without_a_pane_or_registration() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("nospace");
    server
        .tmux(&[
            "new-session",
            "-d",
            "-s",
            "small",
            "-x",
            "120",
            "-y",
            "24",
            "sleep",
            "3600",
        ])
        .assert_success("a 120x24 window");
    let leader = server.tmux(&["display-message", "-p", "-t", "=small:", "#{pane_id}"]);
    let leader = leader.stdout.trim().to_owned();
    server
        .tmux(&["set-option", "-p", "-t", &leader, "@tt-name", "lead"])
        .assert_success("name the Leader");

    let mut child = start_launcher(
        &server,
        &scene,
        &leader,
        &[
            ("FAKE_CLAUDE_SCRIPT", "nospace"),
            ("FAKE_CLAUDE_PAUSE", "1"),
            ("FAKE_CLAUDE_EXIT", "1"),
        ],
        &[],
    );
    scene.wait_paused(&mut child);
    let report = scene.read_report(".partial");
    let started = report["started"].as_array().unwrap().len();
    println!(
        "P10.2: maximum teammate count {started}; respawn of tm1: {}",
        report["respawn"]
    );
    assert!(started >= 1, "{report:#}");
    assert_eq!(report["failure"]["exit"], 1, "{report:#}");
    assert_eq!(
        report["failure"]["stderr"], "tmux: no space for a new pane\n",
        "{report:#}"
    );

    let panes = server.tmux(&[
        "list-panes",
        "-t",
        "=small:",
        "-F",
        "#{pane_id}\t#{@tt-name}\t#{@tt-shim-id}",
    ]);
    let lines: Vec<&str> = panes.stdout.lines().collect();
    assert_eq!(
        lines.len(),
        started + 1,
        "no pane for the failed start: {lines:?}"
    );
    let registered = lines.iter().filter(|line| line.contains(".tm")).count();
    assert_eq!(
        registered, started,
        "no registration for the failed start: {lines:?}"
    );

    let respawn = &report["respawn"];
    if respawn["exit"] != 0 {
        assert_eq!(respawn["exit"], 1, "{respawn}");
        assert_eq!(
            respawn["stderr"], "tmux: no space for a new pane\n",
            "{respawn}"
        );
        let tm1 = lines.iter().find(|line| line.ends_with("\t%2"));
        assert!(
            tm1.is_some_and(|line| line.contains("lead.tm1")),
            "the old tm1 and its registration stay: {lines:?}"
        );
    } else {
        assert!(
            pane_id_by_name(&server, "lead.tm1").is_some(),
            "tm1 respawned"
        );
    }
    scene.go();
    let out = finish(child);
    assert_eq!(out.status, 1, "{out:?}");
    for line in &lines {
        if let Some(pane) = line.split('\t').next() {
            if line.contains(".tm") {
                let _ = server.tmux(&["kill-pane", "-t", pane]);
            }
        }
    }
}

// --- P5: the Shim refuses -------------------------------------------------------------

/// Acceptance: the Shim refuses each unsupported command and flag of P5.8 with exit 1
/// and the P5.8 message, appending one `refused` line to `shim.log`; with `TT_TEAM`
/// unset, the binary started as `tmux` exits 1 with `tmux: not inside a tmux-tools
/// team`.
#[test]
fn the_shim_refuses_unsupported_commands_and_flags_and_logs_each() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("refuse");
    let lead = launch_pane(&server, "lead");
    let mut child = start_launcher(&server, &scene, &lead, &[("FAKE_CLAUDE_PAUSE", "1")], &[]);
    scene.wait_paused(&mut child);
    let team_dir = scene.team_dirs().pop().expect("a team directory");
    let id = team_dir.file_name().unwrap().to_str().unwrap().to_owned();
    let log = team_dir.join("shim.log");
    let log_lines = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>()
    };

    let cases: Vec<(Vec<&str>, String)> = vec![
        (
            vec!["new-session", "-d"],
            "unsupported command: new-session".into(),
        ),
        (vec!["new-window"], "unsupported command: new-window".into()),
        (vec!["splitw"], "unsupported command: splitw".into()),
        (
            vec!["display-message", "-p", "x", ";", "list-panes"],
            "unsupported command: ;".into(),
        ),
        (
            vec!["display-message", "-x", "-p", "x"],
            "unsupported flag -x".into(),
        ),
        (vec!["list-panes", "-a"], "unsupported flag -a".into()),
        (vec!["split-window", "-Z"], "unsupported flag -Z".into()),
        (
            vec!["respawn-pane", "-t", "%1", "x"],
            "respawn-pane needs -k".into(),
        ),
        (
            vec!["respawn-pane", "-k", "-t", "%1", "x"],
            "can't respawn leader pane".into(),
        ),
        (
            vec!["kill-pane", "-t", "%1"],
            "can't kill leader pane".into(),
        ),
        (
            vec!["capture-pane", "-t", "%1"],
            "capture-pane needs -p".into(),
        ),
        (
            vec!["display-message", "-p", "-t", "%9", "x"],
            "can't find pane: %9".into(),
        ),
        (
            vec!["has-session", "-t", "other"],
            "can't find session: other".into(),
        ),
    ];
    for (args, message) in &cases {
        let before = log_lines().len();
        let out = shim(&team_dir, &scene, "tmux", &[], args);
        assert_eq!(out.status, 1, "{args:?}: {out:?}");
        assert_eq!(out.stderr, format!("tmux: {message}\n"), "{args:?}");
        let lines = log_lines();
        assert_eq!(lines.len(), before + 1, "{args:?}: one line appended");
        let last = lines.last().unwrap();
        assert_eq!(last["outcome"], "refused", "{last}");
        assert_eq!(last["message"], message.as_str(), "{last}");
        let argv: Vec<&str> = last["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        assert_eq!(&argv, args, "{last}");
        assert!(
            last.get("exit").is_none() && last.get("stdout").is_none(),
            "{last}"
        );
    }

    // Accepted calls write no line without TT_SHIM_DEBUG=1.
    let before = log_lines().len();
    let version = shim(&team_dir, &scene, "tmux", &[], &["-V"]);
    assert_eq!((version.status, version.stdout.as_str()), (0, "tmux 3.4\n"));
    let ids = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &[
            "-S",
            "/x",
            "display-message",
            "-p",
            "#{session_name}:#{window_id}.#{pane_id}",
        ],
    );
    assert_eq!(ids.stdout, format!("tt-team-{id}:@0.%1\n"), "{ids:?}");
    let ignored = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &[
            "set-option",
            "-p",
            "-t",
            "%1",
            "pane-border-style",
            "fg=red",
        ],
    );
    assert_eq!((ignored.status, ignored.stdout.as_str()), (0, ""));
    assert_eq!(
        log_lines().len(),
        before,
        "no ok line without TT_SHIM_DEBUG=1"
    );

    scene.go();
    finish(child);

    // P5.1: outside a team.
    let link_dir = temp_dir("tmux-link");
    std::os::unix::fs::symlink(BIN, link_dir.join("tmux")).unwrap();
    let outside = wait_for_child(
        Command::new(link_dir.join("tmux"))
            .args(["list-panes"])
            .env_remove("TT_TEAM")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
        "tmux outside a team",
        Duration::from_secs(30),
    );
    assert_eq!(outside.status, 1, "{outside:?}");
    assert_eq!(outside.stderr, "tmux: not inside a tmux-tools team\n");
    let _ = std::fs::remove_dir_all(&link_dir);
}

// --- P2: the Launcher ---------------------------------------------------------------

/// Run `claude-teams` outside every host (no `TMUX`, `TMUX_PANE`, Orca variables, or
/// `TMUX_TOOLS_HOST`) with `scene`'s fake `claude` first on `PATH`.
fn launcher_outside(scene: &Scene, env: &[(&str, &str)], path: Option<&str>) -> CmdOutput {
    let mut command = Command::new(BIN);
    command
        .arg("claude-teams")
        .current_dir(scene.work())
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("TT_TEAM")
        .env(
            "PATH",
            path.map(str::to_owned).unwrap_or_else(|| {
                format!(
                    "{}:{}",
                    scene.bin().display(),
                    std::env::var("PATH").unwrap_or_default()
                )
            }),
        );
    for (key, value) in scene.base_env() {
        command.env(key, value);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_child(child, "claude-teams outside", Duration::from_secs(60))
}

/// Acceptance (P2.7): with no supported host, `claude-teams` prints the fail-open
/// warning, creates no team directory, starts Claude with `--teammate-mode in-process`
/// and the caller's environment, and exits with Claude's status.
#[test]
fn without_a_host_the_launcher_fails_open_to_in_process_teammates() {
    let scene = Scene::new("failopen");
    let out = launcher_outside(&scene, &[("FAKE_CLAUDE_EXIT", "7")], None);
    assert_eq!(out.status, 7, "{out:?}");
    assert_eq!(
        out.stderr,
        "claude-teams: no team: TMUX_PANE is unset; starting claude without teammate panes\n"
    );
    let args = scene.read_report(".args");
    assert_eq!(args, serde_json::json!(["--teammate-mode", "in-process"]));
    let env = scene.read_report(".env");
    assert!(env["TT_TEAM"].is_null() && env["TMUX"].is_null(), "{env}");
    assert!(scene.team_dirs().is_empty(), "no team directory");
}

/// Acceptance (P2.6, P2.2): inside a team the verb exits 1 with
/// `claude-teams: already inside team <id>`; with no resolvable `claude`, exit 1 with
/// `claude-teams: claude not found`. Neither creates a team directory.
#[test]
fn the_launcher_refuses_inside_a_team_and_without_claude() {
    let scene = Scene::new("refuse-launch");
    let inside = launcher_outside(&scene, &[("TT_TEAM", "0a1b2c3d")], None);
    assert_eq!(inside.status, 1, "{inside:?}");
    assert_eq!(
        inside.stderr,
        "claude-teams: already inside team 0a1b2c3d\n"
    );

    let none = launcher_outside(&scene, &[], Some("/usr/bin:/bin"));
    assert_eq!(none.status, 1, "{none:?}");
    assert_eq!(none.stderr, "claude-teams: claude not found\n");
    assert!(scene.team_dirs().is_empty());
}

/// Acceptance (P10.5): a Claude version with no fixture warns once on stderr and the
/// Launcher continues.
#[test]
fn an_untested_claude_version_warns_once_and_continues() {
    let scene = Scene::new("version");
    let out = launcher_outside(
        &scene,
        &[("FAKE_CLAUDE_VERSION", "9.9.9 (Claude Code)")],
        None,
    );
    out.assert_success("claude-teams with claude 9.9.9");
    assert_eq!(
        out.stderr
            .matches("claude-teams: claude 9.9.9 not contract-tested\n")
            .count(),
        1,
        "{}",
        out.stderr
    );
    assert!(scene.report().exists(), "Claude started");
    let tested = launcher_outside(&scene, &[], None);
    assert!(
        !tested.stderr.contains("not contract-tested"),
        "{}",
        tested.stderr
    );
}

// --- P7, P8: parent and orphan --------------------------------------------------------

/// Acceptance: `spawn-agent` and `launch` from the tmux-tools pane `p` record
/// `parent: "p"`; from a pane that is not a tmux-tools pane, `parent: null`; the text
/// table shows `PARENT` after `AGENT`. After `kill --target p`, the child is an Orphan
/// (`orphan:p` in the table), and stays one after a new pane named `p` is created.
#[test]
fn parent_is_recorded_and_a_dead_parent_makes_an_orphan() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("parent");
    std::fs::create_dir_all(scene.dir.join("config/tmux-tools")).unwrap();
    std::fs::write(
        scene.dir.join("config/tmux-tools/agents.toml"),
        "[sleeper]\nbinary = \"/bin/sleep\"\n\n[sleeper.access.default]\nargs = [\"3600\"]\n",
    )
    .unwrap();
    let p = launch_pane(&server, "p");

    cli_from(
        &server,
        &scene,
        &p,
        &[
            "launch",
            "--name",
            "c",
            "--split",
            "v",
            "--cmd",
            "sleep 3600",
        ],
    )
    .assert_success("launch c from p");
    cli_from(
        &server,
        &scene,
        &p,
        &[
            "spawn-agent",
            "sleeper",
            "--bare",
            "--split",
            "v",
            "--name",
            "c2",
        ],
    )
    .assert_success("spawn-agent c2 from p");
    cli(
        &server,
        &scene,
        &[
            "launch",
            "--name",
            "free",
            "--split",
            "v",
            "--cmd",
            "sleep 3600",
        ],
    )
    .assert_success("launch free from the harness pane");

    let rows = list_json(&server, &scene);
    for name in ["c", "c2"] {
        let child = row(&rows, name).unwrap();
        assert_eq!(child["parent"], "p", "{child}");
        assert_eq!(child["orphan"], false, "{child}");
        assert!(
            child["team"].is_null() && child["shim_id"].is_null(),
            "{child}"
        );
    }
    let free = row(&rows, "free").unwrap();
    assert!(free["parent"].is_null(), "{free}");
    assert_eq!(free["orphan"], false, "{free}");

    let table = cli(&server, &scene, &["list", "--all"]);
    let header: Vec<&str> = table
        .stdout
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .collect();
    let agent = header.iter().position(|h| *h == "AGENT").unwrap();
    assert_eq!(header[agent + 1], "PARENT", "{header:?}");

    cli(&server, &scene, &["kill", "--target", "p"]).assert_success("kill p");
    let rows = list_json(&server, &scene);
    assert_eq!(row(&rows, "c").unwrap()["orphan"], true);
    assert_eq!(row(&rows, "c2").unwrap()["orphan"], true);
    let table = cli(&server, &scene, &["list", "--all"]).stdout;
    let c_line = table.lines().find(|line| line.starts_with("c ")).unwrap();
    assert!(c_line.contains("orphan:p"), "{table}");

    // No wait: the new `p` may start within the same second as `c` (M1).
    launch_pane(&server, "p");
    let rows = list_json(&server, &scene);
    assert_eq!(
        row(&rows, "c").unwrap()["orphan"],
        true,
        "a new p is not c's parent"
    );
    assert_eq!(row(&rows, "c2").unwrap()["orphan"], true);
    // M1: `launched_at` has sub-second precision, so launches less than a second apart
    // still order.
    for name in ["c", "p"] {
        let launched = row(&rows, name).unwrap()["launched"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            launched.contains('.') && launched.ends_with('Z'),
            "{name}: sub-second launched_at {launched}"
        );
    }

    // Within one second: a parent launched 0.8 s after its child is a different pane,
    // and a whole-second record (an older version's) is never later than a
    // sub-second record of the same second.
    let set = |name: &str, value: &str| {
        let pane = pane_id_by_name(&server, name).unwrap();
        server
            .tmux(&["set-option", "-p", "-t", &pane, "@tt-launched-at", value])
            .assert_success("set @tt-launched-at");
    };
    set("c", "2026-01-01T00:00:00.100Z");
    set("p", "2026-01-01T00:00:00.900Z");
    assert_eq!(
        row(&list_json(&server, &scene), "c").unwrap()["orphan"],
        true
    );
    set("c", "2026-01-01T00:00:00Z");
    assert_eq!(
        row(&list_json(&server, &scene), "c").unwrap()["orphan"],
        false,
        "a whole-second child and a sub-second parent of the same second"
    );
}

// --- P6.5: the Orca form, with a fake `orca` CLI ---------------------------------------

/// Acceptance (P6.5): on Orca, a `split-window` with `-e ORCA_PANE_KEY=…`,
/// `-e ORCA_TERMINAL_HANDLE=…`, `-e ORCA_TAB_ID=…`, and `-e K=V` gives an Orca
/// pane-create call whose command has `K=V` and none of the three variables, in the
/// Orca Close-on-success wrap (P6.3).
#[test]
fn on_orca_the_shim_drops_the_orca_pane_variables_from_a_teammate_command() {
    let scene = Scene::new("orca");
    write_executable(&scene.bin().join("orca"), FAKE_ORCA);
    let state = scene.dir.join("orca-state.json");
    let orca_log = scene.dir.join("orca-log.jsonl");
    std::fs::write(
        &state,
        r#"{"next": 2, "flags": [], "terminals": [{"handle": "term_lead", "worktreeId": "fake-worktree", "tabId": "tab1", "leafId": "leaf1", "title": "lead", "paneTitle": "lead"}]}"#,
    )
    .unwrap();
    let orca_env = [
        ("ORCA_TERMINAL_HANDLE", "term_lead".to_owned()),
        ("ORCA_WORKTREE_ID", "fake-worktree".to_owned()),
        ("FAKE_ORCA_STATE", state.display().to_string()),
        ("FAKE_ORCA_LOG", orca_log.display().to_string()),
    ];
    let mut command = Command::new(BIN);
    command
        .arg("claude-teams")
        .current_dir(scene.work())
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env(
            "PATH",
            format!(
                "{}:{}",
                scene.bin().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("FAKE_CLAUDE_PAUSE", "1");
    for (key, value) in scene.base_env() {
        command.env(key, value);
    }
    for (key, value) in &orca_env {
        command.env(key, value);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    scene.wait_paused(&mut child);
    let team_dir = scene.team_dirs().pop().expect("a team directory on Orca");
    let env = scene.read_report(".env");
    assert_eq!(env["TT_HOST"], "orca", "{env}");

    let mut shim_env: Vec<(&str, String)> = orca_env.to_vec();
    shim_env.push((
        "PATH",
        format!(
            "{}:{}",
            scene.bin().display(),
            std::env::var("PATH").unwrap()
        ),
    ));
    let out = shim(
        &team_dir,
        &scene,
        "orca",
        &shim_env,
        &[
            "split-window",
            "-d",
            "-t",
            "%1",
            "-h",
            "-e",
            "ORCA_PANE_KEY=tab1:leaf1",
            "-e",
            "ORCA_TERMINAL_HANDLE=term_lead",
            "-e",
            "ORCA_TAB_ID=tab1",
            "-e",
            "K=V",
            "-P",
            "-F",
            "#{pane_id}",
            "--",
            "cat",
        ],
    );
    assert_eq!((out.status, out.stdout.as_str()), (0, "%2\n"), "{out:?}");

    // P5.6 on Orca: a negative `-S` adds that many history lines to the screen, and
    // `-S -` reads all history.
    let mut scroll_env = shim_env.clone();
    scroll_env.push(("FAKE_ORCA_SCROLLBACK", "40".to_owned()));
    let lines = |from: u32| {
        (from..=40)
            .map(|n| format!("line{n}\n"))
            .collect::<String>()
    };
    let tail = shim(
        &team_dir,
        &scene,
        "orca",
        &scroll_env,
        &["capture-pane", "-p", "-S", "-10", "-t", "%2"],
    );
    assert_eq!(
        (tail.status, tail.stdout.clone()),
        (0, lines(26)),
        "{tail:?}"
    );
    let all = shim(
        &team_dir,
        &scene,
        "orca",
        &scroll_env,
        &["capture-pane", "-p", "-S", "-", "-t", "%2"],
    );
    assert_eq!((all.status, all.stdout.clone()), (0, lines(1)), "{all:?}");
    // A negative `-S` beyond the `Tail` range reads all history; it never wraps or panics.
    for start in ["-4294967295", "-4294967296", "-9223372036854775808"] {
        let huge = shim(
            &team_dir,
            &scene,
            "orca",
            &scroll_env,
            &["capture-pane", "-p", "-S", start, "-t", "%2"],
        );
        assert_eq!(
            (huge.status, huge.stdout.clone()),
            (0, lines(1)),
            "-S {start}: {huge:?}"
        );
    }
    let screen = shim(
        &team_dir,
        &scene,
        "orca",
        &scroll_env,
        &["capture-pane", "-p", "-t", "%2"],
    );
    assert_eq!(screen.stdout, lines(36), "{screen:?}");

    let calls: Vec<Vec<String>> = std::fs::read_to_string(&orca_log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let split = calls
        .iter()
        .find(|call| call.len() > 1 && call[0] == "terminal" && call[1] == "split")
        .unwrap_or_else(|| panic!("an Orca pane-create call: {calls:?}"));
    let pane_command = &split[split.iter().position(|a| a == "--command").unwrap() + 1];
    assert!(pane_command.contains("K=V"), "{pane_command}");
    for variable in ["ORCA_PANE_KEY", "ORCA_TERMINAL_HANDLE", "ORCA_TAB_ID"] {
        assert!(
            !pane_command.contains(variable),
            "{variable} in {pane_command}"
        );
    }
    assert!(
        pane_command.starts_with("exec sh -c '")
            && pane_command.contains("|| exec \"${SHELL:-/bin/sh}\" -l"),
        "the Orca Close-on-success wrap: {pane_command}"
    );
    assert_eq!(
        split[split.iter().position(|a| a == "--terminal").unwrap() + 1],
        "term_lead",
        "the first teammate splits the Leader"
    );

    scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
    let state: Value = serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    let handles: Vec<&str> = state["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["handle"].as_str().unwrap())
        .collect();
    assert_eq!(
        handles,
        vec!["term_lead"],
        "the clean exit closed the teammate"
    );
    assert!(!team_dir.exists());
}

// --- Correction batch 1 (M2, M3, L1, M5) ------------------------------------------

/// M2 (P7.1): with `TMUX_PANE` naming a pane that no longer exists, `launch` succeeds
/// and records `parent: null`. tmux 3.5 and later answer a read of a missing pane with
/// exit 0; a `tmux` first on `PATH` reproduces the `can't find pane` failure of earlier
/// versions for that pane only.
#[test]
fn a_stale_calling_pane_records_a_null_parent() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("stale");
    let wrapper_dir = scene.dir.join("tmux-bin");
    std::fs::create_dir_all(&wrapper_dir).unwrap();
    write_executable(
        &wrapper_dir.join("tmux"),
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = %9999 ] && {{ echo \"can't find pane: %9999\" >&2; exit 1; }}; done\nexec '{}' \"$@\"\n",
            real_tmux_path()
        ),
    );
    let state = scene.state().display().to_string();
    let out = server.run_cli(
        &[
            "launch",
            "--name",
            "s1",
            "--session",
            "isolated-root",
            "--cmd",
            "sleep 3600",
        ],
        CliOptions {
            env: &[("XDG_STATE_HOME", &state), ("TMUX_PANE", "%9999")],
            path_prefix: Some(&wrapper_dir),
            ..CliOptions::default()
        },
    );
    out.assert_success("launch with a stale TMUX_PANE");
    let rows = list_json(&server, &scene);
    let s1 = row(&rows, "s1").unwrap_or_else(|| panic!("s1 listed: {rows:#?}"));
    assert!(s1["parent"].is_null(), "{s1}");
}

/// A Team on the isolated server, paused, with its Leader pane `lead`; returns the
/// Launcher, the Leader pane id, and the team directory.
fn paused_team(server: &IsolatedTmuxServer, scene: &Scene) -> (Child, String, PathBuf) {
    let lead = launch_pane(server, "lead");
    let mut child = start_launcher(server, scene, &lead, &[("FAKE_CLAUDE_PAUSE", "1")], &[]);
    scene.wait_paused(&mut child);
    let team_dir = scene.team_dirs().pop().expect("a team directory");
    (child, lead, team_dir)
}

fn pane_active(server: &IsolatedTmuxServer, pane: &str) -> String {
    server
        .tmux(&["display-message", "-p", "-t", pane, "#{pane_active}"])
        .stdout
        .trim()
        .to_owned()
}

/// M3 (P5.6): on tmux, a Shim `split-window -d` and a `respawn-pane -k` create the
/// pane without changing the active pane: the Leader stays active.
#[test]
fn split_window_dash_d_and_respawn_keep_the_leader_active() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("detached");
    let (child, lead, team_dir) = paused_team(&server, &scene);
    assert_eq!(pane_active(&server, &lead), "1", "the Leader starts active");

    let split = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &[
            "split-window",
            "-d",
            "-t",
            "%1",
            "-h",
            "-P",
            "-F",
            "#{pane_id}",
            "--",
            "cat",
        ],
    );
    assert_eq!(
        (split.status, split.stdout.as_str()),
        (0, "%2\n"),
        "{split:?}"
    );
    assert_eq!(
        pane_active(&server, &lead),
        "1",
        "split-window -d keeps the Leader active"
    );

    let respawn = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &["respawn-pane", "-k", "-t", "%2", "--", "sleep 3600"],
    );
    assert_eq!(respawn.status, 0, "{respawn:?}");
    assert_eq!(
        pane_active(&server, &lead),
        "1",
        "respawn-pane keeps the Leader active"
    );
    assert!(pane_id_by_name(&server, "lead.tm1").is_some());

    scene.go();
    finish(child);
}

/// L1 (P6.4): a `respawn-pane` command that does not split (a trailing backslash) gives
/// `agent`, `claude_agent_name` and `claude_team_name` null, and the respawn succeeds.
/// An unclosed quote is also a split failure (unit test in `src/shim.rs`); it is not
/// driven here because `sh` cannot parse such a command, so the pane would exit at once.
#[test]
fn a_respawn_command_that_does_not_split_carries_no_claude_metadata() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("nosplit");
    let (child, _lead, team_dir) = paused_team(&server, &scene);
    let claude = std::fs::canonicalize(scene.bin().join("claude")).unwrap();
    let claude = claude.display().to_string();
    let split = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &["split-window", "-d", "-t", "%1", "--", "cat"],
    );
    assert_eq!(split.status, 0, "{split:?}");
    let option = |key: &str| {
        let pane = pane_id_by_name(&server, "lead.tm1").expect("lead.tm1");
        server
            .tmux(&["display-message", "-p", "-t", &pane, key])
            .stdout
            .trim()
            .to_owned()
    };

    let good = format!("'{claude}' --agent-name r1 --team-name t1");
    let out = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &["respawn-pane", "-k", "-t", "%2", "--", &good],
    );
    assert_eq!(out.status, 0, "{out:?}");
    assert_eq!(
        (
            option("#{@tt-agent}"),
            option("#{@tt-claude-agent-name}"),
            option("#{@tt-claude-team-name}")
        ),
        ("claude".to_owned(), "r1".to_owned(), "t1".to_owned())
    );

    let broken = format!("{claude} --agent-name r2 --team-name t2 \\");
    let out = shim(
        &team_dir,
        &scene,
        "tmux",
        &[],
        &["respawn-pane", "-k", "-t", "%2", "--", &broken],
    );
    assert_eq!(out.status, 0, "{broken}: {out:?}");
    assert_eq!(
        (
            option("#{@tt-agent}"),
            option("#{@tt-claude-agent-name}"),
            option("#{@tt-claude-team-name}")
        ),
        (String::new(), String::new(), String::new()),
        "{broken}"
    );

    scene.go();
    finish(child);
}

/// M5 (P2.2): an empty `PATH` component is the current directory, searched in its
/// place: a `claude` there wins over a later one.
#[test]
fn an_empty_path_component_resolves_claude_in_the_current_directory() {
    let scene = Scene::new("emptypath");
    write_executable(
        &scene.work().join("claude"),
        &FAKE_CLAUDE.replace("2.1.319 (Claude Code)", "7.7.7 (Claude Code)"),
    );
    let path = format!(
        ":{}:{}",
        scene.bin().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = launcher_outside(&scene, &[], Some(&path));
    out.assert_success("claude-teams with an empty PATH component");
    assert!(
        out.stderr
            .contains("claude-teams: claude 7.7.7 not contract-tested"),
        "the current directory's claude ran: {}",
        out.stderr
    );
    assert!(scene.report().exists(), "Claude started");
}

// --- P10.3: the opt-in live run ------------------------------------------------------

/// The opt-in live test (P10.3): `TMUX_TOOLS_LIVE_CLAUDE=1` runs the real `claude`
/// under `claude-teams` on the isolated tmux server with `TT_SHIM_DEBUG=1`,
/// `disableAllHooks: true`, and `--disallowedTools Bash` (every Shim call comes from
/// Claude itself), has it spawn one teammate, and fails when the Claude version has no
/// fixture or when `shim.log` has a `refused` line whose argv equals the argv of no
/// fixture line with exit 1 (placeholders filled). It prints the debug directory, from
/// which `tt-fixture` makes a fixture.
#[test]
fn live_claude_spawns_a_teammate_with_no_refusal_outside_the_fixture() {
    if original_environment()
        .get("TMUX_TOOLS_LIVE_CLAUDE")
        .map(String::as_str)
        != Some("1")
    {
        return;
    }
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let state = temp_dir("live-state");
    let work = std::fs::canonicalize(temp_dir("live-work")).unwrap();
    let lead = launch_pane_running(&server, "lead", "sh");
    thread::sleep(Duration::from_millis(500));
    let prompt = "Create an agent team named probe with exactly one teammate named r1. \
                  Tell r1 to reply to you with the single word PONG. When r1 has replied, \
                  shut r1 down, delete the team, and then reply with the single word DONE.";
    let script = format!(
        "cd {work} && XDG_STATE_HOME={state} TT_SHIM_DEBUG=1 {bin} claude-teams -- \
         --settings '{{\"disableAllHooks\":true}}' --disallowedTools Bash --dangerously-skip-permissions '{prompt}'; \
         echo \"LAUNCHER\"\"-EXIT=$?\"",
        work = work.display(),
        state = state.display(),
        bin = BIN,
    );
    server
        .tmux(&[
            "send-keys",
            "-t",
            &lead,
            "-l",
            "--",
            &format!("sh -c {}", shell_quote(&script)),
        ])
        .assert_success("type the launcher command");
    server
        .tmux(&["send-keys", "-t", &lead, "Enter"])
        .assert_success("enter");

    let debug_root = state.join("tmux-tools/shim-debug");
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut debug_dir = None;
    let mut sent_exit = false;
    loop {
        let screen = server.tmux(&["capture-pane", "-p", "-t", &lead]).stdout;
        if debug_dir.is_none() {
            debug_dir = std::fs::read_dir(&debug_root)
                .ok()
                .and_then(|mut entries| entries.next())
                .and_then(|entry| entry.ok())
                .map(|entry| entry.path());
        }
        if screen.contains("LAUNCHER-EXIT=") {
            println!("P10.3 leader screen at exit:\n{screen}");
            break;
        }
        // Trust the work directory when asked.
        if screen.contains("Yes, I trust this folder") || screen.contains("trust this folder") {
            server.tmux(&["send-keys", "-t", &lead, "Enter"]);
        }
        if !sent_exit
            && screen
                .lines()
                .any(|line| line.trim() == "⏺ DONE" || line.trim_end().ends_with(" DONE"))
        {
            thread::sleep(Duration::from_secs(3));
            server.tmux(&["send-keys", "-t", &lead, "-l", "/exit"]);
            thread::sleep(Duration::from_millis(500));
            server.tmux(&["send-keys", "-t", &lead, "Enter"]);
            sent_exit = true;
        }
        assert!(
            Instant::now() < deadline,
            "the live run did not finish:\n{screen}"
        );
        thread::sleep(Duration::from_secs(2));
    }
    let debug_dir = debug_dir.expect("the debug directory exists");
    println!("P10.3 debug directory: {}", debug_dir.display());
    let log = std::fs::read_to_string(debug_dir.join("shim.log")).expect("shim.log");
    let team: Value =
        serde_json::from_str(&std::fs::read_to_string(debug_dir.join("team.json")).unwrap())
            .unwrap();
    let version = team["claude"]["version"].as_str().unwrap();
    let key = version.split_whitespace().next().unwrap();
    println!("P10.3 Claude version: {version}");
    let fixture_text = std::fs::read_to_string(fixture(key))
        .unwrap_or_else(|_| panic!("no fixture for Claude {key}"));
    let allowed = refused_fixture_argvs(&fixture_text, &team);
    let refused: Vec<Vec<String>> = log
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|entry| entry["outcome"] == "refused")
        .map(|entry| argv_of(&entry))
        .collect();
    let unexpected: Vec<&Vec<String>> = refused
        .iter()
        .filter(|argv| !allowed.contains(argv))
        .collect();
    println!(
        "P10.3 shim.log: {} lines, {} refused, {} outside the fixture's exit 1 lines",
        log.lines().count(),
        refused.len(),
        unexpected.len()
    );
    assert!(
        log.contains("split-window"),
        "Claude spawned a teammate: {log}"
    );
    assert!(
        unexpected.is_empty(),
        "refused lines outside the fixture: {unexpected:#?}"
    );
}

// --- P9.2: `spawn-agent claude` on tmux starts the Launcher ------------------------

/// Run the CLI on `server` with the scene's state and config directories.
fn cli_configured(server: &IsolatedTmuxServer, scene: &Scene, args: &[&str]) -> CmdOutput {
    let state = scene.state().display().to_string();
    let config = scene.dir.join("config").display().to_string();
    server.run_cli(
        args,
        CliOptions {
            env: &[("XDG_STATE_HOME", &state), ("XDG_CONFIG_HOME", &config)],
            timeout: Some(Duration::from_secs(120)),
            ..CliOptions::default()
        },
    )
}

/// The fields of a `prompt` or `wait-idle` JSON result that do not name the pane; text
/// loses its trailing blank lines, which depend on the pane height.
fn pane_independent_result(out: &CmdOutput, keys: &[&str]) -> (i32, Vec<Value>) {
    let fields = match serde_json::from_str::<Value>(&out.stdout) {
        Ok(value) => keys
            .iter()
            .map(|key| match &value[*key] {
                Value::String(text) => Value::String(text.trim_end().to_owned()),
                other => other.clone(),
            })
            .collect(),
        Err(_) => vec![Value::String(out.stderr.clone())],
    };
    (out.status, fields)
}

/// Acceptance (ISSUE-261108-2323-03, PRD-261108-0447-01 P9.1/P9.2): on tmux,
/// `spawn-agent claude --name lead` runs `<tmux-tools> claude-teams --claude <binary> --
/// <claude args>` inside the Keep-alive wrap; `lead` keeps `agent` and Surface, its
/// teammate is `lead.tm1` with `team` and `parent: "lead"`; `prompt` and `wait-idle`
/// on `lead` give the result they give for the same fake `claude` started without the
/// Launcher; `kill --target lead` closes the Leader, and no pane is left.
#[test]
fn spawn_agent_claude_on_tmux_starts_claude_through_the_launcher() {
    for_every_fixture(spawn_agent_claude_on_tmux_starts_claude_through_the_launcher_with);
}

fn spawn_agent_claude_on_tmux_starts_claude_through_the_launcher_with(key: &str) {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let harness_pane = server.pane().to_owned();
    let scene = Scene::new("spawn-lead");
    std::os::unix::fs::symlink(
        scene.bin().join("claude"),
        scene.bin().join("claude-direct"),
    )
    .unwrap();
    std::fs::create_dir_all(scene.dir.join("config/tmux-tools")).unwrap();
    let agents_toml = scene.dir.join("config/tmux-tools/agents.toml");
    let start = scene.dir.join("start");
    let fixture_path = fixture(key).display().to_string();
    // A pane started by tmux gets the server's global environment.
    for (key, value) in scene.base_env().into_iter().chain([
        ("FAKE_CLAUDE_FIXTURE".to_owned(), fixture_path),
        ("FAKE_CLAUDE_VERSION".to_owned(), fake_version(key)),
        ("FAKE_CLAUDE_PAUSE_BEFORE_KILL".to_owned(), "1".to_owned()),
        ("FAKE_CLAUDE_NO_TEAM_SLEEP".to_owned(), "1".to_owned()),
        (
            "FAKE_CLAUDE_WAIT_FOR".to_owned(),
            start.display().to_string(),
        ),
    ]) {
        server
            .tmux(&["set-environment", "-g", &key, &value])
            .assert_success("set-environment");
    }

    // The same fake claude, started without the Launcher (binary basename not `claude`).
    std::fs::write(
        &agents_toml,
        format!(
            "[claude]\nbinary = \"{}\"\n",
            scene.bin().join("claude-direct").display()
        ),
    )
    .unwrap();
    cli_configured(
        &server,
        &scene,
        &["spawn-agent", "claude", "--split", "v", "--name", "direct"],
    )
    .assert_success("spawn-agent claude --name direct");

    let claude = scene.bin().join("claude");
    std::fs::write(
        &agents_toml,
        format!("[claude]\nbinary = \"{}\"\n", claude.display()),
    )
    .unwrap();
    cli_configured(
        &server,
        &scene,
        &["spawn-agent", "claude", "--split", "v", "--name", "lead"],
    )
    .assert_success("spawn-agent claude --name lead");
    std::fs::write(&start, "1").unwrap();

    let paused = PathBuf::from(format!("{}.paused", scene.report().display()));
    let deadline = Instant::now() + Duration::from_secs(60);
    while !paused.exists() {
        assert!(
            Instant::now() < deadline,
            "the fake Leader never paused; lead shows:\n{}",
            server
                .tmux(&[
                    "capture-pane",
                    "-p",
                    "-t",
                    &pane_id_by_name(&server, "lead").unwrap()
                ])
                .stdout
        );
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        scene.read_report(".partial")["mismatches"],
        serde_json::json!([])
    );

    let tmux_tools = std::fs::canonicalize(BIN).unwrap();
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/sh".to_owned());
    let lead_pane = pane_id_by_name(&server, "lead").unwrap();
    let start_command = |pane: &str| {
        let out = server.tmux(&["display-message", "-p", "-t", pane, "#{pane_start_command}"]);
        out.assert_success("pane_start_command");
        out.stdout.trim_end_matches('\n').to_owned()
    };
    assert_eq!(
        start_command(&lead_pane),
        format!(
            // tmux shows the start command as one double-quoted word.
            "\"('{}' 'claude-teams' '--claude' '{}' '--' '--settings' '{{\\\"env\\\":{{\\\"CLAUDE_CODE_NO_FLICKER\\\":\\\"0\\\"}}}}' '--permission-mode' 'plan'); exec {shell}\"",
            tmux_tools.display(),
            claude.display()
        )
    );

    let teams = scene.team_dirs();
    assert_eq!(teams.len(), 1, "one team directory: {teams:?}");
    let team_id = teams[0].file_name().unwrap().to_str().unwrap().to_owned();
    let rows = list_json(&server, &scene);
    let lead = row(&rows, "lead").unwrap_or_else(|| panic!("lead listed: {rows:#?}"));
    assert_eq!(lead["agent"], "claude", "{lead}");
    let tm1 = row(&rows, "lead.tm1").unwrap_or_else(|| panic!("lead.tm1 listed: {rows:#?}"));
    assert_eq!(tm1["team"], team_id.as_str(), "{tm1}");
    assert_eq!(tm1["parent"], "lead", "{tm1}");
    let surface = |pane: &str| {
        server
            .tmux(&["display-message", "-p", "-t", pane, "#{@tt-surface}"])
            .stdout
            .trim()
            .to_owned()
    };
    assert_eq!(surface(&lead_pane), "rich");

    // The Leader answers `prompt` and `wait-idle` as the same fake claude without the
    // Launcher does.
    let prompt_keys = [
        "prompt_sent",
        "output_since_prompt",
        "reason",
        "one_submission_delivery",
        "turn_observed",
        "complete",
    ];
    let wait_keys = ["reason", "final_capture"];
    let mut results = Vec::new();
    for name in ["direct", "lead"] {
        let prompt = cli_configured(
            &server,
            &scene,
            &[
                "prompt",
                "--target",
                name,
                "--timeout",
                "30",
                "--format",
                "json",
                "hello",
            ],
        );
        let wait = cli_configured(
            &server,
            &scene,
            &[
                "wait-idle",
                "--target",
                name,
                "--timeout",
                "30",
                "--format",
                "json",
            ],
        );
        results.push((
            pane_independent_result(&prompt, &prompt_keys),
            pane_independent_result(&wait, &wait_keys),
        ));
    }
    assert_eq!(results[1], results[0], "lead (right) and direct (left)");
    assert_eq!((results[1].0).0, 0, "prompt on lead: {:?}", results[1].0);
    assert_eq!((results[1].1).0, 0, "wait-idle on lead: {:?}", results[1].1);

    // The fake Leader closes lead.tm1 itself and exits 0.
    scene.go();
    let deadline = Instant::now() + Duration::from_secs(120);
    while !scene.report().exists() || pane_id_by_name(&server, "lead.tm1").is_some() {
        assert!(Instant::now() < deadline, "the fake Leader never finished");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(scene.read_report("")["mismatches"], serde_json::json!([]));

    assert_eq!(
        pane_id_by_name(&server, "lead").as_deref(),
        Some(lead_pane.as_str())
    );
    cli(&server, &scene, &["kill", "--target", "lead"]).assert_success("kill lead");
    let tmux_panes = || -> std::collections::BTreeSet<String> {
        let out = server.tmux(&["list-panes", "-a", "-F", "#{pane_id}"]);
        out.assert_success("tmux list-panes -a");
        out.stdout.lines().map(str::to_owned).collect()
    };
    assert!(
        !tmux_panes().contains(&lead_pane),
        "lead's pane {lead_pane} is closed"
    );
    let rows = list_json(&server, &scene);
    assert!(row(&rows, "lead").is_none(), "{rows:#?}");
    cli(&server, &scene, &["kill", "--target", "direct"]).assert_success("kill direct");

    // No leaked pane: tmux-tools and tmux itself both see only the harness pane.
    let only_harness = std::collections::BTreeSet::from([harness_pane.clone()]);
    let rows = list_json(&server, &scene);
    let listed: std::collections::BTreeSet<String> = rows
        .iter()
        .map(|row| {
            let pane = row["pane"].as_str().unwrap();
            pane[pane.rfind('%').expect("a pane id in the pane column")..].to_owned()
        })
        .collect();
    assert_eq!(listed, only_harness, "list --all: {rows:#?}");
    assert_eq!(tmux_panes(), only_harness, "tmux list-panes -a");
}

fn argv_of(entry: &Value) -> Vec<String> {
    entry["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect()
}

/// The argv of every fixture line with exit 1, its placeholders filled with `team`'s
/// values (`{{claude}}` as either Claude path).
fn refused_fixture_argvs(fixture: &str, team: &Value) -> Vec<Vec<String>> {
    let text = |pointer: &str| {
        team.pointer(pointer)
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    };
    let (id, cwd) = (text("/id"), text("/cwd"));
    let mut argvs = Vec::new();
    for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line).unwrap();
        if entry["exit"] != 1 {
            continue;
        }
        for claude in [text("/claude/resolved"), text("/claude/path")] {
            let fill = |word: String| {
                word.replace("{{team_id}}", &id)
                    .replace("{{cwd}}", &cwd)
                    .replace("{{claude}}", &claude)
            };
            argvs.push(argv_of(&entry).into_iter().map(fill).collect());
        }
    }
    argvs
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

// --- PRD-261115-0921-01: the Herdr slice, against the fake `hc` ----------------------

const FAKE_HC: &str = include_str!("contract/fake_hc.py");

/// A Herdr terminal scene: the fake `hc` and the fake `claude` first on `PATH`, and a
/// Leader terminal `t1` registered as `lead` in space `S1` and tab `T1`.
struct HerdrScene {
    scene: Scene,
}

impl HerdrScene {
    fn new(label: &str) -> HerdrScene {
        let scene = Scene::new(label);
        write_executable(&scene.bin().join("hc"), FAKE_HC);
        let herdr = HerdrScene { scene };
        let leader = serde_json::json!({
            "address": "t1", "name": "tt-lead", "cwd": "/", "argv": ["sh"],
            "labels": {"tt.name": "lead", "where": "lead", "herdr.space": "S1", "herdr.tab": "T1"},
            "state": "live", "status": null, "status_ms": null, "turn": 0, "lines": [],
            "how": "history", "complete": true, "keys": [], "sent": [], "on_send": {},
        });
        herdr.save(&serde_json::json!({"next": 2, "terminals": [leader]}));
        herdr
    }

    fn hc_state(&self) -> PathBuf {
        self.scene.dir.join("hc-state.json")
    }

    fn hc_log(&self) -> PathBuf {
        self.scene.dir.join("hc-log.jsonl")
    }

    fn save(&self, state: &Value) {
        std::fs::write(self.hc_state(), serde_json::to_vec(state).unwrap()).unwrap();
    }

    /// The fake's terminals, read through the fake `hc` itself, so an ended command
    /// is applied first (`FAKE_HC_EXEC`).
    fn terminals(&self) -> Vec<Value> {
        let out = self
            .command(Path::new("hc"))
            .args(["ls", "--json", "--all"])
            .output()
            .unwrap();
        assert!(out.status.success(), "hc ls: {out:?}");
        let state: Value =
            serde_json::from_slice(&std::fs::read(self.hc_state()).unwrap()).unwrap();
        state["terminals"].as_array().unwrap().clone()
    }

    fn terminal(&self, address: &str) -> Option<Value> {
        self.terminals()
            .into_iter()
            .find(|terminal| terminal["address"] == address)
    }

    fn edit(&self, address: &str, change: impl FnOnce(&mut Value)) {
        let mut state: Value =
            serde_json::from_slice(&std::fs::read(self.hc_state()).unwrap()).unwrap();
        let terminal = state["terminals"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|terminal| terminal["address"] == address)
            .expect("the terminal exists");
        change(terminal);
        self.save(&state);
    }

    /// Every logged `hc run` call.
    fn runs(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.hc_log())
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
            .filter(|call| call.first().map(String::as_str) == Some("run"))
            .collect()
    }

    /// `program` in the Herdr terminal `t1`: no tmux, Orca, Rex, or team variables.
    fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.scene.work())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("TMUX_TOOLS_HOST")
            .env_remove("ORCA_TERMINAL_HANDLE")
            .env_remove("REX_BLOCK")
            .env_remove("TT_TEAM")
            .env_remove("TT_HOST")
            .env_remove("TT_SHIM_DEBUG")
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.scene.bin().display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HC_TERMINAL_ID", "t1")
            .env("SHELL", "/bin/sh")
            .env("FAKE_HC_STATE", self.hc_state())
            .env("FAKE_HC_LOG", self.hc_log());
        for (key, value) in self.scene.base_env() {
            command.env(key, value);
        }
        command
    }

    fn start_launcher(&self, env: &[(&str, &str)]) -> Child {
        let mut command = self.command(Path::new(BIN));
        command.arg("claude-teams");
        for (key, value) in env {
            command.env(key, value);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn claude-teams on herdr")
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        let child = self
            .command(Path::new(BIN))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            Duration::from_secs(60),
        )
    }

    fn list(&self) -> Vec<Value> {
        let out = self.cli(&["list", "--format", "json"]);
        out.assert_success("list --format json on herdr");
        serde_json::from_str::<Value>(&out.stdout)
            .unwrap_or_else(|e| panic!("list JSON: {e}: {}", out.stdout))
            .as_array()
            .unwrap()
            .clone()
    }

    /// The Shim of `team_dir` on Herdr, with `env` added.
    fn shim(&self, team_dir: &Path, env: &[(&str, &str)], args: &[&str]) -> CmdOutput {
        let id = team_dir.file_name().unwrap().to_str().unwrap();
        let mut command = self.command(&team_dir.join("bin/tmux"));
        command
            .args(args)
            .env("TT_TEAM", id)
            .env("TT_HOST", "herdr");
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_child(child, "shim on herdr", Duration::from_secs(60))
    }
}

/// The value after `flag` in one logged `hc` call.
fn hc_flag<'a>(call: &'a [String], flag: &str) -> Option<&'a str> {
    let at = call.iter().position(|arg| arg == flag)?;
    call.get(at + 1).map(String::as_str)
}

/// Every `--label` value of one logged `hc` call.
fn hc_labels(call: &[String]) -> Vec<String> {
    call.windows(2)
        .filter(|pair| pair[0] == "--label")
        .map(|pair| pair[1].clone())
        .collect()
}

/// Assert the PRD-261115-0921-01 Herdr Teammate terminal create of one `hc run`: the
/// flags with `--rm`, and the split target's space and tab with the team labels.
fn assert_teammate_run(run: &[String], team_id: &str) {
    for flag in ["--bg", "--outlive-parent", "--no-notify", "--rm"] {
        assert!(run.iter().any(|arg| arg == flag), "{flag}: {run:?}");
    }
    let labels = hc_labels(run);
    for label in [
        "herdr.space=S1".to_owned(),
        "herdr.tab=T1".to_owned(),
        format!("tt.team={team_id}"),
        "tt.parent=lead".to_owned(),
        "tt.shim_id=%2".to_owned(),
    ] {
        assert!(labels.contains(&label), "{label}: {labels:?}");
    }
}

fn label_of<'a>(terminal: &'a Value, key: &str) -> &'a Value {
    &terminal["labels"][key]
}

/// Acceptance (PRD-261115-0921-01 P2, P3.1–P3.2, P5, P7.1): `claude-teams` in the
/// Herdr terminal `t1` (registered as `lead`) sets up a Team; each trace fixture
/// replays through the Shim with every stdout and exit as recorded; its
/// `split-window` creates teammate `lead.tm1` by one `hc run` with `--rm` and the
/// Leader's space and tab, listed with `team`, `parent: "lead"`, and `shim_id`; its
/// `respawn-pane -k` runs the new terminal under `lead.tm1.respawn-<pid>`, removes the
/// old terminal, then writes `lead.tm1` to `tt.name` and `where`, and the hc name stays
/// that of the transitional name. A create of `lead.tm1` then fails as taken, and the
/// Shim advances `k`. The clean exit closes every teammate and removes the team
/// directory.
#[test]
fn on_herdr_the_fixture_replays_into_teammate_terminals_in_the_leaders_tab() {
    for_every_fixture(on_herdr_the_fixture_replays_with);
}

fn on_herdr_the_fixture_replays_with(key: &str) {
    let herdr = HerdrScene::new("herdr-replay");
    let fixture = fixture(key).display().to_string();
    let version = fake_version(key);
    let mut child = herdr.start_launcher(&[
        ("FAKE_CLAUDE_FIXTURE", fixture.as_str()),
        ("FAKE_CLAUDE_VERSION", version.as_str()),
        ("FAKE_CLAUDE_PAUSE_BEFORE_KILL", "1"),
        ("FAKE_CLAUDE_SELF", "1"),
    ]);
    herdr.scene.wait_paused(&mut child);
    let partial = herdr.scene.read_report(".partial");
    assert_eq!(partial["mismatches"], serde_json::json!([]), "{partial:#}");

    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");
    let team: Value =
        serde_json::from_slice(&std::fs::read(team_dir.join("team.json")).unwrap()).unwrap();
    let id = team["id"].as_str().unwrap().to_owned();
    assert_eq!(team["host"], "herdr", "{team}");
    assert_eq!(team["server_id"], "fake-core", "{team}");
    assert_eq!(team["leader"]["handle"], "t1", "{team}");
    assert!(team["tmux"].is_null(), "{team}");
    let env = herdr.scene.read_report(".env");
    assert_eq!(env["TT_HOST"], "herdr", "{env}");
    let own: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}.self", herdr.scene.report().display())).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (&own["host"], &own["role"], &own["team"], &own["name"]),
        (
            &Value::from("herdr"),
            &Value::from("leader"),
            &Value::from(id.as_str()),
            &Value::from("lead")
        ),
        "{own}"
    );

    // P3.1–P3.2, P6.1: one `hc run` per create, each a Teammate terminal.
    let runs = herdr.runs();
    assert_eq!(runs.len(), 2, "split-window and respawn-pane: {runs:?}");
    for run in &runs {
        assert_teammate_run(run, &id);
    }
    let first = hc_labels(&runs[0]);
    assert!(first.contains(&"tt.name=lead.tm1".to_owned()), "{first:?}");
    assert!(first.contains(&"where=lead.tm1".to_owned()), "{first:?}");
    let second = hc_labels(&runs[1]);
    let transitional = second
        .iter()
        .find_map(|label| label.strip_prefix("tt.name="))
        .unwrap()
        .to_owned();
    let pid = transitional
        .strip_prefix("lead.tm1.respawn-")
        .unwrap_or_else(|| panic!("a transitional name: {transitional}"));
    assert!(
        !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()),
        "{pid}"
    );
    assert!(
        second.contains(&format!("where={transitional}")),
        "{second:?}"
    );
    let respawn_hc_name = hc_flag(&runs[1], "--name").unwrap().to_owned();
    assert_ne!(Some(respawn_hc_name.as_str()), hc_flag(&runs[0], "--name"));

    // P5.1: the old terminal is gone; the new one carries the name in both labels and
    // keeps the hc name of the transitional name.
    let terminals = herdr.terminals();
    let addresses: Vec<&str> = terminals
        .iter()
        .map(|t| t["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, vec!["t1", "t3"], "{terminals:#?}");
    let teammate = herdr.terminal("t3").unwrap();
    assert_eq!(label_of(&teammate, "tt.name"), "lead.tm1", "{teammate}");
    assert_eq!(label_of(&teammate, "where"), "lead.tm1", "{teammate}");
    assert_eq!(teammate["name"], respawn_hc_name.as_str(), "{teammate}");
    assert_eq!(label_of(&teammate, "tt.agent"), "claude", "{teammate}");
    assert_eq!(
        label_of(&teammate, "tt.claude_agent_name"),
        "r1",
        "{teammate}"
    );
    assert_eq!(label_of(&teammate, "tt.title"), "r1", "{teammate}");

    let rows = herdr.list();
    let tm1 = row(&rows, "lead.tm1").unwrap_or_else(|| panic!("lead.tm1: {rows:#?}"));
    assert_eq!(tm1["pane"], "t3", "{tm1}");
    assert_eq!(tm1["team"], id.as_str(), "{tm1}");
    assert_eq!(tm1["parent"], "lead", "{tm1}");
    assert_eq!(tm1["shim_id"], "%2", "{tm1}");
    assert_eq!(tm1["orphan"], false, "{tm1}");

    // P5.2: a create of the respawned teammate's name fails as taken and creates no
    // terminal; the Shim advances `k` past it.
    let taken = herdr.cli(&[
        "launch", "--name", "lead.tm1", "--split", "v", "--cmd", "cat",
    ]);
    assert_eq!(taken.status, 1, "{taken:?}");
    assert_eq!(herdr.runs().len(), 2, "no hc run for a taken name");
    let mut record = team.clone();
    record["next_teammate"] = Value::from(1);
    std::fs::write(
        team_dir.join("team.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let next = herdr.shim(
        &team_dir,
        &[],
        &[
            "split-window",
            "-d",
            "-t",
            "%1",
            "-P",
            "-F",
            "#{pane_id}",
            "--",
            "cat",
        ],
    );
    assert_eq!((next.status, next.stdout.as_str()), (0, "%3\n"), "{next:?}");
    let rows = herdr.list();
    let tm2 = row(&rows, "lead.tm2").unwrap_or_else(|| panic!("lead.tm2: {rows:#?}"));
    assert_eq!(tm2["shim_id"], "%3", "{tm2}");

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
    assert!(
        !out.stderr.contains("claude-teams: no team"),
        "{}",
        out.stderr
    );
    let report = herdr.scene.read_report("");
    assert_eq!(report["mismatches"], serde_json::json!([]), "{report:#}");
    let addresses: Vec<String> = herdr
        .terminals()
        .iter()
        .map(|t| t["address"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        addresses,
        vec!["t1"],
        "the clean exit closed every teammate"
    );
    assert!(!team_dir.exists(), "the team directory is gone");
}

/// Acceptance (PRD-261115-0921-01 P4.1, P3.5, PRD-261108-0447-01 P8.1, P8.3 on Herdr):
/// a `split-window` with `-e HC_TERMINAL_ID=x -e HC_SCOPE=y -e K=V` runs a teammate
/// command that sets `K=V` and neither Herdr variable; `capture-pane -p -S -10` prints
/// 10 history lines plus the visible screen, `-S -` all history, and no `-S` the
/// screen; `self --format json` in the teammate terminal gives `role: "teammate"`, the
/// team id, and `parent: "lead"`; after Claude exits 1 the teammate stays, `list`
/// reports it as an Orphan, and `kill --target` closes it.
#[test]
fn on_herdr_a_teammate_drops_the_herdr_variables_captures_history_and_can_be_an_orphan() {
    let herdr = HerdrScene::new("herdr-env");
    let mut child = herdr.start_launcher(&[("FAKE_CLAUDE_PAUSE", "1"), ("FAKE_CLAUDE_EXIT", "1")]);
    herdr.scene.wait_paused(&mut child);
    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");

    let out = herdr.shim(
        &team_dir,
        &[],
        &[
            "split-window",
            "-d",
            "-t",
            "%1",
            "-h",
            "-e",
            "HC_TERMINAL_ID=x",
            "-e",
            "HC_SCOPE=y",
            "-e",
            "K=V",
            "-P",
            "-F",
            "#{pane_id}",
            "--",
            "cat",
        ],
    );
    assert_eq!((out.status, out.stdout.as_str()), (0, "%2\n"), "{out:?}");
    let runs = herdr.runs();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let command = runs[0][runs[0].iter().position(|a| a == "--").unwrap() + 1..].join(" ");
    assert!(command.contains("K=V"), "{command}");
    for variable in ["HC_TERMINAL_ID", "HC_SCOPE"] {
        assert!(!command.contains(variable), "{variable} in {command}");
    }

    // P3.5: the screen is the last 5 of 40 lines.
    let address = herdr
        .terminals()
        .iter()
        .find(|t| label_of(t, "tt.shim_id") == "%2")
        .map(|t| t["address"].as_str().unwrap().to_owned())
        .unwrap();
    herdr.edit(&address, |terminal| {
        terminal["lines"] = (1..=40).map(|n| Value::from(format!("line{n}"))).collect();
        terminal["visible"] = Value::from(5);
    });
    let lines = |from: u32| {
        (from..=40)
            .map(|n| format!("line{n}\n"))
            .collect::<String>()
    };
    let capture = |args: &[&str]| {
        let mut all = vec!["capture-pane", "-p"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["-t", "%2"]);
        let out = herdr.shim(&team_dir, &[], &all);
        assert_eq!(out.status, 0, "{args:?}: {out:?}");
        out.stdout
    };
    assert_eq!(capture(&["-S", "-10"]), lines(26));
    assert_eq!(capture(&["-S", "-"]), lines(1));
    assert_eq!(capture(&[]), lines(36));

    // PRD-261108-0447-01 P8.1 on Herdr: `self` in the teammate's own terminal.
    let team_id = team_dir.file_name().unwrap().to_str().unwrap();
    let own = herdr
        .command(Path::new(BIN))
        .env("HC_TERMINAL_ID", &address)
        .args(["self", "--format", "json"])
        .output()
        .unwrap();
    assert!(own.status.success(), "self in the teammate: {own:?}");
    let own: Value = serde_json::from_slice(&own.stdout).unwrap();
    assert_eq!(
        (&own["role"], &own["team"], &own["parent"], &own["name"]),
        (
            &Value::from("teammate"),
            &Value::from(team_id),
            &Value::from("lead"),
            &Value::from("lead.tm1")
        ),
        "{own}"
    );

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 1, "{out:?}");
    let rows = herdr.list();
    let tm1 = row(&rows, "lead.tm1").unwrap_or_else(|| panic!("lead.tm1: {rows:#?}"));
    assert_eq!(tm1["orphan"], true, "{tm1}");
    herdr
        .cli(&["kill", "--target", "lead.tm1"])
        .assert_success("kill --target lead.tm1");
    assert!(herdr.terminal(&address).is_none(), "the orphan is closed");
}

/// Acceptance (PRD-261115-0921-01 P6.1): with the fake `hc` running each command, a
/// teammate whose command exits 0 was started with `--rm` and is gone from
/// `hc ls --all`; a teammate whose command exits 1 keeps a login shell and stays live.
/// The clean Leader exit closes it.
#[test]
fn on_herdr_a_teammate_that_exits_0_is_removed_and_one_that_fails_keeps_a_shell() {
    let herdr = HerdrScene::new("herdr-rm");
    let kept = herdr.scene.dir.join("shell-kept");
    let shell = herdr.scene.bin().join("kept-shell");
    write_executable(
        &shell,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexec sleep 60\n",
            kept.display()
        ),
    );
    let shell = shell.display().to_string();
    let exec = [("FAKE_HC_EXEC", "1"), ("SHELL", shell.as_str())];
    let mut child = herdr.start_launcher(&[("FAKE_CLAUDE_PAUSE", "1"), exec[0]]);
    herdr.scene.wait_paused(&mut child);
    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");

    let wait = |what: &str, done: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            thread::sleep(Duration::from_millis(50));
        }
    };
    // The address the fake gives the next terminal.
    let split = |command: &str| {
        let state: Value =
            serde_json::from_slice(&std::fs::read(herdr.hc_state()).unwrap()).unwrap();
        let address = format!("t{}", state["next"]);
        let out = herdr.shim(
            &team_dir,
            &exec,
            &["split-window", "-d", "-t", "%1", "--", command],
        );
        assert_eq!(out.status, 0, "{out:?}");
        address
    };

    let ok = split("sh -c 'exit 0'");
    let runs = herdr.runs();
    assert!(runs[0].iter().any(|arg| arg == "--rm"), "{:?}", runs[0]);
    wait("the exit-0 teammate is removed", &|| {
        herdr.terminal(&ok).is_none()
    });

    let failed = split("sh -c 'exit 1'");
    wait("the failed teammate starts a shell", &|| kept.exists());
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "-l\n");
    let terminal = herdr.terminal(&failed).expect("the failed teammate stays");
    assert_eq!(terminal["state"], "live", "{terminal}");

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
    assert!(
        herdr.terminal(&failed).is_none(),
        "the clean exit closed it"
    );
}

/// Acceptance (PRD-261115-0921-01 P5.1, P6.1, PRD-261108-0447-01 P6.2): a
/// `respawn-pane -k` whose new command exits 0 before the Shim writes the name (the
/// fake `hc` answers `run` only after the command exits) exits 0 with no output: the
/// create succeeded, Herdr removed the new terminal (`--rm`), and the old terminal is
/// closed.
#[test]
fn on_herdr_a_respawn_whose_command_exits_0_at_once_succeeds() {
    let herdr = HerdrScene::new("herdr-fast-respawn");
    let mut child = herdr.start_launcher(&[("FAKE_CLAUDE_PAUSE", "1")]);
    herdr.scene.wait_paused(&mut child);
    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");

    let split = herdr.shim(
        &team_dir,
        &[],
        &["split-window", "-d", "-t", "%1", "--", "cat"],
    );
    assert_eq!(split.status, 0, "{split:?}");
    let respawn = herdr.shim(
        &team_dir,
        &[("FAKE_HC_EXEC", "wait")],
        &["respawn-pane", "-k", "-t", "%2", "--", "true"],
    );
    assert_eq!(
        (
            respawn.status,
            respawn.stdout.as_str(),
            respawn.stderr.as_str()
        ),
        (0, "", ""),
        "{respawn:?}"
    );
    let addresses: Vec<String> = herdr
        .terminals()
        .iter()
        .map(|t| t["address"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        addresses,
        vec!["t1"],
        "the old terminal is closed and the new one removed"
    );

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
}

/// M3 (PRD-261115-0921-01 P5.1, P6.1, PRD-261109-2223-01 P7.4): on Herdr, `hc kill`
/// of a `--rm` Teammate terminal removes it and answers `not_found`; the close still
/// succeeds. Through the Shim, a `respawn-pane -k` after a `split-window` with no
/// command exits 0 with no output and keeps the new terminal, and a `kill-pane` of
/// the teammate exits 0 and closes it.
#[test]
fn on_herdr_the_shim_closes_a_rm_teammate_that_hc_kill_reports_removed() {
    let herdr = HerdrScene::new("herdr-rm-close-shim");
    let mut child = herdr.start_launcher(&[("FAKE_CLAUDE_PAUSE", "1")]);
    herdr.scene.wait_paused(&mut child);
    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");
    let addresses = || -> Vec<String> {
        herdr
            .terminals()
            .iter()
            .map(|t| t["address"].as_str().unwrap().to_owned())
            .collect()
    };

    let split = herdr.shim(
        &team_dir,
        &[],
        &["split-window", "-d", "-t", "%1", "-P", "-F", "#{pane_id}"],
    );
    assert_eq!(
        (split.status, split.stdout.as_str()),
        (0, "%2\n"),
        "{split:?}"
    );
    let runs = herdr.runs();
    assert!(runs[0].iter().any(|arg| arg == "--rm"), "{:?}", runs[0]);

    let respawn = herdr.shim(
        &team_dir,
        &[],
        &["respawn-pane", "-k", "-t", "%2", "--", "cat"],
    );
    assert_eq!(
        (
            respawn.status,
            respawn.stdout.as_str(),
            respawn.stderr.as_str()
        ),
        (0, "", ""),
        "{respawn:?}"
    );
    assert_eq!(addresses(), vec!["t1", "t3"], "the new terminal stays");
    let teammate = herdr.terminal("t3").unwrap();
    assert_eq!(label_of(&teammate, "tt.name"), "lead.tm1", "{teammate}");

    let kill = herdr.shim(&team_dir, &[], &["kill-pane", "-t", "%2"]);
    assert_eq!(
        (kill.status, kill.stdout.as_str(), kill.stderr.as_str()),
        (0, "", ""),
        "{kill:?}"
    );
    assert_eq!(addresses(), vec!["t1"], "kill-pane closed the teammate");

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
}

/// M3 (PRD-261109-2223-01 P7.4, PRD-261115-0921-01 P6.1): `tmux-tools kill --target`
/// of a `--rm` Teammate terminal, which `hc kill` reports as removed, exits 0 and
/// closes it.
#[test]
fn on_herdr_kill_target_closes_a_rm_teammate_that_hc_kill_reports_removed() {
    let herdr = HerdrScene::new("herdr-rm-close-kill");
    let mut child = herdr.start_launcher(&[("FAKE_CLAUDE_PAUSE", "1")]);
    herdr.scene.wait_paused(&mut child);
    let team_dir = herdr
        .scene
        .team_dirs()
        .pop()
        .expect("a team directory on herdr");

    let split = herdr.shim(
        &team_dir,
        &[],
        &["split-window", "-d", "-t", "%1", "--", "cat"],
    );
    assert_eq!(split.status, 0, "{split:?}");
    let runs = herdr.runs();
    assert!(runs[0].iter().any(|arg| arg == "--rm"), "{:?}", runs[0]);
    assert!(herdr.terminal("t2").is_some(), "the teammate is t2");

    herdr
        .cli(&["kill", "--target", "lead.tm1"])
        .assert_success("kill --target lead.tm1");
    assert!(herdr.terminal("t2").is_none(), "kill --target closed it");

    herdr.scene.go();
    let out = finish(child);
    assert_eq!(out.status, 0, "{out:?}");
}
