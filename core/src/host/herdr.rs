//! The Herdr Host backend: every `hc` CLI invocation `tmux-tools` runs is built here
//! (PRD-261109-2223-01).
//!
//! The backend calls only `hc <command> --json …` and reads its JSON (P2.4). A pane is
//! an hc terminal, addressed by `t<n>`. The calling pane is the terminal named by
//! `HC_TERMINAL_ID`. The Pane registration is stored as `tt.*` labels on the terminal
//! (P4.1), so Herdr uses no Pane record; every terminal is created with its name and
//! registration in one `hc run` (P3.4).

use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::host::history_mark;
use crate::host::{
    CaptureRange, Delivery, HistoryProbe, HistorySince, Host, HostCapabilities, HostPort,
    HostState, ListScope, PaneDescription, PaneListing, PaneSummary, RegistrationKey,
    ScreenCapture, SelectionSource, SplitDirection, Submission, Unsupported, WindowHome,
};
use crate::layout::{Registration, WindowGeometry};
use crate::names::{self, Registered};
use crate::registry::NameTaken;

mod keys;

/// The calling pane's terminal address, set by herdr-core inside its terminals.
pub const HC_TERMINAL_ID: &str = "HC_TERMINAL_ID";
/// The Herdr space id for new terminals when no `--target` and no `HC_TERMINAL_ID`
/// give one (P3.1).
pub const TMUX_TOOLS_HERDR_SPACE: &str = "TMUX_TOOLS_HERDR_SPACE";
/// The Herdr tab id for new terminals when no `--target` and no `HC_TERMINAL_ID`
/// give one (P3.1).
pub const TMUX_TOOLS_HERDR_TAB: &str = "TMUX_TOOLS_HERDR_TAB";

/// The labels through which a terminal shows in the Herdr window (P3.1).
const SPACE_LABEL: &str = "herdr.space";
const TAB_LABEL: &str = "herdr.tab";
/// The label hc shows as the terminal's place; set to the tmux-tools pane name (P4.1).
const WHERE_LABEL: &str = "where";

/// `hc read --lines` for the whole history (P5.1).
const ALL_LINES: &str = "1000000";

/// How many fresh derivations a spawn tries when a derived name is already taken.
const DERIVED_NAME_ATTEMPTS: usize = 16;

/// The hc name of a tmux-tools pane name: `tt-` and the first 16 lowercase hex
/// characters of SHA-256 of its UTF-8 bytes (P1).
pub fn hc_name(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("tt-{}", &hex[..16])
}

/// The label that stores one Pane registration field (P4.1).
fn label_key(key: RegistrationKey) -> &'static str {
    match key {
        RegistrationKey::Name => "tt.name",
        RegistrationKey::Agent => "tt.agent",
        RegistrationKey::Access => "tt.access",
        RegistrationKey::LaunchedAt => "tt.launched_at",
        RegistrationKey::Cwd => "tt.cwd",
        RegistrationKey::Surface => "tt.surface",
        RegistrationKey::SurfaceUnvalidated => "tt.surface_unvalidated",
        RegistrationKey::Parent => "tt.parent",
        RegistrationKey::Team => "tt.team",
        RegistrationKey::ShimId => "tt.shim_id",
        RegistrationKey::Title => "tt.title",
        RegistrationKey::ClaudeAgentName => "tt.claude_agent_name",
        RegistrationKey::ClaudeTeamName => "tt.claude_team_name",
    }
}

/// An `hc` command that ran and reported an error (`{"error":{kind,message,hint}}`).
#[derive(Debug)]
pub struct HcError {
    pub command: String,
    pub kind: String,
    pub message: String,
}

impl std::fmt::Display for HcError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(formatter, "hc {}: {}", self.command, self.kind)
        } else {
            write!(
                formatter,
                "hc {}: {}: {}",
                self.command, self.kind, self.message
            )
        }
    }
}

impl std::error::Error for HcError {}

/// One `hc` terminal as `hc get --json` reports it.
#[derive(Clone, Debug)]
struct Terminal(Value);

impl Terminal {
    fn address(&self) -> &str {
        str_field(&self.0, "address").unwrap_or_default()
    }

    fn label(&self, key: &str) -> Option<&str> {
        self.0
            .get("labels")
            .and_then(|labels| labels.get(key))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }

    fn finished(&self) -> bool {
        str_field(&self.0, "state") == Some("finished")
    }

    fn turn(&self) -> u64 {
        self.0.get("turn").and_then(Value::as_u64).unwrap_or(0)
    }

    /// The program's own status word (OSC 7501 root record).
    fn status_word(&self) -> Option<&str> {
        self.0
            .get("status")
            .filter(|status| status.is_object())
            .and_then(|status| str_field(status, "state"))
    }

    fn status_ms(&self) -> Option<u64> {
        self.0.get("status_ms").and_then(Value::as_u64)
    }

    /// The terminal's screen height; 0 when `hc get` does not report it.
    fn rows(&self) -> usize {
        self.0.get("rows").and_then(Value::as_u64).unwrap_or(0) as usize
    }

    /// P6.2: Host state and the time its word last changed.
    fn host_state(&self) -> Option<(HostState, Option<u64>)> {
        if self.finished() {
            return None;
        }
        let state = match self.status_word()? {
            "idle" => HostState::Idle,
            "working" => HostState::Working,
            "blocked" | "error" => HostState::Blocked,
            "done" => HostState::Done,
            _ => return None,
        };
        Some((state, self.status_ms()))
    }

    fn registered(&self) -> Registered {
        let owned = |key: RegistrationKey| self.label(label_key(key)).map(str::to_owned);
        Registered {
            name: owned(RegistrationKey::Name),
            agent: owned(RegistrationKey::Agent),
            access: owned(RegistrationKey::Access),
            launched_at: owned(RegistrationKey::LaunchedAt),
            cwd: owned(RegistrationKey::Cwd),
            surface: owned(RegistrationKey::Surface),
            surface_unvalidated: self.label(label_key(RegistrationKey::SurfaceUnvalidated))
                == Some("true"),
            parent: owned(RegistrationKey::Parent),
            team: owned(RegistrationKey::Team),
            shim_id: owned(RegistrationKey::ShimId),
            title: owned(RegistrationKey::Title),
            claude_agent_name: owned(RegistrationKey::ClaudeAgentName),
            claude_team_name: owned(RegistrationKey::ClaudeTeamName),
        }
    }
}

/// One `hc read --json` answer.
struct Read {
    text: String,
    how: String,
}

impl Read {
    fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    /// The read as capture text: every line ends with a newline.
    fn capture(&self) -> String {
        lines_text(&self.lines())
    }

    /// Whether the read reports `how: history` (P5.1). The `complete` value `hc read`
    /// reports does not change Completeness.
    fn is_history(&self) -> bool {
        self.how == "history"
    }

    /// Whether the read reports `how: stitched` or `how: screen`; the `prompt` result
    /// is then that whole read (P5.1).
    fn is_stitched_or_screen(&self) -> bool {
        matches!(self.how.as_str(), "stitched" | "screen")
    }
}

fn lines_text(lines: &[&str]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// The History mark (P5.1): the lines of the full read taken before submission, and
/// whether that read reported `how: history`.
#[derive(serde::Serialize, serde::Deserialize)]
struct Mark {
    lines: Vec<String>,
    history: bool,
}

impl Mark {
    /// P5.1: the smallest line index at which `after` differs from the mark read,
    /// where a line that only one read has counts as different; the number of lines
    /// of the mark read when the reads are equal.
    fn first_changed_line(&self, after: &[&str]) -> usize {
        history_mark::first_changed_line(&self.lines, after)
    }
}

/// A prompt sent by [`HostPort::submit_prompt`], whose submission is judged against
/// the terminal as it was read before the send (P6.5).
struct Pending {
    pane: String,
    baseline_turn: u64,
    /// The time before the send, in milliseconds since the Unix epoch.
    sent_ms: u64,
    confirmed: bool,
}

impl Pending {
    /// P6.5: `turn` went up, or the status word is `working` with `status_ms` after the
    /// send. An unchanged pre-send `working` never confirms.
    fn observe(&mut self, terminal: &Terminal) {
        let working_since_send = terminal.status_word() == Some("working")
            && terminal.status_ms().is_some_and(|ms| ms > self.sent_ms);
        if terminal.turn() > self.baseline_turn || working_since_send {
            self.confirmed = true;
        }
    }
}

/// Where the name of a spawn or a Target stands among the terminals (P4.4).
enum NameLookup {
    /// One live terminal carries the name.
    Live(String),
    /// Only dead terminals carried it; they are removed now.
    Dead,
    /// No terminal carries it.
    Unknown,
}

/// The Herdr Host backend.
pub struct HerdrHost {
    source: SelectionSource,
    status: OnceLock<std::result::Result<String, String>>,
    pending: Mutex<Option<Pending>>,
}

impl HerdrHost {
    /// The backend for an invocation that selected Herdr through `source`.
    pub fn new(source: SelectionSource) -> HerdrHost {
        HerdrHost {
            source,
            status: OnceLock::new(),
            pending: Mutex::new(None),
        }
    }

    // --- The CLI -------------------------------------------------------------------

    /// Run `hc <command> --json <args>` and return its JSON answer. Not reaching the CLI
    /// names the host and the selection source (P2.3).
    fn call(&self, command: &[&str], args: &[&str]) -> Result<Value> {
        self.run_hc(Command::new("hc"), command, args)
    }

    /// `hc send <pane> -- <text>`, run without `HC_TERMINAL_ID` so hc delivers the text
    /// exactly as given, with no `<hc from="…">` tag (P6.5).
    fn send(&self, pane: &str, text: &str) -> Result<Value> {
        let mut hc = Command::new("hc");
        hc.env_remove(HC_TERMINAL_ID);
        self.run_hc(hc, &["send"], &[pane, "--", text])
    }

    fn run_hc(&self, mut hc: Command, command: &[&str], args: &[&str]) -> Result<Value> {
        let output = hc
            .args(command)
            .arg("--json")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| self.unreachable(&format!("cannot run the hc CLI: {error}")))?;
        let command_name = command.join(" ");
        if output.status.success() {
            return serde_json::from_slice(&output.stdout).map_err(|error| {
                anyhow!(
                    "hc {command_name} answered no JSON ({error}): {}",
                    String::from_utf8_lossy(&output.stdout).trim()
                )
            });
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let parsed: Option<Value> = serde_json::from_str(&stderr)
            .ok()
            .or_else(|| serde_json::from_slice(&output.stdout).ok());
        let error = parsed
            .as_ref()
            .and_then(|parsed| parsed.get("error"))
            .cloned()
            .unwrap_or(Value::Null);
        Err(HcError {
            command: command_name,
            kind: str_field(&error, "kind").unwrap_or("error").to_owned(),
            message: str_field(&error, "message")
                .map(str::to_owned)
                .unwrap_or(stderr),
        }
        .into())
    }

    fn unreachable(&self, detail: &str) -> anyhow::Error {
        anyhow!(
            "host herdr (selected by {}) is unreachable: {detail}",
            self.source
        )
    }

    /// `core.id` of `hc server status --json`, read once per invocation (P2.5).
    fn core_id(&self) -> Result<&str> {
        let status = self.status.get_or_init(|| {
            let answer = self.call(&["server", "status"], &[]).map_err(|error| {
                match error.downcast_ref::<HcError>() {
                    Some(_) => format!("{}", self.unreachable(&format!("{error:#}"))),
                    None => format!("{error:#}"),
                }
            })?;
            answer
                .pointer("/core/id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    format!(
                        "{}",
                        self.unreachable("hc server status reports no core.id")
                    )
                })
        });
        status.as_deref().map_err(|message| anyhow!("{message}"))
    }

    fn get(&self, pane: &str) -> Result<Terminal> {
        let terminal = Terminal(self.call(&["get"], &[pane])?);
        if let Some(pending) = self.pending.lock().expect("pending prompt").as_mut() {
            if pending.pane == terminal.address() {
                pending.observe(&terminal);
            }
        }
        Ok(terminal)
    }

    fn read(&self, pane: &str, lines: &str) -> Result<Read> {
        let answer = self.call(&["read"], &[pane, "--lines", lines])?;
        Ok(Read {
            text: str_field(&answer, "text").unwrap_or_default().to_owned(),
            how: str_field(&answer, "how").unwrap_or_default().to_owned(),
        })
    }

    /// The addresses of every terminal `hc ls --all` lists.
    fn addresses(&self) -> Result<Vec<String>> {
        let answer = self.call(&["ls"], &["--all"])?;
        Ok(answer
            .get("terminals")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|terminal| str_field(terminal, "address"))
            .map(str::to_owned)
            .collect())
    }

    /// Every terminal that carries `tt.name`, read with `hc get` (`hc ls` rows carry
    /// no labels). A terminal removed between the list and the get is skipped.
    fn registered_terminals(&self) -> Result<Vec<Terminal>> {
        let mut terminals = Vec::new();
        for address in self.addresses()? {
            match self.get(&address) {
                Ok(terminal) => {
                    if terminal.label(label_key(RegistrationKey::Name)).is_some() {
                        terminals.push(terminal);
                    }
                }
                Err(error) if is_kind(&error, "not_found") => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(terminals)
    }

    /// `hc rm` by address; a terminal already removed counts as removed.
    fn remove(&self, address: &str) -> Result<()> {
        match self.call(&["rm"], &[address]) {
            Ok(_) => Ok(()),
            Err(error) if is_kind(&error, "not_found") => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// The registered terminals after P4.4: the live ones, and apart from them the
    /// dead ones (finished, with `tt.name`) that carry `only` — every dead one when
    /// `only` is `None` (`list`) — each removed by address and returned once. Dead
    /// terminals of other names are left for `list` to report.
    fn swept_terminals(&self, only: Option<&str>) -> Result<(Vec<Terminal>, Vec<Terminal>)> {
        let (dead, live): (Vec<Terminal>, Vec<Terminal>) = self
            .registered_terminals()?
            .into_iter()
            .partition(Terminal::finished);
        let dead: Vec<Terminal> = dead
            .into_iter()
            .filter(|terminal| {
                only.is_none_or(|name| {
                    terminal.label(label_key(RegistrationKey::Name)) == Some(name)
                })
            })
            .collect();
        for terminal in &dead {
            self.remove(terminal.address())?;
        }
        Ok((live, dead))
    }

    /// P4.4 for one name: dead terminals that carry it are removed first.
    fn lookup(&self, name: &str) -> Result<NameLookup> {
        let carries =
            |terminal: &Terminal| terminal.label(label_key(RegistrationKey::Name)) == Some(name);
        let (live, dead) = self.swept_terminals(Some(name))?;
        let live: Vec<String> = live
            .iter()
            .filter(|terminal| carries(terminal))
            .map(|terminal| terminal.address().to_owned())
            .collect();
        match live.len() {
            0 if dead.iter().any(carries) => Ok(NameLookup::Dead),
            0 => Ok(NameLookup::Unknown),
            1 => Ok(NameLookup::Live(live.into_iter().next().expect("one"))),
            _ => bail!(
                "name '{}' resolves to multiple panes: {}; use an hc address (t<n>) explicitly",
                name,
                live.join(", ")
            ),
        }
    }

    fn calling_terminal(&self) -> Result<String> {
        // PRD-261108-0447-01 P3.3: inside a team the calling pane is the Leader.
        if let Some(leader) = crate::team::leader_handle() {
            return Ok(leader);
        }
        env_value(HC_TERMINAL_ID).ok_or_else(|| {
            anyhow!(
                "{HC_TERMINAL_ID} is unset: this verb needs it to name the calling Herdr terminal"
            )
        })
    }

    /// The `herdr.space` and `herdr.tab` of a new terminal (P3.1–P3.2): from the
    /// target, else the calling terminal, else the environment. A split joins the tab;
    /// any other placement opens a new tab with a tab id that tmux-tools makes up
    /// ([`made_up_tab`]), which a later split from this terminal joins. No space, no
    /// labels.
    fn placement(
        &self,
        split: Option<SplitDirection>,
        target: Option<&str>,
    ) -> Result<(Option<String>, Option<String>)> {
        let source = match target {
            Some(target) => Some(self.get(target)?),
            None => match env_value(HC_TERMINAL_ID) {
                Some(calling) => Some(self.get(&calling)?),
                None => None,
            },
        };
        let (space, tab) = match &source {
            Some(terminal) => (
                terminal.label(SPACE_LABEL).map(str::to_owned),
                terminal.label(TAB_LABEL).map(str::to_owned),
            ),
            None => (
                env_value(TMUX_TOOLS_HERDR_SPACE),
                env_value(TMUX_TOOLS_HERDR_TAB),
            ),
        };
        let Some(space) = space else {
            return Ok((None, None));
        };
        Ok(match split {
            Some(_) => (Some(space), tab),
            None => (Some(space), Some(made_up_tab())),
        })
    }

    /// The `tt.*` labels of a new terminal named `name` (P4.1).
    fn registration_labels(
        &self,
        name: &str,
        registration: &Registration<'_>,
    ) -> Vec<(RegistrationKey, String)> {
        let agent = registration.agent.as_ref();
        let team = registration.team.as_ref();
        let mut labels = vec![
            (RegistrationKey::Name, Some(name.to_owned())),
            (
                RegistrationKey::Agent,
                agent
                    .map(|agent| agent.agent.to_owned())
                    .or_else(|| team.and_then(|team| team.agent).map(str::to_owned)),
            ),
            (
                RegistrationKey::Access,
                agent.and_then(|agent| agent.access).map(str::to_owned),
            ),
            (
                RegistrationKey::LaunchedAt,
                Some(registration.launched_at.to_owned()),
            ),
            (RegistrationKey::Cwd, registration.cwd.map(str::to_owned)),
            (
                RegistrationKey::Surface,
                agent.map(|agent| agent.surface.to_owned()),
            ),
            (
                RegistrationKey::SurfaceUnvalidated,
                agent.map(|agent| agent.surface_unvalidated.to_string()),
            ),
            (
                RegistrationKey::Parent,
                registration.parent.map(str::to_owned),
            ),
            (RegistrationKey::Team, team.map(|team| team.team.to_owned())),
            (
                RegistrationKey::ShimId,
                team.map(|team| team.shim_id.to_owned()),
            ),
            (
                RegistrationKey::Title,
                team.and_then(|team| team.title).map(str::to_owned),
            ),
            (
                RegistrationKey::ClaudeAgentName,
                team.and_then(|team| team.claude_agent_name)
                    .map(str::to_owned),
            ),
            (
                RegistrationKey::ClaudeTeamName,
                team.and_then(|team| team.claude_team_name)
                    .map(str::to_owned),
            ),
        ];
        labels.retain(|(_, value)| value.as_deref().is_some_and(|value| !value.is_empty()));
        labels
            .into_iter()
            .map(|(key, value)| (key, value.expect("retained")))
            .collect()
    }

    /// One `hc run` of a new terminal named `name`; `Ok(None)` when hc reports the
    /// hc name taken (P4.2).
    fn run_terminal(
        &self,
        name: &str,
        command: &str,
        space: Option<&str>,
        tab: Option<&str>,
        registration: &Registration<'_>,
    ) -> Result<Option<String>> {
        let hc_name = hc_name(name);
        let mut labels: Vec<String> = Vec::new();
        if let Some(space) = space {
            labels.push(format!("{SPACE_LABEL}={space}"));
        }
        if let Some(tab) = tab {
            labels.push(format!("{TAB_LABEL}={tab}"));
        }
        labels.push(format!("{WHERE_LABEL}={name}"));
        for (key, value) in self.registration_labels(name, registration) {
            labels.push(format!("{}={value}", label_key(key)));
        }
        let mut args: Vec<&str> = vec!["--bg", "--outlive-parent", "--no-notify"];
        // PRD-261115-0921-01 P6: a Teammate terminal is removed by Herdr when its
        // Close-on-success wrap exits 0; no other terminal uses `--rm` (P3.4).
        if registration.team.is_some() {
            args.push("--rm");
        }
        args.extend_from_slice(&["--name", &hc_name]);
        if let Some(cwd) = registration.cwd {
            args.extend_from_slice(&["--cwd", cwd]);
        }
        for label in &labels {
            args.extend_from_slice(&["--label", label]);
        }
        args.extend_from_slice(&["--", command]);
        match self.call(&["run"], &args) {
            Ok(answer) => str_field(&answer, "address")
                .map(|address| Some(address.to_owned()))
                .ok_or_else(|| anyhow!("hc run reported no address for the new terminal")),
            Err(error) if is_kind(&error, "name_taken") => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Send hc key names to `pane`. A key hc still rejects ends the send with the
    /// `cannot type on herdr` message of its argument (P5.2).
    fn press(&self, pane: &str, keys: &[keys::Pressed]) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut args: Vec<&str> = vec![pane, "--"];
        args.extend(keys.iter().map(|key| key.name.as_str()));
        match self.call(&["keys"], &args) {
            Ok(_) => Ok(()),
            Err(error) if is_kind(&error, "invalid_params") => {
                let rejected = keys
                    .iter()
                    .find(|key| error.to_string().contains(&format!("\"{}\"", key.name)))
                    .unwrap_or(&keys[0]);
                Err(anyhow!("{}", rejected.refusal))
            }
            Err(error) => Err(error),
        }
    }

    fn update_label(&self, pane: &str, key: &str, value: &str) -> Result<()> {
        let pair = format!("{key}={value}");
        self.call(&["label"], &[pane, "--", &pair])?;
        Ok(())
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Whether `error` is an [`HcError`] of `kind`.
fn is_kind(error: &anyhow::Error, kind: &str) -> bool {
    error
        .downcast_ref::<HcError>()
        .is_some_and(|hc| hc.kind == kind)
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// A new `herdr.tab` id that no other terminal carries (P3.2): `tt-tab-` and 32
/// lowercase hex characters of SHA-256 over this process id, the wall-clock time in
/// nanoseconds, and a per-process counter. The `tt-tab-` prefix keeps it apart from
/// Herdr's own tab ids; the 128-bit digest keeps two made-up ids apart.
fn made_up_tab() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let seed = format!(
        "{}:{nanos}:{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let digest = Sha256::digest(seed.as_bytes());
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("tt-tab-{hex}")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

fn unsupported(feature: &'static str) -> anyhow::Error {
    Unsupported {
        feature,
        host: Host::Herdr,
    }
    .into()
}

/// Whether `value` is an hc address: `t` and a number (P7.6).
fn is_address(value: &str) -> bool {
    value
        .strip_prefix('t')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

impl HostPort for HerdrHost {
    fn host(&self) -> Host {
        Host::Herdr
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
        error.downcast_ref::<HcError>().is_some()
    }

    fn active_pane(&self, _target: &str) -> Result<String> {
        Err(unsupported("--window"))
    }

    fn scope_target(&self, _session: Option<&str>, _window: Option<&str>) -> String {
        String::new()
    }

    fn calling_pane(&self) -> Result<String> {
        self.calling_terminal()
    }

    fn server_identity(&self) -> Result<String> {
        Ok(self.core_id()?.to_owned())
    }

    fn managed_pane(&self) -> Result<String> {
        self.calling_terminal()
    }

    fn create_split(
        &self,
        _target: &str,
        _direction: SplitDirection,
        _percent: Option<u32>,
        _command: &str,
        _detached: bool,
    ) -> Result<String> {
        bail!("herdr creates terminals only with their registration")
    }

    fn create_window(&self, _session: &str, _command: &str) -> Result<String> {
        bail!("herdr creates terminals only with their registration")
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
        let (live, dead) = self.swept_terminals(None)?;
        let mut rows = Vec::new();
        for (terminal, pane_dead) in live
            .iter()
            .map(|terminal| (terminal, false))
            .chain(dead.iter().map(|terminal| (terminal, true)))
        {
            let registered = terminal.registered();
            let address = terminal.address().to_owned();
            rows.push(PaneListing {
                raw: serde_json::to_string(&terminal.0)?,
                session: terminal.label(SPACE_LABEL).unwrap_or_default().to_owned(),
                window_index_sort: 0,
                window_name: terminal.label(TAB_LABEL).unwrap_or_default().to_owned(),
                pane_index_sort: 0,
                pane_id: address.clone(),
                name: registered.name,
                agent: registered.agent,
                access: registered.access,
                launched_at: registered.launched_at,
                pane_dead,
                pane_dead_status: None,
                last_activity: None,
                label: Some(address),
                unknown: false,
                parent: registered.parent,
                team: registered.team,
                shim_id: registered.shim_id,
            });
        }
        Ok(rows)
    }

    fn describe_pane(&self, pane: Option<&str>) -> Result<PaneDescription> {
        let address = match pane {
            Some(pane) => pane.to_owned(),
            None => self.calling_terminal()?,
        };
        let terminal = self.get(&address)?;
        let registered = terminal.registered();
        let summary = PaneSummary {
            session: terminal.label(SPACE_LABEL).unwrap_or_default().to_owned(),
            window_index: String::new(),
            window_name: terminal.label(TAB_LABEL).unwrap_or_default().to_owned(),
            pane_index: String::new(),
            pane_id: terminal.address().to_owned(),
            name: registered.name,
            agent: registered.agent,
            access: registered.access,
            launched_at: registered.launched_at,
            cwd: registered.cwd,
        };
        Ok(PaneDescription {
            raw: format!("{}\n", serde_json::to_string(&terminal.0)?),
            summary: Ok(summary),
        })
    }

    fn pane_exists(&self, pane: &str) -> Result<bool> {
        match self.get(pane) {
            Ok(terminal) => Ok(!terminal.finished()),
            Err(error) if is_kind(&error, "not_found") => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn capture(&self, pane: &str, range: CaptureRange) -> Result<String> {
        match range {
            CaptureRange::Visible => Ok(self.read(pane, "0")?.capture()),
            CaptureRange::All => Ok(self.read(pane, ALL_LINES)?.capture()),
            CaptureRange::Tail(lines) => {
                let read = self.read(pane, &lines.max(1).to_string())?;
                let all = read.lines();
                let start = all.len().saturating_sub(lines.max(1) as usize);
                Ok(lines_text(&all[start..]))
            }
            CaptureRange::Rows { .. } => {
                let screen = self.read(pane, "0")?;
                let last = screen
                    .lines()
                    .into_iter()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default()
                    .to_owned();
                Ok(format!("{last}\n"))
            }
        }
    }

    fn capture_ansi(&self, pane: &str, _start: Option<i64>, _end: Option<i64>) -> Result<String> {
        self.capture(pane, CaptureRange::Visible)
    }

    fn capture_with_screen(&self, pane: &str, range: CaptureRange) -> Result<ScreenCapture> {
        match range {
            // P5.1: the no-scrollback notice fires when the full read reports
            // `how: screen`.
            CaptureRange::All => {
                let read = self.read(pane, ALL_LINES)?;
                Ok(ScreenCapture {
                    alternate_on: read.how == "screen",
                    text: read.capture(),
                })
            }
            range => Ok(ScreenCapture {
                alternate_on: false,
                text: self.capture(pane, range)?,
            }),
        }
    }

    fn poll_screen(&self, pane: &str) -> Result<ScreenCapture> {
        Ok(ScreenCapture {
            alternate_on: false,
            text: self.capture(pane, CaptureRange::Visible)?,
        })
    }

    fn probe_history(&self, _pane: &str) -> Result<(HistoryProbe, String)> {
        bail!("herdr reports no pane geometry; its History mark is a read")
    }

    fn start_output_pipe(&self, _pane: &str, _command: &str) -> Result<()> {
        bail!("herdr has no output pipe")
    }

    fn stop_output_pipe(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    fn send_keys(&self, pane: &str, keys: &[&str]) -> Result<()> {
        // Literal text followed by Enter in one request (`execute`) is one `hc send`:
        // hc pastes the text and presses enter. Every other request is pressed key by
        // key, each argument checked before anything is sent (P5.2).
        if let [text, enter] = keys {
            if *enter == "Enter" && !keys::is_key(text) && !text.is_empty() {
                self.send(pane, text)?;
                return Ok(());
            }
        }
        let mut pressed = Vec::new();
        for key in keys {
            pressed.extend(keys::translate(key)?);
        }
        self.press(pane, &pressed)
    }

    fn send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.press(pane, &keys::literal(text)?)
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
        bail!("herdr delivers prompts with hc send")
    }

    fn close_pane(&self, pane: &str, _retile_window: Option<&str>) -> Result<()> {
        // P7.4: end the program, then remove the terminal; already removed is success.
        // `hc kill` of a `--rm` terminal (PRD-261115-0921-01 P6.1) answers `not_found`
        // ("<t> was removed"): Herdr removed it as the program ended.
        match self.call(&["kill"], &[pane]) {
            Ok(_) => {}
            Err(error) if is_kind(&error, "not_found") => {}
            Err(error) => return Err(error),
        }
        self.remove(pane)
    }

    fn read_registration(&self, pane: &str) -> Result<Registered> {
        Ok(self.get(pane)?.registered())
    }

    fn read_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<Option<String>> {
        let terminal = self.get(pane)?;
        Ok(match key {
            RegistrationKey::SurfaceUnvalidated => terminal
                .label(label_key(key))
                .filter(|value| *value == "true")
                .map(|_| "1".to_owned()),
            key => terminal.label(label_key(key)).map(str::to_owned),
        })
    }

    fn write_registration_value(
        &self,
        pane: &str,
        key: RegistrationKey,
        value: &str,
    ) -> Result<()> {
        let value = match key {
            RegistrationKey::SurfaceUnvalidated => {
                if value == "1" || value == "true" {
                    "true"
                } else {
                    "false"
                }
            }
            _ => value,
        };
        self.update_label(pane, label_key(key), value)?;
        if key == RegistrationKey::Name {
            self.update_label(pane, WHERE_LABEL, value)?;
        }
        Ok(())
    }

    fn clear_registration_value(&self, pane: &str, key: RegistrationKey) -> Result<()> {
        self.update_label(pane, label_key(key), "")
    }

    fn pane_names(&self) -> Result<Vec<(String, String)>> {
        // Live terminals only; no dead terminal is removed here (P4.4: only `list`
        // removes every dead name).
        Ok(self
            .registered_terminals()?
            .iter()
            .filter(|terminal| !terminal.finished())
            .filter_map(|terminal| {
                Some((
                    terminal.address().to_owned(),
                    terminal.label(label_key(RegistrationKey::Name))?.to_owned(),
                ))
            })
            .collect())
    }

    fn window_registrations(&self, _target: &str) -> Result<Vec<Registered>> {
        Ok(Vec::new())
    }

    fn host_state(&self, pane: &str) -> Result<Option<HostState>> {
        Ok(self.host_state_since(pane)?.map(|(state, _)| state))
    }

    fn host_states(&self, panes: &[String]) -> Result<Vec<Option<HostState>>> {
        // A dead terminal `list` removed has no Host state (P6.2).
        panes
            .iter()
            .map(|pane| match self.host_state(pane) {
                Err(error) if is_kind(&error, "not_found") => Ok(None),
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

    // --- Registration at create, prompts, Host state ---------------------------------

    fn creates_registered(&self) -> bool {
        true
    }

    fn is_pane_address(&self, value: &str) -> bool {
        is_address(value)
    }

    fn host_word(&self, pane: &str) -> Result<Option<String>> {
        Ok(self.get(pane)?.status_word().map(str::to_owned))
    }

    fn refuses_unguaranteed_multiline(&self) -> bool {
        true
    }

    fn find_registered(&self, name: &str) -> Result<Option<String>> {
        match self.lookup(name)? {
            NameLookup::Live(address) => Ok(Some(address)),
            // P4.4: a dead name fails as an unknown name and is never tried as an hc
            // address after that.
            NameLookup::Dead => bail!("no pane registered with name {name:?}"),
            NameLookup::Unknown => Ok(None),
        }
    }

    fn create_registered(
        &self,
        command: &str,
        split: Option<SplitDirection>,
        target: Option<&str>,
        registration: &Registration<'_>,
    ) -> Result<(String, String)> {
        let (space, tab) = self.placement(split, target)?;
        if let Some(name) = registration.name {
            // P4.4: dead terminals that carry the name are removed before the spawn.
            if let NameLookup::Live(_) = self.lookup(name)? {
                return Err(NameTaken(name.to_owned()).into());
            }
            return match self.run_terminal(
                name,
                command,
                space.as_deref(),
                tab.as_deref(),
                registration,
            )? {
                Some(address) => Ok((address, name.to_owned())),
                None => Err(NameTaken(name.to_owned()).into()),
            };
        }
        for _ in 0..DERIVED_NAME_ATTEMPTS {
            let name = names::derive_name(registration.name_source);
            // P4.4: dead terminals that carry the derived name are removed first.
            if let NameLookup::Live(_) = self.lookup(&name)? {
                continue;
            }
            if let Some(address) = self.run_terminal(
                &name,
                command,
                space.as_deref(),
                tab.as_deref(),
                registration,
            )? {
                return Ok((address, name));
            }
        }
        bail!(
            "could not derive an unused name from {:?}; pass --name",
            registration.name_source
        )
    }

    fn history_cursor(&self, pane: &str) -> Result<String> {
        let read = self.read(pane, ALL_LINES)?;
        Ok(serde_json::to_string(&Mark {
            lines: read.lines().into_iter().map(str::to_owned).collect(),
            history: read.is_history(),
        })?)
    }

    fn read_history_since(&self, pane: &str, cursor: &str) -> Result<String> {
        Ok(self.read_history_since_complete(pane, cursor)?.text)
    }

    fn read_history_since_complete(&self, pane: &str, cursor: &str) -> Result<HistorySince> {
        let mark: Mark = serde_json::from_str(cursor)
            .map_err(|error| anyhow!("not a herdr History mark: {cursor:?} ({error})"))?;
        let read = self.read(pane, ALL_LINES)?;
        let lines = read.lines();
        // P5.1: the result is the read after settle from the first changed line to its
        // end; it is complete only when both reads report `how: history` and the first
        // changed line is at least the mark read's line count minus `rows`. The
        // `complete` value `hc read` reports does not change Completeness.
        let first_changed = mark.first_changed_line(&lines);
        let both_history = mark.history && read.is_history();
        let complete = both_history && {
            let rows = self.get(pane)?.rows();
            history_mark::within_last_rows(first_changed, mark.lines.len(), rows)
        };
        let incomplete_reason = if !both_history {
            "herdr reports no history read for the pane".to_owned()
        } else {
            "the pane's history changed above its last screen after the mark".to_owned()
        };
        // P5.1: a `how: stitched` or `how: screen` read after settle is the whole result
        // (a stated exception: it does not start at the History mark).
        let text = if read.is_stitched_or_screen() {
            read.capture()
        } else {
            lines_text(lines.get(first_changed..).unwrap_or_default())
        };
        Ok(HistorySince {
            text,
            complete,
            incomplete_reason,
        })
    }

    fn prompt_delivery_guaranteed(&self, pane: &str, _agent: Option<&str>) -> Result<bool> {
        // P6.6: guaranteed for a pane whose program reports OSC 7501 Host state.
        Ok(self.get(pane)?.host_state().is_some())
    }

    fn submit_prompt(
        &self,
        pane: &str,
        text: &str,
        _wait: std::time::Duration,
    ) -> Result<Submission> {
        let baseline = self.get(pane)?;
        let sent_ms = now_ms();
        if let Err(error) = self.send(pane, text) {
            // P6.5: a blocked program refuses the send; no retry, never `--force`.
            if let Some(hc) = error.downcast_ref::<HcError>() {
                if hc.kind == "blocked" {
                    bail!("prompt target blocked: {}", hc.message);
                }
            }
            return Err(error);
        }
        *self.pending.lock().expect("pending prompt") = Some(Pending {
            pane: baseline.address().to_owned(),
            baseline_turn: baseline.turn(),
            sent_ms,
            confirmed: false,
        });
        self.get(pane)?;
        self.settle_submission(pane, Submission::Accepted)
    }

    fn settle_submission(&self, pane: &str, submit: Submission) -> Result<Submission> {
        if submit == Submission::Confirmed {
            return Ok(submit);
        }
        let confirmed = |host: &HerdrHost| {
            host.pending
                .lock()
                .expect("pending prompt")
                .as_ref()
                .is_some_and(|pending| pending.confirmed)
        };
        if !confirmed(self) {
            // One more read judges the terminal as it is at settle.
            self.get(pane)?;
        }
        Ok(if confirmed(self) {
            Submission::Confirmed
        } else {
            submit
        })
    }

    fn host_state_since(&self, pane: &str) -> Result<Option<(HostState, Option<u64>)>> {
        Ok(self.get(pane)?.host_state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hc_name_is_tt_and_16_hex_of_the_sha256_of_the_name() {
        // SHA-256("A.b") = 4ff54a775f90577e… (`printf A.b | shasum -a 256`).
        assert_eq!(hc_name("A.b"), "tt-4ff54a775f90577e");
        assert_eq!(hc_name("A.b").len(), 19);
    }
}
