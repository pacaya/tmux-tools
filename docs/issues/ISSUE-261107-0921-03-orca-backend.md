---
id: ISSUE-261107-0921-03
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: an Orca host backend serves every required verb, selected by TMUX_TOOLS_HOST or ORCA_TERMINAL_HANDLE, with a Pane record registry and a per-backend contract suite
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane record, Pane liveness, Pane registration, Ownership, Target, History mark, Completeness, Turn observation, Keep-alive wrap, Surface, Readiness, Idle detection]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p1-definitions
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
      clause: p9-orca
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p11-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: verification
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
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-02
---

## Agent Brief

**Outcome and scope:** Step 3 of P11: under Orca, every required verb works through the Orca backend, and the contract suite runs once per backend. On every host, `--name` is checked against the P4.1 character set. This issue closes `PRD-261107-0825-01`.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P1 (remaining terms and the host-neutral rewrites); P2.2, P2.6 (host commands only inside a backend; public API); P3.1–P3.4 (selection); P4 (names, name check on every host, registry, restart limit, liveness); P5.1, P5.3 (Orca serves every required verb; exit 3); P6.1, P6.2, P6.4, P6.5 (Host state, waits and their option precedence, submission); P7.3; P8 (`driven` on Orca); P9 (Orca backend); P10 (contract tests); P11.3, P11.4 (rollout, release); `## Contracts`; `## Verification and release`. `tmux-tools:PRD-260918-0323-01` — P1, P3, P7 (one submission, three-valued Pane state, History mark and Completeness). `tmux-tools:ADR-260919-0146-01` (surfaces).

**Current observable:** Inside an Orca terminal without `$TMUX`, every verb uses tmux and its Managed session, so its panes are not visible in Orca. `launch --name 'a/b'` is accepted on tmux.

**Required change:**
- Implement P3.1–P3.4, P4, P6.2, P6.4 and P9 as written. Homes: `HostSelection` in `core/src/host/mod.rs`; the Orca backend in `core/src/host/orca.rs`; `PaneRecord` and `PaneLiveness` in `core/src/registry.rs`.
- The P4.1 name check applies to `spawn-agent` and `launch` on every host, tmux included.
- Every `orca` CLI invocation is built inside the Orca backend (P2.2). `tmux-tools-core` exports `HostSelection` and the Orca backend type (P2.6).
- Approved contracts: `PRD-261107-0825-01` `## Contracts` — `Host` gains `Orca`; `HostSelection`, `PaneRecord`, `PaneLiveness`; Environment variable `TMUX_TOOLS_HOST`; Host detection order; Registry file; the Orca values of Changed JSON results; Exit code 3 for `--session` and `--window` on Orca.
- Convert `tests/integration.rs` into the P10 contract suite. The live Orca run is opt-in and never runs by default. The ambiguous split of P10.3 is caused by a test shim around the real `orca` that turns the first `terminal split` into a timeout error that carries a `leafId`. A receipt or list that a live Orca does not produce on demand is caused by the same shim.
- Add the remaining P1 terms and rewrite the listed `CONTEXT.md` entries. The `Pane liveness` entry states the 120-second spawn timeout (P4.5). The `Target` entry adds the `term_…` handle on Orca (P9.1).
- Release per P11.4: minor version bump; README verb summary, Configuration (`TMUX_TOOLS_HOST`), a host section with the limits of P4.3 (registration lost on an Orca restart), P9.3 (every Orca `prompt` result is `complete: false`), P9.4 (cursor and keypad keys) and P9.6 (the bracketing proxy and its risk), and `## Library: configurable invocation` rewritten for the P2.6 API.

**Acceptance:**
Default run (`cargo test --workspace --features test-fixtures`, no `orca` on `PATH`):
- [ ] The contract suite passes on tmux, and the run passes with no `orca` executable on `PATH`.
- [ ] Every child process of the default suite runs without `TMUX_TOOLS_HOST`, `ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID`, and with no real `orca` on its `PATH`, unless the test sets them. Run from inside an Orca terminal with `orca` on the caller's `PATH` and with the decoy environment below, the default suite passes and `orca terminal list` shows no new pane.
- [ ] Every assertion that was in `tests/integration.rs` before this issue still runs on tmux and passes with unchanged expected values (P2.4); only the JSON keys that ISSUE-261107-0921-02 added differ.
- [ ] On tmux, `launch --name <v>` and `spawn-agent <agent> --name <v>` with each of `a/b`, `..`, `.`, `a b` and the empty string exit 1, stderr is `invalid name: <v>`, and no pane is created. `--name A.b_c-9` is accepted.
- [ ] Given `TMUX_TOOLS_HOST=foo`, any verb exits 1 and stderr names `tmux` and `orca`.
- [ ] Given `$TMUX` and `ORCA_TERMINAL_HANDLE` both set, `self --format json` stdout has `host: "tmux"`. Given `TMUX_TOOLS_HOST=tmux` and only `ORCA_TERMINAL_HANDLE` set, stdout has `host: "tmux"`.
- [ ] Given only `ORCA_TERMINAL_HANDLE` set and no reachable Orca runtime, `list` exits 1, stderr names `orca` and `ORCA_TERMINAL_HANDLE` as the selection source, and no tmux pane or session is created.
- [ ] `HostSelection` and the Orca backend type are public in `cargo doc -p tmux-tools-core`. In `src/cmd/`, `src/main.rs`, `src/util.rs` and `core/src/` outside `core/src/host/orca.rs`, no code starts the `orca` process or builds its argv.

Live run (opt-in switch, from inside an Orca terminal, Orca runtime reachable):
- [ ] The contract suite passes every required verb, and its final check shows no pane it created in `tmux-tools list` or `orca terminal list`.
- [ ] P10.3 scenarios: the shimmed ambiguous `spawn-agent --split h` ends with exactly one new pane in `orca terminal list`, recovered through `--retry-split <leafId>`, and its record holds that pane's handle. A `prompt` whose response spans several read pages returns the whole response text. To a pane that echoes its input bytes, one `send` per argument, in this order, `M-x`, `F1`, `Up`, `y`, `^c`, `<word>`, writes `ESC x`, `ESC O P`, `ESC [ A`, `y`, `0x03` and the word's bytes (P9.4 tmux grammar); `send --literal M-x` writes the three bytes `M-x`. `interrupt` to a pane whose Surface `interrupt_key` is not `C-c` writes that key's P9.4 bytes, and no `orca` call carries `--interrupt`. A refused multi-line `prompt` (P9.6) exits 1 and `orca terminal read` shows no new output in that pane. A `prompt` to a pane that switches to full-screen rendering during the turn reports `complete: false`, and its Pane record is unchanged.
- [ ] Given `TMUX_TOOLS_HOST=orca` and `$TMUX` set, `self --format json` stdout has `host: "orca"`. Every `--format json` result under Orca has `host: "orca"`.
- [ ] Given a pane that `spawn-agent <agent> --name <n>` created, `self --format json` run in that pane prints `{"host":"orca","driven":true,"name":"<n>","agent":"<agent>"}`. In an unregistered Orca terminal it prints `driven: false` and `name: null`.
- [ ] Given `ORCA_TERMINAL_HANDLE` unset and `TMUX_TOOLS_HOST=orca`, `self`, `notify` and `capture` without `--target` exit 1 and stderr names `ORCA_TERMINAL_HANDLE`. With it set, `capture` without `--target` reads the calling pane, and `capture --target <term_… handle>` reads that pane.
- [ ] `capture --session x` exits 3 with `unsupported: --session on orca`, and `capture --window x` exits 3 with `unsupported: --window on orca`.
- [ ] On Orca, `launch --name 'a/b'` exits 1 with `invalid name: a/b`, and `orca terminal list` shows no new pane.
- [ ] Given a directory under `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/orca/` whose name is not the current `runtimeId`, `list` deletes it.
- [ ] Given a Pane record whose handle Orca no longer lists on a complete list, `list --format json` has one row for that name with `status: "dead"`, the record file is deleted, and the next `list` has no row for it. A reservation record with no handle, older than the 120-second spawn timeout, is deleted by `list`. A younger one makes `capture --target <name>` exit 1 with `spawn in progress: <name>`, and `list` omits it.
- [ ] Given a shimmed `terminal list` with `truncated: true` that does not list a record's handle, `capture --target <name>` exits 1 with `pane list incomplete: <name>`, and the record file stays.
- [ ] `list --format json` rows of live records have `session` equal to the pane's `worktreeId`, `pane` equal to its handle, and `status: "running"`.
- [ ] Given a name already reserved, `spawn-agent --name <name>` exits 1 and `orca terminal list` shows no new pane.
- [ ] Given an Orca pane with no Pane record, or a record whose `cwd` differs from the current directory, `kill --target <handle or name>` refuses, and the pane stays in `orca terminal list`. After a successful `kill --target <name>`, the record file is gone and the next `list` has no row for the name.
- [ ] For a driven agent, `status --format json` `host_state` follows the P9.10 mapping of that pane's `orca worktree ps --json` row; it is never `idle`.
- [ ] With a shimmed `orca worktree ps --json` and a static pane screen, and neither `--until` nor `--idle-seconds` given: while the pane's row is `working`, or its main state is `done` with a subagent `working`, neither `wait-idle` nor the `prompt` settle ends before its timeout; a `done` or `blocked` row ends `wait-idle`; a `done` or `blocked` row ends the `prompt` settle only when its `stateStartedAt` is after the submission, and a `done` row whose `stateStartedAt` is before the submission does not end it.
- [ ] `prompt --format json` to a driven agent reports `submit: "confirmed"` when the receipt's stages include `turn_started`. Shimmed receipts give: only `input_accepted` → `"accepted"`; provider `unsupported` → `"assumed"`; error `agent_prompt_blocked`, observation `permission`, or `accepted: false` → exit 1 with the reason on stderr and a single `terminal send`; observation `incarnation_replaced` → exit 1 with `prompt target restarted`. `prompt --timeout 0.2` passes `--wait-submit 1`, and `prompt --timeout 5000` passes `--wait-submit 3600`.
- [ ] Every `prompt --format json` result on Orca has `complete: false`. `capture --all` on Orca prints no full-screen notice.
- [ ] Given a driven agent whose Host state is `done`, `wait-idle --until <regex that never matches> --timeout 3` does not end before its timeout.
- [ ] `rebalance` exits 3 with `unsupported: rebalance on orca`; `launch --split tile` exits 3 with `unsupported: --split tile on orca`; `launch --size <n>` exits 3 with `unsupported: --size on orca`; none creates a pane. `notify` exits 0 with `notify: not delivered on orca` on stderr.

**Verification and release:** The default suite passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir> TMUX_TOOLS_HOST=orca ORCA_TERMINAL_HANDLE=term_decoy ORCA_WORKTREE_ID=decoy`). The implementing agent runs the opt-in live suite and items 2 and 4 of the PRD's `## Verification and release` from a Claude session started inside an Orca terminal on the agent Mac, before review. Conditions: this repository is registered as an Orca worktree; the Orca version and its `orca status` capabilities are named in the handoff; `claude` and `codex` are authenticated. The Host-state part of item 3 is checked with `orca worktree ps --json`. The maintainer does only the Orca GUI and desktop-notification checks of item 3. If no such session is available, the maintainer runs the live suite and items 2–4 before release. Each check is reported with its commands and result in the review handoff. The issue does not close until all pass. Rollback is reverting the release.

**Out of scope:** Rex and Herdr backends; harness skill, hook, guard and `teammateMode` changes; moving `SilverBond` to the port; Pane identity across an Orca restart (`ISSUE-261107-1229-01`).

## Triage Notes
- 2026-11-07 dispatch readiness: not promoted; stays `needs-triage` (Q40–Q50).
- 2026-11-07 Q40–Q47 and Q50 settled in `PRD-261107-0825-01` P4.1, P4.3, P4.5, P6.2, P9.1, P9.3, P9.6, P9.7, P9.10–P9.12, P11.4 and `## Contracts`; Q48 and Q49 settled in this brief.
- 2026-11-07 dispatch readiness after Q40–Q50: not promoted; stays `needs-triage`. Unresolved: Q52 (`list` and name resolution on an incomplete Orca pane list, `PRD-261107-0825-01` `orca-list-incomplete`), Q53 (Orca values of JSON fields with no Orca source, `orca-json-fields`), Q54 (`prompt` on an Orca runtime without `terminal.prompt-delivery.v1`, `orca-no-prompt-delivery`).
