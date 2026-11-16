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

**Required sources:** PRD-261115-0921-01 P1–P9, Contracts, Verification (the Herdr parts); PRD-261108-0447-01 P2–P10 and Contracts (the shim behavior that applies on `herdr`); PRD-261107-0825-01 P2, P3 (Host port, host selection); PRD-261109-2223-01 P3, P4, P7, P8, Contracts (Herdr create, labels including the P3.2 made-up `herdr.tab`, naming, the fake-`hc` suite, and the clauses this slice updates).

**Current observable:** `claude-teams` in a Herdr terminal prints `claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes` and starts Claude with `--teammate-mode in-process`. A Herdr create with no `--split`, or with `--split window`, sets only `herdr.space`, so a teammate of a Leader that tmux-tools opened in a new tab opens in its own tab.

**Required change:**
- P2 on Herdr: `TT_HOST` accepts `tmux`, `orca`, `herdr` (`rex` stays invalid until the Rex slice), and the invalid-value error names those three; Leader handle from `HC_TERMINAL_ID`; fail-open reason `HC_TERMINAL_ID is unset`; `server_id` = Herdr core id; remove `no teammate shim on herdr`.
- P3.1, P3.2, P3.5: registered create on Herdr (never `create_split`), the split target's `herdr.space`/`herdr.tab` labels, and Herdr history capture through `All`/`Visible`/`Tail`.
- PRD-261109-2223-01 P3.2 and its Contracts entry "hc terminal fields written by this tool" (already updated in the PRD): a Herdr create with no `--split`, or with `--split window`, sets `herdr.space` and a `herdr.tab` with a new tab id that tmux-tools makes up, so teammates join a Leader that tmux-tools opened in a new tab; that terminal's made-up `herdr.tab` is `window_id` in its `launch` and `spawn-agent` JSON and `window` in its `status` JSON (PRD-261109-2223-01 P7.3). The README Herdr placement text states this. The `TT_TEST_HERDR=1` and `TMUX_TOOLS_LIVE_HERDR=1` suites (`tests/contract/herdr.rs`) assert the new P3.2: a terminal created without `--split` has a made-up `herdr.tab` that differs from the caller's tab.
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
- [ ] Given a calling Herdr terminal with `herdr.space` = S (fake `hc`), when `launch` runs once with no `--split` and once with `--split window`, then each new terminal carries `herdr.space` = S and a `herdr.tab` that no other terminal carries, and a following `launch --split v --target <first terminal>` creates a terminal with that same `herdr.tab`, observable at the fake `hc` call log; each of the first two `launch --format json` results reports `window_id` equal to its terminal's made-up `herdr.tab`, observable at the CLI's stdout.
- [ ] Given a teammate whose command exits 0, then it was started with `--rm` and is gone from `hc ls --all`; given exit 1, it stays with a shell, observable at the fake `hc`.
- [ ] Given `TT_TEAM` set and `TT_HOST=bogus`, when any verb runs, then it exits 1 and stderr names `tmux`, `orca`, and `herdr` as the valid values, observable at the CLI.
- [ ] Given `HC_TERMINAL_ID` unset with `TMUX_TOOLS_HOST=herdr`, when `claude-teams` runs, then stderr has `claude-teams: no team: HC_TERMINAL_ID is unset; …` and Claude gets `--teammate-mode in-process`.
- [ ] Given `tmux-tools claude-teams` run in a Herdr terminal, then stderr has no `claude-teams: no team` line and a teammate the Leader starts shows in `tmux-tools list --format json` with `team`.
- [ ] PRD-261115-0921-01 P8 items 1–5 and 7 pass in the maintainer's Herdr window, with the Leader started by `spawn-agent claude --name lead` with no `--split` (so it opens in a new tab with a made-up tab id), and the maintainer confirms P8.3: the teammate shows in the Leader's tab.

**Verification and release:** PRD-261115-0921-01 P7.1 suite in the default `cargo test --workspace --features test-fixtures` run with `TT_TEAM` and `TT_HOST` unset; a passing `TT_TEST_HERDR=1` run (PRD-261109-2223-01 P8.3); clippy and fmt clean; tmux and Orca suites unchanged. The P8 live run needs the maintainer's Herdr window with the core started from the GUI session; the run stops for the maintainer's P8.3 visual check. If Herdr does not put the teammate in the Leader's made-up tab, stop with `BLOCKED:` and report; any fallback needs a new maintainer confirmation. The per-item P8 result, the recorded Claude runtime identity, and the maintainer's P8.3 confirmation go into the implementation receipt. Only after the P7.1 suite, P8 items 1–5 and 7, and the P8.3 confirmation pass: merge `origin/development`, take the next minor version above it (PRD-261115-0921-01 P9.1), update the README Herdr text, and install the release binary.

**Out of scope:** Rex (PRD-261115-0921-01 Rex slice); harness rule, guard, and setup-guide rows; a warning about the Herdr core's session; Herdr split-direction control.

## Repair Notes

- kind: repairable
- Prior claim: `implement-issue@agentmac`, claimed 2026-11-15T10:32:10Z; released by triage after the session shut down.
- Maintainer decision 2026-11-15 (Option A, approved change to PRD-261109-2223-01 P3.2): a Herdr create with no `--split`, or with `--split window`, also sets a `herdr.tab` label with an id that tmux-tools makes up. PRD P3.2 and the Contracts entry are updated (`41e0c45`); the brief carries the change.
- Restore: `git cherry-pick --no-commit f8214a000916c26dc2adcfcec3e198945418b232` (local branch `wip/ISSUE-261115-1003-01-20261116T014902Z`, based on `41e0c45`). Same code diff as lane snapshot `agent/wip/teammate-shim-herdr` `e5f32e0` (based on `32b5511`).
- Review file: `issue-261115-1003-01-code-review-20261115-111609.md`; review accepted, M1–M3 fixed and verified.
- Remaining required outcome: implement the P3.2 no-split made-up `herdr.tab` in code, fake-`hc` contract tests (the acceptance item for it), and the `TT_TEST_HERDR=1` / `TMUX_TOOLS_LIVE_HERDR=1` suite assertions, with a passing `TT_TEST_HERDR=1` run; targeted review of that change; then rerun the whole live P8, including P8.3 and P8.5, and the release steps.
- Resume phase: implementation of the P3.2 change (targeted repair episode), then targeted review, then live P8.
- Counts: review repair rounds 1 (M1–M3 in one consolidated repair, all verified); no-progress 0.

## Triage Notes

- 2026-11-16: claim released (session shut down); brief reminted for maintainer decision Option A on PRD-261109-2223-01 P3.2; mint and dispatch readiness passed; prior approval `review-20261115T105821Z` superseded by a fresh dispatch.
