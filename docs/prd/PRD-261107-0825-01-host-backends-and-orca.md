---
id: PRD-261107-0825-01
kind: prd
status: active
scale: epic
stakes: internal
gate: passed 2026-11-07
contracts: approved 2026-11-07
contexts: [tmux-tools]
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane record, Pane liveness, Pane registration, Pane state, Target, Ownership, Tile, Layout intent, History mark, Completeness, Bracketed paste transport, Keep-alive wrap, Surface, Readiness, Idle detection, Turn observation]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: problem
      heading: Problem Statement
    - id: outcome
      heading: Outcome and scope
    - id: p1-definitions
      heading: P1 — Definitions
    - id: p2-host-port
      heading: P2 — One pane-level Host port
    - id: p3-selection
      heading: P3 — Host selection
    - id: p4-registration
      heading: P4 — Pane names and the registry
    - id: p5-verb-classes
      heading: P5 — Required and optional verbs
    - id: p6-host-state
      heading: P6 — Host state, waits, and submission
    - id: p7-notify
      heading: P7 — The notify verb
    - id: p8-self
      heading: P8 — The self verb
    - id: p9-orca
      heading: P9 — Orca backend
    - id: p10-tests
      heading: P10 — Backend contract tests
    - id: p11-rollout
      heading: P11 — Rollout order
    - id: contracts
      heading: Contracts
    - id: verification
      heading: Verification and release
  references:
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
      clause: p7-prompt
    - relation: requires
      repository: tmux-tools
      record: ADR-260919-0146-01
---

# Host backends and the Orca backend

## Problem Statement

`tmux-tools` can drive panes only in tmux. Under an agent terminal host (Orca now; Rex and Herdr later) its panes are invisible to the host GUI and get none of the host's agent status, submission confirmation, or notifications. Callers that want those features must use each host's own CLI, which skips this tool's naming, ownership, readiness, and teardown rules.

## Outcome and scope

`tmux-tools` keeps its verbs and their meaning and runs them against the host it runs under. This PRD delivers the host port, host selection, the registry for hosts without pane options, the `notify` and `self` verbs, the `unsupported` exit code, and the Orca backend. The harness changes that consume this CLI are owned by the harness PRD in repository `claude`.

## Required behavior

### P1 — Definitions

- **Host:** the terminal program whose panes the tool drives: `tmux` or `orca`.
- **Host backend:** the implementation of the P2 port for one host.
- **Host selection:** the host chosen for one invocation and the source of that choice (P3).
- **Host capabilities:** the optional features a backend supports (`## Contracts`).
- **Host state:** the agent state a host itself reports for a pane, or `null` when it reports none.
- **Pane record:** the registry file that holds the Pane registration for a host that cannot store custom data on a pane (P4).
- **Pane liveness:** `live`, `dead`, or `orphan-reservation` (P4).

Every term in this clause is added to `CONTEXT.md` with its code home. These existing `CONTEXT.md` entries are rewritten host-neutral: Pane registration (`@tt-*` options on tmux, the Pane record on a registry host), Ownership (its evidence comes from the Pane registration of the pane's host), Readiness and Idle detection (Host state decides first when it is not `null`, P6.2), and Keep-alive wrap (on Orca, the login shell that stays after the command, P9.9), History mark (on Orca, a read cursor, P9.3), Completeness (its conditions per host, P9.3), and Turn observation (Host state `working` also counts, P9.3).

### P2 — One pane-level Host port

1. `core` defines one `Host` port with pane-level operations: create (new pane, split, or window), list, capture (screen and history from a mark), send text, send keys, close, read and write registration, host state, notify, capabilities, and the layout operations gated by `tile`: apply tiled layout, read window and pane geometry, read and write Layout intent, and move a pane to a new window.
2. `src/cmd/*`, `core/src/layout.rs`, `core/src/target.rs`, `core/src/names.rs`, and `core/src/stream.rs` reach a host only through this port. No module outside a backend builds host commands. Tile, `rebalance`, Floor, Effective floor, Ceiling, and Overflow window logic stays in `core/src/layout.rs` and runs only on a host with `tile`.
3. `core/src/tmux.rs` and all tmux command construction become internals of the tmux backend.
4. With the tmux backend selected, every verb keeps its current output and exit behavior; the existing `tests/integration.rs` suite passes without changed expectations.
5. Rules that are tmux facts stay in the tmux backend: Managed session, Bracketed paste transport, and storing Layout intent as the `@tt-tiled` window option.
6. The public library API of `tmux-tools-core` is the `Host` port, `HostSelection`, `HostCapabilities`, and the backends. The current public tmux paths (`tmux`, `stream`, `TmuxInvocation`) are not kept for compatibility.

### P3 — Host selection

1. When `TMUX_TOOLS_HOST` is set, it selects the host. A value other than `tmux` or `orca` fails the invocation with exit 1 and names the valid values.
2. Otherwise the first match selects: `$TMUX` set → `tmux`; `ORCA_TERMINAL_HANDLE` set → `orca`. Later PRDs add `REX_BLOCK` → `rex` and `HC_TERMINAL_ID` → `herdr`, in that order, after `orca`.
3. When nothing matches, the host is `tmux` with today's behavior, including the Managed session outside tmux.
4. When the selected host's CLI or runtime is unreachable, the invocation fails with exit 1 and names the host and the selection source. It never falls back to another host.
5. Every `--format json` result carries `host`.

### P4 — Pane names and the registry

1. The tool owns pane names. A host title is display only and is never read to resolve a Target.
2. The tmux backend keeps the `@tt-*` pane options as its Pane registration.
3. The Orca backend stores one Pane record per pane at `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/<host>/<server_id>/<name>.json`, where `server_id` is the Orca `runtimeId`.
4. Spawn reserves the name by creating the record with exclusive create before it creates the pane; a name already reserved fails the spawn with exit 1. After the pane exists, the record gains its handle by write-to-temp and atomic rename. When the host reports that creation failed, the reservation is deleted. When the outcome is ambiguous (for example an Orca split timeout, after which the pane may still open), the backend recovers that same pane through the host's retry handle; it then writes the handle, or closes that pane, before it releases the reservation. Creation is never repeated to recover an ambiguous result.
5. Every invocation that resolves or lists records checks them against the host's pane list:
   - handle listed → `live`;
   - handle not listed → `dead`: the record is deleted and the result reports the name as dead;
   - no handle and older than the spawn timeout → `orphan-reservation`: the record is deleted.
   No other state is valid. A dead name is never resolved to another pane. A record with no handle that is younger than the spawn timeout is a spawn in progress: resolving its name fails with exit 1 and `spawn in progress: <name>`, `list` omits it, and reconciliation leaves it in place.
6. There is no daemon and no global lock. Host event streams are read only while the invocation that needs them runs.
7. Ownership on a registry host reads the Pane record: the record exists, and its `cwd` matches the current working directory where both are known.

### P5 — Required and optional verbs

1. Required verbs: `spawn-agent`, `launch`, `execute`, `prompt`, `wait-idle`, `capture`, `send`, `send-enter`, `escape`, `interrupt`, `kill`, `list`, `status`, `notify`, `self`. A backend that cannot serve every required verb does not ship.
2. Optional verb: `rebalance`. Optional placement options: `--split tile` and `--size`.
3. Capabilities are evaluated on each invocation. An optional verb or option invoked on a host whose capabilities lack it exits 3 and prints `unsupported: <verb> on <host>` on stderr, with no side effect.
4. The automatic tile after a spawn is skipped without a message on a host without `tile`; the host controls the layout.
5. A required verb whose host feature is optional (P6, P7) uses its fallback and never exits 3.

### P6 — Host state, waits, and submission

1. Pane state stays the three-valued `idle | busy | unknown` of PRD-260918-0323-01 P3. Host state is a separate value: `working | blocked | done | idle | null`.
2. On a host with `native_state`, `wait-idle` and the settle step of `prompt` end when Host state is `idle`, `done`, or `blocked`, and stay waiting while it is `working`. When Host state is `null`, Readiness and Idle detection decide, as today.
3. `status --format json`, `list --format json`, `wait-idle --format json`, and `prompt --format json` report `host_state`.
4. On a host with `submit_confirmation`, `prompt` uses the host's submission observation instead of the Enter recovery steps. JSON reports `submit`: `confirmed` when the host observed the submission or the turn start; `accepted` when the host accepted the input but observed no submission within the wait; `assumed` when the host gives no observation. Without `submit_confirmation`, `prompt` behaves as today and reports `submit: assumed`.
5. A prompt on every host stays one submission (PRD-260918-0323-01 P1), and its result starts at the History mark and reports Turn observation and Completeness (PRD-260918-0323-01 P7).

### P7 — The notify verb

1. `notify --title <T> --body <B> [--target <name>]` shows a desktop notification for the target pane, or for the calling pane when `--target` is absent.
2. The tmux backend writes OSC 777 through tmux passthrough to the client tty of the pane's session, the behavior of the harness `notify-tmux.sh`.
3. Orca has no notification command; on Orca, notifications come from Orca's own agent hooks, and `notify` follows item 4.
4. On a host without `notify`, the verb exits 0 and prints `notify: not delivered on <host>` on stderr.

### P8 — The self verb

`self --format json` reports `{host, driven, name, agent}` for the calling pane. `driven` is true when the calling pane has a Pane registration (`@tt-name` or `@tt-agent` on tmux; a live Pane record whose handle is the calling pane's handle on Orca). `name` and `agent` are `null` when unknown. Outside every host it reports `host: tmux, driven: false`.

### P9 — Orca backend

1. The backend calls the `orca` CLI with `--json` and reads `result` fields. The calling pane is `ORCA_TERMINAL_HANDLE` and the calling worktree is `ORCA_WORKTREE_ID`. When Orca is selected and a verb needs one of them that is not set (placement without `--target`, including `--split h|v` without `--target`; `self`; `notify` without `--target`), the verb exits 1 and names the missing variable; `server_id` is `result.runtime.runtimeId` from `orca status --json`. [OPEN: orca-runtime-id]
2. Placement: a spawn without `--split`, and `--split window`, opens a new tab with `terminal create --worktree id:$ORCA_WORKTREE_ID --title <name> --command <cmd>`. `--split h|v` runs `terminal split --terminal $ORCA_TERMINAL_HANDLE --direction horizontal|vertical --command <cmd>`. `--split tile` and `--size` exit 3 (P5.3). With `--target`, the split uses the target's handle and the tab opens in the target's worktree (`worktreeId` from `terminal show`). A split timeout is recovered with `--retry-split <leafId>` per P4.4. When a create or split outcome stays ambiguous (no retry handle, or the retry does not resolve the pane), the spawn exits 1 with `spawn outcome unknown: <name>` and the worktree to inspect; the reservation stays until it becomes an orphan reservation, so the name is not reused meanwhile, and a pane that opens later stays unregistered and is visible in `orca terminal list`.
3. `capture` reads `orca terminal read --screen` for the visible screen, and `orca terminal read --cursor <n> --limit <n>` for history. The History mark is the read cursor taken before submission. History reads page with `nextCursor` while `limited` is true and `nextCursor` differs from `latestCursor`, through the settle point; `latestCursor` is never used as a paging token. Completeness is false when any page reports `truncated: true`, when cursors are unavailable (`nextCursor: null`), when the full range cannot be retrieved, when live Orca evidence sampled between the mark and settle shows full-screen rendering (the stream then holds repaint fragments, and the result is the screen read), when that live evidence is unavailable, or when the pane width changed between the mark and settle. A recorded Surface never decides this. [OPEN: orca-live-render] Turn observation is true when `busy_regex` matches a screen read, or Host state `working` is seen, between submission and settle.
4. `send` keeps its current meaning. Without `--literal`, each argument is classified by tmux's own key grammar (`key_string_lookup_string` in tmux `key-string.c`): an argument that tmux parses as a key, including single characters, `^x`, and `C-`/`M-`/`S-` modifiers, is sent as the bytes tmux's xterm key output (`input-keys.c`) produces for it in normal cursor and keypad mode; any other argument is literal text. `--literal` sends every argument as text. `--enter` and `--verify` keep their behavior. The backend writes the bytes with `terminal send --text` and no `--enter`. Known limitation, stated in README: Orca does not expose a pane's application cursor or keypad mode, so cursor and keypad keys always send their normal-mode bytes. `send-enter` sends `\r`; `escape` sends `\x1b`.
5. `interrupt` sends the resolved Surface's `interrupt_key` through the item 4 translation, and keeps every current refusal rule (quit hazard, surface-unvalidated pane). It never uses `terminal send --interrupt`.
6. Before it sends, `prompt` determines delivery with the three outcomes of PRD-260918-0323-01 P1: guaranteed when Orca reports the pane as a running, recognized agent on a host that brackets agent prompts; refused before any transmission when the Surface declares bracketed paste, the text is multi-line, and that is not established; unavailable otherwise. [OPEN: orca-paste-determination] Submission observation and the `submit_confirmation` capability never replace this determination. A guaranteed or unavailable prompt is sent with `terminal send --text <prompt> --enter --wait-submit <seconds>`. An ambiguous transport failure is retried once with the reported `--retry-request` ID and never resent otherwise.
7. `kill` resolves the name to a live handle and runs `terminal close --terminal <handle>`. The self-pane refusal and the Ownership check apply before close.
8. The tab title is the pane name from item 2 and is display only (P4.1). The tool has no title verb.
9. `launch` passes the command as `--command`; the Orca login shell stays after the command exits, which satisfies Keep-alive wrap. `--bare` runs `exec <cmd>` so the pane closes with the command.
10. Native state comes from Orca's agent state for the pane. [OPEN: orca-agent-state]
11. Capabilities: `native_state` and `submit_confirmation` true; `tile`, `rebalance`, `size`, `notify` false. The `submit` mapping: an Orca receipt that reports submission or turn start → `confirmed`; the queued or input-accepted receipt on timeout → `accepted`; provider `old-host` → `assumed`.

### P10 — Backend contract tests

1. `tests/integration.rs` becomes the contract suite: it runs the real binary through every required verb, once per backend. The Host port is tested only through the CLI binary. The tmux run uses the isolated tmux server in the default `cargo test --workspace --features test-fixtures` run.
2. The same suite runs against a live Orca runtime behind an opt-in feature or environment switch; it never runs by default. It runs from inside an Orca terminal, which supplies `ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID`.
3. The Orca run covers an ambiguous split recovered through `--retry-split` (P4.4), a prompt response that spans several read pages (P9.3), key-name and `--literal` sends, a Surface `interrupt_key` other than `C-c`, the arguments `M-x`, `F1`, `Up`, `y`, `^c`, and a plain word, a refused multi-line prompt (P9.6), and a pane whose rendering changes to full-screen with no registration change (P9.3).
4. Every pane a contract run creates is closed by that run, also on failure. A run ends by asserting that neither `tmux-tools list` nor `orca terminal list` shows a pane it created.

### P11 — Rollout order

1. The Host port refactor with only the tmux backend lands first, with zero behavior change (P2.4).
2. `self`, `notify`, exit code 3, and the `host` and `host_state` fields land next, on tmux.
3. The Orca backend lands last.
4. The release that contains the Orca backend bumps the minor version and updates README's verb summary, Configuration (`TMUX_TOOLS_HOST`), and a host section.

## Codebase fit

- Reuse: `src/cmd/*` verb handlers and their output formats; `core/src/agents` (Agent registry, Surface, Readiness patterns); `core/src/idle.rs` (Pane state classifier); `src/cmd/safety.rs` and `names::pane_is_owned` (self-pane refusal, Ownership); `tests/integration.rs` isolated tmux harness.
- Conventions: `CONTEXT.md` glossary; ADR-260919-0146-01 (surfaces); PRD-260918-0323-01 P1, P3, P7 (one submission, three-valued Pane state, History mark and Completeness).
- New homes: `core/src/host/mod.rs` (port, `Host`, `HostSelection`, `HostCapabilities`), `core/src/host/tmux/` (current `tmux.rs` and tmux command construction), `core/src/host/orca.rs`, `core/src/registry.rs` (`PaneRecord`, `PaneLiveness`), `src/cmd/notify.rs`, `src/cmd/self_.rs`; `tests/integration.rs` becomes the per-backend contract suite.
- Planned-record dependencies: none.
- Active-plan overlaps: none in this repository. Repository `claude` owns the consuming harness PRD, which declares `implementation_after` this PRD.
- Library consumers: `SilverBond` depends on `tmux-tools-core` pinned to a Git revision and uses `TmuxInvocation`, `tmux::DEFAULT_TMUX_COMMAND_TIMEOUT`, `stream::capture_ansi`, and `agents`; P2.6 breaks these paths for its next update.

## Contracts

### Business entities

- **`Host`** — enum: `Tmux`, `Orca`.
- **`HostSelection`** — `host: Host`; `source: SelectionSource`, where `SelectionSource` is `Override` | `Detected(env_var: String)` | `Default`.
- **`HostCapabilities`** — `native_state: bool`, `submit_confirmation: bool`, `notify: bool`, `tile: bool`, `rebalance: bool`, `size: bool`.
- **`PaneRecord`** — `schema: "tmux-tools.pane.v1"`, `name: String`, `host: Host`, `server_id: String`, `handle: Option<String>` (`None` = reservation), `agent: Option<String>`, `access: Option<String>`, `surface: Option<String>`, `surface_unvalidated: bool`, `cwd: PathBuf`, `launched_at: String` (RFC 3339).
- **`PaneLiveness`** — enum: `Live`, `Dead`, `OrphanReservation`.

### External contracts

- **Environment variable `TMUX_TOOLS_HOST`** — values `tmux` | `orca`; any other value is an error (P3.1).
- **Host detection order** — P3.2.
- **Registry file** — path per P4.3; content is `PaneRecord` as one JSON object, keys as the field names above, `host` lowercase.
- **Verb `notify`** — `notify --title <T> --body <B> [--target <name>]`; behavior per P7.
- **Verb `self`** — `self --format json` → `{"host": "tmux"|"orca", "driven": bool, "name": string|null, "agent": string|null}`.
- **Changed JSON results** — every `--format json` result adds `host`; `status`, `list`, `wait-idle`, and `prompt` add `host_state: "working"|"blocked"|"done"|"idle"|null`; `prompt` adds `submit: "confirmed"|"accepted"|"assumed"`.
- **Exit code 3** — `unsupported` for an optional verb on a host that lacks it (P5.3). Exit 0 and 1 keep their meaning.

## Verification and release

The Orca backend is done when all of these hold:

1. The P10 contract suite passes on tmux in the default test run and on a live Orca runtime through the opt-in switch.
2. Under Orca, the harness skills `codex-developer` and `claude-cli` each complete spawn, prompt, wait, resume-handle capture, and kill with no skill text changed.
3. Host state for a driven agent shows in the Orca GUI, Orca's own agent-hook notifications fire for driven agents, and `notify` on Orca reports not delivered (P7.4).
4. After those runs, neither `tmux-tools list` nor `orca terminal list` shows a leaked pane.

Release check per step of P11: the existing suite passes unchanged after step 1. Rollback is reverting the release. The previous version does not read Pane records, so records left on disk are inert.

## Out of Scope

- Rex and Herdr backends (later PRDs, same port).
- Renaming `tmux-tools`.
- Agent-to-agent messaging, Orca `orchestration`.
- Claude Code teammate panes (`teammateMode`, `orca claude-teams`).
- Harness skill, hook, and guard changes (harness PRD in repository `claude`).
- Moving `SilverBond` to the `Host` port (work in the `SilverBond` repository).

## Open Questions

### orca-runtime-id
- question: Does Orca `runtimeId` stay the same across a desktop restart while its terminal daemon keeps terminals alive?
- dimension: data/schema
- resolve-by: during-triage
- owner: this PRD

### orca-paste-determination
- question: Which Orca fields establish before sending that a pane is a running, recognized agent on a host that brackets agent prompts (`terminal show` `agentIdentity`, `terminal read` `status`, `orca status` capabilities)?
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

### orca-live-render
- question: Which live Orca fields show full-screen rendering and the pane width at the mark, during the turn, and at settle (`terminal read` `source` and `tailRows`, `terminal show`)?
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

### orca-agent-state
- question: Which Orca field or command gives working/blocked/done/idle for a pane (`terminal show` `agentWait`, `terminal wait --for tui-idle`, or the agent hooks), and how does it map to Host state?
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; ## Verification and release |
| scope boundary | decided | ## Outcome and scope; ## Out of Scope |
| domain terms | decided | P1 |
| architecture shape | decided | P2 |
| stack | n/a | brownfield Rust workspace — existing conventions ratified |
| data/schema | deferred | open-questions: orca-runtime-id |
| contracts/integrations | deferred | open-questions: orca-agent-state, orca-paste-determination, orca-live-render |
| UX (if UI) | n/a | CLI only; output formats per ## Contracts |
| testing/seams | decided | P10 |
