#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::fmt;
use std::io::{self, ErrorKind, Read};
use std::process::{Command, Output, Stdio};
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

pub fn run_checked(args: &[&str]) -> Result<String> {
    check(run(args), args)
}

pub fn run_checked_clean(args: &[&str]) -> Result<String> {
    check(run_clean(args), args)
}

pub fn run_checked_owned(args: &[String]) -> Result<String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_checked(&refs)
}

fn check(result: Result<TmuxOutput>, args: &[&str]) -> Result<String> {
    let output =
        result.with_context(|| format!("failed to run tmux command: tmux {}", args.join(" ")))?;

    if output.exit_code != 0 {
        return Err(anyhow!(
            "tmux command failed (args: {:?}, exit code {}): {}",
            args,
            output.exit_code,
            output.stderr.trim()
        ));
    }

    Ok(output.stdout)
}

fn run_with(user_args: &[&str], clean: bool) -> Result<TmuxOutput> {
    let mut command = command_for(user_args);
    if clean {
        command.env_remove("TMUX");
    }

    let output = match command_output_with_timeout(command, DEFAULT_TMUX_COMMAND_TIMEOUT) {
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

pub fn command_output_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> std::result::Result<Output, CommandOutputError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(CommandOutputError::Spawn)?;
    let stdout = child.stdout.take().ok_or_else(|| {
        CommandOutputError::Output(io::Error::new(
            ErrorKind::Other,
            "failed to capture command stdout",
        ))
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        CommandOutputError::Output(io::Error::new(
            ErrorKind::Other,
            "failed to capture command stderr",
        ))
    })?;
    let stdout_reader = thread::spawn(move || read_pipe(stdout));
    let stderr_reader = thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait().map_err(CommandOutputError::Wait)? {
            Some(status) => {
                let stdout = join_reader(stdout_reader, "stdout")?;
                let stderr = join_reader(stderr_reader, "stderr")?;
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_reader(stdout_reader, "stdout");
                let _ = join_reader(stderr_reader, "stderr");
                return Err(CommandOutputError::Timeout { timeout });
            }
            None => {
                let sleep_for = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10));
                thread::sleep(sleep_for);
            }
        }
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
        Err(_) => Err(CommandOutputError::Output(io::Error::new(
            ErrorKind::Other,
            format!("{stream_name} reader panicked"),
        ))),
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
}
