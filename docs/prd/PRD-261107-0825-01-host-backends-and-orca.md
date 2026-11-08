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
2. `src/cmd/*`, `core/src/layout.rs`, `core/src/target.rs`, `core/src/names.rs`, and `core/src/stream.rs` reach a host only through this port. No module outside a backend builds host commands. Exception: `src/bin/tt-vendor-smoke.rs` (PRD-260918-0323-01 P8) keeps its own private tmux server and commands. Tile, `rebalance`, Floor, Effective floor, Ceiling, and Overflow window logic stays in `core/src/layout.rs` and runs only on a host with `tile`.
3. `core/src/tmux.rs` and all tmux command construction become internals of the tmux backend.
4. With the tmux backend selected, every verb keeps its current output and exit behavior; the existing `tests/integration.rs` suite passes without changed expectations.
5. Rules that are tmux facts stay in the tmux backend: Managed session, Bracketed paste transport, and storing Layout intent as the `@tt-tiled` window option.
6. The public library API of `tmux-tools-core` gains the `Host` port, `HostSelection`, `HostCapabilities`, and the backends. The current public tmux paths (`tmux`, `stream`, `TmuxInvocation`, `with_invocation`, `set_global_invocation`) are not kept for compatibility. `agents`, `format`, `idle`, `layout`, `names`, and `target` stay public and reach a host only through the port.

### P3 — Host selection

1. When `TMUX_TOOLS_HOST` is set, it selects the host. A value other than `tmux` or `orca` fails the invocation with exit 1 and names the valid values.
2. Otherwise the first match selects: `$TMUX` set → `tmux`; `ORCA_TERMINAL_HANDLE` set → `orca`. Later PRDs add `REX_BLOCK` → `rex` and `HC_TERMINAL_ID` → `herdr`, in that order, after `orca`.
3. When nothing matches, the host is `tmux` with today's behavior, including the Managed session outside tmux.
4. When the selected host's CLI or runtime is unreachable, the invocation fails with exit 1 and names the host and the selection source. It never falls back to another host.
5. Every `--format json` result that is a JSON object carries `host`. `list --format json` stays a top-level array: each row carries `host` and `host_state`, and an empty list `[]` carries no `host`.

### P4 — Pane names and the registry

1. The tool owns pane names. A host title is display only and is never read to resolve a Target. On every host, a `--name` value must match `[A-Za-z0-9._-]+` and must not be `.` or `..`. Any other value fails the invocation with exit 1 and `invalid name: <name>` on stderr, and no pane or record is created.
2. The tmux backend keeps the `@tt-*` pane options as its Pane registration.
3. The Orca backend stores one Pane record per pane at `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/<host>/<server_id>/<name>.json`, where `server_id` is the Orca `runtimeId`. Orca makes a new `runtimeId` and new handles on every runtime restart, while its panes stay open. After a restart, every pane spawned before it loses its Pane registration: it stays open in Orca, unregistered, and its name is free again. README states this limit. Every invocation that reads the Orca registry deletes each record directory under `orca/` whose name is not the current `server_id`. Pane identity that survives an Orca restart is not part of this PRD (`ISSUE-261107-1229-01`).
4. Spawn reserves the name by creating the record with exclusive create before it creates the pane; a name already reserved fails the spawn with exit 1. After the pane exists, the record gains its handle by write-to-temp and atomic rename. When the host reports that creation failed, the reservation is deleted. When the outcome is ambiguous (for example an Orca split timeout, after which the pane may still open), the backend recovers that same pane through the host's retry handle; it then writes the handle, or closes that pane, before it releases the reservation. Creation is never repeated to recover an ambiguous result.
5. Every invocation that resolves or lists records checks them against the host's pane list. The list is complete when the host reports it as not truncated and with no omitted host (Orca: `truncated` is not true and `hostScope.omittedHostIds` is empty).
   - handle listed → `live`;
   - handle not listed, and the list is complete → `dead`: the record is deleted and the result reports the name as dead;
   - handle not listed, and the list is incomplete → the record stays; resolving that name fails with exit 1 and `pane list incomplete: <name>` on stderr, and `list` shows that name as a row with `status: "unknown"`;
   - no handle and older than the spawn timeout → `orphan-reservation`: the record is deleted.
   On an incomplete list, a name whose handle is listed is `live` and resolves as usual; only names whose handle is not listed fail. When the list is incomplete, `list` prints `list incomplete: <host>` on stderr (`list incomplete: orca` on Orca). The spawn timeout is a fixed 120 seconds. No other state is valid. A dead name is never resolved to another pane. A record with no handle that is younger than the spawn timeout is a spawn in progress: resolving its name fails with exit 1 and `spawn in progress: <name>`, `list` omits it, and reconciliation leaves it in place.
6. There is no daemon and no global lock. Host event streams are read only while the invocation that needs them runs.
7. Ownership on a registry host reads the Pane record: the record exists, and its `cwd` matches the current working directory where both are known.

### P5 — Required and optional verbs

1. Required verbs: `spawn-agent`, `launch`, `execute`, `prompt`, `wait-idle`, `capture`, `send`, `send-enter`, `escape`, `interrupt`, `kill`, `list`, `status`, `notify`, `self`. A backend that cannot serve every required verb does not ship.
2. Optional verb: `rebalance`. Optional placement options: `--split tile` and `--size`.
3. Capabilities are evaluated on each invocation. An optional verb or option invoked on a host whose capabilities lack it exits 3 and prints `unsupported: <verb> on <host>` on stderr, with no side effect. For an option, `<verb>` is the option as written: `unsupported: --split tile on <host>`, `unsupported: --size on <host>`.
4. The automatic tile after a spawn is skipped without a message on a host without `tile`; the host controls the layout.
5. A required verb whose host feature is optional (P6, P7) uses its fallback and never exits 3.

### P6 — Host state, waits, and submission

1. Pane state stays the three-valued `idle | busy | unknown` of PRD-260918-0323-01 P3. Host state is a separate value: `working | blocked | done | idle | null`.
2. On a host with `native_state`, `wait-idle` and the settle step of `prompt` end when Host state is `idle`, `done`, or `blocked`, and stay waiting while it is `working`. When Host state is `null`, Readiness and Idle detection decide, as today. When `--until` or `--idle-seconds` is given explicitly, that option decides as today and Host state neither ends nor extends the wait.
3. `status --format json`, `list --format json`, `wait-idle --format json`, and `prompt --format json` report `host_state`.
4. On a host with `submit_confirmation`, `prompt` uses the host's submission observation instead of the Enter recovery steps. JSON reports `submit`: `confirmed` when the host observed the submission or the turn start; `accepted` when the host accepted the input but observed no submission within the wait; `assumed` when the host gives no observation. Without `submit_confirmation`, `prompt` behaves as today and reports `submit: assumed`.
5. A prompt on every host stays one submission (PRD-260918-0323-01 P1), and its result starts at the History mark and reports Turn observation and Completeness (PRD-260918-0323-01 P7).

### P7 — The notify verb

1. `notify --title <T> --body <B> [--target <name>]` shows a desktop notification for the target pane, or for the calling pane when `--target` is absent. On tmux, without `--target` and with `TMUX_PANE` unset, the verb exits 1 and names `TMUX_PANE`.
2. The tmux backend writes the tmux-wrapped OSC 777 sequence `ESC P tmux; ESC ESC ]777;notify;<T>;<B> BEL ESC \` to the tty of the target pane. tmux unwraps it through passthrough and writes `ESC ]777;notify;<T>;<B> BEL` to the client tty. This is the main path of the harness `notify-tmux.sh`, with its sanitisation: `<T>` and `<B>` lose bytes 0x00–0x1F and 0x7F, and `;` becomes `,`. The verb does not write the standalone BEL that `notify-tmux.sh` writes after the sequence. When the session has no attached client, the verb exits 0 and prints `notify: not delivered on tmux` on stderr.
3. Orca has no notification command; on Orca, notifications come from Orca's own agent hooks, and `notify` follows item 4.
4. On a host without `notify`, the verb exits 0 and prints `notify: not delivered on <host>` on stderr.

### P8 — The self verb

`self --format json` reports `{host, driven, name, agent}` for the calling pane. `driven` is true when the calling pane has a Pane registration (`@tt-name` or `@tt-agent` on tmux; a live Pane record whose handle is the calling pane's handle on Orca). `name` and `agent` are `null` when unknown. Outside every host it reports `host: tmux, driven: false`.

### P9 — Orca backend

1. The backend calls the `orca` CLI with `--json` and reads `result` fields. The calling pane is `ORCA_TERMINAL_HANDLE` and the calling worktree is `ORCA_WORKTREE_ID`. When Orca is selected and a verb needs one of them that is not set (placement without `--target`, including `--split h|v` without `--target`; `self`; `notify` without `--target`), the verb exits 1 and names the missing variable; `server_id` is `result.runtime.runtimeId` from `orca status --json`. On Orca, a Target is a pane name or a `term_…` handle; the `CONTEXT.md` Target entry says so. A verb that acts on a pane and has no `--target` acts on the calling pane, and exits 1 naming `ORCA_TERMINAL_HANDLE` when it is not set. `--session` and `--window` exit 3 with `unsupported: --session on orca` or `unsupported: --window on orca` on stderr and no side effect (the P5.3 rule).
2. Placement: a spawn without `--split`, and `--split window`, opens a new tab with `terminal create --worktree id:$ORCA_WORKTREE_ID --title <name> --command <cmd>`. `--split h|v` runs `terminal split --terminal $ORCA_TERMINAL_HANDLE --direction horizontal|vertical --command <cmd>`. `--split tile` and `--size` exit 3 (P5.3). With `--target`, the split uses the target's handle and the tab opens in the target's worktree (`worktreeId` from `terminal show`). A split timeout is recovered with `--retry-split <leafId>` per P4.4. When a create or split outcome stays ambiguous (no retry handle, or the retry does not resolve the pane), the spawn exits 1 with `spawn outcome unknown: <name>` and the worktree to inspect; the reservation stays until it becomes an orphan reservation, so the name is not reused meanwhile, and a pane that opens later stays unregistered and is visible in `orca terminal list`.
3. `capture` reads `orca terminal read --screen` for the visible screen, and `orca terminal read --cursor <n> --limit <n>` for history. The History mark is the read cursor taken before submission. History reads page with `nextCursor` while `limited` is true and `nextCursor` differs from `latestCursor`, through the settle point; `latestCursor` is never used as a paging token. Orca reports no pane width and no full-screen (alternate-screen) state, so the conditions of PRD-260918-0323-01 P7 cannot be established: every `prompt` result on Orca reports Completeness false, and README states this. A recorded Surface never decides Completeness. On Orca, `capture --all` prints no full-screen notice. Turn observation is true when `busy_regex` matches a screen read, or Host state `working` is seen, between submission and settle.
4. `send` keeps its current meaning. Without `--literal`, each argument is classified by tmux's own key grammar (`key_string_lookup_string` in tmux `key-string.c`): an argument that tmux parses as a key, including single characters, `^x`, and `C-`/`M-`/`S-` modifiers, is sent as the bytes tmux's xterm key output (`input-keys.c`) produces for it in normal cursor and keypad mode; any other argument is literal text. `--literal` sends every argument as text. `--enter` and `--verify` keep their behavior. The backend writes the bytes with `terminal send --text` and no `--enter`. Known limitation, stated in README: Orca does not expose a pane's application cursor or keypad mode, so cursor and keypad keys always send their normal-mode bytes. `send-enter` sends `\r`; `escape` sends `\x1b`.
5. `interrupt` sends the resolved Surface's `interrupt_key` through the item 4 translation, and keeps every current refusal rule (quit hazard, surface-unvalidated pane). It never uses `terminal send --interrupt`.
6. First, when the `orca status` capabilities lack `terminal.prompt-delivery.v1`, `prompt` exits 1 before any transmission and stderr names `terminal.prompt-delivery.v1`, for every prompt text and Surface; it never sends without `--wait-submit` and never reports `submit: "assumed"` for this case. This rule applies only to `prompt`; every other verb works as usual on such a runtime. Otherwise, before it sends, `prompt` determines delivery with the three outcomes of PRD-260918-0323-01 P1. Delivery is guaranteed when all of these hold: `terminal show` reports an `agentIdentity` equal to the pane's registered agent; `orca worktree ps --json` reports the pane's agent row (item 10) with `hasLivePty: true` and a main-agent state; and the `orca status` capabilities include `terminal.prompt-delivery.v1`. This is a proxy: Orca decides on the server whether to bracket, and no field it reports before sending reproduces that check. README states the remaining risk: Orca can still write the raw text, and a shell in the pane then runs each line. Delivery is refused before any transmission when the Surface declares bracketed paste, the text is multi-line, and delivery is not guaranteed; it is unavailable otherwise. Submission observation and the `submit_confirmation` capability never replace this determination. A guaranteed or unavailable prompt is sent with `terminal send --text <prompt> --enter --wait-submit <seconds>`, where `<seconds>` is the `prompt` settle timeout (`--timeout`, else `TMUX_TOOLS_TIMEOUT`, else 120) rounded up to whole seconds and clamped to 1–3600. An ambiguous transport failure is retried once with the reported `--retry-request` ID and never resent otherwise. The error `agent_prompt_blocked`, the observation `permission`, and a receipt with `accepted: false` end the prompt with exit 1 and the reason on stderr (for `accepted: false`, its `refusedReason`), with no retry. The observation `incarnation_replaced` ends it with exit 1 and `prompt target restarted` on stderr.
7. `kill` resolves the name to a live handle and runs `terminal close --terminal <handle>`. The self-pane refusal and the Ownership check apply before close. After a successful close, `kill` deletes the Pane record.
8. The tab title is the pane name from item 2 and is display only (P4.1). The tool has no title verb.
9. `launch` passes the command as `--command`; the Orca login shell stays after the command exits, which satisfies Keep-alive wrap. `--bare` runs `exec <cmd>` so the pane closes with the command.
10. Host state comes from `orca worktree ps --json`: the agent row in `result.worktrees[].agents[]` whose `paneKey` equals `<tabId>:<leafId>` of the pane (from `terminal show`). Main-agent state `working` → `working`; `blocked` and `waiting` → `blocked`; `done` → `done`; no row, or `hasLivePty: false` → `null`. Orca never yields `idle`. While any entry of the row's `subagents[]` has state `working`, Host state is `working`. In the settle step of `prompt`, a Host state ends the settle only when the row's `stateStartedAt` is after the submission.
11. Capabilities: `native_state` and `submit_confirmation` true; `tile`, `rebalance`, `size`, `notify` false. The `submit` mapping, first match: provider `unsupported` or `old-host`, or observation `unsupported` → `assumed`; stages include `turn_started` → `confirmed`; stages are only `input_accepted` → `accepted`.
12. `list`, with or without `--all`, shows the Pane records under the current `server_id`, after the P4.5 check. In `list` rows on Orca, `session` is the pane's worktree id and `pane` is its handle; a live record has `status: running`. A dead name appears in one result as a row with `status: dead`, and its record is then deleted (P4.5). A record whose handle is not listed on an incomplete list has `status: unknown` (P4.5). In a result that carries a `window` value, it is the pane's tab id.
13. On Orca, every `--format json` key that the result has on tmux is kept, with value `null` when Orca has no source for it, so every host gives the same keys. These keys are `null` on Orca: in `status`, `window_index`, `pane`, `min_pane_cols`, `min_pane_rows`, `below_floor`; in `list` rows, `last_activity`; in `spawn-agent` and `launch`, `relocated`, `window_id`, `min_pane_cols`, `min_pane_rows`, `below_floor`.

### P10 — Backend contract tests

1. `tests/integration.rs` becomes the contract suite: it runs the real binary through every required verb, once per backend. The Host port is tested only through the CLI binary. The tmux run uses the isolated tmux server in the default `cargo test --workspace --features test-fixtures` run.
2. The same suite runs against a live Orca runtime behind an opt-in feature or environment switch; it never runs by default. It runs from inside an Orca terminal, which supplies `ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID`.
3. The Orca run covers an ambiguous split recovered through `--retry-split` (P4.4), a prompt response that spans several read pages (P9.3), key-name and `--literal` sends, a Surface `interrupt_key` other than `C-c`, the arguments `M-x`, `F1`, `Up`, `y`, `^c`, and a plain word, a refused multi-line prompt (P9.6), and a pane whose rendering changes to full-screen with no registration change (P9.3).
4. Every pane a contract run creates is closed by that run, also on failure. A run ends by asserting that neither `tmux-tools list` nor `orca terminal list` shows a pane it created.

### P11 — Rollout order

1. The Host port refactor with only the tmux backend lands first, with zero behavior change (P2.4).
2. `self`, `notify`, exit code 3, and the `host` and `host_state` fields land next, on tmux.
3. The Orca backend lands last.
4. The release that contains the Orca backend bumps the minor version and updates README's verb summary, Configuration (`TMUX_TOOLS_HOST`), a host section that states the Orca limits of P4.3, P9.3, P9.4 and P9.6, and the `## Library: configurable invocation` section, which describes the public API of P2.6 instead of the removed tmux paths.
5. Step 2 adds a README note to the `notify` description: under tmux, delivery needs the tmux option `allow-passthrough` set to `on` (only a pane that is visible in an attached client) or `all` (any pane), and the outer terminal must support OSC 777.

## Codebase fit

- Reuse: `src/cmd/*` verb handlers and their output formats; `core/src/agents` (Agent registry, Surface, Readiness patterns); `core/src/idle.rs` (Pane state classifier); `src/cmd/safety.rs` and `names::pane_is_owned` (self-pane refusal, Ownership); `tests/integration.rs` isolated tmux harness.
- Conventions: `CONTEXT.md` glossary; ADR-260919-0146-01 (surfaces); PRD-260918-0323-01 P1, P3, P7 (one submission, three-valued Pane state, History mark and Completeness).
- New homes: `core/src/host/mod.rs` (port, `Host`, `HostSelection`, `HostCapabilities`), `core/src/host/tmux/` (current `tmux.rs` and tmux command construction), `core/src/host/orca.rs`, `core/src/registry.rs` (`PaneRecord`, `PaneLiveness`), `src/cmd/notify.rs`, `src/cmd/self_.rs`; `tests/integration.rs` becomes the per-backend contract suite.
- Planned-record dependencies: none.
- Active-plan overlaps: none in this repository. Repository `claude` owns the consuming harness PRD, which declares `implementation_after` this PRD.
- Library consumers: `SilverBond` depends on `tmux-tools-core` pinned to a Git revision and uses `TmuxInvocation`, `tmux::DEFAULT_TMUX_COMMAND_TIMEOUT`, `stream::capture_ansi`, and `agents`; P2.6 breaks the first three paths for its next update.

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
- **Changed JSON results** — every `--format json` object result adds `host`; `status`, `wait-idle`, and `prompt` add `host_state: "working"|"blocked"|"done"|"idle"|null`; `list --format json` stays a top-level array, each row adds `host` and `host_state` with the same values, and an empty list `[]` carries no `host`; `prompt` adds `submit: "confirmed"|"accepted"|"assumed"`; `list` row `status` adds `unknown` (P4.5); on Orca, the keys of P9.13 are `null`.
- **Exit code 3** — `unsupported` for an optional verb or option on a host that lacks it (P5.3), and for `--session` and `--window` on Orca (P9.1). Exit 0 and 1 keep their meaning.

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
- Claude Code teammate panes: owned by PRD-261108-0447-01 (tmux-tools tmux shim).
- Harness skill, hook, and guard changes (harness PRD in repository `claude`).
- Moving `SilverBond` to the `Host` port (work in the `SilverBond` repository).

## Open Questions

None.

## Notes

- Maintainer action, outside this PRD: ask Orca upstream for a field that reports, before sending, whether `terminal send` will bracket the text (P9.6).

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; ## Verification and release |
| scope boundary | decided | ## Outcome and scope; ## Out of Scope |
| domain terms | decided | P1 |
| architecture shape | decided | P2 |
| stack | n/a | brownfield Rust workspace — existing conventions ratified |
| data/schema | decided | P4.3; ## Contracts |
| contracts/integrations | decided | P4.5; P9.6; P9.12; P9.13; ## Contracts |
| UX (if UI) | n/a | CLI only; output formats per ## Contracts |
| testing/seams | decided | P10 |
