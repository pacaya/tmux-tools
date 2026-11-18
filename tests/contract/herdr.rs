//! The Herdr backend's contract tests (PRD-261109-2223-01 P8).
//!
//! The default run drives the binary against a fake `hc` (`fake_hc.py`, P8.2); no real
//! herdr-core is reached. With `TT_TEST_HERDR=1` and a real `hc` on the original
//! `PATH`, [`herdr_private_core_contract_suite`] runs the required verbs against a
//! private core `HC_SESSION=tt-tests-<pid>` and, also on failure, stops that core and
//! deletes its state directory (P8.3). It never reaches the default core.
//! With `TMUX_TOOLS_LIVE_HERDR=1`, [`herdr_live_contract_suite`] runs the P8.4 cases in the
//! core of the calling plain Herdr terminal and removes every terminal it created (P8.5).

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
        let _ = std::fs::remove_file(self.dir.join("env.jsonl"));
    }

    /// Every logged call with the `HC_TERMINAL_ID` of its environment (`None` when unset).
    fn calls_with_terminal_id(&self) -> Vec<(Vec<String>, Option<String>)> {
        std::fs::read_to_string(self.dir.join("env.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let call: Value = serde_json::from_str(line).unwrap();
                let argv = serde_json::from_value(call["argv"].clone()).unwrap();
                (argv, call["HC_TERMINAL_ID"].as_str().map(str::to_owned))
            })
            .collect()
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
            .env("FAKE_HC_LOG", self.dir.join("log.jsonl"))
            .env("FAKE_HC_ENV_LOG", self.dir.join("env.jsonl"));
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

/// P2.1, P2.3: an invalid `TMUX_TOOLS_HOST` names all four hosts
/// (PRD-261112-2108-01 P8.2); a missing `hc` or a failing `hc server status` fails
/// with exit 1 naming herdr and the selection source.
#[test]
fn an_unreachable_herdr_names_the_host_and_the_selection_source() {
    let fake = FakeHc::with_caller("herdr-down", json!({}));
    let invalid = fake.cli_with(&["list"], &[("TMUX_TOOLS_HOST", "bogus")], &[]);
    assert_eq!(invalid.status, 1, "{invalid:?}");
    for host in ["tmux", "orca", "rex", "herdr"] {
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

/// Acceptance (PRD-261115-0921-01 P2.1): inside a team, `TT_HOST=bogus` fails every
/// verb with exit 1, and the error names every accepted value: `tmux`, `orca`, `rex`
/// (Rex slice), and `herdr`.
#[test]
fn an_invalid_tt_host_names_every_accepted_host() {
    let fake = FakeHc::with_caller("herdr-tt-host", json!({}));
    for args in [&["list"][..], &["self", "--format", "json"][..]] {
        let out = fake.cli_with(args, &[("TT_TEAM", "0a1b2c3d"), ("TT_HOST", "bogus")], &[]);
        assert_eq!(out.status, 1, "{args:?}: {out:?}");
        assert!(
            out.stderr
                .contains("valid values are tmux, orca, rex and herdr"),
            "{args:?}: {}",
            out.stderr
        );
    }
}

// --- P3: placement ---------------------------------------------------------------------

/// Acceptance: `launch --split v --name A.b` runs one `hc run` with the P3.4 flags,
/// the hc name, and the labels of the calling terminal's space and tab. `--rm` is
/// never used. (No `--split`: [`a_new_tab_create_makes_up_a_tab_id_that_a_later_split_joins`].)
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
}

/// The one `herdr.tab=` value in the labels of one logged `hc run`.
fn tab_label(run: &[String]) -> String {
    let tabs: Vec<String> = labels_of(run)
        .into_iter()
        .filter_map(|label| label.strip_prefix("herdr.tab=").map(str::to_owned))
        .collect();
    assert_eq!(tabs.len(), 1, "exactly one herdr.tab label: {run:?}");
    tabs.into_iter().next().unwrap()
}

/// Acceptance (P3.2, P7.3): from a calling terminal with `herdr.space` = S, `launch`
/// with no `--split` and with `--split window` each sets `herdr.space` = S and a
/// `herdr.tab` that no other terminal carries; each `launch --format json` reports that
/// tab as `window_id`; a later `launch --split v --target <first terminal>` joins the
/// first terminal's tab.
#[test]
fn a_new_tab_create_makes_up_a_tab_id_that_a_later_split_joins() {
    let fake = FakeHc::with_caller(
        "herdr-new-tab",
        json!({"herdr.space": "S", "herdr.tab": "T"}),
    );
    let other = fake.add_terminal(json!({"labels": {"herdr.space": "S", "herdr.tab": "T2"}}));
    let mut made_up = Vec::new();
    for (name, extra) in [("n.one", &[][..]), ("n.two", &["--split", "window"][..])] {
        fake.clear_log();
        let mut args = vec![
            "launch", "--name", name, "--cmd", "sleep 30", "--format", "json",
        ];
        args.extend_from_slice(extra);
        let out = fake.cli(&args);
        out.assert_success("launch in a new tab");
        let runs = fake.calls("run");
        assert_eq!(runs.len(), 1, "{runs:?}");
        let labels = labels_of(&runs[0]);
        assert!(labels.contains(&"herdr.space=S".to_owned()), "{labels:?}");
        let tab = tab_label(&runs[0]);
        let report = json_payload(&out, "launch --format json");
        assert_eq!(report["window_id"].as_str(), Some(tab.as_str()), "{report}");
        let pane = report["pane_id"].as_str().expect("pane_id").to_owned();
        made_up.push((pane, tab));
    }
    let (first, first_tab) = made_up[0].clone();
    let (second, second_tab) = made_up[1].clone();
    // No other terminal carries either made-up tab.
    for terminal in fake.state()["terminals"].as_array().unwrap() {
        let address = terminal["address"].as_str().unwrap();
        let tab = terminal["labels"]["herdr.tab"].as_str();
        if address != first {
            assert_ne!(tab, Some(first_tab.as_str()), "{terminal}");
        }
        if address != second {
            assert_ne!(tab, Some(second_tab.as_str()), "{terminal}");
        }
    }
    assert!(
        !["T", "T2", ""].contains(&first_tab.as_str()),
        "{first_tab}"
    );
    assert_ne!(first_tab, second_tab);
    assert_ne!(other, first);

    fake.clear_log();
    fake.cli(&[
        "launch", "--split", "v", "--target", &first, "--name", "n.three", "--cmd", "sleep 30",
    ])
    .assert_success("launch --split v --target <first terminal>");
    let run = &fake.calls("run")[0];
    assert!(
        labels_of(run).contains(&"herdr.space=S".to_owned()),
        "{run:?}"
    );
    assert_eq!(tab_label(run), first_tab, "{run:?}");
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

/// Acceptance (P6.5): every `hc send` of `prompt` and `execute` runs without
/// `HC_TERMINAL_ID`, so hc delivers the text untagged; every other `hc` call of these
/// verbs keeps it.
#[test]
fn hc_send_runs_without_hc_terminal_id_and_every_other_call_keeps_it() {
    let fake = FakeHc::with_caller("herdr-untagged", json!({}));
    let pane = fake.add_terminal(json!({
        "labels": {"tt.name": "a"}, "status": "working", "status_ms": 5, "lines": ["$ ready"],
    }));
    for args in [
        &["prompt", "--target", "a", "--timeout", "1", "/exit"][..],
        &["execute", "--target", "a", "--no-wait", "echo hi"][..],
    ] {
        fake.clear_log();
        fake.cli(args).assert_success(&format!("{args:?}"));
        let calls = fake.calls_with_terminal_id();
        let sends: Vec<_> = calls
            .iter()
            .filter(|(argv, _)| argv.first().map(String::as_str) == Some("send"))
            .collect();
        assert_eq!(sends.len(), 1, "{args:?}: {calls:?}");
        for (argv, terminal_id) in &calls {
            if argv.first().map(String::as_str) == Some("send") {
                assert_eq!(terminal_id, &None, "{args:?}: {argv:?}");
            } else {
                assert_eq!(terminal_id.as_deref(), Some("t1"), "{args:?}: {argv:?}");
            }
        }
    }
    assert_eq!(fake.terminal(&pane).unwrap()["sent"][0], "/exit");
}

/// Acceptance (P5.2): a character outside the typeable set that has a tmux key name is
/// refused with that key name, the command that sends it as a key, and a pointer to
/// `send --help`; nothing is sent. Other refusals keep their text.
#[test]
fn send_refuses_a_control_character_with_its_key_hint() {
    let fake = FakeHc::with_caller("herdr-key-hint", json!({}));
    fake.add_terminal(json!({"labels": {"tt.name": "probe"}}));
    let other = |key: &str| format!("tmux-tools send --target <pane> {key}");
    let cases = [
        (
            "\u{1b}",
            "U+001B (Escape)",
            "tmux-tools escape --target <pane>, or tmux-tools send --target <pane> Escape"
                .to_owned(),
        ),
        (
            "\r",
            "U+000D (Enter)",
            "tmux-tools send-enter --target <pane>, or tmux-tools send --target <pane> Enter"
                .to_owned(),
        ),
        ("\t", "U+0009 (Tab)", other("Tab")),
        ("\u{7f}", "U+007F (BSpace)", other("BSpace")),
        ("\u{1}", "U+0001 (C-a)", other("C-a")),
        ("\u{1a}", "U+001A (C-z)", other("C-z")),
    ];
    for (text, named, commands) in cases {
        let out = fake.cli(&["send", "--target", "probe", "--literal", text]);
        assert_eq!(out.status, 1, "{text:?}: {out:?}");
        let expected = format!(
            "cannot type on herdr: {named}. Send it as a key: {commands}. See: tmux-tools send --help"
        );
        assert!(out.stderr.contains(&expected), "{text:?}: {}", out.stderr);
        assert!(!out.stderr.contains("probe"), "{text:?}: {}", out.stderr);
    }
    let out = fake.cli(&["send", "--target", "probe", "--literal", "!"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("cannot type on herdr: U+0021")
            && !out.stderr.contains("Send it as a key"),
        "{}",
        out.stderr
    );
    let out = fake.cli(&["send", "--target", "probe", "KP0"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("cannot type on herdr: KP0")
            && !out.stderr.contains("Send it as a key"),
        "{}",
        out.stderr
    );
    assert!(fake.calls("keys").is_empty(), "{:?}", fake.log());
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
    // A `how: stitched` read after settle is the whole result.
    assert_eq!(result["output_since_prompt"], "a\nb\ngo\nc", "{result}");

    fake.edit("t2", |terminal| terminal["how"] = json!("screen"));
    let out = fake.cli(&["capture", "--target", "s", "--all"]);
    out.assert_success("capture --all");
    assert!(out.stderr.contains("no scrollback"), "{}", out.stderr);
    let out = fake.cli(&["capture", "--target", "s", "--lines", "1"]);
    assert_eq!(out.stdout, "c", "{out:?}");
}

/// `prompt --format json` to a terminal whose history is `lines` and whose `hc get`
/// reports `rows`; `on_send` sets the read after settle (the fake adds the prompt
/// line first, then applies `rewrite` and `append`).
fn p51_prompt(label: &str, lines: Value, rows: u64, on_send: Value) -> Value {
    let fake = FakeHc::with_caller(label, json!({}));
    let mut effect = on_send;
    effect["status"] = json!("done");
    fake.add_terminal(json!({
        "labels": {"tt.name": "s"}, "lines": lines, "rows": rows, "how": "history",
        "complete": true, "status": "idle", "status_ms": 1, "on_send": effect,
    }));
    json_payload(
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
    )
}

/// Acceptance (P5.1): the mark read reports `how: history`, the read after settle
/// reports `how: stitched` (then, in a second case, `how: screen`) and starts with the
/// mark read's lines; the result is that whole read and is not complete.
#[test]
fn a_stitched_or_screen_read_after_settle_is_the_whole_result_and_incomplete() {
    for how in ["stitched", "screen"] {
        let result = p51_prompt(
            &format!("herdr-p51-{how}"),
            json!(["l1", "l2", "l3"]),
            24,
            json!({"append": ["R1", "R2"], "how": how}),
        );
        assert_eq!(
            result["output_since_prompt"], "l1\nl2\nl3\ngo\nR1\nR2",
            "how={how}: {result}"
        );
        assert_eq!(result["complete"], false, "how={how}: {result}");
    }
}

/// Acceptance (P5.1): a classic-scroll agent rewrites the last lines of the mark read
/// and adds lines; the result starts at the first changed line and is complete.
#[test]
fn a_rewritten_tail_gives_the_result_from_the_first_changed_line() {
    let result = p51_prompt(
        "herdr-p51-rewrite",
        json!(["l1", "l2", "l3", "l4", "l5"]),
        3,
        json!({"rewrite": ["l1", "l2", "l3", "l4x", "l5x", "R1"]}),
    );
    assert_eq!(result["output_since_prompt"], "l4x\nl5x\nR1", "{result}");
    assert_eq!(result["complete"], true, "{result}");
}

/// Acceptance (P5.1): a shell only adds lines after the end of the mark read; the
/// result is the added lines and is complete.
#[test]
fn appended_lines_are_the_result_and_complete() {
    let result = p51_prompt(
        "herdr-p51-append",
        json!(["$ a", "b"]),
        24,
        json!({"append": ["c"]}),
    );
    assert_eq!(result["output_since_prompt"], "go\nc", "{result}");
    assert_eq!(result["complete"], true, "{result}");
}

/// Acceptance (P5.1): the first changed line (1) is less than the mark read's 5 lines
/// minus `rows` 2; the result starts there and is not complete.
#[test]
fn a_change_above_the_last_rows_lines_is_incomplete() {
    let result = p51_prompt(
        "herdr-p51-deep",
        json!(["l1", "l2", "l3", "l4", "l5"]),
        2,
        json!({"rewrite": ["l1", "X", "l3", "l4", "l5", "R"]}),
    );
    assert_eq!(
        result["output_since_prompt"], "X\nl3\nl4\nl5\nR",
        "{result}"
    );
    assert_eq!(result["complete"], false, "{result}");
}

/// Acceptance (P5.1): equal reads give an empty result; the first changed line is the
/// mark read's 2 lines, at least 2 minus `rows` 0, so it is complete.
#[test]
fn equal_reads_give_an_empty_complete_result() {
    let result = p51_prompt(
        "herdr-p51-equal",
        json!(["a", "b"]),
        0,
        json!({"rewrite": ["a", "b"]}),
    );
    assert_eq!(result["output_since_prompt"], "", "{result}");
    assert_eq!(result["complete"], true, "{result}");
}

/// Acceptance (P5.1): the read after settle is the mark read with its last 2 lines
/// removed; the result is empty, and the first changed line 1 is less than the mark
/// read's 3 lines minus `rows` 1, so it is not complete. With `rows` 2 it is.
#[test]
fn a_shortened_read_gives_an_empty_result_under_the_same_test() {
    let result = p51_prompt(
        "herdr-p51-short",
        json!(["a", "b", "c"]),
        1,
        json!({"rewrite": ["a"]}),
    );
    assert_eq!(result["output_since_prompt"], "", "{result}");
    assert_eq!(result["complete"], false, "{result}");
    let result = p51_prompt(
        "herdr-p51-short2",
        json!(["a", "b", "c"]),
        2,
        json!({"rewrite": ["a"]}),
    );
    assert_eq!(result["output_since_prompt"], "", "{result}");
    assert_eq!(result["complete"], true, "{result}");
}

/// Acceptance (P5.1, Q188): either read reports `complete: false` with `how: history`;
/// Completeness follows the first-changed-line test and not that flag.
#[test]
fn hc_complete_false_does_not_change_completeness() {
    // Only the read after settle reports `complete: false`; the first changed line 3
    // is at least 5 lines minus `rows` 3, so the result is complete.
    let result = p51_prompt(
        "herdr-p51-flag-after",
        json!(["l1", "l2", "l3", "l4", "l5"]),
        3,
        json!({"rewrite": ["l1", "l2", "l3", "l4x", "l5x", "R1"], "complete": false}),
    );
    assert_eq!(result["output_since_prompt"], "l4x\nl5x\nR1", "{result}");
    assert_eq!(result["complete"], true, "{result}");

    // Both reads report `complete: false`; a shell append is complete.
    let fake = FakeHc::with_caller("herdr-p51-flag-both", json!({}));
    fake.add_terminal(json!({
        "labels": {"tt.name": "s"}, "lines": ["$ a", "b"], "rows": 24, "how": "history",
        "complete": false, "status": "idle", "status_ms": 1,
        "on_send": {"status": "done", "append": ["c"]},
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
    assert_eq!(result["output_since_prompt"], "go\nc", "{result}");
    assert_eq!(result["complete"], true, "{result}");

    // The read after settle reports `complete: false`; the first changed line 1 is
    // less than 5 lines minus `rows` 2, so the result is not complete.
    let result = p51_prompt(
        "herdr-p51-flag-deep",
        json!(["l1", "l2", "l3", "l4", "l5"]),
        2,
        json!({"rewrite": ["l1", "X", "l3", "l4", "l5", "R"], "complete": false}),
    );
    assert_eq!(
        result["output_since_prompt"], "X\nl3\nl4\nl5\nR",
        "{result}"
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
printf '%s\n' "${TT_HOST-}" > "$FAKE_CLAUDE_ARGS.host"
exit 0
"#;

/// Acceptance (PRD-261115-0921-01 P2): `claude-teams` in a Herdr terminal sets up a
/// Team: no `claude-teams: no team` line, Claude gets `--teammate-mode tmux`, and
/// `TT_HOST=herdr`; with `HC_TERMINAL_ID` unset (`TMUX_TOOLS_HOST=herdr`) it fails open
/// with `HC_TERMINAL_ID is unset` and in-process teammates, and creates no team
/// directory. `spawn-agent claude` runs the Launcher command in its `hc run`.
#[test]
fn claude_teams_sets_up_a_team_on_herdr_and_spawn_agent_claude_uses_the_launcher() {
    let fake = FakeHc::with_caller("herdr-claude", json!({}));
    write_executable(&fake.dir.join("bin/claude"), FAKE_CLAUDE);
    let args_file = fake.dir.join("claude-args");
    let args_file_str = args_file.display().to_string();
    let teams = fake.dir.join("state/tmux-tools/teams");
    let no_team_dir = || !teams.exists() || std::fs::read_dir(&teams).unwrap().next().is_none();

    let unset = fake.cli_with(
        &["claude-teams"],
        &[
            ("FAKE_CLAUDE_ARGS", args_file_str.as_str()),
            ("TMUX_TOOLS_HOST", "herdr"),
        ],
        &["HC_TERMINAL_ID"],
    );
    unset.assert_success("claude-teams on herdr without HC_TERMINAL_ID");
    assert_eq!(
        unset.stderr,
        "claude-teams: no team: HC_TERMINAL_ID is unset; starting claude without teammate panes\n"
    );
    assert_eq!(
        std::fs::read_to_string(&args_file).unwrap(),
        "--teammate-mode\nin-process\n"
    );
    assert!(no_team_dir(), "no team directory");

    let out = fake.cli_with(
        &["claude-teams"],
        &[("FAKE_CLAUDE_ARGS", args_file_str.as_str())],
        &[],
    );
    out.assert_success("claude-teams on herdr");
    assert!(
        !out.stderr.contains("claude-teams: no team"),
        "{}",
        out.stderr
    );
    assert_eq!(
        std::fs::read_to_string(&args_file).unwrap(),
        "--teammate-mode\ntmux\n"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{args_file_str}.host")).unwrap(),
        "herdr\n"
    );
    assert!(no_team_dir(), "the clean exit removed the team directory");

    fake.clear_log();
    fake.cli(&["spawn-agent", "claude", "--name", "lead"])
        .assert_success("spawn-agent claude");
    let runs = fake.calls("run");
    assert_eq!(runs.len(), 1, "{runs:?}");
    let tmux_tools = std::fs::canonicalize(BIN).unwrap();
    let command = command_of(&runs[0]).join(" ");
    assert!(
        command.contains(&format!(
            "'{}' 'claude-teams' '--claude' 'claude' '--' '--settings' '{{\"env\":{{\"CLAUDE_CODE_NO_FLICKER\":\"0\"}}}}' '--permission-mode' 'plan'",
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

/// P5.1: the smallest line index at which the two reads differ, where a line that only
/// one read has counts as different; the mark read's line count when they are equal.
fn first_changed_line(mark: &[String], after: &[String]) -> usize {
    (0..mark.len().max(after.len()))
        .find(|&index| mark.get(index) != after.get(index))
        .unwrap_or(mark.len())
}

/// The pane's `rows` as `hc get --json` reports it.
fn rows_of(core: &PrivateCore, pane: &str) -> usize {
    core.get(pane)["rows"]
        .as_u64()
        .expect("hc get reports rows") as usize
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
    let first = first_changed_line(&mark, &after);
    let rows = rows_of(&core, &pane);
    // P5.1 (Q188): the `complete` value `hc read` reports does not change Completeness.
    let expected =
        mark_how == "history" && after_how == "history" && first >= mark.len().saturating_sub(rows);
    assert_eq!(
        result["complete"].as_bool(),
        Some(expected),
        "mark how={mark_how} complete={mark_complete}; after how={after_how} \
         complete={after_complete}; first changed line={first} of {} rows={rows}; {result}",
        mark.len()
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

/// The `claude` of the original `PATH`; a missing one fails the opt-in test.
fn claude_on_original_path() -> PathBuf {
    let path = original_environment()
        .get("PATH")
        .cloned()
        .unwrap_or_default();
    path.split(':')
        .map(|entry| Path::new(entry).join("claude"))
        .find(|candidate| candidate.is_file())
        .expect("TT_TEST_HERDR=1 needs `claude` on the original PATH")
}

/// Precondition: `~/.claude/settings.json` sets `env.CLAUDE_CODE_NO_FLICKER` to `"1"`,
/// so a Claude started without a `--settings` override runs full screen; `proves`
/// names what the test's result proves only under it.
fn assert_user_settings_turn_on_fullscreen(proves: &str) {
    let home = original_environment()
        .get("HOME")
        .cloned()
        .expect("TT_TEST_HERDR=1 needs HOME in the original environment");
    let settings_path = Path::new(&home).join(".claude/settings.json");
    let settings: Value = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    assert_eq!(
        settings["env"]["CLAUDE_CODE_NO_FLICKER"].as_str(),
        Some("1"),
        "precondition failed: {} must set env.CLAUDE_CODE_NO_FLICKER to \"1\" so that {proves}",
        settings_path.display()
    );
}

/// Acceptance (P5.1): a full-screen Claude pane that `launch --cmd claude` starts with
/// the user setting `CLAUDE_CODE_NO_FLICKER=1`, so `hc read --json` reports
/// `how: stitched`. `prompt --format json` sends a prompt that makes Claude print a
/// run-unique token; the result (the whole read after settle) contains that token and
/// Completeness is false. Opt-in with `TT_TEST_HERDR=1`; when enabled, a missing
/// `claude` on the original `PATH` fails the test.
#[test]
fn herdr_private_core_full_screen_claude_prompt_is_the_whole_read() {
    if original_environment().get(SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let claude = claude_on_original_path().display().to_string();
    assert_user_settings_turn_on_fullscreen("a plain `launch --cmd claude` runs full screen");
    // `Drop` kills and removes the Claude terminal and stops the core, also on failure.
    let core = PrivateCore::start("fs");
    // The repository root: a directory Claude already trusts, so no trust dialog.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pane = core.pane_of(
        &core.cli_in(
            repo,
            &[
                "launch",
                "--bare",
                "--name",
                "tt.fullscreen",
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
        if matches!(terminal["status"]["state"].as_str(), Some("idle" | "done")) {
            break;
        }
        if Instant::now() >= deadline {
            let (screen, how, _) = history_read(&core, &pane);
            panic!(
                "Claude never reported idle: {terminal}\nhow={how}\n{}",
                screen.join("\n")
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    std::thread::sleep(Duration::from_secs(1));
    let (_, how, _) = history_read(&core, &pane);
    assert_eq!(
        how, "stitched",
        "a full-screen Claude reads as `how: stitched`"
    );

    // A run-unique token that only Claude's reply prints: the prompt carries the two
    // addends, never their sum.
    let seed = u64::from(std::process::id());
    let addend = 161_803_u64;
    let token = (seed + addend).to_string();
    let out = core.cli_in(
        repo,
        &[
            "prompt",
            "--target",
            "tt.fullscreen",
            "--timeout",
            "120",
            "--format",
            "json",
            &format!("Reply with only the decimal sum of {seed} and {addend}, digits only"),
        ],
    );
    let result = json_payload(&out, "prompt full-screen claude");
    let (after, after_how, _) = history_read(&core, &pane);
    assert_eq!(
        after_how, "stitched",
        "still full screen after the prompt: {result}"
    );
    assert!(
        result["output_since_prompt"]
            .as_str()
            .is_some_and(|output| output.contains(&token)),
        "the reply token {token}: {result}"
    );
    // The result is the whole read after settle, not the part from the first changed
    // line: its first non-blank line is the first non-blank line of the test's own read
    // taken right after it.
    let first_after = after
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim());
    let first_output = result["output_since_prompt"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim);
    assert_eq!(
        first_output, first_after,
        "the result starts at the top of the whole read: {result}"
    );
    assert_eq!(result["complete"], false, "{result}");
}

/// Acceptance (ADR-260919-0146-01#claude-rich-launch item 1): with the user's
/// `CLAUDE_CODE_NO_FLICKER=1` setting, a pane `spawn-agent claude` starts on the
/// default `rich` surface is in classic scroll mode, so `hc read --json` reports
/// `how: history`, and `prompt --format json` returns the reply token and none of the
/// text above the History mark. Opt-in with `TT_TEST_HERDR=1`; when enabled, a missing
/// `claude` on the original `PATH` fails the test.
#[test]
fn herdr_private_core_claude_rich_is_classic_scroll() {
    if original_environment().get(SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let claude = claude_on_original_path();
    // Precondition: the user settings turn on fullscreen (`CLAUDE_CODE_NO_FLICKER=1`).
    // Without it Claude is in classic scroll anyway and `how: history` proves nothing
    // about the `rich` surface's `--settings` override.
    assert_user_settings_turn_on_fullscreen("`how: history` proves the `rich` --settings override");
    // `Drop` kills and removes the Claude terminal and stops the core, also on failure.
    let core = PrivateCore::start("rich");
    // The core's terminals do not carry the original `PATH`, so the built-in `claude`
    // runs by the absolute path of the original `PATH`'s `claude`; its surfaces and
    // access profiles stay the built-in ones.
    let agents = core.dir.join("config/tmux-tools/agents.toml");
    let mut toml = std::fs::read_to_string(&agents).unwrap();
    toml.push_str(&format!("\n[claude]\nbinary = \"{}\"\n", claude.display()));
    std::fs::write(&agents, toml).unwrap();
    // The repository root: a directory Claude already trusts, so no trust dialog.
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pane = core.pane_of(
        &core.cli_in(
            repo,
            &[
                "spawn-agent",
                "claude",
                "--name",
                "rich",
                "--format",
                "json",
            ],
        ),
        "spawn-agent claude",
    );
    let wait_idle = || {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let terminal = core.get(&pane);
            if matches!(terminal["status"]["state"].as_str(), Some("idle" | "done")) {
                break;
            }
            if Instant::now() >= deadline {
                let (screen, how, _) = history_read(&core, &pane);
                panic!(
                    "Claude never reported idle: {terminal}\nhow={how}\n{}",
                    screen.join("\n")
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    wait_idle();

    // Run-unique tokens that only Claude's reply prints: the prompt carries the two
    // addends, never their sum.
    let seed = u64::from(std::process::id());
    let ask = |addend: u64| {
        let sum = seed + addend;
        let out = core.cli_in(
            repo,
            &[
                "prompt",
                "--target",
                "rich",
                "--timeout",
                "120",
                "--format",
                "json",
                &format!("Reply with only the decimal sum of {seed} and {addend}, digits only"),
            ],
        );
        (sum.to_string(), json_payload(&out, "prompt claude"))
    };

    // A first turn puts its token above the History mark of the second.
    let (above, first) = ask(271_828);
    assert!(
        first["output_since_prompt"]
            .as_str()
            .is_some_and(|output| output.contains(&above)),
        "the first reply holds its token {above}: {first}"
    );
    wait_idle();
    let (mark, mark_how, mark_complete) = history_read(&core, &pane);
    assert_eq!(mark_how, "history", "classic scroll before the prompt");
    let banner = mark
        .iter()
        .find(|line| line.contains("Claude Code v"))
        .cloned()
        .unwrap_or_else(|| panic!("Claude's banner is above the mark: {mark:?}"));

    let (token, result) = ask(314_159);
    let (after, after_how, after_complete) = history_read(&core, &pane);
    assert_eq!(
        after_how, "history",
        "classic scroll after the prompt: {result}"
    );
    let output = result["output_since_prompt"].as_str().unwrap_or_default();
    assert!(output.contains(&token), "reply token {token}: {result}");
    // The result starts at the first changed line: its first non-blank line is the
    // first non-blank line of the test's own read after settle from that line.
    let first = first_changed_line(&mark, &after);
    let first_after = after[first.min(after.len())..]
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim());
    let first_output = output.lines().find(|line| !line.trim().is_empty());
    assert_eq!(
        first_output.map(str::trim),
        first_after,
        "the result starts at the first changed line {first}: {result}"
    );
    assert!(
        !output.contains(&above),
        "the first turn's token {above} is above the History mark: {result}"
    );
    assert!(
        !output.contains(banner.trim()),
        "the banner is above the History mark: {result}"
    );
    // P5.1 Completeness: both reads report `how: history` (checked above), and the first
    // changed line is at least the mark read's line count minus `rows`; hc's `complete`
    // value does not change it.
    assert_eq!(
        result["complete"],
        true,
        "Completeness: mark complete={mark_complete} lines={}; after complete={after_complete} \
         lines={}; first changed line={}; rows={}; {result}",
        mark.len(),
        after.len(),
        first_changed_line(&mark, &after),
        rows_of(&core, &pane)
    );
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
    // No --split: a new tab with a made-up `herdr.tab`, reported as `window_id`, that
    // a later `--split v` from that terminal joins (P3.2, P7.3).
    let tab_launch = core.cli(&[
        "launch",
        "--name",
        "tt.tab",
        "--cmd",
        "sleep 600",
        "--format",
        "json",
    ]);
    let tab = core.pane_of(&tab_launch, "launch");
    let terminal = core.get(&tab);
    assert_eq!(terminal["labels"]["herdr.space"], "S", "{terminal}");
    let made_up = terminal["labels"]["herdr.tab"]
        .as_str()
        .expect("a made-up herdr.tab")
        .to_owned();
    assert!(!made_up.is_empty() && made_up != "T", "{terminal}");
    assert_eq!(
        json_payload(&tab_launch, "launch")["window_id"].as_str(),
        Some(made_up.as_str())
    );
    let joined = core.pane_of(
        &core.cli(&[
            "launch",
            "--split",
            "v",
            "--target",
            &tab,
            "--name",
            "tt.tab.v",
            "--cmd",
            "sleep 600",
            "--format",
            "json",
        ]),
        "launch --split v --target <new tab>",
    );
    let terminal = core.get(&joined);
    assert_eq!(
        terminal["labels"]["herdr.tab"].as_str(),
        Some(made_up.as_str()),
        "{terminal}"
    );

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
    for name in [
        "tt.v", "tt.tab", "tt.tab.v", "tt.echo", "tt.sh", "tt.agent", "tt.dead",
    ] {
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

// --- P8.4, P8.5: the opt-in live run -----------------------------------------------------

/// The opt-in switch of the live run (PRD-261109-2223-01 `## Contracts`, P8.4).
const LIVE_SWITCH: &str = "TMUX_TOOLS_LIVE_HERDR";

/// How long the live run keeps its terminals open, after every case passed, so the
/// maintainer can check the Herdr window (Verification item 3).
const LIVE_HOLD: Duration = Duration::from_secs(30);

/// One live run in the default core, from a calling Herdr terminal. Every CLI and `hc`
/// call gets the calling terminal's `HC_TERMINAL_ID` and the original `PATH`, never
/// `TMUX` or another `HC_*` variable. Each
/// terminal the run creates carries one of the run-unique names of [`LiveRun::n`] as its
/// `where` label from its creation (P3.4); `Drop` kills and removes, by hc address,
/// every terminal that carries such a name, also on failure, and never another one.
struct LiveRun {
    hc: PathBuf,
    dir: PathBuf,
    tag: String,
    names: std::cell::RefCell<Vec<String>>,
    caller: String,
}

impl LiveRun {
    fn start() -> LiveRun {
        let original = original_environment();
        assert!(
            !original.contains_key("TMUX"),
            "{LIVE_SWITCH}=1 runs from a plain Herdr terminal: unset TMUX"
        );
        let caller = original.get("HC_TERMINAL_ID").cloned().unwrap_or_else(|| {
            panic!("{LIVE_SWITCH}=1 runs from a Herdr terminal: HC_TERMINAL_ID")
        });
        let hc = original
            .get("PATH")
            .map(String::as_str)
            .unwrap_or_default()
            .split(':')
            .map(|entry| Path::new(entry).join("hc"))
            .find(|candidate| candidate.is_file())
            .expect("the live run needs `hc` on PATH");
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let tag = format!("{:x}", (millis ^ u64::from(std::process::id())) & 0xff_ffff);
        let dir = tempfile_dir("herdr-live");
        for sub in ["config/tmux-tools", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/hexecho.py"), HEXECHO);
        // The core's terminals need not carry the original `PATH`: the built-in `claude`
        // runs by the absolute path of the original `PATH`'s `claude`.
        std::fs::write(
            dir.join("config/tmux-tools/agents.toml"),
            format!(
                "[claude]\nbinary = \"{}\"\n",
                claude_on_original_path().display()
            ),
        )
        .unwrap();
        LiveRun {
            hc,
            dir,
            tag,
            names: std::cell::RefCell::new(Vec::new()),
            caller,
        }
    }

    /// The run-unique tmux-tools name for `role`, recorded for the teardown.
    fn n(&self, role: &str) -> String {
        let name = format!("tt.live{}.{role}", self.tag);
        self.names.borrow_mut().push(name.clone());
        name
    }

    /// The default core, from the calling terminal: its `HC_TERMINAL_ID`, the original
    /// `PATH`, no `TMUX`, no other `HC_*` variable, and none of the calling Claude
    /// session's variables.
    fn env(&self, command: &mut Command) {
        let original = original_environment();
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        command
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("PATH", original.get("PATH").cloned().unwrap_or_default());
        match original.get("HC_TERMINAL_ID") {
            Some(value) => command.env("HC_TERMINAL_ID", value),
            None => command.env_remove("HC_TERMINAL_ID"),
        };
        // The run is in the default core (P8.4): neither the CLI nor `hc` gets the
        // caller's core selection.
        for key in ["HC_TERMINAL_SOCKET", "HC_SCOPE", "HC_SESSION"] {
            command.env_remove(key);
        }
    }

    fn hc(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(&self.hc);
        self.env(&mut command);
        command
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("run hc")
    }

    fn hc_json(&self, args: &[&str]) -> Value {
        let out = self.hc(args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("hc {args:?}: {out:?}"))
    }

    /// The CLI from the repository root (a directory Claude already trusts).
    fn cli(&self, args: &[&str]) -> CmdOutput {
        let mut command = Command::new(BIN);
        self.env(&mut command);
        let child = command
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("SHELL", "/bin/sh")
            // `XDG_STATE_HOME` stays as inherited: hc finds the core's socket under it.
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            Duration::from_secs(180),
        )
    }

    fn get(&self, address: &str) -> Value {
        self.hc_json(&["get", "--json", address])
    }

    /// `hc ls --all --json` rows whose `where` label is a name of this run.
    fn own_rows(&self) -> Vec<Value> {
        let names = self.names.borrow();
        self.hc_json(&["ls", "--all", "--json"])["terminals"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|row| {
                row["where"]
                    .as_str()
                    .is_some_and(|name| names.iter().any(|own| own == name))
            })
            .collect()
    }

    /// Wait until `pane`'s screen contains `text`.
    fn wait_screen(&self, pane: &str, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let screen = self.hc_json(&["read", "--json", pane, "--lines", "0"])["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if screen.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{pane} never showed {text:?}: {screen}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        // `own_rows` panics only when hc answers no JSON; a panic here during an unwind
        // would abort, so the teardown is best effort and the leak assertion of the test
        // body is the check.
        let rows = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.own_rows()))
            .unwrap_or_default();
        for row in rows {
            if let Some(address) = row["address"].as_str() {
                let _ = self.hc(&["kill", "--json", address]);
                let _ = self.hc(&["rm", "--json", address]);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// P8.4: the contract suite in the core of the calling Herdr terminal, from a plain
/// Herdr terminal (no `$TMUX`, `HC_TERMINAL_ID` set): a Claude agent pane with Host
/// state, a key-name and a literal send, a multi-line prompt to the Claude pane with
/// guaranteed delivery (P6.6), a `--split v` terminal in the caller's tab, and a
/// no-split terminal in a new tab. The run holds its terminals for [`LIVE_HOLD`] so the
/// maintainer can check the Herdr window, then kills them and asserts that neither
/// `tmux-tools list` nor `hc ls --all --json` shows one (P8.5). Opt-in with
/// `TMUX_TOOLS_LIVE_HERDR=1`.
#[test]
fn herdr_live_contract_suite() {
    if original_environment().get(LIVE_SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let run = LiveRun::start();
    let caller = run.get(&run.caller);
    let space = caller["labels"]["herdr.space"].as_str().map(str::to_owned);
    let tab = caller["labels"]["herdr.tab"].as_str().map(str::to_owned);
    assert!(
        space.is_some() && tab.is_some(),
        "the calling terminal carries herdr.space and herdr.tab: {caller}"
    );
    println!(
        "P8.4 live run {}: caller {} space {space:?} tab {tab:?}",
        run.tag, run.caller
    );
    let pane_of = |out: &CmdOutput, ctx: &str| -> String {
        json_payload(out, ctx)["pane_id"]
            .as_str()
            .expect("pane_id")
            .to_owned()
    };

    let report = json_payload(&run.cli(&["self", "--format", "json"]), "self");
    assert_eq!(report["host"], "herdr", "{report}");

    // A `--split v` terminal joins the caller's tab (P3.1, P3.2).
    let split_name = run.n("v");
    let hexecho = run.dir.join("bin/hexecho.py").display().to_string();
    let split = pane_of(
        &run.cli(&[
            "launch",
            "--split",
            "v",
            "--bare",
            "--name",
            &split_name,
            "--cmd",
            &hexecho,
            "--format",
            "json",
        ]),
        "launch --split v",
    );
    let terminal = run.get(&split);
    assert_eq!(
        terminal["labels"]["herdr.space"].as_str(),
        space.as_deref(),
        "{terminal}"
    );
    assert_eq!(
        terminal["labels"]["herdr.tab"].as_str(),
        tab.as_deref(),
        "{terminal}"
    );
    println!("P8.4 --split v terminal {split} ({split_name}) is in the caller's tab: ok");

    // A key-name and a literal send (P5.2).
    run.wait_screen(&split, "HEXECHO READY");
    run.cli(&["send", "--target", &split_name, "Hi"])
        .assert_success("send literal");
    run.wait_screen(&split, "HEX 48");
    run.wait_screen(&split, "HEX 69");
    run.cli(&["send", "--target", &split_name, "Up"])
        .assert_success("send key");
    run.wait_screen(&split, "HEX 1b5b41");
    println!("P8.4 key-name and literal send: ok");

    // A no-split terminal opens a new tab in the caller's space (P3.2).
    let tab_name = run.n("tab");
    let new_tab = pane_of(
        &run.cli(&[
            "launch",
            "--name",
            &tab_name,
            "--cmd",
            "echo tmux-tools live run: new tab",
            "--format",
            "json",
        ]),
        "launch",
    );
    let terminal = run.get(&new_tab);
    assert_eq!(
        terminal["labels"]["herdr.space"].as_str(),
        space.as_deref(),
        "{terminal}"
    );
    let made_up = terminal["labels"]["herdr.tab"].as_str();
    assert!(
        made_up.is_some_and(|made_up| !made_up.is_empty()) && made_up != tab.as_deref(),
        "a made-up herdr.tab that differs from the caller's tab: {terminal}"
    );
    println!("P8.4 no-split terminal {new_tab} ({tab_name}) opens a new tab: ok");

    // A Claude agent pane with Host state (P6.2).
    let claude_name = run.n("claude");
    let claude = pane_of(
        &run.cli(&[
            "spawn-agent",
            "claude",
            "--name",
            &claude_name,
            "--format",
            "json",
        ]),
        "spawn-agent claude",
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let status = json_payload(
            &run.cli(&["status", "--target", &claude_name, "--format", "json"]),
            "status claude",
        );
        if matches!(status["host_state"].as_str(), Some("idle" | "done")) {
            assert!(status["host_word"].is_string(), "{status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude never reported a Host state: {status}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(1));
    println!("P8.4 Claude pane {claude} ({claude_name}) reports Host state: ok");

    // A multi-line prompt to the Claude pane: guaranteed delivery (P6.6). The run-unique
    // token is a sum that only Claude's reply prints.
    let seed = u64::from(std::process::id());
    let addend = 141_421_u64;
    let token = (seed + addend).to_string();
    let prompt = format!(
        "Reply with only the decimal sum of {seed} and {addend}.\nDigits only, no other text."
    );
    let result = json_payload(
        &run.cli(&[
            "prompt",
            "--target",
            &claude_name,
            "--timeout",
            "120",
            "--format",
            "json",
            &prompt,
        ]),
        "prompt claude",
    );
    assert_eq!(result["one_submission_delivery"], "guaranteed", "{result}");
    assert_eq!(result["submit"], "confirmed", "{result}");
    assert!(
        result["output_since_prompt"]
            .as_str()
            .is_some_and(|output| output.contains(&token)),
        "the reply token {token}: {result}"
    );
    println!("P8.4 multi-line prompt to Claude, guaranteed delivery, reply {token}: ok");

    println!(
        "P8.4 holding the run's terminals for {}s for the Herdr window check",
        LIVE_HOLD.as_secs()
    );
    std::thread::sleep(LIVE_HOLD);

    // P8.5: kill every created terminal, then neither `tmux-tools list` nor
    // `hc ls --all --json` shows one.
    let names = run.names.borrow().clone();
    for name in &names {
        run.cli(&["kill", "--target", name, "--any"])
            .assert_success(&format!("kill {name}"));
    }
    let rows = json_payload(&run.cli(&["list", "--format", "json"]), "final list");
    let listed: Vec<&Value> = rows
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| {
                    row["name"]
                        .as_str()
                        .is_some_and(|name| names.iter().any(|own| own == name))
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(listed.is_empty(), "tmux-tools list shows {listed:?}");
    let created = [split, new_tab, claude];
    let left: Vec<Value> = run.hc_json(&["ls", "--all", "--json"])["terminals"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|row| {
            row["address"]
                .as_str()
                .is_some_and(|address| created.iter().any(|own| own == address))
                || row["where"]
                    .as_str()
                    .is_some_and(|name| names.iter().any(|own| own == name))
        })
        .collect();
    assert!(left.is_empty(), "hc ls --all --json shows {left:?}");
    println!("P8.5 teardown: no terminal of the run is left: ok");
}
