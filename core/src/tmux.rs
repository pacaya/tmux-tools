#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_TMUX_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum CommandOutputError {
    Spawn(io::Error),
    Wait(io::Error),
    Output(io::Error),
    Timeout { timeout: Duration },
}

impl fmt::Display for CommandOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandOutputError::Spawn(error) => write!(f, "failed to spawn command: {error}"),
            CommandOutputError::Wait(error) => write!(f, "failed to wait for command: {error}"),
            CommandOutputError::Output(error) => {
                write!(f, "failed to read command output: {error}")
            }
            CommandOutputError::Timeout { timeout } => {
                write!(f, "command timed out after {timeout:?}")
            }
        }
    }
}

impl std::error::Error for CommandOutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandOutputError::Spawn(error)
            | CommandOutputError::Wait(error)
            | CommandOutputError::Output(error) => Some(error),
            CommandOutputError::Timeout { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmuxInvocation {
    pub prefix: Vec<String>,
    pub socket: Option<String>,
    pub tmux_bin: String,
}

impl Default for TmuxInvocation {
    fn default() -> Self {
        TmuxInvocation {
            prefix: vec![],
            socket: None,
            tmux_bin: "tmux".to_string(),
        }
    }
}

thread_local! {
    static THREAD_INVOCATION: RefCell<Option<TmuxInvocation>> = const { RefCell::new(None) };
}

static GLOBAL_INVOCATION: OnceLock<Mutex<Option<TmuxInvocation>>> = OnceLock::new();

fn global_mutex() -> &'static Mutex<Option<TmuxInvocation>> {
    GLOBAL_INVOCATION.get_or_init(|| Mutex::new(None))
}

fn resolve_invocation() -> TmuxInvocation {
    if let Some(invocation) = THREAD_INVOCATION.with(|cell| cell.borrow().clone()) {
        return invocation;
    }

    if let Some(invocation) = global_mutex().lock().unwrap().clone() {
        return invocation;
    }

    TmuxInvocation::default()
}

/// Set or clear the process-wide default TmuxInvocation.
pub fn set_global_invocation(invocation: Option<TmuxInvocation>) {
    *global_mutex().lock().unwrap() = invocation;
}

/// Run `f` with a thread-local TmuxInvocation override (RAII).
/// Resolution order: thread-local -> global -> plain "tmux".
pub fn with_invocation<T>(invocation: TmuxInvocation, f: impl FnOnce() -> T) -> T {
    struct RestoreInvocation(Option<TmuxInvocation>);

    impl Drop for RestoreInvocation {
        fn drop(&mut self) {
            THREAD_INVOCATION.with(|cell| {
                *cell.borrow_mut() = self.0.take();
            });
        }
    }

    let old = THREAD_INVOCATION.with(|cell| {
        let old = cell.borrow().clone();
        *cell.borrow_mut() = Some(invocation);
        old
    });
    let _restore = RestoreInvocation(old);

    f()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TmuxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn run(args: &[&str]) -> Result<TmuxOutput> {
    run_with(args, false)
}

pub fn run_clean(args: &[&str]) -> Result<TmuxOutput> {
    run_with(args, true)
}

/// Run a tmux command with `input` piped to its stdin. Used by transports that
/// hand tmux their payload out of band (`load-buffer -`) rather than as an argv
/// argument.
pub fn run_with_stdin(args: &[&str], input: &[u8]) -> Result<TmuxOutput> {
    run_with_stdin_clean(args, input, false)
}

pub fn run_checked(args: &[&str]) -> Result<String> {
    check(run(args), args)
}

pub fn run_checked_clean(args: &[&str]) -> Result<String> {
    check(run_clean(args), args)
}

pub fn run_checked_with_stdin(args: &[&str], input: &[u8]) -> Result<String> {
    check(run_with_stdin(args, input), args)
}

pub fn run_checked_owned(args: &[String]) -> Result<String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_checked(&refs)
}

/// A tmux command that ran and exited non-zero, as the `run_checked*` functions report
/// it. Callers tell it apart from a command that could not run (spawn failure, timeout)
/// with `error.downcast_ref::<CommandFailed>()`, which sees through added context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandFailed {
    pub args: Vec<String>,
    pub exit_code: i32,
    /// tmux's stderr, trimmed.
    pub stderr: String,
}

impl fmt::Display for CommandFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tmux command failed (args: {:?}, exit code {}): {}",
            self.args, self.exit_code, self.stderr
        )
    }
}

impl std::error::Error for CommandFailed {}

fn check(result: Result<TmuxOutput>, args: &[&str]) -> Result<String> {
    let output =
        result.with_context(|| format!("failed to run tmux command: tmux {}", args.join(" ")))?;

    if output.exit_code != 0 {
        return Err(CommandFailed {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            exit_code: output.exit_code,
            stderr: output.stderr.trim().to_owned(),
        }
        .into());
    }

    Ok(output.stdout)
}

fn run_with(user_args: &[&str], clean: bool) -> Result<TmuxOutput> {
    let mut command = command_for(user_args);
    if clean {
        command.env_remove("TMUX");
    }

    execute(command, DEFAULT_TMUX_COMMAND_TIMEOUT, None)
}

fn run_with_stdin_clean(user_args: &[&str], input: &[u8], clean: bool) -> Result<TmuxOutput> {
    let mut command = command_for(user_args);
    if clean {
        command.env_remove("TMUX");
    }

    execute(command, DEFAULT_TMUX_COMMAND_TIMEOUT, Some(input))
}

fn execute(command: Command, timeout: Duration, input: Option<&[u8]>) -> Result<TmuxOutput> {
    let output = match command_output_with_timeout_and_stdin(command, timeout, input) {
        Ok(output) => output,
        Err(CommandOutputError::Spawn(error)) if error.kind() == ErrorKind::NotFound => {
            return Err(anyhow!(
                "tmux invocation binary not found in PATH; install tmux or add the configured prefix/bin to PATH"
            ));
        }
        Err(CommandOutputError::Timeout { timeout }) => {
            return Err(anyhow!("tmux command timed out after {timeout:?}"));
        }
        Err(error) => {
            return Err(anyhow!(error)).context("failed to run tmux command");
        }
    };

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(TmuxOutput {
        stdout,
        stderr,
        exit_code,
    })
}

/// Run `command` to completion and collect its exit status, stdout and stderr.
///
/// `timeout` bounds the whole call: the command must exit and both of its output
/// pipes must reach EOF before the deadline, including when a descendant still
/// holds a pipe after the command itself exited. The command runs in its own
/// process group; on timeout that whole group is killed and
/// [`CommandOutputError::Timeout`] is returned.
pub fn command_output_with_timeout(
    command: Command,
    timeout: Duration,
) -> std::result::Result<Output, CommandOutputError> {
    command_output_with_timeout_and_stdin(command, timeout, None)
}

fn command_output_with_timeout_and_stdin(
    mut command: Command,
    timeout: Duration,
    input: Option<&[u8]>,
) -> std::result::Result<Output, CommandOutputError> {
    let deadline = Instant::now() + timeout;
    // A fresh process group lets a timeout kill every descendant that still
    // holds the output pipes, not only the direct child.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(CommandOutputError::Spawn)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        CommandOutputError::Output(io::Error::other("failed to capture command stdout"))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        CommandOutputError::Output(io::Error::other("failed to capture command stderr"))
    })?;
    // Write stdin on its own thread so a payload larger than the pipe buffer
    // cannot deadlock against the child's output.
    let stdin_writer = match input {
        Some(input) => {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                CommandOutputError::Output(io::Error::other("failed to capture command stdin"))
            })?;
            let input = input.to_vec();
            Some(thread::spawn(move || -> io::Result<()> {
                match stdin.write_all(&input) {
                    // The child may exit without draining stdin; its status is
                    // authoritative, so a broken pipe is not an error here.
                    Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
                    other => other,
                }
            }))
        }
        None => None,
    };
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));

    loop {
        // Reap the child only after both pipes reached EOF. Until it is reaped
        // its pid stays allocated, so the process-group id it leads cannot be
        // reused before a timeout kill targets it.
        if stdout_reader.is_finished() && stderr_reader.is_finished() {
            if let Some(status) = child.try_wait().map_err(CommandOutputError::Wait)? {
                finish_stdin_writer(stdin_writer, deadline);
                let stdout = join_reader(stdout_reader, "stdout")?;
                let stderr = join_reader(stderr_reader, "stderr")?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
        }
        if Instant::now() >= deadline {
            kill_process_group(&mut child);
            let _ = child.wait();
            // The reader and stdin threads are detached, not joined: a process
            // outside the group may still hold a pipe, and joining would let it
            // extend the call past the deadline. Each thread ends when its pipe
            // closes.
            return Err(CommandOutputError::Timeout { timeout });
        }
        let sleep_for = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(10));
        thread::sleep(sleep_for);
    }
}

/// Kill the child's whole process group, then the child itself.
///
/// Call only while the child is not yet reaped: its pid then still names the
/// group it leads.
#[cfg(unix)]
fn kill_process_group(child: &mut Child) {
    // The child leads its own group (`process_group(0)`), so its pid is the
    // group id. Never pass 0 or -1 to kill(2): those address the caller's group
    // or every process.
    if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
        if pgid > 1 {
            // SAFETY: kill(2) takes plain integers and touches no memory.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_process_group(child: &mut Child) {
    let _ = child.kill();
}

/// Reap the stdin writer, waiting for it no later than `deadline`, and discard
/// its result. A write failure after the child exited is not itself a command
/// failure; the child's exit status is reported by the caller. A writer still
/// blocked at the deadline is detached so it cannot extend the call.
fn finish_stdin_writer(handle: Option<thread::JoinHandle<io::Result<()>>>, deadline: Instant) {
    let Some(handle) = handle else {
        return;
    };
    while !handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

fn read_pipe(mut pipe: impl Read) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<Vec<u8>>>,
    stream_name: &str,
) -> std::result::Result<Vec<u8>, CommandOutputError> {
    match handle.join() {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(CommandOutputError::Output(error)),
        Err(_) => Err(CommandOutputError::Output(io::Error::other(format!(
            "{stream_name} reader panicked"
        )))),
    }
}

fn command_for(user_args: &[&str]) -> Command {
    let invocation = resolve_invocation();
    let mut command = if invocation.prefix.is_empty() {
        Command::new(&invocation.tmux_bin)
    } else {
        let mut command = Command::new(&invocation.prefix[0]);
        command.args(&invocation.prefix[1..]);
        command.arg(&invocation.tmux_bin);
        command
    };

    if let Some(socket) = invocation.socket.as_deref() {
        command.args(["-L", socket]);
    }
    command.args(user_args);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_mutex() -> &'static Mutex<()> {
        static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_MUTEX.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn a_non_zero_exit_is_a_command_failed_seen_through_context_with_the_same_message() {
        let output = TmuxOutput {
            stdout: String::new(),
            stderr: "can't find pane: %7\n".to_owned(),
            exit_code: 1,
        };
        let error = check(Ok(output), &["list-panes", "-t", "%7"])
            .context("cannot measure the window of pane %7")
            .unwrap_err();

        assert_eq!(
            error.downcast_ref::<CommandFailed>(),
            Some(&CommandFailed {
                args: vec!["list-panes".into(), "-t".into(), "%7".into()],
                exit_code: 1,
                stderr: "can't find pane: %7".into(),
            })
        );
        assert_eq!(
            error.root_cause().to_string(),
            r#"tmux command failed (args: ["list-panes", "-t", "%7"], exit code 1): can't find pane: %7"#
        );
    }

    #[test]
    fn a_command_that_could_not_run_is_not_a_command_failed() {
        let error = check(Err(anyhow!("tmux command timed out")), &["list-panes"]).unwrap_err();

        assert!(error.downcast_ref::<CommandFailed>().is_none());
    }

    #[test]
    fn test_invocation_defaults() {
        let invocation = TmuxInvocation::default();

        assert!(invocation.prefix.is_empty());
        assert_eq!(invocation.socket, None);
        assert_eq!(invocation.tmux_bin, "tmux");
    }

    #[test]
    fn test_resolve_invocation_with_prefix_socket_bin() {
        let _guard = test_mutex().lock().unwrap();
        set_global_invocation(None);

        with_invocation(
            TmuxInvocation {
                prefix: vec!["sudo".into(), "-u".into(), "agent".into()],
                socket: Some("silverbond".into()),
                tmux_bin: "tmux2".into(),
            },
            || {
                let invocation = resolve_invocation();

                assert_eq!(invocation.prefix, ["sudo", "-u", "agent"]);
                assert_eq!(invocation.socket.as_deref(), Some("silverbond"));
                assert_eq!(invocation.tmux_bin, "tmux2");
            },
        );

        set_global_invocation(None);
    }

    #[test]
    fn test_thread_local_beats_global() {
        let _guard = test_mutex().lock().unwrap();
        set_global_invocation(Some(TmuxInvocation {
            prefix: vec!["global".into()],
            socket: None,
            tmux_bin: "tmux".into(),
        }));

        with_invocation(
            TmuxInvocation {
                prefix: vec!["local".into()],
                socket: None,
                tmux_bin: "tmux".into(),
            },
            || {
                assert_eq!(resolve_invocation().prefix, ["local"]);
            },
        );

        assert_eq!(resolve_invocation().prefix, ["global"]);
        set_global_invocation(None);
    }

    #[test]
    fn test_resolve_after_with_invocation_restores() {
        let _guard = test_mutex().lock().unwrap();
        set_global_invocation(Some(TmuxInvocation {
            prefix: vec!["global".into()],
            socket: None,
            tmux_bin: "tmux".into(),
        }));

        with_invocation(
            TmuxInvocation {
                prefix: vec!["local".into()],
                socket: None,
                tmux_bin: "tmux".into(),
            },
            || {
                assert_eq!(resolve_invocation().prefix, ["local"]);
            },
        );

        assert_eq!(resolve_invocation().prefix, ["global"]);
        set_global_invocation(None);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_with_timeout_kills_blocked_process() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "tmux-tools-timeout-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("block.sh");
        fs::write(&script, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let started = Instant::now();
        let error = command_output_with_timeout(Command::new(&script), Duration::from_millis(50))
            .expect_err("blocking command should time out");

        assert!(
            matches!(error, CommandOutputError::Timeout { .. }),
            "expected timeout, got {error:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "blocked command should return promptly"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn timeout_test_root(label: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "tmux-tools-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    fn write_script(path: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, body).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    /// Read the whitespace-separated pids the script wrote; panics when the
    /// script did not write them (the acceptance precondition).
    #[cfg(unix)]
    fn read_pids(path: &std::path::Path) -> Vec<u32> {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("script did not write {}: {error}", path.display()));
        text.split_whitespace()
            .map(|pid| pid.parse().unwrap())
            .collect()
    }

    /// True when `pid` no longer exists or is a zombie.
    #[cfg(unix)]
    fn exited_or_zombie(pid: u32) -> bool {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("run ps");
        let stat = String::from_utf8_lossy(&output.stdout);
        let stat = stat.trim();
        stat.is_empty() || stat.starts_with('Z')
    }

    #[cfg(unix)]
    fn assert_all_gone_within(pids: &[u32], grace: Duration) {
        let returned = Instant::now();
        loop {
            let alive: Vec<u32> = pids
                .iter()
                .copied()
                .filter(|pid| !exited_or_zombie(*pid))
                .collect();
            if alive.is_empty() {
                return;
            }
            if returned.elapsed() >= grace {
                panic!("processes still running {grace:?} after return: {alive:?}");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn command_output_with_timeout_kills_group_when_descendant_holds_pipes() {
        let root = timeout_test_root("timeout-group");
        let pid_file = root.join("pids");
        let script = root.join("group.sh");
        write_script(
            &script,
            &format!(
                "#!/bin/sh\nsleep 5 &\nprintf '%s %s\\n' \"$$\" \"$!\" > '{tmp}'\nmv '{tmp}' '{pids}'\nsleep 5\n",
                tmp = root.join("pids.tmp").display(),
                pids = pid_file.display(),
            ),
        );

        let started = Instant::now();
        let error = command_output_with_timeout(Command::new(&script), Duration::from_secs(1))
            .expect_err("blocking command should time out");
        let elapsed = started.elapsed();

        assert!(
            matches!(error, CommandOutputError::Timeout { .. }),
            "expected timeout, got {error:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1500),
            "timeout should return within 1.5 s of the call's start, took {elapsed:?}"
        );
        let pids = read_pids(&pid_file);
        assert_eq!(
            pids.len(),
            2,
            "expected script and background pids: {pids:?}"
        );
        assert_all_gone_within(&pids, Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_with_timeout_bounds_descendant_after_child_exits() {
        let root = timeout_test_root("timeout-orphan");
        let pid_file = root.join("pids");
        let script = root.join("orphan.sh");
        write_script(
            &script,
            &format!(
                "#!/bin/sh\nsleep 5 &\nprintf '%s\\n' \"$!\" > '{tmp}'\nmv '{tmp}' '{pids}'\nexit 0\n",
                tmp = root.join("pids.tmp").display(),
                pids = pid_file.display(),
            ),
        );

        let started = Instant::now();
        let error = command_output_with_timeout(Command::new(&script), Duration::from_secs(1))
            .expect_err("descendant holding the pipes should time out");
        let elapsed = started.elapsed();

        assert!(
            matches!(error, CommandOutputError::Timeout { .. }),
            "expected timeout, got {error:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1500),
            "timeout should return within 1.5 s of the call's start, took {elapsed:?}"
        );
        let pids = read_pids(&pid_file);
        assert_eq!(pids.len(), 1, "expected the background pid: {pids:?}");
        assert_all_gone_within(&pids, Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_with_timeout_returns_status_and_both_streams() {
        use std::os::unix::process::ExitStatusExt;

        let mut command = Command::new("sh");
        command.args(["-c", "printf out; printf err >&2; exit 3"]);

        let output = command_output_with_timeout(command, Duration::from_secs(5))
            .expect("command should complete");

        assert_eq!(
            output.status.code(),
            Some(3),
            "status {:?}",
            output.status.signal()
        );
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }

    #[cfg(unix)]
    #[test]
    fn command_output_with_timeout_and_stdin_delivers_input() {
        let mut command = Command::new("sh");
        command.args(["-c", "cat; printf done >&2"]);

        let output =
            command_output_with_timeout_and_stdin(command, Duration::from_secs(5), Some(b"hello"))
                .expect("command should complete");

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"hello");
        assert_eq!(output.stderr, b"done");
    }
}
