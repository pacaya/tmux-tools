//! The Launcher: `tmux-tools claude-teams` (PRD-261108-0447-01 P2).
//!
//! It runs in the calling pane, creates a Team, and starts the real `claude` as its
//! child with the Shim first on the child's `PATH`, so every teammate Claude starts is
//! a tmux-tools pane. When it cannot set up a Team it fails open: Claude starts with
//! in-process teammates and the caller's environment.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};

use anyhow::Result;
use clap::Args;
use tmux_tools_core::{
    host::{self, Host, HostSelection, ListScope},
    team::{self, BuiltTeam, ClaudeBinary, TeamLeader, TeamRecord, TmuxEndpoint, LEADER_SHIM_ID},
};

use crate::util::rfc3339_utc_now;

/// The Claude versions with a trace fixture in this build (P10.5), from
/// `tests/fixtures/claude-teams/*.jsonl` (see `build.rs`).
const CONTRACT_TESTED: &[&str] = &include!(concat!(env!("OUT_DIR"), "/contract_tested.rs"));

/// Run Claude in a Team: every teammate is a tmux-tools pane
#[derive(Args, Debug)]
pub struct ClaudeTeamsArgs {
    /// The Claude program, resolved with `command -v` (default `claude`).
    #[arg(long, value_name = "PROGRAM", hide = true)]
    pub(crate) claude: Option<String>,
    /// Arguments passed to Claude.
    #[arg(last = true, value_name = "CLAUDE ARGS")]
    pub(crate) claude_args: Vec<String>,
}

/// Run the verb; returns the exit code.
pub fn run(args: &ClaudeTeamsArgs) -> i32 {
    // P2.6: not fail-open.
    if let Some(id) = team::current_id() {
        eprintln!("claude-teams: already inside team {id}");
        return 1;
    }
    // P2.2: not fail-open.
    let program = args.claude.as_deref().unwrap_or("claude");
    let Some(resolved) = command_v(program) else {
        eprintln!("claude-teams: claude not found");
        return 1;
    };
    let claude = match claude_binary(&resolved) {
        Ok(claude) => claude,
        Err(_) => {
            eprintln!("claude-teams: claude not found");
            return 1;
        }
    };
    // P10.5: once, and the Launcher continues.
    let key = claude.version_key().to_owned();
    if !CONTRACT_TESTED.contains(&key.as_str()) {
        eprintln!("claude-teams: claude {key} not contract-tested");
    }

    team::collect_garbage();
    match set_up(claude) {
        Ok(started) => run_team(started, &args.claude_args),
        Err(reason) => {
            eprintln!("claude-teams: no team: {reason}; starting claude without teammate panes");
            let mut command = Command::new(&resolved);
            command.args(with_teammate_mode(&args.claude_args, "in-process"));
            supervise(command).unwrap_or_else(|error| {
                eprintln!("claude-teams: cannot run claude: {error:#}");
                1
            })
        }
    }
}

/// `command -v <program>` on the caller's `PATH`: the path of a regular executable
/// file, as found.
fn command_v(program: &str) -> Option<PathBuf> {
    let executable = |path: &Path| {
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        let path = PathBuf::from(program);
        return executable(&path).then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    // An empty component is the current directory, searched in its place, as
    // `command -v` does; it resolves to an absolute path.
    let cwd = std::env::current_dir().ok();
    std::env::split_paths(&path)
        .filter_map(|dir| {
            if dir.as_os_str().is_empty() {
                cwd.clone()
            } else {
                Some(dir)
            }
        })
        .map(|dir| dir.join(program))
        .find(|candidate| executable(candidate))
}

/// The Claude binary record of P2.2: the canonical path, its SHA-256, and the first
/// line of its `--version`.
fn claude_binary(resolved: &Path) -> Result<ClaudeBinary> {
    let path = std::fs::canonicalize(resolved)?;
    let sha256 = team::sha256_file(&path)?;
    let output = Command::new(resolved).arg("--version").output()?;
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    Ok(ClaudeBinary {
        resolved: resolved.to_path_buf(),
        path,
        sha256,
        version,
    })
}

/// A Team that is set up, and the environment of its Claude.
struct Started {
    team: BuiltTeam,
    env: Vec<(&'static str, OsString)>,
}

/// Set up the Team (P2.1–P2.3, P4); `Err(reason)` is a P2.7 fail-open.
fn set_up(claude: ClaudeBinary) -> std::result::Result<Started, String> {
    let selection = HostSelection::from_env().map_err(|error| format!("{error:#}"))?;
    let port = host::current();
    let (leader, tmux) = match selection.host {
        Host::Tmux => {
            let pane = non_empty_env("TMUX_PANE").ok_or("TMUX_PANE is unset")?;
            let tmux_env = non_empty_env("TMUX").ok_or("TMUX is unset")?;
            let socket = tmux_env.split(',').next().unwrap_or_default().to_owned();
            let path = command_v("tmux").ok_or("tmux not found")?;
            let path = std::path::absolute(&path).map_err(|error| error.to_string())?;
            (
                pane,
                Some(TmuxEndpoint {
                    path,
                    socket: socket.into(),
                }),
            )
        }
        Host::Orca => {
            let handle = non_empty_env(host::ORCA_TERMINAL_HANDLE)
                .ok_or_else(|| format!("{} is unset", host::ORCA_TERMINAL_HANDLE))?;
            (handle, None)
        }
        // PRD-261115-0921-01 P2.2: the Leader handle on Herdr is `HC_TERMINAL_ID`.
        Host::Herdr => {
            let handle = non_empty_env(host::HC_TERMINAL_ID)
                .ok_or_else(|| format!("{} is unset", host::HC_TERMINAL_ID))?;
            (handle, None)
        }
        // PRD-261112-2108-01 P7.6: Rex has no teammate shim.
        Host::Rex => return Err("no teammate shim on rex".to_owned()),
    };
    let server_id = port
        .server_identity()
        .map_err(|error| format!("{error:#}"))?;
    if !port
        .pane_exists(&leader)
        .map_err(|error| format!("{error:#}"))?
    {
        return Err(format!(
            "calling pane {leader} not found on {}",
            selection.host
        ));
    }
    let shim = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| format!("cannot locate tmux-tools: {error}"))?;
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    let created_at = rfc3339_utc_now().map_err(|error| error.to_string())?;
    let record_for = |id: &str| TeamRecord {
        schema: team::TEAM_SCHEMA.to_owned(),
        id: id.to_owned(),
        host: selection.host,
        server_id: server_id.clone(),
        leader: TeamLeader {
            handle: leader.clone(),
            shim_id: LEADER_SHIM_ID.to_owned(),
        },
        next_teammate: 1,
        launcher_pid: std::process::id(),
        cwd: cwd.clone(),
        created_at: created_at.clone(),
        claude: claude.clone(),
        tmux: tmux.clone(),
    };
    let built = team::build(record_for, &shim).map_err(|error| format!("{error:#}"))?;
    if team::shim_debug() {
        if let Err(error) = built.write_debug_copy() {
            let _ = built.remove_dir();
            return Err(format!("{error:#}"));
        }
    }
    // P2.3: only after the rename.
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut child_path = built.dir.join("bin").into_os_string();
    child_path.push(":");
    child_path.push(path);
    let mut fake_tmux = built.dir.join("tmux-fake").into_os_string();
    fake_tmux.push(format!(",{},0", std::process::id()));
    let env = vec![
        ("PATH", child_path),
        ("TMUX", fake_tmux),
        ("TMUX_PANE", OsString::from(LEADER_SHIM_ID)),
        (team::TT_TEAM, OsString::from(&built.record.id)),
        (team::TT_HOST, OsString::from(selection.host.as_str())),
    ];
    Ok(Started { team: built, env })
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The Claude arguments with `--teammate-mode <mode>` added unless the caller passes
/// `--teammate-mode` (P2.4, P2.7).
fn with_teammate_mode(args: &[String], mode: &str) -> Vec<String> {
    let given = args
        .iter()
        .any(|arg| arg == "--teammate-mode" || arg.starts_with("--teammate-mode="));
    let mut all = Vec::new();
    if !given {
        all.push("--teammate-mode".to_owned());
        all.push(mode.to_owned());
    }
    all.extend(args.iter().cloned());
    all
}

/// Run Claude in the Team, then close the Team on a clean exit (P2.5).
fn run_team(started: Started, claude_args: &[String]) -> i32 {
    let record = &started.team.record;
    let mut command = Command::new(&record.claude.resolved);
    command.args(with_teammate_mode(claude_args, "tmux"));
    for (key, value) in &started.env {
        command.env(key, value);
    }
    let status = match supervise(command) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("claude-teams: cannot run claude: {error:#}");
            return 1;
        }
    };
    if status != 0 {
        // The Teammate panes and the team directory stay.
        return status;
    }
    let port = host::current();
    match port.list_panes(ListScope::All) {
        Ok(rows) => {
            for row in rows {
                if row.team.as_deref() == Some(record.id.as_str())
                    && !row.pane_dead
                    && row.pane_id != record.leader.handle
                {
                    if let Err(error) = port.close_pane(&row.pane_id, None) {
                        eprintln!("claude-teams: cannot close {}: {error:#}", row.pane_id);
                    }
                }
            }
        }
        Err(error) => eprintln!("claude-teams: cannot list the teammate panes: {error:#}"),
    }
    if let Err(error) = started.team.remove_dir() {
        eprintln!("claude-teams: {error:#}");
    }
    0
}

/// The pid of the running Claude, for the signal handlers.
static CHILD: AtomicI32 = AtomicI32::new(0);

extern "C" fn forward(signal: libc::c_int) {
    let child = CHILD.load(Ordering::SeqCst);
    if child > 0 {
        // SAFETY: `kill` is async-signal-safe.
        unsafe {
            libc::kill(child, signal);
        }
    }
}

/// Start `command` and wait for it with the signal and stop rules of P2.5; returns its
/// exit status (128 + signal number for a signal).
fn supervise(mut command: Command) -> Result<i32> {
    // SAFETY: only async-signal-safe calls between fork and exec: the child gets the
    // default dispositions back for the signals the Launcher ignores.
    unsafe {
        command.pre_exec(|| {
            for signal in [
                libc::SIGINT,
                libc::SIGQUIT,
                libc::SIGTSTP,
                libc::SIGTERM,
                libc::SIGHUP,
            ] {
                libc::signal(signal, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let pid = child.id() as libc::pid_t;
    CHILD.store(pid, Ordering::SeqCst);
    // SAFETY: installing dispositions; `forward` only calls `kill`.
    unsafe {
        for signal in [libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP] {
            libc::signal(signal, libc::SIG_IGN);
        }
        for signal in [libc::SIGTERM, libc::SIGHUP] {
            libc::signal(signal, forward as *const () as libc::sighandler_t);
        }
    }
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: waiting for our own child.
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) };
        if waited < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if libc::WIFSTOPPED(status) {
            // Claude stopped: stop the Launcher so the caller's shell gets the
            // terminal; on SIGCONT, continue Claude.
            // SAFETY: plain signal calls on our own pid and our child.
            unsafe {
                libc::raise(libc::SIGSTOP);
                libc::kill(pid, libc::SIGCONT);
            }
            continue;
        }
        if libc::WIFEXITED(status) {
            return Ok(libc::WEXITSTATUS(status));
        }
        if libc::WIFSIGNALED(status) {
            return Ok(128 + libc::WTERMSIG(status));
        }
    }
}
