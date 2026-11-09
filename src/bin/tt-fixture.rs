//! `tt-fixture`: turn a `TT_SHIM_DEBUG=1` `shim.log` into a trace fixture
//! (PRD-261108-0447-01 P10.3).
//!
//! `cargo run --features test-fixtures --bin tt-fixture -- <shim.log>
//!   > tests/fixtures/claude-teams/<version key>.jsonl`
//!
//! It reads each JSONL line of `shim.log` and the `team.json` in the same directory,
//! keeps `argv`, `exit`, and `stdout`, and replaces the team id, the cwd, and the
//! Claude binary paths (`ClaudeBinary.resolved` and `ClaudeBinary.path`, both as
//! `{{claude}}`) with the P10.1 placeholders, longest value first. Built only with the
//! `test-fixtures` feature, so the release build does not contain it.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
struct FixtureLine {
    argv: Vec<String>,
    exit: i64,
    stdout: String,
}

fn main() -> Result<()> {
    let log = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("usage: tt-fixture <shim.log>"))?;
    let log = Path::new(&log);
    let dir = log.parent().unwrap_or(Path::new("."));
    let team: Value = serde_json::from_slice(
        &std::fs::read(dir.join("team.json"))
            .with_context(|| format!("read {}", dir.join("team.json").display()))?,
    )?;
    let field = |pointer: &str| {
        team.pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("team.json has no {pointer}"))
    };
    let mut replacements = vec![
        (field("/id")?, "{{team_id}}"),
        (field("/cwd")?, "{{cwd}}"),
        (field("/claude/resolved")?, "{{claude}}"),
        (field("/claude/path")?, "{{claude}}"),
    ];
    replacements.retain(|(value, _)| !value.is_empty());
    replacements.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
    let fill = |text: &str| {
        replacements
            .iter()
            .fold(text.to_owned(), |text, (value, placeholder)| {
                text.replace(value.as_str(), placeholder)
            })
    };

    let text = std::fs::read_to_string(log).with_context(|| format!("read {}", log.display()))?;
    let mut out = String::new();
    for (number, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Value = serde_json::from_str(line)
            .with_context(|| format!("{}:{}: not JSON", log.display(), number + 1))?;
        let exit = entry["exit"].as_i64().ok_or_else(|| {
            anyhow!(
                "{}:{}: no exit: the log was not written with TT_SHIM_DEBUG=1",
                log.display(),
                number + 1
            )
        })?;
        let argv = entry["argv"]
            .as_array()
            .ok_or_else(|| anyhow!("{}:{}: no argv", log.display(), number + 1))?
            .iter()
            .map(|word| word.as_str().map(fill))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| anyhow!("{}:{}: argv is not strings", log.display(), number + 1))?;
        let fixture = FixtureLine {
            argv,
            exit,
            stdout: fill(entry["stdout"].as_str().unwrap_or_default()),
        };
        out.push_str(&serde_json::to_string(&fixture)?);
        out.push('\n');
    }
    print!("{out}");
    Ok(())
}
