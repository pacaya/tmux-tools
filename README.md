# tmux-tools

Rust CLI for LLM-driven tmux control. Single static binary, sub-10ms cold start.

`tmux-tools` exists because a 200-500ms Python cold start adds up when an agent fires commands from a hot loop. The v1 goal is one fast binary that can launch panes, send input, wait for completion heuristics, and capture results without paying interpreter startup cost each time.

## Install

```sh
cargo install --path .
```

Packaging is deferred for v1.

## Quickstart

```console
$ tmux-tools launch --cmd "bash" --name shell
$ tmux-tools send --target shell "echo hello from tmux-tools" --enter
$ tmux-tools capture --target shell --lines 10
```

```console
$ tmux-tools execute --target shell "cargo test" --format json --timeout 300
```

```console
$ tmux-tools spawn-agent codex --access read-only --name codex-helper
$ tmux-tools prompt --target codex-helper "explain src/main.rs" --idle-seconds 3 --format json
```

## Verb Summary

Most pane verbs accept `--target <name|id>`, `--format concise|json|raw`, `--session NAME`, and `--window NAME`. Defaults: `--format concise`, `--idle-seconds 2.0`, `--timeout 120.0`.

| Verb | Signature | Notes |
| --- | --- | --- |
| `launch` | `--cmd <SHELL> [--name NAME] [--split h\|v] [--size N]` | Creates a pane/window and registers optional `@tt-name`; splits only inside tmux. |
| `send` | `<TEXT> [--enter] [--literal] [--verify]` | Sends keys; `--enter` appends Enter (sent once). Add `--verify` to capture-and-retry Enter up to 3 times if the bottom line is unchanged (opt-in: can double-submit to non-echoing programs like password prompts). |
| `capture` | `[--lines N \| --all]` | Captures visible pane by default; `--all` captures full history. |
| `execute` | `<CMD> [--timeout SEC] [--no-wait]` | Wraps a command with markers and reports output, duration, timeout, and exit code. |
| `wait-idle` | `[--idle-seconds F] [--timeout SEC] [--until REGEX]` | Waits for quiet output, explicit regex, or timeout. |
| `prompt` | `<TEXT> [--idle-seconds F] [--timeout SEC] [--until REGEX]` | Sends text plus Enter, waits, then returns output since the prompt. |
| `spawn-agent` | `<AGENT> [--access PROFILE] [--name NAME] [--cwd PATH] [--split h\|v] [--size N] [-- EXTRA_ARGS...]` | Launches a configured agent profile and registers `@tt-agent`/`@tt-access`. |
| `kill` | `[--target name\|id]` | Kills the target pane. |
| `interrupt` | `[--target name\|id]` | Sends `C-c`. |
| `escape` | `[--target name\|id]` | Sends `Escape`. |
| `list` | `[--session NAME] [--all]` | Lists panes in the managed/current scope by default. |
| `status` | `[--target name\|id]` | Shows session, window, pane, name, and agent metadata. |

## Configuration

Agent profiles are loaded from built-ins and deep-merged with `~/.config/tmux-tools/agents.toml`. Existing built-ins can override `binary`, `ready_regex`, or individual access profiles; new agents need a `binary`.

```toml
# ~/.config/tmux-tools/agents.toml
[codex]
binary = "codex"
ready_regex = "^▌"

[codex.access.read-only]
args = ["--sandbox", "read-only"]

[codex.access.workspace-write]
args = ["--sandbox", "workspace-write"]

[codex.access.full-access]
args = ["--sandbox", "danger-full-access", "--ask-for-approval", "never"]

[demo]
binary = "/usr/local/bin/demo-agent"
ready_regex = "^ready"

[demo.access.default]
args = ["--safe"]
```

Built-ins: Codex has `read-only`, `workspace-write`, and `full-access`; Claude has `plan`, `accept-edits`, and `bypass`; Gemini has `default`. Always pass `--access` for Codex and Claude. `full-access` and `bypass` are dangerous and require explicit user permission.

`TMUX_TOOLS_TIMEOUT` overrides the default 120-second timeout for `execute`, `prompt`, and `wait-idle` when `--timeout` is omitted.

## Status

v1, API may change. CLI-only - MCP wrapper deferred.

## License

MIT, or similar permissive license. TODO: confirm before publishing.
