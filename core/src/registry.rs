//! The registry of Pane records: the Pane registration of a host that cannot store
//! custom data on a pane (P4.3).
//!
//! One record per pane lives at
//! `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/<host>/<server_id>/<name>.json`.
//! A spawn reserves its name by creating the record exclusively before the pane exists
//! (`handle` is `None`), then writes the handle by write-to-temp and atomic rename
//! (P4.4). Every invocation that resolves or lists records checks them against the
//! host's pane list ([`check`], P4.5). There is no daemon and no lock.

use std::collections::HashSet;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::host::Host;
use crate::names::Registered;

/// The `schema` value of every Pane record.
pub const PANE_RECORD_SCHEMA: &str = "tmux-tools.pane.v1";

/// How long a record without a handle counts as a spawn in progress (P4.5).
pub const SPAWN_TIMEOUT: Duration = Duration::from_secs(120);

/// One Pane record: the registry file of one pane.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneRecord {
    pub schema: String,
    pub name: String,
    pub host: Host,
    pub server_id: String,
    /// The host's handle of the pane; `None` while the name is only reserved.
    pub handle: Option<String>,
    pub agent: Option<String>,
    pub access: Option<String>,
    pub surface: Option<String>,
    pub surface_unvalidated: bool,
    pub cwd: PathBuf,
    /// RFC 3339.
    pub launched_at: String,
    /// The name of the tmux-tools pane this pane was created from (PRD-261108-0447-01
    /// P7). Absent key = `None`, as for every field below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The team id of a Teammate pane (P6.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// The Shim id of a Teammate pane (P6.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shim_id: Option<String>,
    /// Display data from `select-pane -T` (P6.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Display data from `--agent-name` of the `respawn-pane` command (P6.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_agent_name: Option<String>,
    /// Display data from `--team-name` of the `respawn-pane` command (P6.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_team_name: Option<String>,
}

impl PaneRecord {
    /// The record as the Pane registration the verbs read.
    pub fn registered(&self) -> Registered {
        let cwd = self.cwd.to_string_lossy();
        Registered {
            name: Some(self.name.clone()),
            agent: self.agent.clone(),
            access: self.access.clone(),
            launched_at: Some(self.launched_at.clone()),
            cwd: (!cwd.is_empty()).then(|| cwd.into_owned()),
            surface: self.surface.clone(),
            surface_unvalidated: self.surface_unvalidated,
            parent: self.parent.clone(),
            team: self.team.clone(),
            shim_id: self.shim_id.clone(),
            title: self.title.clone(),
            claude_agent_name: self.claude_agent_name.clone(),
            claude_team_name: self.claude_team_name.clone(),
        }
    }
}

/// The three valid states of a Pane record against the host's pane list (P4.5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneLiveness {
    /// The host lists the record's handle.
    Live,
    /// The host's complete list does not list the handle: the record is deleted.
    Dead,
    /// The record has no handle and is older than [`SPAWN_TIMEOUT`]: it is deleted.
    OrphanReservation,
}

/// What [`check`] can say about one record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordCheck {
    /// One of the three valid states.
    Settled(PaneLiveness),
    /// The handle is not listed, but the list is incomplete: the record stays.
    Unlisted,
    /// No handle and younger than the spawn timeout: the record stays.
    SpawnInProgress,
}

/// Check one record against the host's pane list. `age` is how long ago the record
/// was created.
pub fn check(
    record: &PaneRecord,
    listed: &HashSet<String>,
    complete: bool,
    age: Duration,
) -> RecordCheck {
    match &record.handle {
        Some(handle) if listed.contains(handle) => RecordCheck::Settled(PaneLiveness::Live),
        Some(_) if complete => RecordCheck::Settled(PaneLiveness::Dead),
        Some(_) => RecordCheck::Unlisted,
        None if age >= SPAWN_TIMEOUT => RecordCheck::Settled(PaneLiveness::OrphanReservation),
        None => RecordCheck::SpawnInProgress,
    }
}

/// `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/<host>`.
pub fn host_dir(host: Host) -> Result<PathBuf> {
    Ok(crate::team::state_root()
        .map_err(|_| anyhow!("cannot locate the Pane registry: HOME is unset"))?
        .join(host.as_str()))
}

/// The registry directory of one host server.
pub fn server_dir(host: Host, server_id: &str) -> Result<PathBuf> {
    Ok(host_dir(host)?.join(server_id))
}

fn record_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

/// Delete every record directory of `host` whose name is not `server_id` (P4.3).
pub fn sweep_other_servers(host: Host, server_id: &str) -> Result<()> {
    let dir = host_dir(host)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() != server_id && entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("delete stale registry {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Reserve `record.name` by creating its record exclusively. A name already reserved
/// fails.
///
/// On Rex the server directory is never made here: the backend publishes it first
/// (PRD-261112-2108-01 P4.1).
pub fn reserve(record: &PaneRecord) -> Result<()> {
    let dir = server_dir(record.host, &record.server_id)?;
    if record.host != Host::Rex {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let path = record_path(&dir, &record.name);
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return Err(NameTaken(record.name.clone()).into());
        }
        Err(error) => return Err(error).with_context(|| format!("create {}", path.display())),
    };
    file.write_all(&serde_json::to_vec(record)?)
        .with_context(|| format!("write {}", path.display()))
}

/// A spawn's name is already reserved by another record.
#[derive(Debug)]
pub struct NameTaken(pub String);

impl std::fmt::Display for NameTaken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "name '{}' is already used by another pane; pick a different name or kill the existing pane",
            self.0
        )
    }
}

impl std::error::Error for NameTaken {}

/// Replace the record of `record.name` by write-to-temp and atomic rename. On Rex the
/// server directory is never made here (see [`reserve`]).
pub fn write(record: &PaneRecord) -> Result<()> {
    let dir = server_dir(record.host, &record.server_id)?;
    if record.host != Host::Rex {
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let path = record_path(&dir, &record.name);
    let temp = dir.join(format!(".{}.{}.tmp", record.name, std::process::id()));
    fs::write(&temp, serde_json::to_vec(record)?)
        .with_context(|| format!("write {}", temp.display()))?;
    fs::rename(&temp, &path).with_context(|| format!("rename {}", path.display()))
}

/// Delete the record of `name`; an absent record is not an error.
pub fn delete(host: Host, server_id: &str, name: &str) -> Result<()> {
    let path = record_path(&server_dir(host, server_id)?, name);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("delete {}", path.display())),
    }
}

/// The current record of `name`, if a valid one exists.
pub fn load(host: Host, server_id: &str, name: &str) -> Result<Option<PaneRecord>> {
    let path = record_path(&server_dir(host, server_id)?, name);
    match fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// Every record of one host server, each with its age, sorted by name. A file that is
/// not a valid record is skipped.
pub fn load_all(host: Host, server_id: &str) -> Result<Vec<(PaneRecord, Duration)>> {
    let dir = server_dir(host, server_id)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
    };
    let now = SystemTime::now();
    let mut records = Vec::new();
    for entry in entries {
        let path = entry?.path();
        // A record is `<name>.json` for any valid name, a leading dot included; the
        // temp file of [`write`] ends in `.tmp` and is never read.
        let is_record = path.extension().is_some_and(|ext| ext == "json");
        if !is_record {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<PaneRecord>(&bytes) else {
            continue;
        };
        let age = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or_default();
        records.push((record, age));
    }
    records.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(handle: Option<&str>) -> PaneRecord {
        PaneRecord {
            schema: PANE_RECORD_SCHEMA.to_owned(),
            name: "a".to_owned(),
            host: Host::Orca,
            server_id: "s".to_owned(),
            handle: handle.map(str::to_owned),
            agent: None,
            access: None,
            surface: None,
            surface_unvalidated: false,
            cwd: PathBuf::from("/w"),
            launched_at: "2026-01-01T00:00:00Z".to_owned(),
            parent: None,
            team: None,
            shim_id: None,
            title: None,
            claude_agent_name: None,
            claude_team_name: None,
        }
    }

    #[test]
    fn check_covers_every_state() {
        let listed: HashSet<String> = ["term_1".to_owned()].into();
        let young = Duration::from_secs(1);
        let old = SPAWN_TIMEOUT;
        assert_eq!(
            check(&record(Some("term_1")), &listed, false, young),
            RecordCheck::Settled(PaneLiveness::Live)
        );
        assert_eq!(
            check(&record(Some("term_2")), &listed, true, young),
            RecordCheck::Settled(PaneLiveness::Dead)
        );
        assert_eq!(
            check(&record(Some("term_2")), &listed, false, young),
            RecordCheck::Unlisted
        );
        assert_eq!(
            check(&record(None), &listed, true, old),
            RecordCheck::Settled(PaneLiveness::OrphanReservation)
        );
        assert_eq!(
            check(&record(None), &listed, true, young),
            RecordCheck::SpawnInProgress
        );
    }

    #[test]
    fn record_json_uses_the_field_names_and_a_lowercase_host() {
        let json = serde_json::to_value(record(None)).unwrap();
        assert_eq!(json["host"], "orca");
        assert_eq!(json["schema"], PANE_RECORD_SCHEMA);
        assert!(json["handle"].is_null());
        assert_eq!(json["surface_unvalidated"], false);
        // PRD-261108-0447-01: an absent team field is an absent key.
        assert!(json.get("parent").is_none() && json.get("team").is_none());
    }
}
