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
/// - Else it is the Leader: it writes `<report>.args`/`.env`, runs `TT_BIN self`
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

// --- P10.2: replay of the trace fixture ---------------------------------------------

/// Acceptance: replaying the 2.1.319 fixture through the Shim inside a Team from pane
/// `lead`, every call's stdout and exit equal the filled fixture line, and `list`
/// shows `lead.tm1` with the team id, `parent: "lead"`, `shim_id: "%2"`,
/// `orphan: false` before the fixture's final `kill-pane`. Every fixture line, that
/// `kill-pane` included, is replayed and compared; after the clean exit no Teammate
/// pane is left and the team directory is gone (P2.5).
#[test]
fn the_fixture_replays_and_a_clean_exit_closes_the_team() {
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("replay");
    let lead = launch_pane(&server, "lead");
    let fixture = fixture("2.1.319");
    let fixture_path = fixture.display().to_string();

    let mut child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
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
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("leftopen");
    let lead = launch_pane(&server, "lead");
    let fixture_path = fixture("2.1.319").display().to_string();
    let child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
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
    let _serial = serial_guard();
    let server = IsolatedTmuxServer::start_inside();
    let scene = Scene::new("crash");
    let lead = launch_pane(&server, "lead");
    let fixture_path = fixture("2.1.319").display().to_string();

    let child = start_launcher(
        &server,
        &scene,
        &lead,
        &[
            ("FAKE_CLAUDE_FIXTURE", &fixture_path),
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
