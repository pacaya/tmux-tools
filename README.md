# tmux-tools

Rust CLI for LLM-driven tmux control. Single static binary, sub-10ms cold start.

`tmux-tools` exists because a 200-500ms Python cold start adds up when an agent fires commands from a hot loop. The v1 goal is one fast binary that can launch panes, send input, wait for completion heuristics, and capture results without paying interpreter startup cost each time.

## Install

```sh
cargo install --path .
```

`tmux-tools --version` reports the installed build. Packaging is deferred for v1.

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
| `launch` | `--cmd <SHELL> [--name NAME] [--split h\|v\|window] [--size N] [--bare]` | Creates a pane/window and registers optional `@tt-name`. Inside tmux the default is a horizontal split that gives the new pane 70% of the width (caller stays visible at 30% on the left); pass `--split window` to revert to the legacy "new window" behavior. `--split h\|v` keeps tmux's native 50/50 unless `--size N` overrides it; `--size N` also overrides the default 70% under the implicit split. By default the command is wrapped as `<cmd>; exec $SHELL` so the pane survives the command's exit and output is preserved; pass `--bare` for raw `tmux split-window <cmd>` semantics. A session it creates gets `history-limit` 50000 (see "Pane history and `prompt` results"). |
| `send` | `<TEXT> [--enter] [--literal] [--verify]` | Sends keys; `--enter` appends Enter (sent once). Add `--verify` to capture-and-retry Enter up to 3 times if the bottom line is unchanged (opt-in: can double-submit to non-echoing programs like password prompts). |
| `capture` | `[--lines N \| --all]` | Captures visible pane by default; `--all` requests full scrollback history. A pane on the alternate screen has no tmux scrollback, so `--all` there returns only the visible screen: raw and concise announce the limit on stderr, and JSON reports `scrollback_available: false`. |
| `execute` | `<CMD> [--timeout SEC] [--no-wait]` | Wraps a command with markers and reports output, duration, timeout, and exit code. |
| `wait-idle` | `[--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX] [--hint-lines N]` | Waits for a stable `ready_regex`, an explicit regex, quiet output, or timeout; quiet output does not complete when the surface's `busy_regex` classifies the pane as busy, in which case only a ready match, `--until`, or the timeout ends the wait. `--ready-stable-seconds` (default 2.0) is how long a `ready_regex` *or* `--until` match must hold before completing (`0` fires on first match). On a concise timeout, `--hint-lines` controls the bottom non-blank pane tail (default `10`; `0` disables it). |
| `prompt` | `<TEXT> [--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX]` | Sends text plus Enter, waits for completion, then returns everything from the pre-submission history mark, so the agent's echo is always included. Settles when the resolved surface classifies idle, or by idle detection only when the surface supplies no patterns. Reports turn observation and completeness on stderr (raw/concise) and as `turn_observed`/`complete` in JSON. |
| `spawn-agent` | `<AGENT> [--surface NAME] [--access PROFILE] [--resume ID] [--name NAME] [--cwd PATH] [--split h\|v\|window] [--size N] [--bare] [-- EXTRA_ARGS...]` | Launches a configured agent surface and registers `@tt-agent`/`@tt-surface` (plus `@tt-access`). `--surface` picks a named rendering; omitted, the agent's default surface is recorded. `--resume ID` validates the identifier against the resolved surface's declared shape, then appends that surface's resume arguments; a malformed identifier — or a surface with no resume syntax — fails before any pane is created, and the surface-supplied resume argument does not mark the pane surface-unvalidated. Trailing caller arguments additionally mark the pane `@tt-surface-unvalidated`. Same default-split (30:70 horizontal) and `--split window` opt-out as `launch`. Same keep-open wrap as `launch`; pass `--bare` to opt out. A session it creates gets `history-limit` 50000 (see "Pane history and `prompt` results"). The `agent=` column from `list` reflects the *original* launch — if the agent crashes the pane survives as a plain shell, but `@tt-agent` is not cleared. |
| `kill` | `[--target name\|id]` | Kills the target pane. |
| `interrupt` | `[--target name\|id]` | Sends the resolved surface's declared `interrupt_key` (`C-c` when the pane has no surface). Refuses when that key quits the agent while idle and the pane is not positively busy, and refuses any surface-unvalidated pane on such a surface; a surface declaring no quit hazard always sends. Same `--force`/`--any` guards as `kill`. |
| `escape` | `[--target name\|id]` | Sends `Escape`. |
| `send-enter` | `[--target name\|id]` | Sends `Enter`. Same `--force`/`--any` guards as `kill`. Useful when an agent didn't process the Enter from `send --enter`/`prompt`. |
| `list` | `[--session NAME] [--all]` | Lists panes in the managed/current scope by default. |
| `status` | `[--target name\|id]` | Shows session, window, pane, name, and agent metadata. |

## Configuration

Agent profiles are loaded from built-ins and deep-merged with `$XDG_CONFIG_HOME/tmux-tools/agents.toml` (falling back to `~/.config/tmux-tools/agents.toml`). Existing built-ins can override `binary`, individual access profiles, or individual surfaces; new agents need a `binary`. A readiness field written at the agent level (the pre-surface shape below) binds to the agent's **pre-surface rendering**; named `surfaces` carry their own fields. See "Surfaces" below.

```toml
# ~/.config/tmux-tools/agents.toml
# Codex's input glyph `›` is on screen idle *and* generating, so readiness keys off
# the status line: `· Ready · Context` when idle/done vs `· Working ·` while busy.
# The built-in codex surface already ships these measured patterns (stamped
# `codex-cli 0.155.1`); the override below is only needed to change them, and is
# shown for the config shape. Pairs with the --ready-stable-seconds debounce, which
# covers the <0.4s residual `· Ready ·` right after a prompt is sent.
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

# Claude Code. Claude's native chrome can't discriminate idle from busy (the prompt
# glyph `❯` is empty in both states, and the footer's `← for agents` suffix shows
# while generating too), and `claude --ax-screen-reader` has no native ready/busy
# chrome at all — so the built-in claude surfaces ship no readiness patterns and
# claude keeps its rich pre-surface rendering as the default. The status-line setup in
# "Detecting readiness from a custom Claude status line" below is opt-in: with it, a
# `[claude.surfaces.rich]` block (like the one shown there) declares the patterns where
# `busy_regex` is honored. The access profiles here match the built-in and need no
# override.
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

`ready_regex` is tested against the bottom non-blank line of the pane by default. Some agents (e.g. Cursor) render a status/footer row *below* their input prompt; set `ready_lines = N` to test the regex against the bottom `N` non-blank lines instead (the regex matches if any of them match). Defaults to `1`. Set `ready_lines = 0` to scan **every** non-blank line (no limit) — use this only with a uniquely-anchored pattern, since it also reaches conversation text above the input box. It's the right choice for a footer whose height varies, e.g. Claude's variable subagent rows (below). Both fields live on a surface, because a pattern is valid for exactly one rendering. A surface's `validated_version` records the agent version the manual smoke measured its declarations against; a flat surface becomes the agent's default only once it ships a `ready_regex`/`busy_regex` pair with that stamp (see "Surfaces" and "Validating vendor surfaces").

When a surface declares readiness patterns, `wait-idle`/`prompt` classify the pane through one three-valued function: `ready_regex` alone means idle, `busy_regex` alone means busy, and any ambiguity — neither matching, or both matching — is *unknown*. Idle detection is `wait-idle`'s completion fallback for exactly the cases the classifier cannot decide (no patterns, no surface, or *unknown*); it never overrides a *busy* classification, and it is not a parallel definition of idleness. `prompt` is stricter: it uses idle detection only when the resolved surface supplies no patterns or the pane resolves to no surface, so with patterns present an *unknown* capture never settles as idle.

A `ready_regex` match — or an explicit `--until` match — must hold continuously for `--ready-stable-seconds` (default 2.0) before `wait-idle`/`prompt` complete (with `ready_matched` / `until_matched`). This debounce guards against a stale indicator that is visible for only a single poll — e.g. a previous turn's status line still showing right after a new prompt is submitted — triggering a premature, wrong completion. Set `--ready-stable-seconds 0` to fire on the first match (legacy behavior; use it for a one-shot `--until` marker that may scroll off-screen before the window elapses).

> **`ready_regex` is version-sensitive chrome.** Agent TUIs change their input glyphs, status lines, and footer rows between releases, which silently breaks a `ready_regex` (it stops matching and falls back to idle/timeout) or, worse, makes it false-match. Re-validate these patterns after upgrading an agent CLI; each surface's `validated_version` records the agent version the manual smoke measured its declarations against.

### Surfaces

A **surface** is a named rendering of an agent. It carries the launch arguments that select the rendering and every behavior valid for that rendering only, so a `ready_regex` calibrated against one rendering never silently governs another. `spawn-agent --surface <name>` selects one; with no `--surface`, the agent's `default_surface` is used. The resolved surface is recorded on the pane as `@tt-surface`; a pane spawned with caller-supplied trailing arguments is additionally marked `@tt-surface-unvalidated`, because those arguments can change the rendering out from under the record.

A surface may also declare `resume.syntax` and `resume.id_regex`. `spawn-agent --resume <ID>` validates `ID` against the resolved surface's declared identifier shape, then appends the expanded syntax to the launch command. An identifier outside that shape — or a surface that declares no resume syntax — fails with a message and creates no pane, so the agent is never launched into an interactive session picker; resolving "the most recent session" is deliberately not provided. Because the resume argument comes from the surface itself, it is validated by construction and does not mark the pane `@tt-surface-unvalidated`.

```toml
[demo]
binary = "/usr/local/bin/demo-agent"
default_surface = "flat"
pre_surface_rendering = "rich"

[demo.surfaces.rich]
args = ["--profile", "tui"]
ready_regex = "^rich ready"
ready_lines = 1

[demo.surfaces.flat]
args = ["--profile", "dshline"]
ready_regex = "^flat ready"
busy_regex = "^flat busy"
validated_version = "demo-agent 1.0"
bracketed_paste = true
interrupt_key = "C-u"
quit_when_idle = true

[demo.surfaces.flat.resume]
syntax = "--resume {id}"
id_regex = "^[0-9a-f-]{36}$"

[demo.access.default]
args = []
```

Field summary:

- `args` — launch arguments selecting the rendering; applied before the access profile's.
- `ready_regex` / `ready_lines` — the idle-and-ready pattern, matched against the bottom `ready_lines` non-blank lines (`0` = whole pane). Valid for this rendering only.
- `busy_regex` — the generating pattern, matched against the whole capture.
- `validated_version` — the agent version the manual smoke measured this surface's declarations against. A flat surface is promoted to the default only once its `ready_regex`/`busy_regex` pair carries this stamp.
- `bracketed_paste` — whether the rendering enables bracketed paste. Declared capability only; the live flag is read from the pane before pasting.
- `interrupt_key` — the key `interrupt` sends for this rendering.
- `quit_when_idle` — whether that key quits the agent when the pane is idle.
- `resume.syntax` / `resume.id_regex` — the resume-argument template and the identifier shape.

`default_surface` names the surface `spawn-agent` uses when `--surface` is omitted; exactly one surface per agent is the default. A named default is honored only once it carries a validated `ready_regex`/`busy_regex` pair — the flat surface's promotion bar — so an agent whose flat surface has no validated patterns keeps its former default. When an `agents.toml` explicitly names a `default_surface` that fails that bar, the load reports it on stderr (`warning: agent …: default_surface … is not smoke-validated … recording … instead`) and records the pre-surface rendering, rather than silently substituting a different surface. `pre_surface_rendering` names the surface matching the rendering the agent launched with before surfaces existed: readiness fields written at the agent level (no `[agent.surfaces.*]` tables) bind there, and a pane with no `@tt-surface` record resolves there. The two differ once a flat surface becomes the default; naming a different `default_surface` never moves `pre_surface_rendering`.

A pre-surface `agents.toml` — agent-level `ready_regex`/`ready_lines` and no surfaces — still loads. An agent declared without surfaces loads as one surface built from those fields; that surface is both its pre-surface rendering and its default.

Built-in surfaces: `claude` ships `flat` (`--ax-screen-reader`, the non-alternate-screen rendering) and `rich` (its full-screen TUI). Neither declares readiness patterns — `--ax-screen-reader` has no native idle/busy chrome, so the flat surface cannot satisfy the promotion bar — so claude is not promoted and `rich` stays both its pre-surface rendering and its default. `codex` ships a single `default` surface, flat and already the default, carrying the measured `· Ready · Context` / `· Working ·` patterns stamped `codex-cli 0.155.1`. `cursor` and `agy` each ship a single unvalidated `default` surface and keep their previous behavior (out of scope for this smoke). `claude`'s two surfaces and `codex`'s `default` surface declare resume (`claude --resume <uuid>`, `codex resume <uuid>`, both a UUID); `cursor` and `agy` declare no resume syntax.

#### Validating vendor surfaces

Agent chrome changes between releases, so every pattern, bracketed-paste declaration and quit hazard above is validated against the live binary by a manually-run smoke, never by `cargo test`. Run it on a machine with the agents installed:

```sh
cargo run --features vendor-smoke --bin tt-vendor-smoke
```

For each `claude`, `codex` and README-documented `dsh` surface it launches the agent, checks a flat surface is still non-alternate-screen, checks the declared `bracketed_paste` against the pane's live `bracket_paste_flag`, establishes idleness from the surface's native boot chrome plus a content-stable window (never from the declared `ready_regex`, so the pattern check is not circular), samples the whole generating turn and requires that `ready_regex` first matches only after the last busy frame, checks that a large (~5 KB) multi-line paste reaches the composer as one unit (a full-size collapse placeholder, or the end sentinel in the composer region) with no turn started — transcript growth and busy frames count as independent turn evidence — confirms a stable idle before sending the declared `interrupt_key` and checking whether the agent quits, and compares the live `--version` with the `validated_version` stamp by exact version-token equality. It exits non-zero on any mismatch. A surface that declares no patterns — rich renderings, and `claude --ax-screen-reader`, which has no native ready/busy chrome — is noted rather than failed. When an agent's version changes, re-run the smoke and re-stamp `validated_version`.

#### `dsh` surfaces

`dsh` is deliberately not a built-in. Declare its two renderings in `agents.toml`:

```toml
[dsh]
binary = "dsh"
default_surface = "flat"
pre_surface_rendering = "rich"

# `dsh --profile tui` accepts `session-<uuid>` or a bare `<uuid>`; dsh prefixes
# `session-` itself. Measured against dsh 0.1.5-rc.1.
[dsh.surfaces.rich]
args = ["--profile", "tui"]
bracketed_paste = true
quit_when_idle = false
validated_version = "dsh 0.1.5-rc.1"

[dsh.surfaces.rich.resume]
syntax = "--resume {id}"
id_regex = "^(session-)?[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"

# `dsh --profile dshline` accepts only `dshline-<uuid>`. The measured footer reads
# `● ready · …` when idle and `… ctrl-c stop …` while generating, and C-c quits the
# idle agent; the validated pair makes this the promoted default. Measured against
# dsh 0.1.5-rc.1.
[dsh.surfaces.flat]
args = ["--profile", "dshline"]
ready_regex = "● ready ·"
busy_regex = "ctrl-c stop"
bracketed_paste = true
quit_when_idle = true
validated_version = "dsh 0.1.5-rc.1"

[dsh.surfaces.flat.resume]
syntax = "--resume {id}"
id_regex = "^dshline-[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"

[dsh.access.default]
args = []
```

`dsh --profile tui` is the rich, full-screen rendering; `dsh --profile dshline` is the flat, non-alternate-screen rendering. Both accept `--resume <id>`, with the identifier shapes above measured against and stamped with `dsh 0.1.5-rc.1`. The flat surface's measured `ready_regex`/`busy_regex` pair promotes it to the default; the rich surface stays the pre-surface rendering, so a pre-surface `agents.toml` override for `dsh` still governs `--profile tui`.

### Detecting readiness from a custom Claude status line

Claude's native footer can't discriminate idle from busy (the `← for agents` suffix is present while generating too, so a regex on it reports a premature "done"), and `claude --ax-screen-reader` has no native ready/busy chrome at all. The built-in `claude` surfaces therefore ship **no** readiness patterns and claude keeps its rich pre-surface rendering as the default; readiness falls back to idle detection, which is reliable but costs up to `--idle-seconds` per turn. For a faster, precise signal, customize Claude Code's status line (`~/.claude/statusline-command.sh`) to emit a Codex-style **state indicator** as the leading segment and point readiness at it in `agents.toml`. These fields are opt-in, not built-in. A typical indicator vocabulary, always rendered as `<indicator> | 🤖 <model> | …` on one line:

- `⚡ working` — generating (keep waiting)
- `🔐 permission` — awaiting an approval prompt (still busy)
- `⚙ N bg (…)` — background tasks/subagents still running (still busy)
- `⏸ waiting` — idle, awaiting input (**ready**)
- `✓ done` — turn complete (**ready**)

Point `claude`'s rich-surface readiness at the ready states in your `agents.toml`. The patterns go on the surface, where `busy_regex` is honored — the agent-level pre-surface shape carries only `ready_regex`/`ready_lines`. Add the same three fields under `[claude.surfaces.flat]` to use it on the flat rendering too:

```toml
[claude]
binary = "claude"

[claude.surfaces.rich]
ready_regex = "(✓ done|⏸ waiting) \\| 🤖"
busy_regex = "(⚡ working|🔐 permission|⚙ [0-9]+ bg( \\([^)]*\\))?) \\| 🤖"
ready_lines = 0   # scan the whole footer: persisting subagent rows push the status line up
```

The ` | 🤖` suffix anchors the match to the real status-line segment, so conversation text that merely contains "✓ done" can't false-match. The `busy_regex` covers **every** busy indicator the status line emits — `⚡ working`, `🔐 permission`, and `⚙ N bg (…)`. Leave any of them out and that state classifies as *unknown*, letting idle detection report a premature completion while the agent is blocked mid-turn; a static `🔐 permission` prompt is exactly that case. `ready_lines = 0` (scan every non-blank line) is what makes the ready match robust: completed subagent/background-task rows persist *below* the status line and push it up — by `N + 3` non-blank lines for `N` subagents — so any fixed window is eventually exceeded by a tall enough footer. The unique ` | 🤖` anchor keeps the unbounded scan safe. Pair it with the `--ready-stable-seconds` debounce so a stale `✓ done` lingering for one poll right after a new prompt can't false-complete.

The leading state indicator is **not** something Claude passes to the status line — it's populated by companion hooks (e.g. `Stop` / `SubagentStop` / `PreToolUse`) that record the main-agent state and a background-work count to per-session temp files the status-line script reads. Without those hooks the segment is absent and readiness simply falls back to idle.

A ready-to-install version of this setup — the status-line script, the state-writer hook, and the `settings.json` / `agents.toml` snippets to wire them — lives in [`examples/claude-statusline/`](examples/claude-statusline/).

Built-ins: Codex and Claude both have `read-only`, `workspace-write`, and `full-access` (plus a safe `default` == `read-only`); Cursor adds a `plan` tier; Antigravity (`agy`) has only `workspace-write` and `full-access`. Always pass `--access` for Codex and Claude. `full-access` is dangerous and requires explicit user permission.

All agents share one access-profile vocabulary (`read-only` / `workspace-write` / `full-access`) so a single `--access` value works across agents, even though each maps the tier onto its own flags. Claude maps `read-only` → `--permission-mode plan`, `workspace-write` → `--permission-mode acceptEdits`, and `full-access` → `--dangerously-skip-permissions` (≡ `bypassPermissions`). Codex maps them onto `--sandbox read-only` / `workspace-write` / `danger-full-access --ask-for-approval never`.

Cursor and Antigravity (now built-ins) reuse the same triad. Cursor maps `read-only` → `--mode ask` (Q&A/analysis, no edits; the default; `plan` is the same tier in plan-building mode), `workspace-write` → `--sandbox enabled` (read+write+shell contained to the workspace, network restricted), and `full-access` → `--force --sandbox disabled` ("run everything", unrestricted, no approvals). Antigravity (`agy`) only exposes `workspace-write` (default, approval-gated) and `full-access` (`--dangerously-skip-permissions`) — version 1.0.3 has **no interactive read-only mode** (no `--plan`/`--ask`/`--permission-mode`), so for a guaranteed no-write run use `agy -p "<prompt>"` headless instead. `full-access` is dangerous and requires explicit user permission.

`TMUX_TOOLS_TIMEOUT` overrides the default 120-second timeout for `execute`, `prompt`, and `wait-idle` when `--timeout` is omitted.

On `wait-idle` timeout, concise output starts with `reason=timed_out duration=<seconds> idle_for=<seconds>`, then a greppable marker and the bottom `--hint-lines` non-blank lines from the loop's final capture. The hint reuses that capture; it does not make another `capture-pane` call. JSON includes numeric `idle_for` seconds and keeps `final_capture` unchanged. Non-timeout concise output keeps its existing one-line shape.

## Pane history and `prompt` results

`launch` and `spawn-agent` raise the `history-limit` of **every session they create** to 50000 before the launched command's window can emit output. `history-limit` is a tmux **session** option (it cannot be set per pane); new windows inherit it. tmux-tools never changes the option on a session it did not create, and never sets it globally. `tmux show-options -t <session> history-limit` reports it. This is what lets `prompt` return a completed turn whose response scrolled past the visible screen: tmux defaults the limit to 2000, so a deep turn can silently drop its own beginning.

For panes created in sessions tmux-tools did not create, set the same default in `~/.tmux.conf` (or on the session before its windows are created):

```tmux
set-option -g history-limit 50000
```

The cost is memory: tmux stores roughly **70 bytes per history line**, so 50000 lines is about 3.5 MB per pane. Lower the value if many high-history panes would be expensive; `prompt` reports an incomplete result rather than guessing when history has evicted lines. tmux-tools never writes `~/.tmux.conf`.

`prompt` takes a **history mark** at the submission point (the cursor row) before it submits, and never advances it: the result begins there and always includes the agent's echo of the prompt. Output from before submission is excluded wherever that removes neither the echo nor the turn's output; when a redraw places either above the mark (a footer or composer redrawn in place, or the alternate screen), it is kept. Echo exclusion is deliberately not attempted. `prompt` reports two independent dimensions on every run:

- **Turn observation** — whether the resolved surface's `busy_regex` was seen between submission and settle. It says the agent took the turn; it makes no claim about where content begins.
- **Completeness** — whether everything after the mark could be supplied. A result is incomplete when the pane is on the alternate screen (no tmux scrollback) or history has evicted lines. Eviction is read from the pane's `history_collected` counter when the running tmux provides it (any increase between mark and settle); otherwise a pane counts as evicted once `history_size >= history_limit - history_limit / 10`, the floor tmux frees to when it trims history.

Raw and concise carry both dimensions on stderr; JSON carries them as the `turn_observed` and `complete` fields. They are always stated, even when the result is empty.

`prompt` settles on the resolved surface's classification or, when the surface supplies no patterns, on idle detection; with patterns present it never treats an unknown capture (neither or both matching) as idle. `wait-idle` is unchanged and keeps its idle-detection fallback.

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

## Testing

The integration suite drives a real tmux server, so `tmux` must be on `PATH`. Several tests need the
`tt-fake-tui` fixture, a second `[[bin]]` that a normal build deliberately does not produce. Run the
full suite with that fixture enabled:

```sh
cargo test --workspace --features test-fixtures
```

A bare `cargo test` still passes; it compiles the fixture-gated tests out and simply runs that many
fewer tests.

The live vendor surfaces are validated separately, by a manual smoke that never runs under
`cargo test` (it drives the real `claude`, `codex` and `dsh` binaries):

```sh
cargo run --features vendor-smoke --bin tt-vendor-smoke
```

See "Validating vendor surfaces" above for what it checks. Re-run it and re-stamp each surface's
`validated_version` whenever a driven agent's version changes.

## Status

v1, API may change. CLI-only - MCP wrapper deferred.

## License

MIT, or similar permissive license. TODO: confirm before publishing.
