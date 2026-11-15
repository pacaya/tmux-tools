---
id: ISSUE-261115-1003-01
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: claude-teams on Herdr sets up a Team whose teammates are Herdr Teammate panes in the Leader's tab; live-checked in the maintainer's Herdr window and released as the next minor version
terms: [Team, Leader, Teammate pane, Launcher, Shim, Orphan, Close-on-success wrap, hc terminal, hc name]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
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
      clause: p5-herdr-respawn
    - relation: requires
      repository: tmux-tools
      record: PRD-261115-0921-01
      clause: p6-herdr-close
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
      record: PRD-261109-2223-01
      clause: p3-placement
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: contracts
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
      record: PRD-261109-2223-01
      clause: p8-tests
---

## Agent Brief

**Outcome and scope:** The Herdr slice of PRD-261115-0921-01 (P1.1): every clause of that PRD for host `herdr`. `claude-teams` (and `spawn-agent claude`) in a Herdr terminal creates a Team, and each teammate Claude starts is a Teammate pane in the Leader's tab. Lands on branch `agent/teammate-shim-herdr`. No tmux, Orca, or Rex behavior changes.

**Required sources:** PRD-261115-0921-01 P1–P9, Contracts, Verification (the Herdr parts); PRD-261108-0447-01 P2–P10 and Contracts (the shim behavior that applies on `herdr`); PRD-261107-0825-01 P2, P3 (Host port, host selection); PRD-261109-2223-01 P3, P4, P7, P8, Contracts (Herdr create, labels, naming, the fake-`hc` suite, and the clauses this slice updates).

**Current observable:** `claude-teams` in a Herdr terminal prints `claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes` and starts Claude with `--teammate-mode in-process`.

**Required change:**
- P2 on Herdr: `TT_HOST` accepts `tmux`, `orca`, `herdr` (`rex` stays invalid until the Rex slice), and the invalid-value error names those three; Leader handle from `HC_TERMINAL_ID`; fail-open reason `HC_TERMINAL_ID is unset`; `server_id` = Herdr core id; remove `no teammate shim on herdr`.
- P3.1, P3.2, P3.5: registered create on Herdr (never `create_split`), the split target's `herdr.space`/`herdr.tab` labels, and Herdr history capture through `All`/`Visible`/`Tail`.
- P4 on Herdr: drop `-e` `HC_TERMINAL_ID` and `HC_SCOPE`.
- P5: respawn under the transitional name, then write the name (`tt.name` and `where`).
- P6: `--rm` for Teammate terminals only.
- Update PRD-261109-2223-01 P3.4, P4.2, P7.7 and the matching `## Contracts` entries to name these exceptions and point to PRD-261115-0921-01.
- Approved contracts (PRD-261115-0921-01 `## Contracts`): `TeamRecord` (host `herdr`, `server_id`), `TeamLeader.handle`, `TT_HOST`, fail-open reasons, teammate pane environment, Herdr Teammate terminal, Herdr respawn name.

**Acceptance:**
- [ ] Given a Leader terminal registered as `lead` and started by `claude-teams` (fake `hc`), when Claude's replayed fixture runs `split-window`, then a teammate `lead.tm1` with `team` = the Team id, `parent` = `lead`, and `shim_id` exists in the Leader's tab, observable at `tmux-tools list --format json` and the fake `hc` call log.
- [ ] Given that Team, when the fixture runs `capture-pane -p -S -10` on the teammate, then the output holds history lines plus the visible screen, observable at the Shim's stdout.
- [ ] Given a teammate, when the fixture runs `respawn-pane -k`, then the new terminal carries `tt.name` and `where` = the old name, the old terminal is gone, and a following create of that name fails as taken and the Shim advances `k`, observable at `tmux-tools list --format json` and the Shim's output.
- [ ] Given `split-window -e HC_TERMINAL_ID=x -e HC_SCOPE=y -e K=V`, then the teammate's command sets `K=V` and sets neither `HC_TERMINAL_ID` nor `HC_SCOPE`, observable at the fake `hc` call log; in the P8 live run the teammate's `HC_TERMINAL_ID` is its own, not the Leader's.
- [ ] Given a teammate whose command exits 0, then it was started with `--rm` and is gone from `hc ls --all`; given exit 1, it stays with a shell, observable at the fake `hc`.
- [ ] Given `TT_TEAM` set and `TT_HOST=bogus`, when any verb runs, then it exits 1 and stderr names `tmux`, `orca`, and `herdr` as the valid values, observable at the CLI.
- [ ] Given `HC_TERMINAL_ID` unset with `TMUX_TOOLS_HOST=herdr`, when `claude-teams` runs, then stderr has `claude-teams: no team: HC_TERMINAL_ID is unset; …` and Claude gets `--teammate-mode in-process`.
- [ ] Given `tmux-tools claude-teams` run in a Herdr terminal, then stderr has no `claude-teams: no team` line and a teammate the Leader starts shows in `tmux-tools list --format json` with `team`.
- [ ] PRD-261115-0921-01 P8 items 1–5 and 7 pass in the maintainer's Herdr window, and the maintainer confirms P8.3.

**Verification and release:** PRD-261115-0921-01 P7.1 suite in the default `cargo test --workspace --features test-fixtures` run with `TT_TEAM` and `TT_HOST` unset; clippy and fmt clean; tmux and Orca suites unchanged. The P8 live run needs the maintainer's Herdr window with the core started from the GUI session; the run stops for the maintainer's P8.3 visual check. The per-item P8 result, the recorded Claude runtime identity, and the maintainer's P8.3 confirmation go into the implementation receipt. Only after the P7.1 suite, P8 items 1–5 and 7, and the P8.3 confirmation pass: merge `origin/development`, take the next minor version above it (PRD-261115-0921-01 P9.1), update the README Herdr text, and install the release binary.

**Out of scope:** Rex (PRD-261115-0921-01 Rex slice); harness rule, guard, and setup-guide rows; a warning about the Herdr core's session; Herdr split-direction control.
