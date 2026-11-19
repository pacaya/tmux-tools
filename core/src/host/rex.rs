//! The Rex Host backend: every `rex` CLI invocation `tmux-tools` runs is built here
//! (PRD-261112-2108-01).
//!
//! A pane is a Rex block, addressed by its block id `block:<uuid>`. The calling pane is
//! the block named by `REX_BLOCK`. Every call passes `--autostart=false`, and `-S
//! "$REX_SERVER"` when `REX_SERVER` is set (P2.3). Rex cannot store custom data on a
//! block, so the Pane registration is a Pane record ([`crate::registry`]) under the
//! server's `instance_id`, in a server directory published atomically with its
//! `server-address` (P4.1).

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::host::history_mark;
use crate::host::orca::keys;
use crate::host::{
    CaptureRange, Delivery, HistoryProbe, HistorySince, Host, HostCapabilities, HostPort,
    HostState, ListScope, PaneDescription, PaneListing, PaneSummary, RegistrationKey,
    ScreenCapture, SelectionSource, SplitDirection, Submission, Unsupported, WindowHome,
};
use crate::layout::{Registration, WindowGeometry};
use crate::names::{self, Registered};
use crate::registry::{self, NameTaken, PaneLiveness, PaneRecord, RecordCheck};

/// The calling block's id, set by Rex inside its blocks.
pub const REX_BLOCK: &str = "REX_BLOCK";
/// The calling block's session id, set by Rex inside its blocks.
pub const REX_SESSION: &str = "REX_SESSION";
/// The address of the Rex server, passed with `-S` (P2.3).
pub const REX_SERVER: &str = "REX_SERVER";
/// The session of a new block when no `--target` and no `REX_SESSION` give one (P3.1).
pub const TMUX_TOOLS_REX_SESSION: &str = "TMUX_TOOLS_REX_SESSION";

/// A Rex label holds at most 128 characters (P3.4).
const LABEL_CHARS: usize = 128;
/// The lowest `api_revision` with which `prompt` works (P6.5).
const PROMPT_API_REVISION: u64 = 12;
/// How long the address check of another server directory may run (P4.1).
const ADDRESS_CHECK_TIMEOUT: Duration = Duration::from_secs(2);
/// The P4.1 address check of another server directory.
const ADDRESS_CHECK_ARGS: [&str; 3] = ["server", "status", "--json"];
/// How long one `rex` call may run.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// The file of a server directory that holds its server address (P4.1).
const SERVER_ADDRESS_FILE: &str = "server-address";
/// The `server-address` value when `REX_SERVER` is not set.
const UNSET_ADDRESS: &str = "unset";
/// The name prefix of a server directory that is being published.
const INIT_PREFIX: &str = ".init-";
/// How many fresh derivations a spawn tries when a derived name is already reserved.
const DERIVED_NAME_ATTEMPTS: usize = 16;

/// A `rex` command that ran and exited non-zero.
#[derive(Debug)]
pub struct RexError {
    pub command: String,
    pub code: Option<i32>,
    pub message: String,
}

impl std::fmt::Display for RexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "rex {} exited {}: {}",
            self.command,
            self.code.unwrap_or(-1),
            self.message
        )
    }
}

impl std::error::Error for RexError {}

/// A `rex` call that timed out or answered output that does not parse: it ran, but
/// gave no usable answer (P4.3 counts it as an unsuccessful listing call).
#[derive(Debug)]
struct NoAnswer(String);

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NoAnswer {}

impl RexError {
    /// Rex reports `not found` with exit 3.
    fn not_found(&self) -> bool {
        self.code == Some(3)
    }
}

/// Which server a call reaches.
#[derive(Clone, Copy)]
enum Server<'a> {
    /// The caller's: `-S "$REX_SERVER"` when it is set (P2.3).
    Caller,
    /// A server directory's stored address (P4.1): `unset` reaches the default server
    /// without `-S` and with `REX_SERVER` removed from the call's environment.
    Stored(&'a str),
}

/// What one `rex` run gave.
enum Ran {
    Exited {
        code: Option<i32>,
        stdout: Vec<u8>,
        stderr: String,
    },
    TimedOut,
}

/// `rex server status --json` of the caller's server.
#[derive(Clone, Debug)]
struct Status {
    instance_id: String,
    api_revision: u64,
}

/// One block of a listing, with its session and window.
#[derive(Clone, Debug)]
struct Block {
    block_id: String,
    label: String,
    session_id: String,
    window_id: Option<String>,
    raw: Value,
}

/// The blocks of every session (P4.3).
struct Listing {
    blocks: Vec<Block>,
    /// `rex ls --json` and `rex block ls -s <session> --json` for every listed session
    /// all succeeded.
    complete: bool,
}

impl Listing {
    fn handles(&self) -> HashSet<String> {
        self.blocks
            .iter()
            .map(|block| block.block_id.clone())
            .collect()
    }

    fn find(&self, block: &str) -> Option<&Block> {
        self.blocks
            .iter()
            .find(|candidate| candidate.block_id == block)
    }
}

/// One record after the P4.3 check.
struct Checked {
    record: PaneRecord,
    check: RecordCheck,
}

/// One `program_status` record: its Host state (P6.2), raw state word, and time.
#[derive(Clone, Debug)]
struct StatusRecord {
    state: String,
    updated_ms: Option<u64>,
    /// `updated_at` at full precision, in nanoseconds since the Unix epoch.
    updated_ns: Option<u128>,
}

impl StatusRecord {
    fn host_state(&self) -> Option<HostState> {
        match self.state.as_str() {
            "idle" => Some(HostState::Idle),
            "working" => Some(HostState::Working),
            "blocked" | "error" => Some(HostState::Blocked),
            "done" => Some(HostState::Done),
            _ => None,
        }
    }
}

/// The History mark (P5.2): the full capture taken before submission, with the
/// alternate-screen state and the `size` width read with it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Mark {
    lines: Vec<String>,
    alternate: bool,
    columns: u64,
}

/// A prompt sent by [`HostPort::submit_prompt`], judged against the `program_status`
/// records read after the send (P6.6, P6.7).
struct Pending {
    pane: String,
    /// The time before the send, in milliseconds since the Unix epoch.
    sent_ms: u64,
    /// The Host state before the send was not `working`.
    baseline_not_working: bool,
    confirmed: bool,
    turn_observed: bool,
}

impl Pending {
    fn observe(&mut self, records: &[StatusRecord]) {
        for record in records {
            if !record.updated_ms.is_some_and(|ms| ms > self.sent_ms) {
                continue;
            }
            self.turn_observed = true;
            if self.baseline_not_working
                && matches!(
                    record.host_state(),
                    Some(HostState::Working | HostState::Done | HostState::Blocked)
                )
            {
                self.confirmed = true;
            }
        }
    }
}

/// The Rex Host backend.
pub struct RexHost {
    source: SelectionSource,
    status: OnceLock<std::result::Result<Status, String>>,
    /// The P4.1 sweep, run once per invocation before the first registry read.
    swept: OnceLock<std::result::Result<(), String>>,
    pending: Mutex<Option<Pending>>,
}

impl RexHost {
    /// The backend for an invocation that selected Rex through `source`.
    pub fn new(source: SelectionSource) -> RexHost {
        RexHost {
            source,
            status: OnceLock::new(),
            swept: OnceLock::new(),
            pending: Mutex::new(None),
        }
    }

    // --- The CLI -------------------------------------------------------------------

    /// Run `rex --autostart=false [-S <address>] <args>`. Not reaching the CLI names the
    /// host and the selection source (P2.4). A `timeout` of `None` sets no limit
    /// (only a `test-fixtures` build gives one; see [`limit`]).
    fn exec(&self, server: Server<'_>, args: &[&str], timeout: Option<Duration>) -> Result<Ran> {
        let mut command = Command::new("rex");
        command.arg("--autostart=false");
        match server {
            Server::Caller => {
                if let Some(address) = env_value(REX_SERVER) {
                    command.args(["-S", &address]);
                }
            }
            Server::Stored(UNSET_ADDRESS) => {
                command.env_remove(REX_SERVER);
            }
            Server::Stored(address) => {
                command.args(["-S", address]);
            }
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| self.unreachable(&format!("cannot run the rex CLI: {error}")))?;
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
        let deadline = timeout.map(|timeout| Instant::now() + timeout);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let stdout = out_reader.join().unwrap_or_default();
        let stderr = err_reader.join().unwrap_or_default();
        Ok(match status {
            Some(status) => Ran::Exited {
                code: status.code(),
                stdout,
                stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
            },
            None => Ran::TimedOut,
        })
    }

    /// Run `rex <args>` against the caller's server; its stdout, or a [`RexError`].
    fn call(&self, args: &[&str]) -> Result<Vec<u8>> {
        let name = command_name(args);
        let timeout = limit(Server::Caller, args, CALL_TIMEOUT);
        match self.exec(Server::Caller, args, timeout)? {
            Ran::Exited {
                code: Some(0),
                stdout,
                ..
            } => Ok(stdout),
            Ran::Exited { code, stderr, .. } => Err(RexError {
                command: name,
                code,
                message: stderr,
            }
            .into()),
            Ran::TimedOut => Err(NoAnswer(format!(
                "rex {name}: no answer within {}s",
                timeout.unwrap_or_default().as_secs_f64()
            ))
            .into()),
        }
    }

    fn call_json(&self, args: &[&str]) -> Result<Value> {
        let stdout = self.call(args)?;
        serde_json::from_slice(&stdout).map_err(|error| {
            NoAnswer(format!(
                "rex {} answered no JSON ({error}): {}",
                command_name(args),
                String::from_utf8_lossy(&stdout).trim()
            ))
            .into()
        })
    }

    fn unreachable(&self, detail: &str) -> anyhow::Error {
        anyhow!(
            "host rex (selected by {}) is unreachable: {detail}",
            self.source
        )
    }

    /// `rex server status --json`, read once per invocation (P2.4–P2.5).
    fn status(&self) -> Result<&Status> {
        let status = self.status.get_or_init(|| {
            let answer = self
                .call_json(&["server", "status", "--json"])
                .map_err(|error| format!("{}", self.unreachable(&format!("{error:#}"))))?;
            let instance_id = str_field(&answer, "instance_id")
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    format!(
                        "{}",
                        self.unreachable("rex server status reports no instance_id")
                    )
                })?
                .to_owned();
            Ok(Status {
                instance_id,
                api_revision: answer
                    .get("api_revision")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            })
        });
        status.as_ref().map_err(|message| anyhow!("{message}"))
    }

    fn calling_block(&self) -> Result<String> {
        // PRD-261108-0447-01 P3.3: inside a team the calling pane is the Leader.
        if let Some(leader) = crate::team::leader_handle() {
            return Ok(leader);
        }
        env_value(REX_BLOCK).ok_or_else(|| {
            anyhow!("{REX_BLOCK} is unset: this verb needs it to name the calling Rex block")
        })
    }

    // --- Listing ---------------------------------------------------------------------

    /// The blocks of every session, and whether every listing call succeeded (P4.3).
    /// A call that exits non-zero, times out, or answers no usable listing is
    /// unsuccessful: the listing is incomplete.
    fn listing(&self) -> Result<Listing> {
        let unsuccessful = |error: &anyhow::Error| {
            self.is_command_failure(error) || error.downcast_ref::<NoAnswer>().is_some()
        };
        let sessions = match self.call_json(&["ls", "--json"]).and_then(|answer| {
            answer
                .get("sessions")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| NoAnswer("rex ls answered no sessions".to_owned()).into())
        }) {
            Ok(sessions) => sessions,
            Err(error) if unsuccessful(&error) => {
                return Ok(Listing {
                    blocks: Vec::new(),
                    complete: false,
                })
            }
            Err(error) => return Err(error),
        };
        let mut blocks = Vec::new();
        let mut complete = true;
        for session in &sessions {
            let Some(session_id) = str_field(session, "session_id") else {
                continue;
            };
            match self.session_blocks(session_id) {
                Ok(listed) => blocks.extend(listed),
                Err(error) if unsuccessful(&error) => complete = false,
                Err(error) => return Err(error),
            }
        }
        Ok(Listing { blocks, complete })
    }

    /// `rex block ls -s <session> --json`.
    fn session_blocks(&self, session: &str) -> Result<Vec<Block>> {
        let answer = self.call_json(&["block", "ls", "-s", session, "--json"])?;
        let listed = answer
            .get("blocks")
            .and_then(Value::as_array)
            .ok_or_else(|| NoAnswer(format!("rex block ls -s {session} answered no blocks")))?;
        Ok(listed
            .iter()
            .filter_map(|block| {
                Some(Block {
                    block_id: str_field(block, "block_id")?.to_owned(),
                    label: str_field(block, "label").unwrap_or_default().to_owned(),
                    session_id: session.to_owned(),
                    window_id: str_field(block, "window_id").map(str::to_owned),
                    raw: block.clone(),
                })
            })
            .collect())
    }

    /// The session of `block`, from a listing; an error when no session lists it.
    fn session_of_block(&self, block: &str) -> Result<String> {
        self.listing()?
            .find(block)
            .map(|block| block.session_id.clone())
            .ok_or_else(|| anyhow!("rex lists no block {block}"))
    }

    // --- Block calls ------------------------------------------------------------------

    fn block_call(&self, block: &str, method: &str) -> Result<Value> {
        self.call_json(&["block", "call", "-b", block, method])
    }

    /// `rows` and `columns` of `rex block call -b <block> size`.
    fn size(&self, block: &str) -> Result<(usize, u64)> {
        let size = self.block_call(block, "size")?;
        Ok((
            size.get("rows").and_then(Value::as_u64).unwrap_or(0) as usize,
            size.get("columns").and_then(Value::as_u64).unwrap_or(0),
        ))
    }

    /// `rex block call -b <block> modes`: whether the alternate screen is on, and the
    /// set modes by number.
    fn modes(&self, block: &str) -> Result<(bool, HashSet<u64>)> {
        let modes = self.block_call(block, "modes")?;
        let alternate = str_field(&modes, "screen").is_some_and(|screen| screen != "primary");
        let set = modes
            .get("modes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|mode| mode.get("set").and_then(Value::as_bool) == Some(true))
            .filter_map(|mode| mode.get("number").and_then(Value::as_u64))
            .collect();
        Ok((alternate, set))
    }

    /// Every `program_status` record of `block`. A read made while a prompt is pending
    /// is also its submission observation.
    fn status_records(&self, block: &str) -> Result<Vec<StatusRecord>> {
        let answer = self.block_call(block, "program_status")?;
        let records: Vec<StatusRecord> = answer
            .get("records")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|record| {
                let updated_ns = str_field(record, "updated_at").and_then(rfc3339_ns);
                Some(StatusRecord {
                    state: str_field(record, "state")?.to_owned(),
                    updated_ms: updated_ns.map(|ns| (ns / 1_000_000) as u64),
                    updated_ns,
                })
            })
            .collect();
        if let Some(pending) = self.pending.lock().expect("pending prompt").as_mut() {
            if pending.pane == block {
                pending.observe(&records);
            }
        }
        Ok(records)
    }

    /// P6.2: the record with the newest `updated_at`, compared at full precision.
    fn newest_record(&self, block: &str) -> Result<Option<StatusRecord>> {
        Ok(self
            .status_records(block)?
            .into_iter()
            .max_by_key(|record| record.updated_ns.unwrap_or(0)))
    }

    /// `rex capture -b <block>` in text format, as lines.
    fn capture_lines(&self, block: &str) -> Result<Vec<String>> {
        let stdout = self.call(&["capture", "-b", block])?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// Send `text` to `block` through `rex send` (P5.3).
    fn send_bytes(&self, block: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.call(&["send", "-b", block, "--", text])?;
        Ok(())
    }

    // --- The registry -------------------------------------------------------------

    /// The registry directory of this server, after the P4.1 sweep of the others.
    fn server_dir(&self) -> Result<(String, PathBuf)> {
        let instance_id = self.status()?.instance_id.clone();
        let swept = self.swept.get_or_init(|| {
            self.sweep(&instance_id)
                .map_err(|error| format!("{error:#}"))
        });
        swept.as_ref().map_err(|message| anyhow!("{message}"))?;
        Ok((
            instance_id.clone(),
            registry::server_dir(Host::Rex, &instance_id)?,
        ))
    }

    /// P4.1: delete a stale `.init-` directory, and every other server directory whose
    /// stored address fails, times out, or reports a different `instance_id`, or that
    /// has no readable `server-address`.
    fn sweep(&self, instance_id: &str) -> Result<()> {
        let dir = registry::host_dir(Host::Rex)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if name.starts_with('.') {
                if name.starts_with(INIT_PREFIX) && age(&path) >= registry::SPAWN_TIMEOUT {
                    remove_dir(&path)?;
                }
                continue;
            }
            if name == instance_id {
                continue;
            }
            let reaches = match std::fs::read_to_string(path.join(SERVER_ADDRESS_FILE)) {
                Ok(address) => self.address_reaches(address.trim(), &name),
                Err(_) => false,
            };
            if !reaches {
                remove_dir(&path)?;
            }
        }
        Ok(())
    }

    /// Whether one `rex server status --json` against `address` answers within 2 s
    /// with `instance_id` (P4.1).
    fn address_reaches(&self, address: &str, instance_id: &str) -> bool {
        match self.exec(
            Server::Stored(address),
            &ADDRESS_CHECK_ARGS,
            address_check_limit(address),
        ) {
            Ok(Ran::Exited {
                code: Some(0),
                stdout,
                ..
            }) => serde_json::from_slice::<Value>(&stdout)
                .ok()
                .is_some_and(|status| str_field(&status, "instance_id") == Some(instance_id)),
            _ => false,
        }
    }

    /// P4.1: publish this server's directory atomically when it is missing: make
    /// `rex/.init-<random>/`, write the complete `server-address` in it, and rename it
    /// to `rex/<instance_id>/`; when the target already exists, delete the temporary
    /// directory and use the target.
    fn publish(&self) -> Result<String> {
        let (instance_id, target) = self.server_dir()?;
        if target.is_dir() {
            return Ok(instance_id);
        }
        let parent = registry::host_dir(Host::Rex)?;
        std::fs::create_dir_all(&parent).with_context(|| format!("create {}", parent.display()))?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let temp = parent.join(format!("{INIT_PREFIX}{}-{nanos}", std::process::id()));
        std::fs::create_dir(&temp).with_context(|| format!("create {}", temp.display()))?;
        let address = env_value(REX_SERVER).unwrap_or_else(|| UNSET_ADDRESS.to_owned());
        if let Err(error) = std::fs::write(temp.join(SERVER_ADDRESS_FILE), &address) {
            let _ = std::fs::remove_dir_all(&temp);
            return Err(error).with_context(|| format!("write {}", temp.display()));
        }
        #[cfg(feature = "test-fixtures")]
        test_hook("before-rename");
        match std::fs::rename(&temp, &target) {
            Ok(()) => Ok(instance_id),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&temp);
                if target.is_dir() {
                    Ok(instance_id)
                } else {
                    Err(error).with_context(|| format!("publish {}", target.display()))
                }
            }
        }
    }

    /// Every record of this server after the P4.3 check against `listing`. Dead
    /// records and orphan reservations are deleted; they stay in the answer once.
    fn checked_records(&self) -> Result<(Listing, Vec<Checked>)> {
        let (instance_id, _) = self.server_dir()?;
        let records = registry::load_all(Host::Rex, &instance_id)?;
        let listing = self.listing()?;
        let listed = listing.handles();
        let mut checked = Vec::new();
        for (record, age) in records {
            let check = registry::check(&record, &listed, listing.complete, age);
            if matches!(
                check,
                RecordCheck::Settled(PaneLiveness::Dead | PaneLiveness::OrphanReservation)
            ) {
                match registry::load(Host::Rex, &instance_id, &record.name)? {
                    Some(current) if current == record => {
                        registry::delete(Host::Rex, &instance_id, &record.name)?;
                    }
                    Some(_) => continue,
                    None => {}
                }
            }
            checked.push(Checked { record, check });
        }
        Ok((listing, checked))
    }

    /// The record whose handle is `block`, without a listing check.
    fn record_for_block(&self, block: &str) -> Result<Option<PaneRecord>> {
        let (instance_id, _) = self.server_dir()?;
        Ok(registry::load_all(Host::Rex, &instance_id)?
            .into_iter()
            .map(|(record, _)| record)
            .find(|record| record.handle.as_deref() == Some(block)))
    }

    fn new_record(
        &self,
        instance_id: &str,
        name: &str,
        registration: &Registration<'_>,
    ) -> PaneRecord {
        let agent = registration.agent.as_ref();
        let team = registration.team.as_ref();
        let owned = |value: Option<&str>| value.map(str::to_owned);
        PaneRecord {
            schema: registry::PANE_RECORD_SCHEMA.to_owned(),
            name: name.to_owned(),
            host: Host::Rex,
            server_id: instance_id.to_owned(),
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
        }
    }

    /// Reserve `record.name` in the published server directory.
    fn reserve_record(&self, record: &PaneRecord) -> Result<()> {
        self.publish()?;
        registry::reserve(record)
    }

    /// Write `record` in the published server directory.
    fn write_record(&self, record: &PaneRecord) -> Result<()> {
        self.publish()?;
        registry::write(record)
    }

    /// Reserve the explicit name, or a fresh derived one.
    fn reserve(&self, registration: &Registration<'_>) -> Result<PaneRecord> {
        let instance_id = self.status()?.instance_id.clone();
        if let Some(name) = registration.name {
            let record = self.new_record(&instance_id, name, registration);
            self.reserve_record(&record)?;
            return Ok(record);
        }
        for _ in 0..DERIVED_NAME_ATTEMPTS {
            let name = names::derive_name(registration.name_source);
            let record = self.new_record(&instance_id, &name, registration);
            match self.reserve_record(&record) {
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

    /// P4.4: an ambiguous create. List the blocks of `session` once, and adopt the one
    /// block whose label is the name's first 128 characters when no live record holds
    /// it and no other record or reservation shares those 128 characters.
    fn recover(&self, record: &PaneRecord, session: &str) -> Result<String> {
        let unknown = || anyhow!("spawn outcome unknown: {} (session {session})", record.name);
        let Ok(blocks) = self.session_blocks(session) else {
            return Err(unknown());
        };
        let label = label_of(&record.name);
        let candidates: Vec<&Block> = blocks.iter().filter(|block| block.label == label).collect();
        let [candidate] = candidates.as_slice() else {
            return Err(unknown());
        };
        let records = registry::load_all(Host::Rex, &record.server_id)?;
        let held = records
            .iter()
            .any(|(other, _)| other.handle.as_deref() == Some(candidate.block_id.as_str()));
        let shared = records
            .iter()
            .any(|(other, _)| other.name != record.name && label_of(&other.name) == label);
        if held || shared {
            return Err(unknown());
        }
        Ok(candidate.block_id.clone())
    }

    /// Rewrite one field of the Pane record of `block` by write-to-temp and atomic
    /// rename. A new `Name` moves the record to that name's file.
    fn update_record(&self, block: &str, key: RegistrationKey, value: Option<&str>) -> Result<()> {
        let mut record = self
            .record_for_block(block)?
            .ok_or_else(|| anyhow!("block {block} has no Pane record"))?;
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
            self.reserve_record(&record)?;
            self.write_record(&record)?;
            registry::delete(Host::Rex, &record.server_id, &old_name)?;
            // The block label follows the pane name (P3.4). It is for display only, so
            // a failed rename after the record moved is a warning, not a failure.
            if let (Some(label), Some(handle)) =
                (relabel(&old_name, &record.name), record.handle.as_deref())
            {
                if let Err(error) = self.rename_block(handle, &label) {
                    let detail = format!("{error:#}").replace('\n', " ");
                    eprintln!("rex: block label not updated: {handle}: {detail}");
                }
            }
            return Ok(());
        }
        self.write_record(&record)
    }

    /// `rex block rename <block> -- <label>`: `--` keeps a label that starts with `-`
    /// from being read as a flag. A block already gone counts as renamed.
    fn rename_block(&self, block: &str, label: &str) -> Result<()> {
        match self.call(&["block", "rename", block, "--", label]) {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .downcast_ref::<RexError>()
                    .is_some_and(RexError::not_found) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// `rex block close <block>`; a block already gone counts as closed (P7.4).
    fn close_block(&self, block: &str) -> Result<()> {
        match self.call(&["block", "close", block]) {
            Ok(_) => Ok(()),
            Err(error)
                if error
                    .downcast_ref::<RexError>()
                    .is_some_and(RexError::not_found) =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

/// The command words of `args`, before the first flag.
fn command_name(args: &[&str]) -> String {
    args.iter()
        .take_while(|arg| !arg.starts_with('-'))
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The new block label when a pane is renamed from `old` to `new`; `None` when the
/// first 128 characters, and so the label, stay the same.
fn relabel(old: &str, new: &str) -> Option<String> {
    let label = label_of(new);
    (label != label_of(old)).then_some(label)
}

/// The block label of a pane name: its first 128 characters (P3.4).
fn label_of(name: &str) -> String {
    name.chars().take(LABEL_CHARS).collect()
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The time limit of one `rex` call whose production limit is `production`
/// ([`CALL_TIMEOUT`], or [`ADDRESS_CHECK_TIMEOUT`] for a P4.1 address check).
///
/// In a `test-fixtures` build, `TMUX_TOOLS_TEST_REX_TIMEOUT_ONLY=<text>` lets a
/// contract test time out exactly the calls it targets, whatever time each fake call
/// takes: a call whose command line (`[-S <address>] <args>`, joined by spaces)
/// contains `<text>` has the address-check limit (2 s), and every other call, address
/// checks included, has no limit (`None`). A `<text>` that no call contains gives no
/// call a limit. Without the control, every call has its production limit.
fn limit(server: Server<'_>, args: &[&str], production: Duration) -> Option<Duration> {
    #[cfg(feature = "test-fixtures")]
    if let Some(only) = env_value("TMUX_TOOLS_TEST_REX_TIMEOUT_ONLY") {
        let mut line: Vec<String> = Vec::new();
        match server {
            Server::Caller => {
                if let Some(address) = env_value(REX_SERVER) {
                    line.extend(["-S".to_owned(), address]);
                }
            }
            Server::Stored(UNSET_ADDRESS) => {}
            Server::Stored(address) => line.extend(["-S".to_owned(), address.to_owned()]),
        }
        line.extend(args.iter().map(|arg| (*arg).to_owned()));
        return line
            .join(" ")
            .contains(&only)
            .then_some(ADDRESS_CHECK_TIMEOUT);
    }
    #[cfg(not(feature = "test-fixtures"))]
    let _ = (server, args);
    Some(production)
}

/// The time limit of the P4.1 address check of a server directory stored as `address`.
fn address_check_limit(address: &str) -> Option<Duration> {
    limit(
        Server::Stored(address),
        &ADDRESS_CHECK_ARGS,
        ADDRESS_CHECK_TIMEOUT,
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// How long ago `path` was last modified; zero when unknown.
fn age(path: &Path) -> Duration {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .unwrap_or_default()
}

/// Delete a directory; one already gone is not an error.
fn remove_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("delete {}", path.display())),
    }
}

/// An RFC 3339 timestamp (`2026-11-15T02:05:47.570986-05:00`, `…Z`) in nanoseconds
/// since the Unix epoch, keeping every fraction digit up to nanoseconds.
fn rfc3339_ns(text: &str) -> Option<u128> {
    let (date, time) = text.split_once(['T', 't', ' '])?;
    let mut date_parts = date.splitn(3, '-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    let (clock, offset_seconds) = if let Some(clock) = time.strip_suffix(['Z', 'z']) {
        (clock, 0)
    } else {
        let at = time.rfind(['+', '-'])?;
        let (clock, offset) = time.split_at(at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = offset[1..].split_once(':')?;
        let hours: i64 = hours.parse().ok()?;
        let minutes: i64 = minutes.parse().ok()?;
        (clock, sign * (hours * 3600 + minutes * 60))
    };
    let mut clock_parts = clock.splitn(3, ':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let seconds = clock_parts.next()?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let second: i64 = whole.parse().ok()?;
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let nanos: i64 = format!("{fraction:0<9}")[..9].parse().ok()?;
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    u128::try_from(i128::from(seconds) * 1_000_000_000 + i128::from(nanos)).ok()
}

fn lines_text<S: AsRef<str>>(lines: &[S]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line.as_ref());
        text.push('\n');
    }
    text
}

fn last_lines<S: AsRef<str>>(lines: &[S], count: usize) -> String {
    lines_text(&lines[lines.len().saturating_sub(count)..])
}

fn unsupported(feature: &'static str) -> anyhow::Error {
    Unsupported {
        feature,
        host: Host::Rex,
    }
    .into()
}

/// Whether `value` is a Rex block id `block:<uuid>` (P1).
fn is_block_id(value: &str) -> bool {
    value.strip_prefix("block:").is_some_and(|uuid| {
        !uuid.is_empty() && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    })
}

/// The bytes of one `send` argument (P5.3): a key by the Orca rule, with
/// application-mode bytes for an unmodified cursor key when mode 1 is on.
fn key_bytes(argument: &str, application_cursor: bool) -> Result<String> {
    let bytes = keys::translate(argument, "Rex")?;
    if application_cursor {
        if let Some((0, keys::Key::Named(named))) = keys::parse(argument) {
            let final_char = match named {
                keys::Named::Up => Some('A'),
                keys::Named::Down => Some('B'),
                keys::Named::Right => Some('C'),
                keys::Named::Left => Some('D'),
                _ => None,
            };
            if let Some(final_char) = final_char {
                return Ok(format!("\x1bO{final_char}"));
            }
        }
    }
    Ok(bytes)
}

/// The test-only hook of the P4.1 publication: `TMUX_TOOLS_TEST_REX_HOOK` is a shell
/// command run with the phase as `$1`, without the variable in its environment.
#[cfg(feature = "test-fixtures")]
fn test_hook(phase: &str) {
    if let Some(hook) = env_value("TMUX_TOOLS_TEST_REX_HOOK") {
        let _ = Command::new("/bin/sh")
            .args(["-c", &hook, "hook", phase])
            .env_remove("TMUX_TOOLS_TEST_REX_HOOK")
            .stdin(Stdio::null())
            .status();
    }
}

impl HostPort for RexHost {
    fn host(&self) -> Host {
        Host::Rex
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
        error.downcast_ref::<RexError>().is_some()
    }

    fn active_pane(&self, _target: &str) -> Result<String> {
        Err(unsupported("--window"))
    }

    fn scope_target(&self, _session: Option<&str>, _window: Option<&str>) -> String {
        String::new()
    }

    fn calling_pane(&self) -> Result<String> {
        self.calling_block()
    }

    fn server_identity(&self) -> Result<String> {
        Ok(self.status()?.instance_id.clone())
    }

    fn managed_pane(&self) -> Result<String> {
        self.calling_block()
    }

    fn create_split(
        &self,
        _target: &str,
        _direction: SplitDirection,
        _percent: Option<u32>,
        _command: &str,
        _detached: bool,
    ) -> Result<String> {
        bail!("rex creates blocks only with a Pane record")
    }

    fn create_window(&self, _session: &str, _command: &str) -> Result<String> {
        bail!("rex creates blocks only with a Pane record")
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
        let (listing, checked) = self.checked_records()?;
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
            let session = listing
                .find(&handle)
                .map(|block| block.session_id.clone())
                .unwrap_or_default();
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
        if !listing.complete {
            eprintln!("list incomplete: rex");
        }
        Ok(rows)
    }

    fn describe_pane(&self, pane: Option<&str>) -> Result<PaneDescription> {
        let block = match pane {
            Some(pane) => pane.to_owned(),
            None => self.calling_block()?,
        };
        let listing = self.listing()?;
        let listed = listing.find(&block);
        let registered = self.read_registration(&block)?;
        let summary = PaneSummary {
            session: listed
                .map(|block| block.session_id.clone())
                .unwrap_or_default(),
            window_index: String::new(),
            window_name: listed
                .and_then(|block| block.window_id.clone())
                .unwrap_or_default(),
            pane_index: String::new(),
            pane_id: block.clone(),
            name: registered.name,
            agent: registered.agent,
            access: registered.access,
            launched_at: registered.launched_at,
            cwd: registered.cwd,
        };
        let raw = listed
            .map(|block| block.raw.clone())
            .unwrap_or_else(|| json!({"block_id": block}));
        Ok(PaneDescription {
            raw: format!("{}\n", serde_json::to_string(&raw)?),
            summary: Ok(summary),
        })
    }

    fn pane_exists(&self, pane: &str) -> Result<bool> {
        Ok(self.listing()?.find(pane).is_some())
    }

    fn capture(&self, pane: &str, range: CaptureRange) -> Result<String> {
        // P5.1: `rex capture -b <block>`, text format, without `--unwrap`.
        let lines = self.capture_lines(pane)?;
        match range {
            CaptureRange::All => Ok(lines_text(&lines)),
            CaptureRange::Tail(0) | CaptureRange::Visible => {
                let (rows, _) = self.size(pane)?;
                Ok(last_lines(&lines, rows))
            }
            CaptureRange::Tail(count) => Ok(last_lines(&lines, count as usize)),
            CaptureRange::Rows { .. } => {
                let (rows, _) = self.size(pane)?;
                let screen = &lines[lines.len().saturating_sub(rows)..];
                let last = screen
                    .iter()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .cloned()
                    .unwrap_or_default();
                Ok(format!("{last}\n"))
            }
        }
    }

    fn capture_ansi(&self, pane: &str, _start: Option<i64>, _end: Option<i64>) -> Result<String> {
        self.capture(pane, CaptureRange::Visible)
    }

    fn capture_with_screen(&self, pane: &str, range: CaptureRange) -> Result<ScreenCapture> {
        // P5.1: the no-scrollback notice fires when `modes` reports the alternate
        // screen on.
        let (alternate_on, _) = self.modes(pane)?;
        Ok(ScreenCapture {
            alternate_on,
            text: self.capture(pane, range)?,
        })
    }

    fn poll_screen(&self, pane: &str) -> Result<ScreenCapture> {
        Ok(ScreenCapture {
            alternate_on: false,
            text: self.capture(pane, CaptureRange::Visible)?,
        })
    }

    fn probe_history(&self, _pane: &str) -> Result<(HistoryProbe, String)> {
        bail!("rex reports no history offset; its History mark is a capture")
    }

    fn start_output_pipe(&self, _pane: &str, _command: &str) -> Result<()> {
        bail!("rex has no output pipe")
    }

    fn stop_output_pipe(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        let (_, modes) = self.modes(pane)?;
        let application_cursor = modes.contains(&1);
        let mut bytes = String::new();
        for key in keys {
            bytes.push_str(&key_bytes(key, application_cursor)?);
        }
        self.send_bytes(pane, &bytes)
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.send_bytes(pane, text)
    }

    fn bracketed_paste_enabled(&self, pane: &str) -> Result<bool> {
        Ok(self.modes(pane)?.1.contains(&2004))
    }

    fn paste_text(
        &self,
        _pane: &str,
        _text: &str,
        _confirm: &mut dyn FnMut() -> Result<bool>,
    ) -> Result<()> {
        bail!("rex delivers prompts with a block.event paste")
    }

    fn close_pane(&self, pane: &str, _retile_window: Option<&str>) -> Result<()> {
        let record = self.record_for_block(pane)?;
        self.close_block(pane)?;
        if let Some(record) = record {
            registry::delete(Host::Rex, &record.server_id, &record.name)?;
        }
        Ok(())
    }

    fn read_registration(&self, pane: &str) -> Result<Registered> {
        Ok(self
            .record_for_block(pane)?
            .map(|record| record.registered())
            .unwrap_or_default())
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
        // A dead or unlisted block has no Host state.
        panes
            .iter()
            .map(|pane| match self.host_state(pane) {
                Err(error) if self.is_command_failure(&error) => Ok(None),
                state => state,
            })
            .collect()
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

    fn is_pane_address(&self, value: &str) -> bool {
        is_block_id(value)
    }

    fn host_word(&self, pane: &str) -> Result<Option<String>> {
        Ok(self.newest_record(pane)?.map(|record| record.state))
    }

    fn refuses_unguaranteed_multiline(&self) -> bool {
        true
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
        let anchor = match split {
            Some(_) => Some(match target {
                Some(target) => target.to_owned(),
                None => self.calling_block().map_err(|_| {
                    anyhow!(
                        "{REX_BLOCK} is unset: a split needs the calling Rex block as its \
                         anchor; pass --target or run inside a Rex block"
                    )
                })?,
            }),
            None => None,
        };
        // P3.1: the target block's session, else `REX_SESSION`, else
        // `TMUX_TOOLS_REX_SESSION`.
        let session = match target {
            Some(target) => self.session_of_block(target)?,
            None => env_value(REX_SESSION)
                .or_else(|| env_value(TMUX_TOOLS_REX_SESSION))
                .ok_or_else(|| {
                    anyhow!(
                        "no Rex session for the new block: {REX_SESSION} and \
                         {TMUX_TOOLS_REX_SESSION} are unset; pass --target or set one of them"
                    )
                })?,
        };
        let cwd = registration
            .cwd
            .map(str::to_owned)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|dir| dir.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "/".to_owned());

        let mut record = self.reserve(registration)?;
        let name = record.name.clone();
        let label = label_of(&name);
        let mut args: Vec<&str> = match (&anchor, split) {
            (Some(anchor), Some(direction)) => vec![
                "split",
                match direction {
                    SplitDirection::Horizontal => "--split=right",
                    SplitDirection::Vertical => "--split=below",
                },
                "-b",
                anchor,
            ],
            _ => vec!["run", "-s", &session],
        };
        args.extend_from_slice(&[
            "--focus=false",
            "--label",
            &label,
            "--cwd",
            &cwd,
            "--json",
            "--",
            "/bin/sh",
            "-c",
            command,
        ]);
        let created = self.exec(
            Server::Caller,
            &args,
            limit(Server::Caller, &args, CALL_TIMEOUT),
        )?;
        let handle = match created {
            Ran::Exited {
                code: Some(0),
                stdout,
                ..
            } => serde_json::from_slice::<Value>(&stdout)
                .ok()
                .and_then(|answer| {
                    answer
                        .get("block_ids")
                        .and_then(Value::as_array)
                        .and_then(|ids| ids.first())
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
            // P4.4: exit 3 (not found) or 4 (no server) is a definite failure.
            Ran::Exited {
                code: Some(code @ (3 | 4)),
                stderr,
                ..
            } => {
                let _ = registry::delete(Host::Rex, &record.server_id, &name);
                return Err(RexError {
                    command: command_name(&args),
                    code: Some(code),
                    message: stderr,
                }
                .into());
            }
            _ => None,
        };
        let handle = match handle {
            Some(handle) => handle,
            None => self.recover(&record, &session)?,
        };

        record.handle = Some(handle.clone());
        if let Err(error) = self.write_record(&record) {
            // The reservation is released only once the block is closed.
            return match self.close_block(&handle) {
                Ok(()) => {
                    let _ = registry::delete(Host::Rex, &record.server_id, &name);
                    Err(error.context(format!(
                        "spawn {name}: could not record block {handle}, so it was closed"
                    )))
                }
                Err(close_error) => Err(anyhow!(
                    "spawn {name}: could not record block {handle} ({error:#}) and could not \
                     close it ({close_error:#}); block {handle} stays open and unregistered, \
                     and the name stays reserved until it becomes an orphan reservation"
                )),
            };
        }
        Ok((handle, name))
    }

    fn history_cursor(&self, pane: &str) -> Result<String> {
        // P5.2: the History mark is a full capture taken before submission.
        let lines = self.capture_lines(pane)?;
        let (alternate, _) = self.modes(pane)?;
        let (_, columns) = self.size(pane)?;
        Ok(serde_json::to_string(&Mark {
            lines,
            alternate,
            columns,
        })?)
    }

    fn read_history_since(&self, pane: &str, cursor: &str) -> Result<String> {
        Ok(self.read_history_since_complete(pane, cursor)?.text)
    }

    fn read_history_since_complete(&self, pane: &str, cursor: &str) -> Result<HistorySince> {
        let mark: Mark = serde_json::from_str(cursor)
            .map_err(|error| anyhow!("not a rex History mark: {cursor:?} ({error})"))?;
        let lines = self.capture_lines(pane)?;
        let (rows, columns) = self.size(pane)?;
        let (alternate, _) = self.modes(pane)?;
        let after: Vec<&str> = lines.iter().map(String::as_str).collect();
        let first_changed = history_mark::first_changed_line(&mark.lines, &after);
        let primary = !mark.alternate && !alternate;
        let same_width = mark.columns == columns;
        let in_rows = history_mark::within_last_rows(first_changed, mark.lines.len(), rows);
        let complete = primary && same_width && in_rows;
        let incomplete_reason = if !primary {
            "the block was on the alternate screen, which keeps no scrollback".to_owned()
        } else if !same_width {
            "the block's width changed between the mark and settle".to_owned()
        } else {
            "rex dropped the oldest scrollback lines after the mark".to_owned()
        };
        // P5.2: an incomplete result is the last `rows` lines of the capture after
        // settle (a stated exception: it does not start at the History mark).
        let text = if complete {
            lines_text(lines.get(first_changed..).unwrap_or_default())
        } else {
            last_lines(&lines, rows)
        };
        Ok(HistorySince {
            text,
            complete,
            incomplete_reason,
        })
    }

    fn prompt_delivery_guaranteed(&self, pane: &str, _agent: Option<&str>) -> Result<bool> {
        // P6.5: an older server cannot take a prompt; nothing is sent.
        let revision = self.status()?.api_revision;
        if revision < PROMPT_API_REVISION {
            bail!("rex api revision {revision} is older than {PROMPT_API_REVISION}");
        }
        // P6.4: guaranteed when mode 2004 is on before the send.
        self.bracketed_paste_enabled(pane)
    }

    fn submit_prompt(&self, pane: &str, text: &str, _wait: Duration) -> Result<Submission> {
        let baseline_not_working = self
            .newest_record(pane)?
            .and_then(|record| record.host_state())
            != Some(HostState::Working);
        let session = self.session_of_block(pane)?;
        // P6.3: a `paste` event, then Enter.
        let event = json!({
            "session_id": session,
            "block_id": pane,
            "event": {"type": "paste", "payload": {"text": text}},
        })
        .to_string();
        // The send boundary (P6.8): taken immediately before the transmission, after
        // every pre-send read.
        let sent_ms = now_ms();
        *self.pending.lock().expect("pending prompt") = Some(Pending {
            pane: pane.to_owned(),
            sent_ms,
            baseline_not_working,
            confirmed: false,
            turn_observed: false,
        });
        self.call(&["api", "call", "block.event", &event])?;
        self.call(&["send-key", "-b", pane, "enter"])?;
        self.status_records(pane)?;
        self.settle_submission(pane, Submission::Accepted)
    }

    fn settle_submission(&self, pane: &str, submit: Submission) -> Result<Submission> {
        if submit == Submission::Confirmed {
            return Ok(submit);
        }
        let confirmed = |host: &RexHost| {
            host.pending
                .lock()
                .expect("pending prompt")
                .as_ref()
                .is_some_and(|pending| pending.confirmed)
        };
        if !confirmed(self) {
            self.status_records(pane)?;
        }
        Ok(if confirmed(self) {
            Submission::Confirmed
        } else {
            submit
        })
    }

    fn send_boundary_ms(&self, pane: &str) -> Option<u64> {
        self.pending
            .lock()
            .expect("pending prompt")
            .as_ref()
            .filter(|pending| pending.pane == pane)
            .map(|pending| pending.sent_ms)
    }

    fn turn_observed(&self, _pane: &str) -> bool {
        // P6.7: a record with `updated_at` after the send.
        self.pending
            .lock()
            .expect("pending prompt")
            .as_ref()
            .is_some_and(|pending| pending.turn_observed)
    }

    fn host_state_since(&self, pane: &str) -> Result<Option<(HostState, Option<u64>)>> {
        Ok(self
            .newest_record(pane)?
            .and_then(|record| Some((record.host_state()?, record.updated_ms))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_timestamps_with_offsets_compare_as_instants() {
        // 2026-11-15T07:05:47.570Z is 1794726347570 ms (`date -u -j -f … +%s`).
        assert_eq!(
            rfc3339_ns("2026-11-15T07:05:47.570986Z"),
            Some(1_794_726_347_570_986_000)
        );
        assert_eq!(
            rfc3339_ns("2026-11-15T02:05:47.570986-05:00"),
            Some(1_794_726_347_570_986_000)
        );
        assert_eq!(rfc3339_ns("1970-01-01T00:00:01+00:00"), Some(1_000_000_000));
    }

    #[test]
    fn a_rename_relabels_only_when_the_first_128_characters_change() {
        let name = |length: usize, last: char| {
            let mut name: String = "a".repeat(length - 1);
            name.push(last);
            name
        };
        // Two names of 128 or more characters that share the first 128: no rename.
        assert_eq!(relabel(&name(130, 'x'), &name(129, 'y')), None);
        assert_eq!(relabel(&name(128, 'a'), &name(140, 'z')), None);
        // A rename to a 129-character name: the label is its first 128 characters.
        let new = format!("b{}", name(128, 'c'));
        assert_eq!(new.chars().count(), 129);
        assert_eq!(relabel("lead.tm1", &new), Some(new[..128].to_owned()));
        assert_eq!(
            relabel("lead.tm1.respawn-1", "lead.tm1").as_deref(),
            Some("lead.tm1")
        );
    }

    /// P4.1: with no test-only control set, the address check of another server
    /// directory may run 2 seconds and every other `rex` call 120 seconds.
    #[test]
    fn with_no_test_control_an_address_check_has_2_s_and_a_call_120_s() {
        assert!(std::env::var_os("TMUX_TOOLS_TEST_REX_TIMEOUT_ONLY").is_none());
        for address in ["unix:///tmp/tt-x.sock", UNSET_ADDRESS] {
            assert_eq!(address_check_limit(address), Some(Duration::from_secs(2)));
        }
        assert_eq!(
            limit(Server::Caller, &["ls", "--json"], CALL_TIMEOUT),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn a_block_id_is_block_and_a_uuid() {
        assert!(is_block_id("block:01a1de06-3ec1-758d-b220-3ae833f0c0d6"));
        assert!(!is_block_id("block:"));
        assert!(!is_block_id("a.b"));
    }
}
