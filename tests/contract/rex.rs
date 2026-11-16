//! The Rex backend's contract tests (PRD-261112-2108-01 P8).
//!
//! The default run drives the binary against a fake `rex` (`fake_rex.py`, P8.2); no
//! real Rex server is reached. With `TT_TEST_REX=1` and a real `rex` on the original
//! `PATH`, [`rex_private_server_contract_suite`] runs the required verbs against a
//! private server it starts (`rex server run --listen unix://<dir>/s.sock …`) and, also
//! on failure, stops that server and deletes `<dir>` (P8.3). It never reaches the
//! default server. With `TMUX_TOOLS_LIVE_REX=1`, [`rex_live_contract_suite`] runs the
//! P8.4 cases on the default server from the calling plain Rex terminal and closes every
//! block it created (P8.6).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{json_payload, original_environment, tempfile_dir, wait_for_child, CmdOutput, BIN};

const FAKE_REX: &str = include_str!("fake_rex.py");

/// The fake's instance id of its default server.
const INSTANCE: &str = "inst-a";
/// The calling block, its session and its window.
const CALLER: &str = "block:00000000-0000-4000-8000-000000000001";
const SESSION: &str = "session:00000000-0000-4000-8000-0000000000aa";
const WINDOW: &str = "window:00000000-0000-4000-8000-0000000000bb";

fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in `rex` first on the CLI's `PATH`, its servers, and the run's dirs.
struct FakeRex {
    dir: PathBuf,
}

impl FakeRex {
    /// A fake whose default server `inst-a` holds the calling block in one session.
    fn start(label: &str) -> FakeRex {
        let dir = tempfile_dir(label);
        for sub in ["bin", "state", "config", "work"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/rex"), FAKE_REX);
        let fake = FakeRex { dir };
        fake.save(&json!({
            "next": 100,
            "servers": [server(&["default"], INSTANCE)],
        }));
        fake.add_block(INSTANCE, json!({"block_id": CALLER, "label": "lead"}));
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

    fn edit_server(&self, instance: &str, change: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        let server = state["servers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|server| server["instance_id"] == instance)
            .expect("the server exists");
        change(server);
        self.save(&state);
    }

    fn add_server(&self, addresses: &[&str], instance: &str) {
        let mut state = self.state();
        state["servers"]
            .as_array_mut()
            .unwrap()
            .push(server(addresses, instance));
        self.save(&state);
    }

    /// Add a block with `fields` over the defaults to `instance`; returns its id.
    fn add_block(&self, instance: &str, fields: Value) -> String {
        let mut state = self.state();
        let next = state["next"].as_u64().unwrap();
        state["next"] = json!(next + 1);
        let mut block = json!({
            "block_id": format!("block:00000000-0000-4000-8000-{next:012}"),
            "session_id": SESSION, "window_id": WINDOW, "label": "", "argv": [], "cwd": "/",
            "lines": [], "rows": 24, "columns": 80, "alternate": false, "modes": [],
            "records": [], "sent": [], "pasted": [], "keys": [], "on_send": {},
        });
        for (key, value) in fields.as_object().unwrap() {
            block[key] = value.clone();
        }
        let id = block["block_id"].as_str().unwrap().to_owned();
        let server = state["servers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|server| server["instance_id"] == instance)
            .unwrap();
        server["blocks"].as_array_mut().unwrap().push(block);
        self.save(&state);
        id
    }

    fn block(&self, id: &str) -> Option<Value> {
        self.state()["servers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|server| server["blocks"].as_array().unwrap().clone())
            .find(|block| block["block_id"] == id)
    }

    fn edit_block(&self, id: &str, change: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        let block = state["servers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .flat_map(|server| server["blocks"].as_array_mut().unwrap().iter_mut())
            .find(|block| block["block_id"] == id)
            .expect("the block exists");
        change(block);
        self.save(&state);
    }

    fn block_count(&self) -> usize {
        self.state()["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|server| server["blocks"].as_array().unwrap().len())
            .sum()
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

    /// Every logged call whose command words (after the global flags) start with
    /// `words`.
    fn calls(&self, words: &[&str]) -> Vec<Vec<String>> {
        self.log()
            .into_iter()
            .filter(|call| {
                let command = command_words(call);
                command.len() >= words.len() && command[..words.len()] == *words
            })
            .collect()
    }

    /// Every create (`run` or `split`) call.
    fn creates(&self) -> Vec<Vec<String>> {
        let mut creates = self.calls(&["run"]);
        creates.extend(self.calls(&["split"]));
        creates
    }

    /// `<XDG_STATE_HOME>/tmux-tools/rex`.
    fn registry(&self) -> PathBuf {
        self.dir.join("state/tmux-tools/rex")
    }

    fn record(&self, instance: &str, name: &str) -> Option<Value> {
        std::fs::read(self.registry().join(instance).join(format!("{name}.json")))
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }

    /// Write a Pane record `name` holding `handle` under `instance`, with its
    /// `server-address`.
    fn write_record(&self, instance: &str, address: &str, name: &str, handle: Option<&str>) {
        let dir = self.registry().join(instance);
        std::fs::create_dir_all(&dir).unwrap();
        if !dir.join("server-address").exists() {
            std::fs::write(dir.join("server-address"), address).unwrap();
        }
        let work = std::fs::canonicalize(self.dir.join("work")).unwrap();
        let record = json!({
            "schema": "tmux-tools.pane.v1", "name": name, "host": "rex", "server_id": instance,
            "handle": handle, "agent": null, "access": null, "surface": null,
            "surface_unvalidated": false, "cwd": work, "launched_at": "2026-11-15T00:00:00Z",
        });
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }

    fn server_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = std::fs::read_dir(self.registry())
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort();
        dirs
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)], remove: &[&str]) -> Command {
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
            .env("REX_BLOCK", CALLER)
            .env("REX_SESSION", SESSION)
            .env("SHELL", "/bin/sh")
            .env("XDG_STATE_HOME", self.dir.join("state"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("FAKE_REX_STATE", self.state_path())
            .env("FAKE_REX_LOG", self.dir.join("log.jsonl"));
        for (key, value) in env {
            command.env(key, value);
        }
        for key in remove {
            command.env_remove(key);
        }
        command
    }

    /// Run the CLI on Rex, selected by `REX_BLOCK`, with `env` added and `remove`
    /// removed. `TMUX` is unset.
    fn cli_with(&self, args: &[&str], env: &[(&str, &str)], remove: &[&str]) -> CmdOutput {
        let child = self
            .command(args, env, remove)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn tmux-tools");
        wait_for_child(
            child,
            &format!("tmux-tools {args:?}"),
            Duration::from_secs(60),
        )
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        self.cli_with(args, &[], &[])
    }
}

impl Drop for FakeRex {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A fake server reached at `addresses` with one session.
fn server(addresses: &[&str], instance: &str) -> Value {
    json!({
        "addresses": addresses, "instance_id": instance, "api_revision": 13,
        "sessions": [{"session_id": SESSION, "label": "main"}], "blocks": [],
    })
}

/// The command words of one logged call, after the global flags.
fn command_words(call: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    let mut index = 0;
    while index < call.len() {
        let arg = &call[index];
        if arg.starts_with("--autostart") {
            index += 1;
        } else if arg == "-S" || arg == "--timeout" {
            index += 2;
        } else if arg.starts_with('-') {
            break;
        } else {
            words.push(arg.clone());
            index += 1;
        }
    }
    words
}

/// The value after `flag` in one logged call.
fn flag_value<'a>(call: &'a [String], flag: &str) -> Option<&'a str> {
    let at = call.iter().position(|arg| arg == flag)?;
    call.get(at + 1).map(String::as_str)
}

fn has(call: &[String], arg: &str) -> bool {
    call.iter().any(|value| value == arg)
}

/// The command after `--` of one logged call.
fn command_of(call: &[String]) -> Vec<String> {
    let at = call.iter().position(|arg| arg == "--").expect("a `--`");
    call[at + 1..].to_vec()
}

/// A pane name of `length` characters.
fn long_name(length: usize) -> String {
    (0..length)
        .map(|index| (b'a' + (index % 26) as u8) as char)
        .collect()
}

// --- P2: selection ---------------------------------------------------------------------

/// Acceptance (P2.2, P7.2): with a fake `rex` and `REX_BLOCK` set and no `$TMUX` and
/// no `ORCA_TERMINAL_HANDLE`, `self` reports `host: "rex"`; P2.6: a stale `TMUX_PANE`
/// is never read for the calling pane (`self`, `status`).
#[test]
fn rex_block_selects_rex_and_the_calling_pane_is_that_block() {
    let fake = FakeRex::start("rex-select");
    let stale = [("TMUX_PANE", "%9")];
    let report = json_payload(
        &fake.cli_with(&["self", "--format", "json"], &stale, &[]),
        "self",
    );
    assert_eq!(report["host"], "rex", "{report}");
    assert_eq!(report["driven"], false, "{report}");

    // P7.2: driven when a live record holds `REX_BLOCK`.
    fake.write_record(INSTANCE, "unset", "lead", Some(CALLER));
    let report = json_payload(
        &fake.cli_with(&["self", "--format", "json"], &stale, &[]),
        "self driven",
    );
    assert_eq!(report["driven"], true, "{report}");
    assert_eq!(report["name"], "lead", "{report}");

    let status = json_payload(
        &fake.cli_with(&["status", "--format", "json"], &stale, &[]),
        "status",
    );
    assert_eq!(status["pane_id"], CALLER, "{status}");
    assert_eq!(status["session"], SESSION, "{status}");
    assert_eq!(status["window"], WINDOW, "{status}");
    assert!(status["host_word"].is_null(), "{status}");
    assert!(status["host_state"].is_null(), "{status}");

    // P2.6: without `REX_BLOCK`, `self` and `status` exit 1 naming it.
    for args in [
        vec!["self", "--format", "json"],
        vec!["status", "--format", "json"],
    ] {
        let out = fake.cli_with(&args, &[("TMUX_TOOLS_HOST", "rex")], &["REX_BLOCK"]);
        assert_eq!(out.status, 1, "{args:?}: {out:?}");
        assert!(out.stderr.contains("REX_BLOCK"), "{args:?}: {}", out.stderr);
    }
}

/// Acceptance (P2.1, P2.3, P2.4): an invalid `TMUX_TOOLS_HOST` names all four hosts;
/// every call passes `--autostart=false`, and `-S "$REX_SERVER"` when it is set; a
/// missing `rex` or a failing `rex server status` fails with exit 1 naming rex and the
/// selection source, and never falls back.
#[test]
fn an_unreachable_rex_names_the_host_and_the_selection_source() {
    let fake = FakeRex::start("rex-down");
    let invalid = fake.cli_with(&["self"], &[("TMUX_TOOLS_HOST", "bogus")], &[]);
    assert_eq!(invalid.status, 1, "{invalid:?}");
    for host in ["tmux", "orca", "rex", "herdr"] {
        assert!(invalid.stderr.contains(host), "{}", invalid.stderr);
    }

    fake.add_server(&["unix:///tmp/tt-rex.sock"], "inst-b");
    fake.cli_with(
        &["list", "--format", "json"],
        &[("REX_SERVER", "unix:///tmp/tt-rex.sock")],
        &[],
    )
    .assert_success("list with REX_SERVER");
    let log = fake.log();
    assert!(!log.is_empty());
    for call in &log {
        assert!(has(call, "--autostart=false"), "{call:?}");
        assert_eq!(
            flag_value(call, "-S"),
            Some("unix:///tmp/tt-rex.sock"),
            "{call:?}"
        );
    }

    fake.clear_log();
    fake.edit_server(INSTANCE, |server| server["down"] = json!(true));
    let down = fake.cli(&["list"]);
    assert_eq!(down.status, 1, "{down:?}");
    assert!(
        down.stderr.contains("rex") && down.stderr.contains("REX_BLOCK"),
        "{}",
        down.stderr
    );
    let down = fake.cli_with(&["list"], &[("TMUX_TOOLS_HOST", "rex")], &[]);
    assert_eq!(down.status, 1, "{down:?}");
    assert!(
        down.stderr.contains("rex") && down.stderr.contains("TMUX_TOOLS_HOST"),
        "{}",
        down.stderr
    );

    let without_rex = Command::new(BIN)
        .args(["self", "--format", "json"])
        .env_remove("TMUX")
        .env("REX_BLOCK", CALLER)
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    assert_eq!(without_rex.status.code(), Some(1), "{without_rex:?}");
    let stderr = String::from_utf8_lossy(&without_rex.stderr);
    assert!(
        stderr.contains("rex") && stderr.contains("REX_BLOCK"),
        "{stderr}"
    );
}

// --- P3: placement ---------------------------------------------------------------------

/// Acceptance (P3.3–P3.4): `launch --split v --name N` with a 129-character `N` runs one
/// `split --split=below -b <calling block>` with `--focus=false`, the first 128
/// characters of `N` as `--label`, `--cwd`, `--json`, `--autostart=false`, and `-S
/// <REX_SERVER>`, and no `--keep-open`. A 128-character name is the label unchanged.
#[test]
fn launch_split_v_splits_below_the_calling_block_with_a_128_character_label() {
    let fake = FakeRex::start("rex-place");
    fake.edit_server(INSTANCE, |server| {
        server["addresses"] = json!(["default", "unix:///tmp/tt-place.sock"])
    });
    let name = long_name(129);
    fake.cli_with(
        &[
            "launch", "--split", "v", "--name", &name, "--cmd", "sleep 30",
        ],
        &[("REX_SERVER", "unix:///tmp/tt-place.sock")],
        &[],
    )
    .assert_success("launch --split v");
    let creates = fake.creates();
    assert_eq!(creates.len(), 1, "{creates:?}");
    let split = &creates[0];
    assert_eq!(command_words(split), ["split"], "{split:?}");
    assert!(has(split, "--split=below"), "{split:?}");
    assert_eq!(flag_value(split, "-b"), Some(CALLER), "{split:?}");
    assert!(has(split, "--focus=false"), "{split:?}");
    assert_eq!(
        flag_value(split, "--label"),
        Some(&name[..128]),
        "{split:?}"
    );
    let work = std::fs::canonicalize(fake.dir.join("work")).unwrap();
    assert_eq!(
        flag_value(split, "--cwd"),
        Some(work.to_str().unwrap()),
        "{split:?}"
    );
    assert!(has(split, "--json"), "{split:?}");
    assert!(has(split, "--autostart=false"), "{split:?}");
    assert_eq!(
        flag_value(split, "-S"),
        Some("unix:///tmp/tt-place.sock"),
        "{split:?}"
    );
    assert!(!has(split, "--keep-open"), "{split:?}");
    // Pane names keep no length limit: the record has the whole name.
    let record = fake.record(INSTANCE, &name).expect("the record");
    assert_eq!(record["name"], name.as_str(), "{record}");

    fake.clear_log();
    let name = long_name(128);
    fake.cli(&["launch", "--split", "h", "--name", &name, "--cmd", "true"])
        .assert_success("launch --split h");
    let split = &fake.creates()[0];
    assert!(has(split, "--split=right"), "{split:?}");
    assert_eq!(
        flag_value(split, "--label"),
        Some(name.as_str()),
        "{split:?}"
    );
}

/// P3.1–P3.2: no `--split` (and `--split window`) runs `rex run -s <session>`; the
/// session is the `--target` block's, else `REX_SESSION`, else
/// `TMUX_TOOLS_REX_SESSION`; with none the spawn exits 1 naming both and creates
/// nothing. P3.3: a split with no target and no `REX_BLOCK` exits 1 naming it.
#[test]
fn a_new_window_goes_to_the_target_session_else_the_environment() {
    let fake = FakeRex::start("rex-session");
    fake.cli(&["launch", "--name", "w1", "--cmd", "true"])
        .assert_success("launch");
    let run = &fake.creates()[0];
    assert_eq!(command_words(run), ["run"], "{run:?}");
    assert_eq!(flag_value(run, "-s"), Some(SESSION), "{run:?}");
    assert!(has(run, "--focus=false"), "{run:?}");

    // The target's session wins.
    let other_session = "session:00000000-0000-4000-8000-0000000000cc";
    fake.edit_server(INSTANCE, |server| {
        server["sessions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"session_id": other_session, "label": "other"}))
    });
    let other = fake.add_block(INSTANCE, json!({"session_id": other_session}));
    fake.clear_log();
    fake.cli(&[
        "launch", "--split", "window", "--target", &other, "--name", "w2", "--cmd", "true",
    ])
    .assert_success("launch --target");
    assert_eq!(
        flag_value(&fake.creates()[0], "-s"),
        Some(other_session),
        "{:?}",
        fake.log()
    );

    // Else `TMUX_TOOLS_REX_SESSION`.
    fake.clear_log();
    fake.cli_with(
        &["launch", "--name", "w3", "--cmd", "true"],
        &[("TMUX_TOOLS_REX_SESSION", other_session)],
        &["REX_SESSION"],
    )
    .assert_success("launch from TMUX_TOOLS_REX_SESSION");
    assert_eq!(flag_value(&fake.creates()[0], "-s"), Some(other_session));

    // No session anywhere: exit 1, nothing created, no reservation.
    fake.clear_log();
    let out = fake.cli_with(
        &["launch", "--name", "w4", "--cmd", "true"],
        &[],
        &["REX_SESSION"],
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("REX_SESSION") && out.stderr.contains("TMUX_TOOLS_REX_SESSION"),
        "{}",
        out.stderr
    );
    assert!(fake.creates().is_empty(), "{:?}", fake.log());
    assert!(fake.record(INSTANCE, "w4").is_none());

    // A split with no anchor: exit 1 naming `REX_BLOCK`.
    let out = fake.cli_with(
        &["launch", "--split", "v", "--name", "w5", "--cmd", "true"],
        &[("TMUX_TOOLS_HOST", "rex")],
        &["REX_BLOCK"],
    );
    assert_eq!(out.status, 1, "{out:?}");
    assert!(out.stderr.contains("REX_BLOCK"), "{}", out.stderr);
    assert!(fake.creates().is_empty(), "{:?}", fake.log());
}

/// Acceptance (P3.5): `rebalance`, `--split tile`, `--size`, `--session` and
/// `--window` exit 3 with the PRD-261107-0825-01 P5.3 message and no create.
#[test]
fn optional_placement_exits_3_on_rex_without_a_create() {
    let fake = FakeRex::start("rex-exit3");
    for (args, message) in [
        (vec!["rebalance"], "unsupported: rebalance on rex"),
        (
            vec!["launch", "--split", "tile", "--cmd", "true"],
            "unsupported: --split tile on rex",
        ),
        (
            vec!["launch", "--size", "30", "--cmd", "true"],
            "unsupported: --size on rex",
        ),
        (
            vec!["launch", "--session", "s", "--cmd", "true"],
            "unsupported: --session on rex",
        ),
        (
            vec!["launch", "--window", "w", "--cmd", "true"],
            "unsupported: --window on rex",
        ),
    ] {
        let out = fake.cli(&args);
        assert_eq!(out.status, 3, "{args:?}: {out:?}");
        assert!(out.stderr.contains(message), "{args:?}: {}", out.stderr);
    }
    assert!(fake.creates().is_empty(), "{:?}", fake.log());
}

// --- P5: capture, send, and keys ---------------------------------------------------------

/// Acceptance (P1, P5.1): a block id with no registration is captured directly; a
/// registered name is captured through its record. `--lines n` is the last `n` lines,
/// `--lines 0` and no option the last `rows` lines, `--all` the whole capture; the
/// no-scrollback notice fires when `modes` reports the alternate screen.
#[test]
fn capture_resolves_a_name_or_a_block_id_and_reads_rex_capture() {
    let fake = FakeRex::start("rex-capture");
    let lines = json!(["l1", "l2", "l3", "l4", "l5"]);
    let bare = fake.add_block(INSTANCE, json!({"lines": lines, "rows": 2}));
    let named = fake.add_block(INSTANCE, json!({"lines": ["N1", "N2"]}));
    fake.write_record(INSTANCE, "unset", "N", Some(&named));

    let out = fake.cli(&["capture", "--target", &bare, "--all"]);
    out.assert_success("capture block id");
    assert_eq!(out.stdout, "l1\nl2\nl3\nl4\nl5", "{out:?}");
    assert!(!out.stderr.contains("no scrollback"), "{}", out.stderr);
    let captures = fake.calls(&["capture"]);
    assert_eq!(flag_value(&captures[0], "-b"), Some(bare.as_str()));
    assert!(!has(&captures[0], "--unwrap"), "{captures:?}");

    fake.clear_log();
    let out = fake.cli(&["capture", "--target", "N", "--all"]);
    out.assert_success("capture name");
    assert_eq!(out.stdout, "N1\nN2", "{out:?}");
    assert_eq!(
        flag_value(&fake.calls(&["capture"])[0], "-b"),
        Some(named.as_str())
    );

    let out = fake.cli(&["capture", "--target", &bare, "--lines", "2"]);
    assert_eq!(out.stdout, "l4\nl5", "{out:?}");
    let out = fake.cli(&["capture", "--target", &bare, "--lines", "0"]);
    assert_eq!(
        out.stdout, "l4\nl5",
        "--lines 0 is the last rows lines: {out:?}"
    );
    let out = fake.cli(&["capture", "--target", &bare]);
    assert_eq!(out.stdout, "l4\nl5", "{out:?}");

    fake.edit_block(&bare, |block| block["alternate"] = json!(true));
    let out = fake.cli(&["capture", "--target", &bare, "--all"]);
    out.assert_success("capture --all on the alternate screen");
    assert!(out.stderr.contains("no scrollback"), "{}", out.stderr);

    // An unknown name that is not a block id fails.
    let out = fake.cli(&["capture", "--target", "nobody"]);
    assert_eq!(out.status, 1, "{out:?}");
}

/// P5.3–P5.5: key arguments become xterm bytes and literal text stays text, both
/// through `rex send -b <block>`; cursor keys use application-mode bytes when mode 1 is
/// on; `--enter` adds `\r`; `send-enter` sends `\r` and `escape` `\x1b`.
#[test]
fn send_writes_xterm_bytes_through_rex_send() {
    let fake = FakeRex::start("rex-send");
    let pane = fake.add_block(INSTANCE, json!({}));
    fake.write_record(INSTANCE, "unset", "p", Some(&pane));
    fake.cli(&["send", "--target", "p", "hi there", "--enter"])
        .assert_success("literal send");
    fake.cli(&["send", "--target", "p", "C-c"])
        .assert_success("key send");
    fake.cli(&["send", "--target", "p", "Up"])
        .assert_success("Up in normal mode");
    fake.edit_block(&pane, |block| block["modes"] = json!([1]));
    fake.cli(&["send", "--target", "p", "Up"])
        .assert_success("Up in application mode");
    fake.cli(&["send-enter", "--target", "p"])
        .assert_success("send-enter");
    fake.cli(&["escape", "--target", "p", "--any"])
        .assert_success("escape");
    let sent: Vec<String> = fake.block(&pane).unwrap()["sent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|text| text.as_str().unwrap().to_owned())
        .collect();
    let joined: String = sent.concat();
    assert_eq!(
        joined,
        "hi there\r\x03\x1b[A\x1bOA\r\x1b",
        "{sent:?}: {:?}",
        fake.log()
    );
    for call in fake.calls(&["send"]) {
        assert_eq!(flag_value(&call, "-b"), Some(pane.as_str()), "{call:?}");
    }
}

// --- P4: registration, names, and lifetime ---------------------------------------------

/// P4.1: the Pane record is `rex/<instance_id>/<name>.json` with `server_id` the
/// `instance_id` and `handle` the block id; the server directory holds
/// `server-address` (`unset` without `REX_SERVER`). A taken name fails with exit 1 and
/// creates no block (PRD-261107-0825-01 P4.4).
#[test]
fn the_pane_record_lives_under_the_instance_id_and_a_taken_name_fails() {
    let fake = FakeRex::start("rex-record");
    fake.write_record(INSTANCE, "unset", "lead", Some(CALLER));
    let out = fake.cli(&[
        "launch", "--name", "rec", "--cmd", "sleep 30", "--format", "json",
    ]);
    let launched = json_payload(&out, "launch");
    let block = launched["pane_id"].as_str().unwrap().to_owned();
    assert!(block.starts_with("block:"), "{launched}");
    assert_eq!(launched["host"], "rex", "{launched}");
    let window = fake.block(&block).unwrap()["window_id"].clone();
    assert_eq!(launched["window_id"], window, "{launched}");
    for key in ["relocated", "min_pane_cols", "min_pane_rows", "below_floor"] {
        assert!(launched[key].is_null(), "{key}: {launched}");
    }
    let record = fake.record(INSTANCE, "rec").expect("the record");
    assert_eq!(record["host"], "rex", "{record}");
    assert_eq!(record["server_id"], INSTANCE, "{record}");
    assert_eq!(record["handle"], block.as_str(), "{record}");
    // PRD-261108-0447-01 P7: the calling tmux-tools pane is the parent.
    assert_eq!(record["parent"], "lead", "{record}");
    assert_eq!(
        std::fs::read_to_string(fake.registry().join(INSTANCE).join("server-address")).unwrap(),
        "unset"
    );

    let blocks = fake.block_count();
    let out = fake.cli(&["launch", "--name", "rec", "--cmd", "sleep 30"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert_eq!(fake.block_count(), blocks);
}

/// P4.3, P7.3: a record whose block a complete listing lacks is `dead`: `list` shows it
/// once and deletes it. Live rows carry the block id and the Rex session id, `status:
/// running`, and no `window` key. A listing whose `rex block ls` fails is incomplete:
/// the record stays, `list` shows `status: unknown` and prints `list incomplete: rex`,
/// and resolving that name fails with `pane list incomplete: <name>`.
#[test]
fn list_reports_live_dead_and_unknown_records() {
    let fake = FakeRex::start("rex-list");
    let live = fake.add_block(INSTANCE, json!({}));
    fake.write_record(INSTANCE, "unset", "a", Some(&live));
    fake.write_record(
        INSTANCE,
        "unset",
        "gone",
        Some("block:00000000-0000-4000-8000-999999999999"),
    );
    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let row = rows.iter().find(|row| row["name"] == "a").unwrap();
    assert_eq!(row["pane"], live.as_str(), "{row}");
    assert_eq!(row["session"], SESSION, "{row}");
    assert_eq!(row["status"], "running", "{row}");
    assert_eq!(row["host"], "rex", "{row}");
    assert!(row.get("window").is_none(), "{row}");
    assert!(row["last_activity"].is_null(), "{row}");
    let gone = rows.iter().find(|row| row["name"] == "gone").unwrap();
    assert_eq!(gone["status"], "dead", "{gone}");
    assert!(
        fake.record(INSTANCE, "gone").is_none(),
        "the dead record is deleted"
    );

    fake.write_record(
        INSTANCE,
        "unset",
        "lost",
        Some("block:00000000-0000-4000-8000-888888888888"),
    );
    fake.edit_server(INSTANCE, |server| {
        server["fail_block_ls"] = json!([SESSION])
    });
    let out = fake.cli(&["list", "--format", "json"]);
    out.assert_success("list on an incomplete listing");
    assert!(
        out.stderr.contains("list incomplete: rex"),
        "{}",
        out.stderr
    );
    let rows: Value = serde_json::from_str(&out.stdout).unwrap();
    let lost = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "lost")
        .cloned()
        .unwrap();
    assert_eq!(lost["status"], "unknown", "{lost}");
    assert!(fake.record(INSTANCE, "lost").is_some(), "the record stays");
    let out = fake.cli(&["capture", "--target", "lost"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("pane list incomplete: lost"),
        "{}",
        out.stderr
    );
}

/// A second session holding one block registered as `b`; the first holds `a`.
fn two_session_records(fake: &FakeRex) {
    fake.edit_server(INSTANCE, |server| {
        server["sessions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"session_id": SESSION2, "label": "second"}))
    });
    let first = fake.add_block(INSTANCE, json!({}));
    fake.write_record(INSTANCE, "unset", "a", Some(&first));
    let second = fake.add_block(INSTANCE, json!({"session_id": SESSION2}));
    fake.write_record(INSTANCE, "unset", "b", Some(&second));
}

/// The second session of [`two_session_records`].
const SESSION2: &str = "session:00000000-0000-4000-8000-0000000000cc";

/// `list --format json` on an incomplete listing: exit 0, `list incomplete: rex`, and
/// the status of each named row; every record stays.
fn assert_incomplete_list(fake: &FakeRex, env: &[(&str, &str)], expect: &[(&str, &str)]) {
    let out = fake.cli_with(&["list", "--format", "json"], env, &[]);
    out.assert_success("list on an incomplete listing");
    assert!(
        out.stderr.contains("list incomplete: rex"),
        "{}",
        out.stderr
    );
    let rows: Value = serde_json::from_str(&out.stdout).unwrap();
    for (name, status) in expect {
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == *name)
            .unwrap_or_else(|| panic!("no row {name}: {rows}"));
        assert_eq!(row["status"], *status, "{name}: {row}");
        assert!(
            fake.record(INSTANCE, name).is_some(),
            "{name}: the record stays"
        );
    }
}

/// The call timeout of the M2 cases, in seconds.
const SHORT_CALL: (&str, &str) = ("TMUX_TOOLS_TEST_REX_CALL_TIMEOUT", "1");

/// Acceptance (P4.3): a `rex ls --json` that times out or answers output that does not
/// parse makes the listing incomplete: records stay, rows are `unknown`, `list
/// incomplete: rex` is printed, and a verb on a name fails with `pane list incomplete:
/// <name>`.
#[test]
fn a_timed_out_or_unparseable_rex_ls_makes_the_listing_incomplete() {
    for (case, change) in [
        ("timeout", json!({"ls_sleep": 5})),
        ("garbage", json!({"ls_garbage": true})),
    ] {
        let fake = FakeRex::start(&format!("rex-ls-{case}"));
        two_session_records(&fake);
        fake.edit_server(INSTANCE, |server| {
            for (key, value) in change.as_object().unwrap() {
                server[key] = value.clone();
            }
        });
        assert_incomplete_list(&fake, &[SHORT_CALL], &[("a", "unknown"), ("b", "unknown")]);
        let out = fake.cli_with(&["capture", "--target", "a"], &[SHORT_CALL], &[]);
        assert_eq!(out.status, 1, "{case}: {out:?}");
        assert!(
            out.stderr.contains("pane list incomplete: a"),
            "{case}: {}",
            out.stderr
        );
    }
}

/// Acceptance (P4.3): a `rex block ls -s <session> --json` that times out or answers
/// output that does not parse makes the listing incomplete; the other session's blocks
/// stay listed, so only that session's record is `unknown` and fails by name.
#[test]
fn a_timed_out_or_unparseable_block_ls_keeps_the_other_sessions() {
    for (case, key) in [
        ("timeout", "block_ls_sleep"),
        ("garbage", "block_ls_garbage"),
    ] {
        let fake = FakeRex::start(&format!("rex-block-ls-{case}"));
        two_session_records(&fake);
        fake.edit_server(INSTANCE, |server| server[key] = json!([SESSION2]));
        assert_incomplete_list(&fake, &[SHORT_CALL], &[("a", "running"), ("b", "unknown")]);
        let out = fake.cli_with(&["capture", "--target", "b"], &[SHORT_CALL], &[]);
        assert_eq!(out.status, 1, "{case}: {out:?}");
        assert!(
            out.stderr.contains("pane list incomplete: b"),
            "{case}: {}",
            out.stderr
        );
        fake.cli_with(&["capture", "--target", "a"], &[SHORT_CALL], &[])
            .assert_success("a listed name still resolves");
    }
}

/// Acceptance (P4.4): a create that times out with one block in the target session
/// whose label is the name's first 128 characters gives the record that block id;
/// creation is not repeated. Output that does not parse is ambiguous the same way.
#[test]
fn an_ambiguous_create_adopts_the_one_block_with_the_label() {
    let fake = FakeRex::start("rex-adopt");
    let name = long_name(129);
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 1, "make": true})
    });
    let out = fake.cli(&[
        "launch", "--name", &name, "--cmd", "sleep 30", "--format", "json",
    ]);
    let launched = json_payload(&out, "an ambiguous create");
    assert_eq!(fake.creates().len(), 1, "creation is never repeated");
    let made = fake.state()["servers"][0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["label"] == name[..128])
        .unwrap()["block_id"]
        .clone();
    assert_eq!(launched["pane_id"], made, "{launched}");
    assert_eq!(fake.record(INSTANCE, &name).unwrap()["handle"], made);
    assert!(!fake.calls(&["block", "ls"]).is_empty());

    fake.clear_log();
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 0, "make": true, "garbage": true})
    });
    fake.cli(&["launch", "--name", "g", "--cmd", "true"])
        .assert_success("output that does not parse");
    assert!(fake.record(INSTANCE, "g").unwrap()["handle"].is_string());
    assert_eq!(fake.creates().len(), 1);
}

/// Acceptance (P4.4): an ambiguous create is not adopted, and exits 1 with `spawn
/// outcome unknown: <name> (session <session id>)` keeping the reservation, when no
/// block has the label, or when another reservation shares the name's first 128
/// characters; creation is not repeated. Exit 3 and exit 4 are definite failures: the
/// reservation is deleted.
#[test]
fn an_unresolved_create_keeps_the_reservation_and_a_definite_failure_deletes_it() {
    let fake = FakeRex::start("rex-unknown");
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 1, "make": false})
    });
    let out = fake.cli(&["launch", "--name", "u1", "--cmd", "true"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr
            .contains(&format!("spawn outcome unknown: u1 (session {SESSION})")),
        "{}",
        out.stderr
    );
    assert_eq!(fake.creates().len(), 1, "creation is never repeated");
    let reservation = fake.record(INSTANCE, "u1").expect("the reservation stays");
    assert!(reservation["handle"].is_null(), "{reservation}");

    // Another reservation shares the first 128 characters: no adoption.
    let prefix = long_name(128);
    let mine = format!("{prefix}x");
    let theirs = format!("{prefix}y");
    fake.write_record(INSTANCE, "unset", &theirs, None);
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 1, "make": true})
    });
    fake.clear_log();
    let out = fake.cli(&["launch", "--name", &mine, "--cmd", "true"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr
            .contains(&format!("spawn outcome unknown: {mine}")),
        "{}",
        out.stderr
    );
    assert!(fake.record(INSTANCE, &mine).unwrap()["handle"].is_null());
    assert_eq!(fake.creates().len(), 1);

    for code in [3, 4] {
        fake.edit_server(INSTANCE, |server| {
            server["create"] = json!({"exit": code, "make": false})
        });
        let name = format!("definite{code}");
        let out = fake.cli(&["launch", "--name", &name, "--cmd", "true"]);
        assert_eq!(out.status, 1, "exit {code}: {out:?}");
        assert!(
            !out.stderr.contains("spawn outcome unknown"),
            "{}",
            out.stderr
        );
        assert!(
            fake.record(INSTANCE, &name).is_none(),
            "exit {code}: the reservation is deleted"
        );
    }
}

/// P4.4: a candidate block that a live record already holds is not adopted.
#[test]
fn an_ambiguous_create_never_adopts_a_block_a_live_record_holds() {
    let fake = FakeRex::start("rex-held");
    let held = fake.add_block(INSTANCE, json!({"label": "h"}));
    fake.write_record(INSTANCE, "unset", "other", Some(&held));
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 1, "make": false})
    });
    let out = fake.cli(&["launch", "--name", "h", "--cmd", "true"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("spawn outcome unknown: h"),
        "{}",
        out.stderr
    );
    assert!(fake.record(INSTANCE, "h").unwrap()["handle"].is_null());
}

/// P4.4: after a timed-out create, two blocks with the name's label are not adopted;
/// the spawn exits 1 with `spawn outcome unknown`, keeps the reservation without a
/// handle, and does not repeat the create.
#[test]
fn an_ambiguous_create_with_two_matching_labels_is_unknown() {
    let fake = FakeRex::start("rex-two-labels");
    fake.add_block(INSTANCE, json!({"label": "dup"}));
    fake.edit_server(INSTANCE, |server| {
        server["create"] = json!({"exit": 1, "make": true})
    });
    let out = fake.cli(&["launch", "--name", "dup", "--cmd", "true"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr
            .contains(&format!("spawn outcome unknown: dup (session {SESSION})")),
        "{}",
        out.stderr
    );
    let labelled = fake.state()["servers"][0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["label"] == "dup")
        .count();
    assert_eq!(labelled, 2, "the create made the second block");
    assert_eq!(fake.creates().len(), 1, "creation is never repeated");
    let reservation = fake.record(INSTANCE, "dup").expect("the reservation stays");
    assert!(reservation["handle"].is_null(), "{reservation}");
}

/// Acceptance (P4.1): one server reached once with `REX_SERVER` unset and once with
/// its explicit address uses one `rex/<instance_id>/` directory, whose `server-address`
/// stays the first one written, and neither record is deleted.
#[test]
fn one_server_reached_two_ways_uses_one_directory() {
    let fake = FakeRex::start("rex-two-ways");
    fake.edit_server(INSTANCE, |server| {
        server["addresses"] = json!(["default", "unix:///tmp/tt-two.sock"])
    });
    fake.cli(&["launch", "--name", "first", "--cmd", "true"])
        .assert_success("REX_SERVER unset");
    fake.cli_with(
        &["launch", "--name", "second", "--cmd", "true"],
        &[("REX_SERVER", "unix:///tmp/tt-two.sock")],
        &[],
    )
    .assert_success("explicit address");
    fake.cli(&["list"]).assert_success("list");
    assert_eq!(fake.server_dirs(), [INSTANCE]);
    assert!(fake.record(INSTANCE, "first").is_some());
    assert!(fake.record(INSTANCE, "second").is_some());
    assert_eq!(
        std::fs::read_to_string(fake.registry().join(INSTANCE).join("server-address")).unwrap(),
        "unset"
    );
}

/// A fake with a second live server `inst-live` whose directory holds a record.
fn sweep_fake(label: &str) -> FakeRex {
    let fake = FakeRex::start(label);
    fake.add_server(&["unix:///tmp/tt-live.sock"], "inst-live");
    fake.write_record("inst-live", "unix:///tmp/tt-live.sock", "l", None);
    fake
}

/// Run one sweep (`list`) and require that only the live server's directory remains;
/// every address check passes `--autostart=false`.
fn assert_swept_to_live(fake: &FakeRex) {
    fake.cli(&["list"]).assert_success("list sweeps");
    assert_eq!(fake.server_dirs(), ["inst-live"], "{:?}", fake.log());
    for call in fake.calls(&["server", "status"]) {
        assert!(has(&call, "--autostart=false"), "{call:?}");
    }
}

/// Acceptance (P4.1): a directory whose stored address fails is deleted; a live
/// server's directory survives.
#[test]
fn the_sweep_deletes_a_directory_whose_address_check_fails() {
    let fake = sweep_fake("rex-sweep-failing");
    fake.write_record("inst-failing", "unix:///tmp/tt-none.sock", "f", None);
    assert_swept_to_live(&fake);
}

/// Acceptance (P4.1): a directory whose address check runs past 2 seconds is deleted;
/// a live server's directory survives.
#[test]
fn the_sweep_deletes_a_directory_whose_address_check_times_out() {
    let fake = sweep_fake("rex-sweep-slow");
    fake.add_server(&["unix:///tmp/tt-slow.sock"], "inst-slow");
    fake.edit_server("inst-slow", |server| server["status_sleep"] = json!(5));
    fake.write_record("inst-slow", "unix:///tmp/tt-slow.sock", "s", None);
    let started = Instant::now();
    assert_swept_to_live(&fake);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the slow check times out after 2 s: {:?}",
        started.elapsed()
    );
    assert!(
        fake.calls(&["server", "status"])
            .iter()
            .any(|call| flag_value(call, "-S") == Some("unix:///tmp/tt-slow.sock")),
        "{:?}",
        fake.log()
    );
}

/// Acceptance (P4.1): a directory whose stored address reports a different
/// `instance_id` is deleted; a live server's directory survives.
#[test]
fn the_sweep_deletes_a_directory_whose_address_reports_another_instance() {
    let fake = sweep_fake("rex-sweep-moved");
    fake.add_server(&["unix:///tmp/tt-moved.sock"], "inst-new");
    fake.write_record("inst-old", "unix:///tmp/tt-moved.sock", "o", None);
    assert_swept_to_live(&fake);
}

/// Acceptance (P4.1): a directory with no readable `server-address` is deleted; a
/// live server's directory survives.
#[test]
fn the_sweep_deletes_a_directory_without_a_server_address() {
    let fake = sweep_fake("rex-sweep-noaddress");
    std::fs::create_dir_all(fake.registry().join("inst-noaddress")).unwrap();
    assert_swept_to_live(&fake);
}

/// Acceptance (P4.1): a live `unset` directory survives a sweep run with `REX_SERVER`
/// set: its check has no `-S` and no `REX_SERVER`, so it reaches the default server.
#[test]
fn the_sweep_keeps_a_live_unset_directory_when_rex_server_is_set() {
    let fake = sweep_fake("rex-sweep-unset");
    fake.write_record(INSTANCE, "unset", "d", None);
    fake.cli_with(
        &["list"],
        &[("REX_SERVER", "unix:///tmp/tt-live.sock")],
        &[],
    )
    .assert_success("list from the live server");
    assert_eq!(
        fake.server_dirs(),
        [INSTANCE, "inst-live"],
        "{:?}",
        fake.log()
    );
}

/// P4.1: a `.init-` directory older than 120 seconds is deleted, a newer one is kept,
/// and every other entry whose name starts with `.` is skipped.
#[test]
fn a_stale_init_directory_is_deleted_and_a_new_one_kept() {
    let fake = FakeRex::start("rex-init");
    let registry = fake.registry();
    for dir in [".init-old", ".init-new", ".other"] {
        std::fs::create_dir_all(registry.join(dir)).unwrap();
    }
    let touched = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(registry.join(".init-old"))
        .status()
        .unwrap();
    assert!(touched.success());
    fake.cli(&["list"]).assert_success("list");
    assert_eq!(fake.server_dirs(), [".init-new", ".other"]);
}

/// P4.1: when `rex/<instance_id>/` appears after the missing-directory check and
/// before the rename, the spawn succeeds, its record is in the existing directory,
/// that directory's `server-address` is unchanged, and no `.init-` directory remains.
#[test]
fn a_directory_published_meanwhile_is_used_as_it_is() {
    let fake = FakeRex::start("rex-publish-race");
    let target = fake.registry().join(INSTANCE);
    let hook = format!(
        "[ \"$1\" = before-rename ] && mkdir -p '{dir}' && printf other > '{dir}/server-address'",
        dir = target.display()
    );
    fake.cli_with(
        &["launch", "--name", "raced", "--cmd", "true"],
        &[("TMUX_TOOLS_TEST_REX_HOOK", &hook)],
        &[],
    )
    .assert_success("launch while another invocation publishes");
    assert!(fake.record(INSTANCE, "raced").unwrap()["handle"].is_string());
    assert_eq!(
        std::fs::read_to_string(target.join("server-address")).unwrap(),
        "other"
    );
    assert_eq!(fake.server_dirs(), [INSTANCE]);
}

/// Acceptance (P4.1): a sweep from another live server that runs while a new server
/// directory is being published leaves it alone; the reservation survives, and a
/// second spawn of the same name is refused.
#[test]
fn a_sweep_during_publication_keeps_the_new_directory() {
    let fake = FakeRex::start("rex-publish-sweep");
    fake.add_server(&["unix:///tmp/tt-other.sock"], "inst-other");
    // The sweep runs inside the publication, between the complete `server-address`
    // and the rename, from another live server.
    let sweep = fake
        .command(
            &["list"],
            &[("REX_SERVER", "unix:///tmp/tt-other.sock")],
            &[],
        )
        .get_envs()
        .filter_map(|(key, value)| Some(format!("{}='{}'", key.to_str()?, value?.to_str()?)))
        .collect::<Vec<_>>()
        .join(" ");
    let hook = format!(
        "[ \"$1\" = before-rename ] && cd '{}' && env -u TMUX -u TMUX_PANE {sweep} '{BIN}' list >/dev/null 2>&1",
        fake.dir.join("work").display()
    );
    fake.cli_with(
        &["launch", "--name", "fresh", "--cmd", "true"],
        &[("TMUX_TOOLS_TEST_REX_HOOK", &hook)],
        &[],
    )
    .assert_success("launch while another server sweeps");
    let sweeps = fake
        .calls(&["server", "status"])
        .into_iter()
        .filter(|call| flag_value(call, "-S") == Some("unix:///tmp/tt-other.sock"))
        .count();
    assert!(
        sweeps >= 1,
        "the other server's invocation ran: {:?}",
        fake.log()
    );
    assert!(
        fake.record(INSTANCE, "fresh").is_some(),
        "{:?}",
        fake.server_dirs()
    );
    // Another sweep from the other server keeps the published directory.
    fake.cli_with(
        &["list"],
        &[("REX_SERVER", "unix:///tmp/tt-other.sock")],
        &[],
    )
    .assert_success("a second sweep");
    assert!(fake.record(INSTANCE, "fresh").is_some());
    let blocks = fake.block_count();
    let out = fake.cli(&["launch", "--name", "fresh", "--cmd", "true"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert_eq!(fake.block_count(), blocks);
}

// --- P6: Host state and prompt ---------------------------------------------------------

/// A block registered as `a` with `fields`, for the `prompt` cases.
fn prompt_block(fake: &FakeRex, fields: Value) -> String {
    let mut fields = fields;
    if fields.get("modes").is_none() {
        fields["modes"] = json!([2004]);
    }
    let block = fake.add_block(INSTANCE, fields);
    fake.write_record(INSTANCE, "unset", "a", Some(&block));
    block
}

fn prompt(fake: &FakeRex, timeout: &str, text: &str) -> Value {
    json_payload(
        &fake.cli(&[
            "prompt",
            "--target",
            "a",
            "--timeout",
            timeout,
            "--format",
            "json",
            text,
        ]),
        "prompt",
    )
}

/// P6.2: Host state is the newest `program_status` record's state (`error` maps to
/// `blocked`, no record to `null`); `host_word` is that record's raw state.
#[test]
fn status_reports_the_newest_program_status_record() {
    let fake = FakeRex::start("rex-status");
    let block = prompt_block(
        &fake,
        json!({"records": [
            {"state": "working", "updated_at": "2026-01-01T00:00:02Z"},
            {"id": "x", "state": "done", "updated_at": "2026-01-01T00:00:01-01:00"},
        ]}),
    );
    let status = json_payload(
        &fake.cli(&["status", "--target", "a", "--format", "json"]),
        "status",
    );
    // 00:00:01-01:00 is 01:00:01Z, newer than 00:00:02Z.
    assert_eq!(status["host_word"], "done", "{status}");
    assert_eq!(status["host_state"], "done", "{status}");
    for key in [
        "window_index",
        "pane",
        "min_pane_cols",
        "min_pane_rows",
        "below_floor",
    ] {
        assert!(status[key].is_null(), "{key}: {status}");
    }
    fake.edit_block(&block, |block| {
        block["records"] = json!([{"state": "error", "updated_at": "2026-01-01T00:00:00Z"}])
    });
    let status = json_payload(
        &fake.cli(&["status", "--target", "a", "--format", "json"]),
        "status error",
    );
    assert_eq!(status["host_word"], "error", "{status}");
    assert_eq!(status["host_state"], "blocked", "{status}");
    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
    assert_eq!(rows[0]["host_state"], "blocked", "{rows}");
}

/// P6.2: the newest record is chosen at full timestamp precision: `working` at
/// .570900 is newer than `idle` at .570100, also when it is listed first.
#[test]
fn the_newest_record_is_chosen_below_one_millisecond() {
    let fake = FakeRex::start("rex-status-precision");
    prompt_block(
        &fake,
        json!({"records": [
            {"state": "working", "updated_at": "2026-01-01T00:00:00.570900Z"},
            {"state": "idle", "updated_at": "2026-01-01T00:00:00.570100Z"},
        ]}),
    );
    let status = json_payload(
        &fake.cli(&["status", "--target", "a", "--format", "json"]),
        "status",
    );
    assert_eq!(status["host_word"], "working", "{status}");
    assert_eq!(status["host_state"], "working", "{status}");
}

/// Acceptance (P6.3, P6.6): a block whose Host state before the send is not `working`
/// and that gets a `done` record after the send (and, in another case, a raw `error`)
/// gives `submit: confirmed`; the prompt goes as a `block.event` paste and then `rex
/// send-key enter`.
#[test]
fn a_record_after_the_send_confirms_the_submission() {
    for state in ["done", "error"] {
        let fake = FakeRex::start(&format!("rex-confirm-{state}"));
        let block = prompt_block(
            &fake,
            json!({"lines": ["$ ready"], "on_send": {"records": [state], "append": ["REPLY"]}}),
        );
        let result = prompt(&fake, "5", "hello");
        assert_eq!(result["submit"], "confirmed", "{state}: {result}");
        assert_eq!(result["one_submission_delivery"], "guaranteed", "{result}");
        assert_eq!(result["turn_observed"], true, "{result}");
        assert_eq!(result["complete"], true, "{result}");
        assert_eq!(result["output_since_prompt"], "hello\nREPLY", "{result}");
        let block = fake.block(&block).unwrap();
        assert_eq!(block["pasted"], json!(["hello"]), "{block}");
        assert_eq!(block["keys"], json!(["enter"]), "{block}");
        let log = fake.log();
        let paste = log
            .iter()
            .position(|call| command_words(call).starts_with(&["api".to_owned()]))
            .unwrap();
        let enter = log
            .iter()
            .position(|call| command_words(call) == ["send-key"])
            .unwrap();
        assert!(paste < enter, "{log:?}");
    }
}

/// Acceptance (P6.6, P6.8): with only an older `idle` record, the settle does not end
/// on it (it ends at the timeout) and `submit` is `accepted`; a block that is `working`
/// before the send is never confirmed.
#[test]
fn an_older_record_never_ends_the_settle_or_confirms() {
    let fake = FakeRex::start("rex-older");
    let block = prompt_block(
        &fake,
        json!({"records": [{"state": "idle", "updated_at": "2020-01-01T00:00:00Z"}]}),
    );
    let result = prompt(&fake, "1", "hello");
    assert_eq!(result["submit"], "accepted", "{result}");
    assert_eq!(result["reason"], "timed_out", "{result}");
    assert_eq!(result["turn_observed"], false, "{result}");

    fake.edit_block(&block, |block| {
        block["records"] = json!([{"state": "working", "updated_at": "2020-01-01T00:00:00Z"}]);
        block["on_send"] = json!({"records": ["done"]});
    });
    let result = prompt(&fake, "5", "again");
    assert_eq!(result["submit"], "accepted", "{result}");
}

/// P6.8: a terminal record published during the pre-send listing (after the caller's
/// mark, before the transmission) is not newer than the send, so it does not end the
/// settle and is no Turn observation.
#[test]
fn a_record_published_before_the_transmission_never_ends_the_settle() {
    let fake = FakeRex::start("rex-pre-send");
    let block = prompt_block(&fake, json!({}));
    fake.edit_server(INSTANCE, |server| {
        server["on_block_ls"] = json!({"block": block, "records": ["idle"]})
    });
    let result = prompt(&fake, "1", "hello");
    let records = fake.block(&block).unwrap()["records"].clone();
    assert_eq!(records.as_array().unwrap().len(), 1, "published: {records}");
    assert_eq!(result["reason"], "timed_out", "{result}");
    assert_eq!(result["turn_observed"], false, "{result}");
    assert_eq!(result["submit"], "accepted", "{result}");
}

/// P6.7: a record after the send is a Turn observation also when no record after the
/// send has state `working`.
#[test]
fn a_record_after_the_send_is_a_turn_observation_without_working() {
    let fake = FakeRex::start("rex-turn");
    prompt_block(&fake, json!({"on_send": {"records": ["idle"]}}));
    let result = prompt(&fake, "5", "hello");
    assert_eq!(result["turn_observed"], true, "{result}");
    assert_eq!(result["host_state"], "idle", "{result}");
    assert_eq!(result["reason"], "ready_matched", "{result}");
    assert_eq!(
        result["submit"], "accepted",
        "an idle record does not confirm: {result}"
    );
}

/// Acceptance (P6.5): with `api_revision` 11, `prompt` exits 1 with `rex api revision
/// 11 is older than 12` and sends no paste and no key; 12 works.
#[test]
fn an_old_api_revision_refuses_the_prompt_before_any_transmission() {
    let fake = FakeRex::start("rex-revision");
    prompt_block(&fake, json!({"on_send": {"records": ["done"]}}));
    fake.edit_server(INSTANCE, |server| server["api_revision"] = json!(11));
    let out = fake.cli(&["prompt", "--target", "a", "--timeout", "1", "hello"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("rex api revision 11 is older than 12"),
        "{}",
        out.stderr
    );
    assert!(fake.calls(&["api"]).is_empty(), "{:?}", fake.log());
    assert!(fake.calls(&["send-key"]).is_empty(), "{:?}", fake.log());
    assert!(fake.calls(&["send"]).is_empty(), "{:?}", fake.log());
    // Other verbs keep working.
    fake.cli(&["capture", "--target", "a"])
        .assert_success("capture on revision 11");

    fake.edit_server(INSTANCE, |server| server["api_revision"] = json!(12));
    let result = prompt(&fake, "5", "hello");
    assert_eq!(result["submit"], "confirmed", "{result}");
}

/// Acceptance (P6.4): a multi-line prompt to a block without mode 2004 on is refused
/// before any transmission; a one-line prompt is sent as unavailable delivery.
#[test]
fn a_multi_line_prompt_without_bracketed_paste_is_refused() {
    let fake = FakeRex::start("rex-multiline");
    prompt_block(
        &fake,
        json!({"modes": [], "on_send": {"records": ["done"]}}),
    );
    let out = fake.cli(&["prompt", "--target", "a", "--timeout", "1", "one\ntwo"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(fake.calls(&["api"]).is_empty(), "{:?}", fake.log());
    assert!(fake.calls(&["send-key"]).is_empty(), "{:?}", fake.log());
    let result = prompt(&fake, "5", "one");
    assert_eq!(result["one_submission_delivery"], "unavailable", "{result}");
}

/// A `prompt --format json` against a block with `lines`, `rows` and `columns`, whose
/// `on_send` adds a `done` record and `effect`.
fn p52_prompt(label: &str, fields: Value, effect: Value) -> Value {
    let fake = FakeRex::start(label);
    let mut fields = fields;
    let mut effect = effect;
    effect["records"] = json!(["done"]);
    fields["on_send"] = effect;
    prompt_block(&fake, fields);
    prompt(&fake, "5", "go")
}

/// Acceptance (P5.2): a full scrollback (Rex drops the oldest line for each new line)
/// between the mark and settle gives Completeness false and the last `rows` lines of
/// the capture after settle.
#[test]
fn a_full_scrollback_is_incomplete_and_gives_the_last_rows_lines() {
    let result = p52_prompt(
        "rex-p52-full",
        json!({"lines": ["l1", "l2", "l3", "l4"], "rows": 2}),
        json!({"append": ["R1", "R2", "R3"], "drop": 4}),
    );
    // After settle: go, R1, R2, R3 (the four oldest lines dropped).
    assert_eq!(result["complete"], false, "{result}");
    assert_eq!(result["output_since_prompt"], "R2\nR3", "{result}");
}

/// Acceptance (P5.2): a width change between the mark and settle gives Completeness
/// false and the last `rows` lines.
#[test]
fn a_width_change_is_incomplete_and_gives_the_last_rows_lines() {
    let result = p52_prompt(
        "rex-p52-width",
        json!({"lines": ["l1"], "rows": 2, "columns": 80}),
        json!({"append": ["R1", "R2"], "columns": 100}),
    );
    assert_eq!(result["complete"], false, "{result}");
    assert_eq!(result["output_since_prompt"], "R1\nR2", "{result}");
}

/// Acceptance (P5.2): the alternate screen at settle gives Completeness false and the
/// last `rows` lines.
#[test]
fn the_alternate_screen_is_incomplete_and_gives_the_last_rows_lines() {
    let result = p52_prompt(
        "rex-p52-alternate",
        json!({"lines": ["l1"], "rows": 2}),
        json!({"rewrite": ["S1", "S2", "S3"], "alternate": true}),
    );
    assert_eq!(result["complete"], false, "{result}");
    assert_eq!(result["output_since_prompt"], "S2\nS3", "{result}");
}

/// P5.2: `rows` is read after settle: a height change between the mark and settle
/// uses the new `rows` for the guard; the result starts at the first changed line.
#[test]
fn a_height_change_uses_the_rows_after_settle() {
    // Mark: 5 lines; the first changed line is 2. With the mark's `rows` 2 the guard
    // (2 >= 5 - 2) fails; with `rows` 3 after settle it holds.
    let result = p52_prompt(
        "rex-p52-height",
        json!({"lines": ["l1", "l2", "l3", "l4", "l5"], "rows": 2}),
        json!({"rewrite": ["l1", "l2", "X3", "X4", "X5"], "rows": 3}),
    );
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["output_since_prompt"], "X3\nX4\nX5", "{result}");
}

// --- P7: notify, self, list, kill, launch, claude-teams ----------------------------------

/// Acceptance (P7.1): `notify` exits 0 and reports it was not delivered on rex.
#[test]
fn notify_is_not_delivered_on_rex() {
    let fake = FakeRex::start("rex-notify");
    let out = fake.cli(&["notify", "--title", "t", "--body", "b"]);
    out.assert_success("notify");
    assert_eq!(out.stderr, "notify: not delivered on rex\n");
}

/// P7.4: `kill` applies the self-pane refusal and the Ownership check, runs `rex block
/// close <block>`, and deletes the record; a block already gone counts as closed.
#[test]
fn kill_closes_the_block_and_deletes_the_record() {
    let fake = FakeRex::start("rex-kill");
    fake.write_record(INSTANCE, "unset", "lead", Some(CALLER));
    let out = fake.cli(&["kill", "--target", "lead"]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(out.stderr.contains("calling pane"), "{}", out.stderr);
    let unowned = fake.add_block(INSTANCE, json!({}));
    let out = fake.cli(&["kill", "--target", &unowned]);
    assert_eq!(out.status, 1, "{out:?}");
    assert!(
        out.stderr.contains("not created by tmux-tools"),
        "{}",
        out.stderr
    );
    assert!(
        fake.calls(&["block", "close"]).is_empty(),
        "{:?}",
        fake.log()
    );

    let launched = json_payload(
        &fake.cli(&[
            "launch", "--name", "k", "--cmd", "sleep 30", "--format", "json",
        ]),
        "launch k",
    );
    let block = launched["pane_id"].as_str().unwrap().to_owned();
    fake.cli(&["kill", "--target", "k"])
        .assert_success("kill k");
    let closes = fake.calls(&["block", "close"]);
    assert_eq!(closes.len(), 1, "{closes:?}");
    assert!(has(&closes[0], &block), "{closes:?}");
    assert!(fake.block(&block).is_none());
    assert!(fake.record(INSTANCE, "k").is_none());

    // `not_found` is success: the record of a block closed meanwhile is deleted too.
    let gone = fake.add_block(INSTANCE, json!({}));
    fake.write_record(INSTANCE, "unset", "gone", Some(&gone));
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in *'block close'*) echo 'Error: not found' >&2; exit 3;; esac\nexec '{}' \"$@\"\n",
        fake.dir.join("bin/rex.real").display()
    );
    std::fs::rename(fake.dir.join("bin/rex"), fake.dir.join("bin/rex.real")).unwrap();
    write_executable(&fake.dir.join("bin/rex"), &script);
    fake.cli(&["kill", "--target", "gone"])
        .assert_success("kill a block rex no longer finds");
    assert!(fake.record(INSTANCE, "gone").is_none());
}

/// P7.5: `launch` runs `(<command>); exec $SHELL`; `--bare` runs the command with no
/// wrap.
#[test]
fn launch_keeps_the_shell_unless_bare() {
    let fake = FakeRex::start("rex-keep");
    fake.cli(&["launch", "--name", "keep", "--cmd", "make test"])
        .assert_success("launch");
    let run = &fake.creates()[0];
    assert_eq!(
        command_of(run),
        ["/bin/sh", "-c", "(make test); exec /bin/sh"],
        "{run:?}"
    );
    fake.clear_log();
    fake.cli(&["launch", "--bare", "--name", "bare", "--cmd", "make test"])
        .assert_success("launch --bare");
    assert_eq!(
        command_of(&fake.creates()[0]),
        ["/bin/sh", "-c", "make test"]
    );
}

/// A fake `claude`: it records its arguments and `TT_HOST`; with `FAKE_CLAUDE_SPLIT`
/// set it runs two `split-window` calls through the Shim on `PATH` (the first with the
/// Rex per-block variables and `K=V` as `-e` settings) and exits with that value; with
/// `FAKE_CLAUDE_ONE` set it runs one `split-window`, writes `TT_TEAM` and the output of
/// `$TT_BIN list --format json` beside the arguments, and exits 0; with
/// `FAKE_CLAUDE_RESPAWN` set it runs one `split-window`, then `respawn-pane -k -t %2`,
/// and exits 3; it exits 9 without `TT_TEAM`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "2.1.319 (Claude Code)"; exit 0; fi
printf '%s\n' "$@" > "$FAKE_CLAUDE_ARGS"
printf '%s\n' "${TT_HOST-}" > "$FAKE_CLAUDE_ARGS.host"
if [ -n "${FAKE_CLAUDE_ONE-}" ]; then
  [ -n "${TT_TEAM-}" ] || exit 9
  printf '%s' "$TT_TEAM" > "$FAKE_CLAUDE_ARGS.team"
  tmux split-window -d -P -F '#{pane_id}' -- cat > "$FAKE_CLAUDE_ARGS.splits" 2>&1 || exit 7
  "$TT_BIN" list --format json > "$FAKE_CLAUDE_ARGS.list" || exit 8
  exit 0
fi
if [ -n "${FAKE_CLAUDE_RESPAWN-}" ]; then
  [ -n "${TT_TEAM-}" ] || exit 9
  tmux split-window -d -P -F '#{pane_id}' -- cat > "$FAKE_CLAUDE_ARGS.splits" 2>&1 || exit 7
  tmux respawn-pane -k -t %2 -- cat >> "$FAKE_CLAUDE_ARGS.splits" 2>&1 || exit 8
  exit 3
fi
if [ -n "${FAKE_CLAUDE_SPLIT-}" ]; then
  # Outside a Team, `tmux` is not this Team's Shim: never call it.
  [ -n "${TT_TEAM-}" ] || exit 9
  tmux split-window -d -P -F '#{pane_id}' -e REX_BLOCK=x -e REX_SESSION=y \
    -e REX_SERVER=z -e K=V -- printenv >> "$FAKE_CLAUDE_ARGS.splits" 2>&1 || exit 7
  tmux split-window -d -P -F '#{pane_id}' -- cat >> "$FAKE_CLAUDE_ARGS.splits" 2>&1 || exit 7
  exit "$FAKE_CLAUDE_SPLIT"
fi
exit 0
"#;

/// Acceptance (PRD-261115-0921-01 P2): `claude-teams` in a plain (unregistered) Rex
/// block sets up a Team: no `claude-teams: no team` line, Claude gets `--teammate-mode
/// tmux` and `TT_HOST=rex`, the one teammate it starts shows in `list --format json` as
/// `team-<team id>.tm1` with `team` and `parent: null` (PRD-261108-0447-01 P2.8, P6.1),
/// and the clean exit closes it and removes the team directory; with `REX_BLOCK`
/// unset (`TMUX_TOOLS_HOST=rex`) it fails open with `REX_BLOCK is unset` and
/// in-process teammates, and creates no team directory.
#[test]
fn claude_teams_sets_up_a_team_on_rex_and_fails_open_without_rex_block() {
    let fake = FakeRex::start("rex-claude");
    write_executable(&fake.dir.join("bin/claude"), FAKE_CLAUDE);
    let args_file = fake.dir.join("claude-args");
    let args_file_str = args_file.display().to_string();
    let teams = fake.dir.join("state/tmux-tools/teams");
    let no_team_dir = || !teams.exists() || std::fs::read_dir(&teams).unwrap().next().is_none();

    let unset = fake.cli_with(
        &["claude-teams"],
        &[
            ("FAKE_CLAUDE_ARGS", args_file_str.as_str()),
            ("TMUX_TOOLS_HOST", "rex"),
        ],
        &["REX_BLOCK"],
    );
    unset.assert_success("claude-teams on rex without REX_BLOCK");
    assert_eq!(
        unset.stderr,
        "claude-teams: no team: REX_BLOCK is unset; starting claude without teammate panes\n"
    );
    assert_eq!(
        std::fs::read_to_string(&args_file).unwrap(),
        "--teammate-mode\nin-process\n"
    );
    assert!(no_team_dir(), "no team directory");

    let blocks_before = fake.block_count();
    let out = fake.cli_with(
        &["claude-teams"],
        &[
            ("FAKE_CLAUDE_ARGS", args_file_str.as_str()),
            ("FAKE_CLAUDE_ONE", "1"),
            ("TT_BIN", BIN),
        ],
        &[],
    );
    out.assert_success("claude-teams on rex");
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
        "rex\n"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{args_file_str}.splits")).unwrap(),
        "%2\n"
    );
    let team = std::fs::read_to_string(format!("{args_file_str}.team")).unwrap();
    assert_eq!(team.len(), 8, "{team:?}");
    let rows: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{args_file_str}.list")).unwrap())
            .unwrap();
    let name = format!("team-{team}.tm1");
    let teammate = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name.as_str())
        .unwrap_or_else(|| panic!("{name}: {rows:#}"));
    assert_eq!(teammate["team"], team.as_str(), "{teammate}");
    assert_eq!(teammate["parent"], Value::Null, "{teammate}");
    assert_eq!(teammate["shim_id"], "%2", "{teammate}");
    assert!(no_team_dir(), "the clean exit removed the team directory");
    assert_eq!(
        fake.block_count(),
        blocks_before,
        "the clean exit closed the teammate"
    );
}

/// Acceptance (PRD-261115-0921-01 P3.1, P3.3, P4.1, PRD-261108-0447-01 P8): with the
/// calling block registered as `lead`, Claude's two `split-window` calls create
/// `lead.tm1` by `rex split --split=right -b <leader>` and `lead.tm2` by `rex split
/// --split=below -b <lead.tm1>`; the first teammate command keeps `K=V` and none of
/// the `-e` settings of `REX_BLOCK`, `REX_SESSION`, `REX_SERVER`. After a failed exit
/// `list --format json` shows both with `team`, `parent: "lead"`, and `shim_id`, as
/// Orphans, and `kill` closes them.
#[test]
fn on_rex_teammates_split_right_of_the_leader_then_below_and_drop_the_rex_variables() {
    let fake = FakeRex::start("rex-teammates");
    fake.write_record(INSTANCE, "unset", "lead", Some(CALLER));
    write_executable(&fake.dir.join("bin/claude"), FAKE_CLAUDE);
    let args_file = fake.dir.join("claude-args");
    let args_file_str = args_file.display().to_string();
    let out = fake.cli_with(
        &["claude-teams"],
        &[
            ("FAKE_CLAUDE_ARGS", args_file_str.as_str()),
            ("FAKE_CLAUDE_SPLIT", "3"),
        ],
        &[],
    );
    assert_eq!(out.status, 3, "{out:?}");
    assert_eq!(
        std::fs::read_to_string(format!("{args_file_str}.splits")).unwrap(),
        "%2\n%3\n"
    );

    let splits = fake.calls(&["split"]);
    assert_eq!(splits.len(), 2, "{splits:?}");
    assert!(has(&splits[0], "--split=right"), "{:?}", splits[0]);
    assert_eq!(flag_value(&splits[0], "-b"), Some(CALLER));
    assert_eq!(flag_value(&splits[0], "--label"), Some("lead.tm1"));
    let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
    let row = |name: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("{name}: {rows}"))
            .clone()
    };
    let tm1 = row("lead.tm1");
    let tm2 = row("lead.tm2");
    assert!(has(&splits[1], "--split=below"), "{:?}", splits[1]);
    assert_eq!(flag_value(&splits[1], "-b"), tm1["pane"].as_str());
    assert_eq!(flag_value(&splits[1], "--label"), Some("lead.tm2"));

    let command = command_of(&splits[0]).join(" ");
    assert!(command.contains("'K=V'"), "{command}");
    for dropped in ["REX_BLOCK=", "REX_SESSION=", "REX_SERVER="] {
        assert!(!command.contains(dropped), "{dropped}: {command}");
    }

    let team = tm1["team"].as_str().expect("team").to_owned();
    assert_eq!(team.len(), 8, "{tm1}");
    for (teammate, shim_id) in [(&tm1, "%2"), (&tm2, "%3")] {
        assert_eq!(teammate["team"], team.as_str(), "{teammate}");
        assert_eq!(teammate["parent"], "lead", "{teammate}");
        assert_eq!(teammate["shim_id"], shim_id, "{teammate}");
        assert_eq!(teammate["orphan"], true, "{teammate}");
    }
    for name in ["lead.tm1", "lead.tm2"] {
        fake.cli(&["kill", "--target", name])
            .assert_success(&format!("kill {name}"));
    }
    assert_eq!(fake.block_count(), 1, "only the Leader block is left");
}

/// PRD-261112-2108-01 P3.4: a teammate respawn on Rex creates the new block under a
/// transitional name and then moves its record to `lead.tm1`; the block label follows
/// by `rex block rename <block> -- lead.tm1`. The label is for display only (P3 item
/// 2): a block gone before the rename, or a failed rename (one warning line), still
/// gives a successful respawn.
#[test]
fn a_rex_teammate_respawn_renames_the_block_label_to_the_pane_name() {
    for mode in ["ok", "gone", "fail"] {
        let fake = FakeRex::start("rex-respawn");
        fake.write_record(INSTANCE, "unset", "lead", Some(CALLER));
        match mode {
            "gone" => fake.edit_server(INSTANCE, |server| server["rename_gone"] = json!(true)),
            "fail" => fake.edit_server(INSTANCE, |server| server["rename_fail"] = json!(true)),
            _ => {}
        }
        write_executable(&fake.dir.join("bin/claude"), FAKE_CLAUDE);
        let args_file = fake.dir.join("claude-args");
        let args_file_str = args_file.display().to_string();
        let out = fake.cli_with(
            &["claude-teams"],
            &[
                ("FAKE_CLAUDE_ARGS", args_file_str.as_str()),
                ("FAKE_CLAUDE_RESPAWN", "1"),
            ],
            &[],
        );
        assert_eq!(out.status, 3, "{mode}: {out:?}");
        let splits = std::fs::read_to_string(format!("{args_file_str}.splits")).unwrap();
        let warning = splits
            .strip_prefix("%2\n")
            .unwrap_or_else(|| panic!("{mode}: the respawn succeeded: {splits:?}"));
        let renames = fake.calls(&["block", "rename"]);
        let tm1_name = "lead.tm1";
        assert_eq!(renames.len(), 1, "{mode}: {:?}", fake.log());
        assert_eq!(
            renames[0][renames[0].len() - 2..],
            ["--", tm1_name],
            "{mode}: the label follows `--`"
        );
        if mode == "fail" {
            assert_eq!(warning.lines().count(), 1, "{mode}: {warning:?}");
            assert!(
                warning.starts_with("rex: block label not updated: block:"),
                "{mode}: {warning:?}"
            );
        } else {
            assert_eq!(warning, "", "{mode}: no warning");
        }
        let rows = json_payload(&fake.cli(&["list", "--format", "json"]), "list");
        let names: Vec<&str> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["name"].as_str())
            .collect();
        assert!(names.contains(&tm1_name), "{mode}: {rows}");
        assert!(
            !names.iter().any(|name| name.contains(".respawn-")),
            "{mode}: {rows}"
        );
        if mode == "gone" {
            continue;
        }
        let tm1 = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == tm1_name)
            .unwrap()
            .clone();
        let block = fake
            .block(tm1["pane"].as_str().expect("the block of the teammate"))
            .expect("the respawned block exists");
        let label = block["label"].as_str().unwrap();
        match mode {
            "fail" => assert!(
                label.starts_with("lead.tm1.respawn-"),
                "{mode}: the transitional label stays: {block}"
            ),
            _ => assert_eq!(label, "lead.tm1", "{mode}: {block}"),
        }
        fake.cli(&["kill", "--target", tm1_name])
            .assert_success(&format!("kill {tm1_name}"));
    }
}

// --- P8.3: the private-server run, opt-in ------------------------------------------------

/// The opt-in switch of the private-server run.
const SWITCH: &str = "TT_TEST_REX";

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

/// A `rex` in front of the real one whose creates end as a timeout after the block is
/// made, so the spawn is ambiguous (P4.4) on every run.
const TIMEOUT_REX: &str = r#"#!/bin/sh
for arg in "$@"; do
  case "$arg" in
    run|split)
      "$TT_REAL_REX" "$@" >/dev/null 2>&1
      echo "Error: session.new_window request timed out after 10s; increase --timeout" >&2
      exit 1;;
    --*|-S|unix://*) ;;
    *) break;;
  esac
done
exec "$TT_REAL_REX" "$@"
"#;

/// The calling Claude session's variables (`CLAUDECODE`, `CLAUDE_*`), kept from the
/// private server and its blocks.
fn claude_session_vars() -> Vec<String> {
    original_environment()
        .keys()
        .filter(|key| *key == "CLAUDECODE" || key.starts_with("CLAUDE_"))
        .cloned()
        .collect()
}

/// The real `rex` of the original `PATH`.
fn real_rex() -> PathBuf {
    original_environment()
        .get("PATH")
        .map(String::as_str)
        .unwrap_or_default()
        .split(':')
        .map(|entry| Path::new(entry).join("rex"))
        .find(|candidate| candidate.is_file())
        .expect("TT_TEST_REX=1 needs `rex` on PATH")
}

/// The operator's `tmux-tools/rex/` registry: every file under it with its bytes.
fn operator_registry() -> Vec<(PathBuf, Vec<u8>)> {
    let original = original_environment();
    let state = match original
        .get("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
    {
        Some(state) => PathBuf::from(state),
        None => Path::new(original.get("HOME").expect("HOME")).join(".local/state"),
    };
    let mut files = Vec::new();
    let mut stack = vec![state.join("tmux-tools/rex")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.push((path.clone(), Vec::new()));
                stack.push(path);
            } else {
                files.push((path.clone(), std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    files.sort();
    files
}

/// The default server's `rex ls --json`, read only (status, stdout).
fn default_server_listing(rex: &Path) -> (Option<i32>, String) {
    let out = Command::new(rex)
        .args(["--autostart=false", "ls", "--json"])
        .env_remove("REX_SERVER")
        .stdin(Stdio::null())
        .output()
        .expect("run rex ls");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// The private-server Leader's fake `claude` (PRD-261115-0921-01 P7.2). `--version`
/// prints `FAKE_CLAUDE_VERSION` (default `2.1.322`). As a teammate (`--agent-id` or
/// `--agent-name`) it sleeps. As the Leader it waits for the file `<report>.start`, then
/// by `FAKE_CLAUDE_MODE`:
/// - unset: replays the trace fixture `FAKE_CLAUDE_FIXTURE` through the Shim without
///   its `kill-pane` (so `lead.tm1` stays), runs one more `split-window` with the Rex
///   per-block variables and `K=V` as `-e` settings whose teammate writes its
///   environment to `<report>.tm2-env`, writes the result (with the output of `TT_BIN
///   self --format json`) to `<report>.paused`, waits for `<report>.go`, and exits 0;
/// - `full`: replays every line of `FAKE_CLAUDE_FIXTURE`, its `kill-pane` included,
///   writes the result to `<report>.done`, and exits 0;
/// - `fail`: runs one `split-window` whose teammate sleeps, writes the result to
///   `<report>.done`, and exits 3.
///
/// `{{python}}` is the absolute `python3`.
const REPLAY_CLAUDE: &str = r##"#!{{python}}
import json, os, shlex, subprocess, sys, time
args = sys.argv[1:]
if args[:1] == ["--version"]:
    print(os.environ.get("FAKE_CLAUDE_VERSION", "2.1.322") + " (Claude Code)")
    sys.exit(0)
if "--agent-id" in args or "--agent-name" in args:
    time.sleep(3600)
    sys.exit(0)
report = os.environ["FAKE_CLAUDE_REPORT"]
def wait(path):
    deadline = time.time() + 120
    while not os.path.exists(path) and time.time() < deadline:
        time.sleep(0.05)
wait(report + ".start")
def publish(path, obj):
    # Write and close a temporary file, then rename it: existence means complete.
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(obj, f)
    os.replace(tmp, path)
def tmux(argv):
    p = subprocess.run(["tmux"] + argv, capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr
result = {"args": args, "mismatches": []}
own = subprocess.run([os.environ["TT_BIN"], "self", "--format", "json"],
                     capture_output=True, text=True)
result["self"] = own.stdout + own.stderr
team, cwd, claude = os.environ["TT_TEAM"], os.getcwd(), os.path.abspath(sys.argv[0])
mode = os.environ.get("FAKE_CLAUDE_MODE", "")
result["team"] = team
if mode == "fail":
    code, out, err = tmux(["split-window", "-d", "-t", "%1", "-P", "-F", "#{pane_id}",
                           "--", "sleep 3600"])
    result["split"] = {"exit": code, "stdout": out, "stderr": err}
    publish(report + ".done", result)
    sys.exit(3)
def fill(s):
    return s.replace("{{team_id}}", team).replace("{{cwd}}", cwd).replace("{{claude}}", claude)
result["calls"] = 0
for line in open(os.environ["FAKE_CLAUDE_FIXTURE"]):
    if not line.strip():
        continue
    entry = json.loads(line)
    argv = [fill(a) for a in entry["argv"]]
    if "kill-pane" in argv and mode != "full":
        continue
    result["calls"] += 1
    code, out, err = tmux(argv)
    expected = fill(entry["stdout"])
    if code != entry["exit"] or out != expected:
        result["mismatches"].append({"argv": argv, "exit": code, "stdout": out,
                                     "stderr": err, "expected_exit": entry["exit"],
                                     "expected_stdout": expected})
if mode == "full":
    publish(report + ".done", result)
    sys.exit(0)
env_file = shlex.quote(report + ".tm2-env")
code, out, err = tmux(["split-window", "-d", "-t", "%1", "-P", "-F", "#{pane_id}",
                       "-e", "REX_BLOCK=x", "-e", "REX_SESSION=y", "-e", "REX_SERVER=z",
                       "-e", "K=V", "--",
                       "env > %s.tmp && mv %s.tmp %s; exec sleep 3600"
                       % (env_file, env_file, env_file)])
result["split2"] = {"exit": code, "stdout": out, "stderr": err}
publish(report + ".paused", result)
wait(report + ".go")
sys.exit(0)
"##;

/// Every trace fixture of `tests/fixtures/claude-teams/`, oldest version first
/// (PRD-261108-0447-01 P10.2).
fn all_fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-teams");
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    fixtures.sort_by_key(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| {
                stem.split('.')
                    .map(|part| part.parse::<u64>().unwrap_or(0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    assert!(!fixtures.is_empty(), "a trace fixture");
    fixtures
}

/// The newest trace fixture of `tests/fixtures/claude-teams/`.
fn newest_fixture() -> PathBuf {
    all_fixtures().pop().expect("a trace fixture")
}

/// The first `program` on the suite's `PATH`.
fn on_path(program: &str) -> PathBuf {
    std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|entry| Path::new(entry).join(program))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{program} on PATH"))
}

/// The rect `[x, y, width, height]` of `block` in a `rex session inspect --json` layout.
fn block_rect(layout: &Value, block: &str) -> Option<[f64; 4]> {
    if layout["block_id"] == block {
        let rect = layout["rect"].as_array()?;
        return Some([
            rect[0].as_f64()?,
            rect[1].as_f64()?,
            rect[2].as_f64()?,
            rect[3].as_f64()?,
        ]);
    }
    let split = layout.get("split")?;
    block_rect(&split["before"], block).or_else(|| block_rect(&split["after"], block))
}

/// A POSIX single-quoted word.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// One private-server run: the real `rex`, a private server listening on
/// `unix://<dir>/s.sock` with its state under `<dir>/state`, and the run's dirs. `Drop`
/// closes every block of that server, stops it, and deletes `<dir>`, also on failure.
struct PrivateServer {
    rex: PathBuf,
    dir: PathBuf,
    address: String,
    server: Option<std::process::Child>,
    session: String,
    caller: String,
}

impl PrivateServer {
    fn start() -> PrivateServer {
        let rex = real_rex();
        // A Unix socket path holds about 100 bytes: a short directory under /tmp.
        let dir = PathBuf::from(format!("/tmp/ttrex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["state", "xdg", "config/tmux-tools", "work", "bin", "shim"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/agent.py"), AGENT);
        write_executable(&dir.join("bin/hexecho.py"), HEXECHO);
        write_executable(&dir.join("shim/rex"), TIMEOUT_REX);
        std::fs::write(
            dir.join("config/tmux-tools/agents.toml"),
            format!(
                "[ttagent]\nbinary = \"{}\"\n\n[ttagent.access.default]\nargs = []\n",
                dir.join("bin/agent.py").display()
            ),
        )
        .unwrap();
        let address = format!("unix://{}/s.sock", dir.display());
        let mut command = Command::new(&rex);
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        let server = command
            .args(["server", "run", "--listen", &address, "--state-dir"])
            .arg(dir.join("state"))
            .arg("--restore=false")
            .current_dir(&dir)
            .env_remove("REX_SERVER")
            .env_remove("REX_BLOCK")
            .env_remove("REX_SESSION")
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            // Every tmux-tools invocation inside a block gets the run's registry (P8.5).
            .env("XDG_STATE_HOME", dir.join("xdg"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the private rex server");
        let mut private = PrivateServer {
            rex,
            dir,
            address,
            server: Some(server),
            session: String::new(),
            caller: String::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while !private
            .rex(&["server", "status", "--json"])
            .status
            .success()
        {
            assert!(
                Instant::now() < deadline,
                "the private server never came up"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let created = private.rex_json(&["new", "tt-tests", "--json"]);
        private.session = created["session_id"].as_str().unwrap().to_owned();
        private.caller = created["initial_windows"][0]["block_ids"][0]
            .as_str()
            .unwrap()
            .to_owned();
        private
    }

    /// The real `rex`, always against the private server.
    fn rex(&self, args: &[&str]) -> std::process::Output {
        assert!(self.address.starts_with("unix:///tmp/ttrex-"));
        Command::new(&self.rex)
            .args(["--autostart=false", "-S", &self.address])
            .args(args)
            .env_remove("REX_SERVER")
            .stdin(Stdio::null())
            .output()
            .expect("run rex")
    }

    fn rex_json(&self, args: &[&str]) -> Value {
        let out = self.rex(args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("rex {args:?}: {out:?}"))
    }

    fn blocks(&self) -> Vec<Value> {
        self.rex_json(&["block", "ls", "-s", &self.session, "--json"])["blocks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn cli(&self, args: &[&str]) -> CmdOutput {
        self.cli_with(args, &[])
    }

    /// The CLI against the private server, from the calling block; `prefix` comes first
    /// on `PATH`.
    fn cli_with(&self, args: &[&str], env: &[(&str, &str)]) -> CmdOutput {
        let path = format!(
            "{}:{}",
            self.rex.parent().unwrap().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(BIN);
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        command
            .args(args)
            .current_dir(self.dir.join("work"))
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("PATH", path)
            .env("REX_SERVER", &self.address)
            .env("REX_BLOCK", &self.caller)
            .env("REX_SESSION", &self.session)
            .env("SHELL", "/bin/sh")
            .env("XDG_STATE_HOME", self.dir.join("xdg"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"));
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
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

    /// Wait until `block`'s capture contains `text`.
    fn wait_capture(&self, block: &str, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let capture =
                String::from_utf8_lossy(&self.rex(&["capture", "-b", block]).stdout).into_owned();
            if capture.contains(text) {
                return capture;
            }
            assert!(
                Instant::now() < deadline,
                "{block} never showed {text:?}: {capture}"
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

    /// The real `rex` against the private server, for teardown: never panics, and
    /// gives `None` when it cannot run or does not exit within `timeout` (it is then
    /// killed).
    fn teardown_rex(&self, args: &[&str], timeout: Duration) -> Option<std::process::Output> {
        if !self.address.starts_with("unix:///tmp/ttrex-") {
            return None;
        }
        let mut child = Command::new(&self.rex)
            .args(["--autostart=false", "-S", &self.address])
            .args(args)
            .env_remove("REX_SERVER")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stdout, &mut buffer);
            buffer
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let stdout = reader.join().unwrap_or_default();
        status.map(|status| std::process::Output {
            status,
            stdout,
            stderr: Vec::new(),
        })
    }

    /// Close every block, stop the server, and delete `<dir>`. Best effort and
    /// never panics: a failed or unparseable listing or close is skipped, the server
    /// child is always killed and reaped, and `<dir>` is always deleted.
    fn stop(&mut self) {
        let call = Duration::from_secs(10);
        if self.server.is_some() && !self.session.is_empty() {
            let blocks = self
                .teardown_rex(&["block", "ls", "-s", &self.session, "--json"], call)
                .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok())
                .and_then(|listing| listing["blocks"].as_array().cloned())
                .unwrap_or_default();
            for block in blocks {
                if let Some(id) = block["block_id"].as_str() {
                    let _ = self.teardown_rex(&["block", "close", id], call);
                }
            }
        }
        if let Some(mut server) = self.server.take() {
            let _ = self.teardown_rex(&["server", "stop"], call);
            let deadline = Instant::now() + Duration::from_secs(10);
            while server.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Drop for PrivateServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// P8.3: every required verb against a private server. Opt-in with `TT_TEST_REX=1`.
#[test]
fn rex_private_server_contract_suite() {
    if original_environment().get(SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let rex = real_rex();
    let default_before = default_server_listing(&rex);
    let registry_before = operator_registry();
    let mut server = PrivateServer::start();

    // self, status (P2, P7.2).
    let report = json_payload(&server.cli(&["self", "--format", "json"]), "self");
    assert_eq!(report["host"], "rex", "{report}");
    assert_eq!(report["driven"], false, "{report}");
    let status = json_payload(&server.cli(&["status", "--format", "json"]), "status");
    assert_eq!(status["pane_id"], server.caller.as_str(), "{status}");
    assert_eq!(status["session"], server.session.as_str(), "{status}");

    // launch with --split v and without (P3).
    let split = server.pane_of(
        &server.cli(&[
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
    let tab = server.pane_of(
        &server.cli(&[
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
    let blocks = server.blocks();
    let label = |id: &str| {
        blocks
            .iter()
            .find(|block| block["block_id"] == id)
            .map(|block| block["label"].clone())
    };
    assert_eq!(label(&split), Some(json!("tt.v")), "{blocks:?}");
    assert_eq!(label(&tab), Some(json!("tt.tab")), "{blocks:?}");
    let window = |id: &str| {
        blocks
            .iter()
            .find(|block| block["block_id"] == id)
            .map(|block| block["window_id"].clone())
    };
    assert_eq!(
        window(&split),
        window(&server.caller),
        "a split joins the caller's window"
    );
    assert_ne!(window(&tab), window(&server.caller), "a new window");

    // A duplicate live name (P4) and the exit-3 options (P3.5).
    let dup = server.cli(&["launch", "--name", "tt.v", "--cmd", "true"]);
    assert_eq!(dup.status, 1, "{dup:?}");
    assert_eq!(server.cli(&["rebalance"]).status, 3);
    assert_eq!(
        server
            .cli(&["launch", "--split", "tile", "--cmd", "true"])
            .status,
        3
    );

    // list (P7.3).
    let rows = json_payload(&server.cli(&["list", "--format", "json"]), "list");
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

    // send, send-enter, escape, interrupt against a raw echo (P5.3–P5.5).
    let hexecho = server.dir.join("bin/hexecho.py").display().to_string();
    let echo = server.pane_of(
        &server.cli(&[
            "launch", "--bare", "--name", "tt.echo", "--cmd", &hexecho, "--format", "json",
        ]),
        "launch hexecho",
    );
    server.wait_capture(&echo, "HEXECHO READY");
    server
        .cli(&["send", "--target", "tt.echo", "Hi"])
        .assert_success("send literal");
    server.wait_capture(&echo, "HEX 4869");
    server
        .cli(&["send", "--target", "tt.echo", "Up"])
        .assert_success("send key");
    server.wait_capture(&echo, "HEX 1b5b41");
    server
        .cli(&["send-enter", "--target", "tt.echo"])
        .assert_success("send-enter");
    server.wait_capture(&echo, "HEX 0d");
    server
        .cli(&["escape", "--target", "tt.echo"])
        .assert_success("escape");
    server.wait_capture(&echo, "HEX 1b\n");
    server
        .cli(&["interrupt", "--target", "tt.echo"])
        .assert_success("interrupt");
    server.wait_capture(&echo, "HEX 03");

    // execute and capture in a shell (P5.1).
    server
        .cli(&[
            "launch", "--name", "tt.sh", "--cmd", "true", "--format", "json",
        ])
        .assert_success("launch shell");
    std::thread::sleep(Duration::from_millis(500));
    let executed = json_payload(
        &server.cli(&[
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
    let captured = server.cli(&["capture", "--target", "tt.sh", "--all"]);
    captured.assert_success("capture --all");
    assert!(captured.stdout.contains("tt-marker-ok"), "{captured:?}");
    server
        .cli(&["capture", "--target", "tt.sh", "--lines", "2"])
        .assert_success("capture --lines");

    // spawn-agent, prompt, wait-idle with Host state (P6).
    let agent = server.pane_of(
        &server.cli(&[
            "spawn-agent",
            "ttagent",
            "--name",
            "tt.agent",
            "--format",
            "json",
        ]),
        "spawn-agent",
    );
    server.wait_capture(&agent, "AGENT READY");
    let result = json_payload(
        &server.cli(&[
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
        &server.cli(&[
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
        &server.cli(&["status", "--target", "tt.agent", "--format", "json"]),
        "status agent",
    );
    assert_eq!(status["host_word"], "done", "{status}");

    // notify (P7.1).
    let notified = server.cli(&["notify", "--title", "t", "--body", "b"]);
    notified.assert_success("notify");
    assert!(
        notified.stderr.contains("notify: not delivered on rex"),
        "{}",
        notified.stderr
    );

    // P8.3: a create that times out does not exit 4, so P4.4 treats it as ambiguous.
    let mut timed_out = false;
    for timeout in ["2ms", "1ms", "500us", "200us", "100us"] {
        let out = server.rex(&[
            "--timeout",
            timeout,
            "run",
            "-s",
            &server.session,
            "--focus=false",
            "--label",
            "tt.direct-timeout",
            "--json",
            "--",
            "/bin/sh",
            "-c",
            "exec sleep 600",
        ]);
        assert_ne!(out.status.code(), Some(4), "{out:?}");
        if String::from_utf8_lossy(&out.stderr).contains("timed out") {
            timed_out = true;
        }
    }
    assert!(timed_out, "no create timed out");
    for block in server.blocks() {
        if block["label"] == "tt.direct-timeout" {
            let _ = server.rex(&["block", "close", block["block_id"].as_str().unwrap()]);
        }
    }
    // An ambiguous create (a timeout after the block is made) is recovered by its label.
    let shim_path = format!(
        "{}:{}",
        server.dir.join("shim").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let real = server.rex.display().to_string();
    let adopted = server.pane_of(
        &server.cli_with(
            &[
                "launch",
                "--name",
                "tt.ambiguous",
                "--cmd",
                "sleep 600",
                "--format",
                "json",
            ],
            &[("PATH", &shim_path), ("TT_REAL_REX", &real)],
        ),
        "an ambiguous launch",
    );
    let labelled: Vec<Value> = server
        .blocks()
        .into_iter()
        .filter(|block| block["label"] == "tt.ambiguous")
        .collect();
    assert_eq!(labelled.len(), 1, "{labelled:?}");
    assert_eq!(labelled[0]["block_id"], adopted.as_str());

    // PRD-261115-0921-01 P2–P4, P7.2: `claude-teams` in a plain Rex block registered as
    // `lead` sets up a Team; the replayed fixture and one more `split-window` give
    // `lead.tm1` right of the Leader and `lead.tm2` below it, each a Rex block.
    claude_teams_on_the_private_server(&server);

    // PRD-261108-0447-01 P10.2 on the private server: every trace fixture, in full, in
    // its own Team; then the P8.3 Orphan of a failed Launcher and its `kill`.
    every_fixture_replays_on_the_private_server(&server);
    a_failed_leader_leaves_an_orphan_on_the_private_server(&server);

    // kill every created block (P7.4), then nothing created is left (P8.6).
    for name in [
        "lead",
        "tt.v",
        "tt.tab",
        "tt.echo",
        "tt.sh",
        "tt.agent",
        "tt.ambiguous",
    ] {
        server
            .cli(&["kill", "--target", name, "--any"])
            .assert_success(&format!("kill {name}"));
    }
    let rows = json_payload(&server.cli(&["list", "--format", "json"]), "final list");
    assert!(rows.as_array().is_none_or(Vec::is_empty), "{rows}");
    let left: Vec<String> = server
        .blocks()
        .iter()
        .filter_map(|block| block["block_id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        left,
        vec![server.caller.clone()],
        "only the caller block is left"
    );

    // Verification item 2: the private server is stopped and its directory is gone; the
    // default server and the operator's registry are unchanged.
    let address = server.address.clone();
    let dir = server.dir.clone();
    server.stop();
    assert!(!dir.exists(), "{} is gone", dir.display());
    let after = Command::new(&rex)
        .args([
            "--autostart=false",
            "-S",
            &address,
            "server",
            "status",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!after.status.success(), "the private server is stopped");
    assert_eq!(default_server_listing(&rex), default_before);
    assert_eq!(operator_registry(), registry_before);
}

/// PRD-261115-0921-01 P2, P3.1, P3.3, P4, P7.2 and the PRD-261108-0447-01 P8 output on
/// the private server: a Leader block launched as `lead` runs `tmux-tools claude-teams`
/// with [`REPLAY_CLAUDE`]. Its stderr has no `claude-teams: no team` line; `list
/// --format json` shows `lead.tm1` and `lead.tm2` with `team`, `parent: "lead"`, and
/// `shim_id`; `rex session inspect --json` places `lead.tm1` right of the Leader and
/// `lead.tm2` below `lead.tm1`; `lead.tm2` has `K=V`, its own `REX_BLOCK`, and none of
/// the dropped `-e` values or the team variables. The clean exit closes both teammates
/// and removes the team directory. The Leader block stays for the caller's `kill`.
/// Launch a Leader block named `name` that runs `tmux-tools claude-teams` with
/// [`REPLAY_CLAUDE`], report `<dir>/<name>-report`, stderr in `<dir>/<name>.stderr`,
/// and `env` as extra `K=V` settings. Gives the Leader block, the report path, and the
/// stderr path.
fn launch_leader(
    server: &PrivateServer,
    name: &str,
    env: &[(&str, &str)],
) -> (String, String, PathBuf) {
    let python = on_path("python3");
    write_executable(
        &server.dir.join("bin/claude"),
        &REPLAY_CLAUDE.replace("{{python}}", &python.display().to_string()),
    );
    let report_str = server
        .dir
        .join(format!("{name}-report"))
        .display()
        .to_string();
    let stderr_file = server.dir.join(format!("{name}.stderr"));
    let path = format!(
        "{}:{}:{}",
        server.dir.join("bin").display(),
        server.rex.parent().unwrap().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let extra: String = env
        .iter()
        .map(|(key, value)| format!("{key}={} ", sh_quote(value)))
        .collect();
    // A block has no `REX_SERVER` of a private server: the Leader gets it, and the
    // run's registry and `PATH`, explicitly.
    let leader_command = format!(
        "env -u TT_TEAM -u TT_HOST -u TMUX -u TMUX_PANE REX_SERVER={} PATH={} \
         XDG_STATE_HOME={} TT_BIN={} FAKE_CLAUDE_REPORT={} {extra}{} \
         claude-teams 2> {}",
        sh_quote(&server.address),
        sh_quote(&path),
        sh_quote(&server.dir.join("xdg").display().to_string()),
        sh_quote(BIN),
        sh_quote(&report_str),
        sh_quote(BIN),
        sh_quote(&stderr_file.display().to_string()),
    );
    let leader = server.pane_of(
        &server.cli(&[
            "launch",
            "--name",
            name,
            "--cmd",
            &leader_command,
            "--format",
            "json",
        ]),
        &format!("launch the Leader {name}"),
    );
    (leader, report_str, stderr_file)
}

/// Wait until the Leader's `<report>.done` exists and give its JSON.
fn wait_done(report: &str, stderr_file: &Path) -> Value {
    let done = PathBuf::from(format!("{report}.done"));
    let deadline = Instant::now() + Duration::from_secs(90);
    while !done.exists() {
        assert!(
            Instant::now() < deadline,
            "the Leader never finished: stderr {:?}",
            std::fs::read_to_string(stderr_file)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    serde_json::from_slice(&std::fs::read(&done).unwrap()).unwrap()
}

fn claude_teams_on_the_private_server(server: &PrivateServer) {
    let fixture = newest_fixture();
    let fixture_str = fixture.display().to_string();
    let (leader, report_str, stderr_file) =
        launch_leader(server, "lead", &[("FAKE_CLAUDE_FIXTURE", &fixture_str)]);
    std::fs::write(format!("{report_str}.start"), "1").unwrap();
    let paused = PathBuf::from(format!("{report_str}.paused"));
    let tm2_env = PathBuf::from(format!("{report_str}.tm2-env"));
    let deadline = Instant::now() + Duration::from_secs(90);
    while !(paused.exists() && tm2_env.exists()) {
        assert!(
            Instant::now() < deadline,
            "the Leader never paused: stderr {:?}",
            std::fs::read_to_string(&stderr_file)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let result: Value = serde_json::from_slice(&std::fs::read(&paused).unwrap()).unwrap();
    assert_eq!(result["mismatches"], json!([]), "{result:#}");
    assert_eq!(result["split2"]["exit"], 0, "{result:#}");
    assert_eq!(result["split2"]["stdout"], "%3\n", "{result:#}");
    assert_eq!(
        result["args"],
        json!(["--teammate-mode", "tmux"]),
        "{result:#}"
    );
    let stderr = std::fs::read_to_string(&stderr_file).unwrap();
    assert!(!stderr.contains("claude-teams: no team"), "{stderr}");

    // The Team record (P2.2, P2.3).
    let teams = server.dir.join("xdg/tmux-tools/teams");
    let team_dirs: Vec<PathBuf> = std::fs::read_dir(&teams)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert_eq!(team_dirs.len(), 1, "{team_dirs:?}");
    let team: Value =
        serde_json::from_slice(&std::fs::read(team_dirs[0].join("team.json")).unwrap()).unwrap();
    let instance = server.rex_json(&["server", "status", "--json"])["instance_id"].clone();
    assert_eq!(team["host"], "rex", "{team}");
    assert_eq!(team["server_id"], instance, "{team}");
    assert_eq!(team["leader"]["handle"], leader.as_str(), "{team}");
    assert!(team["tmux"].is_null(), "{team}");
    let id = team["id"].as_str().unwrap().to_owned();
    let own: Value = serde_json::from_str(result["self"].as_str().unwrap())
        .unwrap_or_else(|_| panic!("self --format json: {result:#}"));
    assert_eq!(
        (&own["host"], &own["role"], &own["team"], &own["name"]),
        (
            &Value::from("rex"),
            &Value::from("leader"),
            &Value::from(id.as_str()),
            &Value::from("lead")
        ),
        "{own}"
    );

    // `list --format json` (PRD-261108-0447-01 P8).
    let rows = json_payload(&server.cli(&["list", "--format", "json"]), "list the team");
    let row = |name: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("{name}: {rows:#}"))
            .clone()
    };
    let tm1 = row("lead.tm1");
    let tm2 = row("lead.tm2");
    for (teammate, shim_id) in [(&tm1, "%2"), (&tm2, "%3")] {
        assert_eq!(teammate["team"], id.as_str(), "{teammate}");
        assert_eq!(teammate["parent"], "lead", "{teammate}");
        assert_eq!(teammate["shim_id"], shim_id, "{teammate}");
        assert_eq!(teammate["orphan"], false, "{teammate}");
        assert_eq!(teammate["status"], "running", "{teammate}");
    }
    let tm1_block = tm1["pane"].as_str().unwrap().to_owned();
    let tm2_block = tm2["pane"].as_str().unwrap().to_owned();

    // PRD-261112-2108-01 P3.4: the fixture's `respawn-pane` of `lead.tm1` renamed the
    // new block from its transitional label to the pane name.
    let tm1_label = server
        .blocks()
        .into_iter()
        .find(|block| block["block_id"] == tm1_block.as_str())
        .map(|block| block["label"].clone());
    assert_eq!(tm1_label, Some(json!("lead.tm1")), "{tm1_block}");

    // P3.1, P3.3: the layout of the Leader's window.
    let inspect = server.rex_json(&["session", "inspect", &server.session, "--json"]);
    let rect = |block: &str| {
        inspect["windows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|window| window["layers"].as_array().cloned().unwrap_or_default())
            .find_map(|layer| block_rect(&layer["layout"], block))
            .unwrap_or_else(|| panic!("{block} in {inspect:#}"))
    };
    let (lead_rect, tm1_rect, tm2_rect) = (rect(&leader), rect(&tm1_block), rect(&tm2_block));
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(tm1_rect[0], lead_rect[0] + lead_rect[2]) && close(tm1_rect[1], lead_rect[1]),
        "lead.tm1 is right of the Leader: {lead_rect:?} {tm1_rect:?}"
    );
    assert!(
        close(tm2_rect[1], tm1_rect[1] + tm1_rect[3]) && close(tm2_rect[0], tm1_rect[0]),
        "lead.tm2 is below lead.tm1: {tm1_rect:?} {tm2_rect:?}"
    );

    // P4: the teammate's own environment.
    let env = std::fs::read_to_string(&tm2_env).unwrap();
    let value = |key: &str| {
        env.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
    };
    assert_eq!(value("K").as_deref(), Some("V"), "{env}");
    assert_eq!(value("REX_BLOCK"), Some(tm2_block.clone()), "{env}");
    assert_ne!(value("REX_BLOCK"), Some(leader.clone()), "{env}");
    assert_ne!(value("REX_SESSION").as_deref(), Some("y"), "{env}");
    assert_ne!(value("REX_SERVER").as_deref(), Some("z"), "{env}");
    for key in ["TT_TEAM", "TT_HOST", "TMUX"] {
        assert_eq!(value(key), None, "{key}: {env}");
    }

    // The clean exit closes both teammates and removes the team directory (P2.5).
    std::fs::write(format!("{report_str}.go"), "1").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while team_dirs[0].exists() {
        assert!(
            Instant::now() < deadline,
            "the clean exit never removed the team directory"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let left: Vec<Value> = server
        .blocks()
        .into_iter()
        .filter(|block| block["block_id"] == tm1_block || block["block_id"] == tm2_block)
        .collect();
    assert!(left.is_empty(), "the teammates are closed: {left:?}");
}

/// The `list --format json` rows of the private server.
fn private_rows(server: &PrivateServer, ctx: &str) -> Vec<Value> {
    json_payload(&server.cli(&["list", "--format", "json"]), ctx)
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// PRD-261115-0921-01 P7.2, PRD-261108-0447-01 P10.2 on the private server: each trace
/// fixture of `tests/fixtures/claude-teams/` is replayed in full, its `kill-pane`
/// included, by its own Leader block `rp<n>` in its own Team, with the fixture's
/// version; every call's stdout and exit equal the filled fixture line. The
/// `kill-pane` closes the teammate's block, and the clean exit removes the team
/// directory. Each Leader block is then killed.
fn every_fixture_replays_on_the_private_server(server: &PrivateServer) {
    for (index, fixture) in all_fixtures().iter().enumerate() {
        let key = fixture.file_stem().unwrap().to_str().unwrap().to_owned();
        eprintln!("P10.2 private-server replay of fixture {key}");
        let name = format!("rp{}", index + 1);
        let fixture_str = fixture.display().to_string();
        let (_leader, report, stderr_file) = launch_leader(
            server,
            &name,
            &[
                ("FAKE_CLAUDE_FIXTURE", &fixture_str),
                ("FAKE_CLAUDE_VERSION", &key),
                ("FAKE_CLAUDE_MODE", "full"),
            ],
        );
        std::fs::write(format!("{report}.start"), "1").unwrap();
        let result = wait_done(&report, &stderr_file);
        let lines = std::fs::read_to_string(fixture)
            .unwrap()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        assert_eq!(result["calls"], lines as u64, "{key}: {result:#}");
        assert_eq!(result["mismatches"], json!([]), "{key}: {result:#}");

        let team_dir = server
            .dir
            .join("xdg/tmux-tools/teams")
            .join(result["team"].as_str().unwrap());
        let deadline = Instant::now() + Duration::from_secs(60);
        while team_dir.exists() {
            assert!(
                Instant::now() < deadline,
                "{key}: the clean exit never removed the team directory"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let stderr = std::fs::read_to_string(&stderr_file).unwrap();
        assert!(!stderr.contains("claude-teams:"), "{key}: {stderr}");

        let teammate = format!("{name}.tm1");
        let rows = private_rows(server, "list after the replay");
        assert!(
            rows.iter().all(|row| row["name"] != teammate.as_str()),
            "{key}: {teammate} is gone: {rows:#?}"
        );
        let labelled: Vec<Value> = server
            .blocks()
            .into_iter()
            .filter(|block| block["label"] == teammate.as_str())
            .collect();
        assert!(
            labelled.is_empty(),
            "{key}: kill-pane closed {teammate}: {labelled:?}"
        );

        server
            .cli(&["kill", "--target", &name, "--any"])
            .assert_success(&format!("kill {name}"));
    }
}

/// PRD-261108-0447-01 P2.5, P8.2–P8.4 on the private server: a Leader `orph` whose
/// Claude starts one teammate and exits 3 leaves `orph.tm1` live; `list --format json`
/// reports it with `team`, `parent: "orph"`, and `orphan: true` (the Team's
/// `leader.lock` is no longer held), and `kill --target orph.tm1` removes it from
/// `tmux-tools list` and from the Rex block listing. The Leader block is then killed.
fn a_failed_leader_leaves_an_orphan_on_the_private_server(server: &PrivateServer) {
    let (_leader, report, stderr_file) =
        launch_leader(server, "orph", &[("FAKE_CLAUDE_MODE", "fail")]);
    std::fs::write(format!("{report}.start"), "1").unwrap();
    let result = wait_done(&report, &stderr_file);
    assert_eq!(result["split"]["exit"], 0, "{result:#}");
    assert_eq!(result["split"]["stdout"], "%2\n", "{result:#}");
    let team = result["team"].as_str().unwrap().to_owned();

    // The Launcher exits after Claude: wait until its lock is released.
    let deadline = Instant::now() + Duration::from_secs(60);
    let row = loop {
        let rows = private_rows(server, "list the orphan");
        let row = rows
            .iter()
            .find(|row| row["name"] == "orph.tm1")
            .unwrap_or_else(|| panic!("orph.tm1 survives the failed Leader: {rows:#?}"))
            .clone();
        if row["orphan"] == true {
            break row;
        }
        assert!(
            Instant::now() < deadline,
            "orph.tm1 never became an Orphan: {row}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(row["team"], team.as_str(), "{row}");
    assert_eq!(row["parent"], "orph", "{row}");
    assert_eq!(row["shim_id"], "%2", "{row}");
    let block = row["pane"].as_str().unwrap().to_owned();
    assert!(
        server
            .blocks()
            .iter()
            .any(|b| b["block_id"] == block.as_str()),
        "the teammate block survives"
    );
    assert!(
        server.dir.join("xdg/tmux-tools/teams").join(&team).exists(),
        "a failed exit keeps the team directory"
    );

    server
        .cli(&["kill", "--target", "orph.tm1"])
        .assert_success("kill the Orphan orph.tm1");
    let rows = private_rows(server, "list after the Orphan kill");
    assert!(
        rows.iter().all(|row| row["name"] != "orph.tm1"),
        "{rows:#?}"
    );
    assert!(
        server
            .blocks()
            .iter()
            .all(|b| b["block_id"] != block.as_str()),
        "the Orphan's block is closed"
    );
    server
        .cli(&["kill", "--target", "orph", "--any"])
        .assert_success("kill orph");
}

// --- P8.4, P8.6: the opt-in live run on the default server ------------------------------

/// The opt-in switch of the live run (PRD-261112-2108-01 `## Contracts`, P8.4).
const LIVE_SWITCH: &str = "TMUX_TOOLS_LIVE_REX";

/// The calling plain Rex terminal's variables that the live run passes on (P2.2, P2.3,
/// P3.1); the suite removes them from its own environment when it loads.
const LIVE_REX_VARS: [&str; 4] = [
    "REX_BLOCK",
    "REX_SESSION",
    "REX_SERVER",
    "TMUX_TOOLS_REX_SESSION",
];

/// The `claude` of the original `PATH`; a missing one fails the opt-in run.
fn live_claude() -> PathBuf {
    original_environment()
        .get("PATH")
        .map(String::as_str)
        .unwrap_or_default()
        .split(':')
        .map(|entry| Path::new(entry).join("claude"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("{LIVE_SWITCH}=1 needs `claude` on PATH"))
}

/// One live run on the default Rex server, from the calling plain Rex terminal. Every
/// CLI and `rex` call gets that terminal's `REX_*` variables and the original `PATH`,
/// never `TMUX` or `TMUX_PANE`. Each block the run creates carries one of the
/// run-unique names of [`LiveRun::n`] as its label (P3.4); `Drop` kills the run's
/// names, then closes every block id the run recorded (whatever the block listing
/// gives), and then every listed block that carries such a label, also on failure,
/// and never another block (P8.6).
struct LiveRun {
    rex: PathBuf,
    dir: PathBuf,
    tag: String,
    caller: String,
    session: String,
    names: std::cell::RefCell<Vec<String>>,
    created: std::cell::RefCell<Vec<String>>,
}

impl LiveRun {
    fn start() -> LiveRun {
        let original = original_environment();
        assert!(
            !original.contains_key("TMUX"),
            "{LIVE_SWITCH}=1 runs from a plain Rex terminal: unset TMUX"
        );
        let caller = original
            .get("REX_BLOCK")
            .cloned()
            .unwrap_or_else(|| panic!("{LIVE_SWITCH}=1 runs from a Rex terminal: REX_BLOCK"));
        let rex = original
            .get("PATH")
            .map(String::as_str)
            .unwrap_or_default()
            .split(':')
            .map(|entry| Path::new(entry).join("rex"))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| panic!("{LIVE_SWITCH}=1 needs `rex` on PATH"));
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let tag = format!("{:x}", (millis ^ u64::from(std::process::id())) & 0xff_ffff);
        let dir = tempfile_dir("rex-live");
        for sub in ["config/tmux-tools", "bin"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        write_executable(&dir.join("bin/hexecho.py"), HEXECHO);
        // A block need not carry the original `PATH`: the built-in `claude` runs by the
        // absolute path of the original `PATH`'s `claude`.
        std::fs::write(
            dir.join("config/tmux-tools/agents.toml"),
            format!("[claude]\nbinary = \"{}\"\n", live_claude().display()),
        )
        .unwrap();
        let mut run = LiveRun {
            rex,
            dir,
            tag,
            caller,
            session: String::new(),
            names: std::cell::RefCell::new(Vec::new()),
            created: std::cell::RefCell::new(Vec::new()),
        };
        run.session = run
            .all_blocks()
            .into_iter()
            .find(|(_, block)| block["block_id"] == run.caller.as_str())
            .map(|(session, _)| session)
            .unwrap_or_else(|| panic!("the default server lists no block {}", run.caller));
        run
    }

    /// The run-unique tmux-tools name for `role`, recorded for the teardown.
    fn n(&self, role: &str) -> String {
        let name = format!("tt.live{}.{role}", self.tag);
        self.names.borrow_mut().push(name.clone());
        name
    }

    /// The calling terminal's Rex: its `REX_*` variables, the original `PATH`, no
    /// `TMUX`, and none of the calling Claude session's variables.
    fn env(&self, command: &mut Command) {
        let original = original_environment();
        for key in claude_session_vars() {
            command.env_remove(key);
        }
        command
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("PATH", original.get("PATH").cloned().unwrap_or_default());
        for key in LIVE_REX_VARS {
            match original.get(key) {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
    }

    /// The real `rex` as the backend calls it: `--autostart=false`, and `-S
    /// "$REX_SERVER"` when the calling terminal sets it (P2.3).
    fn rex(&self, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(&self.rex);
        self.env(&mut command);
        command.arg("--autostart=false");
        if let Some(address) = original_environment().get("REX_SERVER") {
            command.args(["-S", address]);
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("run rex")
    }

    fn rex_json(&self, args: &[&str]) -> Value {
        let out = self.rex(args);
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("rex {args:?}: {out:?}"))
    }

    /// Every block of every session, with its session id: `rex ls --json`, then `rex
    /// block ls -s <session> --json` for each session.
    fn all_blocks(&self) -> Vec<(String, Value)> {
        let sessions = self.rex_json(&["ls", "--json"])["sessions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut blocks = Vec::new();
        for session in sessions {
            let Some(id) = session["session_id"].as_str() else {
                continue;
            };
            for block in self.rex_json(&["block", "ls", "-s", id, "--json"])["blocks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                blocks.push((id.to_owned(), block));
            }
        }
        blocks
    }

    /// The listed blocks the run created: by block id, or by a label that is a name of
    /// this run.
    fn own_blocks(&self) -> Vec<(String, Value)> {
        let names = self.names.borrow().clone();
        let created = self.created.borrow().clone();
        self.all_blocks()
            .into_iter()
            .filter(|(_, block)| {
                block["block_id"]
                    .as_str()
                    .is_some_and(|id| created.iter().any(|own| own == id))
                    || block["label"]
                        .as_str()
                        .is_some_and(|label| names.iter().any(|own| own == label))
            })
            .collect()
    }

    /// The CLI from the repository root (a directory Claude already trusts), with the
    /// operator's registry (`XDG_STATE_HOME` as inherited) and the run's agent config.
    fn cli(&self, args: &[&str]) -> CmdOutput {
        let mut command = Command::new(BIN);
        self.env(&mut command);
        let child = command
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("SHELL", "/bin/sh")
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

    /// The created block's id from a spawn's JSON result, recorded for the teardown.
    fn pane_of(&self, out: &CmdOutput, ctx: &str) -> String {
        let pane = json_payload(out, ctx)["pane_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{ctx}: pane_id: {out:?}"))
            .to_owned();
        self.created.borrow_mut().push(pane.clone());
        pane
    }

    /// The listed block `id` and its session.
    fn block(&self, id: &str) -> (String, Value) {
        self.all_blocks()
            .into_iter()
            .find(|(_, block)| block["block_id"] == id)
            .unwrap_or_else(|| panic!("the default server lists no block {id}"))
    }

    /// Wait until the raw echo in `block` has received the bytes `hex`, whatever reads
    /// they arrived in.
    fn wait_hex(&self, block: &str, hex: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let capture =
                String::from_utf8_lossy(&self.rex(&["capture", "-b", block]).stdout).into_owned();
            let received: String = capture
                .lines()
                .filter_map(|line| line.trim().strip_prefix("HEX "))
                .collect();
            if received.contains(hex) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{block} never received {hex}: {capture}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Wait until `block`'s capture contains `text`.
    fn wait_capture(&self, block: &str, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let capture =
                String::from_utf8_lossy(&self.rex(&["capture", "-b", block]).stdout).into_owned();
            if capture.contains(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{block} never showed {text:?}: {capture}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for LiveRun {
    fn drop(&mut self) {
        // Best effort: a panic here during an unwind would abort, so each step is
        // caught, and the leak assertion of the test body is the check.
        let names = self.names.borrow().clone();
        for name in &names {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.cli(&["kill", "--target", name, "--any"])
            }));
        }
        // Close every recorded block id first, independently of any listing result;
        // then, as an extra pass, every listed block with a run label (a create whose
        // id is unknown). Never the caller's block.
        let close = |id: &str| {
            if id != self.caller {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.rex(&["block", "close", id])
                }));
            }
        };
        let created = self.created.borrow().clone();
        for id in &created {
            close(id);
        }
        let listed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.own_blocks()))
            .unwrap_or_default();
        for (_, block) in listed {
            if let Some(id) = block["block_id"].as_str() {
                if !created.iter().any(|own| own == id) {
                    close(id);
                }
            }
        }
        // `list` reconciles the records of blocks closed above.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.cli(&["list", "--format", "json"])
        }));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// P8.4: the contract suite on the default Rex server, from a plain Rex terminal (no
/// `$TMUX`, `REX_BLOCK` set): a `--split v` block beside the caller, a key send and a
/// literal send, a Claude agent pane with Host state, and a multi-line prompt to the
/// Claude pane with guaranteed delivery (P6.4). New blocks go to the caller's session
/// (P3.1). P8.6: the run kills every block it created, also on failure, and ends by
/// asserting that neither `tmux-tools list` nor the Rex block listing shows one.
/// Opt-in with `TMUX_TOOLS_LIVE_REX=1`.
#[test]
fn rex_live_contract_suite() {
    if original_environment().get(LIVE_SWITCH).map(String::as_str) != Some("1") {
        return;
    }
    let run = LiveRun::start();

    // The run reaches the default server: the server the calling terminal selects is
    // the one `rex` reaches without `-S` and without `REX_SERVER`.
    let selected = run.rex_json(&["server", "status", "--json"]);
    let default = Command::new(&run.rex)
        .args(["--autostart=false", "server", "status", "--json"])
        .env_remove("REX_SERVER")
        .stdin(Stdio::null())
        .output()
        .expect("run rex server status");
    let default: Value = serde_json::from_slice(&default.stdout)
        .unwrap_or_else(|_| panic!("rex server status on the default server: {default:?}"));
    assert_eq!(
        selected["instance_id"], default["instance_id"],
        "{LIVE_SWITCH}=1 runs on the default server: {selected} vs {default}"
    );
    let (_, caller_block) = run.block(&run.caller);
    let caller_window = caller_block["window_id"].clone();
    println!(
        "P8.4 live run {}: server {} caller {} session {} window {caller_window}",
        run.tag, selected["instance_id"], run.caller, run.session
    );

    let report = json_payload(&run.cli(&["self", "--format", "json"]), "self");
    assert_eq!(report["host"], "rex", "{report}");
    let status = json_payload(&run.cli(&["status", "--format", "json"]), "status");
    assert_eq!(status["pane_id"], run.caller.as_str(), "{status}");

    // A `--split v` block beside the caller: in the caller's session and window, with
    // the pane name as its label (P3.1, P3.3, P3.4).
    let split_name = run.n("v");
    let hexecho = run.dir.join("bin/hexecho.py").display().to_string();
    let split = run.pane_of(
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
    let (session, block) = run.block(&split);
    assert_eq!(session, run.session, "{block}");
    assert_eq!(block["window_id"], caller_window, "{block}");
    assert_eq!(block["label"], split_name.as_str(), "{block}");
    println!("P8.4 --split v block {split} ({split_name}) is beside the caller: ok");

    // A literal send and a key send (P5.3).
    run.wait_capture(&split, "HEXECHO READY");
    run.cli(&["send", "--target", &split_name, "Hi"])
        .assert_success("send literal");
    run.wait_hex(&split, "4869");
    run.cli(&["send", "--target", &split_name, "Up"])
        .assert_success("send key");
    run.wait_hex(&split, "48691b5b41");
    println!("P8.4 literal and key send: ok");

    // A Claude agent pane with Host state (P6.2), in the caller's session (P3.1).
    let claude_name = run.n("claude");
    let claude = run.pane_of(
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
    let (session, block) = run.block(&claude);
    assert_eq!(session, run.session, "{block}");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let status = json_payload(
            &run.cli(&["status", "--target", &claude_name, "--format", "json"]),
            "status claude",
        );
        if matches!(status["host_state"].as_str(), Some("idle" | "done")) {
            assert!(status["host_word"].is_string(), "{status}");
            println!(
                "P8.4 Claude pane {claude} ({claude_name}) reports Host state {} ({}): ok",
                status["host_state"], status["host_word"]
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude never reported a Host state: {status}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(1));

    // A multi-line prompt to the Claude pane: guaranteed delivery (P6.4). The run-unique
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
    assert_eq!(result["host"], "rex", "{result}");
    assert_eq!(result["one_submission_delivery"], "guaranteed", "{result}");
    assert_eq!(result["submit"], "confirmed", "{result}");
    assert!(
        result["output_since_prompt"]
            .as_str()
            .is_some_and(|output| output.contains(&token)),
        "the reply token {token}: {result}"
    );
    println!("P8.4 multi-line prompt to Claude, guaranteed delivery, reply {token}: ok");

    // P8.6: kill every created block, then neither `tmux-tools list` nor the Rex block
    // listing shows one.
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
                        || row["pane"]
                            .as_str()
                            .is_some_and(|pane| run.created.borrow().iter().any(|own| own == pane))
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(listed.is_empty(), "tmux-tools list shows {listed:?}");
    let left = run.own_blocks();
    assert!(left.is_empty(), "the Rex block listing shows {left:?}");
    println!("P8.6 teardown: no block of the run is left: ok");
}
