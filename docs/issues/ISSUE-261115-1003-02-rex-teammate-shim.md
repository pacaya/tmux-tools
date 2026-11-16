---
id: ISSUE-261115-1003-02
kind: issue
category: enhancement
status: in-review
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-16T06:51:03Z
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

**Required sources:** PRD-261115-0921-01 P1–P4, P7–P9, Contracts, Verification (the Rex parts); PRD-261108-0447-01 P1–P10 and Contracts (the shim behavior that applies on `rex`: Launcher, team selection, Team record, shim surface, Teammate panes, parent, output, spawn-agent, fixtures); PRD-261112-2108-01 P1, P3, P4, P7, P8, Contracts (Rex create, registration, the clause this slice updates, and the `TT_TEST_REX=1` suite).

**Current observable:** `claude-teams` in a Rex block prints `claude-teams: no team: no teammate shim on rex; starting claude without teammate panes` and starts Claude with `--teammate-mode in-process`.

**Required change:**
- P2 on Rex: `TT_HOST` accepts `rex` in addition to the values already accepted on the branch at release, and the invalid-value error names every value accepted then (`tmux`, `orca`, `rex`, and `herdr` only when the Herdr slice has landed); Leader handle from `REX_BLOCK`; fail-open reason `REX_BLOCK is unset`; `server_id` = Rex `instance_id`; remove `no teammate shim on rex`.
- P3.1, P3.3: Rex placement — first teammate right of the Leader, later ones below the highest-numbered live teammate. Rex create errors keep PRD-261112-2108-01 P4.4 (P3.4).
- P4 on Rex: drop `-e` `REX_BLOCK`, `REX_SESSION`, `REX_SERVER`.
- Update PRD-261112-2108-01 P7.6 and the matching `## Contracts` entry to point to PRD-261115-0921-01.
- Approved contracts (PRD-261115-0921-01 `## Contracts`): `TeamRecord` (host `rex`, `server_id`), `TeamLeader.handle`, `TT_HOST`, fail-open reasons, teammate pane environment, Rex Teammate block placement.

**Acceptance:**
- [ ] Given a Leader block registered as `lead` and started by `claude-teams` on the private Rex server, when Claude's replayed fixture runs `split-window` twice, then teammates `lead.tm1` (right of the Leader) and `lead.tm2` (below `lead.tm1`) exist with `team` = the Team id, `parent` = `lead`, and `shim_id`, observable at `tmux-tools list --format json` and the private server's layout (`rex session inspect`).
- [ ] Given `split-window -e REX_BLOCK=x -e REX_SESSION=y -e REX_SERVER=z -e K=V`, then in the teammate's environment `K=V` is present, `REX_BLOCK`, `REX_SESSION`, `REX_SERVER` do not have the values `x`, `y`, `z`, and `REX_BLOCK` is the teammate's own block id, not the Leader's, observable at the teammate's printed environment.
- [ ] Given `REX_BLOCK` unset with `TMUX_TOOLS_HOST=rex`, when `claude-teams` runs, then stderr has `claude-teams: no team: REX_BLOCK is unset; …` and Claude gets `--teammate-mode in-process`.
- [ ] Given `tmux-tools claude-teams` run in a plain Rex block, then stderr has no `claude-teams: no team` line and a teammate the Leader starts shows in `tmux-tools list --format json` with `team`.
- [ ] PRD-261115-0921-01 P8 items 1, 2, 4, 5, 7 pass in a new Rex tab.

**Verification and release:** PRD-261115-0921-01 P7.2 with `TT_TEST_REX=1` and the default suite, both with `TT_TEAM` and `TT_HOST` unset; clippy and fmt clean; tmux, Orca, and Herdr suites unchanged. P8 live run in a Rex tab the run creates; its per-item result and the recorded Claude runtime identity go into the implementation receipt. Only after the P7.2 suite and those P8 items pass: merge `origin/development`, take the next minor version above it (PRD-261115-0921-01 P9.1), update the README Rex text, and install the release binary.

**Out of scope:** Herdr; harness rule, guard, and setup-guide rows; geometry-based Rex placement; a `no space` Shim error on Rex.

## Context Pack — canonical at claim (2026-11-16T06:51:03Z)

**Contract approval:** `dispatch-20261116T064948Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261115-1003-02` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `67568005c981c62a235599a0054ee0184436e58484a5e52aba26d6293eeecc3a` · `objects/67568005c981c62a235599a0054ee0184436e58484a5e52aba26d6293eeecc3a.md`

**Required source objects:**
- `tmux-tools:PRD-261108-0447-01#contracts` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `241363a4efd4189d9f149e8f5d0f3844771647554109b145a8554dc26e37d12c` · `objects/241363a4efd4189d9f149e8f5d0f3844771647554109b145a8554dc26e37d12c.md`
- `tmux-tools:PRD-261108-0447-01#p1-definitions` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `5f2f67ebe183937a3d6c0bb54cd0f1a00a7614e80a493f5ce5ae71987a0c5ca6` · `objects/5f2f67ebe183937a3d6c0bb54cd0f1a00a7614e80a493f5ce5ae71987a0c5ca6.md`
- `tmux-tools:PRD-261108-0447-01#p10-tests` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `bf1a91dd7f52ca8cfaaa10a9bc020b976fc3e8a6a1377b98fdd740051bba0cb1` · `objects/bf1a91dd7f52ca8cfaaa10a9bc020b976fc3e8a6a1377b98fdd740051bba0cb1.md`
- `tmux-tools:PRD-261108-0447-01#p2-launcher` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c` · `objects/6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c.md`
- `tmux-tools:PRD-261108-0447-01#p3-team-selection` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `d014d6fde9e61030997482ab48b0deec9e261e4822e7b4fd745f8eaa07247f97` · `objects/d014d6fde9e61030997482ab48b0deec9e261e4822e7b4fd745f8eaa07247f97.md`
- `tmux-tools:PRD-261108-0447-01#p4-team-record` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `67a1722e1af96f495903579c2bddc3ebd1d39dda881480ba81cb2ac0d0150fbc` · `objects/67a1722e1af96f495903579c2bddc3ebd1d39dda881480ba81cb2ac0d0150fbc.md`
- `tmux-tools:PRD-261108-0447-01#p5-shim-surface` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `71e1d71379efedb8f9fd374f425f4ea48ece67dd2052d19bbd38815039eeef9f` · `objects/71e1d71379efedb8f9fd374f425f4ea48ece67dd2052d19bbd38815039eeef9f.md`
- `tmux-tools:PRD-261108-0447-01#p6-teammate-panes` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `e18f30de1e63f34d7f6e73184ec994ffe4ff17441958f2df0fa8e0aa46abbe75` · `objects/e18f30de1e63f34d7f6e73184ec994ffe4ff17441958f2df0fa8e0aa46abbe75.md`
- `tmux-tools:PRD-261108-0447-01#p7-parent` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2` · `objects/95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2.md`
- `tmux-tools:PRD-261108-0447-01#p8-output` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `02f668ee1a94c80c0a7b4728ebffec6a91308e700cdb1e50fe0857d4397eb97c` · `objects/02f668ee1a94c80c0a7b4728ebffec6a91308e700cdb1e50fe0857d4397eb97c.md`
- `tmux-tools:PRD-261108-0447-01#p9-spawn-agent` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `491e059f74bb88b21be83ba2fcb07852f44d8dbb401da1357567f7e582075fc4` · `objects/491e059f74bb88b21be83ba2fcb07852f44d8dbb401da1357567f7e582075fc4.md`
- `tmux-tools:PRD-261112-2108-01#contracts` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `11c1bde745db73543c46e8c39386237b4d3a237006ae47b1908f506ccfeea039` · `objects/11c1bde745db73543c46e8c39386237b4d3a237006ae47b1908f506ccfeea039.md`
- `tmux-tools:PRD-261112-2108-01#p1-definitions` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `af58d8456463e362e2306e67ed47ea46fc06ca21e0c9229b66747358bca77c25` · `objects/af58d8456463e362e2306e67ed47ea46fc06ca21e0c9229b66747358bca77c25.md`
- `tmux-tools:PRD-261112-2108-01#p3-placement` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `2762ef21e9d3d5948ecf560a5c8a2ddf5c111ff562da601f2d2985703bcbbc7c` · `objects/2762ef21e9d3d5948ecf560a5c8a2ddf5c111ff562da601f2d2985703bcbbc7c.md`
- `tmux-tools:PRD-261112-2108-01#p4-registration` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `21b2c9c7820f2ddbdde9b6c386bff5e4b594d4f7e94d5794a33efc95594733d2` · `objects/21b2c9c7820f2ddbdde9b6c386bff5e4b594d4f7e94d5794a33efc95594733d2.md`
- `tmux-tools:PRD-261112-2108-01#p7-other-verbs` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `5652f6b9f74d50f277f5f91367903f1ac1cbe24df00cdbf6701de9a522f5897f` · `objects/5652f6b9f74d50f277f5f91367903f1ac1cbe24df00cdbf6701de9a522f5897f.md`
- `tmux-tools:PRD-261112-2108-01#p8-tests` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `f7102c0030dc591e4e076dd53421ebefc580298b4f3d957455311ea547996684` · `objects/f7102c0030dc591e4e076dd53421ebefc580298b4f3d957455311ea547996684.md`
- `tmux-tools:PRD-261115-0921-01#contracts` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `0deb6e7f41b4ffa5fdfa7173b776e0d14fa7a80ee87d676f0fee978c6afe25a0` · `objects/0deb6e7f41b4ffa5fdfa7173b776e0d14fa7a80ee87d676f0fee978c6afe25a0.md`
- `tmux-tools:PRD-261115-0921-01#outcome` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `6fe95f6d4c18978dc734768ddc514d2851046fa8cde73a9f03f06f95e6926e28` · `objects/6fe95f6d4c18978dc734768ddc514d2851046fa8cde73a9f03f06f95e6926e28.md`
- `tmux-tools:PRD-261115-0921-01#p1-slices` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `5a7b4432da65e97c8a33493180f7ffa876069c19daa659c68e7b5a1601486225` · `objects/5a7b4432da65e97c8a33493180f7ffa876069c19daa659c68e7b5a1601486225.md`
- `tmux-tools:PRD-261115-0921-01#p2-launcher` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `3e58be8e36f459c9ffc11f31ff24a8d0507693d963351666ef088927056dd2af` · `objects/3e58be8e36f459c9ffc11f31ff24a8d0507693d963351666ef088927056dd2af.md`
- `tmux-tools:PRD-261115-0921-01#p3-create` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `f1220e4eb6ebfe5ec182fa8c1b0c36b0028dd4499e0e1bb6ea84cc219952b10b` · `objects/f1220e4eb6ebfe5ec182fa8c1b0c36b0028dd4499e0e1bb6ea84cc219952b10b.md`
- `tmux-tools:PRD-261115-0921-01#p4-environment` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `f75590351dda19fdaecf28b1d89f4bcc077f3c06e00242603ee3966874709d8e` · `objects/f75590351dda19fdaecf28b1d89f4bcc077f3c06e00242603ee3966874709d8e.md`
- `tmux-tools:PRD-261115-0921-01#p7-tests` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `e41f8983210af082c0a16eb020b058722d7b05cd3f48a5b1c10f41a81d0be441` · `objects/e41f8983210af082c0a16eb020b058722d7b05cd3f48a5b1c10f41a81d0be441.md`
- `tmux-tools:PRD-261115-0921-01#p8-live` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `c334dce6077ac67e2f397257789873fe6b3feb1dbacdb1483a2825380e2fe9e8` · `objects/c334dce6077ac67e2f397257789873fe6b3feb1dbacdb1483a2825380e2fe9e8.md`
- `tmux-tools:PRD-261115-0921-01#p9-rollout` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `666a8206ceb513d32252c233c89deadb0d1b0a6f31bd6e274a1cf1fa45478770` · `objects/666a8206ceb513d32252c233c89deadb0d1b0a6f31bd6e274a1cf1fa45478770.md`
- `tmux-tools:PRD-261115-0921-01#verification` · revision `791699ca9bc8142f7097380c498bab0a86e74004` · sha256 `6f3d71dbccddba9bbb477cbfd05f5d51213aa9e31f8d87638e67b749d229fc46` · `objects/6f3d71dbccddba9bbb477cbfd05f5d51213aa9e31f8d87638e67b749d229fc46.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261115-1003-02/manifest.json`

**Test seam:** `tmux-tools list --format json`, the private Rex server layout (`rex session inspect`), the teammate's printed environment, and `claude-teams` stderr, in the `TT_TEST_REX=1` suite

## Triage Notes

- 2026-11-16: `rex-no-space` settled by the maintainer at triage — P3.4 no-space mapping, its P7.2 test, P8.6, and the Shim error contract removed; PRD-261112-2108-01 P4.4 unchanged.
