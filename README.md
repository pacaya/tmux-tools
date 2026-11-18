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

Every verb runs on the selected host, tmux, Orca, Rex or Herdr (see "Hosts"). Every `--format json` object carries `host`; `list --format json` rows carry `host` and `host_state`. On every host a `--name` value must match `[A-Za-z0-9._-]+` and must not be `.` or `..`; any other value exits 1 with `invalid name: <name>` and creates no pane. An optional verb or option the host lacks (`rebalance`, `--split tile`, `--size`; on Orca, Rex and Herdr also `--session` and `--window`) exits 3 with `unsupported: <verb> on <host>`.

| Verb | Signature | Notes |
| --- | --- | --- |
| `launch` | `--cmd <SHELL> [--name NAME] [--split tile\|h\|v\|window] [--size N] [--bare]` | Creates a pane/window and always registers `@tt-name`: the `--name` value, or, when `--name` is absent, a name derived from the command's first word plus a random suffix (`--cmd 'npm run dev'` → `npm-a3f2`; the stem is `pane` when that word has no letter or digit). `concise` and `json` output report the registered name, which `--target` accepts like any other. **The default changed:** inside tmux, placement is now `tile` (it used to be a horizontal split giving the new pane 70%). A tile placement splits, then retiles the window with tmux's `tiled` layout and marks it with the `@tt-tiled` window option, when the window already held a pane tmux-tools owns (`@tt-name` or `@tt-agent`, from the same cwd) before the split. First-pane rule: the first pane put into a window not yet owned, such as your own shell's window, is still the horizontal split giving it 70%, with no retile, so tiling engages from the second pane. Sizing opts out of tiling and is placed as before: `--split h\|v` (tmux's native 50/50 unless `--size N` overrides it), `--size N` alone (a horizontal split at N%), and `--split window` (a new window); `--split h\|v` and `--size` also clear `@tt-tiled` on the window they target. `--split tile --size N` is rejected, because a tiled layout has no percentage. Without `--target`, `--session`/`--window` still create a new window unless `--split tile\|h\|v` is given, which splits the scoped window. By default the command is wrapped as `<cmd>; exec $SHELL` so the pane survives the command's exit and output is preserved; pass `--bare` for raw `tmux split-window <cmd>` semantics. A session it creates gets `history-limit` 50000 (see "Pane history and `prompt` results"). |
| `send` | `<TEXT> [--enter] [--literal] [--verify]` | Sends keys; `--enter` appends Enter (sent once). Add `--verify` to capture-and-retry Enter up to 3 times if the bottom line is unchanged (opt-in: can double-submit to non-echoing programs like password prompts). |
| `capture` | `[--lines N \| --all]` | Captures visible pane by default; `--all` requests full scrollback history. A pane on the alternate screen has no tmux scrollback, so `--all` there returns only the visible screen: raw and concise announce the limit on stderr, and JSON reports `scrollback_available: false`. |
| `execute` | `<CMD> [--timeout SEC] [--no-wait]` | Wraps a command with markers and reports output, duration, timeout, and exit code. |
| `wait-idle` | `[--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX] [--hint-lines N]` | Waits for a stable `ready_regex`, an explicit regex, quiet output, or timeout; quiet output does not complete when the surface's `busy_regex` classifies the pane as busy, in which case only a ready match, `--until`, or the timeout ends the wait. `--ready-stable-seconds` (default 2.0) is how long a `ready_regex` *or* `--until` match must hold before completing (`0` fires on first match). On a concise timeout, `--hint-lines` controls the bottom non-blank pane tail (default `10`; `0` disables it). |
| `prompt` | `<TEXT> [--idle-seconds F] [--ready-stable-seconds F] [--timeout SEC] [--until REGEX]` | Sends text plus Enter, waits for completion, then returns everything from the pre-submission history mark, so the agent's echo is always included. Settles when the resolved surface classifies idle, or by idle detection only when the surface supplies no patterns. Reports turn observation and completeness on stderr (raw/concise) and as `turn_observed`/`complete` in JSON. |
| `spawn-agent` | `<AGENT> [--surface NAME] [--access PROFILE] [--resume ID] [--name NAME] [--cwd PATH] [--split tile\|h\|v\|window] [--size N] [--bare] [-- EXTRA_ARGS...]` | Launches a configured agent surface and registers `@tt-agent`/`@tt-surface` (plus `@tt-access`) and `@tt-name`: the `--name` value, or, when `--name` is absent, a name derived from the agent binary's basename plus a random suffix (`codex-91bd`); `concise` and `json` output report it. `--surface` picks a named rendering; omitted, the agent's default surface is recorded. `--resume ID` validates the identifier against the resolved surface's declared shape, then appends that surface's resume arguments; a malformed identifier — or a surface with no resume syntax — fails before any pane is created, and the surface-supplied resume argument does not mark the pane surface-unvalidated. Trailing caller arguments additionally mark the pane `@tt-surface-unvalidated`. Same changed default as `launch`: tiled placement inside tmux instead of the former 70% horizontal split, the same first-pane rule, the same sizing opt-outs (`--split h\|v`, `--size N` alone, `--split window`), and the same rejection of `--split tile --size N`. Same keep-open wrap as `launch`; pass `--bare` to opt out. A session it creates gets `history-limit` 50000 (see "Pane history and `prompt` results"). The `agent=` column from `list` reflects the *original* launch — if the agent crashes the pane survives as a plain shell, but `@tt-agent` is not cleared. |
| `kill` | `[--target name\|id]` | Kills the target pane. |
| `interrupt` | `[--target name\|id]` | Sends the resolved surface's declared `interrupt_key` (`C-c` when the pane has no surface). Refuses when that key quits the agent while idle and the pane is not positively busy, and refuses any surface-unvalidated pane on such a surface; a surface declaring no quit hazard always sends. Same `--force`/`--any` guards as `kill`. |
| `escape` | `[--target name\|id]` | Sends `Escape`. |
| `send-enter` | `[--target name\|id]` | Sends `Enter`. Same `--force`/`--any` guards as `kill`. Useful when an agent didn't process the Enter from `send --enter`/`prompt`. |
| `list` | `[--session NAME] [--all]` | Lists panes in the managed/current scope by default. JSON rows carry `parent`, `team`, `shim_id` (each `null` when absent) and `orphan`; the table has a `PARENT` column after `AGENT`, which shows `orphan:<parent>` (or `orphan`) for an Orphan: a pane whose parent is not live, or is a newer pane of the same name, or a Teammate pane whose Team has no running Launcher. `orphan` is reported only; no verb closes a pane for it. On Orca it lists the Pane records of the current runtime: `session` is the pane's worktree id, `pane` its `term_…` handle, and `status` is `running`, `dead` (shown once, then the record is deleted) or `unknown` (the pane list was incomplete; stderr says `list incomplete: orca`). |
| `status` | `[--target name\|id]` | Shows session, window, pane, name, and agent metadata; `concise` and `json` add the pane's window geometry (`min_pane_cols`, `min_pane_rows`) and floor verdict (`below_floor`, see "Floors"). `raw` is unchanged. |
| `rebalance` | `[--target name\|id]` | Retiles the target pane's window with tmux's `tiled` layout, the arrangement tile placement applies, and reports it: `concise` and `json` carry `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor` with the meanings `launch` gives them (see "Floors"); `raw` prints the window id. Use it to repair a window left ragged, for example by a raw `tmux split-window`, instead of a raw `tmux select-layout tiled`. Inside tmux with no selector it acts on the calling pane's window; outside tmux it needs `--target`, `--session` or `--window` and otherwise fails, so it never creates the managed session. It refuses with a non-zero exit, leaving the window untouched, unless the window carries `@tt-tiled` (layout intent) and holds a pane tmux-tools owns (`@tt-name` or `@tt-agent`, from the same cwd) — the window guard `kill` applies before it retiles; the error names the guard that failed, and there is no override. It moves no pane, so it cannot repair an over-full window: exit 0 with `below_floor` true means the window is still below its effective floor. Like `status`, it uses the registry only for floors: an `agents.toml` that cannot be read or parsed gives a warning and the built-in floors. |
| `notify` | `--title T --body B [--target name\|id]` | Shows a desktop notification for the target pane, or for the calling pane (`TMUX_PANE`) when `--target` is absent; without `--target` and with `TMUX_PANE` unset it exits 1. On tmux it writes the tmux-wrapped OSC 777 sequence to the pane's tty, and tmux passes `ESC ]777;notify;T;B BEL` to the client's terminal; control bytes are removed from the title and body and `;` becomes `,`. Under tmux, delivery needs the tmux option `allow-passthrough` set to `on` (only a pane that is visible in an attached client) or `all` (any pane), and the outer terminal must support OSC 777. When the pane's session has no attached client, or the host cannot notify, it exits 0 and prints `notify: not delivered on <host>` on stderr. Orca has no notification command (its own agent hooks notify), so on Orca `notify` always reports `notify: not delivered on orca`. |
| `self` | `[--format concise\|json\|raw]` | Reports whether the calling pane is driven by tmux-tools: `json` is `{"host":"tmux","driven":bool,"name":string\|null,"agent":string\|null}`. `driven` is true when the pane has `@tt-name` or `@tt-agent`; `name` and `agent` are null when unknown. Outside tmux it reports `host: tmux, driven: false`. On Orca the calling pane is `ORCA_TERMINAL_HANDLE` (required), and it is driven when a live Pane record holds that handle. `json` also carries `team` (team id or `null`), `role` (`leader` inside a team, `teammate` in a Teammate pane, else `null`) and `parent`. |
| `claude-teams` | `[-- CLAUDE_ARGS...]` | Runs Claude in a Team in the calling pane: every teammate Claude starts is a tmux-tools pane. See "Claude teams". |

## Configuration

### Host selection: `TMUX_TOOLS_HOST`

`TMUX_TOOLS_HOST=tmux|orca|rex|herdr` selects the host explicitly; any other value fails every verb with exit 1 and names the valid values (`tmux`, `orca`, `rex`, `herdr`). Unset, the first match selects: `$TMUX` set → tmux; `ORCA_TERMINAL_HANDLE` set → orca; `REX_BLOCK` set → rex; `HC_TERMINAL_ID` set → herdr; otherwise tmux, with the Managed session outside tmux as before. When the selected host's CLI or runtime is unreachable, the verb exits 1 and names the host and what selected it; it never falls back to another host.

### Agent profiles

Agent profiles are loaded from built-ins and deep-merged with `$XDG_CONFIG_HOME/tmux-tools/agents.toml` (falling back to `~/.config/tmux-tools/agents.toml`). Existing built-ins can override `binary`, individual access profiles, or individual surfaces; new agents need a `binary`. A readiness field written at the agent level (the pre-surface shape below) binds to the agent's **pre-surface rendering**; named `surfaces` carry their own fields. See "Surfaces" below.

```toml
# ~/.config/tmux-tools/agents.toml
# Codex's input glyph `›` is on screen idle *and* generating, so readiness keys off
# the status line: `· Ready · Context` when idle/done vs `· Working ·` while busy.
# The built-in codex surface already ships these measured patterns (stamped
# `codex-cli 0.168.0`) and the launch argument `--no-alt-screen`; the override below
# is only needed to change the patterns, and is shown for the config shape. It keeps
# `--no-alt-screen`, because a surface's `args` change only when written. Pairs with the --ready-stable-seconds debounce, which
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
- `min_cols` / `min_rows` — the surface's floor, the smallest pane its readiness signal stays matchable in (see "Floors").

`default_surface` names the surface `spawn-agent` uses when `--surface` is omitted; exactly one surface per agent is the default. A named default is honored only once it carries a validated `ready_regex`/`busy_regex` pair — the flat surface's promotion bar — so an agent whose flat surface has no validated patterns keeps its former default. When an `agents.toml` explicitly names a `default_surface` that fails that bar, the load reports it on stderr (`warning: agent …: default_surface … is not smoke-validated … recording … instead`) and records the pre-surface rendering, rather than silently substituting a different surface. `pre_surface_rendering` names the surface matching the rendering the agent launched with before surfaces existed: readiness fields written at the agent level (no `[agent.surfaces.*]` tables) bind there, and a pane with no `@tt-surface` record resolves there. The two differ once a flat surface becomes the default; naming a different `default_surface` never moves `pre_surface_rendering`.

A pre-surface `agents.toml` — agent-level `ready_regex`/`ready_lines` and no surfaces — still loads. An agent declared without surfaces loads as one surface built from those fields; that surface is both its pre-surface rendering and its default.

Built-in surfaces: `claude` ships `flat` (`--ax-screen-reader`, the reduced screen-reader rendering) and `rich` (Claude's own interface in classic scroll mode). `rich` has the launch arguments `--settings '{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}'` (two argv items, placed before the access-profile arguments). Claude is full screen only when `CLAUDE_CODE_NO_FLICKER=1` is set, and the user settings file overrides the process environment; `--settings` merges with the user settings, so a `CLAUDE_CODE_NO_FLICKER=1` user setting does not apply to panes that `spawn-agent claude` starts on `rich`. Those panes keep tmux scrollback (`#{alternate_on}` is `0`), and on Herdr `hc read` reads them as `how: history`. Claude sessions that tmux-tools does not start with a surface (a `claude` you start yourself, and agent-team teammates that Claude Code starts) keep the user setting. Neither `claude` surface declares readiness patterns — `--ax-screen-reader` has no native idle/busy chrome, so the flat surface cannot satisfy the promotion bar — so claude is not promoted and `rich` stays both its pre-surface rendering and its default. `codex` ships a single `default` surface, flat and already the default. It has the launch argument `--no-alt-screen` (placed before the access-profile arguments), so codex runs inline, not on the alternate screen, and its panes keep tmux scrollback (`#{alternate_on}` is `0`). It carries the measured `· Ready · Context` / `· Working ·` patterns stamped `codex-cli 0.167.0`. `cursor` and `agy` each ship a single unvalidated `default` surface and keep their previous behavior (out of scope for this smoke). `claude`'s two surfaces and `codex`'s `default` surface declare resume (`claude --resume <uuid>`, `codex resume <uuid>`, both a UUID); `cursor` and `agy` declare no resume syntax.

#### Validating vendor surfaces

Agent chrome changes between releases, so every pattern, bracketed-paste declaration and quit hazard above is validated against the live binary by a manually-run smoke, never by `cargo test`. Run it on a machine with the agents installed:

```sh
cargo run --features vendor-smoke --bin tt-vendor-smoke
```

For each `claude`, `codex` and README-documented `dsh` surface it launches the agent, checks that a flat surface (including `codex` `default`, launched with `--no-alt-screen`), and the classic-scroll `claude` `rich` surface, is not on the alternate screen, checks the declared `bracketed_paste` against the pane's live `bracket_paste_flag`, establishes idleness from the surface's native boot chrome plus a content-stable window (never from the declared `ready_regex`, so the pattern check is not circular), samples the whole generating turn and requires that `ready_regex` first matches only after the last busy frame, checks that a large (~5 KB) multi-line paste reaches the composer as one unit (a full-size collapse placeholder, or the end sentinel in the composer region) with no turn started — transcript growth and busy frames count as independent turn evidence — confirms a stable idle before sending the declared `interrupt_key` and checking whether the agent quits, measures `claude` `rich`'s declared resume syntax (a session started with `--session-id` and one finished turn is relaunched through `--resume <id>` and must show that turn's reply again), and compares the live `--version` with the `validated_version` stamp by exact version-token equality. It exits non-zero on any mismatch. A surface that declares no patterns — rich renderings, and `claude --ax-screen-reader`, which has no native ready/busy chrome — is noted rather than failed. When an agent's version changes, re-run the smoke and re-stamp `validated_version`.

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

### Floors

A surface may declare a **floor**: `min_cols` and `min_rows`, the smallest pane its readiness signal stays matchable in. Both are optional and merge by field like the surface's other fields, so overriding one axis of a built-in surface keeps the built-in's other axis (`min_cols = 50` on `codex`'s `default` surface gives 50×9, not 50×16). Written at the agent level (the pre-surface shape), `min_cols`/`min_rows` bind to the agent's pre-surface rendering, as agent-level `ready_regex`/`ready_lines` do — never to a `default_surface` that differs from it. An agent declared without surfaces gets them on its one synthesized surface.

```toml
[codex.surfaces.default]
min_cols = 50          # min_rows stays at the shipped 9; args stay ["--no-alt-screen"]

[myagent]
binary = "/usr/local/bin/myagent"
min_cols = 40          # agent level: binds to the pre-surface rendering; rows default to 16
```

A pane's floor resolves from its `@tt-agent` and `@tt-surface` the way readiness does: a recorded surface is that surface, and no record is the agent's pre-surface rendering. An axis the resolved surface does not declare takes the global default of **58×16**; an agent the registry does not know, or a recorded surface it does not have, takes 58×16 for both. That default is an assumption, not a measurement: it is the componentwise maximum of the shipped floors, the most conservative value to assert for a TUI nobody has measured. A pane with no `@tt-agent` (a plain `launch`, or a pane made by raw `tmux split-window`) contributes no floor, but it is still measured. A window's effective floor is the componentwise maximum over its panes' floors.

`launch` and `spawn-agent` report `below_floor` for the window the pane landed in, and `status` reports it with `min_pane_cols`/`min_pane_rows` for the window of the pane it resolves (`concise` and `json` only; `raw` is unchanged). One exception: when the new pane no longer exists at a placement step after the split (registration, retile, the `@tt-tiled` marker, or measurement — for example a `--bare` command that already exited), `launch` and `spawn-agent` still exit 0, leave out `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor` (absent keys in `json`), report `name` as the name chosen for the pane even if it was never written, and print `warning: pane <id> no longer exists; window geometry not measured` on stderr. `below_floor` is true when the window's smallest pane width or height is below the matching axis of its effective floor; a window without agent panes is never below floor. The floor is advisory: the tool reports a breach and does not move or refuse a pane, and raw tmux splits bypass it. `launch` and `status` use the registry only for floors, so when `agents.toml` cannot be read or parsed they print `warning: agent registry <path>: <error>; using built-in floors` and carry on with the built-ins; `spawn-agent` and `wait-idle` still fail on such a file.

Shipped floors, each one step above the readiness cliff measured on 2026-08-01, each against the readiness rules of one surface:

| Surface | Floor | Measured against |
| --- | --- | --- |
| `codex` `default` | 58×9 | its shipped `ready_regex`/`ready_lines` (the status line wraps at 56 columns) |
| `cursor` `default` | 36×9 | its shipped `ready_regex`/`ready_lines` |
| `claude` `rich` | 26×16 | the opt-in status-line pattern a user declares on it (see "Detecting readiness from a custom Claude status line"); at 14 rows `ready_lines = 0` no longer finds the indicator |

`claude` `flat`, `agy` `default` and every other surface ship no floor and take 58×16. A floor is derived from the `ready_regex`/`ready_lines` it was measured with: changing either on that surface makes its floor invalid, and the tool does not enforce this coupling — re-measure and set `min_cols`/`min_rows` yourself. The 2026-08-01 measurement recorded no agent versions, and a surface's `validated_version` does not cover its floor.

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

## Hosts

tmux-tools drives the panes of one host per invocation through a pane-level Host port: tmux, Orca through the `orca` CLI, Rex through the `rex` CLI, or Herdr through the herdr-core `hc` CLI. The tmux backend keeps every behaviour described in this README.

### Orca

Inside an Orca terminal (`ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID` set, no `$TMUX`) every verb drives Orca panes. A Target is a pane name or a `term_…` handle; a verb without `--target` acts on the calling pane. A spawn without `--split`, or with `--split window`, opens a tab in the calling worktree (or the target's worktree); `--split h|v` runs `orca terminal split --direction horizontal|vertical` on the calling or target pane. Orca keeps the pane open in its login shell after the command exits, which is the keep-alive wrap; `--bare` runs `exec <cmd>`. Host state (`host_state`) comes from `orca worktree ps --json`: `working`, `blocked`, `done` or `null` (Orca never reports `idle`); without `--until` or `--idle-seconds`, `wait-idle` and the settle step of `prompt` end on `done` or `blocked` and keep waiting while it is `working`. `prompt` reports `submit` from Orca's send receipt (`confirmed`, `accepted` or `assumed`). JSON keys Orca has no source for (`window_index`, `pane`, the window geometry fields, `relocated`, `last_activity`) are `null`.

Limits on Orca:

- **Registration is lost on an Orca restart.** Orca cannot store data on a pane, so each pane's registration is a Pane record at `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/orca/<runtimeId>/<name>.json`. Orca makes a new `runtimeId` and new handles on every runtime restart while its panes stay open: after a restart every pane spawned before it stays open in Orca but is unregistered, and its name is free again. Record directories of earlier runtimes are deleted.
- **Every `prompt` result is `complete: false`.** Orca reports no pane width and no full-screen state, so Completeness cannot be established. `capture --all` prints no full-screen notice. The History mark is Orca's read cursor; history is paged to the settle point, and a full-screen program, which keeps no line history, yields its rendered screen.
- **Cursor and keypad keys always send their normal-mode bytes.** `send` classifies each argument by tmux's own key grammar and writes the bytes tmux's xterm key output produces (`M-x` → `ESC x`, `F1` → `ESC O P`, `Up` → `ESC [ A`, `^c` → `0x03`); Orca does not expose a pane's application cursor or keypad mode, so an application that enabled it receives the normal-mode bytes. `interrupt` sends the surface's `interrupt_key` the same way, never Orca's `--interrupt`.
- **One-submission delivery is a proxy, with a remaining risk.** `prompt` refuses before sending anything when the runtime lacks the capability `terminal.prompt-delivery.v1`. Otherwise delivery is guaranteed only when `terminal show` reports the pane's registered agent, `worktree ps` reports that agent live, and the capability is present; a multi-line prompt to a bracketed-paste surface is refused when delivery is not guaranteed. Orca decides on its server whether to bracket the text, and nothing it reports before sending reproduces that check: it can still write the raw text, and a shell in the pane then runs each line.

### Herdr

Herdr terminals are driven through the herdr-core `hc` CLI: the backend runs only `hc <command> … --json` and reads its JSON; it never calls `herdr-app` and never uses `hc connect`.

**Selection.** Herdr is selected by `TMUX_TOOLS_HOST=herdr`, or, with `TMUX_TOOLS_HOST` unset, by `HC_TERMINAL_ID` when neither `$TMUX` nor `ORCA_TERMINAL_HANDLE` is set; under tmux the host stays tmux unless `TMUX_TOOLS_HOST=herdr`. When `hc` is not on `PATH` or `hc server status --json` fails, the verb exits 1 and names `herdr` and what selected it. The calling pane is always `HC_TERMINAL_ID` (never `$TMUX` or `TMUX_PANE`, also when a stale `TMUX_PANE` is set); without it, `self` and every verb that acts on the calling pane without `--target` exit 1 and name `HC_TERMINAL_ID`. A Target is a tmux-tools pane name or an hc address `t<n>`; a registered name wins, and a value is used as an hc address only when no registered name matches it.

**Placement.** A new terminal starts with `hc run --bg --outlive-parent --no-notify --name tt-<16 lowercase hex of SHA-256(name)> --cwd <cwd>` and its labels, so its name and registration exist when it is created. It shows in the Herdr window only through the labels `herdr.space` and `herdr.tab`, taken, first match, from the `--target` terminal, else the calling terminal (`HC_TERMINAL_ID`), else `TMUX_TOOLS_HERDR_SPACE` and `TMUX_TOOLS_HERDR_TAB` (a Herdr space id and tab id). When no source gives a space, the terminal gets neither label and stays hidden.

- `--split h` and `--split v` set both labels, so the terminal joins that tab; Herdr chooses its position, and the split direction has no effect.
- No `--split`, and `--split window`, set `herdr.space` and a `herdr.tab` with a new tab id that tmux-tools makes up (`tt-tab-<32 lowercase hex>`) and that no other terminal carries, so Herdr opens a new tab; that tab id is `window_id` in the `launch` and `spawn-agent` JSON and `window` in the `status` JSON. A later `--split h` or `--split v` whose label source is this terminal joins that tab.
- `--split tile`, `--size`, `--session`, `--window` and `rebalance` exit 3 with no side effect; the automatic tile after a spawn is skipped.

**Labels.** The Pane registration is stored as hc labels on the terminal, all string values; an absent label is unset: `tt.name`, `tt.agent`, `tt.access`, `tt.launched_at` (RFC 3339), `tt.cwd`, `tt.surface`, `tt.surface_unvalidated` (`"true"` or `"false"`), `tt.parent`, `tt.team`, `tt.shim_id`, `tt.title`, `tt.claude_agent_name`, `tt.claude_team_name`. The label `where` is the tmux-tools pane name, and `herdr.space` and `herdr.tab` place the terminal. Herdr uses no Pane record. A name is unique among live terminals through its hc name: a spawn whose hc name is taken exits 1 and creates no terminal. A terminal in state `finished` that carries `tt.name` is dead: every verb that resolves, lists or spawns that name first removes it (`hc rm`), and `list` shows it once with `status: dead`. Ownership reads `tt.name` and `tt.cwd`.

**Verbs.**

- `list` shows the terminals of `hc ls --all --json` that carry `tt.name`: `pane` is the hc address, `session` the `herdr.space` label, and a live terminal has `status: running`; rows have no `window` key. In `status`, `spawn-agent` and `launch`, `window` is the `herdr.tab` label or `null`. Every other JSON key with no Herdr source is `null`.
- `kill` runs `hc kill`, then `hc rm`. `launch` keeps the shell after the command; `--bare` lets the terminal finish with the command.
- Host state (`host_state`) comes from the `status` of `hc get --json`: `idle`, `working`, `blocked` and `done` map to themselves, `error` maps to `blocked`, anything else and a finished terminal map to `null`. `status --format json` adds `host_word`, the exact hc status word (on every host; always `null` on tmux and Orca).
- `prompt` submits with `hc send` (never `--force`). Every `hc send` that tmux-tools runs (`prompt`, and `execute`) runs without `HC_TERMINAL_ID` in its environment, so hc adds no `<hc from="…">` tag and delivers the text exactly as given: a text that starts with `/<command>` runs as that command in an agent such as Claude or Codex. Every other `hc` call keeps `HC_TERMINAL_ID`. When hc refuses because the program is blocked, it exits 1 with `prompt target blocked: <reason>`. `submit` is `confirmed` when the terminal's `turn` goes up, or its status word changes to `working` after the send; otherwise `accepted`.
- `notify` exits 0 and prints `notify: not delivered on herdr` on stderr.
- `claude-teams` (and `spawn-agent claude`) in a Herdr terminal sets up a Team (see "Claude teams"). Every teammate Claude starts is a Teammate terminal in the Leader's tab, placed by Herdr; the split direction has no effect. A teammate terminal starts with `--rm`, so Herdr removes it when its command exits 0; on any other exit it stays, with a shell. The Leader is the terminal `HC_TERMINAL_ID`; without it, `claude-teams` fails open with `claude-teams: no team: HC_TERMINAL_ID is unset; starting claude without teammate panes`.

Limits on Herdr:

- **`send` types only a fixed character set.** `hc keys` sends one key per character. The typeable set is space, `a`–`z`, `A`–`Z`, `0`–`9`, and `` ' , - . / ; = [ \ ] ` ``. `hc keys` cannot type any other character, including `! " # $ % & ( ) * + : < > ? @ ^ _ { | } ~`. Every argument is checked before anything is sent: a character outside the set exits 1 with `cannot type on herdr: U+<XXXX>`. When that character has a tmux key name (U+001B `Escape`, U+000D `Enter`, U+0009 `Tab`, U+007F `BSpace`, otherwise U+0001–U+001A `C-a`–`C-z`), the message also names the key and how to send it as a key, for example `cannot type on herdr: U+001B (Escape). Send it as a key: tmux-tools escape --target <pane>, or tmux-tools send --target <pane> Escape. See: tmux-tools send --help` (`<pane>` is literal text; `Enter` points to `send-enter`, every other key to `tmux-tools send --target <pane> <key>`). A key name with no `hc keys` name exits 1 with `cannot type on herdr: <argument>`; nothing is sent then. Only when `hc keys` still rejects a key during the send can part of the input be sent. `send-enter` sends `enter`, `escape` sends `esc`, and `interrupt` sends the surface's `interrupt_key` this way (never `hc interrupt`).
- **One-submission delivery is guaranteed only for a pane with Host state.** `hc send` wraps the text in bracketed paste when the program has mode 2004 on, and writes it raw otherwise; hc does not report the mode. Delivery is guaranteed for a pane whose Host state is not `null` (the program reports OSC 7501). A multi-line prompt to any other pane is refused before anything is sent; other prompts to it are sent with delivery `unavailable`.
- **A full-screen pane gives its whole read.** The History mark is one `hc read --lines 1000000` before submission; the `prompt` result is the read after settle from the first line that differs from the mark read. When the read after settle reports `how: stitched` or `how: screen` (a full-screen program), the result is that whole read and does not start at the mark. Completeness is true only when both reads report `how: history` and the first changed line is at least the number of lines of the mark read minus `rows`, so output that only appends after the mark read is complete. `capture --all` prints the no-scrollback notice when its read reports `how: screen`.

### Rex

Rex blocks are driven through the `rex` CLI. Every `rex` call passes `--autostart=false`, so tmux-tools never starts a Rex server, and passes `-S "$REX_SERVER"` when `REX_SERVER` is set.

**Selection.** Rex is selected by `TMUX_TOOLS_HOST=rex`, or, with `TMUX_TOOLS_HOST` unset, by `REX_BLOCK` when neither `$TMUX` nor `ORCA_TERMINAL_HANDLE` is set; under tmux the host stays tmux unless `TMUX_TOOLS_HOST=rex`. When `rex` is not on `PATH` or `rex server status --json` fails, the verb exits 1 and names `rex` and what selected it; it never falls back to another host. The calling pane is always the block named by `REX_BLOCK` (never `$TMUX` or `TMUX_PANE`, also when a stale `TMUX_PANE` is set); without it, `self` and every verb that acts on the calling pane without `--target` exit 1 and name `REX_BLOCK`. A Target is a tmux-tools pane name or a block id `block:<uuid>`; a registered name wins, and a value is used as a block id only when no registered name matches it.

**Placement.** The session of a new block is, first match, the `--target` block's session, `REX_SESSION`, or `TMUX_TOOLS_REX_SESSION` (a Rex session id). When none gives a session, the spawn exits 1, names `REX_SESSION` and `TMUX_TOOLS_REX_SESSION`, and creates nothing.

- No `--split`, and `--split window`, run `rex run -s <session>`, which opens a new Rex window (a tab in the Rex app) in that session.
- `--split h` runs `rex split --split=right` and `--split v` runs `rex split --split=below`, with `-b <anchor>`: the `--target` block, else the calling block. When neither exists, the spawn exits 1, names `REX_BLOCK`, and creates nothing.
- Every create passes `--focus=false`, so focus does not change, `--label` with the first 128 characters of the pane name (a Rex label holds at most 128 characters; pane names keep no length limit), `--cwd`, and `--json`, and never `--keep-open`.
- `--split tile`, `--size`, `--session`, `--window` and `rebalance` exit 3 with no side effect; the automatic tile after a spawn is skipped.

**Pane record.** Rex cannot store custom data on a block, so each pane's registration is a Pane record at `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/rex/<instance_id>/<name>.json`, where `instance_id` comes from `rex server status --json` and the record's `handle` is the block id. Name validation, reservation, the 120-second spawn timeout and Ownership work as on Orca. The block label is for display only and never resolves a Target. A record whose block is missing from a complete listing is dead; when `rex ls --json` or a `rex block ls -s <session> --json` call fails, the listing is incomplete and `list` prints `list incomplete: rex` on stderr. When a create gives no block id and did not fail definitely (a timeout, an unexpected exit, unparseable output), the spawn adopts the one block of the target session whose label is the name's first 128 characters, when no live record holds that block and no other record or reservation shares those 128 characters; otherwise it exits 1 with `spawn outcome unknown: <name> (session <session id>)` and keeps the reservation. A create that exits 3 or 4 has failed and frees the name. Creation is never repeated.

**Servers and the `server-address` sweep check.** Each server directory `rex/<instance_id>/` holds the file `server-address`: the `REX_SERVER` value, or `unset` when `REX_SERVER` is not set. The directory is published atomically from a temporary `rex/.init-<random>/` directory; an `.init-` directory older than 120 seconds is deleted. tmux-tools deletes another server directory only after one `rex server status --json` against its stored address (with `-S <address>`, or for `unset` without `-S` and without `REX_SERVER`, so it reaches the default server) with a 2-second timeout fails, times out, or reports a different `instance_id`, or when the directory has no readable `server-address`. A server you reach both with `REX_SERVER` unset and with its explicit address uses one directory.

**Verbs.**

- `capture` runs `rex capture -b <block>` in text format; `--lines 0` gives the last `rows` lines (`rex block call -b <block> size`), and the no-scrollback notice fires when `rex block call -b <block> modes` reports the alternate screen on.
- `send` writes xterm bytes through `rex send -b <block>`: key names become the bytes tmux's key output produces (cursor keys use application-mode bytes when the block has mode 1 on), and other arguments are sent as literal text. `send-enter` sends `\r`, `escape` sends `\x1b`, and `interrupt` sends the surface's `interrupt_key` the same way.
- Host state (`host_state`) comes from the `rex block call -b <block> program_status` record with the newest `updated_at`: `idle`, `working`, `blocked` and `done` map to themselves, `error` maps to `blocked`, and no record maps to `null`. `status --format json` reports `host_word` as that record's `state`, or `null`.
- `prompt` pastes the text with `rex api call block.event` (event type `paste`) and then sends Enter with `rex send-key -b <block> enter`. `submit` is `confirmed` when the Host state before the send was not `working` and a `program_status` record newer than the send reports `working`, `done` or `blocked` (a raw `error` record confirms); otherwise `accepted`. Only a record newer than the send ends the settle step.
- `list` rows: `pane` is the block id, `session` the Rex session id, `status` is `running`, `dead` or `unknown`, and rows have no `window` key. In `status`, `spawn-agent` and `launch`, `window` is the Rex window id. Every other JSON key with no Rex source is `null`.
- `kill` runs `rex block close <block>` (a block that is already gone counts as closed), then deletes the record. `launch` keeps the shell after the command; `--bare` lets the block close when the command exits.
- `self --format json` reports `host: "rex"`; `driven` is true when a live record holds `REX_BLOCK`.
- `notify` exits 0 and prints `notify: not delivered on rex` on stderr.
- `claude-teams` (and `spawn-agent claude`) in a Rex block sets up a Team (see "Claude teams"). Every teammate Claude starts is a Teammate block in the Leader's window: the first teammate goes right of the Leader, and each later teammate goes below the live Teammate block with the highest number; Rex has no `tile`. The block label follows the pane name (its first 128 characters), also after a teammate respawn. The Leader is the block `REX_BLOCK`; without it, `claude-teams` fails open with `claude-teams: no team: REX_BLOCK is unset; starting claude without teammate panes`. A teammate block has its own `REX_BLOCK`: the Shim drops `-e` settings of `REX_BLOCK`, `REX_SESSION` and `REX_SERVER`.

Limits on Rex:

- **Delivery is guaranteed only with bracketed paste on.** One-submission delivery is guaranteed when `modes` reports mode 2004 on before the send. A multi-line prompt to a block without mode 2004 on is refused before anything is sent; other prompts to such a block are sent with delivery `unavailable`.
- **`prompt` needs API revision 12.** When `api_revision` from `rex server status --json` is below 12, `prompt` exits 1 before anything is sent with `rex api revision <n> is older than 12`. Other verbs keep working.
- **Rex drops the oldest scrollback silently.** Once a block's scrollback is full, Rex drops its oldest line for each new line. The History mark is a full capture before submission, and the `prompt` result starts at the first line that differs from it. Completeness is true only when the mark capture and the capture after settle were both taken with the alternate screen off and the same width, and the first changed line is at least the mark capture's line count minus `rows`. Otherwise (a full scrollback, a width change, the alternate screen) the result is the last `rows` lines of the capture after settle, with `complete: false`, and does not start at the History mark.

## Claude teams

`tmux-tools claude-teams [-- <claude args>]` runs in the calling pane and starts the real `claude` (the `command -v claude` of your `PATH`) as its child, in a Team, with `--teammate-mode tmux` added unless you pass `--teammate-mode`. Your settings must enable agent teams; the verb sets neither `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` nor `--agent-teams`. Every teammate Claude starts is a tmux-tools pane named `<leader name>.tm<k>` (`team-<team id>.tm<k>` when the calling pane has no tmux-tools name) with `team`, `parent` and `shim_id`, on tmux, on Orca, on Herdr and on Rex. A clean Claude exit (status 0) closes every teammate pane and deletes the team directory; any other exit keeps them, and `list` then reports them with `orphan: true` so you can read their output and `kill --target <name>` them. The verb exits with Claude's status.

**The Leader's child processes see a fake tmux.** In Claude and every process it starts, `TMUX` is `<team dir>/tmux-fake,<pid>,0` (not a socket), `TMUX_PANE` is `%1`, and `<team dir>/bin` — whose only file is the Shim, a `tmux` link to `tmux-tools` — is first on `PATH`; `TT_TEAM` and `TT_HOST` name the Team and its host. So a `tmux` command run by the Leader or its tools reaches the Shim, not tmux. The Shim serves only the tmux subset Claude uses for teammates (`display-message`, `list-panes`, `split-window`, `respawn-pane -k`, `kill-pane`, `send-keys`, `capture-pane -p`, `has-session`, `select-pane -T`; `set-option`, `select-layout` and `resize-pane` are ignored), only for the panes of its own Team, and refuses everything else with exit 1 and `tmux: <reason>`. It has no compatibility path for `#{@tt-*}` formats: they expand to the empty string. Inside a team, `tmux-tools` verbs act on the Leader as the calling pane and ignore `TMUX_PANE`.

**Release order.** Do not use `claude-teams` before the harness rollout steps 2 (hooks) and 3 (text and scripts) of PRD-261108-0235-01 are released: inside a Leader, `$TMUX` and `tmux` reach the Shim, so the harness hooks must call `tmux-tools self` instead of `tmux display-message`.

- **Fail-open start.** When no Team can be set up — no supported host (for example a plain terminal, where `TMUX_PANE` is unset), an invalid `TMUX_TOOLS_HOST`, an unreachable host, or a team directory that cannot be created — the verb prints `claude-teams: no team: <reason>; starting claude without teammate panes` and starts Claude with `--teammate-mode in-process` and your environment unchanged. Inside a team it refuses with `claude-teams: already inside team <id>`, and without a `claude` with `claude-teams: claude not found` (exit 1 for both).
- **Claude-version warning.** A Claude version without a trace fixture in this build (`tests/fixtures/claude-teams/<version>.jsonl`) prints `claude-teams: claude <version> not contract-tested` once on stderr; the verb continues.
- **`shim-debug/` stays.** With `TT_SHIM_DEBUG=1`, the Shim logs every call with its exit and stdout to `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/shim-debug/<team id>/shim.log`, next to a copy of `team.json`. tmux-tools never deletes these directories; delete them yourself. Without it, only refusals are logged, in the team directory.
- **`spawn-agent claude` starts the Launcher.** On tmux, on Orca, on Herdr and on Rex, `spawn-agent` of an agent whose binary basename is `claude` runs `<tmux-tools path> claude-teams --claude <binary> -- <claude args>` in the new pane (on tmux inside the same keep-open wrap as before), so the Leader's teammates are tmux-tools panes `<name>.tm<k>`; `orca claude-teams` is never used. The Pane registration, `agent` and Surface are as before. Orca does not report a Leader under the Launcher reliably: its Host state can be `null`, and while a teammate lives it can be `working`, so `wait-idle` and `prompt` on the Leader can end only by `--until` or timeout.
- **No space for a teammate on tmux.** On tmux, when the window has no space for a new pane, a teammate start fails with `tmux: no space for a new pane`; the Shim requests no size.

## Pane history and `prompt` results

`launch` and `spawn-agent` raise the `history-limit` of **every session they create** to 50000 before the launched command's window can emit output. `history-limit` is a tmux **session** option (it cannot be set per pane); new windows inherit it. tmux-tools never changes the option on a session it did not create, and never sets it globally. `tmux show-options -t <session> history-limit` reports it. This is what lets `prompt` return a completed turn whose response scrolled past the visible screen: tmux defaults the limit to 2000, so a deep turn can silently drop its own beginning.

For panes created in sessions tmux-tools did not create, set the same default in `~/.tmux.conf` (or on the session before its windows are created):

```tmux
set-option -g history-limit 50000
```

The cost is memory: tmux stores roughly **70 bytes per history line**, so 50000 lines is about 3.5 MB per pane. Lower the value if many high-history panes would be expensive; `prompt` reports an incomplete result rather than guessing when history has evicted lines. tmux-tools never writes `~/.tmux.conf`.

`prompt` takes a **history mark** at the submission point (the cursor row) before it submits, and never advances it: the result begins there and always includes the agent's echo of the prompt. Output from before submission is excluded wherever that removes neither the echo nor the turn's output; when a redraw places either above the mark (a footer or composer redrawn in place, or the alternate screen), it is kept. Echo exclusion is deliberately not attempted. `prompt` reports two independent dimensions on every run:

- **Turn observation** — whether the resolved surface's `busy_regex` was seen between submission and settle. It says the agent took the turn; it makes no claim about where content begins.
- **Completeness** — whether everything after the mark could be supplied. A result is incomplete when the pane is on the alternate screen (no tmux scrollback), history has evicted lines, or the pane's width changed between the mark and settle (a width change rewraps history, so the mark no longer locates the submission point). Eviction is read from the pane's `history_collected` counter when the running tmux provides it (any increase between mark and settle); otherwise a pane counts as evicted once `history_size >= history_limit - history_limit / 10`, the floor tmux frees to when it trims history.

Raw and concise carry both dimensions on stderr; JSON carries them as the `turn_observed` and `complete` fields. They are always stated, even when the result is empty.

`prompt` settles on the resolved surface's classification or, when the surface supplies no patterns, on idle detection; with patterns present it never treats an unknown capture (neither or both matching) as idle. `wait-idle` is unchanged and keeps its idle-detection fallback.

## Library: the Host port

`tmux-tools-core` is the library behind the CLI. Its public modules are `host`, `registry`, `team`, `agents`, `format`, `idle`, `layout`, `names` and `target`; the modules other than `host` reach a host only through the Host port. The former tmux paths (`tmux`, `stream`, `TmuxInvocation`, `with_invocation`, `set_global_invocation`) are gone.

- `host::HostPort` — the pane-level port: create, list, capture, send text and keys, close, the Pane registration, Host state, notify, capabilities, and the layout operations gated by `tile`. `host::TmuxHost` and `host::OrcaHost` implement it.
- `host::HostSelection` — `{ host: Host, source: SelectionSource }`, made by `HostSelection::from_env()` (or `from_lookup` over any variable source) with the rules of "Host selection". `host::selection()` returns the invocation's selection and `host::current()` its backend.
- `host::HostCapabilities` — `native_state`, `submit_confirmation`, `notify`, `tile`, `rebalance`, `size`. An optional feature a host lacks is the error `host::Unsupported`.
- `host::Host`, `host::HostState`, `host::Submission` — the host, its reported agent state, and what `prompt` knows of its submission.
- `registry::PaneRecord` and `registry::PaneLiveness` — the Pane records of a host without pane data (Orca) and their `live` / `dead` / `orphan-reservation` states.
- `team::TeamRecord` (with `TeamLeader`, `ClaudeBinary`, `TmuxEndpoint`) and `team::TeamRole` — the Team record of `claude-teams` and a pane's role in a Team. Inside a team (`TT_TEAM` set) `TT_HOST` selects the host first (`SelectionSource::Team`).

```rust
use tmux_tools_core::host::{self, Host, HostSelection};

let selection = HostSelection::from_env()?;
let backend = host::current();
assert_eq!(backend.host(), selection.host);
if selection.host == Host::Orca {
    assert!(!backend.capabilities().tile);
}
```

## Testing

The integration suite drives a real tmux server, so `tmux` must be on `PATH`. Several tests need the
`tt-fake-tui` fixture, a second `[[bin]]` that a normal build deliberately does not produce. Run the
full suite with that fixture enabled:

```sh
cargo test --workspace --features test-fixtures
```

`tests/claude_teams.rs` replays each Claude trace fixture through the Shim inside a Team on the
isolated tmux server, with a fake `claude`. A new Claude version gets its fixture from a
`TT_SHIM_DEBUG=1` `shim.log` (from the opt-in live run below, or any other run). The live run
starts Claude with `disableAllHooks: true` and `--disallowedTools Bash`, and fails when the version
has no fixture or when `shim.log` has a refused call that is not one of the fixture's `exit` 1 lines
(for Claude 2.1.319 the startup probes `show -Av mouse`, `show -gv focus-events`, `show-environment -g`):

```sh
TMUX_TOOLS_LIVE_CLAUDE=1 cargo test --features test-fixtures --test claude_teams live_claude -- --nocapture
cargo run --features test-fixtures --bin tt-fixture -- <shim-debug dir>/shim.log > tests/fixtures/claude-teams/<version>.jsonl
```

A bare `cargo test` still passes; it compiles the fixture-gated tests out and simply runs that many
fewer tests. The default run never reaches Orca: its child processes get no `TMUX_TOOLS_HOST`,
`ORCA_TERMINAL_HANDLE` or `ORCA_WORKTREE_ID`, and no `orca` on `PATH`.

The same contract suite runs against a live Orca runtime only when opted in, from inside an Orca
terminal with the runtime reachable:

```sh
TMUX_TOOLS_LIVE_ORCA=1 cargo test --features test-fixtures --test integration orca_contract_suite -- --nocapture
```

It closes every pane it creates, also on failure, and ends by asserting that neither `tmux-tools list`
nor `orca terminal list` shows one of them.

The default run never reaches herdr-core either: the Herdr tests drive a fake `hc`, and child
processes get no `HC_*` variables and no `hc` on `PATH`. With `TT_TEST_HERDR=1` and `hc` on `PATH`,
the Herdr contract suite runs against a private core `HC_SESSION=tt-tests-<pid>-<tag>`; each test stops
its core and deletes its own `~/.local/state/herdr-core/tt-tests-<pid>-<tag>`, also on failure. Its
Claude tests need `claude` on `PATH`; two of them (the full-screen test and the `rich` test) need
`env.CLAUDE_CODE_NO_FLICKER` set to `"1"` in `~/.claude/settings.json`:

```sh
TT_TEST_HERDR=1 cargo test --features test-fixtures --test integration herdr_ -- --nocapture
```

The live Herdr run uses the core of the calling terminal, so start it from a plain Herdr terminal (no
`$TMUX`, `HC_TERMINAL_ID` set) that carries `herdr.space` and `herdr.tab`:

```sh
TMUX_TOOLS_LIVE_HERDR=1 cargo test --features test-fixtures --test integration herdr_live_contract_suite -- --nocapture
```

It starts a `--split v` terminal in the caller's tab, a no-split terminal in a new tab, and a Claude
pane through `spawn-agent claude`; it sends a key name and literal text, and a multi-line prompt to
Claude with guaranteed delivery. It keeps these terminals for 30 seconds so you can check the Herdr
window, then kills them, also on failure, and ends by asserting that neither `tmux-tools list` nor
`hc ls --all --json` shows one of them.

The default run never reaches a Rex server either: the Rex tests drive a fake `rex`, and child
processes get no `REX_*` variables and no `rex` on `PATH`. With `TT_TEST_REX=1` and `rex` on `PATH`,
the Rex contract suite runs against a private server (`rex server run --listen unix://<dir>/s.sock`)
with its own registry; at the end, also on failure, it stops that server and deletes `<dir>`. It never
reaches the default server:

```sh
TT_TEST_REX=1 cargo test --features test-fixtures --test integration rex_private_server_contract_suite -- --nocapture
```

The live Rex run uses the default Rex server and your registry, so start it from a plain Rex terminal
(no `$TMUX`, `REX_BLOCK` set) on the default server, with `claude` on `PATH`:

```sh
TMUX_TOOLS_LIVE_REX=1 cargo test --features test-fixtures --test integration rex_live_contract_suite -- --nocapture
```

It starts a `--split v` block beside the calling block and a Claude pane through `spawn-agent
claude` in the calling session; it sends literal text and a key name, waits for Claude's Host state,
and sends a multi-line prompt to Claude with guaranteed delivery. It then kills these blocks, also on
failure, and ends by asserting that neither `tmux-tools list` nor the Rex block listing shows one of
them.

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
