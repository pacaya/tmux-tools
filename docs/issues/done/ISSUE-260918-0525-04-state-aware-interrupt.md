---
id: ISSUE-260918-0525-04
kind: issue
category: bug
status: done
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-09-19T05:33:23Z
context: tmux-tools
summary: interrupt sends the surface's declared key and refuses whenever that key could quit the agent
terms: [Surface, Pane state, Surface-unvalidated pane]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p5-interrupt
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-02
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-03
---

## Agent Brief

**Outcome and scope:** `interrupt` never quits an agent it was asked to interrupt.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p5-interrupt`.

**Current observable:** `interrupt` always sends `C-c`, and at least one agent rendering quits on
`C-c` when idle.

**Required change:** P5 as written. Send the resolved surface's interrupt key. On a quit-hazard
surface, refuse anything other than positively busy from P3's classifier, and refuse any
surface-unvalidated pane outright. Every refusal says what to do instead. A surface declaring no
quit hazard sends its declared key on every state result; a pane with no surface sends `C-c` as
today. `--force` and `--any` are unchanged.

**Acceptance:**
- [ ] Given a fixture pane on a quit-hazard surface, when `interrupt` runs while it is idle, and
  again while its state is unknown, then both are refused with guidance and no key reaches the
  fixture; while it is busy, the key is sent. Observable at the CLI and in what the fixture receives.
- [ ] Given a quit-hazard pane spawned with caller-supplied trailing arguments, when `interrupt`
  runs while the fixture shows its busy footer, then it is refused.
- [ ] Given a busy fixture pane on a quit-hazard surface whose declared interrupt key is not `C-c`,
  when `interrupt` runs, then the fixture receives the declared key, not `C-c`.
- [ ] Given a fixture pane on a surface declaring no quit hazard, when `interrupt` runs while idle,
  while unknown, and on a surface-unvalidated pane, then the declared key is sent each time.
- [ ] Given a `launch`ed shell pane, when `interrupt` runs, then `C-c` is sent as today.

**Verification and release:** Integration tests at the CLI seam with `tt-fake-tui`; full workspace
suite green under the documented test command.

**Out of scope:** Measuring real agents' quit behavior (P8, `ISSUE-260918-0525-07`); detecting that
an agent has exited.
## Context Pack — canonical at claim (2026-09-19T05:33:23Z)

**Approval:** dispatch-20260919T043335Z (/Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/approvals/ISSUE-260918-0525-04.json) · issue tmux-tools:ISSUE-260918-0525-04@f2f69619688b457ceabf8737eca15fc3a9d319b8 · sha256 2557a8bb50faa92a50723736ea515ccc83c6e128fd5f312b8b46a537db92ed71

**Contract bundle:** /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-04 · manifest /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-04/manifest.json

**Required source objects:**
- tmux-tools:PRD-260918-0323-01#p5-interrupt · revision 61a74879a6f1fe53be714833274b2d256c28c7a5 · sha256 93a3ad0c6dbb6f2d5d789705ce3553cc2e325ea347ea6a1a55d50b4a9766297c · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-04/objects/93a3ad0c6dbb6f2d5d789705ce3553cc2e325ea347ea6a1a55d50b4a9766297c.md

**Public seam:** observable at `the CLI and in what the fixture receives` (integration tests at the CLI seam with `tt-fake-tui`)

**Mode:** canonical

## Code Review

Review: `issue-260918-0525-04-code-review-20260919-055716.md` — Outcome: EXHAUSTED

## Triage Notes

- 2026-09-19 (implement-issue): parked on `wip/ISSUE-260918-0525-04` (5d178ae). Unresolved blocker: M1 in `issue-260918-0525-04-code-review-20260919-055716.md`. In the refusal tests, the second sentinel barrier (after the JSON run) waits only for *any* sentinel rather than a new one. A key delivered by the JSON-format interrupt can therefore go unobserved. Both reviewers rechecked it as STILL_BROKEN after the single allowed correction; M2–M4 are fixed. Required action: make `send_sentinel_and_wait` wait for a new sentinel (for example, compare the count before and after), then run the full suite and close.

## Resolution

- **Commit:** `fix: interrupt sends the surface's declared key and refuses whenever that key could quit the agent (ISSUE-260918-0525-04)`
- **Route:** direct maintainer-authorized finish. The record was parked `ready-for-human` on `wip/ISSUE-260918-0525-04` (5d178ae). The maintainer explicitly authorized finishing it directly, bypassing `/triage` and `/implement-issue`. The wip branch was merged, and the one open blocker (M1) was fixed in test code only: `send_sentinel_and_wait` now waits for a new sentinel. No product code changed beyond the parked implementation.
- **TDD:** red-green at the CLI seam with `tt-fake-tui` (fixture key report bounded by a `C-g` sentinel barrier).
- **Review:** dual review (Claude and Codex). M2–M4 were fixed within the bounded loop. After the single allowed correction, M1 remained STILL_BROKEN, so the loop ended EXHAUSTED. This record closed outside the bounded review loop by maintainer authorization, and no second broad review was run. M1 was verified by a red/green sabotage check. With the key sent in the JSON-format refusal path, all five refusal tests failed: `interrupt_on_quit_hazard_surface_refuses_while_idle`, `…_refuses_while_unknown`, `interrupt_refuses_surface_unvalidated_pane_even_when_busy`, `interrupt_malformed_hazardous_unvalidated_pane_uses_unvalidated_refusal`, `interrupt_on_validated_hazardous_surface_with_malformed_regex_refuses`. With the guard restored, all 11 interrupt tests passed. Aggregate outcome: ACCEPTED (M1–M4 fixed).
- **Suite:** `cargo test --workspace --features test-fixtures`: 182 passed, 0 failed. `cargo test --workspace`: 136 passed, 0 failed. `cargo clippy --workspace --all-targets --features test-fixtures` and `cargo fmt --check` clean.
- **Date:** 2026-09-19 (UTC)
