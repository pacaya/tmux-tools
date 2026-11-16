---
id: ISSUE-261115-1003-02
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: claude-teams on Rex sets up a Team whose teammates are Rex blocks right of the Leader and below the newest teammate; live-checked in a Rex tab and released as the next minor version
terms: [Team, Leader, Teammate pane, Launcher, Shim, Orphan, Rex block]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261114-0707-02
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: outcome
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p1-slices
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p2-launcher
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p3-create
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p4-environment
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p7-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p8-live
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p9-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: verification
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: contracts
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
      clause: p9-spawn-agent
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p1-definitions
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p3-placement
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p8-tests
---

## Agent Brief

**Outcome and scope:** The Rex slice of PRD-261115-0921-01 (P1.1): every clause of that PRD for host `rex`. `claude-teams` (and `spawn-agent claude`) in a Rex block creates a Team, and each teammate is a Rex block. Lands on its own branch, cut from `development` after ISSUE-261114-0707-02 has landed. No tmux, Orca, or Herdr behavior changes.

**Required sources:** PRD-261115-0921-01 P1–P4, P7–P9, Contracts, Verification (the Rex parts) and Open Question `rex-no-space`; PRD-261108-0447-01 P1–P10 and Contracts (the shim behavior that applies on `rex`: Launcher, team selection, Team record, shim surface, Teammate panes, parent, output, spawn-agent, fixtures); PRD-261112-2108-01 P1, P3, P4, P7, P8, Contracts (Rex create, registration, the clause this slice updates, and the `TT_TEST_REX=1` suite).

**Current observable:** `claude-teams` in a Rex block prints `claude-teams: no team: no teammate shim on rex; starting claude without teammate panes` and starts Claude with `--teammate-mode in-process`.

**Required change:**
- P2 on Rex: `TT_HOST` accepts `rex` in addition to the values already accepted on the branch at release, and the invalid-value error names every value accepted then (`tmux`, `orca`, `rex`, and `herdr` only when the Herdr slice has landed); Leader handle from `REX_BLOCK`; fail-open reason `REX_BLOCK is unset`; `server_id` = Rex `instance_id`; remove `no teammate shim on rex`.
- P3.1, P3.3: Rex placement — first teammate right of the Leader, later ones below the highest-numbered live teammate. P3.4, its P7.2 no-space test, and P8.6 exactly as settled at triage (see Triage Notes).
- P4 on Rex: drop `-e` `REX_BLOCK`, `REX_SESSION`, `REX_SERVER`.
- Update PRD-261112-2108-01 P7.6 and the matching `## Contracts` entry to point to PRD-261115-0921-01.
- Approved contracts (PRD-261115-0921-01 `## Contracts`): `TeamRecord` (host `rex`, `server_id`), `TeamLeader.handle`, `TT_HOST`, fail-open reasons, teammate pane environment, Rex Teammate block placement, Shim error on Rex (as settled at triage).

**Acceptance:**
- [ ] Given a Leader block registered as `lead` and started by `claude-teams` on the private Rex server, when Claude's replayed fixture runs `split-window` twice, then teammates `lead.tm1` (right of the Leader) and `lead.tm2` (below `lead.tm1`) exist with `team` = the Team id, `parent` = `lead`, and `shim_id`, observable at `tmux-tools list --format json` and the private server's layout (`rex session inspect`).
- [ ] Given `split-window -e REX_BLOCK=x -e REX_SESSION=y -e REX_SERVER=z -e K=V`, then in the teammate's environment `K=V` is present, `REX_BLOCK`, `REX_SESSION`, `REX_SERVER` do not have the values `x`, `y`, `z`, and `REX_BLOCK` is the teammate's own block id, not the Leader's, observable at the teammate's printed environment.
- [ ] Given `REX_BLOCK` unset with `TMUX_TOOLS_HOST=rex`, when `claude-teams` runs, then stderr has `claude-teams: no team: REX_BLOCK is unset; …` and Claude gets `--teammate-mode in-process`.
- [ ] Given `tmux-tools claude-teams` run in a plain Rex block, then stderr has no `claude-teams: no team` line and a teammate the Leader starts shows in `tmux-tools list --format json` with `team`.
- [ ] When P3.4 is kept: given the fake `rex` answers a split with the refusal recorded in PRD-261115-0921-01 P3.4, when the fixture runs `split-window`, then the Shim exits 1 with `tmux: no space for a new pane`, no registration exists, `k` is not used, and the next `split-window` succeeds with that `k`, observable at the Shim's exit and stderr and at `tmux-tools list --format json`.
- [ ] PRD-261115-0921-01 P8 items 1, 2, 4, 5, 7 (and 6 when P3.4 is kept) pass in a new Rex tab.

**Verification and release:** PRD-261115-0921-01 P7.2 with `TT_TEST_REX=1` and the default suite, both with `TT_TEAM` and `TT_HOST` unset; clippy and fmt clean; tmux, Orca, and Herdr suites unchanged. P8 live run in a Rex tab the run creates; its per-item result and the recorded Claude runtime identity go into the implementation receipt. Only after the P7.2 suite and those P8 items pass: merge `origin/development`, take the next minor version above it (PRD-261115-0921-01 P9.1), update the README Rex text, and install the release binary.

**Out of scope:** Herdr; harness rule, guard, and setup-guide rows; geometry-based Rex placement.

## Triage Notes

- Blocker for `ready-for-agent`: `rex-no-space` (PRD-261115-0921-01 Open Questions).
