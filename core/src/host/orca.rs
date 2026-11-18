//! The Orca Host backend: every `orca` CLI invocation `tmux-tools` runs is built here.
//!
//! The backend calls `orca … --json` and reads `result` fields (P9.1). The calling pane
//! is `ORCA_TERMINAL_HANDLE` and the calling worktree `ORCA_WORKTREE_ID`. Orca cannot
//! store custom data on a pane, so the Pane registration is a Pane record
//! ([`crate::registry`]) under the runtime's `runtimeId`.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::host::{
    CaptureRange, Delivery, HistoryProbe, Host, HostCapabilities, HostPort, HostState, ListScope,
    PaneDescription, PaneListing, PaneSummary, RegistrationKey, ScreenCapture, SelectionSource,
    SplitDirection, Submission, Unsupported, WindowHome,
};
use crate::layout::{Registration, WindowGeometry};
use crate::names::{self, Registered};
use crate::registry::{self, NameTaken, PaneLiveness, PaneRecord, RecordCheck};

/// tmux's key grammar, shared with the Herdr backend (PRD-261109-2223-01 P5.2).
pub(super) mod keys;

/// The calling pane's terminal handle, set by Orca inside its terminals.
pub const ORCA_TERMINAL_HANDLE: &str = "ORCA_TERMINAL_HANDLE";
/// The calling pane's worktree id, set by Orca inside its terminals.
pub const ORCA_WORKTREE_ID: &str = "ORCA_WORKTREE_ID";

/// The runtime capability without which `prompt` refuses (P9.6).
const PROMPT_DELIVERY_CAPABILITY: &str = "terminal.prompt-delivery.v1";

/// Lines per history read page.
const READ_PAGE_LINES: u32 = 1000;

/// How long one `orca` call may run beyond any wait it asks Orca to perform.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// The commands with no side effect, which are retried when they exit 0 without a JSON
/// answer: Orca 1.4.253 answered so for a few seconds after a pane was created. A
/// command that acts (`terminal create`, `split`, `send`, `close`) is never retried, as
/// its exit 0 can mean the action already happened.
const READ_ONLY_COMMANDS: &[&str] = &[
    "status",
    "terminal list",
    "terminal show",
    "terminal read",
    "worktree ps",
];

/// The pauses between the attempts of a retried read-only call, growing: at most 5
/// attempts over at most 1.9 s of waiting.
const EMPTY_ANSWER_PAUSES: [Duration; 4] = [
    Duration::from_millis(200),
    Duration::from_millis(300),
    Duration::from_millis(500),
    Duration::from_millis(900),
];

/// How many fresh derivations a spawn tries when a derived name is already reserved.
const DERIVED_NAME_ATTEMPTS: usize = 16;

/// The Orca Host backend.
pub struct OrcaHost {
    source: SelectionSource,
    status: OnceLock<std::result::Result<RuntimeStatus, String>>,
    /// `terminal show` per handle, read once per invocation (tab and leaf ids are fixed).
    shown: Mutex<HashMap<String, Value>>,
    /// The `paneKey` (`<tabId>:<leafId>`) of every handle a `terminal list` reported.
    pane_keys: Mutex<HashMap<String, String>>,
}

#[derive(Clone, Debug)]
struct RuntimeStatus {
    runtime_id: String,
    capabilities: Vec<String>,
}

/// The `ok: false` codes with which the Orca CLI reports that it cannot reach the
/// runtime (P3.4).
const UNREACHABLE_CODES: &[&str] = &[
    "runtime_unavailable",
    "runtime_connection_failed",
    "runtime_disconnected",
    "runtime_manually_disconnected",
    "runtime_open_failed",
    "runtime_open_timeout",
    "runtime_environment_asleep",
    "runtime_environment_waking",
    "runtime_environment_wake_failed",
    "not_running",
    "host_unreachable",
    "unreachable",
];

/// An `orca` command that ran and answered `ok: false`.
#[derive(Debug)]
pub struct OrcaError {
    pub command: String,
    pub code: String,
    pub message: String,
    pub data: Value,
    /// The selection source, when `code` reports an unreachable runtime (P3.4).
    pub unreachable: Option<SelectionSource>,
}

impl std::fmt::Display for OrcaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(source) = &self.unreachable {
            write!(
                formatter,
                "host orca (selected by {source}) is unreachable: "
            )?;
        }
        if self.message.is_empty() || self.message == self.code {
            write!(formatter, "orca {}: {}", self.command, self.code)
        } else {
            write!(
                formatter,
                "orca {}: {}: {}",
                self.command, self.code, self.message
            )
        }
    }
}

impl std::error::Error for OrcaError {}

/// One `terminal list` answer.
struct TerminalList {
    terminals: Vec<Value>,
    /// `truncated` is not true and `hostScope.omittedHostIds` is empty (P4.5).
    complete: bool,
}

impl TerminalList {
    fn handles(&self) -> HashSet<String> {
        self.terminals
            .iter()
            .filter_map(|terminal| str_field(terminal, "handle"))
            .map(str::to_owned)
            .collect()
    }

    fn find(&self, handle: &str) -> Option<&Value> {
        self.terminals
            .iter()
            .find(|terminal| str_field(terminal, "handle") == Some(handle))
    }
}

/// What one `orca` run printed on stdout.
enum Answer {
    Json(Value),
    /// Empty or non-JSON stdout, with the exit code and stderr.
    NotJson {
        code: Option<i32>,
        stderr: String,
    },
}

/// One record after the P4.5 check.
struct Checked {
    record: PaneRecord,
    check: RecordCheck,
}

impl OrcaHost {
    /// The backend for an invocation that selected Orca through `source`.
    pub fn new(source: SelectionSource) -> OrcaHost {
        OrcaHost {
            source,
            status: OnceLock::new(),
            shown: Mutex::new(HashMap::new()),
            pane_keys: Mutex::new(HashMap::new()),
        }
    }

    // --- The CLI -------------------------------------------------------------------

    /// Run `orca <args> --json` and return its `result`. Not reaching the CLI names the
    /// host and the selection source (P3.4).
    fn call(&self, args: &[&str]) -> Result<Value> {
        self.call_with_timeout(args, CALL_TIMEOUT)
    }

    fn call_with_timeout(&self, args: &[&str], timeout: Duration) -> Result<Value> {
        let command_name = args
            .iter()
            .take_while(|arg| !arg.starts_with("--"))
            .copied()
            .collect::<Vec<_>>()
            .join(" ");
        let pauses: &[Duration] = if READ_ONLY_COMMANDS.contains(&command_name.as_str()) {
            &EMPTY_ANSWER_PAUSES
        } else {
            &[]
        };
        let mut pauses = pauses.iter();
        loop {
            match self.call_once(args, &command_name, timeout)? {
                Answer::Json(parsed) => return self.result(parsed, command_name),
                Answer::NotJson { code, stderr } => match pauses.next() {
                    Some(pause) if code == Some(0) => std::thread::sleep(*pause),
                    _ => bail!(
                        "orca {command_name} exited {} without a JSON answer: {}",
                        code.unwrap_or(-1),
                        stderr
                    ),
                },
            }
        }
    }

    /// One run of `orca <args> --json`.
    fn call_once(&self, args: &[&str], command_name: &str, timeout: Duration) -> Result<Answer> {
        let mut child = Command::new("orca")
            .args(args)
            .arg("--json")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| self.unreachable(&format!("cannot run the orca CLI: {error}")))?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let out_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = stdout.read_to_end(&mut buffer);
            buffer
        });
        let err_reader = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = stderr.read_to_end(&mut buffer);
            buffer
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "orca {command_name}: no answer within {}s",
                    timeout.as_secs()
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stdout = out_reader.join().unwrap_or_default();
        let stderr = err_reader.join().unwrap_or_default();
        Ok(match serde_json::from_slice(&stdout) {
            Ok(parsed) => Answer::Json(parsed),
            Err(_) => Answer::NotJson {
                code: status.code(),
                stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
            },
        })
    }

    /// The `result` of a JSON answer, or its error.
    fn result(&self, parsed: Value, command_name: String) -> Result<Value> {
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(parsed.get("result").cloned().unwrap_or(Value::Null));
        }
        let error = parsed.get("error").cloned().unwrap_or(Value::Null);
        let code = str_field(&error, "code").unwrap_or("error").to_owned();
        Err(OrcaError {
            command: command_name,
            unreachable: UNREACHABLE_CODES
                .contains(&code.as_str())
                .then(|| self.source.clone()),
            code,
            message: str_field(&error, "message").unwrap_or_default().to_owned(),
            data: error.get("data").cloned().unwrap_or(Value::Null),
        }
        .into())
    }

    fn unreachable(&self, detail: &str) -> anyhow::Error {
        anyhow!(
            "host orca (selected by {}) is unreachable: {detail}",
            self.source
        )
    }

    /// `orca status`, read once per invocation.
    fn status(&self) -> Result<&RuntimeStatus> {
        let status = self.status.get_or_init(|| {
            let result = self.call(&["status"]).map_err(|error| {
                let reported = error.downcast_ref::<OrcaError>();
                if reported.is_some_and(|orca| orca.unreachable.is_none()) {
                    format!("{}", self.unreachable(&format!("{error:#}")))
                } else {
                    format!("{error:#}")
                }
            })?;
            let runtime = result.get("runtime").cloned().unwrap_or(Value::Null);
            if runtime.get("reachable").and_then(Value::as_bool) == Some(false) {
                return Err(format!(
                    "{}",
                    self.unreachable("orca status reports the runtime as not reachable")
                ));
            }
            let runtime_id = str_field(&runtime, "runtimeId")
                .ok_or_else(|| {
                    format!(
                        "{}",
                        self.unreachable("orca status reports no result.runtime.runtimeId")
                    )
                })?
                .to_owned();
            let capabilities = runtime
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Ok(RuntimeStatus {
                runtime_id,
                capabilities,
            })
        });
        status.as_ref().map_err(|message| anyhow!("{message}"))
    }

    /// The registry's `server_id`: the runtime's `runtimeId`. Reading it also deletes
    /// the record directories of every other runtime (P4.3).
    fn server_id(&self) -> Result<String> {
        let runtime_id = self.status()?.runtime_id.clone();
        registry::sweep_other_servers(Host::Orca, &runtime_id)?;
        Ok(runtime_id)
    }

    fn terminal_list(&self) -> Result<TerminalList> {
        let result = self.call(&["terminal", "list"])?;
        let terminals = result
            .get("terminals")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let truncated = result.get("truncated").and_then(Value::as_bool) == Some(true);
        let omitted = result
            .pointer("/hostScope/omittedHostIds")
            .and_then(Value::as_array)
            .is_some_and(|ids| !ids.is_empty());
        let mut pane_keys = self.pane_keys.lock().expect("pane key cache");
        for terminal in &terminals {
            if let (Some(handle), Some(tab), Some(leaf)) = (
                str_field(terminal, "handle"),
                str_field(terminal, "tabId"),
                str_field(terminal, "leafId"),
            ) {
                pane_keys.insert(handle.to_owned(), format!("{tab}:{leaf}"));
            }
        }
        drop(pane_keys);
        Ok(TerminalList {
            terminals,
            complete: !truncated && !omitted,
        })
    }

    /// `terminal show` of `handle`, once per invocation.
    fn show(&self, handle: &str) -> Result<Value> {
        if let Some(shown) = self.shown.lock().expect("show cache").get(handle) {
            return Ok(shown.clone());
        }
        let result = self.call(&["terminal", "show", "--terminal", handle])?;
        let terminal = result.get("terminal").cloned().unwrap_or(result);
        self.shown
            .lock()
            .expect("show cache")
            .insert(handle.to_owned(), terminal.clone());
        Ok(terminal)
    }

    fn calling_handle(&self) -> Result<String> {
        // PRD-261108-0447-01 P3.3: inside a team the calling pane is the Leader.
        if let Some(leader) = crate::team::leader_handle() {
            return Ok(leader);
        }
        env_value(ORCA_TERMINAL_HANDLE).ok_or_else(|| {
            anyhow!(
                "{ORCA_TERMINAL_HANDLE} is unset: this verb needs it to name the calling Orca pane"
            )
        })
    }

    // --- The registry ---------------------------------------------------------------

    /// The pane list and every record of the current runtime after the P4.5 check
    /// against it. Dead records and orphan reservations are deleted; they stay in the
    /// answer once.
    ///
    /// The records are read before the list, so a handle a concurrent spawn writes
    /// after the list was taken is never checked against that older list. A record is
    /// deleted only while it is still the record that was checked.
    fn checked_records(&self) -> Result<(TerminalList, Vec<Checked>)> {
        let server_id = self.server_id()?;
        let records = registry::load_all(Host::Orca, &server_id)?;
        let list = self.terminal_list()?;
        let listed = list.handles();
        let mut checked = Vec::new();
        for (record, age) in records {
            let check = registry::check(&record, &listed, list.complete, age);
            if matches!(
                check,
                RecordCheck::Settled(PaneLiveness::Dead | PaneLiveness::OrphanReservation)
            ) {
                match registry::load(Host::Orca, &server_id, &record.name)? {
                    Some(current) if current == record => {
                        registry::delete(Host::Orca, &server_id, &record.name)?;
                    }
                    // Rewritten since it was read (a spawn wrote its handle): the new
                    // record waits for the next check.
                    Some(_) => continue,
                    None => {}
                }
            }
            checked.push(Checked { record, check });
        }
        Ok((list, checked))
    }

    /// The record whose handle is `handle`, without a list check.
    fn record_for_handle(&self, handle: &str) -> Result<Option<PaneRecord>> {
        let server_id = self.server_id()?;
        Ok(registry::load_all(Host::Orca, &server_id)?
            .into_iter()
            .map(|(record, _)| record)
            .find(|record| record.handle.as_deref() == Some(handle)))
    }

    fn new_record(&self, name: &str, registration: &Registration<'_>) -> Result<PaneRecord> {
        let agent = registration.agent.as_ref();
        let team = registration.team.as_ref();
        let owned = |value: Option<&str>| value.map(str::to_owned);
        Ok(PaneRecord {
            schema: registry::PANE_RECORD_SCHEMA.to_owned(),
            name: name.to_owned(),
            host: Host::Orca,
            server_id: self.server_id()?,
            handle: None,
            agent: agent
                .map(|agent| agent.agent.to_owned())
                .or_else(|| owned(team.and_then(|team| team.agent))),
            access: agent.and_then(|agent| agent.access).map(str::to_owned),
            surface: agent.map(|agent| agent.surface.to_owned()),
            surface_unvalidated: agent.is_some_and(|agent| agent.surface_unvalidated),
            cwd: registration.cwd.unwrap_or_default().into(),
            launched_at: registration.launched_at.to_owned(),
            parent: owned(registration.parent),
            team: team.map(|team| team.team.to_owned()),
            shim_id: team.map(|team| team.shim_id.to_owned()),
            title: owned(team.and_then(|team| team.title)),
            claude_agent_name: owned(team.and_then(|team| team.claude_agent_name)),
            claude_team_name: owned(team.and_then(|team| team.claude_team_name)),
        })
    }

    /// Reserve the explicit name, or a fresh derived one.
    fn reserve(&self, registration: &Registration<'_>) -> Result<PaneRecord> {
        if let Some(name) = registration.name {
            let record = self.new_record(name, registration)?;
            registry::reserve(&record)?;
            return Ok(record);
        }
        for _ in 0..DERIVED_NAME_ATTEMPTS {
            let record =
                self.new_record(&names::derive_name(registration.name_source), registration)?;
            match registry::reserve(&record) {
                Ok(()) => return Ok(record),
                Err(error) if error.downcast_ref::<NameTaken>().is_some() => continue,
                Err(error) => return Err(error),
            }
        }
        bail!(
            "could not derive an unused name from {:?}; pass --name",
            registration.name_source
        )
    }

    // --- Reading ---------------------------------------------------------------------

    /// One read of `handle`: `result.terminal`.
    fn read(&self, handle: &str, extra: &[&str]) -> Result<Value> {
        let mut args = vec!["terminal", "read", "--terminal", handle];
        args.extend_from_slice(extra);
        let result = self.call(&args)?;
        Ok(result.get("terminal").cloned().unwrap_or(result))
    }

    fn screen(&self, handle: &str) -> Result<String> {
        Ok(join_tail(&self.read(handle, &["--screen"])?))
    }

    /// Every retained line after `cursor`, paging with `nextCursor` while `limited` is
    /// true and `nextCursor` differs from `latestCursor` (P9.3).
    fn read_pages(&self, handle: &str, cursor: &str) -> Result<String> {
        let limit = page_lines().to_string();
        let mut cursor = cursor.to_owned();
        let mut text = String::new();
        loop {
            let page = self.read(handle, &["--cursor", &cursor, "--limit", &limit])?;
            text.push_str(&join_tail(&page));
            let limited = page.get("limited").and_then(Value::as_bool) == Some(true);
            let next = cursor_field(&page, "nextCursor");
            let latest = cursor_field(&page, "latestCursor");
            match next {
                Some(next) if limited && Some(&next) != latest.as_ref() && next != cursor => {
                    cursor = next;
                }
                _ => return Ok(text),
            }
        }
    }

    /// The whole retained history of `handle`.
    fn read_all(&self, handle: &str) -> Result<String> {
        let preview = self.read(handle, &["--limit", "1"])?;
        match cursor_field(&preview, "oldestCursor") {
            Some(oldest) => self.read_pages(handle, &oldest),
            None => Ok(join_tail(&self.read(handle, &[])?)),
        }
    }

    // --- Host state -------------------------------------------------------------------

    /// The `paneKey` of `handle`: from a `terminal list` of this invocation, else from
    /// `terminal show`. A pane Orca no longer knows has none.
    fn pane_key(&self, handle: &str) -> Result<Option<String>> {
        if let Some(key) = self.pane_keys.lock().expect("pane key cache").get(handle) {
            return Ok(Some(key.clone()));
        }
        let shown = match self.show(handle) {
            Ok(shown) => shown,
            Err(error) if self.is_command_failure(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(
            match (str_field(&shown, "tabId"), str_field(&shown, "leafId")) {
                (Some(tab), Some(leaf)) => Some(format!("{tab}:{leaf}")),
                _ => None,
            },
        )
    }

    /// The `worktree ps` agent row of `handle` (P9.10), joined by `paneKey`.
    fn agent_row(&self, handle: &str) -> Result<Option<Value>> {
        // A pane Orca no longer knows (a dead or unlisted record) has no Host state.
        let Some(pane_key) = self.pane_key(handle)? else {
            return Ok(None);
        };
        Ok(ps_row(&self.call(&["worktree", "ps"])?, &pane_key))
    }
}

/// The agent row whose `paneKey` is `pane_key` in one `worktree ps` answer.
fn ps_row(ps: &Value, pane_key: &str) -> Option<Value> {
    ps.get("worktrees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|worktree| worktree.get("agents").and_then(Value::as_array))
        .flatten()
        .find(|row| str_field(row, "paneKey") == Some(pane_key))
        .cloned()
}

/// The P9.10 mapping of one agent row: Host state and the time its main state began.
pub(crate) fn row_host_state(row: &Value) -> Option<(HostState, Option<u64>)> {
    if row.get("hasLivePty").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let main = row.get("mainAgent").filter(|main| main.is_object());
    let state = main
        .and_then(|main| str_field(main, "state"))
        .or_else(|| str_field(row, "state"))?;
    let began = main
        .and_then(|main| main.get("stateStartedAt"))
        .or_else(|| row.get("stateStartedAt"))
        .and_then(Value::as_f64)
        .map(|ms| ms as u64);
    let subagent_working = row
        .get("subagents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|subagent| str_field(subagent, "state") == Some("working"));
    if subagent_working {
        return Some((HostState::Working, began));
    }
    let host_state = match state {
        "working" => HostState::Working,
        "blocked" | "waiting" => HostState::Blocked,
        "done" => HostState::Done,
        _ => return None,
    };
    Some((host_state, began))
}

/// The P9.11 `submit` mapping of a send receipt's `prompt`, after the refusals.
pub(crate) fn receipt_submission(prompt: Option<&Value>) -> Submission {
    let Some(prompt) = prompt else {
        return Submission::Assumed;
    };
    let provider = str_field(prompt, "provider");
    let observation = str_field(prompt, "observation");
    if matches!(provider, Some("unsupported" | "old-host")) || observation == Some("unsupported") {
        return Submission::Assumed;
    }
    let stages: Vec<&str> = prompt
        .get("stages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if stages.contains(&"turn_started") {
        Submission::Confirmed
    } else if !stages.is_empty() && stages.iter().all(|stage| *stage == "input_accepted") {
        Submission::Accepted
    } else {
        Submission::Assumed
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// A cursor field, which Orca reports as a string (or a number on some hosts).
fn cursor_field(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(cursor) => Some(cursor.clone()),
        Value::Number(cursor) => Some(cursor.to_string()),
        _ => None,
    }
}

/// The first value under `key` anywhere in `value`.
fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|inner| find_key(inner, key))),
        Value::Array(items) => items.iter().find_map(|inner| find_key(inner, key)),
        _ => None,
    }
}

fn join_tail(terminal: &Value) -> String {
    let mut text = String::new();
    for line in terminal
        .get("tail")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        text.push_str(line.as_str().unwrap_or_default());
        text.push('\n');
    }
    text
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Lines per history page; `TMUX_TOOLS_TEST_ORCA_PAGE_LINES` overrides it in a
/// `test-fixtures` build so a contract test can make a response span several pages.
fn page_lines() -> u32 {
    #[cfg(feature = "test-fixtures")]
    if let Some(lines) = env_value("TMUX_TOOLS_TEST_ORCA_PAGE_LINES").and_then(|v| v.parse().ok()) {
        return lines;
    }
    READ_PAGE_LINES
}

fn unsupported(feature: &'static str) -> anyhow::Error {
    Unsupported {
        feature,
        host: Host::Orca,
    }
    .into()
}

impl HostPort for OrcaHost {
    fn host(&self) -> Host {
        Host::Orca
    }

    fn capabilities(&self) -> HostCapabilities {
        let capabilities = HostCapabilities {
            native_state: true,
            submit_confirmation: true,
            notify: false,
            tile: false,
            rebalance: false,
            size: false,
        };
        #[cfg(feature = "test-fixtures")]
        let capabilities = crate::host::with_test_override(capabilities);
        capabilities
    }

    fn is_command_failure(&self, error: &anyhow::Error) -> bool {
        error
            .downcast_ref::<OrcaError>()
            .is_some_and(|orca| orca.unreachable.is_none())
    }

    fn active_pane(&self, _target: &str) -> Result<String> {
        Err(unsupported("--window"))
    }

    fn scope_target(&self, _session: Option<&str>, _window: Option<&str>) -> String {
        String::new()
    }

    fn calling_pane(&self) -> Result<String> {
        self.calling_handle()
    }

    fn managed_pane(&self) -> Result<String> {
        self.calling_handle()
    }

    fn server_identity(&self) -> Result<String> {
        self.server_id()
    }

    fn create_split(
        &self,
        _target: &str,
        _direction: SplitDirection,
        _percent: Option<u32>,
        _command: &str,
        _detached: bool,
    ) -> Result<String> {
        bail!("orca creates panes only with a Pane record")
    }

    fn create_window(&self, _session: &str, _command: &str) -> Result<String> {
        bail!("orca creates panes only with a Pane record")
    }

    fn new_window_session(&self, _home: WindowHome<'_>) -> Result<String> {
        Err(unsupported("--session"))
    }

    fn ensure_managed_session(&self) -> Result<String> {
        Err(unsupported("--session"))
    }

    fn list_panes(&self, scope: ListScope<'_>) -> Result<Vec<PaneListing>> {
        if let ListScope::Session(_) = scope {
            return Err(unsupported("--session"));
        }
        let (list, checked) = self.checked_records()?;
        let mut rows = Vec::new();
        for Checked { record, check } in checked {
            let (dead, unknown) = match check {
                RecordCheck::Settled(PaneLiveness::Live) => (false, false),
                RecordCheck::Settled(PaneLiveness::Dead) => (true, false),
                RecordCheck::Unlisted => (false, true),
                RecordCheck::Settled(PaneLiveness::OrphanReservation)
                | RecordCheck::SpawnInProgress => continue,
            };
            let handle = record.handle.clone().unwrap_or_default();
            let session = list
                .find(&handle)
                .and_then(|terminal| str_field(terminal, "worktreeId"))
                .unwrap_or_default()
                .to_owned();
            rows.push(PaneListing {
                raw: serde_json::to_string(&record)?,
                session,
                window_index_sort: 0,
                window_name: String::new(),
                pane_index_sort: 0,
                pane_id: handle.clone(),
                name: Some(record.name.clone()),
                agent: record.agent.clone(),
                access: record.access.clone(),
                launched_at: Some(record.launched_at.clone()),
                pane_dead: dead,
                pane_dead_status: None,
                last_activity: None,
                label: Some(handle),
                unknown,
                parent: record.parent.clone(),
                team: record.team.clone(),
                shim_id: record.shim_id.clone(),
            });
        }
        if !list.complete {
            eprintln!("list incomplete: orca");
        }
        Ok(rows)
    }

    fn describe_pane(&self, pane: Option<&str>) -> Result<PaneDescription> {
        let handle = match pane {
            Some(pane) => pane.to_owned(),
            None => self.calling_handle()?,
        };
        let shown = self.show(&handle)?;
        let registered = self.read_registration(&handle)?;
        let summary = PaneSummary {
            session: str_field(&shown, "worktreeId")
                .unwrap_or_default()
                .to_owned(),
            window_index: String::new(),
            window_name: str_field(&shown, "tabId").unwrap_or_default().to_owned(),
            pane_index: String::new(),
            pane_id: handle,
            name: registered.name,
            agent: registered.agent,
            access: registered.access,
            launched_at: registered.launched_at,
            cwd: registered.cwd,
        };
        Ok(PaneDescription {
            raw: format!("{}\n", serde_json::to_string(&shown)?),
            summary: Ok(summary),
        })
    }

    fn pane_exists(&self, pane: &str) -> Result<bool> {
        Ok(self.terminal_list()?.find(pane).is_some())
    }

    fn capture(&self, pane: &str, range: CaptureRange) -> Result<String> {
        match range {
            CaptureRange::Visible => self.screen(pane),
            CaptureRange::All => self.read_all(pane),
            CaptureRange::Tail(lines) => {
                let limit = lines.max(1).to_string();
                Ok(join_tail(&self.read(pane, &["--limit", &limit])?))
            }
            CaptureRange::Rows { .. } => {
                let screen = self.screen(pane)?;
                let last = screen
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default();
                Ok(format!("{last}\n"))
            }
        }
    }

    fn capture_ansi(&self, pane: &str, _start: Option<i64>, _end: Option<i64>) -> Result<String> {
        self.screen(pane)
    }

    fn capture_with_screen(&self, pane: &str, range: CaptureRange) -> Result<ScreenCapture> {
        // Orca reports no alternate-screen state (P9.3).
        Ok(ScreenCapture {
            alternate_on: false,
            text: self.capture(pane, range)?,
        })
    }

    fn poll_screen(&self, pane: &str) -> Result<ScreenCapture> {
        Ok(ScreenCapture {
            alternate_on: false,
            text: self.screen(pane)?,
        })
    }

    fn probe_history(&self, _pane: &str) -> Result<(HistoryProbe, String)> {
        bail!("orca reports no pane geometry; its History mark is a read cursor")
    }

    fn start_output_pipe(&self, _pane: &str, _command: &str) -> Result<()> {
        bail!("orca has no output pipe")
    }

    fn stop_output_pipe(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        let mut bytes = String::new();
        for key in keys {
            bytes.push_str(&keys::translate(key, "Orca")?);
        }
        self.send_text(pane, &bytes)
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let text_arg = format!("--text={text}");
        let result = self.call(&["terminal", "send", "--terminal", pane, &text_arg])?;
        let send = result.get("send").cloned().unwrap_or(Value::Null);
        if send.get("accepted").and_then(Value::as_bool) == Some(false) {
            bail!(
                "orca refused the input to {pane}: {}",
                str_field(&send, "refusedReason").unwrap_or("no reason given")
            );
        }
        Ok(())
    }

    fn bracketed_paste_enabled(&self, _pane: &str) -> Result<bool> {
        Ok(false)
    }

    fn paste_text(
        &self,
        _pane: &str,
        _text: &str,
        _confirm: &mut dyn FnMut() -> Result<bool>,
    ) -> Result<()> {
        bail!("orca delivers prompts with terminal send")
    }

    fn close_pane(&self, pane: &str, _retile_window: Option<&str>) -> Result<()> {
        let record = self.record_for_handle(pane)?;
        self.call(&["terminal", "close", "--terminal", pane])?;
        if let Some(record) = record {
            registry::delete(Host::Orca, &record.server_id, &record.name)?;
        }
        Ok(())
    }

    fn read_registration(&self, pane: &str) -> Result<Registered> {
        Ok(match self.record_for_handle(pane)? {
            Some(record) => record.registered(),
            None => Registered {
                name: None,
                agent: None,
                access: None,
                launched_at: None,
                cwd: None,
                surface: None,
                surface_unvalidated: false,
                ..Default::default()
            },
        })
    }

    fn read_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<Option<String>> {
        let registered = self.read_registration(pane)?;
        Ok(match key {
            RegistrationKey::Name => registered.name,
            RegistrationKey::Agent => registered.agent,
            RegistrationKey::Access => registered.access,
            RegistrationKey::LaunchedAt => registered.launched_at,
            RegistrationKey::Cwd => registered.cwd,
            RegistrationKey::Surface => registered.surface,
            RegistrationKey::SurfaceUnvalidated => {
                registered.surface_unvalidated.then(|| "1".to_owned())
            }
            RegistrationKey::Parent => registered.parent,
            RegistrationKey::Team => registered.team,
            RegistrationKey::ShimId => registered.shim_id,
            RegistrationKey::Title => registered.title,
            RegistrationKey::ClaudeAgentName => registered.claude_agent_name,
            RegistrationKey::ClaudeTeamName => registered.claude_team_name,
        })
    }

    fn write_registration_value(
        &self,
        pane: &str,
        key: RegistrationKey,
        value: &str,
    ) -> Result<()> {
        self.update_record(pane, key, Some(value))
    }

    fn clear_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<()> {
        self.update_record(pane, key, None)
    }

    fn pane_names(&self) -> Result<Vec<(String, String)>> {
        let (_, checked) = self.checked_records()?;
        Ok(checked
            .into_iter()
            .filter(|checked| checked.check == RecordCheck::Settled(PaneLiveness::Live))
            .filter_map(|checked| Some((checked.record.handle?, checked.record.name)))
            .collect())
    }

    fn window_registrations(&self, _target: &str) -> Result<Vec<Registered>> {
        Ok(Vec::new())
    }

    fn host_state(&self, pane: &str) -> Result<Option<HostState>> {
        Ok(self.host_state_since(pane)?.map(|(state, _)| state))
    }

    fn host_states(&self, panes: &[String]) -> Result<Vec<Option<HostState>>> {
        let keys = panes
            .iter()
            .map(|pane| self.pane_key(pane))
            .collect::<Result<Vec<_>>>()?;
        if keys.iter().all(Option::is_none) {
            return Ok(vec![None; panes.len()]);
        }
        // One `worktree ps` serves every row.
        let ps = self.call(&["worktree", "ps"])?;
        Ok(keys
            .iter()
            .map(|key| {
                key.as_deref()
                    .and_then(|key| ps_row(&ps, key))
                    .as_ref()
                    .and_then(row_host_state)
                    .map(|(state, _)| state)
            })
            .collect())
    }

    fn notify(&self, _pane: &str, _title: &str, _body: &str) -> Result<Delivery> {
        Ok(Delivery::NotDelivered)
    }

    fn apply_tiled_layout(&self, _target: &str) -> Result<()> {
        Err(unsupported("rebalance"))
    }

    fn window_geometry(&self, _target: &str) -> Result<WindowGeometry> {
        Err(unsupported("rebalance"))
    }

    fn window_identity(&self, _target: &str) -> Result<(String, u32)> {
        Err(unsupported("rebalance"))
    }

    fn read_layout_intent(&self, _target: &str) -> Result<bool> {
        Ok(false)
    }

    fn write_layout_intent(&self, _target: &str, _tiled: bool) -> Result<()> {
        Ok(())
    }

    fn session_of(&self, _target: &str) -> Result<String> {
        Err(unsupported("--session"))
    }

    fn session_windows(&self, _session: &str) -> Result<Vec<String>> {
        Err(unsupported("--session"))
    }

    fn join_pane(&self, _pane: &str, _window: &str) -> Result<()> {
        Err(unsupported("rebalance"))
    }

    fn break_pane(&self, _pane: &str, _session: &str) -> Result<()> {
        Err(unsupported("rebalance"))
    }

    // --- Pane records, prompts, Host state ---------------------------------------------

    fn uses_pane_records(&self) -> bool {
        true
    }

    fn shell_stays_after_command(&self) -> bool {
        true
    }

    fn pane_starts_in_caller_dir(&self) -> bool {
        false
    }

    fn is_pane_address(&self, value: &str) -> bool {
        crate::target::is_orca_handle(value)
    }

    fn find_registered(&self, name: &str) -> Result<Option<String>> {
        let (_, checked) = self.checked_records()?;
        for Checked { record, check } in checked {
            if record.name != name {
                continue;
            }
            return match check {
                RecordCheck::Settled(PaneLiveness::Live) => Ok(record.handle),
                RecordCheck::Settled(_) => Ok(None),
                RecordCheck::Unlisted => bail!("pane list incomplete: {name}"),
                RecordCheck::SpawnInProgress => bail!("spawn in progress: {name}"),
            };
        }
        Ok(None)
    }

    fn create_registered(
        &self,
        command: &str,
        split: Option<SplitDirection>,
        target: Option<&str>,
        registration: &Registration<'_>,
    ) -> Result<(String, String)> {
        // Every input the create needs is read before the name is reserved, so a
        // missing variable leaves no reservation behind.
        let placement = match split {
            None => {
                let worktree = match target {
                    Some(target) => str_field(&self.show(target)?, "worktreeId")
                        .ok_or_else(|| anyhow!("orca terminal show reports no worktreeId for {target}"))?
                        .to_owned(),
                    None => env_value(ORCA_WORKTREE_ID).ok_or_else(|| {
                        anyhow!("{ORCA_WORKTREE_ID} is unset: a new Orca tab needs the calling worktree; pass --target or run inside an Orca terminal")
                    })?,
                };
                Placement::Tab { worktree }
            }
            Some(direction) => Placement::Split {
                handle: match target {
                    Some(target) => target.to_owned(),
                    None => self.calling_handle()?,
                },
                direction: match direction {
                    SplitDirection::Horizontal => "horizontal",
                    SplitDirection::Vertical => "vertical",
                },
            },
        };

        let mut record = self.reserve(registration)?;
        let name = record.name.clone();
        let release = |record: &PaneRecord| {
            let _ = registry::delete(Host::Orca, &record.server_id, &record.name);
        };

        let created = match &placement {
            Placement::Tab { worktree } => {
                let selector = format!("id:{worktree}");
                self.call(&[
                    "terminal",
                    "create",
                    "--worktree",
                    &selector,
                    "--title",
                    &name,
                    "--command",
                    command,
                ])
            }
            Placement::Split { handle, direction } => self.call(&[
                "terminal",
                "split",
                "--terminal",
                handle,
                "--direction",
                direction,
                "--command",
                command,
            ]),
        };

        let handle = match created {
            Ok(result) => find_key(&result, "handle")
                .and_then(Value::as_str)
                .map(str::to_owned),
            Err(error) => {
                let Some(orca_error) = error.downcast_ref::<OrcaError>() else {
                    // The call could not complete: the pane may exist.
                    return Err(self.outcome_unknown(&name, &placement, &error));
                };
                let leaf = find_key(&orca_error.data, "leafId")
                    .or_else(|| find_key(&orca_error.data, "newLeafId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                match (&placement, leaf) {
                    (Placement::Split { handle, direction }, Some(leaf)) => {
                        // P4.4: recover that same pane; creation is never repeated.
                        match self.call(&[
                            "terminal",
                            "split",
                            "--terminal",
                            handle,
                            "--direction",
                            direction,
                            "--command",
                            command,
                            "--retry-split",
                            &leaf,
                        ]) {
                            Ok(result) => find_key(&result, "handle")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            Err(retry_error) => {
                                return Err(self.outcome_unknown(&name, &placement, &retry_error));
                            }
                        }
                    }
                    _ if orca_error.code.contains("timeout") => {
                        return Err(self.outcome_unknown(&name, &placement, &error));
                    }
                    _ => {
                        release(&record);
                        return Err(error);
                    }
                }
            }
        };
        let Some(handle) = handle else {
            return Err(self.outcome_unknown(
                &name,
                &placement,
                &anyhow!("orca reported no handle for the new pane"),
            ));
        };

        record.handle = Some(handle.clone());
        if let Err(error) = registry::write(&record) {
            // P4.4: the reservation is released only once the pane is closed.
            return match self.call(&["terminal", "close", "--terminal", &handle]) {
                Ok(_) => {
                    release(&record);
                    Err(error.context(format!(
                        "spawn {name}: could not record pane {handle}, so it was closed"
                    )))
                }
                Err(close_error) => Err(anyhow!(
                    "spawn {name}: could not record pane {handle} ({error:#}) and could not \
                     close it ({close_error:#}); pane {handle} stays open and unregistered, and \
                     the name stays reserved until it becomes an orphan reservation"
                )),
            };
        }
        Ok((handle, name))
    }

    fn history_cursor(&self, pane: &str) -> Result<String> {
        let read = self.read(pane, &["--limit", "1"])?;
        cursor_field(&read, "nextCursor")
            .or_else(|| cursor_field(&read, "latestCursor"))
            .ok_or_else(|| anyhow!("orca reports no read cursor for {pane}"))
    }

    fn read_history_since(&self, pane: &str, cursor: &str) -> Result<String> {
        // A full-screen program keeps no line history: Orca then serves its rendered
        // screen, and the result is that screen (as on tmux's alternate screen).
        let preview = self.read(pane, &["--limit", "1"])?;
        if str_field(&preview, "source") == Some("screen") {
            return self.screen(pane);
        }
        self.read_pages(pane, cursor)
    }

    fn prompt_delivery_guaranteed(&self, pane: &str, agent: Option<&str>) -> Result<bool> {
        if !self
            .status()?
            .capabilities
            .iter()
            .any(|capability| capability == PROMPT_DELIVERY_CAPABILITY)
        {
            bail!(
                "refusing to prompt pane {pane}: this Orca runtime lacks the capability \
                 {PROMPT_DELIVERY_CAPABILITY}, so it cannot deliver a prompt as one submission"
            );
        }
        let Some(agent) = agent else {
            return Ok(false);
        };
        let identity_matches = str_field(&self.show(pane)?, "agentIdentity") == Some(agent);
        if !identity_matches {
            return Ok(false);
        }
        let row_live = self.agent_row(pane)?.is_some_and(|row| {
            row.get("hasLivePty").and_then(Value::as_bool) == Some(true)
                && row_host_state(&row).is_some()
        });
        Ok(row_live)
    }

    fn submit_prompt(&self, pane: &str, text: &str, wait: Duration) -> Result<Submission> {
        let seconds = (wait.as_secs_f64().ceil() as u64).clamp(1, 3600);
        let wait_arg = seconds.to_string();
        let text_arg = format!("--text={text}");
        let base = [
            "terminal",
            "send",
            "--terminal",
            pane,
            text_arg.as_str(),
            "--enter",
            "--wait-submit",
            wait_arg.as_str(),
        ];
        let timeout = CALL_TIMEOUT + Duration::from_secs(seconds);
        let result = match self.call_with_timeout(&base, timeout) {
            Ok(result) => result,
            Err(error) => {
                let Some(orca_error) = error.downcast_ref::<OrcaError>() else {
                    return Err(error);
                };
                if orca_error.code == "agent_prompt_blocked" {
                    bail!(
                        "prompt blocked: agent_prompt_blocked: {}",
                        orca_error.message
                    );
                }
                // An ambiguous transport failure carries the request ID to retry with,
                // once; nothing is resent otherwise.
                let Some(request) = find_key(&orca_error.data, "retryRequestId")
                    .or_else(|| find_key(&orca_error.data, "requestId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    return Err(error);
                };
                let mut retry: Vec<&str> = base.to_vec();
                retry.extend_from_slice(&["--retry-request", &request]);
                match self.call_with_timeout(&retry, timeout) {
                    Ok(result) => result,
                    Err(retry_error) => {
                        if let Some(orca_error) = retry_error.downcast_ref::<OrcaError>() {
                            if orca_error.code == "agent_prompt_blocked" {
                                bail!(
                                    "prompt blocked: agent_prompt_blocked: {}",
                                    orca_error.message
                                );
                            }
                        }
                        return Err(retry_error);
                    }
                }
            }
        };
        let send = result.get("send").cloned().unwrap_or(Value::Null);
        if send.get("accepted").and_then(Value::as_bool) == Some(false) {
            bail!(
                "prompt refused: {}",
                str_field(&send, "refusedReason").unwrap_or("accepted: false")
            );
        }
        let prompt = send.get("prompt").filter(|prompt| prompt.is_object());
        match prompt.and_then(|prompt| str_field(prompt, "observation")) {
            Some("permission") => bail!(
                "prompt blocked: permission: the agent shows a permission prompt; resolve it in the pane"
            ),
            Some("incarnation_replaced") => bail!("prompt target restarted"),
            _ => {}
        }
        Ok(receipt_submission(prompt))
    }

    fn host_state_since(&self, pane: &str) -> Result<Option<(HostState, Option<u64>)>> {
        Ok(self.agent_row(pane)?.as_ref().and_then(row_host_state))
    }
}

/// Where a spawn opens its pane on Orca.
enum Placement {
    Tab {
        worktree: String,
    },
    Split {
        handle: String,
        direction: &'static str,
    },
}

impl OrcaHost {
    /// Rewrite one field of the Pane record of `pane` by write-to-temp and atomic
    /// rename. A new `Name` moves the record to that name's file.
    fn update_record(&self, pane: &str, key: RegistrationKey, value: Option<&str>) -> Result<()> {
        let mut record = self
            .record_for_handle(pane)?
            .ok_or_else(|| anyhow!("pane {pane} has no Pane record"))?;
        let owned = value.map(str::to_owned);
        let old_name = record.name.clone();
        match key {
            RegistrationKey::Name => {
                record.name = owned.ok_or_else(|| anyhow!("a Pane record needs a name"))?
            }
            RegistrationKey::Agent => record.agent = owned,
            RegistrationKey::Access => record.access = owned,
            RegistrationKey::LaunchedAt => record.launched_at = owned.unwrap_or_default(),
            RegistrationKey::Cwd => record.cwd = owned.unwrap_or_default().into(),
            RegistrationKey::Surface => record.surface = owned,
            RegistrationKey::SurfaceUnvalidated => {
                record.surface_unvalidated =
                    owned.is_some_and(|value| value == "1" || value == "true")
            }
            RegistrationKey::Parent => record.parent = owned,
            RegistrationKey::Team => record.team = owned,
            RegistrationKey::ShimId => record.shim_id = owned,
            RegistrationKey::Title => record.title = owned,
            RegistrationKey::ClaudeAgentName => record.claude_agent_name = owned,
            RegistrationKey::ClaudeTeamName => record.claude_team_name = owned,
        }
        if record.name != old_name {
            // The new name is reserved exclusively first, so it never replaces another
            // pane's record.
            registry::reserve(&record)?;
            registry::write(&record)?;
            return registry::delete(Host::Orca, &record.server_id, &old_name);
        }
        registry::write(&record)
    }

    /// P9.2: an ambiguous create or split. The reservation stays until it becomes an
    /// orphan reservation, so the name is not reused meanwhile.
    fn outcome_unknown(
        &self,
        name: &str,
        placement: &Placement,
        cause: &anyhow::Error,
    ) -> anyhow::Error {
        let worktree = match placement {
            Placement::Tab { worktree } => worktree.clone(),
            Placement::Split { handle, .. } => self
                .show(handle)
                .ok()
                .and_then(|shown| str_field(&shown, "worktreeId").map(str::to_owned))
                .unwrap_or_else(|| format!("the worktree of {handle}")),
        };
        anyhow!(
            "spawn outcome unknown: {name}: inspect worktree {worktree} with `orca terminal list`; \
             the name stays reserved for {}s ({cause:#})",
            registry::SPAWN_TIMEOUT.as_secs()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_state_follows_the_p9_10_mapping() {
        let row = |state: &str| json!({"paneKey": "t:l", "hasLivePty": true, "state": state, "stateStartedAt": 5});
        assert_eq!(
            row_host_state(&row("working")),
            Some((HostState::Working, Some(5)))
        );
        assert_eq!(
            row_host_state(&row("blocked")),
            Some((HostState::Blocked, Some(5)))
        );
        assert_eq!(
            row_host_state(&row("waiting")),
            Some((HostState::Blocked, Some(5)))
        );
        assert_eq!(
            row_host_state(&row("done")),
            Some((HostState::Done, Some(5)))
        );
        assert_eq!(
            row_host_state(&json!({"hasLivePty": false, "state": "working"})),
            None
        );
        assert_eq!(
            row_host_state(
                &json!({"hasLivePty": true, "state": "done", "stateStartedAt": 1,
                "subagents": [{"state": "working"}]})
            ),
            Some((HostState::Working, Some(1)))
        );
    }

    #[test]
    fn submit_follows_the_p9_11_mapping() {
        assert_eq!(
            receipt_submission(Some(
                &json!({"provider": "claude", "stages": ["input_accepted", "turn_started"]})
            )),
            Submission::Confirmed
        );
        assert_eq!(
            receipt_submission(Some(
                &json!({"provider": "claude", "stages": ["input_accepted"]})
            )),
            Submission::Accepted
        );
        assert_eq!(
            receipt_submission(Some(
                &json!({"provider": "unsupported", "stages": ["input_accepted", "turn_started"]})
            )),
            Submission::Assumed
        );
        assert_eq!(
            receipt_submission(Some(
                &json!({"provider": "x", "observation": "unsupported", "stages": []})
            )),
            Submission::Assumed
        );
        assert_eq!(receipt_submission(None), Submission::Assumed);
    }
}
