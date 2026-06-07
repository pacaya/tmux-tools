#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::io::ErrorKind;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

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

    let output = match command.output() {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(anyhow!(
                "tmux invocation binary not found in PATH; install tmux or add the configured prefix/bin to PATH"
            ));
        }
        Err(error) => {
            return Err(error).context("failed to run tmux command");
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
}
