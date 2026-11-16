//! The Shim: `tmux-tools` started with `argv[0]` basename `tmux` (PRD-261108-0447-01
//! P5, P6).
//!
//! It serves the tmux calls of the Claude that a Launcher started, acting only on the
//! panes of its own Team, through the Host port: it never runs tmux itself and builds
//! no host command. It exits only 0 or 1 and writes its errors to stderr as
//! `tmux: <message>`. Every exit 1 is a refusal, logged to `shim.log` (P5.9).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tmux_tools_core::{
    host::{self, Host, HostPort, ListScope, RegistrationKey, SplitDirection},
    layout::{Registration, TeammateRegistration},
    names::{self, Registered},
    registry::NameTaken,
    team::{self, TeamRecord, LEADER_SHIM_ID},
};

use crate::util::rfc3339_utc_now;

/// The version the Shim reports for `-V`.
const TMUX_VERSION: &str = "tmux 3.4";
/// The one fake window of every Team (P5.4).
const WINDOW_ID: &str = "@0";
/// The longest Teammate pane name built from the Leader name (P6.1).
const MAX_NAME_BYTES: usize = 100;
/// The `-e` variables the Shim drops on `host`: a teammate gets its own pane's values
/// (P6.5; Herdr: PRD-261115-0921-01 P4.1).
fn dropped_env(host: Host) -> &'static [&'static str] {
    match host {
        Host::Herdr => &["HC_TERMINAL_ID", "HC_SCOPE"],
        Host::Tmux | Host::Orca | Host::Rex => {
            &["ORCA_PANE_KEY", "ORCA_TERMINAL_HANDLE", "ORCA_TAB_ID"]
        }
    }
}

/// An exit 1 of the Shim: its message, printed as `tmux: <message>`.
#[derive(Debug)]
struct Refusal(String);

impl<E: std::fmt::Display> From<E> for Refusal {
    fn from(error: E) -> Refusal {
        Refusal(error.to_string())
    }
}

fn refuse<T>(message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(message.into()))
}

type Outcome = std::result::Result<String, Refusal>;

/// Run the Shim with `argv` (after `argv[0]`); returns the exit code.
pub fn run(argv: Vec<String>) -> i32 {
    // P5.1: outside a team the Shim refuses and writes no log line.
    let Some(team_id) = team::current_id() else {
        eprintln!("tmux: not inside a tmux-tools team");
        return 1;
    };
    // P3.2: no team directory, so no log line either.
    if !team::team_dir(&team_id).is_ok_and(|dir| dir.join("team.json").exists()) {
        eprintln!("tmux: team not found: {team_id}");
        return 1;
    }
    let outcome = match host::selection() {
        Ok(_) => serve(&argv),
        Err(error) => Err(Refusal(format!("{error:#}"))),
    };
    let (exit, stdout, message) = match &outcome {
        Ok(stdout) => (0, stdout.as_str(), ""),
        Err(Refusal(message)) => (1, "", message.as_str()),
    };
    print!("{stdout}");
    let _ = std::io::stdout().flush();
    if exit != 0 {
        eprintln!("tmux: {message}");
    }
    log(&team_id, &argv, exit, stdout, message);
    exit
}

/// One `shim.log` line (P5.9).
#[derive(Serialize)]
struct LogLine<'a> {
    time: String,
    pid: u32,
    outcome: &'a str,
    message: &'a str,
    argv: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    exit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<&'a str>,
}

/// Append one line for a refusal, or for every call with `TT_SHIM_DEBUG=1`.
fn log(team_id: &str, argv: &[String], exit: i32, stdout: &str, message: &str) {
    let debug = team::shim_debug();
    if exit == 0 && !debug {
        return;
    }
    let line = LogLine {
        time: rfc3339_utc_now().unwrap_or_default(),
        pid: std::process::id(),
        outcome: if exit == 0 { "ok" } else { "refused" },
        message,
        argv,
        exit: debug.then_some(exit),
        stdout: debug.then_some(stdout),
    };
    let Ok(path) = team::shim_log_path(team_id) else {
        return;
    };
    let Ok(mut text) = serde_json::to_string(&line) else {
        return;
    };
    text.push('\n');
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

// --- Parsing (P5.3) ----------------------------------------------------------------

/// The flags of one command: each flag letter and whether it takes a value.
type FlagSpec = &'static [(char, bool)];

#[derive(Debug, Default)]
struct Parsed {
    flags: Vec<(char, Option<String>)>,
    rest: Vec<String>,
}

impl Parsed {
    fn has(&self, flag: char) -> bool {
        self.flags.iter().any(|(letter, _)| *letter == flag)
    }

    fn value(&self, flag: char) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(letter, _)| *letter == flag)
            .and_then(|(_, value)| value.as_deref())
    }

    fn values(&self, flag: char) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(letter, _)| *letter == flag)
            .filter_map(|(_, value)| value.as_deref())
            .collect()
    }
}

/// Parse flag clusters with `spec` until `--` or the first word that is not a flag.
fn parse_flags(words: &[String], spec: FlagSpec) -> Result<Parsed, Refusal> {
    let mut parsed = Parsed::default();
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            index += 1;
            break;
        }
        if !word.starts_with('-') || word == "-" {
            break;
        }
        let letters: Vec<char> = word[1..].chars().collect();
        let mut position = 0;
        while position < letters.len() {
            let letter = letters[position];
            let Some((_, takes_value)) = spec.iter().find(|(flag, _)| *flag == letter) else {
                return refuse(format!("unsupported flag -{letter}"));
            };
            if *takes_value {
                let rest: String = letters[position + 1..].iter().collect();
                let value = if rest.is_empty() {
                    index += 1;
                    match words.get(index) {
                        Some(value) => value.clone(),
                        None => return refuse(format!("unsupported flag -{letter}")),
                    }
                } else {
                    rest
                };
                parsed.flags.push((letter, Some(value)));
                break;
            }
            parsed.flags.push((letter, None));
            position += 1;
        }
        index += 1;
    }
    parsed.rest = words[index..].to_vec();
    Ok(parsed)
}

/// Apply the separator rule of P5.3: a word `;`, or a word ending in `;` with no `\`
/// before it, is a command separator (refused, P5.8); a trailing `\;` is a literal `;`.
fn unescape_separators(words: &[String]) -> Result<Vec<String>, Refusal> {
    words
        .iter()
        .map(|word| {
            if let Some(stem) = word.strip_suffix("\\;") {
                Ok(format!("{stem};"))
            } else if word.ends_with(';') {
                refuse("unsupported command: ;")
            } else {
                Ok(word.clone())
            }
        })
        .collect()
}

fn serve(argv: &[String]) -> Outcome {
    let global = parse_flags(argv, &[('S', true), ('L', true), ('f', true), ('V', false)])?;
    if global.has('V') {
        return Ok(format!("{TMUX_VERSION}\n"));
    }
    let words = unescape_separators(&global.rest)?;
    let Some((command, args)) = words.split_first() else {
        return refuse("unsupported command: new-session");
    };
    match command.as_str() {
        // P5.7: accepted and ignored with every flag and argument.
        "set-option" | "select-layout" | "resize-pane" => Ok(String::new()),
        "display-message" => display_message(&parse_flags(args, &[('p', false), ('t', true)])?),
        "list-panes" => list_panes(&parse_flags(args, &[('t', true), ('F', true)])?),
        "split-window" => split_window(&parse_flags(
            args,
            &[
                ('d', false),
                ('h', false),
                ('v', false),
                ('b', false),
                ('f', false),
                ('l', true),
                ('c', true),
                ('e', true),
                ('P', false),
                ('F', true),
                ('t', true),
            ],
        )?),
        "respawn-pane" => respawn_pane(&parse_flags(
            args,
            &[('k', false), ('c', true), ('e', true), ('t', true)],
        )?),
        "kill-pane" => kill_pane(&parse_flags(args, &[('t', true)])?),
        "send-keys" => send_keys(&parse_flags(args, &[('l', false), ('t', true)])?),
        "capture-pane" => capture_pane(&parse_flags(
            args,
            &[
                ('p', false),
                ('J', false),
                ('S', true),
                ('E', true),
                ('t', true),
            ],
        )?),
        "has-session" => has_session(&parse_flags(args, &[('t', true)])?),
        "select-pane" => select_pane(&parse_flags(args, &[('T', true), ('t', true)])?),
        other => refuse(format!("unsupported command: {other}")),
    }
}

// --- The Team and its panes ----------------------------------------------------------

/// One pane of the Team as Claude sees it.
#[derive(Clone, Debug)]
struct Member {
    /// `%1` for the Leader, `%<k+1>` for teammate `k`.
    shim_id: String,
    /// The host handle.
    handle: String,
    /// 0 for the Leader, else the teammate number `k`.
    number: u32,
    title: Option<String>,
}

impl Member {
    fn is_leader(&self) -> bool {
        self.number == 0
    }
}

struct Team {
    record: &'static TeamRecord,
    host: &'static dyn HostPort,
}

fn team() -> Result<Team, Refusal> {
    let record = team::current()?.ok_or_else(|| Refusal("not inside a tmux-tools team".into()))?;
    Ok(Team {
        record,
        host: host::current(),
    })
}

/// The teammate number of a Shim id `%<k+1>`.
fn teammate_number(shim_id: &str) -> Option<u32> {
    shim_id
        .strip_prefix('%')?
        .parse::<u32>()
        .ok()
        .filter(|n| *n >= 2)
        .map(|n| n - 1)
}

impl Team {
    fn session(&self) -> String {
        format!("tt-team-{}", self.record.id)
    }

    fn leader(&self) -> Member {
        Member {
            shim_id: LEADER_SHIM_ID.to_owned(),
            handle: self.record.leader.handle.clone(),
            number: 0,
            title: None,
        }
    }

    /// The live panes of the Team: the Leader, then the Teammate panes by number.
    fn members(&self) -> Result<Vec<Member>, Refusal> {
        let mut teammates: Vec<Member> = Vec::new();
        for row in self.host.list_panes(ListScope::All)? {
            if row.team.as_deref() != Some(self.record.id.as_str()) || row.pane_dead || row.unknown
            {
                continue;
            }
            let Some(shim_id) = row.shim_id.clone() else {
                continue;
            };
            let Some(number) = teammate_number(&shim_id) else {
                continue;
            };
            teammates.push(Member {
                shim_id,
                handle: row.pane_id.clone(),
                number,
                title: None,
            });
        }
        teammates.sort_by_key(|member| member.number);
        let mut members = vec![self.leader()];
        members.extend(teammates);
        Ok(members)
    }

    /// The pane a target names (P5.4); a window or session target means the Leader.
    /// A target outside the Team is `can't find pane: <target>` (P5.2).
    fn resolve(&self, target: Option<&str>) -> Result<Member, Refusal> {
        let Some(target) = target else {
            return Ok(self.leader());
        };
        let not_found = || Refusal(format!("can't find pane: {target}"));
        let session = self.session();
        let local = match target.strip_prefix(session.as_str()) {
            Some("") => return Ok(self.leader()),
            Some(rest) => rest.strip_prefix(':').ok_or_else(not_found)?,
            None => target,
        };
        let pane = match local {
            WINDOW_ID => return Ok(self.leader()),
            local => match local.strip_prefix("@0.") {
                Some(pane) => pane,
                None => local,
            },
        };
        if !pane.starts_with('%') {
            return Err(not_found());
        }
        if pane == LEADER_SHIM_ID {
            return Ok(self.leader());
        }
        self.members()?
            .into_iter()
            .find(|member| member.shim_id == pane)
            .ok_or_else(not_found)
    }

    /// The registration of `member`, its stored title included.
    fn registration(&self, member: &Member) -> Result<Registered, Refusal> {
        Ok(self.host.read_registration(&member.handle)?)
    }

    /// Expand a format for `member` (P5.5).
    fn expand(&self, format: &str, member: &Member) -> Result<String, Refusal> {
        let title = match &member.title {
            Some(title) => title.clone(),
            None if member.is_leader() => String::new(),
            None => self.registration(member)?.title.unwrap_or_default(),
        };
        Ok(expand_format(format, |variable| {
            match variable {
                "pane_id" => member.shim_id.clone(),
                "window_id" => WINDOW_ID.to_owned(),
                "window_index" => "0".to_owned(),
                "window_name" => "team".to_owned(),
                "session_name" => self.session(),
                "session_id" => "$0".to_owned(),
                "pane_title" => title.clone(),
                "allow-passthrough" | "focus-events" | "status" => "off".to_owned(),
                "pane_top" | "pane_left" | "window_bigger" => "0".to_owned(),
                "status-position" => "bottom".to_owned(),
                // `client_termtype` and every other variable expand to nothing.
                _ => String::new(),
            }
        }))
    }
}

/// Expand every `#{…}` of `format` with `value`; text outside `#{…}` is copied.
fn expand_format(format: &str, value: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = format;
    while let Some(start) = rest.find("#{") {
        out.push_str(&rest[..start]);
        let body = &rest[start + 2..];
        let mut depth = 1;
        let mut end = None;
        for (offset, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => {
                out.push_str(&value(&body[..end]));
                rest = &body[end + 1..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

// --- Commands (P5.6) ------------------------------------------------------------------

fn display_message(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let member = team.resolve(parsed.value('t'))?;
    if !parsed.has('p') {
        return Ok(String::new());
    }
    Ok(format!(
        "{}\n",
        team.expand(&parsed.rest.join(" "), &member)?
    ))
}

fn list_panes(parsed: &Parsed) -> Outcome {
    let team = team()?;
    team.resolve(parsed.value('t'))?;
    let format = parsed.value('F').unwrap_or("#{pane_id}");
    let mut out = String::new();
    for member in team.members()? {
        out.push_str(&team.expand(format, &member)?);
        out.push('\n');
    }
    Ok(out)
}

fn has_session(parsed: &Parsed) -> Outcome {
    let team = team()?;
    match parsed.value('t') {
        None => Ok(String::new()),
        Some(target) => {
            let session = target.strip_prefix('=').unwrap_or(target);
            let session = session.split(':').next().unwrap_or_default();
            if session == team.session() {
                Ok(String::new())
            } else {
                refuse(format!("can't find session: {target}"))
            }
        }
    }
}

fn select_pane(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let member = team.resolve(parsed.value('t'))?;
    let Some(title) = parsed.value('T') else {
        return Ok(String::new());
    };
    if !member.is_leader() {
        team.host
            .write_registration_value(&member.handle, RegistrationKey::Title, title)?;
    }
    Ok(String::new())
}

fn send_keys(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let member = team.resolve(parsed.value('t'))?;
    if parsed.has('l') {
        team.host.send_text(&member.handle, &parsed.rest.concat())?;
    } else {
        let keys: Vec<&str> = parsed.rest.iter().map(String::as_str).collect();
        team.host.send_keys(&member.handle, &keys)?;
    }
    Ok(String::new())
}

fn capture_pane(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let member = team.resolve(parsed.value('t'))?;
    if !parsed.has('p') {
        return refuse("capture-pane needs -p");
    }
    let line = |value: &str| -> Result<i64, Refusal> {
        value
            .parse::<i64>()
            .map_err(|_| Refusal(format!("unsupported flag value {value}")))
    };
    let start = parsed
        .value('S')
        .filter(|start| *start != "-")
        .map(line)
        .transpose()?;
    if !team.host.addresses_lines() {
        // A host without line addressing (Orca, Herdr): `-S -` reads all history, and a
        // negative `-S` reads that many history lines plus the visible screen (P5.6,
        // PRD-261115-0921-01 P3.5).
        let range = match (parsed.value('S'), start) {
            (Some("-"), _) => host::CaptureRange::All,
            (_, Some(start)) if start < 0 => {
                let visible = team
                    .host
                    .capture(&member.handle, host::CaptureRange::Visible)?
                    .lines()
                    .count() as u64;
                // Beyond the `Tail` limit, the request covers all history.
                u32::try_from(start.unsigned_abs().saturating_add(visible))
                    .map_or(host::CaptureRange::All, host::CaptureRange::Tail)
            }
            _ => host::CaptureRange::Visible,
        };
        return Ok(team.host.capture(&member.handle, range)?);
    }
    let range = match (parsed.value('S'), parsed.value('E')) {
        (None, None) => host::CaptureRange::Visible,
        (Some("-"), _) => host::CaptureRange::All,
        (start, end) => host::CaptureRange::Rows {
            start: start.map(line).transpose()?.unwrap_or(0),
            end: match end {
                None | Some("-") => i64::from(u16::MAX),
                Some(end) => line(end)?,
            },
        },
    };
    Ok(team.host.capture(&member.handle, range)?)
}

fn kill_pane(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let member = team.resolve(parsed.value('t'))?;
    if member.is_leader() {
        return refuse("can't kill leader pane");
    }
    let _lock = team::lock_team(&team.record.id)?;
    team.host.close_pane(&member.handle, None)?;
    Ok(String::new())
}

/// The pane command of P6.1: `cd '<dir>' && env K=V … <command>`, with the `-e`
/// settings of [`dropped_env`] of `host` dropped (P6.5). Without a command, the
/// caller's login shell.
fn pane_command(parsed: &Parsed, host: Host) -> String {
    let command = match parsed.rest.as_slice() {
        [] => std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_owned()),
        [one] => one.clone(),
        many => many
            .iter()
            .map(|word| shell_quote(word))
            .collect::<Vec<_>>()
            .join(" "),
    };
    let settings: Vec<String> = parsed
        .values('e')
        .into_iter()
        .filter(|setting| {
            let key = setting.split('=').next().unwrap_or_default();
            !dropped_env(host).contains(&key)
        })
        .map(shell_quote)
        .collect();
    let command = if settings.is_empty() {
        command
    } else {
        format!("env {} {command}", settings.join(" "))
    };
    match parsed.value('c') {
        Some(dir) => format!("cd {} && {command}", shell_quote(dir)),
        None => command,
    }
}

/// The Close-on-success wrap (P6.3): exit 0 closes the pane; any other exit keeps a
/// login shell in it. The same on every host.
fn close_on_success(command: &str) -> String {
    let script = format!("{command} || exec \"${{SHELL:-/bin/sh}}\" -l");
    format!("exec sh -c {}", shell_quote(&script))
}

/// POSIX single-quote `value`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Whether a host create failed because the window has no space (P6.1): tmux reports
/// `no space for new pane` (`no space for a new pane` in tmux 3.5 and later).
fn no_space(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}");
    message.contains("no space for new pane") || message.contains("no space for a new pane")
}

/// The fields a Teammate pane carries (P6.4).
#[derive(Clone, Debug, Default)]
struct TeammateFields {
    name: String,
    parent: Option<String>,
    shim_id: String,
    launched_at: String,
    cwd: Option<String>,
    title: Option<String>,
    agent: Option<String>,
    claude_agent_name: Option<String>,
    claude_team_name: Option<String>,
    access: Option<String>,
    surface: Option<String>,
    surface_unvalidated: bool,
}

impl Team {
    /// Create a pane running `command` by splitting `target` in `direction`, and give
    /// it `fields` as its Pane registration. Returns the new handle. With `detached`
    /// the new pane does not become active, on a host that supports it (M3: tmux).
    fn create_teammate(
        &self,
        target: &str,
        direction: SplitDirection,
        command: &str,
        fields: &TeammateFields,
        detached: bool,
    ) -> std::result::Result<String, anyhow::Error> {
        // PRD-261115-0921-01 P3.1: a host that creates panes only with their
        // registration (Herdr), or keeps Pane records, takes the registered create.
        if self.host.creates_registered() {
            let (handle, _) = self.host.create_registered(
                command,
                Some(direction),
                Some(target),
                &Registration {
                    name: Some(&fields.name),
                    name_source: "",
                    launched_at: &fields.launched_at,
                    cwd: fields.cwd.as_deref(),
                    agent: None,
                    parent: fields.parent.as_deref(),
                    team: Some(TeammateRegistration {
                        team: &self.record.id,
                        shim_id: &fields.shim_id,
                        title: fields.title.as_deref(),
                        claude_agent_name: fields.claude_agent_name.as_deref(),
                        claude_team_name: fields.claude_team_name.as_deref(),
                        agent: fields.agent.as_deref(),
                    }),
                },
            )?;
            return Ok(handle);
        }
        let handle = self
            .host
            .create_split(target, direction, None, command, detached)?;
        let mut writes: Vec<(RegistrationKey, String)> = vec![
            (names::KEY_NAME, fields.name.clone()),
            (names::KEY_LAUNCHED_AT, fields.launched_at.clone()),
            (names::KEY_TEAM, self.record.id.clone()),
            (names::KEY_SHIM_ID, fields.shim_id.clone()),
        ];
        let optional = [
            (names::KEY_CWD, &fields.cwd),
            (names::KEY_PARENT, &fields.parent),
            (names::KEY_TITLE, &fields.title),
            (names::KEY_AGENT, &fields.agent),
            (names::KEY_CLAUDE_AGENT_NAME, &fields.claude_agent_name),
            (names::KEY_CLAUDE_TEAM_NAME, &fields.claude_team_name),
            (names::KEY_ACCESS, &fields.access),
            (names::KEY_SURFACE, &fields.surface),
        ];
        for (key, value) in optional {
            if let Some(value) = value {
                writes.push((key, value.clone()));
            }
        }
        if fields.surface_unvalidated {
            writes.push((names::KEY_SURFACE_UNVALIDATED, "1".to_owned()));
        }
        for (key, value) in writes {
            self.host.write_registration_value(&handle, key, &value)?;
        }
        Ok(handle)
    }

    /// The Leader name at this call: the name of the Leader pane's Pane registration
    /// (P2.8); `None` when the Leader is not a tmux-tools pane.
    fn leader_name(&self) -> Result<Option<String>, Refusal> {
        Ok(self.registration(&self.leader())?.name)
    }

    /// Whether `name` is already taken by a pane or a reservation.
    fn name_taken(&self, name: &str) -> Result<bool, Refusal> {
        Ok(self.host.find_registered(name)?.is_some())
    }
}

/// The Teammate pane name for `k` (P6.1).
fn teammate_name(leader: Option<&str>, team_id: &str, k: u32) -> String {
    if let Some(leader) = leader {
        let name = format!("{leader}.tm{k}");
        if name.len() <= MAX_NAME_BYTES && names::validate_name(&name).is_ok() {
            return name;
        }
    }
    format!("team-{team_id}.tm{k}")
}

fn split_window(parsed: &Parsed) -> Outcome {
    let team = team()?;
    team.resolve(parsed.value('t'))?;
    let _lock = team::lock_team(&team.record.id)?;
    let dir = team::team_dir(&team.record.id)?;
    let mut record = team::load_from(&dir)?;
    let members = team.members()?;
    let teammates: Vec<&Member> = members.iter().filter(|m| !m.is_leader()).collect();

    // P6.1: right of the Leader first; then below the tallest teammate (tmux) or the
    // highest-numbered one (a host without `tile`).
    let (target, direction) = match teammates.last() {
        None => (
            team.record.leader.handle.clone(),
            SplitDirection::Horizontal,
        ),
        Some(highest) if team.host.capabilities().tile => {
            let geometry = team.host.window_geometry(&team.record.leader.handle)?;
            let tallest = teammates
                .iter()
                .filter_map(|member| {
                    geometry
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == member.handle)
                        .map(|pane| (pane.rows, member.number, member.handle.clone()))
                })
                .max_by_key(|(rows, number, _)| (*rows, *number))
                .map(|(_, _, handle)| handle)
                .unwrap_or_else(|| highest.handle.clone());
            (tallest, SplitDirection::Vertical)
        }
        Some(highest) => (highest.handle.clone(), SplitDirection::Vertical),
    };

    let leader_name = team.leader_name()?;
    let command = close_on_success(&pane_command(parsed, team.host.host()));
    let launched_at = rfc3339_utc_now()?;
    let cwd = std::env::current_dir()
        .ok()
        .map(|cwd| cwd.to_string_lossy().into_owned());
    let mut k = record.next_teammate.max(1);
    let handle = loop {
        let name = teammate_name(leader_name.as_deref(), &record.id, k);
        if team.name_taken(&name)? {
            k += 1;
            continue;
        }
        let fields = TeammateFields {
            name,
            parent: leader_name.clone(),
            shim_id: format!("%{}", k + 1),
            launched_at: launched_at.clone(),
            cwd: cwd.clone(),
            ..TeammateFields::default()
        };
        match team.create_teammate(&target, direction, &command, &fields, parsed.has('d')) {
            Ok(handle) => break handle,
            Err(error) if error.downcast_ref::<NameTaken>().is_some() => k += 1,
            Err(error) if no_space(&error) => return refuse("no space for a new pane"),
            Err(error) => return Err(Refusal(format!("{error:#}"))),
        }
    };
    record.next_teammate = k + 1;
    team::write_to(&dir, &record)?;

    if !parsed.has('P') {
        return Ok(String::new());
    }
    let member = Member {
        shim_id: format!("%{}", k + 1),
        handle,
        number: k,
        title: Some(String::new()),
    };
    let format = parsed.value('F').unwrap_or("#{pane_id}");
    Ok(format!("{}\n", team.expand(format, &member)?))
}

fn respawn_pane(parsed: &Parsed) -> Outcome {
    let team = team()?;
    let old = team.resolve(parsed.value('t'))?;
    if !parsed.has('k') {
        return refuse("respawn-pane needs -k");
    }
    if old.is_leader() {
        return refuse("can't respawn leader pane");
    }
    let _lock = team::lock_team(&team.record.id)?;
    let registered = team.registration(&old)?;
    let claude = command_words(&parsed.rest)
        .and_then(|words| claude_command(&words, &team.record.claude.path));
    let read = |flag: &str| claude.as_ref().and_then(|words| flag_value(words, flag));
    let name = registered.name.clone().unwrap_or_default();
    let mut fields = TeammateFields {
        name: name.clone(),
        parent: registered.parent.clone(),
        shim_id: old.shim_id.clone(),
        launched_at: registered.launched_at.clone().unwrap_or_default(),
        cwd: registered.cwd.clone(),
        title: registered.title.clone(),
        agent: claude.as_ref().map(|_| "claude".to_owned()),
        claude_agent_name: read("--agent-name"),
        claude_team_name: read("--team-name"),
        access: registered.access.clone(),
        surface: registered.surface.clone(),
        surface_unvalidated: registered.surface_unvalidated,
    };
    let command = close_on_success(&pane_command(parsed, team.host.host()));
    // On a registry host the old record holds the name until the old pane is closed,
    // and Herdr refuses a second live terminal with the name (PRD-261115-0921-01 P5),
    // so the new pane is registered under a transitional name and renamed after.
    let transitional = team.host.creates_registered();
    if transitional {
        fields.name = format!("{name}.respawn-{}", std::process::id());
    }
    let handle = match team.create_teammate(
        &old.handle,
        SplitDirection::Vertical,
        &command,
        &fields,
        true,
    ) {
        Ok(handle) => handle,
        Err(error) if no_space(&error) => return refuse("no space for a new pane"),
        Err(error) => return Err(Refusal(format!("{error:#}"))),
    };
    team.host.close_pane(&old.handle, None)?;
    if transitional {
        if let Err(error) =
            team.host
                .write_registration_value(&handle, RegistrationKey::Name, &name)
        {
            // A Herdr Teammate terminal is removed when its command exits 0
            // (PRD-261115-0921-01 P6.1), possibly before the name is written; the
            // create succeeded and the old terminal is closed, so the respawn is
            // complete (PRD-261108-0447-01 P6.2).
            let removed = team.host.host() == Host::Herdr
                && matches!(team.host.pane_exists(&handle), Ok(false));
            if !removed {
                return Err(error.into());
            }
        }
    }
    Ok(String::new())
}

// --- Reading the respawn command (P6.4) ----------------------------------------------

/// The words of the `respawn-pane` command: one word is split with POSIX quoting
/// limited to blanks, single quotes, double quotes, and backslash, with no expansion;
/// several words are taken as they are.
fn command_words(rest: &[String]) -> Option<Vec<String>> {
    match rest {
        [one] => split_words(one),
        many => Some(many.to_vec()),
    }
}

/// Split shell text into words (no expansion). The unquoted words `&&`, `||`, `;`, and
/// `|` stay separate words. `None` when the split fails: an unclosed single or double
/// quote, or a trailing backslash (P6.4).
fn split_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | '\t' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        closed = true;
                        break;
                    }
                    word.push(inner);
                }
                if !closed {
                    return None;
                }
            }
            '"' => {
                in_word = true;
                let mut closed = false;
                while let Some(inner) = chars.next() {
                    match inner {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.peek() {
                            Some(&next @ ('"' | '\\' | '$' | '`')) => {
                                word.push(next);
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        other => word.push(other),
                    }
                }
                if !closed {
                    return None;
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next()?);
            }
            ';' | '&' | '|' if !in_word => {
                let mut op = ch.to_string();
                if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                    op.push(ch);
                    chars.next();
                }
                words.push(op);
            }
            ';' | '&' | '|' => {
                words.push(std::mem::take(&mut word));
                in_word = false;
                let mut op = ch.to_string();
                if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                    op.push(ch);
                    chars.next();
                }
                words.push(op);
            }
            other => {
                in_word = true;
                word.push(other);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Some(words)
}

/// The words of the first command that runs the Team's Claude binary, after `cd`
/// commands and after `env` and its `K=V` words.
fn claude_command(words: &[String], claude: &Path) -> Option<Vec<String>> {
    words
        .split(|word| matches!(word.as_str(), "&&" | "||" | ";" | "|"))
        .find_map(|command| {
            let mut rest = command;
            if rest.first().is_some_and(|word| word == "cd") {
                return None;
            }
            if rest.first().is_some_and(|word| word == "env") {
                rest = &rest[1..];
                while rest
                    .first()
                    .is_some_and(|word| !word.starts_with('-') && word.contains('='))
                {
                    rest = &rest[1..];
                }
            }
            let program = rest.first()?;
            runs_claude(program, claude).then(|| rest.to_vec())
        })
}

/// Whether `program` is the Team's Claude binary, or canonicalizes to it. A word
/// without `/` is resolved through the Leader's `PATH`.
fn runs_claude(program: &str, claude: &Path) -> bool {
    if Path::new(program) == claude {
        return true;
    }
    let path = if program.contains('/') {
        Some(PathBuf::from(program))
    } else {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(program))
                .find(|candidate| candidate.is_file())
        })
    };
    path.and_then(|path| std::fs::canonicalize(path).ok())
        .is_some_and(|canonical| canonical == claude)
}

/// The value of `--flag <v>` or `--flag=<v>` among `words`.
fn flag_value(words: &[String], flag: &str) -> Option<String> {
    let prefix = format!("{flag}=");
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        if word == flag {
            return iter.next().cloned();
        }
        if let Some(value) = word.strip_prefix(&prefix) {
            return Some(value.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn flag_clusters_take_values_from_the_rest_or_the_next_word() {
        let spec: FlagSpec = &[('d', false), ('t', true), ('l', true), ('P', false)];
        let parsed = parse_flags(&words(&["-dt%2", "-l", "30%", "-P", "cat"]), spec).unwrap();
        assert_eq!(parsed.value('t'), Some("%2"));
        assert_eq!(parsed.value('l'), Some("30%"));
        assert!(parsed.has('d') && parsed.has('P'));
        assert_eq!(parsed.rest, words(&["cat"]));
        let error = parse_flags(&words(&["-x"]), spec).unwrap_err();
        assert_eq!(error.0, "unsupported flag -x");
        let parsed = parse_flags(&words(&["--", "-d"]), spec).unwrap();
        assert_eq!(parsed.rest, words(&["-d"]));
    }

    #[test]
    fn separators_are_refused_and_escaped_ones_are_literal() {
        assert!(unescape_separators(&words(&[";"])).is_err());
        assert!(unescape_separators(&words(&["a;"])).is_err());
        assert_eq!(
            unescape_separators(&words(&["a\\;"])).unwrap(),
            words(&["a;"])
        );
    }

    #[test]
    fn formats_expand_known_variables_and_empty_others() {
        let out = expand_format("#{pane_id}:#{unknown}x#{a{b}}", |name| match name {
            "pane_id" => "%2".to_owned(),
            _ => String::new(),
        });
        assert_eq!(out, "%2:x");
    }

    #[test]
    fn the_claude_command_is_found_after_cd_and_env() {
        let claude = Path::new("/opt/claude/2.1.319");
        let split = split_words(
            "cd '/w d' && env A=1 'B=x y' /opt/claude/2.1.319 --agent-name 'r 1' --team-name=t",
        )
        .unwrap();
        let command = claude_command(&split, claude).unwrap();
        assert_eq!(flag_value(&command, "--agent-name").as_deref(), Some("r 1"));
        assert_eq!(flag_value(&command, "--team-name").as_deref(), Some("t"));
        assert!(claude_command(&split_words("cat").unwrap(), claude).is_none());
        for broken in [
            "/opt/claude/2.1.319 'open",
            "/opt/claude/2.1.319 \"open",
            "x \\",
        ] {
            assert_eq!(split_words(broken), None, "{broken}");
        }
    }

    #[test]
    fn dropped_env_settings_never_reach_the_pane_command() {
        let parsed = parse_flags(
            &words(&[
                "-e",
                "ORCA_PANE_KEY=a",
                "-e",
                "ORCA_TERMINAL_HANDLE=b",
                "-e",
                "ORCA_TAB_ID=c",
                "-e",
                "K=V",
                "cat",
            ]),
            &[('e', true)],
        )
        .unwrap();
        assert_eq!(pane_command(&parsed, Host::Orca), "env 'K=V' cat");
    }
}
