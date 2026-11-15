---
id: ISSUE-261114-0707-01
kind: issue
category: enhancement
status: in-review
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-15T07:01:36Z
brief_contract: agent-brief.v3
context: tmux-tools
summary: tmux-tools runs every required verb on Rex through a rex Host backend over the rex CLI, tested against a fake rex and a private Rex server
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Pane record, Pane state, Target, Ownership, Parent, Rex block, History mark, Completeness, Keep-alive wrap, Surface, Readiness, Idle detection, Turn observation]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: implementation_after
      repository: tmux-tools
      record: PRD-261109-2223-01
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p1-definitions
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p2-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p3-placement
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p6-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p9-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: verification
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
---
## Agent Brief

**Outcome and scope:** With host `rex` selected, every required verb of PRD-261107-0825-01 P5.1 works on Rex blocks as PRD-261112-2108-01 P1–P7 state, and the default suite plus the `TT_TEST_REX=1` private-server suite prove it. This issue owns P1–P7, P8.1–P8.3, P8.5, P8.6, the P9.3 cut point, P9.4, and Verification items 1–2. It does not land the branch. The live run, README, version bump, release, and the P9.3 landing order are ISSUE-261114-0707-02.

**Required sources:** `tmux-tools:PRD-261112-2108-01` clauses `p1-definitions` through `p8-tests`, `p9-rollout` (items 3–4 only), `contracts`, and `verification` (items 1–2). These are the rules being implemented. Also the upstream clauses they adopt: `tmux-tools:PRD-261107-0825-01` `p2-host-port`, `p3-selection`, `p4-registration`, `p5-verb-classes`, `p6-host-state`, `p7-notify`, `p8-self`, `p9-orca`, `p10-tests`; `tmux-tools:PRD-261108-0447-01` `p2-launcher`, `p7-parent`; `tmux-tools:PRD-260918-0323-01` `p1-transport`, `p3-state`, `p4-capture`, `p7-prompt`; `tmux-tools:PRD-261109-2223-01` `p5-io` (the first-changed-line rule that moves to a shared module) and `p6-state`.

**Current observable:** `TMUX_TOOLS_HOST=rex tmux-tools list` exits 1 and does not name `rex` as a valid value. A plain Rex terminal (no `$TMUX`, `REX_BLOCK` set) does not select `rex`.

**Required change:**
- First, move the first-changed-line rule out of `core/src/host/herdr.rs` into one shared module under `core/src/host/` that both backends call (P5.2), with the Herdr tests unchanged except for the P8.2 invalid-value exception.
- Then add the `rex` backend and wire it in: selection P2, placement P3, the Rex Pane record with atomic server-directory publication, the address-check sweep, and spawn recovery P4, capture, keys, and send P5, Host state and `prompt` P6, and the other verbs P7, including the `claude-teams` fail-open of P7.6. The tmux, Orca, and Herdr results do not change (P9.4); the Orca sweep keeps PRD-261107-0825-01 P4.3.
- Add `tests/contract/fake_rex.py`, `tests/contract/rex.rs`, and the `TT_TEST_REX=1` suite (P8.2, P8.3, P8.5, P8.6).
- Update `CONTEXT.md` as P1 lists.
- Work on branch `agent/rex-backend`, cut as P9.3 states.
- Approved contracts: PRD-261112-2108-01 `## Contracts`, all items except the `TMUX_TOOLS_LIVE_REX=1` test switch (ISSUE-261114-0707-02).

**Acceptance:**
- [ ] Given a fake `rex` and `REX_BLOCK` set with no `$TMUX` and no `ORCA_TERMINAL_HANDLE`, when `tmux-tools self --format json` runs, then it reports `host: "rex"`; given `TMUX_TOOLS_HOST=bogus`, the invocation exits 1 and names `tmux`, `orca`, `rex`, and `herdr`. Observable at stdout, stderr, and the exit status.
- [ ] Given a fake `rex`, `REX_SERVER` set, `REX_SESSION` set, and a calling block, when `launch --split v --name N` runs with a 129-character `N`, then the fake `rex` receives one `split --split=below -b <calling block>` with `--focus=false`, `--label` equal to the first 128 characters of `N`, `--cwd`, `--json`, `--autostart=false`, and `-S <REX_SERVER>`, and no `--keep-open`. Observable at the fake `rex` call log.
- [ ] Given a fake block `block:<uuid>` with no registration and no registered name equal to that value, when `capture --target block:<uuid>` runs, then the fake `rex` captures that block; given a registered name `N` whose record holds block B, `capture --target N` makes the fake `rex` capture B. Observable at the CLI output and the fake `rex` call log.
- [ ] Given a fake `rex`, when `rebalance`, `--split tile`, `--size`, `--session`, or `--window` is used, then the verb exits 3 with the PRD-261107-0825-01 P5.3 message and the fake `rex` receives no create. Observable at the exit status, stderr, and call log.
- [ ] Given a create that times out and one block in the target session with the matching label, when the spawn runs, then the record gains that block id; when another reservation in the same server directory has a name with the same first 128 characters, the spawn instead exits 1 with `spawn outcome unknown: <name> (session <session id>)` and creation is not repeated; when the create exits 3 or 4, the reservation is deleted. Observable at the exit status, stderr, the call log, and the registry directory.
- [ ] Given one fake server reached once with `REX_SERVER` unset and once with its explicit address, when both invocations read the registry, then one `rex/<instance_id>/` directory holds both records and neither is deleted; given a second directory whose stored address fails, times out after 2 seconds, or reports a different `instance_id`, or that has no readable `server-address`, then that directory is deleted; given a second live server, its directory survives. Observable at the registry directory.
- [ ] Given a new server directory being published while a sweep from another live server runs, when both finish, then the reservation survives and a second spawn of the same name is refused. Observable at the registry directory and the spawn's exit status.
- [ ] Given a fake block whose scrollback is full (or whose width changes, or whose alternate screen is on) between the History mark and settle, when `prompt --format json` runs, then the result is the last `rows` lines of the capture after settle, with `rows` from the `size` after settle, and Completeness is false. Observable at `prompt --format json`.
- [ ] Given a fake block whose Host state before the send is not `working` and that gets a `program_status` record after the send with state `done` (and, in another case, raw `error`), when `prompt` runs, then `submit` is `confirmed`; given only an older `idle` record, the settle does not end on it and `submit` is `accepted`. Observable at `prompt --format json`.
- [ ] Given `api_revision` 11, when `prompt` runs, then it exits 1 with `rex api revision 11 is older than 12` and the fake `rex` receives no paste or send; given a multi-line prompt to a block without mode 2004 on, it is refused before any transmission. Observable at stderr, the exit status, and the call log.
- [ ] Given host `rex` and `REX_BLOCK` set, when `notify` runs, then it exits 0 with `notify: not delivered on rex` on stderr; when `claude-teams` runs with a fake `claude`, then stderr carries `claude-teams: no team: no teammate shim on rex; starting claude without teammate panes`. Observable at stderr and the exit status.
- [ ] Given the tmux, Orca, and Herdr contract tests, when the default suite runs, then they pass unchanged, except that `an_unreachable_herdr_names_the_host_and_the_selection_source` uses a value that is not a host as its invalid `TMUX_TOOLS_HOST` and checks that stderr names `tmux`, `orca`, `rex`, and `herdr` (P8.2). Observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given `TT_TEST_REX=1` with a real `rex` on `PATH`, when the contract suite runs, then every required verb passes against a private server, a create that times out does not exit 4, and afterwards the private server is stopped, its directory is gone, the default server's `rex ls --json` is unchanged, the operator's `tmux-tools/rex/` registry is unchanged, and no created block remains. Observable at the test run and those checks.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. `TT_TEST_REX=1 cargo test --workspace --features test-fixtures` passes on the agent Mac. No release from this issue.

**Out of scope:** the `TMUX_TOOLS_LIVE_REX=1` live run, README, version bump, and install (ISSUE-261114-0707-02); the Rex teammate shim; harness `rex` guard and poll rows; Rex event streams, `rex block stream`, `rex resize`, and block identity across a Rex server restart.

## Context Pack — canonical at claim (2026-11-15T07:01:36Z)

**Contract approval:** `review-20261115T051749Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261114-0707-01` · revision `18caa59d00e71b43515de9e9d1be1eeb46b2fad2` · sha256 `28716f954677320c2447ce5a0fa397316f92f7a7590b31b672a95a8f81dff70f`

**Required source objects:**
- `tmux-tools:PRD-260918-0323-01#p1-transport` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18` · `objects/8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18.md`
- `tmux-tools:PRD-260918-0323-01#p3-state` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435` · `objects/5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435.md`
- `tmux-tools:PRD-260918-0323-01#p4-capture` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648` · `objects/40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648.md`
- `tmux-tools:PRD-260918-0323-01#p7-prompt` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `05941c8a9b107818f53b60318fcb6990b213c10e6b0f3470e28eaf1feefa458c` · `objects/05941c8a9b107818f53b60318fcb6990b213c10e6b0f3470e28eaf1feefa458c.md`
- `tmux-tools:PRD-261107-0825-01#p10-tests` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb` · `objects/a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb.md`
- `tmux-tools:PRD-261107-0825-01#p2-host-port` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271` · `objects/57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271.md`
- `tmux-tools:PRD-261107-0825-01#p3-selection` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef` · `objects/f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef.md`
- `tmux-tools:PRD-261107-0825-01#p4-registration` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `4f3186b44eac6fb0216ee87b1f469f422617cadb577ee7033ab286156b50986c` · `objects/4f3186b44eac6fb0216ee87b1f469f422617cadb577ee7033ab286156b50986c.md`
- `tmux-tools:PRD-261107-0825-01#p5-verb-classes` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `126f8593490e6c1e977cd250f022086d9b8e12236740621981f2238d89d8acbe` · `objects/126f8593490e6c1e977cd250f022086d9b8e12236740621981f2238d89d8acbe.md`
- `tmux-tools:PRD-261107-0825-01#p6-host-state` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `8116a17457d9e0d4d77810f04fb8d25e9ceb7d9648425937e1a10758ecba2389` · `objects/8116a17457d9e0d4d77810f04fb8d25e9ceb7d9648425937e1a10758ecba2389.md`
- `tmux-tools:PRD-261107-0825-01#p7-notify` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `314e45ba7b329a7205cffe4ca13f1ef0295742983604de237a67bb2c2cddf9f4` · `objects/314e45ba7b329a7205cffe4ca13f1ef0295742983604de237a67bb2c2cddf9f4.md`
- `tmux-tools:PRD-261107-0825-01#p8-self` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `c9b5c2c4a75da7c6af673a9c3e73e9295a84bae0f5e841abd0790f711207e329` · `objects/c9b5c2c4a75da7c6af673a9c3e73e9295a84bae0f5e841abd0790f711207e329.md`
- `tmux-tools:PRD-261107-0825-01#p9-orca` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `689b0faaccf9198710903fe9aac136378099c40b1f0ce0727a655ee9b31c773c` · `objects/689b0faaccf9198710903fe9aac136378099c40b1f0ce0727a655ee9b31c773c.md`
- `tmux-tools:PRD-261108-0447-01#p2-launcher` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c` · `objects/6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c.md`
- `tmux-tools:PRD-261108-0447-01#p7-parent` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2` · `objects/95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2.md`
- `tmux-tools:PRD-261109-2223-01#p5-io` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `16c605a4a5c69459aa491661c76fc4bea358946393d5482093ab186588d012d3` · `objects/16c605a4a5c69459aa491661c76fc4bea358946393d5482093ab186588d012d3.md`
- `tmux-tools:PRD-261109-2223-01#p6-state` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `0cdfb0022698c1c2b41e63870910987d21cea4f2a1f935f1c9a617ab97cf430d` · `objects/0cdfb0022698c1c2b41e63870910987d21cea4f2a1f935f1c9a617ab97cf430d.md`
- `tmux-tools:PRD-261112-2108-01#contracts` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `11c1bde745db73543c46e8c39386237b4d3a237006ae47b1908f506ccfeea039` · `objects/11c1bde745db73543c46e8c39386237b4d3a237006ae47b1908f506ccfeea039.md`
- `tmux-tools:PRD-261112-2108-01#p1-definitions` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `af58d8456463e362e2306e67ed47ea46fc06ca21e0c9229b66747358bca77c25` · `objects/af58d8456463e362e2306e67ed47ea46fc06ca21e0c9229b66747358bca77c25.md`
- `tmux-tools:PRD-261112-2108-01#p2-selection` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `2a349aa253b443e118dbe430906dcf4689a80b2da6781053765291de7c965bad` · `objects/2a349aa253b443e118dbe430906dcf4689a80b2da6781053765291de7c965bad.md`
- `tmux-tools:PRD-261112-2108-01#p3-placement` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `2762ef21e9d3d5948ecf560a5c8a2ddf5c111ff562da601f2d2985703bcbbc7c` · `objects/2762ef21e9d3d5948ecf560a5c8a2ddf5c111ff562da601f2d2985703bcbbc7c.md`
- `tmux-tools:PRD-261112-2108-01#p4-registration` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `21b2c9c7820f2ddbdde9b6c386bff5e4b594d4f7e94d5794a33efc95594733d2` · `objects/21b2c9c7820f2ddbdde9b6c386bff5e4b594d4f7e94d5794a33efc95594733d2.md`
- `tmux-tools:PRD-261112-2108-01#p5-io` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `2b189784c6126af38379e9469ac9f448d2bbceb04d32645f0ca4c5c0243b3b1a` · `objects/2b189784c6126af38379e9469ac9f448d2bbceb04d32645f0ca4c5c0243b3b1a.md`
- `tmux-tools:PRD-261112-2108-01#p6-state` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `7960ff054e8830a43ee5e12594d7ca8a8bc4f079cc003f6287d7d15a1da696bc` · `objects/7960ff054e8830a43ee5e12594d7ca8a8bc4f079cc003f6287d7d15a1da696bc.md`
- `tmux-tools:PRD-261112-2108-01#p7-other-verbs` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `5652f6b9f74d50f277f5f91367903f1ac1cbe24df00cdbf6701de9a522f5897f` · `objects/5652f6b9f74d50f277f5f91367903f1ac1cbe24df00cdbf6701de9a522f5897f.md`
- `tmux-tools:PRD-261112-2108-01#p8-tests` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `f7102c0030dc591e4e076dd53421ebefc580298b4f3d957455311ea547996684` · `objects/f7102c0030dc591e4e076dd53421ebefc580298b4f3d957455311ea547996684.md`
- `tmux-tools:PRD-261112-2108-01#p9-rollout` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `4d0234f606cd1cfe25dce65d9dc2a00eec246b0831608dc0192d8fac8d24e147` · `objects/4d0234f606cd1cfe25dce65d9dc2a00eec246b0831608dc0192d8fac8d24e147.md`
- `tmux-tools:PRD-261112-2108-01#verification` · revision `e218fd4a9c1150c4488fd30731e617f14e57ea86` · sha256 `0c28ec664ce373454aaea445c69984a4f1ede7b79a3f46312bba799ea27a7ccb` · `objects/0c28ec664ce373454aaea445c69984a4f1ede7b79a3f46312bba799ea27a7ccb.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261114-0707-01/manifest.json`

**Test seam:** the `tmux-tools` CLI (stdout, stderr, exit status, `prompt --format json`) with the fake `rex` call log and the registry directory, via `cargo test --workspace --features test-fixtures` (`tests/contract/rex.rs`, `tests/contract/fake_rex.py`) and the `TT_TEST_REX=1` private-server suite

## Triage Notes
