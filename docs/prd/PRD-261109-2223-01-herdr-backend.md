---
id: PRD-261109-2223-01
kind: prd
status: active
scale: epic
stakes: internal
gate: passed 2026-11-09
contracts: approved 2026-11-09
contexts: [tmux-tools]
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Pane state, Target, Ownership, Parent, hc terminal, hc name, History mark, Completeness, Keep-alive wrap, Surface, Readiness, Idle detection, Turn observation]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: problem
      heading: Problem Statement
    - id: outcome
      heading: Outcome and scope
    - id: p1-definitions
      heading: P1 — Definitions
    - id: p2-selection
      heading: P2 — Host selection
    - id: p3-placement
      heading: P3 — Placement and visibility
    - id: p4-registration
      heading: P4 — Pane registration, names, and lifetime
    - id: p5-io
      heading: P5 — Capture, send, and keys
    - id: p6-state
      heading: P6 — Host state and prompt
    - id: p7-other-verbs
      heading: P7 — notify, self, list, kill, launch
    - id: p8-tests
      heading: P8 — Contract tests
    - id: p9-rollout
      heading: P9 — Rollout and release
    - id: contracts
      heading: Contracts
    - id: verification
      heading: Verification and release
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p2-host-port
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p3-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p5-verb-classes
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p6-host-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p7-notify
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p8-self
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p9-orca
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p2-launcher
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p7-parent
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p1-transport
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p3-state
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p4-capture
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p7-prompt
---

# Herdr host backend

## Problem Statement

Under Herdr, panes that `tmux-tools` creates are invisible in the Herdr window and get none of Herdr's agent state. Callers that want Herdr's features must use `hc` directly, which skips this tool's naming, ownership, readiness, and teardown rules.

## Outcome and scope

`tmux-tools` runs every required verb against Herdr through a `herdr` Host backend that implements the Host port of PRD-261107-0825-01 P2 over the `hc` CLI (herdr-core). Driven terminals show in the caller's Herdr space and tab, and Host state comes from the program's own OSC 7501 report. This PRD owns the backend, its selection, its registration storage, its tests, and its release. Herdr is the second host backend after Orca; Rex follows.

## Required behavior

### P1 — Definitions

- **Host** gains the value `herdr`: herdr-core terminals driven through the `hc` CLI.
- **hc terminal:** one herdr-core terminal, addressed by `t<n>`. On Herdr, a pane is an hc terminal.
- **hc name:** the `--name` of an hc terminal that this tool creates: `tt-` followed by the first 16 lowercase hex characters of SHA-256 of the UTF-8 tmux-tools pane name.
- On Herdr, the calling pane is the hc terminal named by `HC_TERMINAL_ID`.

`CONTEXT.md` is updated: Host, Host backend, Host selection, Host capabilities, and Host state name the Herdr backend, its detection rule, and its values; Pane registration names the `tt.*` labels (P4.1); Ownership names its Herdr evidence (P4.5); Keep-alive wrap names its Herdr rule (P7.5); Target names the hc address form (P7.6); `hc terminal` and `hc name` are minted.

### P2 — Host selection

1. `TMUX_TOOLS_HOST` accepts `tmux`, `orca`, and `herdr`. Any other value fails with exit 1 and names all three.
2. Detection order, first match: `$TMUX` set → `tmux`; `ORCA_TERMINAL_HANDLE` set → `orca`; `HC_TERMINAL_ID` set → `herdr`. Under tmux the host stays `tmux` unless `TMUX_TOOLS_HOST=herdr`.
3. When Herdr is selected and `hc` is not on `PATH` or `hc server status --json` fails, the invocation fails with exit 1 and names `herdr` and the selection source (PRD-261107-0825-01 P3.4).
4. The backend calls only `hc <command> … --json` CLI commands and reads their JSON. It never calls `herdr-app` and never uses `hc connect`.
5. `server_identity` is `core.id` from `hc server status --json`.
6. Every lookup of the calling pane goes through the Host port: `self`, `status` without `--target`, the self-pane refusal, and `tt.parent` recording use `HC_TERMINAL_ID` on Herdr and never read `$TMUX` or `TMUX_PANE` there, also when a stale `TMUX_PANE` is set.

### P3 — Placement and visibility

1. A new terminal shows in the Herdr window only through the labels `herdr.space` and `herdr.tab`. Their source, first match: with `--target`, the target terminal's labels; with `HC_TERMINAL_ID` set, the calling terminal's labels; otherwise `TMUX_TOOLS_HERDR_SPACE` and `TMUX_TOOLS_HERDR_TAB`. When no source gives a space, the terminal gets neither label and stays hidden.
2. `--split h` and `--split v` set both `herdr.space` and `herdr.tab`, so the terminal joins that tab; Herdr chooses its position and the split direction has no effect. No `--split`, and `--split window`, set only `herdr.space`, so Herdr opens a new tab.
3. `--split tile`, `--size`, `--session`, `--window`, and `rebalance` exit 3 with the PRD-261107-0825-01 P5.3 messages and no side effect. The automatic tile after a spawn is skipped (P5.4).
4. The terminal starts with `hc run --bg --outlive-parent --no-notify --name <hc name> --cwd <cwd>` plus its labels (P3.1, P4.1), so its name and registration exist when it is created. Herdr creates through the port's `create_registered` path, not the create-then-name path of `layout::place`, and measures no geometry. `--rm` is never used.

### P4 — Pane registration, names, and lifetime

1. The Pane registration is stored as hc labels on the terminal: each `RegistrationKey` is the label `tt.<key in snake_case>` with a string value (`tt.surface_unvalidated` is `"true"` or `"false"`); an absent label is `None`. The label `where` is set to the tmux-tools pane name. Herdr uses no Pane record (`uses_pane_records()` is false).
2. Name validation stays PRD-261107-0825-01 P4.1. Uniqueness among live terminals comes from the hc name: when `hc run` reports that the name is taken, the spawn fails with exit 1 and no terminal is created. A name is free again when its terminal is removed (P4.4).
3. The `tt.parent` label follows PRD-261108-0447-01 P7.
4. A terminal in state `finished` that carries `tt.name` is dead. Every invocation that resolves, lists, or spawns a name first removes, by hc address, each dead terminal that carries that name (`hc rm`); `list` shows each removed name once with `status: dead`. A verb that resolves only a dead name fails with exit 1 as for an unknown name, and that value is never tried as an hc address after that (P7.6). A dead name never resolves to another terminal. A spawn of that name then proceeds. Terminals are always resolved and removed by hc address, never by hc name.
5. Ownership reads the `tt.*` labels: `tt.name` is present, and `tt.cwd` matches the current working directory where both are known.

### P5 — Capture, send, and keys

1. `capture` uses `hc read --lines <n>`; `--lines 0` gives the visible screen; `capture --all` uses `hc read --lines 1000000`. The History mark is the read taken before submission by one `hc read --lines 1000000`. The first changed line is the first line index at which the read after settle differs from the mark read; a line that the mark read does not have counts as different. The `prompt` result is the read after settle from the first changed line to its end; when the read after settle reports `how: stitched` or `how: screen`, the result is that whole read instead. This is a stated exception to PRD-261107-0825-01 P6.5 and PRD-260918-0323-01 P7 (Mark): such a result does not start at the History mark. Completeness is true only when both reads report `complete: true` and `how: history` and the first changed line is inside the last `rows` lines (from `hc get --json`) of the mark read; otherwise it is false (this includes `how: stitched` full-screen reads). The PRD-260918-0323-01 P4 no-scrollback notice fires when the `capture --all` read reports `how: screen`.
2. `send` keeps its PRD-261107-0825-01 P9.4 meaning. Each argument that tmux's key grammar parses as a key is sent as the matching `hc keys` key name. Literal text without Enter is sent through `hc keys`, one key per character. Every argument is checked before anything is sent: a key name with no `hc keys` name ends the verb with exit 1 and stderr `cannot type on herdr: <argument as written>`; a character outside the typeable set ends it with exit 1 and stderr `cannot type on herdr: U+<XXXX>` (uppercase hex, at least four digits). The typeable set is: space (sent as the key `space`), `a`–`z`, `A`–`Z` (sent as `shift+<lowercase letter>`), `0`–`9`, and `` ' , - . / ; = [ \ ] ` `` (sent as themselves). `hc keys` cannot type any other character, including `! " # $ % & ( ) * + : < > ? @ ^ _ { | } ~`. Nothing is sent when either check fails. When `hc keys` still rejects a key during the send, the verb exits 1 with the same message; only then can part of the input be sent. `--enter` adds the key `enter`.
3. `send-enter` sends the key `enter`; `escape` sends the key `esc`.
4. `interrupt` sends the resolved Surface's `interrupt_key` through P5.2 and keeps every current refusal rule. It never uses `hc interrupt`.

### P6 — Host state and prompt

1. Capabilities: `native_state` and `submit_confirmation` true; `tile`, `rebalance`, `size`, and `notify` false.
2. Host state comes from `status` in `hc get --json`: `idle`, `working`, `blocked`, and `done` map to themselves; `error` maps to `blocked`; `null`, any other word, or a `finished` terminal map to `null`. Waits follow PRD-261107-0825-01 P6.2.
3. `status --format json` reports `host_word`: the exact hc status word, or `null`. On tmux and Orca, `host_word` is always `null`.
4. In the settle step of `prompt`, a Host state ends the settle only when `status_ms` is after the submission.
5. `prompt` submits with `hc send <terminal> <text>` and never `--force`. When hc refuses because the program is blocked, `prompt` exits 1 with stderr `prompt target blocked: <reason>` and no retry. `submit` is `confirmed` when, against a baseline read before the send, `turn` goes up or the status word changes to `working` with `status_ms` after the send; an unchanged pre-send `working` never confirms. It is `accepted` when `hc send` succeeds and neither is seen within the settle timeout.
6. Delivery (PRD-260918-0323-01 P1): `hc send` wraps the text in bracketed paste when the program has mode 2004 on, and writes it raw otherwise; hc does not report the mode. Delivery is guaranteed for a pane whose Host state is not `null` (the program reports OSC 7501). A multi-line prompt to any other pane is refused before any transmission; other prompts are sent as unavailable delivery.

### P7 — notify, self, list, kill, launch

1. `notify` exits 0 and prints `notify: not delivered on herdr` on stderr (PRD-261107-0825-01 P7.4).
2. `self --format json` reports `host: "herdr"`. `driven` is true when the calling terminal has a `tt.name` label. Without `HC_TERMINAL_ID`, `self`, and every verb that acts on the calling pane without `--target`, exits 1 and names `HC_TERMINAL_ID`.
3. `list` shows the terminals of `hc ls --all --json` that carry `tt.name`, after P4.4. In rows, `pane` is the hc address, `session` is the `herdr.space` label or `null`, and a live terminal has `status: running`; rows gain no `window` key. In a result that already carries a `window` value (`status`, `spawn-agent`, `launch`), it is the `herdr.tab` label or `null`. Every `--format json` key that the result has on tmux and that has no Herdr source is `null` (the PRD-261107-0825-01 P9.13 rule).
4. `kill` resolves the name, applies the self-pane refusal and the Ownership check, runs `hc kill`, then `hc rm`. A `not_found` result after the kill counts as success.
5. `launch` keeps the shell after the command as tmux does (Keep-alive wrap); `--bare` runs the command so the terminal finishes with it.
6. A Target on Herdr is a tmux-tools pane name or an hc address `t<n>`. A registered name wins; a value is used as an hc address only when no registered name matches it.
7. `claude-teams` cannot set up a Team on host `herdr`: it fails open per PRD-261108-0447-01 P2.7 with `<reason>` `no teammate shim on herdr`. So `spawn-agent claude` on Herdr starts Claude with in-process teammates, and the tmux and Orca Launcher paths do not change.

### P8 — Contract tests

1. The seam is the CLI binary, driven by `tests/integration.rs` (PRD-261107-0825-01 P10.1). One in-crate unit test covers the hc name function.
2. The default `cargo test --workspace --features test-fixtures` run tests the binary against a fake `hc` (`tests/contract/fake_hc.py`). It covers P2 (including P2.6 with a stale `TMUX_PANE`), P3, P4.1–P4.4 (including a duplicate name, and a finished `--bare` terminal followed by a respawn of its name), P5.2, P6.2, P6.3, P6.5 (including a send while the previous turn stays `working`), P7 (including P7.7 with a binary named `claude`), and the exit-3 cases.
3. `TT_TEST_HERDR=1` runs the full P10 contract suite against a private core (`HC_SESSION=tt-tests-<pid>`). At the end, also on failure, the run stops that core and deletes its state directory `~/.local/state/herdr-core/tt-tests-<pid>`. It never reaches the default core.
4. `TMUX_TOOLS_LIVE_HERDR=1` runs the contract suite in the default core from a plain Herdr terminal (no `$TMUX`, `HC_TERMINAL_ID` set). It covers a Claude agent pane with Host state, a key-name and a literal send, a multi-line prompt to the Claude pane (guaranteed delivery, P6.6), and a `--split v` pane in the caller's tab.
5. Every terminal a run creates is killed and removed by that run, also on failure. A run ends by asserting that neither `tmux-tools list` nor `hc ls --all --json` shows a terminal it created.

### P9 — Rollout and release

1. The release bumps the minor version. README gains a Herdr host section (selection, P3 placement, labels, P5.2 typeable set and P6.6 delivery rule, `notify`) and lists `herdr` under `TMUX_TOOLS_HOST`.
2. A release requires a passing `TT_TEST_HERDR=1` run and a passing `TMUX_TOOLS_LIVE_HERDR=1` run.
3. The tmux and Orca results change only by the added `host_word` key (P6.3).

## Codebase fit

- Reuse: the `HostPort` trait, its `create_registered` creation path, and its native registration read and write (`core/src/host/mod.rs`); `core/src/host/orca/keys.rs` key-grammar parsing; `src/cmd/safety.rs` and `names::pane_is_owned`; `tests/contract/fake_orca.py` and `tests/contract/orca.rs` as patterns for the fake host and the opt-in live run.
- Conventions: `CONTEXT.md`; ADR-260919-0146-01 (surfaces); PRD-261107-0825-01 (Host port); PRD-260918-0323-01 P1, P3, P7.
- New homes: `core/src/host/herdr.rs` (and `core/src/host/herdr/keys.rs` if the key map needs its own file); `tests/contract/fake_hc.py`; `tests/contract/herdr.rs`.
- Planned-record dependencies: the `requires` references in frontmatter; no `implementation_after`.
- Shared code that changes: `src/cmd/self_.rs`, `src/cmd/status.rs`, `core/src/target.rs` (P2.6); the creation dispatch in `core/src/layout.rs` (P3.4); `src/cmd/claude_teams.rs` (P7.7); `src/cmd/list.rs` only for Herdr rows (P7.3).
- Active-plan overlaps: none. PRD-261108-0447-01 (shim) is implemented; its Herdr shim is out of scope here.

## Contracts

### Business entities

- **`Host`** — enum: `Tmux`, `Orca`, `Herdr`; `as_str` gives `tmux`, `orca`, `herdr`.
- **`HostCapabilities`** — unchanged fields; Herdr values per P6.1.
- **`HostState`** — unchanged.
- **Herdr Pane registration** — hc labels, all string values, absent = `None`: `tt.name`, `tt.agent`, `tt.access`, `tt.launched_at` (RFC 3339), `tt.cwd`, `tt.surface`, `tt.surface_unvalidated` (`"true"` | `"false"`), `tt.parent`, `tt.team`, `tt.shim_id`, `tt.title`, `tt.claude_agent_name`, `tt.claude_team_name`. `PaneRecord` is not used on Herdr.

### External contracts

- **`TMUX_TOOLS_HOST`** — `tmux` | `orca` | `herdr` (P2.1).
- **Detection order** — P2.2.
- **`TMUX_TOOLS_HERDR_SPACE`, `TMUX_TOOLS_HERDR_TAB`** — Herdr space id and tab id for new terminals when no `--target` and no `HC_TERMINAL_ID` (P3.1).
- **hc terminal fields written by this tool** — `--name tt-<16 lowercase hex of SHA-256(name)>`; labels `herdr.space`, `herdr.tab` (P3), `where=<name>`, and the `tt.*` labels; flags `--outlive-parent --no-notify`.
- **JSON results on Herdr** — `host: "herdr"`; `list` rows per P7.3 (no `window` key); `window` in `status`, `spawn-agent`, `launch` is the `herdr.tab` label or `null`; keys with no Herdr source are `null`.
- **`status --format json`** — adds `host_word: string | null` on every host; always `null` on tmux and Orca.
- **`self --format json`** — `host` adds `"herdr"`; `driven` per P7.2.
- **Exit code 3 on Herdr** — `--split tile`, `--size`, `--session`, `--window`, `rebalance` (P3.3).
- **stderr texts** — `notify: not delivered on herdr` (exit 0); `cannot type on herdr: U+<XXXX>` and `cannot type on herdr: <argument as written>` (exit 1); `prompt target blocked: <reason>` (exit 1); `claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes` (P7.7).
- **Test switches** — `TT_TEST_HERDR=1` (private core), `TMUX_TOOLS_LIVE_HERDR=1` (live run).

## Verification and release

1. The default suite passes with the fake-`hc` tests, and the existing tmux and Orca tests pass with only the added `host_word` key.
2. `TT_TEST_HERDR=1` passes, and afterwards `~/.local/state/herdr-core/` holds no `tt-tests-*` directory and the default core's `hc ls --all --json` is unchanged.
3. `TMUX_TOOLS_LIVE_HERDR=1` passes in a plain Herdr terminal started in the maintainer's space and tab. The maintainer confirms in the Herdr window: the `--split v` terminal shows in the caller's tab, a no-split terminal shows as a new tab, and the Claude pane shows Herdr's agent state.
4. After the runs, neither `tmux-tools list` nor `hc ls --all --json` shows a terminal the runs created.

Rollback is reverting the release: the previous version cannot select `herdr`, and `tt.*` labels left on terminals are inert.

## Out of Scope

- The `claude-teams` teammate shim on Herdr (a later tmux-tools PRD, after this backend).
- Harness guard and poll rows for raw `hc` commands (repository `claude`).
- `herdr-app` window control (split direction, focus) and `hc connect` / `attach`.
- The Rex backend.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | Problem Statement; Verification and release |
| scope boundary | decided | Outcome and scope; Out of Scope |
| domain terms | decided | P1 |
| architecture shape | n/a | ratifies the PRD-261107-0825-01 Host port; no new boundary |
| stack | n/a | no new stack; `hc` CLI only (P2.4) |
| data/schema | decided | P4.1; Contracts |
| contracts/integrations | decided | P2–P7; Contracts |
| UX (if UI) | decided | P3 (window placement); Verification item 3 |
| testing/seams | decided | P8 |

