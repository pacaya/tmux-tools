---
id: ISSUE-261109-2336-01
kind: issue
category: enhancement
status: done
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-10T01:51:23Z
brief_contract: agent-brief.v3
context: tmux-tools
summary: tmux-tools runs every required verb on Herdr through a herdr Host backend over the hc CLI, tested against a fake hc and a private hc core
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Target, Ownership, Parent, Keep-alive wrap, History mark, Completeness, hc terminal, hc name]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p1-definitions
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p2-selection
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
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p6-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p9-rollout
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
---
## Agent Brief

**Outcome and scope:** With host `herdr` selected, every required verb of PRD-261107-0825-01 P5.1 works on hc terminals as PRD-261109-2223-01 P1–P7 state, and the default suite plus the `TT_TEST_HERDR=1` private-core suite prove it. This issue owns P1–P7, P8.1–P8.3, P8.5, and P9.3. The live run, README, version bump, and release are ISSUE-261109-2336-02.

**Required sources:** `tmux-tools:PRD-261109-2223-01` clauses `p1-definitions` through `p8-tests`, `p9-rollout` (item 3 only), and `contracts`. These are the rules being implemented. Also the upstream clauses they adopt: `tmux-tools:PRD-261107-0825-01` `p2-host-port`, `p3-selection`, `p4-registration`, `p5-verb-classes`, `p6-host-state`, `p7-notify`, `p8-self`, `p9-orca`, `p10-tests`; `tmux-tools:PRD-261108-0447-01` `p2-launcher`, `p7-parent`; `tmux-tools:PRD-260918-0323-01` `p1-transport`, `p3-state`, `p4-capture`, `p7-prompt`.

**Current observable:** `TMUX_TOOLS_HOST=herdr tmux-tools list` exits 1 and names only `tmux` and `orca` as valid values. A plain Herdr terminal selects `tmux`.

**Required change:**
- First, change the shared code without changing tmux or Orca behaviour:
  - calling-pane lookups go through the Host port (P2.6);
  - creation dispatch no longer depends on Pane records (P3.4);
  - `status --format json` gains `host_word` on every host (P6.3).
- Then add the `herdr` backend and wire it in: selection P2, placement P3, label registration and dead-name removal P4, keys and capture P5, Host state and prompt P6, and the other verbs P7, including the `claude-teams` fail-open of P7.7.
- Add `tests/contract/fake_hc.py` and the `TT_TEST_HERDR=1` suite (P8.2, P8.3, P8.5).
- Update `CONTEXT.md` as P1 lists.
- Approved contracts: PRD-261109-2223-01 `## Contracts`, all items except `TMUX_TOOLS_LIVE_HERDR=1` (ISSUE-261109-2336-02).

**Acceptance:**
- [ ] Given a fake `hc` and `HC_TERMINAL_ID` set with no `$TMUX`, when `tmux-tools self --format json` runs, then it reports `host: "herdr"`, observable at the CLI's stdout.
- [ ] Given a fake `hc` and a calling terminal with `herdr.space=S` and `herdr.tab=T`, when `launch --split v --name A.b` runs, then the fake `hc` receives one `run` with `--outlive-parent --no-notify`, `--name tt-<16 hex of SHA-256("A.b")>`, the labels `herdr.space=S`, `herdr.tab=T`, `where=A.b`, and `tt.name=A.b`, and no `--rm`. With no `--split`, the `run` has `herdr.space=S` and no `herdr.tab`. Observable at the fake `hc` call log.
- [ ] Given a fake `hc`, when `rebalance`, `--split tile`, `--size`, `--session`, or `--window` is used, then the verb exits 3 with the P5.3 message and the fake `hc` receives no `run`, observable at the exit status, stderr, and call log.
- [ ] Given a finished terminal that carries `tt.name=X`, when `launch --bare --name X` runs, then the old terminal is removed by address and the spawn succeeds; when `kill --target X` runs instead, it exits 1 and never acts on another terminal. Observable at the exit status and the fake `hc` call log.
- [ ] Given a fake `hc` whose terminal reports `working` before and after a send with an unchanged `turn`, when `prompt` runs, then `submit` is `accepted`, not `confirmed`, observable at `prompt --format json`.
- [ ] Given a multi-line prompt on Herdr to a pane that P6.6, as recorded at readiness, refuses, when `prompt` runs, then it exits 1 and the fake `hc` receives no `send`, observable at the exit status and call log.
- [ ] Given a Claude pane in a private core, when `prompt --format json` and `capture --all` run, then Completeness, the `capture --all` read, and the no-scrollback notice match P5.1 as recorded at readiness, observable at the CLI output.
- [ ] Given `send` with an argument containing a character outside the typeable set, when it runs, then it exits 1 with `cannot type on herdr: U+<XXXX>` and the fake `hc` receives no `keys`, observable at stderr and the call log.
- [ ] Given a fake `hc`, `HC_TERMINAL_ID` set, and a fake `claude`, when `claude-teams` runs with host `herdr` selected, then its stderr carries `claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes`, the fake `claude` starts with `--teammate-mode in-process`, and no team directory exists, observable at stderr, the fake `claude` argv, and the state directory. When `spawn-agent claude` runs on Herdr, the fake `hc` receives one `run` whose command is that `claude-teams` Launcher command, observable at the call log.
- [ ] Given the tmux and Orca contract tests, when the default suite runs, then they pass, and their only output change is the added `host_word: null` in `status`, observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given `TT_TEST_HERDR=1` with a real `hc` on `PATH`, when the contract suite runs, then every required verb passes against `HC_SESSION=tt-tests-<pid>`. Afterwards `~/.local/state/herdr-core/` holds no `tt-tests-*` directory, and the default core's `hc ls --all --json` is unchanged. Observable at the test run and those two checks.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. `TT_TEST_HERDR=1 cargo test --workspace --features test-fixtures` passes on the agent Mac with `hc` 0.1.19 or later. No release from this issue.

**Out of scope:** the `TMUX_TOOLS_LIVE_HERDR=1` live run, README, version bump, and install (ISSUE-261109-2336-02); the Herdr teammate shim; harness rows for `hc`; `herdr-app`.

## Context Pack — canonical at claim (2026-11-10T01:51:23Z)

**Contract approval:** `review-20261110T015021Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261109-2336-01` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `f84d5269470643e7a789a8f934cd0df2b68705b415ad8930107d9bce5259f227` · `objects/f84d5269470643e7a789a8f934cd0df2b68705b415ad8930107d9bce5259f227.md`

**Required source objects:**
- `tmux-tools:PRD-260918-0323-01#p1-transport` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18` · `objects/8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18.md`
- `tmux-tools:PRD-260918-0323-01#p3-state` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435` · `objects/5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435.md`
- `tmux-tools:PRD-260918-0323-01#p4-capture` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648` · `objects/40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648.md`
- `tmux-tools:PRD-260918-0323-01#p7-prompt` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `05941c8a9b107818f53b60318fcb6990b213c10e6b0f3470e28eaf1feefa458c` · `objects/05941c8a9b107818f53b60318fcb6990b213c10e6b0f3470e28eaf1feefa458c.md`
- `tmux-tools:PRD-261107-0825-01#p10-tests` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb` · `objects/a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb.md`
- `tmux-tools:PRD-261107-0825-01#p2-host-port` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271` · `objects/57dfaef1b2a9e0c453e377e6b8b7f07730d8e891de67ab911255357cea195271.md`
- `tmux-tools:PRD-261107-0825-01#p3-selection` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef` · `objects/f9e205e8b5a0046153da498982bd63d179b48d390062414e75e01487a48b0bef.md`
- `tmux-tools:PRD-261107-0825-01#p4-registration` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `4f3186b44eac6fb0216ee87b1f469f422617cadb577ee7033ab286156b50986c` · `objects/4f3186b44eac6fb0216ee87b1f469f422617cadb577ee7033ab286156b50986c.md`
- `tmux-tools:PRD-261107-0825-01#p5-verb-classes` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `126f8593490e6c1e977cd250f022086d9b8e12236740621981f2238d89d8acbe` · `objects/126f8593490e6c1e977cd250f022086d9b8e12236740621981f2238d89d8acbe.md`
- `tmux-tools:PRD-261107-0825-01#p6-host-state` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `8116a17457d9e0d4d77810f04fb8d25e9ceb7d9648425937e1a10758ecba2389` · `objects/8116a17457d9e0d4d77810f04fb8d25e9ceb7d9648425937e1a10758ecba2389.md`
- `tmux-tools:PRD-261107-0825-01#p7-notify` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `314e45ba7b329a7205cffe4ca13f1ef0295742983604de237a67bb2c2cddf9f4` · `objects/314e45ba7b329a7205cffe4ca13f1ef0295742983604de237a67bb2c2cddf9f4.md`
- `tmux-tools:PRD-261107-0825-01#p8-self` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `c9b5c2c4a75da7c6af673a9c3e73e9295a84bae0f5e841abd0790f711207e329` · `objects/c9b5c2c4a75da7c6af673a9c3e73e9295a84bae0f5e841abd0790f711207e329.md`
- `tmux-tools:PRD-261107-0825-01#p9-orca` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `689b0faaccf9198710903fe9aac136378099c40b1f0ce0727a655ee9b31c773c` · `objects/689b0faaccf9198710903fe9aac136378099c40b1f0ce0727a655ee9b31c773c.md`
- `tmux-tools:PRD-261108-0447-01#p2-launcher` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c` · `objects/6b2eb1fbd83eaa40af4b96d98442b69f4cdcc43c00acbde2a864611d425c035c.md`
- `tmux-tools:PRD-261108-0447-01#p7-parent` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2` · `objects/95737dfe26516ef9c5236d638b465fbdd35e179211b3a0e850323e24b663aac2.md`
- `tmux-tools:PRD-261109-2223-01#contracts` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `74b0f28b971c631b93e25756a4063734b1e81a09241130c9445c6d5a62b5bf19` · `objects/74b0f28b971c631b93e25756a4063734b1e81a09241130c9445c6d5a62b5bf19.md`
- `tmux-tools:PRD-261109-2223-01#p1-definitions` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `82b62d7c42d3236aacc25afb16a306e669abbbc5ade72a2ce7510c3a6cb546bc` · `objects/82b62d7c42d3236aacc25afb16a306e669abbbc5ade72a2ce7510c3a6cb546bc.md`
- `tmux-tools:PRD-261109-2223-01#p2-selection` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `086c6b3b9b8eebb36d77e3edcf45228e7310789176b4dc63d28e11249ee4cc98` · `objects/086c6b3b9b8eebb36d77e3edcf45228e7310789176b4dc63d28e11249ee4cc98.md`
- `tmux-tools:PRD-261109-2223-01#p3-placement` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `19dc5433ed7432410116eb836a6647790d40769363ff8b5167b63d8dab5332ef` · `objects/19dc5433ed7432410116eb836a6647790d40769363ff8b5167b63d8dab5332ef.md`
- `tmux-tools:PRD-261109-2223-01#p4-registration` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `8af2f4d5f27c4abf5b4df4eeb5aa40249be20b0e527896fda049aea174e5eb9f` · `objects/8af2f4d5f27c4abf5b4df4eeb5aa40249be20b0e527896fda049aea174e5eb9f.md`
- `tmux-tools:PRD-261109-2223-01#p5-io` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `be9e9141fe77a08b1f549fe40aa0081c37ecb600e9a85e6bc6f298cd34c36b28` · `objects/be9e9141fe77a08b1f549fe40aa0081c37ecb600e9a85e6bc6f298cd34c36b28.md`
- `tmux-tools:PRD-261109-2223-01#p6-state` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `0cdfb0022698c1c2b41e63870910987d21cea4f2a1f935f1c9a617ab97cf430d` · `objects/0cdfb0022698c1c2b41e63870910987d21cea4f2a1f935f1c9a617ab97cf430d.md`
- `tmux-tools:PRD-261109-2223-01#p7-other-verbs` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `94d294169cd23f8d55ba41342e7a351c8b1ddba4a90bbc30fd24e0f9cb050358` · `objects/94d294169cd23f8d55ba41342e7a351c8b1ddba4a90bbc30fd24e0f9cb050358.md`
- `tmux-tools:PRD-261109-2223-01#p8-tests` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `a2fcb876a7cae3bf27e8286858a8f3b6ce8a387ef5d9662e1b99198dc12a3526` · `objects/a2fcb876a7cae3bf27e8286858a8f3b6ce8a387ef5d9662e1b99198dc12a3526.md`
- `tmux-tools:PRD-261109-2223-01#p9-rollout` · revision `2ae0af8b09f314bf016fe6d93108b40db4d39536` · sha256 `f1d170ac88556b789f78c87e078e378cc982007671db6197579634d962a8287a` · `objects/f1d170ac88556b789f78c87e078e378cc982007671db6197579634d962a8287a.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261109-2336-01/manifest.json`

**Test seam:** the `tmux-tools` CLI binary driven by `tests/integration.rs` against a fake `hc` (`tests/contract/fake_hc.py`, call log, state directory) and, with `TT_TEST_HERDR=1`, against a private hc core (`HC_SESSION=tt-tests-<pid>`); `tmux-tools claude-teams` stderr and the fake `claude` argv for P7.7.

## Code Review

Review: `issue-261109-2336-01-code-review-20261110-024758.md` — Outcome: ACCEPTED

- Scope proposal Q1 (found during correction, confirmed in a private core) — CHANGE: PRD-261109-2223-01 P5.1 gives an empty `prompt` result for a Claude pane on Herdr: hc reads Claude as a 120-line `how: stitched` screen with no growing history, the reply is drawn above the History mark, so "every line after that point" is empty (`complete: false`). The implementation follows P5.1 as approved. OWNER: maintainer (Changes prior decision on P5.1).

## Triage Notes

- 2026-11-09: triage step done. Private-core live checks for `herdr-bracketed-paste`, `herdr-keys-charset`, `herdr-history-mark` ran; the maintainer approved each result (Q178–Q180); they are recorded in PRD-261109-2223-01 P5.1, P5.2, P6.6, P8.4.
- 2026-11-09: dispatch readiness passed; brief repaired (finished triage-step paragraph deleted; P7.7 acceptance moved to the claude-teams Launcher stderr and the spawn-agent call log); ready-for-agent.

## Resolution

- Commit: `feat: tmux-tools runs every required verb on Herdr through a herdr Host backend over the hc CLI, tested against a fake hc and a private hc core (ISSUE-261109-2336-01)`
- Route: `claude — deepseek-suspended` (Opus, medium) for implementation and batches 1–2; batch 3 `claude — regression` (Opus, high), which found M4 a false positive and changed nothing.
- TDD: red-green at the CLI binary seam (fake `hc` call log and state; `TT_TEST_HERDR=1` private core).
- Review: `issue-261109-2336-01-code-review-20261110-024758.md` — ACCEPTED after 3 correction batches; M1–M3 fixed, M4 and NEW-1/CODEX-2 dismissed. Scope proposal Q1 (P5.1 empty prompt result for a Claude pane) is with the maintainer.
- Suite: `TT_TEST_HERDR=1 cargo test --workspace --features test-fixtures` 514 passed, 0 failed; default run 339 passed, 1 failed (`wait_idle_timeout_concise_first_line_includes_idle_for`, timing flake that also fails on base 12bb8c4). Both run with this session's `TT_TEAM`/`TT_HOST`/`TMUX` unset; with them set, four Orca live-run tests fail with `team not found` on base too. Leak checks clean: no `tt-tests-*` under `~/.local/state/herdr-core/`; the default core's `hc ls --all --json` is unchanged.
- Date: 2026-11-10
