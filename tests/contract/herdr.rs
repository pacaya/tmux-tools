//! The Herdr backend's contract tests (PRD-261109-2223-01 P8).
//!
//! The default run drives the binary against a fake `hc` (`fake_hc.py`, P8.2); no real
//! herdr-core is reached. With `TT_TEST_HERDR=1` and a real `hc` on the original
//! `PATH`, [`herdr_private_core_contract_suite`] runs the required verbs against a
//! private core `HC_SESSION=tt-tests-<pid>` and, also on failure, stops that core and
//! deletes its state directory (P8.3). It never reaches the default core.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{json_payload, original_environment, tempfile_dir, wait_for_child, CmdOutput, BIN};

const FAKE_HC: &str = include_str!("fake_hc.py");

fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in `hc` first on the CLI's `PATH`, its terminals, and the run's dirs.
struct FakeHc {
    dir: PathBuf,
}

impl FakeHc {
    fn start(label: &str) -> FakeHc {
        let dir = tempfile_dir(label);
        for sub in ["bin", "state", "config", "work"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/hc"), FAKE_HC);
        let fake = FakeHc { dir };
        fake.save(&json!({"next": 1, "terminals": []}));
        fake
    }

    /// A fake whose calling terminal `t1` carries `labels`.
    fn with_caller(label: &str, labels: Value) -> FakeHc {
        let fake = FakeHc::start(label);
        fake.add_terminal(json!({"labels": labels}));
        fake
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.state_path()).unwrap()).unwrap()
    }

    fn save(&self, state: &Value) {
        std::fs::write(self.state_path(), serde_json::to_vec(state).unwrap()).unwrap();
    }

    /// Add a live terminal with `fields` over the defaults; returns its address.
    fn add_terminal(&self, fields: Value) -> String {
        let mut state = self.state();
        let next = state["next"].as_u64().unwrap();
        let address = format!("t{next}");
        let mut terminal = json!({
            "address": address, "name": null, "cwd": "/", "argv": ["sh"], "labels": {},
            "state": "live", "status": null, "status_ms": null, "turn": 0, "lines": [],
            "how": "history", "complete": true, "keys": [], "sent": [], "on_send": {},
        });
        for (key, value) in fields.as_object().unwrap() {
            terminal[key] = value.clone();
        }
        state["terminals"].as_array_mut().unwrap().push(terminal);
        state["next"] = json!(next + 1);
        self.save(&state);
        address
    }

    fn terminal(&self, address: &str) -> Option<Value> {
        self.state()["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|terminal| terminal["address"] == address)
            .cloned()
    }

    fn edit(&self, address: &str, change: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        let terminal = state["terminals"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|terminal| terminal["address"] == address)
            .expect("the terminal exists");
        change(terminal);
        self.save(&state);
    }

    fn log(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.dir.join("log.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn clear_log(&self) {
        let _ = std::fs::remove_file(self.dir.join("log.jsonl"));
    }

    /// Every logged call of the `hc` command `name`.
    fn calls(&self, name: &str) -> Vec<Vec<String>> {
        self.log()
            .into_iter()
            .filter(|call| call.first().map(String::as_str) == Some(name))
            .collect()
    }

    /// Run the CLI on Herdr, selected by `HC_TERMINAL_ID=t1`, with `env` added and
    /// `remove` removed. `TMUX` is unset.
    fn cli_with(&self, args: &[&str], env: &[(&str, &str)], remove: &[&str]) -> CmdOutput {
        let path = format!(
            "{}:{}",
            self.dir.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(BIN);
        command
            .args(args)
            .current_dir(self.dir.join("work"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("PATH", path)
            .env("HC_TERMINAL_ID", "t1")
            .env("SHELL", "/bin/sh")
            .env("XDG_STATE_HOME", self.dir.join("state"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("FAKE_HC_STATE", self.state_path())
            .env("FAKE_HC_LOG", self.dir.join("log.jsonl"));
        for (key, value) in env {
            command.env(key, value);
        }
        for key in remove {
            command.env_remove(key);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tmux-tools");
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            Duration::from_secs(30),
        )
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        self.cli_with(args, &[], &[])
    }
}

impl Drop for FakeHc {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The value after `flag` in one logged call.
fn flag_value<'a>(call: &'a [String], flag: &str) -> Option<&'a str> {
    let at = call.iter().position(|arg| arg == flag)?;
    call.get(at + 1).map(String::as_str)
}

/// Every `--label` value of one logged call.
fn labels_of(call: &[String]) -> Vec<String> {
    call.windows(2)
        .filter(|pair| pair[0] == "--label")
        .map(|pair| pair[1].clone())
        .collect()
}

/// The command after `--` of one logged call.
fn command_of(call: &[String]) -> Vec<String> {
    let at = call.iter().position(|arg| arg == "--").expect("a `--`");
    call[at + 1..].to_vec()
}

// --- P2: selection ---------------------------------------------------------------------

/// Acceptance: with a fake `hc` and `HC_TERMINAL_ID` set and no `$TMUX`, `self`
/// reports `host: "herdr"`; P2.6: a stale `TMUX_PANE` is never read for the calling
/// pane (`self`, `status`).
#[test]
fn hc_terminal_id_selects_herdr_and_the_calling_pane_is_that_terminal() {
    let fake = FakeHc::with_caller(
        "herdr-select",
        json!({"tt.name": "lead", "herdr.space": "S"}),
    );
    let stale = [("TMUX_PANE", "%9")];
    let report = json_payload(
        &fake.cli_with(&["self", "--format", "json"], &stale, &[]),
        "self",
    );
    assert_eq!(report["host"], "herdr", "{report}");
    assert_eq!(report["driven"], true, "{report}");
    assert_eq!(report["name"], "lead", "{report}");

    let status = json_payload(
        &fake.cli_with(&["status", "--format", "json"], &stale, &[]),
        "status",
    );
    assert_eq!(status["pane_id"], "t1", "{status}");
    assert_eq!(status["session"], "S", "{status}");
    assert!(status["window"].is_null(), "{status}");
    assert!(status["host_word"].is_null(), "{status}");

    // P7.2: without `HC_TERMINAL_ID`, `self` exits 1 naming it.
    let out = fake.cli_with(
        &["self", "--format", "json"],
        &[("TMUX_TOOLS_HOST", "herdr")],
        &["HC_TERMINAL_ID"],
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(out.stderr.contains("HC_TERMINAL_ID"), "{}", out.stderr);
}

/// P2.1, P2.3: `TMUX_TOOLS_HOST` names all three hosts; a missing `hc` or a failing
/// `hc server status` fails with exit 1 naming herdr and the selection source.
#[test]
fn an_unreachable_herdr_names_the_host_and_the_selection_source() {
    let fake = FakeHc::with_caller("herdr-down", json!({}));
    let invalid = fake.cli_with(&["list"], &[("TMUX_TOOLS_HOST", "rex")], &[]);
    assert_eq!(invalid.status, 1, "{invalid:?}");
    for host in ["tmux", "orca", "herdr"] {
        assert!(invalid.stderr.contains(host), "{}", invalid.stderr);
    }

    let down = fake.cli_with(&["list"], &[("FAKE_HC_DOWN", "1")], &[]);
    assert_eq!(down.status, 1, "{down:?}");
    assert!(
        down.stderr.contains("herdr") && down.stderr.contains("HC_TERMINAL_ID"),
        "{}",
        down.stderr
    );

    let without_hc = Command::new(BIN)
        .args(["self", "--format", "json"])
        .env_remove("TMUX")
        .env("HC_TERMINAL_ID", "t1")
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    assert_eq!(without_hc.status.code(), Some(1), "{without_hc:?}");
    let stderr = String::from_utf8_lossy(&without_hc.stderr);
    assert!(
        stderr.contains("herdr") && stderr.contains("HC_TERMINAL_ID"),
        "{stderr}"
    );
}

// --- P3: placement ---------------------------------------------------------------------

/// Acceptance: `launch --split v --name A.b` runs one `hc run` with the P3.4 flags,
/// the hc name, and the labels of the calling terminal's space and tab; with no
/// `--split` only the space. `--rm` is never used.
#[test]
fn launch_places_the_terminal_in_the_callers_space_and_tab() {
    let fake = FakeHc::with_caller("herdr-place", json!({"herdr.space": "S", "herdr.tab": "T"}));
    fake.cli(&[
        "launch", "--split", "v", "--name", "A.b", "--cmd", "sleep 30",
    ])
    .assert_success("launch --split v");
    let runs = fake.calls("run");
    assert_eq!(runs.len(), 1, "{runs:?}");
    let run = &runs[0];
    assert!(run.contains(&"--outlive-parent".to_owned()), "{run:?}");
    assert!(run.contains(&"--no-notify".to_owned()), "{run:?}");
    assert!(!run.contains(&"--rm".to_owned()), "{run:?}");
    // tt- and the first 16 hex of SHA-256("A.b") (`printf A.b | shasum -a 256`).
    assert_eq!(
        flag_value(run, "--name"),
        Some("tt-4ff54a775f90577e"),
        "{run:?}"
    );
    let labels = labels_of(run);
    for label in ["herdr.space=S", "herdr.tab=T", "where=A.b", "tt.name=A.b"] {
        assert!(labels.contains(&label.to_owned()), "{label}: {labels:?}");
    }

    fake.clear_log();
    fake.cli(&["launch", "--name", "c.d", "--cmd", "sleep 30"])
        .assert_success("launch without --split");
    let labels = labels_of(&fake.calls("run")[0]);
    assert!(labels.contains(&"herdr.space=S".to_owned()), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.starts_with("herdr.tab=")),
        "{labels:?}"
    );
}

/// P3.1: with `--target`, the target's labels; with no calling terminal, the
/// `TMUX_TOOLS_HERDR_*` variables; with no space anywhere, neither label.
#[test]
fn placement_labels_come_from_the_target_else_the_environment() {
    let fake = FakeHc::with_caller("herdr-place-src", json!({"herdr.space": "S"}));
    let other = fake.add_terminal(json!({"labels": {"herdr.space": "S2", "herdr.tab": "T2"}}));
    fake.cli(&[
        "launch", "--split", "h", "--target", &other, "--name", "x1", "--cmd", "true",
    ])
    .assert_success("launch --target");
    let labels = labels_of(&fake.calls("run")[0]);
    assert!(
        labels.contains(&"herdr.space=S2".to_owned())
            && labels.contains(&"herdr.tab=T2".to_owned()),
        "{labels:?}"
    );

    fake.clear_log();
    fake.cli_with(
        &["launch", "--split", "v", "--name", "x2", "--cmd", "true"],
        &[
            ("TMUX_TOOLS_HOST", "herdr"),
            ("TMUX_TOOLS_HERDR_SPACE", "ES"),
            ("TMUX_TOOLS_HERDR_TAB", "ET"),
        ],
        &["HC_TERMINAL_ID"],
    )
    .assert_success("launch from the environment");
    let labels = labels_of(&fake.calls("run")[0]);
    assert!(
        labels.contains(&"herdr.space=ES".to_owned())
            && labels.contains(&"herdr.tab=ET".to_owned()),
        "{labels:?}"
    );

    fake.clear_log();
    fake.cli_with(
        &["launch", "--split", "v", "--name", "x3", "--cmd", "true"],
        &[("TMUX_TOOLS_HOST", "herdr"), ("TMUX_TOOLS_HERDR_TAB", "ET")],
        &["HC_TERMINAL_ID"],
    )
    .assert_success("launch with no space");
    let labels = labels_of(&fake.calls("run")[0]);
    assert!(
        !labels.iter().any(|label| label.starts_with("herdr.")),
        "{labels:?}"
    );
}

/// Acceptance (P3.3): `rebalance`, `--split tile`, `--size`, `--session` and
/// `--window` exit 3 with the P5.3 message and no `hc run`.
#[test]
fn optional_placement_exits_3_on_herdr_without_a_run() {
    let fake = FakeHc::with_caller("herdr-exit3", json!({}));
    for (args, message) in [
        (vec!["rebalance"], "unsupported: rebalance on herdr"),
        (
            vec!["launch", "--split", "tile", "--cmd", "true"],
            "unsupported: --split tile on herdr",
        ),
        (
            vec!["launch", "--size", "30", "--cmd", "true"],
            "unsupported: --size on herdr",
        ),
        (
            vec!["launch", "--session", "s", "--cmd", "true"],
            "unsupported: --session on herdr",
        ),
        (
            vec!["launch", "--window", "w", "--cmd", "true"],
            "unsupported: --window on herdr",
        ),
        (
            vec!["capture", "--window", "w"],
            "unsupported: --window on herdr",
        ),
    ] {
        let out = fake.cli(&args);
        assert_eq!(out.status, 3, "{args:?}: {out:?}");
        assert!(out.stderr.contains(message), "{args:?}: {}", out.stderr);
    }
    assert!(fake.calls("run").is_empty(), "{:?}", fake.log());
}

// --- P4: registration, names, and lifetime -----------------------------------------------

/// P4.1–P4.2: the registration is `tt.*` labels; a taken name fails with exit 1 and
/// creates no terminal.
#[test]
fn a_duplicate_live_name_fails_and_creates_no_terminal() {
    let fake = FakeHc::with_caller("herdr-dup", json!({"tt.name": "lead"}));
    fake.cli(&["launch", "--name", "dup", "--cmd", "sleep 30"])
        .assert_success("launch dup");
    let first = fake.calls("run");
    let labels = labels_of(&first[0]);
    for label in ["tt.parent=lead", "tt.launched_at=", "tt.cwd="] {
        assert!(
            labels.iter().any(|value| value.starts_with(label)),
            "{label}: {labels:?}"
        );
    }
    let terminals = fake.state()["terminals"].as_array().unwrap().len();
    let out = fake.cli(&["launch", "--name", "dup", "--cmd", "sleep 30"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert_eq!(
        fake.state()["terminals"].as_array().unwrap().len(),
        terminals
    );

    // hc's own `name_taken` also fails the spawn: a live terminal holds the hc name
    // tt-<16 hex of SHA-256("dup2")> (`printf dup2 | shasum -a 256`).
    fake.add_terminal(json!({"name": "tt-64dea6ce587eb07f"}));
    let terminals = fake.state()["terminals"].as_array().unwrap().len();
    let out = fake.cli(&["launch", "--name", "dup2", "--cmd", "sleep 30"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert_eq!(
        fake.state()["terminals"].as_array().unwrap().len(),
        terminals
    );
}

/// Acceptance (P4.4): a finished terminal carrying `tt.name=X` is dead. `kill --target
/// X` exits 1 and acts on no terminal; `launch --bare --name X` removes the old
/// terminal by address and spawns.
#[test]
fn a_dead_name_is_removed_and_never_resolves_to_another_terminal() {
    let fake = FakeHc::with_caller("herdr-dead", json!({}));
    let dead =
        fake.add_terminal(json!({"name": "tt-x", "state": "finished", "labels": {"tt.name": "X"}}));
    // A live terminal named by the address form must not be reached either.
    fake.add_terminal(json!({"labels": {"tt.name": "other"}}));

    let out = fake.cli(&["kill", "--target", "X"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(fake.calls("kill").is_empty(), "{:?}", fake.log());
    assert!(
        fake.terminal(&dead).is_none(),
        "the dead terminal is removed"
    );
    let rms = fake.calls("rm");
    assert_eq!(rms.len(), 1, "{rms:?}");
    assert!(rms[0].contains(&dead), "{rms:?}");

    let dead =
        fake.add_terminal(json!({"name": "tt-x", "state": "finished", "labels": {"tt.name": "X"}}));
    fake.clear_log();
    fake.cli(&["launch", "--bare", "--name", "X", "--cmd", "true"])
        .assert_success("respawn of a dead name");
    let log = fake.log();
    let rm_at = log
        .iter()
        .position(|call| call[0] == "rm" && call.contains(&dead));
    let run_at = log.iter().position(|call| call[0] == "run");
    assert!(
        rm_at.is_some() && run_at.is_some() && rm_at < run_at,
        "{log:?}"
    );
    assert_eq!(
        command_of(&fake.calls("run")[0]),
        vec!["true".to_owned()],
        "--bare runs the command bare"
    );
}

/// P4.4, P7.3: `list` shows each dead name once with `status: dead`, removes it, and
/// lists live terminals with the hc address and the space label (no `window` key).
#[test]
fn list_shows_live_terminals_and_each_dead_name_once() {
    let fake = FakeHc::with_caller("herdr-list", json!({}));
    let live = fake.add_terminal(json!({"labels": {"tt.name": "a", "herdr.space": "S"}}));
    fake.add_terminal(json!({"state": "finished", "labels": {"tt.name": "gone"}}));
    fake.add_terminal(json!({"labels": {}}));
    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let row = rows.iter().find(|row| row["name"] == "a").unwrap();
    assert_eq!(row["pane"], live.as_str(), "{row}");
    assert_eq!(row["session"], "S", "{row}");
    assert_eq!(row["status"], "running", "{row}");
    assert_eq!(row["host"], "herdr", "{row}");
    assert!(row.get("window").is_none(), "{row}");
    let gone = rows.iter().find(|row| row["name"] == "gone").unwrap();
    assert_eq!(gone["status"], "dead", "{gone}");
    assert!(gone["session"].is_null(), "{gone}");

    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list again");
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
}

/// P4.4: resolving or spawning one name removes only the dead terminals that carry
/// that name; every other dead name stays for `list` to report once as `status: dead`.
#[test]
fn resolving_or_spawning_one_name_leaves_other_dead_names_for_list() {
    let fake = FakeHc::with_caller("herdr-dead-scope", json!({}));
    let dead =
        |name: &str| fake.add_terminal(json!({"state": "finished", "labels": {"tt.name": name}}));
    let resolved = dead("job");
    let respawned = dead("again");
    let kept = dead("keep");

    let out = fake.cli(&["status", "--target", "job"]);
    assert_eq!(out.status, 1, "{out:?}");
    fake.cli(&["launch", "--bare", "--name", "again", "--cmd", "true"])
        .assert_success("respawn of one dead name");
    fake.cli(&["launch", "--bare", "--cmd", "true"])
        .assert_success("spawn with a derived name");
    let removed: Vec<String> = fake
        .calls("rm")
        .iter()
        .map(|call| call.last().unwrap().clone())
        .collect();
    assert_eq!(removed, [resolved, respawned], "{:?}", fake.log());
    assert!(fake.terminal(&kept).is_some(), "the other dead name stays");

    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
    let dead_rows: Vec<&Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["status"] == "dead")
        .collect();
    assert_eq!(dead_rows.len(), 1, "{rows}");
    assert_eq!(dead_rows[0]["name"], "keep", "{rows}");
    assert!(fake.terminal(&kept).is_none(), "list removes it");
}

/// P4.5, P7.4: on Herdr, Ownership needs `tt.name`; a terminal labelled only
/// `tt.agent` is refused without `--any` and gets no `hc kill` or `hc rm`.
#[test]
fn kill_refuses_an_agent_only_terminal_without_any() {
    let fake = FakeHc::with_caller("herdr-agent-only", json!({}));
    let agent_only = fake.add_terminal(json!({"labels": {"tt.agent": "claude"}}));
    let out = fake.cli(&["kill", "--target", &agent_only]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("not created by tmux-tools"),
        "{}",
        out.stderr
    );
    assert!(fake.calls("kill").is_empty(), "{:?}", fake.log());
    assert!(fake.calls("rm").is_empty(), "{:?}", fake.log());
    assert!(fake.terminal(&agent_only).is_some());
}

/// P7.4: `kill` applies the self-pane refusal and the Ownership check, then runs `hc
/// kill` and `hc rm`.
#[test]
fn kill_refuses_the_calling_and_unowned_terminals_then_kills_and_removes() {
    let fake = FakeHc::with_caller("herdr-kill", json!({"tt.name": "lead"}));
    let unowned = fake.add_terminal(json!({"labels": {}}));
    let out = fake.cli(&["kill", "--target", "lead"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(out.stderr.contains("calling pane"), "{}", out.stderr);
    let out = fake.cli(&["kill", "--target", &unowned]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("not created by tmux-tools"),
        "{}",
        out.stderr
    );
    assert!(fake.calls("kill").is_empty(), "{:?}", fake.log());

    fake.cli(&["launch", "--name", "k", "--cmd", "sleep 30"])
        .assert_success("launch k");
    fake.clear_log();
    fake.cli(&["kill", "--target", "k"])
        .assert_success("kill k");
    let order: Vec<String> = fake
        .log()
        .into_iter()
        .map(|call| call[0].clone())
        .filter(|name| name == "kill" || name == "rm")
        .collect();
    assert_eq!(order, ["kill", "rm"], "{:?}", fake.log());
}

/// P7.5: `launch` keeps a shell after the command; `--bare` runs it so the terminal
/// finishes with it. `hc run` gets the invoking cwd.
#[test]
fn launch_keeps_the_shell_unless_bare() {
    let fake = FakeHc::with_caller("herdr-keep", json!({}));
    fake.cli(&["launch", "--name", "keep", "--cmd", "make test"])
        .assert_success("launch");
    let run = &fake.calls("run")[0];
    assert_eq!(
        command_of(run),
        vec!["(make test); exec /bin/sh".to_owned()],
        "{run:?}"
    );
    let work = std::fs::canonicalize(fake.dir.join("work")).unwrap();
    assert_eq!(
        flag_value(run, "--cwd"),
        Some(work.to_str().unwrap()),
        "{run:?}"
    );
}

// --- P5: send and keys -----------------------------------------------------------------------

/// Acceptance (P5.2): a character outside the typeable set exits 1 with
/// `cannot type on herdr: U+<XXXX>` and sends no keys; key names and literal text are
/// `hc keys` names, and `--enter` adds `enter`.
#[test]
fn send_types_hc_key_names_and_refuses_untypeable_input() {
    let fake = FakeHc::with_caller("herdr-send", json!({}));
    let pane = fake.add_terminal(json!({"labels": {"tt.name": "p"}}));
    let out = fake.cli(&["send", "--target", "p", "hi!"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("cannot type on herdr: U+0021"),
        "{}",
        out.stderr
    );
    let out = fake.cli(&["send", "--target", "p", "KP0"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("cannot type on herdr: KP0"),
        "{}",
        out.stderr
    );
    assert!(fake.calls("keys").is_empty(), "{:?}", fake.log());

    fake.cli(&["send", "--target", "p", "Hi x", "--enter"])
        .assert_success("literal send");
    fake.cli(&["send", "--target", "p", "C-c"])
        .assert_success("key send");
    fake.cli(&["send-enter", "--target", "p"])
        .assert_success("send-enter");
    fake.cli(&["escape", "--target", "p", "--any"])
        .assert_success("escape");
    let keys: Vec<String> = fake.terminal(&pane).unwrap()["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        keys,
        ["shift+h", "i", "space", "x", "enter", "ctrl+c", "enter", "esc"],
        "{:?}",
        fake.log()
    );

    // A key hc still rejects ends the send with the same message.
    let out = fake.cli_with(
        &["send", "--target", "p", "F12"],
        &[("FAKE_HC_REJECT_KEYS", "f12")],
        &[],
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("cannot type on herdr: F12"),
        "{}",
        out.stderr
    );
}

// --- P6: Host state and prompt ---------------------------------------------------------------

/// P6.2–P6.3: `status` reports `host_word` and the mapped Host state; `error` is
/// `blocked`.
#[test]
fn status_reports_the_hc_status_word_and_its_host_state() {
    let fake = FakeHc::with_caller("herdr-status", json!({}));
    let pane = fake.add_terminal(
        json!({"labels": {"tt.name": "w", "herdr.tab": "T"}, "status": "working", "status_ms": 5}),
    );
    let status = json_payload(
        &fake.cli(&["status", "--target", "w", "--format", "json"]),
        "status",
    );
    assert_eq!(status["host_word"], "working", "{status}");
    assert_eq!(status["host_state"], "working", "{status}");
    assert_eq!(status["window"], "T", "{status}");
    assert_eq!(status["pane_id"], pane.as_str(), "{status}");
    for key in [
        "window_index",
        "pane",
        "min_pane_cols",
        "min_pane_rows",
        "below_floor",
    ] {
        assert!(status[key].is_null(), "{key}: {status}");
    }
    fake.edit(&pane, |terminal| terminal["status"] = json!("error"));
    let status = json_payload(
        &fake.cli(&["status", "--target", "w", "--format", "json"]),
        "status",
    );
    assert_eq!(status["host_word"], "error", "{status}");
    assert_eq!(status["host_state"], "blocked", "{status}");
    // P6.2: a wait ends on `blocked` (from `error`).
    let waited = json_payload(
        &fake.cli(&[
            "wait-idle",
            "--target",
            "w",
            "--timeout",
            "5",
            "--format",
            "json",
        ]),
        "wait-idle",
    );
    assert_eq!(waited["host_state"], "blocked", "{waited}");
    fake.edit(&pane, |terminal| terminal["status"] = json!("thinking"));
    let status = json_payload(
        &fake.cli(&["status", "--target", "w", "--format", "json"]),
        "status",
    );
    assert_eq!(status["host_word"], "thinking", "{status}");
    assert!(status["host_state"].is_null(), "{status}");
}

/// Acceptance (P6.5): a terminal that reports `working` before and after the send with
/// an unchanged `turn` gives `submit: accepted`; a `turn` that goes up gives
/// `confirmed`; a blocked program refuses with `prompt target blocked:`.
#[test]
fn prompt_submit_is_confirmed_only_by_a_new_turn_or_a_new_working() {
    let fake = FakeHc::with_caller("herdr-prompt", json!({}));
    let pane = fake.add_terminal(json!({
        "labels": {"tt.name": "a"}, "status": "working", "status_ms": 5, "lines": ["$ ready"],
    }));
    let out = fake.cli(&[
        "prompt",
        "--target",
        "a",
        "--timeout",
        "1",
        "--format",
        "json",
        "hello",
    ]);
    let result = json_payload(&out, "prompt while working");
    assert_eq!(result["submit"], "accepted", "{result}");
    assert_eq!(fake.terminal(&pane).unwrap()["sent"], json!(["hello"]));

    fake.edit(&pane, |terminal| {
        terminal["status"] = json!("idle");
        terminal["on_send"] = json!({"turn": 1, "status": "done", "append": ["REPLY"]});
    });
    let out = fake.cli(&[
        "prompt",
        "--target",
        "a",
        "--timeout",
        "5",
        "--format",
        "json",
        "again",
    ]);
    let result = json_payload(&out, "prompt with a new turn");
    assert_eq!(result["submit"], "confirmed", "{result}");
    assert_eq!(result["one_submission_delivery"], "guaranteed", "{result}");
    assert_eq!(result["host_state"], "done", "{result}");
    assert_eq!(result["complete"], true, "{result}");
    assert!(
        result["output_since_prompt"]
            .as_str()
            .unwrap()
            .contains("REPLY"),
        "{result}"
    );

    fake.edit(&pane, |terminal| terminal["status"] = json!("blocked"));
    let out = fake.cli(&["prompt", "--target", "a", "--timeout", "1", "blocked"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("prompt target blocked: "),
        "{}",
        out.stderr
    );
    assert!(
        !fake
            .log()
            .iter()
            .any(|call| call.contains(&"--force".to_owned())),
        "{:?}",
        fake.log()
    );
}

/// Acceptance (P6.6): a multi-line prompt to a pane with no Host state exits 1 and
/// sends nothing.
#[test]
fn a_multi_line_prompt_to_a_pane_without_host_state_is_refused() {
    let fake = FakeHc::with_caller("herdr-multiline", json!({}));
    let pane = fake.add_terminal(json!({"labels": {"tt.name": "sh"}}));
    let out = fake.cli(&["prompt", "--target", "sh", "--timeout", "1", "one\ntwo"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(fake.calls("send").is_empty(), "{:?}", fake.log());
    assert_eq!(fake.terminal(&pane).unwrap()["sent"], json!([]));
}

/// P5.1: Completeness is false when the history read is not a complete history read,
/// and the `capture --all` notice fires on `how: screen`.
#[test]
fn completeness_and_the_scrollback_notice_follow_the_hc_read() {
    let fake = FakeHc::with_caller("herdr-complete", json!({}));
    fake.add_terminal(json!({
        "labels": {"tt.name": "s"}, "lines": ["a", "b"], "how": "stitched", "complete": true,
        "status": "idle", "status_ms": 1, "on_send": {"status": "done", "append": ["c"]},
    }));
    let result = json_payload(
        &fake.cli(&[
            "prompt",
            "--target",
            "s",
            "--timeout",
            "5",
            "--format",
            "json",
            "go",
        ]),
        "prompt",
    );
    assert_eq!(result["complete"], false, "{result}");
    assert_eq!(result["output_since_prompt"], "go\nc", "{result}");

    fake.edit("t2", |terminal| terminal["how"] = json!("screen"));
    let out = fake.cli(&["capture", "--target", "s", "--all"]);
    out.assert_success("capture --all");
    assert!(out.stderr.contains("no scrollback"), "{}", out.stderr);
    let out = fake.cli(&["capture", "--target", "s", "--lines", "1"]);
    assert_eq!(out.stdout, "c", "{out:?}");
}

/// P5.1: both reads are complete history reads, but the anchor line moved between
/// the mark and the settle read, so the result is not complete.
#[test]
fn a_moved_anchor_line_makes_the_prompt_result_incomplete() {
    let fake = FakeHc::with_caller("herdr-anchor", json!({}));
    fake.add_terminal(json!({
        "labels": {"tt.name": "s"}, "lines": ["a", "b"], "how": "history", "complete": true,
        "status": "idle", "status_ms": 1,
        "on_send": {"status": "done", "rewrite": ["x", "y", "go"], "append": ["c"]},
    }));
    let result = json_payload(
        &fake.cli(&[
            "prompt",
            "--target",
            "s",
            "--timeout",
            "5",
            "--format",
            "json",
            "go",
        ]),
        "prompt",
    );
    assert_eq!(result["complete"], false, "{result}");
}

// --- P7: notify, claude-teams, spawn-agent ---------------------------------------------------

/// P7.1: `notify` exits 0 and reports it was not delivered.
#[test]
fn notify_is_not_delivered_on_herdr() {
    let fake = FakeHc::with_caller("herdr-notify", json!({}));
    let out = fake.cli(&["notify", "--title", "t", "--body", "b"]);
    out.assert_success("notify");
    assert_eq!(out.stderr, "notify: not delivered on herdr\n");
}

const FAKE_CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "2.1.319 (Claude Code)"; exit 0; fi
printf '%s\n' "$@" > "$FAKE_CLAUDE_ARGS"
exit 0
"#;

/// Acceptance (P7.7): `claude-teams` on Herdr fails open with `no teammate shim on
/// herdr`, starts Claude with in-process teammates, and creates no team directory;
/// `spawn-agent claude` runs the Launcher command in its `hc run`.
#[test]
fn claude_teams_fails_open_on_herdr_and_spawn_agent_claude_uses_the_launcher() {
    let fake = FakeHc::with_caller("herdr-claude", json!({}));
    write_executable(&fake.dir.join("bin/claude"), FAKE_CLAUDE);
    let args_file = fake.dir.join("claude-args");
    let args_file_str = args_file.display().to_string();
    let out = fake.cli_with(
        &["claude-teams"],
        &[("FAKE_CLAUDE_ARGS", args_file_str.as_str())],
        &[],
    );
    out.assert_success("claude-teams on herdr");
    assert_eq!(
        out.stderr,
        "claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes\n"
    );
    assert_eq!(
        std::fs::read_to_string(&args_file).unwrap(),
        "--teammate-mode\nin-process\n"
    );
    assert!(
        !fake.dir.join("state/tmux-tools/teams").exists()
            || std::fs::read_dir(fake.dir.join("state/tmux-tools/teams"))
                .unwrap()
                .next()
                .is_none(),
        "no team directory"
    );

    fake.clear_log();
    fake.cli(&["spawn-agent", "claude", "--name", "lead"])
        .assert_success("spawn-agent claude");
    let runs = fake.calls("run");
    assert_eq!(runs.len(), 1, "{runs:?}");
    let tmux_tools = std::fs::canonicalize(BIN).unwrap();
    let command = command_of(&runs[0]).join(" ");
    assert!(
        command.contains(&format!(
            "'{}' 'claude-teams' '--claude' 'claude' '--' '--permission-mode' 'plan'",
            tmux_tools.display()
        )),
        "{command}"
    );
    let labels = labels_of(&runs[0]);
    for label in [
        "tt.agent=claude",
        "tt.surface=rich",
        "tt.surface_unvalidated=false",
    ] {
        assert!(labels.contains(&label.to_owned()), "{label}: {labels:?}");
    }
}

// --- P8.3: the private-core run, opt-in -------------------------------------------------------

/// The opt-in switch of the private-core run.
const SWITCH: &str = "TT_TEST_HERDR";

/// A program that reports OSC 7501 Host state: `idle`, then for each prompt (one
/// bracketed paste or one line, ended by Enter) `working`, a reply, and `done`.
const AGENT: &str = r#"#!/usr/bin/env python3
import os, sys, termios, tty, time
fd = sys.stdin.fileno()
tty.setraw(fd)
def out(text): os.write(1, text.encode())
def report(state): out("\033]7501;state=%s\007" % state)
out("\033[?2004h")
report("idle")
out("AGENT READY\r\n")
buf, pasting = "", False
while True:
    data = os.read(fd, 4096)
    if not data:
        break
    buf += data.decode("utf-8", "replace")
    while True:
        if buf.startswith("\033[200~"):
            pasting, buf = True, buf[6:]
            continue
        end = buf.find("\033[201~")
        if pasting and end >= 0:
            pasting, buf = False, buf[:end] + buf[end + 6:]
            continue
        cr = buf.find("\r")
        if not pasting and cr >= 0:
            prompt, buf = buf[:cr], buf[cr + 1:]
            report("working")
            time.sleep(0.4)
            lines = prompt.replace("\r", "\n").split("\n")
            out("REPLY lines=%d first=%s\r\n" % (len(lines), lines[0]))
            report("done")
            continue
        break
"#;

/// A raw echo of every byte as hex.
const HEXECHO: &str = r#"#!/usr/bin/env python3
import os, sys, tty
fd = sys.stdin.fileno()
tty.setraw(fd)
os.write(1, b"HEXECHO READY\r\n")
while True:
    data = os.read(fd, 1024)
    if not data:
        break
    os.write(1, ("HEX " + data.hex() + "\r\n").encode())
"#;

/// One private-core run: the real `hc`, its private `HC_SESSION`, and its dirs. `Drop`
/// removes every terminal, stops the core, and deletes its state directory.
struct PrivateCore {
    hc: PathBuf,
    session: String,
    dir: PathBuf,
    caller: String,
}

impl PrivateCore {
    /// A private core `HC_SESSION=tt-tests-<pid>-<tag>`; each test has its own `tag`.
    fn start(tag: &str) -> PrivateCore {
        let path = original_environment()
            .get("PATH")
            .cloned()
            .unwrap_or_default();
        let hc = path
            .split(':')
            .map(|entry| Path::new(entry).join("hc"))
            .find(|candidate| candidate.exists())
            .expect("TT_TEST_HERDR=1 needs `hc` on PATH");
        let session = format!("tt-tests-{}-{tag}", std::process::id());
        assert!(session.starts_with("tt-tests-"));
        let dir = tempfile_dir("herdr-private");
        for sub in ["state", "config/tmux-tools", "work", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/agent.py"), AGENT);
        write_executable(&dir.join("bin/hexecho.py"), HEXECHO);
        std::fs::write(
            dir.join("config/tmux-tools/agents.toml"),
            format!(
                "[ttagent]\nbinary = \"{}\"\n\n[ttagent.access.default]\nargs = []\n",
                dir.join("bin/agent.py").display()
            ),
        )
        .unwrap();
        let mut core = PrivateCore {
            hc,
            session,
            dir,
            caller: String::new(),
        };
        let caller = core.hc_json(&[
            "run",
            "--bg",
            "--json",
            "--no-notify",
            "--outlive-parent",
            "--name",
            "tt-caller",
            "--label",
            "herdr.space=S",
            "--label",
            "herdr.tab=T",
            "--",
            "exec sleep 600",
        ]);
        core.caller = caller["address"]
            .as_str()
            .expect("caller address")
            .to_owned();
        core
    }

    /// The real `hc`, always with the private session.
    fn hc(&self, args: &[&str]) -> std::process::Output {
        assert!(self.session.starts_with("tt-tests-"));
        let mut command = Command::new(&self.hc);
        // The core's terminals inherit the core's environment: a Claude started in one
        // must not see the calling Claude session's variables.
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        command
            .args(args)
            .env("HC_SESSION", &self.session)
            .env_remove("HC_TERMINAL_SOCKET")
            .env_remove("HC_SCOPE")
            .env_remove("HC_TERMINAL_ID")
            // hc keeps its cores under `XDG_STATE_HOME`; the run's core is the private
            // session under `~/.local/state/herdr-core/`.
            .env_remove("XDG_STATE_HOME")
            .stdin(Stdio::null())
            .output()
            .expect("run hc")
    }

    fn hc_json(&self, args: &[&str]) -> Value {
        let out = self.hc(args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("hc {args:?}: {out:?}"))
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        self.cli_in(&self.dir.join("work"), args)
    }

    /// [`PrivateCore::cli`] with the CLI's cwd at `cwd`.
    fn cli_in(&self, cwd: &Path, args: &[&str]) -> CmdOutput {
        let path = format!(
            "{}:{}",
            self.hc.parent().unwrap().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(BIN);
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        let child = command
            .args(args)
            .current_dir(cwd)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("HC_TERMINAL_SOCKET")
            .env_remove("HC_SCOPE")
            .env("PATH", path)
            .env("HC_SESSION", &self.session)
            .env("HC_TERMINAL_ID", &self.caller)
            .env("SHELL", "/bin/sh")
            // The CLI's `hc` must reach the same private core as [`PrivateCore::hc`].
            .env_remove("XDG_STATE_HOME")
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            Duration::from_secs(120),
        )
    }

    fn get(&self, address: &str) -> Value {
        self.hc_json(&["get", "--json", address])
    }

    fn addresses(&self) -> Vec<String> {
        self.hc_json(&["ls", "--all", "--json"])["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|terminal| terminal["address"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Wait until `pane`'s screen contains `text`.
    fn wait_screen(&self, pane: &str, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let screen = self.hc_json(&["read", "--json", pane, "--lines", "0"])["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if screen.contains(text) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "{pane} never showed {text:?}: {screen}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn pane_of(&self, out: &CmdOutput, ctx: &str) -> String {
        json_payload(out, ctx)["pane_id"]
            .as_str()
            .expect("pane_id")
            .to_owned()
    }
}

impl Drop for PrivateCore {
    fn drop(&mut self) {
        for address in self.addresses() {
            let _ = self.hc(&["kill", "--json", &address]);
            let _ = self.hc(&["rm", "--json", &address]);
        }
        let _ = self.hc(&["server", "stop"]);
        std::thread::sleep(Duration::from_millis(300));
        if let Some(home) = original_environment().get("HOME") {
            let state = Path::new(home)
                .join(".local/state/herdr-core")
                .join(&self.session);
            let _ = std::fs::remove_dir_all(state);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The calling Claude session's variables (`CLAUDECODE`, `CLAUDE_*`), kept from any
/// private core and its terminals.
fn claude_session_vars() -> Vec<String> {
    original_environment()
        .keys()
        .filter(|key| key.as_str() == "CLAUDECODE" || key.starts_with("CLAUDE_"))
        .cloned()
        .collect()
}

/// One `hc read --lines 1000000` as P5.1 sees it: the lines, `how`, and `complete`.
fn history_read(core: &PrivateCore, pane: &str) -> (Vec<String>, String, bool) {
    let read = core.hc_json(&["read", "--json", pane, "--lines", "1000000"]);
    let lines = read["text"]
        .as_str()
        .unwrap_or_default()
        .split('\n')
        .map(str::to_owned)
        .collect();
    (
        lines,
        read["how"].as_str().unwrap_or_default().to_owned(),
        read["complete"].as_bool() == Some(true),
    )
}

/// Acceptance (P5.1): a real Claude pane in a private core. `prompt --format json`
/// reports Completeness as the test's own `hc read --lines 1000000` reads before and
/// after it imply, and `capture --all` gives the no-scrollback notice exactly when its
/// read reports `how: screen`, and its stdout is the text of the test's own full read
/// taken right after it. Opt-in with `TT_TEST_HERDR=1`; when enabled, a missing `claude`
/// on the original `PATH` fails the test.
#[test]
fn herdr_private_core_claude_completeness_and_capture_all() {
    if original_environment().get(SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let path = original_environment()
        .get("PATH")
        .cloned()
        .unwrap_or_default();
    let claude = path
        .split(':')
        .map(|entry| Path::new(entry).join("claude"))
        .find(|candidate| candidate.is_file())
        .expect("TT_TEST_HERDR=1 needs `claude` on the original PATH");
    // `Drop` kills and removes the Claude terminal and stops the core, also on failure.
    let core = PrivateCore::start("claude");
    // The repository root: a directory Claude already trusts, so no trust dialog.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let claude = claude.display().to_string();
    let pane = core.pane_of(
        &core.cli_in(
            repo,
            &[
                "launch",
                "--bare",
                "--name",
                "tt.claude",
                "--cmd",
                &claude,
                "--format",
                "json",
            ],
        ),
        "launch claude",
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let terminal = core.get(&pane);
        if terminal["status"]["state"] == "idle" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude never reported idle: {terminal}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    std::thread::sleep(Duration::from_secs(1));

    let (mark, mark_how, mark_complete) = history_read(&core, &pane);
    let out = core.cli_in(
        repo,
        &[
            "prompt",
            "--target",
            "tt.claude",
            "--timeout",
            "120",
            "--format",
            "json",
            "Reply with the single word PONG",
        ],
    );
    let result = json_payload(&out, "prompt claude");
    let (after, after_how, after_complete) = history_read(&core, &pane);
    let anchored = mark
        .len()
        .checked_sub(1)
        .is_some_and(|index| after.get(index) == mark.last());
    let expected = mark_how == "history"
        && mark_complete
        && after_how == "history"
        && after_complete
        && anchored;
    assert_eq!(
        result["complete"].as_bool(),
        Some(expected),
        "mark how={mark_how} complete={mark_complete}; after how={after_how} \
         complete={after_complete}; anchored={anchored}; {result}"
    );

    // P5.1: `capture --all` reads `hc read --lines 1000000`. Its raw stdout must be the
    // text of the test's own full read taken right after it. Normalisation: trailing
    // blank lines (and the final newline) are dropped from both sides; nothing else.
    // An idle Claude screen can still repaint between the two reads, so the pair is
    // retried up to 3 times; a persistent difference fails.
    fn without_trailing_blank_lines(text: &str) -> Vec<&str> {
        let mut lines: Vec<&str> = text.lines().collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        lines
    }
    let mut attempt = 0;
    loop {
        attempt += 1;
        let captured = core.cli_in(
            repo,
            &[
                "capture",
                "--target",
                "tt.claude",
                "--all",
                "--format",
                "raw",
            ],
        );
        let (read, capture_how, _) = history_read(&core, &pane);
        captured.assert_success("capture --all claude");
        assert_eq!(
            captured.stderr.contains("has no scrollback"),
            capture_how == "screen",
            "how={capture_how}: {}",
            captured.stderr
        );
        let read = read.join("\n");
        let got = without_trailing_blank_lines(&captured.stdout);
        let want = without_trailing_blank_lines(&read);
        if got == want {
            assert!(
                !want.is_empty(),
                "the full read of the Claude pane is empty"
            );
            break;
        }
        assert!(
            attempt < 3,
            "capture --all differs from hc read --lines 1000000 (how={capture_how}):\n\
             --- capture --all\n{}\n--- hc read\n{read}",
            captured.stdout
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// P8.3: every required verb against a private core. Opt-in with `TT_TEST_HERDR=1`.
#[test]
fn herdr_private_core_contract_suite() {
    if original_environment().get(SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let core = PrivateCore::start("suite");

    // self, status (P2, P7.2).
    let report = json_payload(&core.cli(&["self", "--format", "json"]), "self");
    assert_eq!(report["host"], "herdr", "{report}");
    assert_eq!(report["driven"], false, "{report}");
    let status = json_payload(&core.cli(&["status", "--format", "json"]), "status");
    assert_eq!(status["pane_id"], core.caller.as_str(), "{status}");
    assert_eq!(status["session"], "S", "{status}");
    assert!(status["host_word"].is_null(), "{status}");

    // launch with --split v and without (P3).
    let split = core.pane_of(
        &core.cli(&[
            "launch",
            "--split",
            "v",
            "--name",
            "tt.v",
            "--cmd",
            "sleep 600",
            "--format",
            "json",
        ]),
        "launch --split v",
    );
    let terminal = core.get(&split);
    // tt- and the first 16 hex of SHA-256("tt.v") (`printf tt.v | shasum -a 256`).
    assert_eq!(terminal["name"], "tt-f031389725fe9ca3", "{terminal}");
    assert_eq!(terminal["labels"]["herdr.space"], "S", "{terminal}");
    assert_eq!(terminal["labels"]["herdr.tab"], "T", "{terminal}");
    assert_eq!(terminal["labels"]["where"], "tt.v", "{terminal}");
    assert_eq!(terminal["labels"]["tt.name"], "tt.v", "{terminal}");
    let tab = core.pane_of(
        &core.cli(&[
            "launch",
            "--name",
            "tt.tab",
            "--cmd",
            "sleep 600",
            "--format",
            "json",
        ]),
        "launch",
    );
    let terminal = core.get(&tab);
    assert_eq!(terminal["labels"]["herdr.space"], "S", "{terminal}");
    assert!(terminal["labels"].get("herdr.tab").is_none(), "{terminal}");

    // A duplicate live name (P4.2) and the exit-3 options (P3.3).
    let dup = core.cli(&["launch", "--name", "tt.v", "--cmd", "true"]);
    assert_eq!(dup.status, 1, "{dup:?}");
    assert_eq!(core.cli(&["rebalance"]).status, 3);
    assert_eq!(
        core.cli(&["launch", "--split", "tile", "--cmd", "true"])
            .status,
        3
    );

    // list (P7.3).
    let rows = json_payload(&core.cli(&["list", "--format", "json"]), "list");
    let names: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert!(
        names.contains(&"tt.v") && names.contains(&"tt.tab"),
        "{rows}"
    );

    // send, send-enter, escape, interrupt against a raw echo (P5.2–P5.4).
    let hexecho = core.dir.join("bin/hexecho.py").display().to_string();
    let echo = core.pane_of(
        &core.cli(&[
            "launch", "--bare", "--name", "tt.echo", "--cmd", &hexecho, "--format", "json",
        ]),
        "launch hexecho",
    );
    core.wait_screen(&echo, "HEXECHO READY");
    core.cli(&["send", "--target", "tt.echo", "Hi"])
        .assert_success("send literal");
    core.wait_screen(&echo, "HEX 48");
    core.cli(&["send", "--target", "tt.echo", "Up"])
        .assert_success("send key");
    core.wait_screen(&echo, "HEX 1b5b41");
    core.cli(&["send-enter", "--target", "tt.echo"])
        .assert_success("send-enter");
    core.wait_screen(&echo, "HEX 0d");
    core.cli(&["escape", "--target", "tt.echo"])
        .assert_success("escape");
    core.wait_screen(&echo, "HEX 0d\nHEX 1b");
    let refused = core.cli(&["send", "--target", "tt.echo", "a$b"]);
    assert_eq!(refused.status, 1, "{refused:?}");
    assert!(
        refused.stderr.contains("cannot type on herdr: U+0024"),
        "{}",
        refused.stderr
    );
    core.cli(&["interrupt", "--target", "tt.echo"])
        .assert_success("interrupt");
    core.wait_screen(&echo, "HEX 03");

    // execute and capture in a shell (P5.1).
    let shell = core.pane_of(
        &core.cli(&[
            "launch", "--name", "tt.sh", "--cmd", "true", "--format", "json",
        ]),
        "launch shell",
    );
    std::thread::sleep(Duration::from_millis(500));
    let executed = json_payload(
        &core.cli(&[
            "execute",
            "--target",
            "tt.sh",
            "--timeout",
            "20",
            "--format",
            "json",
            "echo tt-marker-ok",
        ]),
        "execute",
    );
    assert!(executed.to_string().contains("tt-marker-ok"), "{executed}");
    let captured = core.cli(&["capture", "--target", "tt.sh", "--all"]);
    captured.assert_success("capture --all");
    assert!(captured.stdout.contains("tt-marker-ok"), "{captured:?}");
    core.cli(&["capture", "--target", "tt.sh", "--lines", "2"])
        .assert_success("capture --lines");
    // A multi-line prompt to a pane with no Host state is refused (P6.6).
    let refused = core.cli(&["prompt", "--target", "tt.sh", "--timeout", "5", "one\ntwo"]);
    assert_eq!(refused.status, 1, "{refused:?}");
    let _ = shell;

    // spawn-agent, prompt, wait-idle with Host state (P6).
    let agent = core.pane_of(
        &core.cli(&[
            "spawn-agent",
            "ttagent",
            "--name",
            "tt.agent",
            "--format",
            "json",
        ]),
        "spawn-agent",
    );
    core.wait_screen(&agent, "AGENT READY");
    let result = json_payload(
        &core.cli(&[
            "prompt",
            "--target",
            "tt.agent",
            "--timeout",
            "30",
            "--format",
            "json",
            "first\nsecond",
        ]),
        "prompt",
    );
    assert_eq!(result["one_submission_delivery"], "guaranteed", "{result}");
    assert_eq!(result["submit"], "confirmed", "{result}");
    assert_eq!(result["host_state"], "done", "{result}");
    assert!(
        result["output_since_prompt"]
            .as_str()
            .unwrap()
            .contains("REPLY lines=2 first=first"),
        "{result}"
    );
    let waited = json_payload(
        &core.cli(&[
            "wait-idle",
            "--target",
            "tt.agent",
            "--timeout",
            "10",
            "--format",
            "json",
        ]),
        "wait-idle",
    );
    assert_eq!(waited["host_state"], "done", "{waited}");
    let status = json_payload(
        &core.cli(&["status", "--target", "tt.agent", "--format", "json"]),
        "status agent",
    );
    assert_eq!(status["host_word"], "done", "{status}");

    // notify (P7.1).
    let notified = core.cli(&["notify", "--title", "t", "--body", "b"]);
    notified.assert_success("notify");
    assert!(
        notified.stderr.contains("notify: not delivered on herdr"),
        "{}",
        notified.stderr
    );

    // A finished --bare terminal is dead, and its name spawns again (P4.4).
    core.cli(&["launch", "--bare", "--name", "tt.dead", "--cmd", "true"])
        .assert_success("launch dead");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let rows = json_payload(&core.cli(&["list", "--format", "json"]), "list dead");
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "tt.dead")
            .cloned();
        match row {
            Some(row) if row["status"] == "dead" => break,
            _ => {
                assert!(Instant::now() < deadline, "tt.dead never finished: {rows}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    core.cli(&[
        "launch",
        "--bare",
        "--name",
        "tt.dead",
        "--cmd",
        "sleep 600",
    ])
    .assert_success("respawn dead");

    // kill every created terminal (P7.4), then nothing created is left (P8.5).
    for name in ["tt.v", "tt.tab", "tt.echo", "tt.sh", "tt.agent", "tt.dead"] {
        core.cli(&["kill", "--target", name, "--any"])
            .assert_success(&format!("kill {name}"));
    }
    let rows = json_payload(&core.cli(&["list", "--format", "json"]), "final list");
    assert!(rows.as_array().is_none_or(Vec::is_empty), "{rows}");
    let left: Vec<String> = core.addresses();
    assert_eq!(
        left,
        vec![core.caller.clone()],
        "only the run's caller terminal is left"
    );
}
