---
id: ISSUE-260918-0525-07
kind: issue
category: enhancement
status: done
context: tmux-tools
brief_contract: agent-brief.v2
summary: a manual version-stamped smoke validates the claude, codex and README-documented dsh surfaces against the live binary, and flat surfaces become default once validated
terms: [Surface, Flat surface, Rich surface, ready_regex, busy_regex]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p8-smoke
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p2-surface
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-02
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-03
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-04
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-05
---

## Agent Brief

**Outcome and scope:** The `claude`, `codex` and README-documented `dsh` surfaces are validated against the live binary by a
manual smoke that records the version it checked. Flat surfaces that pass become the default.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p8-smoke`;
`tmux-tools:PRD-260918-0323-01#p2-surface` (only the default-promotion bar and the rule that
pre-surface readiness fields stay bound to the surface matching the pre-surface rendering).

**Current observable:** Flat surfaces have no validated `ready_regex`/`busy_regex`, and every
agent defaults to its pre-surface rendering.

**Required change:** A manually run smoke, outside `cargo test`, covering each `claude`, `codex` and README-documented `dsh` surface for
every check P8 lists, and recording the validated agent version the way `agents.toml` already does
for codex and claude. Ship the flat surfaces' measured patterns and each surface's measured
quit-when-idle behavior and bracketed-paste capability, on each `claude`, `codex` and README-documented `dsh`
surface. Promote a flat surface to default only when both its `ready_regex` and
`busy_regex` are validated and stamped with the agent version; every other agent keeps its current
default. Readiness fields in a pre-surface `agents.toml` stay bound to the pre-surface-rendering
surface after promotion, and such a file still loads.

**Acceptance:**
- [ ] Before merge, the maintainer runs the smoke on a machine with the agents installed, and it
  passes. It fails on any surface whose declared quit-when-idle or bracketed-paste value differs
  from what it measures, or whose declared patterns do not match. It validates every pattern a
  surface declares; rich surfaces are not required to gain patterns, and a quit-hazard surface with
  no `busy_regex` is refused by `interrupt` by design. The non-alternate-screen check
  applies to flat surfaces only.
- [ ] Every `claude`, `codex` and README-documented `dsh` surface ships the smoke-measured quit-when-idle and
  bracketed-paste declarations, and every flat surface among them ships its measured
  `ready_regex` and `busy_regex`. Each is stamped with the validated agent version beside the
  declaration. Those stamps are the persisted smoke record, observable in the registry source and
  README.
- [ ] Given a flat surface with both patterns validated, when `spawn-agent` runs with no surface
  named, then that surface is recorded; given an agent whose flat surface lacks a validated
  `busy_regex`, its pre-surface-rendering surface is recorded. Observable in pane registration.
- [ ] Given a pre-surface `agents.toml` overriding readiness fields for an agent whose flat surface
  was promoted, then the file loads, the overrides govern the pre-surface-rendering surface, and the flat default
  keeps its shipped patterns, observable through `wait-idle` on a pane of each surface.

**Verification and release:** The manual smoke, plus integration tests for default resolution and
the `agents.toml` binding; full workspace suite green under the documented test command. Release is
a CLI version bump, with rollback by git revert. When the version of `claude`, `codex` or `dsh` changes, re-run the
smoke and re-stamp the validated version.

**Out of scope:** Live agent binaries in `cargo test` or CI; measuring or adding surfaces for
`cursor` or `agy`, whose single surfaces keep the values `ISSUE-260918-0525-02` ships.
## Context Pack — canonical at claim (2026-09-19T08:10:20Z)

**Approval:** dispatch-20260919T043849Z (/Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/approvals/ISSUE-260918-0525-07.json) · issue tmux-tools:ISSUE-260918-0525-07@289c810211fae68df21fac97dc27c26958c1c5dd · sha256 19decc06d01a22600c230d48d2ef0cfe6397bd567edeb59f1e82b759b7d1b688

**Contract bundle:** /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-07 · manifest /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-07/manifest.json

**Required source objects:**
- tmux-tools:PRD-260918-0323-01#p2-surface · revision a4b9f7977809b28cb673226ea7562f5f588f10b2 · sha256 89e022e32cffd813184bc0c6e201a2e28a09fa2312ddf3c7d27c2898edd794d0 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-07/objects/89e022e32cffd813184bc0c6e201a2e28a09fa2312ddf3c7d27c2898edd794d0.md
- tmux-tools:PRD-260918-0323-01#p8-smoke · revision a4b9f7977809b28cb673226ea7562f5f588f10b2 · sha256 e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-07/objects/e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2.md

**Public seam:** observable at `the manual smoke pass/fail result; version-stamped declarations in the registry source and README; surface recorded in pane registration by spawn-agent; wait-idle on a pane of each surface`

**Mode:** canonical

## Code Review

Review: `issue-260918-0525-07-code-review-20260919-085416.md` — Outcome: ACCEPTED

## Repair Notes

- kind: infrastructure
- Attempt: claimed by `implement-issue@Mac-mini-4` at 2026-09-19T08:10:20Z; parked 2026-09-19T09:44Z.
- Failed operation: `planning-contracts.sh bundle-verify --mode canonical` (and `revision-check`) against `.planning-contracts/approvals/ISSUE-260918-0525-07.json` / `.planning-contracts/bundles/ISSUE-260918-0525-07`. Actual: `brief-contract-unsupported` ("nonterminal issue requires brief_contract: agent-brief.v2"). Expected: `complete: true` — the same check passed at claim and at review start. Cause: uncommitted external edits to the shared `domain-modeling/scripts/planning_contracts.py` (not in this repository) made mid-run and still changing when retried; the approved issue carries no `brief_contract` field, and adding one changes the approved frontmatter, so it is not fixable here. The same CLI blocks the lifecycle refresh, the implementation receipt and completion-verify, so this handoff has no approval-envelope refresh.
- Implementation: WIP branch `wip/ISSUE-260918-0525-07-20260919T094437Z`, commit `2edeb9d7e17bba861c194ea68893fc3c0b5fc126` (route deepseek; TDD red-green at spawn-agent pane registration and wait-idle).
- Review: ACCEPTED after 2 of 3 correction batches (H1, H2, M1–M5 all verified FIXED by both reviewers); no-progress count 0.
- Evidence suite on the parked tree (contract gate skipped): `cargo test --workspace --features test-fixtures` 186 passed; `cargo test --workspace` 138 passed; `cargo test --features vendor-smoke --bin tt-vendor-smoke` 5 passed; clippy `-D warnings` and `fmt --check` clean. Live manual smoke (claude 2.1.278, codex-cli 0.155.1, dsh 0.1.5-rc.1): `RESULT: PASS`, all 5 surfaces.
- Resume phase: Step 6 (full suite under a passing contract check), then Step 7 close. First confirm the planning-contracts CLI change is committed and whether this issue needs a `brief_contract` migration/re-approval (owned by that CLI change, not by this issue). Then retry the failed `bundle-verify` once, restore the WIP commit with `git cherry-pick --no-commit`, and continue.

## Resolution

- Commit: `feat: a manual version-stamped smoke validates the claude, codex and README-documented dsh surfaces against the live binary, and flat surfaces become default once validated (ISSUE-260918-0525-07)`
- Route: deepseek (implementation and correction batches, per the review record).
- TDD: red-green at spawn-agent pane registration and wait-idle.
- Review: ACCEPTED after 2 of 3 correction batches; H1, H2, M1–M5 verified FIXED by both reviewers (`issue-260918-0525-07-code-review-20260919-085416.md`).
- Suite: `cargo test --workspace --features test-fixtures` 186 passed; `cargo test --workspace` 138 passed; `cargo test --features vendor-smoke --bin tt-vendor-smoke` 5 passed; clippy `-D warnings` and `fmt --check` clean.
- Smoke: live manual smoke `RESULT: PASS` on all 5 surfaces (claude 2.1.278, codex-cli 0.155.1, dsh 0.1.5-rc.1); versions unchanged at close, not re-run.
- Date: 2026-09-19T10:01:25Z
- Closed from the parked wip commit 2edeb9d by maintainer authorization after the brief_contract epoch migration; no re-review.
