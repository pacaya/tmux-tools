---
id: ISSUE-260801-1310-07
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Relocate a pane to another window when placing it would push its origin below the effective floor
prd: PRD-260801-0656-01
terms: [Overflow window, Effective floor, Layout intent, Ownership]
blocked_by: [ISSUE-260801-1310-05, ISSUE-260802-0128-01]
---

## Agent Brief

**Category:** enhancement
**Summary:** On a floor breach, move the newly created pane to a window with room, or to a fresh one

**Current behavior:**
A placement that pushes its window below the effective floor is detected and reported, but the pane
stays where it landed. The window remains crowded and every pane in it — including agents whose
readiness signal depends on the geometry — stays below the floor.

**Desired behavior:**
When the post-retile measurement shows the destination window below its effective floor, the
**newly created** pane is moved elsewhere and the origin window is retiled back to the arrangement
it had. Moving the new pane rather than an existing one keeps the disruption on the pane whose
agent just started, which is the one measured to tolerate an immediate resize.

Candidate destinations are the current session's windows that carry the layout-intent marker and
are owned by this invocation. They are tried in ascending window index for determinism. Because
geometry is never forecast, capacity is established by **trial**: join the pane in, retile, measure,
and if that window now breaches its own effective floor, move on. A rejected candidate is **retiled
again after the pane leaves**, so a failed trial restores its arrangement rather than the ragged one
pane removal produces. The candidate set is the session's eligible marked windows, so the search is
finite; when every candidate breaches, the pane goes to a fresh window.

Two tmux commands are involved and they are not interchangeable. Joining into an **existing** window
is a join operation; breaking out to a **new** window is a break operation, which makes the moved
pane the sole pane of a fresh window and therefore cannot pack into an existing one. Both select the
destination window unless told not to — the pane must move without stealing the user's focus, since
placements routinely happen in the background.

A fresh fallback window is tiled and marked after the move. This is not automatic: the layout-intent
marker is a **window** option, and only **pane** options travel with a relocated pane, so a broken-out
window inherits global window options and carries no marker until one is set. Without that step the
new window would be invisible to every later overflow search.

Freed capacity in the caller's own window needs no ordering rule: that window is the split target and
is always tried first.

The destination is **reported, not selected**.

Relocating the new pane restores a window that *this placement* pushed below the floor. It does not
repair a window that was already below it — a window crowded by a direct `tmux split-window` or by a
concurrent overshoot stays crowded, and that is surfaced rather than fixed.

Note where that crowding is surfaced. The placement's `below_floor` describes the window the pane
**finally landed in**, which after a successful relocation is not the crowded one — so a relocation
away from an already-crowded origin reports `relocated: true` and `below_floor: false`, and both are
correct. The origin's condition is inspected with `status` pointed at that window, which is exactly
why `status` carries the verdict.

**Key interfaces:**
- The layout module's placement operation gains the trial-and-move stage after measurement, and
  returns which window the pane finally landed in.
- Overflow-candidate ordering is pure given the session's windows and their measured geometries, and
  is unit-testable without a tmux server.
- Both spawning verbs gain a `relocated` verdict recording that a breach forced a move; the reported
  destination window id already exists and now reflects where the pane actually ended up.
- Pane user-options survive a move, so registration and ownership travel with the pane and need no
  re-writing.

**Acceptance criteria:**
- [ ] A placement that would push its window below the effective floor results in the new pane
      residing in a different window, with the origin window's surviving panes retiled. Observable at
      the CLI seam against the private test server: fill a deliberately small window to the floor,
      place once more, and the new pane's window differs from the target's. This assertion fails
      before this change.
- [ ] When an eligible marked window in the session has room, the pane lands there rather than in a
      newly created window.
- [ ] When the lowest-index candidate breaches but a higher-index one has room, the pane lands in the
      higher-index one — a single-candidate attempt is not sufficient.
- [ ] A candidate rejected during the trial is left retiled, not ragged — asserted as **two conjuncts
      in the same run**, on the previous criterion's setup. First: the pane landed in the higher-index
      candidate, which establishes that the lower-index one was actually tried and rejected. Second:
      the rejected candidate's `window_layout` string is identical to what it was before the
      placement. The first conjunct is what gives the second teeth. On its own, "the candidate is not
      ragged" is **already true today** with no relevant code executed at all — nothing in the current
      tree ever perturbs another window, so an untouched candidate is trivially tidy, and a no-op
      trial-and-move stage or one that skips the first candidate entirely would pass. Comparing the
      layout string rather than eyeballing evenness also catches the specific bug this guards: a
      trial that joins, rejects, and forgets to retile on the way out.
- [ ] When no candidate has room, the pane lands in a fresh window that carries the layout-intent
      marker afterwards; querying that option on the new window returns a value.
- [ ] The relocation does not change which window is active. The check has **two conjoined asserts in
      the same run**, and the first is what gives the second teeth: assert the placement actually
      relocated (`relocated` is true and the destination window differs from the target), *then*
      assert the session's current window is unchanged. Without that precondition the criterion is
      green on today's code and on a no-op trial-and-move stage alike — today's tree issues no
      `join-pane`, `break-pane`, or `select-window` at all, and `split-window` can only act within a
      pane's own window, so "current window unchanged" holds in every scenario, breaching or not.
      Pinning only the *setup*, as an earlier round did, guarantees the opportunity to relocate but
      not that the implementation took it. Assert on the session's current window, not on a client's:
      the private test server's session is created detached, so no client exists and any
      client-scoped query returns nothing — vacuous a second way. Both `join-pane` and `break-pane`
      move the session's current window when `-d` is omitted, which is the mistake this catches.
- [ ] Both verbs report `relocated` and the final destination window id. A placement that does **not**
      breach reports `relocated` false and a destination window id equal to the target's — without
      this direction, an implementation that relocates unconditionally passes every other criterion.
- [ ] A window already below its floor before the placement is not repaired — asserted with the same
      relocation precondition as the two criteria above, in one run. First establish that this slice's
      trial-and-move path actually ran (`relocated` is true and the destination differs from the
      target); only then assert the origin window's surviving panes are still below the floor, with
      `status` pointed at that origin reporting it below floor. Without the precondition an
      implementation that never relocates anything satisfies this in full: the blocker slice alone
      makes `status` report a crowded window as below floor, and nothing in the current tree perturbs
      a window this invocation did not target — a crowded window's `window_layout` is byte-identical
      across an unrelated placement. The placement's own `below_floor` is **not** the assertion
      surface here — it describes the window the pane finally landed in, which after a successful
      relocation is a window with room.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Both halves matter.
      `--workspace`: this repo is a two-member workspace and a bare `cargo test` from the root tests
      only the binary package, silently skipping the `core` package — where this slice's
      overflow-candidate ordering and its unit tests live. Compare
      the `Executable` lines printed by `cargo test --no-run` against those printed by
      `cargo test --workspace --no-run`; neither executes a test. Isolation: point `TMUX` (socket
      path, server pid, session id) and
      `TMUX_PANE` at a server started as `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`,
      or use this repo's private harness once it exists. A run sharing the developer's tmux server is
      not a valid baseline — the pane-creating tests fail there, which is the hazard
      ISSUE-260801-1310-01 exists to remove.

**Out of scope:**
- Searching outside the current session for a destination.
- Cross-process locking to prevent concurrent spawns overshooting the floor — a lock cannot cover
  direct `tmux split-window` calls, so it cannot make the floor an invariant regardless.
- Repairing an over-full window after the fact.
- Predicting post-move geometry instead of trialling it.
- Moving an existing pane rather than the newly created one.

## Triage Notes

**Readiness gate (cold-reader): FAIL** (round 1)

Two blocking gaps, both fixed in place before re-gate:

- **Class 9 arm A — the last criterion was unsatisfiable as written.** "A window already below its
  floor before the placement is not repaired, and the placement reports that it is still below floor"
  asks one field to describe two different windows. `below_floor` describes the window the pane
  **landed in**; the whole point of this issue is that on a breach the pane leaves the crowded window,
  so a correct implementation reports `below_floor: false` in exactly the scenario the criterion
  demands `true`. An implementer satisfying it literally would have had to report the origin's
  condition under a field documented to describe the destination. Rewritten onto `status`, which the
  preceding issue gives the verdict to precisely for inspecting a window without placing into it, and
  the field's scope is now stated in Desired behavior so the distinction is not left to inference.
- **Class 7 — the focus criterion asserted against a client that does not exist.** It named "the
  client's current window", but the harness this suite runs on creates its session detached
  (`new-session -d`), so there is no attached client and a client-scoped tmux query returns nothing —
  the assertion would have passed no matter what the code did to focus. Re-pointed at the session's
  current window, which exists in a detached session and is what `join-pane`/`break-pane` change
  without `-d`.

Non-blocking, folded into the same edit: the `relocated` criterion had no negative case and was
satisfiable by an implementation that relocates every placement; it now asserts the non-breaching
direction as well, where `relocated` is false and the destination equals the target.

**Readiness gate (cold-reader): FAIL** (round 2)

Both tmux premises this record rests on were independently verified on a private server, and both hold:
`join-pane`/`break-pane` move the session's current window when `-d` is omitted and leave it alone with
`-d`, and a `new-session -d` session has no attached client.

One blocking gap, fixed in place before re-gate:

- **Class 9 arm A — the focus criterion was still vacuous, one level up from round 1.** Moving it off
  the client fixed the detached-session vacuity but left it unanchored to the scenario it exists for:
  it said only "before and after the placement", and the tree contains no `join-pane`/`break-pane` call
  at all, so *any* placement leaves the session's current window unchanged today. A compliant test
  could assert it against a non-breaching placement and pass with zero relevant code. Now anchored to
  the same fill-to-floor-then-place setup as the first criterion, so the placement under test is one
  that actually relocates.

A second finding — that this record's blockers have not yet reached a `PASS` stamp — is a true
observation about a batch mid-convergence rather than a defect in this brief. Every record in the epic
is being gated in the same pass; the reader confirmed both blockers' current text does deliver what
this record consumes (`status`'s floor verdict from one, the ownership predicate and `@tt-tiled` marker
from the other). It resolves as the batch converges, and nothing here changes in response.

**Readiness gate (cold-reader): FAIL** (round 3)

One blocking gap, fixed in place before re-gate — the third round on the same criterion, and the
distinction the reader drew is the one that finally closes it:

- **Class 9 arm A.** Round 2 re-anchored the focus criterion to the breaching *setup*. That fixed
  toothlessness against a buggy post-change implementation but left the baseline-already-green half
  open, because the setup guarantees the *opportunity* to relocate, not that the implementation took
  it. Today's tree issues no `join-pane`, `break-pane`, or `select-window` anywhere, and
  `split-window` can only act within a pane's own window — verified live — so "session's current
  window unchanged" is true in every scenario, breaching or not, and a no-op trial-and-move stage
  passes this bullet in isolation while failing only the first criterion. The check now carries two
  conjoined asserts in one run: relocation *happened* (`relocated` true, destination ≠ target), then
  focus unchanged. The first is the precondition that gives the second teeth.

The reader was careful to distinguish this from the crowded-origin criterion, which it checked for the
same shape and correctly judged exempt: that one is green-before and green-after against a surface
this slice structurally never touches, which is a genuine preservation guard rather than a hole.

Also corrected here: `cargo test --workspace`, after a sibling gate established that the bare form
does not run the `core` package — where this slice's candidate-ordering logic lands.

**Readiness gate (cold-reader): FAIL** (round 4)

The focus criterion is finally closed, and confirmed by live measurement rather than argument:
`join-pane` and `break-pane` each move the session's current window without `-d` and leave it alone
with `-d`; a detached session has no client; and because the tree contains no cross-window call at all,
`relocated` can never be true today — so the first conjunct is unsatisfiable at baseline and the whole
assertion is red. A total no-op now fails on the first conjunct, and a relocate-but-forget-`-d`
implementation fails on the second. That is what three rounds were reaching for.

One fresh gap, the same defect one bullet over:

- **Class 9 arm A — "a rejected candidate is left retiled, not ragged" was vacuous.** The reader
  reproduced this repo's own Problem Statement to show it: four flagless launches against one window
  until tmux refused with `no space for a new pane`, while a marked candidate window's `window_layout`
  string stayed byte-identical throughout. Nothing in today's tree ever perturbs another window, so an
  untouched candidate is trivially tidy and the criterion is green with zero relevant code — a no-op
  trial stage, or one that skips the first candidate entirely, passes it. Fixed with the same
  two-conjunct shape the focus criterion just received: the pane landed in the *higher-index*
  candidate (establishing the lower one was tried and rejected), and the rejected candidate's
  `window_layout` is unchanged. Comparing the layout string rather than judging evenness by eye also
  targets the exact bug — a trial that joins, rejects, and forgets to retile on the way out.

Worth noting for whoever reads this next: both of this record's vacuity findings have the same root.
Every "the tool left X alone" criterion is green by default in a tree where the tool cannot touch X
yet, so each one needs a companion assertion proving the tool *did* act. That is now true of both.

**Readiness gate (cold-reader): FAIL** (round 5)

One finding, and it **overturns round 3's own reasoning** — correctly. Round 3 examined the
crowded-origin criterion under this same lens and exempted it as a genuine preservation guard, on the
ground that this slice structurally never touches an already-crowded window. That dismissal does not
survive: the criterion is satisfied in full by an implementation that never relocates anything,
because the blocker slice alone makes `status` report a crowded window as below floor, and nothing in
the tree perturbs a window this invocation did not target. Demonstrated rather than argued — a crowded
window's `window_layout` was byte-identical across an unrelated placement by the current binary.

The lesson generalises past "leaves alone": a criterion can also be vacuous because a **blocker**
already guarantees its postcondition. Structural non-touching is not the same as unfalsifiability, and
round 3 conflated them. Fixed with the same relocation precondition the sibling criteria carry, which
is now the record's uniform shape — establish the tool acted, then assert what it left alone.

The reader confirmed every other criterion is self-witnessing under the same lens, and re-verified the
stamp history of both blockers, including that this record's own stamp sequence parses to the intended
authoritative verdict.

**Readiness gate (cold-reader): PASS** (round 6)

No findings. Both lenses were applied exhaustively rather than spot-checked. A scan of the acceptance
block for absence and leaves-alone language found exactly three criteria, all three carrying the
action-precondition — no fourth instance exists. And the blocker-guarantee lens was run across all nine
criteria against both blockers read in full, which produced the useful negative result: the
crowded-origin criterion is the *only* place a blocker already guaranteed the naked postcondition, and
its precondition neutralises that.

One finding there is worth keeping. The fresh-window fallback criterion is **not** covered by the
marker slice's own logic: that slice marks windows through the normal split-and-retile placement path,
never through a `break-pane`-created window. So marking the fallback window is genuinely this slice's
work, which is exactly why the PRD calls it out as not automatic.

The tmux premises were re-derived live once more at gate time — detached sessions have no client,
`join-pane` without `-d` moves the session's current window while `-d` does not, and a window this
invocation did not target keeps a byte-identical `window_layout` across a placement. That last one is
the mechanism round 5's finding rests on, reconfirmed rather than assumed.
