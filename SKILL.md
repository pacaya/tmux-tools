---
name: tmux-tools
description: "Use this skill when launching shell commands or AI sub-agents in a managed tmux session, monitoring their output, or composing send + wait + capture flows. Trigger on tmux-tools, tmux pane control, long-running shell commands, AI subagents, wait-idle, prompt, capture, execute, or spawn-agent workflows."
---

# tmux-tools

`tmux-tools` is a Rust CLI for driving tmux panes from agents: launch shells, spawn AI agents, send input, wait for quiet output, capture results, and coordinate panes by stable names.

## When to Use

- You need to drive an AI subagent such as Codex, Claude, Gemini, or a configured custom agent.
- You need to run a long shell command in tmux and capture its output later.
- You need a one-shot command wrapper that returns command output and exit status.
- You need to coordinate multiple panes by name instead of raw tmux pane ids.
- You need to compose `send` + `wait-idle` + `capture`, or use `prompt` for that common flow.
- You need programmatic tmux output in `json`, or raw pane output for debugging.

## Verbs Cheatsheet

Most pane verbs accept `--target <name|id>`, `--format concise|json|raw`, `--session NAME`, and `--window NAME`. `--format concise` is the default. Targets can be registered names, pane ids like `%3`, or window ids like `@7`.

`launch --cmd <SHELL> [--name NAME] [--split h|v|window] [--size N] [--bare]`: launch a shell command in a new tmux pane/window and optionally register a friendly name. Inside tmux the default is a horizontal split that gives the new pane 70% of the width so caller and callee stay side-by-side; pass `--split window` to revert to the legacy "new window" behavior. `--split h|v` keeps tmux's native 50/50 unless `--size N` overrides it; `--size N` also overrides the default 70% under the implicit split. By default the command is wrapped as `<cmd>; exec $SHELL` so the pane outlives a fast-failing command and its output stays visible; pass `--bare` for raw `tmux split-window <cmd>` semantics where the pane closes the moment the command exits. Example: `tmux-tools launch --cmd "bash" --name shell --split v --size 40`.

`send <TEXT> [--enter] [--literal] [--verify]`: send text to a target pane; `--enter` appends a single Enter, `--literal` uses tmux literal key sending, and `--verify` enables capture-and-retry of Enter (up to 3 attempts) when the bottom line is unchanged. Leave `--verify` off for non-echoing programs (passwords, full-screen TUIs) where retries can double-submit. Example: `tmux-tools send --target shell "cargo test" --enter`.

`capture [--lines N | --all]`: capture the target pane. With no line flag it captures the visible pane; `--lines N` captures the tail range; `--all` captures full history. Example: `tmux-tools capture --target build --lines 50`.

`execute <CMD> [--timeout SEC] [--no-wait]`: send a wrapped command to the target pane, wait for unique end markers, and report output, duration, timeout state, and exit code when available. Example: `tmux-tools execute --target shell "cargo test" --format json --timeout 300`.

`wait-idle [--idle-seconds F] [--timeout SEC] [--until REGEX]`: wait until the visible pane capture is unchanged for `F` seconds, an explicit regex matches, or timeout is reached. Defaults are `--idle-seconds 2.0` and `--timeout 120.0`. Example: `tmux-tools wait-idle --target codex-helper --idle-seconds 3 --until "Done"`.

`prompt <TEXT> [--idle-seconds F] [--timeout SEC] [--until REGEX]`: send text plus Enter, wait for completion, and return only output appended after the prompt. Defaults are `--idle-seconds 2.0` and `--timeout 120.0`. Example: `tmux-tools prompt --target codex-helper "explain this file" --idle-seconds 3`.

`spawn-agent <AGENT> [--access PROFILE] [--name NAME] [--cwd PATH] [--split h|v|window] [--size N] [--bare] [-- EXTRA_ARGS...]`: look up an agent in the registry, launch its binary with the selected access profile and extra args, then register pane metadata. Same default-split (30:70 horizontal) and `--split window` opt-out as `launch`. Same keep-open wrap as `launch` — if the agent crashes the pane survives as a plain shell so the failure output is preserved. Pass `--bare` to opt out. Note: `@tt-agent` is set at launch and not cleared, so `list`'s `agent=` column reflects the original launch even after the agent has exited. Example: `tmux-tools spawn-agent codex --access read-only --name codex-helper --cwd .`.

`kill`: kill the target pane and report the pane/name that was closed. By default refuses to kill the calling pane (`$TMUX_PANE`) — pass `--force` to override — and refuses to kill panes that tmux-tools did not create (no `@tt-name` or `@tt-agent`) or whose recorded `@tt-cwd` differs from the current cwd — pass `--any` to override. Example: `tmux-tools kill --target codex-helper`.

`interrupt`: send `C-c` to the target pane. Same `--force` (self-pane) and `--any` (non-owned or cross-cwd) safety guards as `kill`. Example: `tmux-tools interrupt --target build`.

`escape`: send `Escape` to the target pane. Same `--force` (self-pane) and `--any` (non-owned or cross-cwd) safety guards as `kill`. Example: `tmux-tools escape --target claude-plan`.

`send-enter`: send a standalone `Enter` to the target pane. Same `--force` (self-pane) and `--any` (non-owned or cross-cwd) safety guards as `kill`. Use when an agent didn't act on the Enter that `send --enter` or `prompt` already submitted; this nudges only Enter, never re-types the prompt. Example: `tmux-tools send-enter --target codex-helper`.

`list [--session NAME] [--all]`: list panes with registered name, agent, session, pane label, access, launch time, and status. By default it lists the managed `tmux-tools` session plus the current session when inside tmux. Example: `tmux-tools list --format json`.

`status [--target name|id]`: show current or target pane status. With no target outside tmux, it reports `not in tmux`. Example: `tmux-tools status --target shell --format json`.

## Common Patterns

Spawn a sub-agent and prompt it:

```sh
tmux-tools spawn-agent codex --access read-only --name codex-helper
tmux-tools prompt --target codex-helper "explain this file" --idle-seconds 3
```

Run a one-shot command with structured output:

```sh
tmux-tools execute --target shell "cargo test" --format json --timeout 300
```

Watch a long-running pane periodically:

```sh
tmux-tools capture --target build --lines 50
```

`capture` defaults to the visible pane; add `--all` when full scrollback history is required.

## Completion-Heuristics Gotchas

- `--idle-seconds` is the dominant completion signal: a quiet pane means done. If the pane prints heartbeat lines, increase `--idle-seconds`.
- `--until <regex>` short-circuits the idle wait when an explicit terminator string appears anywhere in the stripped visible capture.
- Built-in registry `ready_regex` values are `codex = "^▌"`, `claude = "^>"`, and `gemini = "^>"`. In this v1 build, `prompt` and `wait-idle` rely on idle/`--until`; pass `--until` when you need an explicit terminator.
- `--timeout` is a hard ceiling. `TMUX_TOOLS_TIMEOUT` overrides the default timeout for `execute`, `prompt`, and `wait-idle` only when `--timeout` is omitted; an explicit `--timeout` wins.
- `capture --lines 0` intentionally returns 0 lines. `--lines` is `Option<u32>`, so zero is not treated as false or unset.
- Idle polling samples the visible pane every 250 ms after stripping ANSI escape sequences.

## Agent Permission Notes

- Codex profiles: `read-only` maps to `--sandbox read-only` and is the safety profile to choose; `workspace-write` maps to `--sandbox workspace-write`; `full-access` maps to `--sandbox danger-full-access --ask-for-approval never` and must never be used without explicit user permission.
- Claude profiles: `plan` maps to `--permission-mode plan`; `accept-edits` maps to `--permission-mode acceptEdits`; `bypass` maps to `--permission-mode bypassPermissions` and is dangerous because it bypasses permission prompts.
- Gemini has only the `default` profile, with no additional args.
- Be explicit with `--access`. The registry chooses an agent's `default` profile when present, otherwise the first configured profile by name; the built-in Codex and Claude profiles do not define a `default`.
- Agent profiles are deep-merged from `~/.config/tmux-tools/agents.toml`, so users can override built-ins or add their own agents.

## Format Flags

Use `--format concise|json|raw`. `concise` is the default and strips ANSI, trims trailing blank lines, and collapses repeated idle-prompt/duplicate lines for agent-readable output. `json` is the intended path for programmatic consumption and includes structured fields for targets, names, output, durations, and exit status where relevant. `raw` prints the underlying tmux capture/output with minimal processing and is best for debugging.

## Targeting

`--target <name|id>` accepts a registered name, a pane id like `%3`, or a window id like `@7` whose active pane will be used. Registered metadata lives directly on tmux pane user options: `@tt-name`, `@tt-agent`, `@tt-access`, `@tt-launched-at`, and `@tt-cwd` (the cwd at launch time, used by destructive verbs to scope to caller-created panes). With no target, the smart default is the current pane when inside tmux; outside tmux it creates or uses the managed `tmux-tools` session and chooses its most recently active pane. `--session` and `--window` scope that smart default for pane verbs that resolve existing targets. `launch` creates a new pane/window according to its launch rules; do not use `--target` to choose a launch destination.
