---
id: ISSUE-261108-2323-01
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: tmux-tools claude-teams starts Claude with a tmux Shim so that every teammate is a tmux-tools pane with team, parent, and shim_id, on tmux and on Orca by hand
terms: [Host, Host selection, Pane record, Pane registration, Pane liveness, Ownership, Target, Keep-alive wrap, Team, Team record, Leader, Teammate pane, Launcher, Shim, Shim id, Parent, Orphan, Close-on-success wrap]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: outcome
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p1-definitions
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p2-launcher
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p3-team-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p4-team-record
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p5-shim-surface
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p6-teammate-panes
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p7-parent
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p8-output
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p11-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: verification
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
      clause: contracts
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-03
---

## Agent Brief

**Outcome and scope:** Step 1 of `PRD-261108-0447-01` P11: `tmux-tools claude-teams` runs Claude in a Team on tmux and on Orca. Every teammate that Claude starts is a tmux-tools pane with `team`, `parent`, and `shim_id`. `self` and `list` report the new fields and `orphan`. `spawn-agent` and `launch` record `parent`. With no usable host, the verb fails open to in-process teammates. `spawn-agent claude` does not use the Launcher in this issue.

**Required sources:** `tmux-tools:PRD-261108-0447-01` — `outcome`; P1 (terms and `CONTEXT.md` entries); P2 (Launcher); P3 (selection inside a team); P4 (team record and directory); P5 (Shim surface); P6 (Teammate panes, including the Orca form of the P6.3 wrap and the P6.5 fake-`orca` test; not the P6.5 live-run sentence); P7 (`parent`); P8.1–P8.4, and P8.5 except its Orca live check; P10.1–P10.5; P11.1 and P11.4; `## Contracts` (every item except "Pane command of `spawn-agent claude` on Orca"); `## Verification and release` items 1, 3 (tmux, through the P10.2 tests), 5, 6. `tmux-tools:PRD-261107-0825-01` — P1, P2 (Host port and its operations, no host commands outside a backend), P3 (selection that P3 of the shim PRD extends), P4 (names, reservation, registry), P5, P8 (`self`), P9.1, P9.4, P9.9 (Orca calling pane, `send`, `launch --bare` wrap), P10.1, P10.4 (contract suite, no leaked pane), `## Contracts`.

**Current observable:** `tmux-tools claude-teams` is not a verb. A `tmux-tools` binary started as `tmux` behaves as `tmux-tools`. `list --format json` and `self --format json` have no `parent`, `team`, `shim_id`, `orphan`, or `role` keys.

**Required change:**
- First step: move the isolated tmux test harness out of `tests/integration.rs` into a shared test module that `tests/integration.rs` and the new `tests/claude_teams.rs` both use. The suite stays green after this step.
- Implement P1–P8 and P10.1–P10.5 of `PRD-261108-0447-01` as written.
- Capture the first trace fixture `tests/fixtures/claude-teams/2.1.319.jsonl` with `tt-fixture` from a `TT_SHIM_DEBUG=1` `shim.log` of Claude 2.1.319.
- Bump the minor version. Write the README text of P11.4.
- Approved contracts: `PRD-261108-0447-01` `## Contracts` — `TeamRecord`, `TeamLeader`, `ClaudeBinary`, `TmuxEndpoint`, `TeamRole`; changed `PaneRecord` and `SelectionSource`; derived `orphan`; verb `claude-teams`; Child environment set by the Launcher; Environment read by host selection; Environment `TT_SHIM_DEBUG`; Teammate pane environment on Orca; Team directory; Debug directory; Shim CLI; tmux pane options (new); Pane record file; Teammate pane name; Changed JSON; Changed text; Stderr warning; Trace fixture file; Test helper `tt-fixture`.

**Acceptance:**
- [ ] Given a fake `claude` on `PATH` that replays `tests/fixtures/claude-teams/2.1.319.jsonl` inside a Team on the isolated tmux server, when `tmux-tools claude-teams` runs from a pane named `lead`, then the stdout and exit of every Shim call equal the filled fixture line, and `list --format json` shows `lead.tm1` with `team` equal to the team id, `parent: "lead"`, `shim_id: "%2"`, and `orphan: false`.
- [ ] Given that Team, when the fake `claude` exits 0, then `claude-teams` exits 0, no Teammate pane of the Team is live, `list` has no row for them, and the team directory is gone. When it exits 1, then `claude-teams` exits 1, the Teammate panes stay, and after the Launcher ends `list --format json` shows them with `orphan: true`; `kill --target lead.tm1` closes the pane.
- [ ] Inside a Team, `self --format json` in the Leader reports `role: "leader"` and the team id; in a Teammate pane it reports `role: "teammate"`, the team id, and `parent`.
- [ ] `spawn-agent` and `launch` from a tmux-tools pane named `p` record `parent: "p"` in `list --format json`; from a pane that is not a tmux-tools pane, `parent: null`. The `list` text table shows `PARENT` after `AGENT`.
- [ ] Given pane `c` created by `spawn-agent` or `launch` from the tmux-tools pane `p`, after `kill --target p`, `list --format json` shows `c` with `orphan: true` and the text table shows `orphan:p`; after a new pane named `p` is created, `c` stays `orphan: true`.
- [ ] The P10.2 test in a 120×24 window passes: the `split-window` that exits 1 with `tmux: no space for a new pane` creates no pane and no registration, and the respawn of `tm1` exits 0 or exits 1 with the same message while the old pane and its registration stay.
- [ ] The Shim refuses each unsupported command and flag of P5.8 with exit 1 and the P5.8 message, and appends one `refused` line to `shim.log`. With `TT_TEAM` unset, the binary started as `tmux` exits 1 with `tmux: not inside a tmux-tools team`.
- [ ] Given a terminal with no supported host, `claude-teams` prints the P2.7 warning, creates no team directory, starts the fake `claude` with `--teammate-mode in-process`, and exits with its status. With `TT_TEAM` set, it exits 1 with `claude-teams: already inside team <id>`; with no resolvable `claude`, it exits 1 with `claude-teams: claude not found`.
- [ ] Given a fake `claude` that reports a version with no fixture, `claude-teams` prints `claude-teams: claude <version key> not contract-tested` on stderr once and continues.
- [ ] Given a Team on Orca with a fake `orca` CLI on `PATH` that records its calls, when the Shim runs a `split-window` whose argv has `-e ORCA_PANE_KEY=…`, `-e ORCA_TERMINAL_HANDLE=…`, `-e ORCA_TAB_ID=…`, and one unrelated `-e K=V`, then the recorded Orca pane-create command has `K=V` and none of `ORCA_PANE_KEY`, `ORCA_TERMINAL_HANDLE`, and `ORCA_TAB_ID`.
- [ ] `cargo build --release` contains no `tt-fixture` binary.

**Verification and release:** The default `cargo test --workspace --features test-fixtures` run passes; each run ends with no leaked pane (`PRD-261107-0825-01` P10.4). Before review, the implementing agent runs the P10.3 opt-in live test with Claude 2.1.319 on the isolated tmux server (it passes by `PRD-261108-0447-01` P10.3), and, from a Claude session inside an Orca terminal on the agent Mac, runs `claude-teams` by hand in a tmux-tools pane named `lead`. Both runs start Claude with `disableAllHooks: true`; this issue does not wait for the harness `PRD-261108-0235-01` rollout. In the by-hand run: one teammate shows in `orca terminal list` and in `tmux-tools list --format json` as `lead.tm1` with `team` equal to the team id and `parent: "lead"`; a clean exit closes it; after the Launcher is killed, `lead.tm1` shows `orphan: true` and `kill --target lead.tm1` closes it; no pane leaks. Each check is reported with its commands and result in the review handoff. The issue does not close until all pass. Rollback is reverting the release.

**Out of scope:** `spawn-agent claude` through the Launcher on any host (`PRD-261108-0447-01` P9); the Orca contract run additions of P10.6 and the live Orca checks of P6.5 (the P6.5 fake-`orca` test is in scope) and P8.5; harness hook, skill, and `cl` alias changes (repository `claude`); Rex and Herdr; Pane identity across an Orca restart (`ISSUE-261107-1229-01`).

## Triage Notes
- 2026-11-09: `needs-triage → ready-for-agent` (dispatch readiness passed; `orca-close-on-clean-exit` decided in `PRD-261108-0447-01` P6.3).
- 2026-11-09: REOPENED at claim — implementer PREMISE_CHANGED (Claude 2.1.319 startup probes `show`/`show-environment` are refused by P5.8). Lead decision: P10.3 pass rule corrected in `PRD-261108-0447-01`; brief verification line aligned; claim reset `in-progress → ready-for-agent` and re-dispatched.
