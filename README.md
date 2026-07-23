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

Most pane verbs accept `--target <name|id>`, `--format concise|json|raw`, `--session NAME`, and `--window NAME`. Defaults: `--format concise`, `--idle-seconds 2.0`, `--ready-stable-seconds 2.0`, `--timeout 120.0`.

| Verb | Signature | Notes |
| --- | --- | --- |
| `launch` | `--cmd <SHELL> [--name NAME] [--split h\|v\|window] [--size N] [--bare]` | Creates a pane/window and registers optional `@tt-name`. Inside tmux the default is a horizontal split that gives the new pane 70% of the width (caller stays visible at 30% on the left); pass `--split window` to revert to the legacy "new window" behavior. `--split h\|v` keeps tmux's native 50/50 unless `--size N` overrides it; `--size N` also overrides the default 70% under the implicit split. By default the command is wrapped as `<cmd>; exec $SHELL` so the pane survives the command's exit and output is preserved; pass `--bare` for raw `tmux split-window <cmd>` semantics. |
| `send` | `<TEXT> [--enter] [--literal] [--verify]` | Sends keys; `--enter` appends Enter (sent once). Add `--verify` to capture-and-retry Enter up to 3 times if the bottom line is unchanged (opt-in: can double-submit to non-echoing programs like password prompts). |
| `capture` | `[--lines N \| --all]` | Captures visible pane by default; `--all` captures full history. |
| `execute` | `<CMD> [--timeout SEC] [--no-wait]` | Wraps a command with markers and reports output, duration, timeout, and exit code. |
| `wait-idle` | `[--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX] [--hint-lines N]` | Waits for quiet output, explicit regex, or timeout. `--ready-stable-seconds` (default 2.0) is how long a `ready_regex` *or* `--until` match must hold before completing (`0` fires on first match). On a concise timeout, `--hint-lines` controls the bottom non-blank pane tail (default `10`; `0` disables it). |
| `prompt` | `<TEXT> [--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX]` | Sends text plus Enter, waits, then returns output since the prompt. |
| `spawn-agent` | `<AGENT> [--access PROFILE] [--name NAME] [--cwd PATH] [--split h\|v\|window] [--size N] [--bare] [-- EXTRA_ARGS...]` | Launches a configured agent profile and registers `@tt-agent`/`@tt-access`. Same default-split (30:70 horizontal) and `--split window` opt-out as `launch`. Same keep-open wrap as `launch`; pass `--bare` to opt out. The `agent=` column from `list` reflects the *original* launch — if the agent crashes the pane survives as a plain shell, but `@tt-agent` is not cleared. |
| `kill` | `[--target name\|id]` | Kills the target pane. |
| `interrupt` | `[--target name\|id]` | Sends `C-c`. |
| `escape` | `[--target name\|id]` | Sends `Escape`. |
| `send-enter` | `[--target name\|id]` | Sends `Enter`. Same `--force`/`--any` guards as `kill`. Useful when an agent didn't process the Enter from `send --enter`/`prompt`. |
| `list` | `[--session NAME] [--all]` | Lists panes in the managed/current scope by default. |
| `status` | `[--target name\|id]` | Shows session, window, pane, name, and agent metadata. |

## Configuration

Agent profiles are loaded from built-ins and deep-merged with `$XDG_CONFIG_HOME/tmux-tools/agents.toml` (falling back to `~/.config/tmux-tools/agents.toml`). Existing built-ins can override `binary`, `ready_regex`, `ready_lines`, or individual access profiles; new agents need a `binary`.

```toml
# ~/.config/tmux-tools/agents.toml
# Codex's input glyph `›` is on screen idle *and* generating, so readiness keys off
# the status line: `· Ready · Context` when idle/done vs `· Working ·` while busy.
# (The built-in codex profile ships no ready_regex — it falls back to idle — so opt
# into the faster status-line signal here.) Pairs with the --ready-stable-seconds
# debounce, which covers the <0.4s residual `· Ready ·` right after a prompt is sent.
[codex]
binary = "codex"
ready_regex = "· Ready · Context"
ready_lines = 2

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

# Cursor CLI. Its input prompt sits above a mode/status/cwd footer (3 rows), so
# ready_lines widens the ready_regex scan to the bottom 4 non-blank lines.
[cursor]
binary = "cursor-agent"
ready_regex = "→ (Add a follow-up|Plan, search, build anything)\\s*$"
ready_lines = 4

[cursor.access.read-only]
args = ["--mode", "ask"]

[cursor.access.workspace-write]
args = ["--sandbox", "enabled"]

[cursor.access.full-access]
args = ["--force", "--sandbox", "disabled"]

# Claude Code. The built-in claude profile ships NO ready_regex (like codex): the
# prompt glyph `❯` is empty in both states and the footer's `← for agents` suffix
# shows while generating too, so native chrome can't discriminate idle from busy —
# readiness falls back to idle detection, which is reliable for Claude. For a faster
# signal, customize Claude's status line and opt in with `ready_lines = 0` (see
# "Detecting readiness from a custom Claude status line" below). The access profiles
# here match the built-in and need no override.
[claude]
binary = "claude"

[claude.access.read-only]
args = ["--permission-mode", "plan"]

[claude.access.workspace-write]
args = ["--permission-mode", "acceptEdits"]

[claude.access.full-access]
args = ["--dangerously-skip-permissions"]

# Antigravity CLI. Its bottom-most line is a footer: `? for shortcuts` when idle,
# `esc to cancel` while generating. agy 1.0.3 has no interactive read-only mode.
[agy]
binary = "agy"
ready_regex = "\\? for shortcuts"

[agy.access.workspace-write]
args = []

[agy.access.full-access]
args = ["--dangerously-skip-permissions"]
```

`ready_regex` is tested against the bottom non-blank line of the pane by default. Some agents (e.g. Cursor) render a status/footer row *below* their input prompt; set `ready_lines = N` to test the regex against the bottom `N` non-blank lines instead (the regex matches if any of them match). Defaults to `1`. Set `ready_lines = 0` to scan **every** non-blank line (no limit) — use this only with a uniquely-anchored pattern, since it also reaches conversation text above the input box. It's the right choice for a footer whose height varies, e.g. Claude's variable subagent rows (below).

A `ready_regex` match — or an explicit `--until` match — must hold continuously for `--ready-stable-seconds` (default 2.0) before `wait-idle`/`prompt` complete (with `ready_matched` / `until_matched`). This debounce guards against a stale indicator that is visible for only a single poll — e.g. a previous turn's status line still showing right after a new prompt is submitted — triggering a premature, wrong completion. Set `--ready-stable-seconds 0` to fire on the first match (legacy behavior; use it for a one-shot `--until` marker that may scroll off-screen before the window elapses).

> **`ready_regex` is version-sensitive chrome.** Agent TUIs change their input glyphs, status lines, and footer rows between releases, which silently breaks a `ready_regex` (it stops matching and falls back to idle/timeout) or, worse, makes it false-match. Re-validate these patterns after upgrading an agent CLI; the shipped configs note the version they were validated against.

### Detecting readiness from a custom Claude status line

The built-in `claude` profile ships no `ready_regex` — Claude's native footer can't discriminate idle from busy (the `← for agents` suffix is present while generating too, so a regex on it reports a premature "done"), so readiness falls back to idle detection. That's reliable but costs up to `--idle-seconds` per turn. For a faster, precise signal, customize Claude Code's status line (`~/.claude/statusline-command.sh`) to emit a Codex-style **state indicator** as the leading segment and point `claude`'s `ready_regex` at it. A typical indicator vocabulary, always rendered as `<indicator> | 🤖 <model> | …` on one line:

- `⚡ working` — generating (keep waiting)
- `🔐 permission` — awaiting an approval prompt (still busy)
- `⚙ N bg (…)` — background tasks/subagents still running (still busy)
- `⏸ waiting` — idle, awaiting input (**ready**)
- `✓ done` — turn complete (**ready**)

Point `claude`'s readiness at the two ready states in your `agents.toml`:

```toml
[claude]
binary = "claude"
ready_regex = "(✓ done|⏸ waiting) \\| 🤖"
ready_lines = 0   # scan the whole footer: persisting subagent rows push the status line up
```

The ` | 🤖` suffix anchors the match to the real status-line segment, so conversation text that merely contains "✓ done" can't false-match. `ready_lines = 0` (scan every non-blank line) is what makes this robust: completed subagent/background-task rows persist *below* the status line and push it up — by `N + 3` non-blank lines for `N` subagents — so any fixed window is eventually exceeded by a tall enough footer. The unique ` | 🤖` anchor keeps the unbounded scan safe. Pair it with the `--ready-stable-seconds` debounce so a stale `✓ done` lingering for one poll right after a new prompt can't false-complete.

The leading state indicator is **not** something Claude passes to the status line — it's populated by companion hooks (e.g. `Stop` / `SubagentStop` / `PreToolUse`) that record the main-agent state and a background-work count to per-session temp files the status-line script reads. Without those hooks the segment is absent and readiness simply falls back to idle. `⚡ working`, `🔐 permission`, and `⚙ N bg` deliberately do **not** match `ready_regex` — they're "still busy": while subagents/background agents run, the footer animates (per-second task-row timers) so idle stays suppressed and completion fires only once everything is truly done. The one busy state idle can't distinguish is a *static* `🔐 permission` prompt (it goes quiet, so idle reports done) — which for full-access driving never arises.

A ready-to-install version of this setup — the status-line script, the state-writer hook, and the `settings.json` / `agents.toml` snippets to wire them — lives in [`examples/claude-statusline/`](examples/claude-statusline/).

Built-ins: Codex and Claude both have `read-only`, `workspace-write`, and `full-access` (plus a safe `default` == `read-only`); Cursor adds a `plan` tier; Antigravity (`agy`) has only `workspace-write` and `full-access`. Always pass `--access` for Codex and Claude. `full-access` is dangerous and requires explicit user permission.

All agents share one access-profile vocabulary (`read-only` / `workspace-write` / `full-access`) so a single `--access` value works across agents, even though each maps the tier onto its own flags. Claude maps `read-only` → `--permission-mode plan`, `workspace-write` → `--permission-mode acceptEdits`, and `full-access` → `--dangerously-skip-permissions` (≡ `bypassPermissions`). Codex maps them onto `--sandbox read-only` / `workspace-write` / `danger-full-access --ask-for-approval never`.

Cursor and Antigravity (now built-ins) reuse the same triad. Cursor maps `read-only` → `--mode ask` (Q&A/analysis, no edits; the default; `plan` is the same tier in plan-building mode), `workspace-write` → `--sandbox enabled` (read+write+shell contained to the workspace, network restricted), and `full-access` → `--force --sandbox disabled` ("run everything", unrestricted, no approvals). Antigravity (`agy`) only exposes `workspace-write` (default, approval-gated) and `full-access` (`--dangerously-skip-permissions`) — version 1.0.3 has **no interactive read-only mode** (no `--plan`/`--ask`/`--permission-mode`), so for a guaranteed no-write run use `agy -p "<prompt>"` headless instead. `full-access` is dangerous and requires explicit user permission.

`TMUX_TOOLS_TIMEOUT` overrides the default 120-second timeout for `execute`, `prompt`, and `wait-idle` when `--timeout` is omitted.

On `wait-idle` timeout, concise output starts with `reason=timed_out duration=<seconds> idle_for=<seconds>`, then a greppable marker and the bottom `--hint-lines` non-blank lines from the loop's final capture. The hint reuses that capture; it does not make another `capture-pane` call. JSON includes numeric `idle_for` seconds and keeps `final_capture` unchanged. Non-timeout concise output keeps its existing one-line shape.

## Library: configurable invocation

`tmux_tools_core` exposes a programmatic API for shaping how every tmux subprocess is spawned. Re-exported from the crate root: `TmuxInvocation`, `set_global_invocation`, `with_invocation`.

### `TmuxInvocation`

```rust
pub struct TmuxInvocation {
    pub prefix: Vec<String>,   // command prefix (empty by default)
    pub socket: Option<String>,  // tmux socket name → `-L <name>` (None by default)
    pub tmux_bin: String,       // tmux binary name (defaults to "tmux")
}
```

`TmuxInvocation::default()` → `tmux_bin = "tmux"`, empty `prefix`, `socket = None`.

### `set_global_invocation(invocation: Option<TmuxInvocation>)`

Sets or clears the process-wide default invocation. Pass `None` to restore plain `tmux`.

### `with_invocation(inv: TmuxInvocation, f: impl FnOnce() -> T) -> T`

RAII thread-local override. Correct for per-thread / concurrent use (e.g. `spawn_blocking` threads each running commands under a different user). Takes precedence over the global invocation.

### Resolution order

thread-local → process-global → plain `tmux` (`TmuxInvocation::default()`).

### Command construction

Every `tmux::run*` call resolves the active invocation and builds:

- **Prefix non-empty:** `Command::new(prefix[0])` with args `prefix[1..] ++ [tmux_bin] ++ ["-L", socket]? ++ user_args`
- **Prefix empty:** `Command::new(tmux_bin)` with args `["-L", socket]? ++ user_args`

This is how a consumer (e.g. SilverBond) runs all tmux control commands (`send-keys`, `capture-pane`, `new-session`, etc.) as another user via `sudo -u <user> … -L <socket>`.

### Example

```rust
use tmux_tools_core::{with_invocation, TmuxInvocation};

with_invocation(
    TmuxInvocation {
        prefix: vec!["sudo".into(), "-u".into(), "agent".into(), "-H".into(), "--".into()],
        socket: Some("silverbond".into()),
        tmux_bin: "tmux".into(),
    },
    || {
        // Every tmux call in this closure runs as:
        // sudo -u agent -H -- tmux -L silverbond <args>
        tmux_tools_core::tmux::run_checked(&["new-session", "-d", "-s", "work"])?;
        Ok::<_, anyhow::Error>(())
    },
)?;
```

## Status

v1, API may change. CLI-only - MCP wrapper deferred.

## License

MIT, or similar permissive license. TODO: confirm before publishing.
