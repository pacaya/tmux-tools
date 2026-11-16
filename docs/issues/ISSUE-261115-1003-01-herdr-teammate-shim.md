---
id: ISSUE-261115-1003-01
kind: issue
category: enhancement
status: in-review
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-16T02:00:54Z
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

## Context Pack — canonical at claim (2026-11-16T02:00:54Z)

**Contract approval:** `dispatch-20261116T015757Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261115-1003-01` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `93b9e1eef9b70ba8a8427898438de80aaa39c4470efbcd9f841981931976bc81`

**Required source objects:**
- `tmux-tools:PRD-261107-0825-01#p2-host-port` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271` · `objects/57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271.md`
- `tmux-tools:PRD-261107-0825-01#p3-selection` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef` · `objects/f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef.md`
- `tmux-tools:PRD-261108-0447-01#contracts` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `241363a4efd4189d9f149e8f5d0f3844771647554109b145a8554dc26e37d12c` · `objects/241363a4efd4189d9f149e8f5d0f3844771647554109b145a8554dc26e37d12c.md`
- `tmux-tools:PRD-261108-0447-01#p10-tests` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `bf1a91dd7f52ca8cfaaa10a9bc020b976fc3e8a6a1377b98fdd740051bba0cb1` · `objects/bf1a91dd7f52ca8cfaaa10a9bc020b976fc3e8a6a1377b98fdd740051bba0cb1.md`
- `tmux-tools:PRD-261108-0447-01#p2-launcher` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c` · `objects/6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c.md`
- `tmux-tools:PRD-261108-0447-01#p3-team-selection` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `d014d6fde9e61030997482ab48b0deec9e261e4822e7b4fd745f8eaa07247f97` · `objects/d014d6fde9e61030997482ab48b0deec9e261e4822e7b4fd745f8eaa07247f97.md`
- `tmux-tools:PRD-261108-0447-01#p4-team-record` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `67a1722e1af96f495903579c2bddc3ebd1d39dda881480ba81cb2ac0d0150fbc` · `objects/67a1722e1af96f495903579c2bddc3ebd1d39dda881480ba81cb2ac0d0150fbc.md`
- `tmux-tools:PRD-261108-0447-01#p5-shim-surface` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `71e1d71379efedb8f9fd374f425f4ea48ece67dd2052d19bbd38815039eeef9f` · `objects/71e1d71379efedb8f9fd374f425f4ea48ece67dd2052d19bbd38815039eeef9f.md`
- `tmux-tools:PRD-261108-0447-01#p6-teammate-panes` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `e18f30de1e63f34d7f6e73184ec994ffe4ff17441958f2df0fa8e0aa46abbe75` · `objects/e18f30de1e63f34d7f6e73184ec994ffe4ff17441958f2df0fa8e0aa46abbe75.md`
- `tmux-tools:PRD-261108-0447-01#p7-parent` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2` · `objects/95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2.md`
- `tmux-tools:PRD-261108-0447-01#p8-output` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `02f668ee1a94c80c0a7b4728ebffec6a91308e700cdb1e50fe0857d4397eb97c` · `objects/02f668ee1a94c80c0a7b4728ebffec6a91308e700cdb1e50fe0857d4397eb97c.md`
- `tmux-tools:PRD-261108-0447-01#p9-spawn-agent` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `491e059f74bb88b21be83ba2fcb07852f44d8dbb401da1357567f7e582075fc4` · `objects/491e059f74bb88b21be83ba2fcb07852f44d8dbb401da1357567f7e582075fc4.md`
- `tmux-tools:PRD-261109-2223-01#contracts` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `862f8b8d12a263aa45cfb4acbd16a809fc7a0ffcd6ba687aa0b8f476022e600d` · `objects/862f8b8d12a263aa45cfb4acbd16a809fc7a0ffcd6ba687aa0b8f476022e600d.md`
- `tmux-tools:PRD-261109-2223-01#p3-placement` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `868fbda3bb4ebe26d1f55aee59c79ba2b8a397aed235e49ec1d02fce70fb4aaa` · `objects/868fbda3bb4ebe26d1f55aee59c79ba2b8a397aed235e49ec1d02fce70fb4aaa.md`
- `tmux-tools:PRD-261109-2223-01#p4-registration` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `8af2f4d5f27c4abf5b4df4eeb5aa40249be20b0e527896fda049aea174e5eb9f` · `objects/8af2f4d5f27c4abf5b4df4eeb5aa40249be20b0e527896fda049aea174e5eb9f.md`
- `tmux-tools:PRD-261109-2223-01#p7-other-verbs` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `94d294169cd23f8d55ba41342e7a351c8b1ddba4a90bbc30fd24e0f9cb050358` · `objects/94d294169cd23f8d55ba41342e7a351c8b1ddba4a90bbc30fd24e0f9cb050358.md`
- `tmux-tools:PRD-261109-2223-01#p8-tests` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `a2fcb876a7cae3bf27e8286858a8f3b6ce8a387ef5d9662e1b99198dc12a3526` · `objects/a2fcb876a7cae3bf27e8286858a8f3b6ce8a387ef5d9662e1b99198dc12a3526.md`
- `tmux-tools:PRD-261115-0921-01#contracts` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `73f0786f91ef8d2078dce172d5235d595d74b5e080f51dc6d47d7a6e8542dad9` · `objects/73f0786f91ef8d2078dce172d5235d595d74b5e080f51dc6d47d7a6e8542dad9.md`
- `tmux-tools:PRD-261115-0921-01#outcome` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `6fe95f6d4c18978dc734768ddc514d2851046fa8cde73a9f03f06f95e6926e28` · `objects/6fe95f6d4c18978dc734768ddc514d2851046fa8cde73a9f03f06f95e6926e28.md`
- `tmux-tools:PRD-261115-0921-01#p1-slices` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `5a7b4432da65e97c8a33493180f7ffa876069c19daa659c68e7b5a1601486225` · `objects/5a7b4432da65e97c8a33493180f7ffa876069c19daa659c68e7b5a1601486225.md`
- `tmux-tools:PRD-261115-0921-01#p2-launcher` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `3e58be8e36f459c9ffc11f31ff24a8d0507693d963351666ef088927056dd2af` · `objects/3e58be8e36f459c9ffc11f31ff24a8d0507693d963351666ef088927056dd2af.md`
- `tmux-tools:PRD-261115-0921-01#p3-create` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `5c8749c2c946c8844e8342e726ba74e7bc54dc693b3b944b48da1192ad44a5c7` · `objects/5c8749c2c946c8844e8342e726ba74e7bc54dc693b3b944b48da1192ad44a5c7.md`
- `tmux-tools:PRD-261115-0921-01#p4-environment` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `f75590351dda19fdaecf28b1d89f4bcc077f3c06e00242603ee3966874709d8e` · `objects/f75590351dda19fdaecf28b1d89f4bcc077f3c06e00242603ee3966874709d8e.md`
- `tmux-tools:PRD-261115-0921-01#p5-herdr-respawn` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `e1c23ec267585ae4fa3b5e401058397fc71e518234f995cee8d5e279f24a134e` · `objects/e1c23ec267585ae4fa3b5e401058397fc71e518234f995cee8d5e279f24a134e.md`
- `tmux-tools:PRD-261115-0921-01#p6-herdr-close` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `685a4f7993051fdb0615214c4f2a6b3ff714a7064617d5670e6c9db8100f5d1e` · `objects/685a4f7993051fdb0615214c4f2a6b3ff714a7064617d5670e6c9db8100f5d1e.md`
- `tmux-tools:PRD-261115-0921-01#p7-tests` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `516f87c3723b21c7859a034142a0af2f9de3683ee7da61bce2ebb8f3a17ce54a` · `objects/516f87c3723b21c7859a034142a0af2f9de3683ee7da61bce2ebb8f3a17ce54a.md`
- `tmux-tools:PRD-261115-0921-01#p8-live` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `05020efdbae2fb97f228cf6b244b32cc47593a2068424489dca02f8bf3041440` · `objects/05020efdbae2fb97f228cf6b244b32cc47593a2068424489dca02f8bf3041440.md`
- `tmux-tools:PRD-261115-0921-01#p9-rollout` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `666a8206ceb513d32252c233c89deadb0d1b0a6f31bd6e274a1cf1fa45478770` · `objects/666a8206ceb513d32252c233c89deadb0d1b0a6f31bd6e274a1cf1fa45478770.md`
- `tmux-tools:PRD-261115-0921-01#verification` · revision `9a24d0f9f8bcd94a56a51ebb3301c84bc207838b` · sha256 `6f3d71dbccddba9bbb477cbfd05f5d51213aa9e31f8d87638e67b749d229fc46` · `objects/6f3d71dbccddba9bbb477cbfd05f5d51213aa9e31f8d87638e67b749d229fc46.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261115-1003-01/manifest.json`

**Test seam:** `tmux-tools list --format json`, the fake `hc` call log, the Shim's stdout, and the CLI's stdout/stderr (Agent Brief Acceptance)

## Code Review

Review: `issue-261115-1003-01-code-review-20261115-111609.md` — Outcome: ACCEPTED

Targeted review of the P3.2 repair episode (diff vs `f8214a0`): Outcome: ACCEPTED — no findings.

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
