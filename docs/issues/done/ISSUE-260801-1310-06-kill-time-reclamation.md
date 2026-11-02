---
id: ISSUE-260801-1310-06
kind: issue
category: enhancement
status: done
brief_contract: agent-brief.v3
context: tmux-tools
summary: kill retiles a tool-tiled window so survivors reclaim the removed pane's space
terms: [Layout intent, Ownership, Exited-agent pane]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0128-01
---

## Agent Brief

**Outcome and scope:** When `kill` removes a pane from a window that carries `@tt-tiled` and holds a
pane this invocation owns, the window is retiled so the remaining panes reclaim the space. This issue
also owns the shared window guard in the core layout module; ISSUE-260802-0205-01 reuses it. It also
owns the core layout module's kill-time removal operation, which sends the pane removal and, when the
window guard holds, the retile to tmux as one command sequence. `kill` does not use `rebalance`.

**Required sources:** tmux-tools:PRD-260801-0656-01 § Ownership (shared predicate, window
eligibility); § Layout intent: the `@tt-tiled` window marker (marker semantics, read before
destruction); § Reclamation on kill (retile rule, same command sequence, last pane, `--any` scope);
§ Module shape (layout module owns the retile and the kill-time removal operation; no verb-to-verb
dependency); § How to run the suite,
and what a surviving failure means (closing check).

**Current observable:** `tmux-tools kill` destroys the target pane and never retiles its window.

**Required change:**
- Shared window guard, in the core layout module, owned here: read the window's `@tt-tiled` marker and
  evaluate the shared ownership predicate across every pane of the window. Its result tells which
  guard failed (marker absent, or no owned pane), so ISSUE-260802-0205-01 can report it.
- `kill` evaluates the guard on the target's window before destruction. It must not reuse the safety
  evaluator's target-pane verdict as the ownership answer.
- Kill-time removal operation, in the core layout module, owned here: it sends the pane removal and,
  when the window guard holds, the retile to tmux as one command sequence, so a `kill --force` on the
  caller's own pane still retiles. `kill` removes panes through this operation and does not use
  `rebalance`.
- When both guards hold, retile the window after every removal.
- When the target is the last pane in its window, issue no retile and emit no error.
- `--any` controls destruction only: a window that fails the guard is not retiled with `--any`; a
  window that passes the guard is retiled with or without `--any`, also when the killed pane itself
  is unowned.
- `kill` depends on the core layout module for the retile, never on the launch verb.
- Safety guards, denial messages, and `kill` output in all formats stay unchanged.

**Acceptance:**
All criteria run at the CLI seam (`tmux-tools kill`, exit code and stdout, geometry read from tmux)
against the isolated 280x82 server. "Retiled" means the remaining panes' geometry equals the geometry
an explicit `select-layout tiled` of the same panes produces. "Not retiled" means the remaining panes'
geometry differs from the geometry `select-layout tiled` gives for them; a fixture used for a "not
retiled" check must be built so that tiled and untiled geometry differ. Where a criterion names no fixture, use a
marked window of five tiled panes and kill one; any other fixture must fail without the retile.
- [ ] Given a marked, owned window of six tiled panes, when any two panes are killed, then each of the
      four remaining panes is at least 40 rows tall.
- [ ] Given a marked, owned window of six tiled panes, when one pane outside the last row is killed,
      then the window is retiled.
- [ ] Given two windows that differ only in the `@tt-tiled` marker, each holding a pane this
      invocation owns, when the corresponding pane is killed in each, then only the marked window is
      retiled and the unmarked window is not retiled.
- [ ] Given a marked window whose only owned pane is the target, when it is killed, then the window is
      retiled.
- [ ] Given a marked window with an unowned target and an owned sibling, when `kill --any` kills the
      target, then the window is retiled.
- [ ] Given a marked window whose panes carry a foreign `@tt-cwd`, and a control window built the same
      way and differing only in `@tt-cwd`, when `kill --any` kills the corresponding pane in each, then
      the foreign window is not retiled and the control window is retiled.
- [ ] Given a marked, owned five-pane window, when a process in one pane runs `kill --force` on its own
      pane, then the remaining panes are retiled.
- [ ] Given a marked window whose single pane this invocation owns, when that pane is killed, then
      exit code is 0, the window is gone, and no error is emitted.
- [ ] Given the existing kill tests, when the suite runs, then they pass unmodified and the safety
      guards, denial messages, and output are unchanged.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an
isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and
that this change did not introduce is reported, not fixed.

**Out of scope:** reclaiming exited-agent panes; retiling on `interrupt`, `escape`, or `send-enter`;
changing which panes `kill` may destroy; repairing a window already below its floor before the kill.

## Triage Notes

2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied.
