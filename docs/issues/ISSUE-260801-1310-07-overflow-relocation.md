---
id: ISSUE-260801-1310-07
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: Relocate a pane to another window when placing it would push its origin below the effective floor
terms: [Overflow window, Effective floor, Layout intent, Ownership]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-05
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0128-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-06
---

## Agent Brief

**Outcome and scope:** When a tiled placement leaves its window below the effective floor, the newly
created pane moves to an eligible marked window in the target window's session that has room, or to a
fresh marked window in that session; the origin is retiled; focus does not change; both spawning
verbs report `relocated` and the final `window_id`. Only the new pane moves.

**Required sources:** tmux-tools:PRD-260801-0656-01 — § Placement and overflow (the trial, join vs
break, fallback marking, `-d`), § Floor: per surface, in the registry (pane floor and effective floor),
§ Ownership and § Layout intent (candidate eligibility), § Flags (opt-outs placed as today),
§ Reporting surface (`relocated`, `window_id`, `below_floor` describe the final window; `status` for
the origin), § Accepted limits (crowding not repaired), § Module shape (overflow trial inside `layout`, `place`
returns where the pane landed), § Solution (no invocation starts failing), ## Out of Scope (one
session; no locking, repair or prediction), § Testing Decisions (seams, closing check).

**Current observable:** With ISSUE-260801-1310-05 and ISSUE-260802-0128-01 landed, a tiled placement
that breaches leaves the new pane in the target window and reports `below_floor: true`; neither verb
reports `relocated`.

**Required change:**
- The layout module's `place` gains a trial-and-move stage after the post-retile measurement and
  returns the window the pane finally landed in. It runs only when that measurement shows the target
  window below its effective floor. Each pane's floor is its resolved surface floor, resolved through
  ISSUE-260801-1310-05 from `@tt-agent` and `@tt-surface` (the incoming pane's from the agent and
  resolved surface passed to `place`).
- Scope of the stage: placements that tiled the target window. Explicit sizing opt-outs (`--split h|v`,
  `--size`) are placed as today, and a window that was not owned before the split receives the pane
  and is not retiled (PRD § Flags, § Ownership).
- Session binding: candidate windows and the fresh fallback window are in the session of the target
  (origin) window, never the caller's session.
- Candidates: the session's windows other than the origin that pass the shared window guard owned by
  ISSUE-260801-1310-06 (`@tt-tiled` marker and an owned pane), with no second marker read or ownership
  scan, judged before any pane is joined into them, tried in ascending window index. Overflow-candidate ordering (eligible marked windows, ascending index) is pure given
  the session's windows, their markers and their panes' registrations, and is unit-testable without a
  tmux server.
- Trial per candidate: `join-pane -d` the new pane into it, retile, measure against the candidate's
  effective floor including the new pane. If it does not breach, the pane stays. If it breaches, move
  the pane on and retile the rejected candidate again after the pane leaves. A refused `join-pane`
  counts as a rejected trial: the search continues to the next candidate or the fresh-window fallback,
  and the invocation still exits 0 and reports the final window.
- Fallback when every candidate breaches or none exists: `break-pane -d` with the target window's
  session named as destination, then tile the new window and set `@tt-tiled` on it.
- After any move, retile the origin window.
- No move or retile selects a window: the session's current window is unchanged.
- Both spawning verbs report `relocated` (true only when a breach forced a move) in `concise` and
  `json`; `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor` describe the window the pane
  finally landed in. `raw` still emits only the pane id. Pane user-options travel with the pane; do
  not rewrite registration after a move.
- A window already below its floor before the placement is not repaired; only the new pane leaves it.

**Acceptance:** All CLI-seam checks run on the private 280×82 test server in its inside-tmux mode, and
read tmux state independently with `tmux -L <socket>` queries (`#{window_id}`, `#{session_name}`,
`#{window_layout}`, `show-options -w @tt-tiled`).
- [ ] Given an owned `@tt-tiled` window sized so one more tiled pane breaches its effective floor and
      no other window in the session, when `spawn-agent` places into it, then the new pane is in a
      different window than the target's and the origin's surviving panes are retiled (their
      `window_layout` equals the one `select-layout tiled` produces for them), observable at the
      `spawn-agent --format json` output and tmux queries. Fails before this change.
- [ ] Given that breaching origin and an eligible marked window in the same session with room, when
      `spawn-agent` places into the origin, then the reported `window_id` is that eligible window's id
      and the session gains no window.
- [ ] Given a breaching origin and three eligible candidates, where the lowest-index one breaches with
      one more pane and the middle and highest-index ones have room, when `spawn-agent` places into the
      origin, then in the same run the reported `window_id` is the middle candidate's and the
      lowest-index candidate's `window_layout` equals its value before the placement.
- [ ] Given a breaching origin and no candidate with room, when `spawn-agent` places into it, then the
      reported `window_id` is a window that did not exist before the placement and
      `show-options -w -t <window_id> @tt-tiled` returns a value.
- [ ] Given a breaching placement that joins into an eligible candidate, and separately one that falls
      back to `break-pane`, when each completes, then in the same run `relocated` is true, the reported
      `window_id` differs from the target's, the session's current window
      (`display -p -t <session> '#{window_id}'`) equals its value before the placement, and
      `show-options -p -t <pane_id>` on the moved pane gives `@tt-name` equal to the reported name,
      `@tt-agent` the spawned agent, `@tt-surface` its resolved surface and `@tt-cwd` the invoking cwd.
- [ ] Given a breaching placement whose `--target` is in a session other than the caller's
      (`TMUX_PANE` in another session on the same server), where the caller's session has an eligible
      marked window with room and the target's session has no candidate with room, when it completes,
      then the reported `window_id` belongs to the target's session (`#{session_name}` of that window),
      observable at the `spawn-agent --format json` output.
- [ ] Given a breaching placement, then both `launch` and `spawn-agent` report `relocated` and the final
      `window_id` in `concise` and `json`. Given a placement that does not breach, then `relocated` is
      false and `window_id` equals the target window's id.
- [ ] Given (a) an owned window where `spawn-agent --split v` leaves it below its effective floor, and
      (b) a first placement into a window not owned before the split that leaves it below its effective
      floor, when each completes, then the pane stays in the target window and `json` reports
      `relocated: false`, `below_floor: true` and the target's `window_id`.
- [ ] Given an owned over-full target window, still below its effective floor after
      `select-layout tiled` before the placement, when
      `spawn-agent` places into it, then in the same run `relocated` is true, the reported `window_id`
      differs from the target's, and `status` pointed at the origin window reports it below floor.
- [ ] Unit tests in `core` cover overflow-candidate ordering: unmarked windows, windows without an
      owned pane, and the origin are excluded; the remainder are in ascending index order.

**Verification and release:** Breaching scenarios use `spawn-agent` with the `AgentsConfig` registry
injected through `XDG_CONFIG_HOME` and `tt-fake-tui` (tests/integration.rs) under the `test-fixtures`
feature, with surface floors chosen so the 280×82 server breaches; they run without an installed
vendor agent and fail, never skip, when the behavior is absent. `cargo test --workspace --features
test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A
failure that survives isolation and that this change did not introduce is reported, not fixed. No
release action.

**Out of scope:**
- Searching outside the target window's session for a destination.
- Cross-process locking against concurrent overshoot.
- Repairing a window that was already below its floor, or any over-full window after the fact.
- Predicting post-move geometry instead of trialling it.
- Moving an existing pane rather than the newly created one.
- Relocating opt-out placements or placements into a window not owned before the split.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied.
