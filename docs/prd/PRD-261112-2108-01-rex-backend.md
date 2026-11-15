---
id: PRD-261112-2108-01
kind: prd
status: active
scale: epic
stakes: internal
gate: passed 2026-11-14
contracts: approved 2026-11-14
contexts: [tmux-tools]
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Pane record, Pane state, Target, Ownership, Parent, Rex block, History mark, Completeness, Keep-alive wrap, Surface, Readiness, Idle detection, Turn observation]
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
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p6-state
    - relation: implementation_after
      repository: tmux-tools
      record: PRD-261109-2223-01
---

# Rex host backend

## Problem Statement

Under Rex, panes that `tmux-tools` creates do not show in the Rex app and get none of Rex's program state. Callers that want Rex must use the `rex` CLI directly, which skips this tool's naming, ownership, readiness, and teardown rules.

## Outcome and scope

`tmux-tools` runs every required verb against Rex through a `rex` Host backend that implements the Host port of PRD-261107-0825-01 P2 over the `rex` CLI and its control API. Driven blocks show in the target's or caller's Rex session. This PRD owns the backend, its selection, its Pane registration, its tests, and its release. The Rex teammate shim and the harness `rex` guard rows are out of scope.

## Required behavior

### P1 — Definitions

- **Host** gains the value `rex`: Rex blocks driven through the `rex` CLI.
- **Rex block:** one Rex terminal block, addressed by its block id `block:<uuid>`. On Rex, a pane is a Rex block.
- On Rex, the calling pane is the block named by `REX_BLOCK`.
- A Target on Rex is a registered tmux-tools pane name or a block id. A registered name wins; a value is used as a block id only when no registered name matches it.

`CONTEXT.md` is updated: Host, Host backend, Host selection, Host capabilities, and Host state name the Rex backend, its detection rule, and its values; Pane registration names the Rex Pane record (P4.1); Target names the block-id form; History mark and Completeness name the Rex rules (P5.2); Keep-alive wrap names the Rex rule (P7.5); `Rex block` is minted.

### P2 — Host selection

1. `TMUX_TOOLS_HOST` accepts `tmux`, `orca`, `rex`, and `herdr`. Any other value fails with exit 1 and names all four.
2. Detection order, first match: `$TMUX` set → `tmux`; `ORCA_TERMINAL_HANDLE` set → `orca`; `REX_BLOCK` set → `rex`; `HC_TERMINAL_ID` set → `herdr`. Under tmux the host stays `tmux` unless `TMUX_TOOLS_HOST=rex`.
3. Every `rex` invocation passes `--autostart=false`, and passes `-S "$REX_SERVER"` when `REX_SERVER` is set. The P4.1 sweep check still passes `--autostart=false`; it replaces only the `-S "$REX_SERVER"` rule with the stored address of the directory it checks.
4. Rex is unreachable when `rex` is not on `PATH` or `rex server status --json` fails. When Rex is selected and unreachable, the invocation fails with exit 1, names `rex` and the selection source, and never falls back to another host (PRD-261107-0825-01 P3.4).
5. `server_identity` is `instance_id` from `rex server status --json`.
6. Every lookup of the calling pane goes through the Host port: `self`, `status` without `--target`, the self-pane refusal, and the Pane record's `parent` use `REX_BLOCK` on Rex and never read `$TMUX` or `TMUX_PANE` there, also when a stale `TMUX_PANE` is set. Without `REX_BLOCK`, `self` and every verb that acts on the calling pane without `--target` exits 1 and names `REX_BLOCK`.

### P3 — Placement and visibility

1. The session of a new block, first match: the `--target` block's session; `REX_SESSION`; `TMUX_TOOLS_REX_SESSION`. When none gives a session, the spawn exits 1, names `REX_SESSION` and `TMUX_TOOLS_REX_SESSION`, and creates nothing.
2. No `--split`, and `--split window`: `rex run -s <session>`, which opens a new Rex window (a tab in the Rex app) in that session.
3. `--split h` → `rex split --split=right`, and `--split v` → `rex split --split=below`, with `-b <anchor>`. The anchor is the `--target` block, else the calling block. When neither exists, the spawn exits 1, names `REX_BLOCK`, and creates nothing.
4. Every create passes `--focus=false`, `--label <label>`, `--cwd <cwd>`, and `--json`, and never `--keep-open`. The label is the first 128 characters of the pane name, because a Rex label holds at most 128 characters. Pane names keep no length limit.
5. `--split tile`, `--size`, `--session`, `--window`, and `rebalance` exit 3 with the PRD-261107-0825-01 P5.3 messages and no side effect. The automatic tile after a spawn is skipped (P5.4).

### P4 — Pane registration, names, and lifetime

1. The Pane registration is a Pane record at `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/rex/<instance_id>/<name>.json`, with `server_id` = `instance_id` and `handle` = block id. Name validation, reservation, reconciliation, the 120-second spawn timeout, and Ownership follow PRD-261107-0825-01 P4.1 and P4.3–P4.7 as for Orca, except the P4.3 rule that deletes every server directory whose name is not the current `server_id`: on Rex, only the address check below deletes another server directory. Each server directory holds the file `server-address`: the `REX_SERVER` value, or `unset` when `REX_SERVER` is not set. The file name cannot be a Pane record, because it has no `.json` suffix. A server directory is published atomically: the invocation makes `rex/.init-<random>/`, writes the complete `server-address` in it, and renames it to `rex/<instance_id>/`; when the target already exists, it deletes its temporary directory and uses the target. `reserve` and `write` never make a server directory themselves; they use this step when the directory is missing. Every invocation that reads the Rex registry skips entries directly in `rex/` whose names start with `.`, except that it deletes such a `.init-` directory older than 120 seconds. Records inside a server directory keep the name set of PRD-261107-0825-01 P4.1, including names that start with `.`. It checks each other directory under `rex/`: it runs one `rex server status --json` against that directory's stored address (with `-S <address>`; for `unset`, without `-S` and with `REX_SERVER` removed from the call's environment, so it reaches the default server whatever `REX_SERVER` the caller has) with a 2-second timeout, and deletes the directory only when that call fails, times out, or reports a different `instance_id`. A directory with no readable `server-address` file is deleted. The same server reached with `REX_SERVER` unset and with its explicit address uses one directory.
2. The block label (P3.4) is for display only. It is never read to resolve a Target; only P4.4 reads it.
3. A block listing is complete only when `rex ls --json` and `rex block ls -s <session> --json` for every listed session all succeed. A record whose block id is missing from a complete listing is `dead`. An incomplete listing follows PRD-261107-0825-01 P4.5, and `list` prints `list incomplete: rex` on stderr.
4. Spawn result unknown. A create that exits 3 (not found) or 4 (no server, or connection refused) has failed: the reservation is deleted. Any other create result with no block id (a timeout, another non-zero exit, output that does not parse) is ambiguous. On an ambiguous result, the backend lists the blocks of the target session once. When exactly one block has a label equal to the first 128 characters of the pane name, no live record holds it, and no other record or reservation in this server's directory has a name with the same first 128 characters, the record gains that block id. Otherwise the spawn exits 1 with stderr `spawn outcome unknown: <name> (session <session id>)` and keeps the reservation until it is an orphan reservation (PRD-261107-0825-01 P4.5). Creation is never repeated. This replaces the retry-handle recovery of PRD-261107-0825-01 P4.4 on Rex, which has no retry handle.
5. The record's `parent` follows PRD-261108-0447-01 P7.

### P5 — Capture, send, and keys

1. Every capture is `rex capture -b <block>` in text format, without `--unwrap`. `--lines n` gives the last `n` lines; `--lines 0` gives the last `rows` lines, with `rows` from `rex block call -b <block> size`; `--all` gives the whole capture. The PRD-260918-0323-01 P4 no-scrollback notice fires when `rex block call -b <block> modes` reports the alternate screen on.
2. The History mark is a full capture taken before submission. The `prompt` result starts at the first changed line, by the rule of PRD-261109-2223-01 P5.1. That rule moves out of `core/src/host/herdr.rs` into one shared module that both backends call. `rows` is read from one `rex block call -b <block> size` taken after settle, and gives `rows` both for the guard and for the result. Completeness is true only when the mark capture and the capture after settle were both taken with the alternate screen off and the same `size` width, and the first changed line is at least the number of lines of the mark capture minus `rows`; otherwise it is false. The line-count guard catches a full scrollback, where Rex drops the oldest line for each new line. When Completeness is false for any of these reasons (full scrollback, width change, alternate screen), the `prompt` result is the last `rows` lines of the capture after settle. This is a stated exception to PRD-261107-0825-01 P6.5 and PRD-260918-0323-01 P7 (Mark), as for Herdr's `how: screen` read in PRD-261109-2223-01 P5.1: such a result does not start at the History mark. README states that Rex drops the oldest scrollback silently.
3. `send` keeps its PRD-261107-0825-01 P9.4 meaning. Arguments that tmux's key grammar parses as keys become xterm bytes by the Orca rule (`core/src/host/orca/keys.rs`); cursor keys use application-mode bytes when `modes` reports mode 1 on. Key bytes and literal text are sent through `rex send -b <block>`. `--enter` adds `\r`.
4. `send-enter` sends `\r`; `escape` sends `\x1b`.
5. `interrupt` sends the resolved Surface's `interrupt_key` through P5.3 and keeps every current refusal rule.

### P6 — Host state and prompt

1. Capabilities: `native_state` and `submit_confirmation` true; `tile`, `rebalance`, `size`, and `notify` false.
2. Host state comes from the `rex block call -b <block> program_status` record with the newest `updated_at`: `idle`, `working`, `blocked`, and `done` map to themselves; `error` maps to `blocked`; no record maps to `null`. `status --format json` reports `host_word` as that record's `state`, or `null`. Waits follow PRD-261107-0825-01 P6.2.
3. `prompt` sends the text with `rex api call block.event` (event type `paste`, payload `text`), then `rex send-key -b <block> enter`.
4. Delivery (PRD-260918-0323-01 P1) is guaranteed when `modes` reports mode 2004 on before the send. A multi-line prompt to a block without mode 2004 on is refused before any transmission; other prompts to such a block are sent as unavailable delivery.
5. When `api_revision` from `rex server status --json` is below 12, `prompt` exits 1 before any transmission with stderr `rex api revision <n> is older than 12`. Other verbs keep working. A revision above 12 is accepted.
6. `submit` is `confirmed` when the Host state before the send was not `working`, and after the send a `program_status` record has `updated_at` after the send and a Host state (P6.2) of `working`, `done`, or `blocked`; a raw `error` record therefore confirms. Otherwise `submit` is `accepted`. No event stream is read.
7. A `program_status` record with `updated_at` after the send is a Turn observation, also when no record after the send has state `working`.
8. In the settle step of `prompt`, a Host state ends the settle only when its `program_status` record has `updated_at` after the send. An older `idle` or `done` record never ends the settle.

### P7 — notify, self, list, kill, launch

1. `notify` exits 0 and prints `notify: not delivered on rex` on stderr (PRD-261107-0825-01 P7.4).
2. `self --format json` reports `host: "rex"`. `driven` is true when a live record holds `REX_BLOCK`.
3. `list` rows on Rex: `pane` is the block id, `session` is the Rex session id, and `status` is `running`, `dead`, or `unknown` (P4.3); rows gain no `window` key. In a result that already carries a `window` value (`status`, `spawn-agent`, `launch`), it is the Rex window id. Every `--format json` key that the result has on tmux and that has no Rex source is `null` (the PRD-261107-0825-01 P9.13 rule).
4. `kill` resolves the name, applies the self-pane refusal and the Ownership check, runs `rex block close <block>`, and then deletes the record. A `not_found` result counts as success.
5. `launch` runs `(<command>); exec $SHELL` (Keep-alive wrap). `--bare` runs the command with no wrap, so the block closes when the command exits.
6. `claude-teams` cannot set up a Team on host `rex`: it fails open per PRD-261108-0447-01 P2.7 with `<reason>` `no teammate shim on rex`. The tmux, Orca, and Herdr paths do not change.

### P8 — Contract tests

1. The seam is the CLI binary, driven by `tests/integration.rs` (PRD-261107-0825-01 P10.1). No new seam.
2. The default `cargo test --workspace --features test-fixtures` run tests the binary against a fake `rex` (`tests/contract/fake_rex.py`). It covers P2 (including P2.3 flags, P2.4, and P2.6 with a stale `TMUX_PANE`), P3, P4 (including a stale `instance_id` directory, an incomplete listing, and each P4.4 branch, with exit 3 and exit 4 as definite failures), P5.1–P5.4, P6 (including P6.4 refusal, P6.5, and both P6.6 results, a raw `error` record after the send, and a P6.7 Turn observation with no `working` record, and a P6.8 settle that an older `idle` record does not end), P7, the exit-3 cases, pane names of 128 and 129 characters (label and P4.4 match), an ambiguous spawn that is not adopted because another record or reservation shares the first 128 characters of its name, the P5.2 guard with a full scrollback and its last-`rows`-lines result, one server reached with `REX_SERVER` unset and with its explicit address (one directory, no sweep), two distinct live servers whose directories both survive a sweep, a live `unset` directory that survives a sweep run with `REX_SERVER` set, and each P4.1 deletion trigger as its own case: the status call fails, the call times out after 2 seconds, the call reports a different `instance_id`, and the directory has no readable `server-address`. Each P5.2 Completeness condition (full scrollback, width change, alternate screen) is its own case, each with Completeness false and the last-`rows`-lines result, and a case with a height change between the mark and settle that uses the `rows` after settle. A deterministic case interleaves the publication of a new server directory with a sweep from another live server, and checks that the reservation survives and still refuses a duplicate name; another case checks that a stale `.init-` directory older than 120 seconds is deleted and a newer one is kept; another case makes `rex/<instance_id>/` appear after the missing-directory check and before the rename, and checks that the spawn succeeds, its record is in the existing directory, that directory's `server-address` is unchanged, and no `.init-` directory remains. The existing Herdr tests pass unchanged after the P5.2 move, with one exception: `an_unreachable_herdr_names_the_host_and_the_selection_source` in `tests/contract/herdr.rs` uses a value that is not a host as its invalid `TMUX_TOOLS_HOST` and checks that stderr names all four hosts.
3. `TT_TEST_REX=1` runs the full PRD-261107-0825-01 P10 contract suite against a private server (`rex server run --listen unix://<dir>/s.sock --state-dir <dir>/state --restore=false`, reached with `-S unix://<dir>/s.sock`). It checks that a create that times out does not exit 4, so P4.4 treats it as ambiguous. At the end, also on failure, the run stops that server and deletes `<dir>`. It never reaches the default server.
4. `TMUX_TOOLS_LIVE_REX=1` runs from a plain Rex terminal (no `$TMUX`, `REX_BLOCK` set) on the default server. It covers a Claude agent pane with Host state, a key send and a literal send, a guaranteed multi-line prompt to the Claude pane (P6.4), and a `--split v` pane beside the caller.
5. The fake and `TT_TEST_REX=1` runs give every tmux-tools invocation, including those inside created blocks, a temporary `XDG_STATE_HOME`, and delete it at the end. They never read or write the operator's registry.
6. Every block a run creates is closed by that run, also on failure. A run ends by asserting that neither `tmux-tools list` nor the Rex block listing shows a block it created.

### P9 — Rollout and release

1. The release is 0.9.0. README gains a Rex host section (selection, `REX_SERVER` and the `server-address` sweep check, P3 placement and session source, the Pane record, P5.2 scrollback limit, P6.4 delivery rule, P6.5 revision floor, `notify`) and lists `rex` under `TMUX_TOOLS_HOST`.
2. A release requires a passing `TT_TEST_REX=1` run, a passing `TMUX_TOOLS_LIVE_REX=1` run, and the maintainer's confirmation in the Rex app: the `--split v` block shows beside the caller, a no-split block shows as a new tab, focus does not change, and the Claude pane shows its Rex state.
3. Branch `agent/rex-backend` is cut from `agent/host-backends-herdr` at `3db4491` and lands only after the Herdr backend has landed.
4. The tmux, Orca, and Herdr results do not change.

## Codebase fit

- Reuse: the `HostPort` trait and the registry path (`core/src/host/mod.rs`, `core/src/registry.rs`); the Orca Pane record rules and key conversion (`core/src/host/orca/keys.rs`); the Herdr first-changed-line rule (`Mark::first_changed_line` in `core/src/host/herdr.rs`) and `host_word`; `src/cmd/safety.rs` and `names::pane_is_owned`; `tests/contract/fake_hc.py`, `fake_orca.py`, `herdr.rs`, and `orca.rs` as patterns.
- Conventions: `CONTEXT.md`; ADR-260919-0146-01 (surfaces); PRD-261107-0825-01 (Host port); PRD-261109-2223-01 (Herdr backend).
- New homes: `core/src/host/rex.rs`; a shared module for the first-changed-line rule under `core/src/host/`; `tests/contract/fake_rex.py`; `tests/contract/rex.rs`.
- Shared code that changes: `core/src/host/mod.rs` (`Host`, selection, `current`); `core/src/target.rs` (Rex Target resolution); `core/src/registry.rs` (records keyed by host, not only Orca; on Rex the server directory holds a `server-address` file and is published by the P4.1 temporary-directory rename; `reserve` and `write` no longer call `create_dir_all` on Rex; on Rex only, the sweep skips `.`-prefixed entries directly in `rex/` and deletes another server directory only after the P4.1 address check; the Orca sweep keeps PRD-261107-0825-01 P4.3); `src/cmd/claude_teams.rs` (P7.6); `src/cmd/safety.rs`, `src/cmd/launch.rs`, `src/cmd/list.rs`, and `src/cmd/status.rs` where they branch on `Host::Herdr` or `Host::Orca`.
- Planned-record dependencies: the `requires` references in frontmatter; `implementation_after` PRD-261109-2223-01, because the branch builds on unlanded Herdr code.
- Active-plan overlaps: PRD-261109-2223-01 (Herdr) owns the code on the base branch; this PRD lands after it.

## Contracts

### Business entities

- **`Host`** — enum: `Tmux`, `Orca`, `Rex`, `Herdr`; `as_str` gives `tmux`, `orca`, `rex`, `herdr`.
- **`HostCapabilities`** — unchanged fields; Rex values per P6.1.
- **`HostState`** — unchanged.
- **`PaneRecord`** — unchanged fields. On Rex: `host: "rex"`, `server_id` = Rex `instance_id`, `handle` = `block:<uuid>`.

### External contracts

- **`TMUX_TOOLS_HOST`** — `tmux` | `orca` | `rex` | `herdr` (P2.1).
- **Detection order** — P2.2.
- **Environment variables read** — `REX_BLOCK` (calling block), `REX_SESSION` (calling session), `REX_SERVER` (server address for `-S`), and the new `TMUX_TOOLS_REX_SESSION` (session for a new block when no `--target` and no `REX_SESSION`, P3.1).
- **Registry path** — `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/rex/<instance_id>/<name>.json`, plus `rex/<instance_id>/server-address` (P4.1).
- **Created-block properties** — label = first 128 characters of the pane name, `--focus=false`, no `--keep-open` (P3.4).
- **Rex calls used** — `rex server status --json` (`instance_id`, `api_revision`); `rex ls --json`; `rex block ls -s <session> --json`; `rex run` and `rex split` (P3); `rex capture`; `rex block call -b <block> size | modes | program_status`; `rex send`; `rex send-key enter`; `rex api call block.event` with a `paste` event; `rex block close`. Every call carries P2.3 flags. `prompt` needs `api_revision` 12 or higher (P6.5).
- **JSON results on Rex** — `host: "rex"`; `list` rows and `window` per P7.3; keys with no Rex source are `null`; `host_word` per P6.2; `submit` per P6.6.
- **`self --format json`** — `host` adds `"rex"`; `driven` per P7.2.
- **Exit code 3 on Rex** — `--split tile`, `--size`, `--session`, `--window`, `rebalance` (P3.5).
- **stderr texts** — `notify: not delivered on rex` (exit 0); `rex api revision <n> is older than 12` (exit 1); `spawn outcome unknown: <name> (session <session id>)` (exit 1); `claude-teams: no team: no teammate shim on rex; starting claude without teammate panes` (P7.6); the PRD-261107-0825-01 P4 registry texts, with `list incomplete: rex`.
- **Test switches** — `TT_TEST_REX=1` (private server), `TMUX_TOOLS_LIVE_REX=1` (live run).

## Verification and release

1. The default suite passes with the fake-`rex` tests, and the existing tmux, Orca, and Herdr tests pass unchanged, except for the P8.2 invalid-value exception.
2. `TT_TEST_REX=1` passes. Afterwards the private server is stopped, its directory is gone, the default server's `rex ls --json` is unchanged, and the operator's `tmux-tools/rex/` registry is unchanged.
3. `TMUX_TOOLS_LIVE_REX=1` passes in a plain Rex terminal. The maintainer confirms the P9.2 items in the Rex app.
4. After the runs, neither `tmux-tools list` nor the Rex block listing shows a block the runs created.

Rollback is reverting the release: the previous version cannot select `rex`, and records left under `tmux-tools/rex/` are inert.

## Out of Scope

- The `claude-teams` teammate shim on Rex (a later tmux-tools PRD, after this backend).
- Harness `rex` guard and poll rows (repository `claude`).
- Rex event streams, `rex block stream`, `rex resize`, and block identity across a Rex server restart.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | Problem Statement; Verification and release |
| scope boundary | decided | Outcome and scope; Out of Scope |
| domain terms | decided | P1 |
| architecture shape | n/a | implements the PRD-261107-0825-01 Host port; no new boundary |
| stack | n/a | no new stack; `rex` CLI and control API only |
| data/schema | decided | P4.1; Contracts |
| contracts/integrations | decided | P2–P7; Contracts |
| UX (if UI) | decided | P3 (placement, focus); P9.2 |
| testing/seams | decided | P8 |
