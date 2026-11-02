---
id: ISSUE-261102-0012-01
kind: issue
category: bug
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: prompt reports its result incomplete when the pane's width changed between the history mark and settle
terms: [History mark, Completeness, Tile]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p7-prompt
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: verification
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-01
---

## Agent Brief

**Outcome and scope:** `prompt` reports `complete: false` whenever the target pane's width at settle differs from its width at the history mark. No other `prompt` behavior changes.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p7-prompt` — Completeness conditions and reporting channels; `tmux-tools:PRD-260918-0323-01#verification` — the width-change scenario, the `tt-fake-tui` fixture and the CLI seam; `tmux-tools:PRD-260801-0656-01` — § Accepted limits (retiles are not deferred for in-flight prompts; this report is the mitigation), § Sequencing (pane-creating slices land after the private harness).

**Current observable:** On a main-screen pane, a width change during an in-flight `prompt` still yields `complete: true`.

**Required change:**
- Read the pane's width in the same probe at the mark and at settle; a difference makes the result incomplete, as a third condition beside no-scrollback and eviction.
- The width condition combines with the existing conditions; any one makes the result incomplete.
- Completeness is reported on the existing channels unchanged: stderr for raw and concise, the structured field for JSON.

**Acceptance:**
- [ ] Given a fixture pane on the main screen, when its width is changed while `prompt` waits for settle, then `prompt --format json` reports `complete: false`; observable at the `tmux-tools prompt` CLI seam on an isolated tmux server. Fails before this change.
- [ ] Given the same fixture and exchange with the width unchanged, then `prompt --format json` reports `complete: true`.
- [ ] Existing `prompt` completeness and turn-observation tests pass with their assertions unchanged. The test-only probe-rewriting tmux wrapper may change only to pass the pane width through.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:** Recomputing or re-anchoring the mark after a width change; deferring or suppressing retiles while a prompt is in flight; height-only changes; any change to `capture` or `wait-idle`.

## Triage Notes

- 2026-11-02: minted from maintainer decision Q3 (retile vs in-flight prompt) during auto-tile dispatch readiness; mint-check repair applied.
