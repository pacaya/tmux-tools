#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::io::ErrorKind;
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TmuxOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn run(args: &[&str]) -> Result<TmuxOutput> {
    run_with(Command::new("tmux").args(args))
}

pub fn run_clean(args: &[&str]) -> Result<TmuxOutput> {
    run_with(Command::new("tmux").args(args).env_remove("TMUX"))
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
    let output = result
        .with_context(|| format!("failed to run tmux command: tmux {}", args.join(" ")))?;

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

fn run_with(command: &mut Command) -> Result<TmuxOutput> {
    let output = match command.output() {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(anyhow!(
                "tmux binary not found in PATH; install tmux or add it to PATH"
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
