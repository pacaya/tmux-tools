//! Teams: one run of `tmux-tools claude-teams`, its Team record, and its team directory
//! (PRD-261108-0447-01 P4).
//!
//! A Team is one Leader and its Teammate panes. Its state is the Team record
//! (`team.json`) in the team directory
//! `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/teams/<team id>/`. There is no
//! daemon: a Team is running while its Launcher holds `leader.lock`. Inside a team
//! (`TT_TEAM` set), host selection, the calling pane, and the tmux endpoint come from
//! the Team record (P3).

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::host::Host;

/// The `schema` value of every Team record.
pub const TEAM_SCHEMA: &str = "tmux-tools.team.v1";
/// The team id of the invocation's Team (P2.3).
pub const TT_TEAM: &str = "TT_TEAM";
/// The selected host of the invocation's Team (P2.3, P3.1).
pub const TT_HOST: &str = "TT_HOST";
/// `1` logs every Shim call with its exit and stdout (P5.9) in the debug directory.
pub const TT_SHIM_DEBUG: &str = "TT_SHIM_DEBUG";
/// The Shim id of the Leader.
pub const LEADER_SHIM_ID: &str = "%1";

const TEAM_JSON: &str = "team.json";
const TEAM_LOCK: &str = "team.lock";
const LEADER_LOCK: &str = "leader.lock";
const SHIM_LOG: &str = "shim.log";

/// The state of one Team.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TeamRecord {
    pub schema: String,
    /// 8 lowercase hex digits.
    pub id: String,
    pub host: Host,
    /// The Orca `runtimeId`; on tmux the server socket path; on Herdr the core id.
    pub server_id: String,
    pub leader: TeamLeader,
    /// The next teammate number `k`; starts at 1.
    pub next_teammate: u32,
    pub launcher_pid: u32,
    pub cwd: PathBuf,
    /// RFC 3339.
    pub created_at: String,
    pub claude: ClaudeBinary,
    /// `Some` only on tmux; `None` on Orca and Herdr.
    pub tmux: Option<TmuxEndpoint>,
}

/// The Leader of a Team. It has no name: the Shim reads the Leader name from the
/// Leader's current Pane registration by `handle` (P2.8, P6.1).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TeamLeader {
    /// The host pane handle: a tmux `%id`, an Orca `term_…`, or the Herdr
    /// `HC_TERMINAL_ID`.
    pub handle: String,
    /// Always [`LEADER_SHIM_ID`].
    pub shim_id: String,
}

/// The real `claude` the Launcher started (P2.2).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClaudeBinary {
    /// The `command -v` result as the Launcher used it.
    pub resolved: PathBuf,
    /// The canonical path.
    pub path: PathBuf,
    /// Lowercase hex SHA-256 of the file at `path`.
    pub sha256: String,
    /// The first line of `--version`.
    pub version: String,
}

impl ClaudeBinary {
    /// The Claude version key: the first blank-separated word of `version` (P10.1).
    pub fn version_key(&self) -> &str {
        version_key(&self.version)
    }
}

/// The first blank-separated word of a `--version` line (P10.1).
pub fn version_key(version: &str) -> &str {
    version.split_whitespace().next().unwrap_or_default()
}

/// The real tmux of a Team on tmux (P4.2).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmuxEndpoint {
    /// The absolute path of the real tmux.
    pub path: PathBuf,
    /// The first comma field of the caller's `$TMUX`.
    pub socket: PathBuf,
}

/// The role of the calling pane in a Team (`self`, P8.1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    Leader,
    Teammate,
}

/// `TT_TEAM` names a Team whose record does not exist (P3.2). The CLI prints it
/// unchanged and exits 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TeamNotFound(pub String);

impl std::fmt::Display for TeamNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "team not found: {}", self.0)
    }
}

impl std::error::Error for TeamNotFound {}

// --- Paths -------------------------------------------------------------------------

/// `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools`.
pub fn state_root() -> Result<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        Some(state) => PathBuf::from(state),
        None => dirs::home_dir()
            .ok_or_else(|| anyhow!("cannot locate the state directory: HOME is unset"))?
            .join(".local")
            .join("state"),
    };
    Ok(state.join("tmux-tools"))
}

/// `<state root>/teams`.
pub fn teams_dir() -> Result<PathBuf> {
    Ok(state_root()?.join("teams"))
}

/// The team directory of `id`.
pub fn team_dir(id: &str) -> Result<PathBuf> {
    Ok(teams_dir()?.join(id))
}

/// The debug directory of `id` (P4.5).
pub fn debug_dir(id: &str) -> Result<PathBuf> {
    Ok(state_root()?.join("shim-debug").join(id))
}

/// The Shim log of the Team `id`: in the debug directory with `TT_SHIM_DEBUG=1`, else
/// in the team directory (P5.9).
pub fn shim_log_path(id: &str) -> Result<PathBuf> {
    Ok(if shim_debug() {
        debug_dir(id)?.join(SHIM_LOG)
    } else {
        team_dir(id)?.join(SHIM_LOG)
    })
}

/// Whether `TT_SHIM_DEBUG=1` is in this process's environment.
pub fn shim_debug() -> bool {
    std::env::var(TT_SHIM_DEBUG).is_ok_and(|value| value == "1")
}

/// The `team.lock` path of the Team `id`.
pub fn team_lock_path(id: &str) -> Result<PathBuf> {
    Ok(team_dir(id)?.join(TEAM_LOCK))
}

// --- The Team record ---------------------------------------------------------------

/// Read the Team record in `dir`.
pub fn load_from(dir: &Path) -> Result<TeamRecord> {
    let path = dir.join(TEAM_JSON);
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

/// The Team record of `id`; `None` when its team directory or record does not exist.
pub fn load(id: &str) -> Result<Option<TeamRecord>> {
    let dir = team_dir(id)?;
    match fs::metadata(dir.join(TEAM_JSON)) {
        Ok(_) => Ok(Some(load_from(&dir)?)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", dir.display())),
    }
}

/// Write `record` as `team.json` in `dir` by write-to-temp and atomic rename.
pub fn write_to(dir: &Path, record: &TeamRecord) -> Result<()> {
    let path = dir.join(TEAM_JSON);
    let temp = dir.join(format!(".{TEAM_JSON}.{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec_pretty(record)?)
        .with_context(|| format!("write {}", temp.display()))?;
    fs::rename(&temp, &path).with_context(|| format!("rename {}", path.display()))
}

/// The team id of this invocation: `TT_TEAM` when set and not empty.
pub fn current_id() -> Option<String> {
    std::env::var(TT_TEAM)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The Team record of this invocation, read once. `Ok(None)` outside a team; an error
/// ([`TeamNotFound`]) when `TT_TEAM` names a Team whose record does not exist (P3.2).
pub fn current() -> Result<Option<&'static TeamRecord>> {
    static CURRENT: OnceLock<std::result::Result<Option<TeamRecord>, String>> = OnceLock::new();
    let cell = CURRENT.get_or_init(|| {
        let Some(id) = current_id() else {
            return Ok(None);
        };
        match load(&id) {
            Ok(Some(record)) => Ok(Some(record)),
            Ok(None) => Err(id),
            Err(_) => Err(id),
        }
    });
    match cell {
        Ok(record) => Ok(record.as_ref()),
        Err(id) => Err(TeamNotFound(id.clone()).into()),
    }
}

/// The Leader pane handle of this invocation's Team, which is the calling pane inside a
/// team (P3.3); `None` outside a team.
pub fn leader_handle() -> Option<String> {
    current()
        .ok()
        .flatten()
        .map(|record| record.leader.handle.clone())
}

/// The real tmux of this invocation's Team on tmux (P3.4); `None` outside a team.
pub fn tmux_endpoint() -> Option<&'static TmuxEndpoint> {
    current()
        .ok()
        .flatten()
        .and_then(|record| record.tmux.as_ref())
}

// --- Locks -------------------------------------------------------------------------

/// An exclusive `flock` held while the value lives. The file is opened with
/// close-on-exec, so no child process inherits the lock.
#[derive(Debug)]
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Open (creating it) and lock `path`, waiting for the lock.
    pub fn exclusive(path: &Path) -> io::Result<FileLock> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        flock(&file, libc::LOCK_EX)?;
        Ok(FileLock { file })
    }

    /// Open the existing `path` without creating it and try the exclusive lock without
    /// waiting. `Ok(None)` when another process holds it.
    pub fn try_existing(path: &Path) -> io::Result<Option<FileLock>> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => Ok(Some(FileLock { file })),
            Err(error) if error.raw_os_error() == Some(libc::EWOULDBLOCK) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: `file` owns a valid descriptor for the duration of the call.
        let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: as in `flock`; closing the file releases the lock as well.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Hold `team.lock` of the Team `id` (every change of `team.json` and every Shim pane
/// create or close, P4.1).
pub fn lock_team(id: &str) -> Result<FileLock> {
    let path = team_lock_path(id)?;
    FileLock::exclusive(&path).with_context(|| format!("lock {}", path.display()))
}

/// Whether the Team `id` is running: its team directory exists and its `leader.lock`
/// is held (P4.4, P8.3).
pub fn is_running(id: &str) -> bool {
    let Ok(dir) = team_dir(id) else {
        return false;
    };
    leader_lock_held(&dir)
}

fn leader_lock_held(dir: &Path) -> bool {
    match FileLock::try_existing(&dir.join(LEADER_LOCK)) {
        Ok(Some(_released_on_drop)) => false,
        Ok(None) => true,
        Err(_) => false,
    }
}

// --- Building and removing a team directory ----------------------------------------

/// A team directory the Launcher built and holds `leader.lock` of.
#[derive(Debug)]
pub struct BuiltTeam {
    pub record: TeamRecord,
    pub dir: PathBuf,
    /// Held for the Launcher's whole life (P4.1).
    _leader_lock: FileLock,
}

/// 8 random lowercase hex digits.
pub fn new_team_id() -> String {
    let mut bytes = [0u8; 4];
    let random = File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    if random.is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let mixed = (nanos as u64) ^ (u64::from(std::process::id()) << 32);
        bytes = (mixed as u32 ^ (mixed >> 32) as u32).to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Build a team directory (P4.1): `teams/.init-<id>/` by exclusive create, its
/// `leader.lock` locked, `team.lock`, `team.json` (from `record_for(id)`), and
/// `bin/tmux` → `shim`, then rename it to `teams/<id>/` with no replace. A collision
/// takes a new id.
pub fn build(record_for: impl Fn(&str) -> TeamRecord, shim: &Path) -> Result<BuiltTeam> {
    let teams = teams_dir()?;
    fs::create_dir_all(&teams).with_context(|| format!("create {}", teams.display()))?;
    let (mut id, init) = loop {
        let id = new_team_id();
        let init = teams.join(format!(".init-{id}"));
        match fs::create_dir(&init) {
            Ok(()) => break (id, init),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("create {}", init.display())),
        }
    };
    let built = (|| -> Result<(FileLock, TeamRecord, PathBuf)> {
        let new_lock = init.join(format!("{LEADER_LOCK}.new"));
        let lock = FileLock::exclusive(&new_lock)
            .with_context(|| format!("lock {}", new_lock.display()))?;
        fs::rename(&new_lock, init.join(LEADER_LOCK))
            .with_context(|| format!("rename {}", new_lock.display()))?;
        File::create(init.join(TEAM_LOCK)).context("create team.lock")?;
        let bin = init.join("bin");
        fs::create_dir(&bin).with_context(|| format!("create {}", bin.display()))?;
        std::os::unix::fs::symlink(shim, bin.join("tmux")).context("create bin/tmux")?;
        let mut record = record_for(&id);
        write_to(&init, &record)?;
        loop {
            let dir = teams.join(&id);
            match rename_no_replace(&init, &dir) {
                Ok(()) => return Ok((lock, record, dir)),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    id = new_team_id();
                    record = record_for(&id);
                    write_to(&init, &record)?;
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("rename {}", init.display()))
                }
            }
        }
    })();
    match built {
        Ok((lock, record, dir)) => Ok(BuiltTeam {
            record,
            dir,
            _leader_lock: lock,
        }),
        Err(error) => {
            let _ = fs::remove_dir_all(&init);
            Err(error)
        }
    }
}

/// Rename `from` to `to`, failing with `AlreadyExists` when `to` exists.
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    // SAFETY: both arguments are valid NUL-terminated paths.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    // SAFETY: as above.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EEXIST) || error.raw_os_error() == Some(libc::ENOTEMPTY) {
        return Err(io::Error::new(ErrorKind::AlreadyExists, error));
    }
    Err(error)
}

impl BuiltTeam {
    /// Create the debug directory and its copy of `team.json` (P4.5).
    pub fn write_debug_copy(&self) -> Result<PathBuf> {
        let dir = debug_dir(&self.record.id)?;
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        write_to(&dir, &self.record)?;
        Ok(dir)
    }

    /// Delete the team directory (P2.5). The lock stays held until `self` drops.
    pub fn remove_dir(&self) -> Result<()> {
        fs::remove_dir_all(&self.dir).with_context(|| format!("delete {}", self.dir.display()))
    }
}

/// Garbage collection (P4.4): delete every `teams/<id>/` and `teams/.init-<id>/`
/// directory whose `leader.lock` this process can lock; keep every other directory.
/// It never touches `shim-debug/`.
pub fn collect_garbage() {
    let Ok(teams) = teams_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&teams) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        // A directory without `leader.lock` is skipped; one whose lock is held is kept.
        let Ok(Some(_held)) = FileLock::try_existing(&path.join(LEADER_LOCK)) else {
            continue;
        };
        let _ = fs::remove_dir_all(&path);
    }
}

/// Lowercase hex SHA-256 of the file at `path`.
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_key_is_the_first_word() {
        assert_eq!(version_key("2.1.319 (Claude Code)"), "2.1.319");
        assert_eq!(version_key(""), "");
    }

    #[test]
    fn team_ids_are_8_lowercase_hex() {
        let id = new_team_id();
        assert_eq!(id.len(), 8);
        assert!(id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
