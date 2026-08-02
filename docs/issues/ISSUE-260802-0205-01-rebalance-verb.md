---
id: ISSUE-260802-0205-01
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: A rebalance verb so a crowded window can be retiled through the tool instead of raw tmux
prd: PRD-260801-0656-01
terms: [Tile, Layout intent, Ownership, Effective floor, Advisory floor]
blocked_by: [ISSUE-260802-0128-01, ISSUE-260801-1310-05]
---

## Agent Brief

**Category:** enhancement
**Summary:** Expose the layout module's rebalance operation as a verb, so an over-full window can be repaired without shelling out to raw tmux

**Current behavior:**
After the epic lands, a window can end up below its effective floor in two ways the tool detects but
cannot repair. Concurrent spawns overshoot: each is a separate short-lived process that measures
before the others have split, so *k* simultaneous spawns can each conclude there was room. And a pane
created by a direct `tmux split-window` consumes geometry the tool never placed. The PRD accepts both
and says so plainly — the overshoot "is **not** self-correcting — no later operation moves panes out
of an over-full window; a subsequent spawn places itself elsewhere and leaves the crowding in place.
It is surfaced through `status` and `below_floor`."

So the tool reports the condition and offers no way to act on it. The verb list is create, talk to,
read, destroy, inspect — there is no layout verb at all. A human can run `tmux select-layout tiled`
by hand, but an agent driving this tool has to shell out to raw tmux, which is the same bypass the
PRD identifies as how the advisory floor gets circumvented in the first place.

**Desired behavior:**
A `rebalance` verb retiles a window through the tool and reports the geometry that resulted.

It takes the same target grammar as the other verbs and resolves to the pane's containing window; with
no target it uses the calling pane's window. It applies the same two guards `kill` uses before
retiling — the window must carry the layout-intent marker and must contain a pane owned by this
invocation — because a verb that reshapes any window on request would hand callers a way around the
ownership rule the rest of the epic is built on. A window failing either guard is refused with a
diagnostic naming which guard failed, not silently ignored.

It reports the same measured geometry fields the spawning verbs already emit, so a caller can act on
the result without a second inspection: the window id and the smallest measured pane width and height,
plus the floor verdict. `raw` stays byte-for-byte consistent with the convention the other verbs
follow.

Rebalancing cannot fix a window that is over-full: retiling redistributes geometry, it does not remove
panes. A window below its effective floor before the call is very likely still below it afterwards, and
the verb says so through the verdict rather than pretending otherwise. What it repairs is the *ragged*
case — a window whose panes drifted out of an even arrangement — which is the common outcome of a
concurrent overshoot or a hand-made split.

**Key interfaces:**
- The layout module's existing rebalance operation, unchanged. This slice is a verb wrapping it; if it
  needs new behavior in the layout module, that is a signal the scope is wrong.
- The shared ownership predicate and the layout-intent marker read, both already used by `kill`.
- The verb dispatcher and the target-resolution path both verbs already share.
- Output rendering reuses the spawning verbs' geometry fields rather than inventing spellings; the
  field names are fixed by the PRD's § Reporting surface.

**Acceptance criteria:**
- [ ] `rebalance` exists as a verb: invoking it with `--help` exits zero and it appears in the
      top-level help output. Neither is true before this change.
- [ ] Rebalancing a marked, owned window whose panes are ragged leaves them in tmux's tiled
      arrangement. Observable at the CLI seam against the private test server: build a ragged window by
      splitting without the tool, rebalance it, and the resulting pane heights are within one row of
      each other. Red before this change, because the verb does not exist.
- [ ] A window with no layout-intent marker is refused, exits non-zero, and its pane geometry is
      unchanged. The same test must also rebalance a **marked** window and assert it *is* retiled, so
      an implementation that refuses everything cannot pass.
- [ ] A window containing no pane owned by this invocation is refused and left unchanged, with the
      same positive control in the same test.
- [ ] The refusal diagnostics distinguish the two guards — a caller can tell an unmarked window from an
      unowned one without reading the source.
- [ ] The verb reports the window id, the smallest measured pane width and height, and the floor
      verdict, under the field names the spawning verbs use.
- [ ] A window still below its effective floor after rebalancing reports that verdict rather than
      reporting success — retiling redistributes geometry and cannot remove panes.
- [ ] `cargo test --workspace` passes, run against an isolated tmux server per the PRD's Testing
      Decisions.

**Out of scope:**
- Moving panes out of an over-full window. Relocation happens at placement time; this verb only
  retiles what is there, and the PRD's rejection of after-the-fact repair stands.
- Killing exited-agent panes to reclaim their geometry — unchanged, and rejected for the same reason
  as everywhere else in this epic: it would destroy preserved crash output.
- A layout template argument. This verb applies the epic's one template; a window wanting something
  else is a hand-arranged window, which is what the marker exists to protect.
- Setting or clearing the layout-intent marker. The verb reads it; it does not grant itself
  permission by writing one onto a window it was refused.
- Rebalancing every window in a session at once.
- Any change to placement, floors, relocation, or kill-time reclamation.

## Triage Notes

Minted after the epic's breakdown was gated, in response to a scope question from the user: whether
the tmux verbs this epic uses internally — `join-pane`, `break-pane`, `select-window` — should also be
exposed as CLI surface.

For the first two the answer is no: they are internal calls inside the overflow-relocation slice, and
nothing needs them as verbs. For `select-window` the answer is a firmer no — the epic's "reported, not
selected" rule and the `-d` on both move commands exist precisely so placement never steals focus, so
a focus verb would cut against the design rather than complete it.

The question did surface a real gap, which is this record. The PRD accepts that concurrent overshoot
and raw splits leave windows crowded, and routes both to `status`/`below_floor` — detection with no
remedy reachable through the tool. That asymmetry is defensible for a human at a terminal and awkward
for an agent, which is the primary caller.

Deliberately **not** folded into the epic: the eight slices were mid-gate when this was raised, and
adding scope would have restarted convergence on records one finding from passing. It is sequenced
after the marker and the floor verdict exist, since it consumes both.

**Readiness gate (cold-reader): PASS** (round 1)

No findings. The blocker chain was traced item by item rather than assumed, including one transitive
hop this record leaves implicit: the reported field names come from ISSUE-260801-1310-04, which is not
a direct blocker here but is one of ISSUE-260802-0128-01's, so it is guaranteed to land first. The
duplication check confirmed nothing else in the epic already offers an on-demand retile — `kill`'s
reclamation fires only on pane removal and is not callable — and that the PRD's own Accepted-limits
section names this record as the remedy it chose.

Class 6 was checked on both prongs, including whether "verb", "guards", and "reporting" could be
separate slices. Rejected for the right reason: a guardless rebalance would be an unsafe intermediate
state, handing callers a way around the ownership rule the rest of the epic rests on.

Baseline verified by execution rather than reading — `tmux-tools rebalance --help` exits 2 with
`unrecognized subcommand`, and the verb is absent from top-level help, so the first two criteria are
genuinely red.

One loose thread the reader flagged without firing, recorded here so it is not lost: this brief says
it applies "the same two guards `kill` uses", and `kill`'s combined pre-destroy guard is built in
ISSUE-260801-1310-06, which is *not* a blocker of this record. The wording is loose but the dependency
is not missing — the guard semantics are pinned in the PRD's § Reclamation on kill, and the primitives
this record actually needs (the marker read and the ownership predicate) come from its declared
blockers transitively. Reading the sibling's code is not required.
