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

**Outcome and scope:** Step 3 of P11: under Orca, every required verb works through the Orca backend, and the contract suite runs once per backend. This issue closes `PRD-261107-0825-01`.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P1 (remaining terms and the host-neutral rewrites); P2.2, P2.6 (host commands only inside a backend; public API); P3.1–P3.4 (selection); P4 (names and registry); P5.1 (Orca serves every required verb); P6.1, P6.2, P6.4, P6.5 (Host state, waits, submission); P7.3; P8 (`driven` on Orca); P9 (Orca backend); P10 (contract tests); P11.3, P11.4 (rollout, release); `## Contracts`; `## Verification and release`. `tmux-tools:PRD-260918-0323-01` — P1, P3, P7 (one submission, three-valued Pane state, History mark and Completeness). `tmux-tools:ADR-260919-0146-01` (surfaces).

**Current observable:** Inside an Orca terminal without `$TMUX`, every verb uses tmux and its Managed session, so its panes are not visible in Orca.

**Required change:**
- Implement P3.1–P3.4, P4, P6.2, P6.4 and P9 as written. Homes: `HostSelection` in `core/src/host/mod.rs`; the Orca backend in `core/src/host/orca.rs`; `PaneRecord` and `PaneLiveness` in `core/src/registry.rs`.
- Every `orca` CLI invocation is built inside the Orca backend (P2.2). `tmux-tools-core` exports `HostSelection` and the Orca backend type (P2.6).
- Approved contracts: `PRD-261107-0825-01` `## Contracts` — `Host` gains `Orca`; `HostSelection`, `PaneRecord`, `PaneLiveness`; Environment variable `TMUX_TOOLS_HOST`; Host detection order; Registry file; the Orca values of Changed JSON results.
- Convert `tests/integration.rs` into the P10 contract suite. The live Orca run is opt-in, never runs by default, and covers every P10.3 scenario.
- Add the remaining P1 terms and rewrite the listed `CONTEXT.md` entries. The `Pane liveness` entry states the 120-second spawn timeout (P4.5).
- Release per P11.4: minor version bump; README verb summary, Configuration (`TMUX_TOOLS_HOST`), a host section, and the P9.4 key limitation.

**Acceptance:**
Default run (`cargo test --workspace --features test-fixtures`, no `orca` on `PATH`):
- [ ] The contract suite passes on tmux, and the run passes with no `orca` executable on `PATH`.
- [ ] Every assertion that was in `tests/integration.rs` before this issue still runs on tmux and passes with unchanged expected values (P2.4); only the JSON keys that ISSUE-261107-0921-02 added differ.
- [ ] Given `TMUX_TOOLS_HOST=foo`, any verb exits 1 and stderr names `tmux` and `orca`.
- [ ] Given `$TMUX` and `ORCA_TERMINAL_HANDLE` both set, `self --format json` stdout has `host: "tmux"`. Given `TMUX_TOOLS_HOST=tmux` and only `ORCA_TERMINAL_HANDLE` set, stdout has `host: "tmux"`.
- [ ] Given only `ORCA_TERMINAL_HANDLE` set and no reachable Orca runtime, `list` exits 1, stderr names `orca` and `ORCA_TERMINAL_HANDLE` as the selection source, and no tmux pane or session is created.
- [ ] `HostSelection` and the Orca backend type are public in `cargo doc -p tmux-tools-core`. In `src/cmd/`, `src/main.rs`, `src/util.rs` and `core/src/` outside `core/src/host/orca.rs`, no code starts the `orca` process or builds its argv.

Live run (opt-in switch, from inside an Orca terminal, Orca runtime reachable):
- [ ] The contract suite passes every required verb and every P10.3 scenario, and its final check shows no pane it created in `tmux-tools list` or `orca terminal list`.
- [ ] Given `TMUX_TOOLS_HOST=orca` and `$TMUX` set, `self --format json` stdout has `host: "orca"`. Every `--format json` result under Orca has `host: "orca"`.
- [ ] Given a pane that `spawn-agent <agent> --name <n>` created, `self --format json` run in that pane prints `{"host":"orca","driven":true,"name":"<n>","agent":"<agent>"}`. In an unregistered Orca terminal it prints `driven: false` and `name: null`.
- [ ] Given `ORCA_TERMINAL_HANDLE` unset and `TMUX_TOOLS_HOST=orca`, `self` and `notify` without `--target` exit 1 and stderr names `ORCA_TERMINAL_HANDLE`.
- [ ] Given a Pane record whose handle Orca no longer lists, `list` reports the name as dead and the record file is deleted. A reservation record with no handle, older than the 120-second spawn timeout, is deleted by `list`. A younger one makes `capture --target <name>` exit 1 with `spawn in progress: <name>`, and `list` omits it.
- [ ] Given a name already reserved, `spawn-agent --name <name>` exits 1 and `orca terminal list` shows no new pane.
- [ ] Given an Orca pane with no Pane record, or a record whose `cwd` differs from the current directory, `kill --target <handle or name>` refuses, and the pane stays in `orca terminal list`.
- [ ] `prompt --format json` to a driven agent reports `submit: "confirmed"` when Orca's receipt reports submission or turn start, `"accepted"` for the queued or input-accepted receipt on timeout, and `"assumed"` for provider `old-host` (P9.11).
- [ ] `rebalance` exits 3 with `unsupported: rebalance on orca`; `launch --split tile` exits 3 with `unsupported: --split tile on orca`; `launch --size <n>` exits 3 with `unsupported: --size on orca`; none creates a pane. `notify` exits 0 with `notify: not delivered on orca` on stderr.

**Verification and release:** The default suite passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir>`). The implementing agent runs the live checks from inside an Orca terminal on the agent Mac, before review: the opt-in suite, and items 2–4 of the PRD's `## Verification and release`. The agent reports each check with its commands and result in the review handoff. The issue does not close until all pass. Rollback is reverting the release.

**Out of scope:** Rex and Herdr backends; harness skill, hook, guard and `teammateMode` changes; moving `SilverBond` to the port.

## Triage Notes
- 2026-11-07 dispatch readiness: not promoted; stays `needs-triage`. Orca 1.4.252 evidence answers the four `PRD-261107-0825-01` open questions (`orca-runtime-id`, `orca-paste-determination`, `orca-live-render`, `orca-agent-state`) against the PRD's assumptions; each consequence needs a maintainer choice. Unresolved: Q40 (Pane record identity across an Orca restart), Q41 (truncated `terminal list` in reconciliation), Q42 (pane name to registry file name), Q43 (Host state mapping, freshness, subagents, `--until` precedence), Q44 (pre-send paste determination), Q45 (Completeness on Orca), Q46 (`submit` receipt mapping and `--wait-submit` value), Q47 (Orca addressing and `list` fields), Q48 (P10.3 expected results and the ambiguous-split trigger), Q49 (live-run environment and owner of the GUI checks), Q50 (owner of the README library section).
