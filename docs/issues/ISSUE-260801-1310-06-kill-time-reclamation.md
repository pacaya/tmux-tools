---
id: ISSUE-260801-1310-06
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: kill retiles a tool-tiled window so survivors reclaim the removed pane's space
prd: PRD-260801-0656-01
terms: [Layout intent, Ownership, Exited-agent pane]
blocked_by: [ISSUE-260802-0128-01]
---

## Agent Brief

**Category:** enhancement
**Summary:** Removing a pane from a window this tool tiled re-evens the survivors

**Current behavior:**
`kill` destroys the target pane and stops. tmux hands the freed space to whichever neighbour can
absorb it, so the window keeps its previous row count and at least some survivors stay at the density
the larger pane count implied. Measured on a private server at 280×82: six tiled panes are ~140×26–28
each; after two are killed, which survivors grow depends on which panes went — some pairs leave two
survivors at 28 rows and two at 53 — but across every pair tried, at least one survivor was still
≤28 rows. Retiling the four gives them all 40–41.

That gap persists until something else happens to retile the window, which in a wave-based workflow
is exactly the interval when results are being read.

**Desired behavior:**
When a pane is removed from a window this tool tiled and owns, the window is retiled so the
survivors reclaim the space.

Two guards decide whether that happens, and both must be read **before** the pane is destroyed:

*Layout intent.* Only windows carrying the `@tt-tiled` marker are retiled. A window someone
arranged by hand has no marker, and flattening it into a grid because a pane in it exited would
make the documented opt-out survive placement but not teardown.

*Ownership.* The window must contain a pane owned by this invocation, using the shared predicate.

Reading both before the kill matters because pane options vanish with the pane: if the pane being
destroyed holds the window's only ownership evidence, evaluating afterwards would forbid the
rebalance at precisely the moment reclamation is due.

An earlier design retiled only when the tiled row count dropped, on the reasoning that nothing
changed geometrically otherwise. That is false — six panes form three even rows while five leave a
full-width straggler — so the window is retiled on every removal from an eligible window. Retiling
after a removal can only enlarge survivors, so it cannot push a window below its floor.

Two cases fall outside the rule and must not be treated as failures:

*The last pane.* Killing a window's only pane destroys the window along with it, so there is no window
left to rearrange and no survivors to reclaim anything. The retile is skipped, silently — attempting
it would address a window that no longer exists.

*`--any` does not reach the layout guard.* The override governs **destruction** — it is what lets a
caller kill a pane that is not theirs or was created from another cwd — and it stops there. The retile
decision is evaluated separately and still requires the window to be marked *and* to hold a pane this
invocation owns. So `kill --any` on a stranger's pane destroys that pane and leaves the stranger's
window arranged exactly as it was. Widening `--any` to the layout guard would take the override's
blast radius from one pane to a whole window the caller never had a claim on; and because the tool's
own panes never need `--any`, nothing legitimate loses reclamation.

Separate is **orthogonal**, not merely weaker, and both directions are load-bearing. `--any` grants
no layout permission: on a stranger's window the guard still answers no. It also revokes none: when
the guard answers yes on its own — the window is marked and holds a pane this invocation owns — the
retile happens, whether or not `--any` was passed. That second direction is not a corner case; it is
the only way to reach a pane the safety evaluator refuses. An unowned pane cannot be killed without
`--any` at all, so the mixed-window case below is reachable only through the override, and an
implementation that treats `--any` as suppressing the retile would get that case wrong while looking
correct on every other.

**Key interfaces:**
- The kill verb gains a dependency on the layout module's rebalance operation. It must not gain a
  dependency on the launch verb; the shared floor beneath both is the layout module.
- The safety evaluation the verb already performs answers a **different, narrower question** than the
  layout guard needs, and this is the trap in this slice. It resolves registration for the *target
  pane only* and returns that one record; the layout guard asks whether the *window* contains any pane
  this invocation owns. Those diverge in a case that really occurs: killing an unowned pane from a
  window where a sibling **is** owned — the window is eligible, but the target's own record says
  not-owned. Capturing the safety verdict and reusing it as the layout answer would skip the retile
  exactly there.
- So the pre-destruction read must **enumerate the window's panes** and evaluate ownership across them
  with the shared predicate, not lift the target's verdict. It still has to happen before the kill, for
  the reason below. Reading the layout-intent marker is a window-option read and is unaffected.

**Acceptance criteria:**

A constraint that binds **every** criterion below asserting a window *is* retiled, and is easy to
miss: for many fixtures a retile changes nothing, so such a criterion is green before and after and
proves nothing.

What decides it is **which pane is removed relative to the tiled grid, not how many survive.** Killing
a pane whose absence leaves the remaining panes already in tiled positions gives a retile nothing to
do. Measured on a private server at 280×82, sweeping every kill position from three panes to six:
at three, killing either top pane leaves what a retile would produce, while killing the full-width
bottom pane does discriminate; at four, killing either bottom-row pane is already exactly the tiled
layout; at six, likewise for the bottom row. So a survivor-count rule cannot express the property —
five survivors of a six-pane window is a no-op case — and any criterion phrased as a minimum count
will certify fixtures that prove nothing.

**Where a criterion states no fixture of its own, use five panes and kill one.** That was measured to
discriminate at all five kill positions, which is why it is the default rather than an arbitrary
choice. Assert the survivors' geometry against an explicit retile rather than by eye. Criterion 1
states its own fixture — six panes, kill two — verified discriminating across all fifteen pairs.

Do not substitute a different fixture without re-measuring it the same way, position by position.
This hazard bit criterion 1 at round 1, and it bit the first draft of this very paragraph at round 8,
which asserted a survivor-count minimum derived from measuring a single kill position per pane count.
- [ ] Killing a pane from a marked, owned window leaves the survivors retiled. Observable at the CLI
      seam against the private test server on its fixed 280×82 window: place six panes into a tiled
      window, kill two, and each of the four survivors is at least 40 rows tall. Red before this
      change for any choice of which two panes are killed: without the retile at least one survivor is
      still ≤28 rows, verified by sweeping the kill-pairs on a private server.
- [ ] Killing a pane from a window with no `@tt-tiled` marker leaves the remaining panes' geometry
      exactly as tmux left it — a hand-arranged window is not flattened. The control this is paired
      with must be a **mechanical copy of the same fixture differing in the marker alone**: same pane
      count, same starting geometry, and — this is the part that is easy to get wrong — **the
      unmarked window must also contain a pane this invocation owns**. Set the marker on one, leave
      it unset on the other, kill the corresponding pane in each, and assert the marked window's
      survivors are retiled while the unmarked window's are not. Build the unmarked window from raw
      panes instead and ownership, not the marker, explains the difference: an implementation that
      gates on ownership alone and never reads `@tt-tiled` then passes this criterion — and it passes
      every other criterion in this brief too, so nothing here would catch it. Layout intent is one
      of this slice's two guards; this is the only criterion that tests it.
- [ ] Killing the pane that carries the window's only ownership evidence still retiles the window,
      demonstrating both guards are evaluated before destruction.
- [ ] `kill --any` on an **unowned** pane in a marked window where a *different* pane is owned still
      retiles the window. The `--any` is mandatory and is not a detail of convenience: the safety
      evaluator refuses an unregistered target outright, so without the flag nothing is destroyed,
      "did not retile" is trivially true, and the criterion is green in both directions while
      discriminating nothing. The flag does not weaken the layout guard — it governs destruction only
      (§ Desired behavior), so the guard is still evaluated on the window's own panes and here answers
      yes. This is the case that separates the window-level guard from the target-pane safety verdict:
      the target's own registration says not-owned, while the window is eligible. An implementation
      that reuses the safety evaluator's answer for the layout decision fails here and passes every
      other criterion.
- [ ] `kill --any` on a pane in a window owned by a different working directory destroys the pane and
      does **not** retile that window — the surviving panes' geometry is unchanged. Three things make
      this criterion non-vacuous, and all three are required. The `--any`: without it the safety
      evaluator denies the kill outright, so nothing is destroyed and "did not retile" is trivially
      true of today's code. A positive control in the **same test** — otherwise an implementation
      whose retile step is a permanent no-op satisfies this criterion exactly as well as a correct
      one. And the fixture window must be **marked** — a foreign window that is marked is the
      realistic case anyway, since another project's run tiled it. Produce the control **mechanically
      from that fixture**: build it with the same construction path, the same commands, and the same
      flags — `--any` included, even though an owned target does not need it — changing exactly one
      thing, the recorded `@tt-cwd`. Do not specify it by listing dimensions it must match. An
      enumeration leaves every dimension nobody thought of free, and this criterion has already been
      through that: its previous form listed marker, pane count, and geometry, and the flag turned out
      to be a fourth dimension outside the list, which is precisely how such a list fails.
      Leave the fixture unmarked and the marker, not ownership, explains the absent retile, so an
      implementation in which `--any` *does* reach the layout guard — the precise defect this
      criterion exists to exclude — passes it.
- [ ] Killing a window's **last** pane succeeds and exits zero: the window is gone and no error about
      a missing window is emitted. This is a **regression guard** — it passes before and after, since
      today's `kill` attempts no retile and so has nothing to fail on a destroyed window. It is here
      to catch this slice's own new failure mode: a retile issued against a window that tmux has
      already removed.
- [ ] The verb's existing safety guards, denial messages, and output are unchanged; its existing
      tests pass unmodified.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Both halves matter.
      `--workspace`: this repo is a two-member workspace and a bare `cargo test` from the root tests
      only the binary package, silently skipping the `core` package — where the layout module this
      slice calls into lives. Compare the `Executable` lines printed by `cargo test --no-run` against
      those printed by `cargo test --workspace --no-run`; neither executes a test. Isolation: point
      `TMUX` (socket path, server pid, session id) and `TMUX_PANE` at a server started as
      `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`, or use this repo's private harness
      once it exists. A run sharing the developer's tmux server is not a valid baseline — the
      pane-creating tests fail there, which is the hazard ISSUE-260801-1310-01 exists to remove.

**Out of scope:**
- Reclaiming panes whose agent has exited but which the keep-alive wrap holds open — they count
  toward geometry like any other pane and are never destroyed automatically, because that would
  discard the crash output the wrap exists to preserve. Driver skills already kill their own panes.
- Retiling on `interrupt`, `escape`, or `send-enter`, none of which remove a pane.
- Any change to which panes `kill` is willing to destroy — `--any` keeps exactly the reach it has
  today over destruction, and gains none over layout.
- Repairing a window that was already below its floor before the kill.

## Triage Notes

**Readiness gate (cold-reader): FAIL** (round 1)

Two blocking gaps, both fixed in place before re-gate:

- **Class 9 arm A — the foreign-cwd criterion was vacuous.** "Killing a pane from a window owned by a
  different working directory does not retile it" is satisfied by today's code: the safety evaluator
  denies that kill outright, so no pane is destroyed and no retile could occur regardless of what this
  issue builds. It could never go red. It also left `--any` undecided, which is the flag that makes
  the scenario reachable at all. Resolved by the user: `--any` governs destruction only and does not
  reach the layout guard. The criterion now passes `--any`, so the kill actually happens and the
  no-retile assertion has something to be false about. Recorded upstream in the PRD's § Reclamation
  on kill.
- **Class 9 arm B — no positive control on the unmarked-window criterion.** "An unmarked window is not
  retiled" is satisfied by an implementation that never retiles anything, which is also what the
  pre-change code does. The criterion now requires the same test to assert a marked window *is*
  retiled.

Non-blocking, folded into the same edit:

- The first criterion's geometry assertion was "near-equal geometry" over four panes killed down to
  three. At 280×82 that is green before the change — tmux hands the freed space to a neighbour and the
  survivors' heights stay within a row of each other anyway — so it could not detect the defect.
  Replaced with the PRD's own measured six-panes-kill-two case, where the retiled survivors reach
  ~41 rows against the 26–28 the un-retiled window leaves.
- Killing a window's last pane was unaddressed. tmux destroys the window with the pane, so a retile
  attempted afterwards would target a window that no longer exists; the case is now explicitly a skip
  with a criterion asserting it exits clean.

**Readiness gate (cold-reader): FAIL** (round 2)

Three findings, all fixed in place before re-gate. The reader verified the geometry claims itself on a
private server rather than accepting them, which is how the first one surfaced:

- **Class 7(a) / class 1 — a false citation, and a figure that was not generally true.** The first
  criterion attributed the six-panes-kill-two measurement to the PRD. It is not there: the PRD's only
  remark on the subject is the qualitative "going from six panes to five". Worse, the figure itself
  overstated uniformity — a kill-pair sweep on a private server shows some pairs leave two survivors
  at 28 rows and two at **53**, not "all four at 26–28". The criterion's actual test survives intact,
  because at least one survivor is ≤28 for every pair tried while a retile gives all four 40–41, so
  "each survivor ≥40" is red before and green after regardless of which panes are killed. Attribution
  dropped, the claim restated as what was actually measured, and the criterion re-anchored on the
  minimum rather than on all four.
- **Class 9 arm B — the `--any` criterion had no positive control.** Round 1 added one to the
  unmarked-window criterion and I failed to carry it across: an implementation whose retile step is a
  permanent no-op satisfied the foreign-cwd criterion exactly as well as a correct one. The same test
  now also kills from an owned, marked window and asserts those survivors *are* retiled.
- **Class 9 arm A, surfaced rather than silently exempted — the last-pane criterion is already true
  today**, verified by running the current binary against a private server: `kill --force` on a
  solo-pane window exits zero, the window is gone, no error. Correct, since today's `kill` attempts no
  retile. It is now labelled a regression guard, matching the convention this epic's sibling records
  use, and states the new failure mode it exists to catch — a retile issued against a window tmux has
  already removed.

**Readiness gate (cold-reader): FAIL** (round 3)

The round-2 fixes all held, and the geometry claim was re-derived rather than accepted: the reader
swept **all fifteen** kill-pairs on a private 280×82 server. Six tiled panes measure 139–140×26–28;
every pair leaves a minimum survivor height ≤28 without a retile, with the `[28, 28, 53, 53]` shape
appearing exactly as the brief describes; and retiling the surviving four yields `[40, 40, 41, 41]` in
all fifteen cases. So the first criterion is red before and green after for every choice of panes —
but note the margin: the retiled minimum is **exactly** 40, so the `≥40` bar has none. That is correct
today and worth knowing if a future tmux changes its row arithmetic.

One blocking gap, fixed in place before re-gate:

- **Class 7(a) — a bare count with no discovery command.** The final criterion carried "Measured green
  on an isolated server 2026-08-02 — 52 unit and 10 integration tests". The rule against decaying
  figures is unconditional on brief text under edit regardless of accuracy — and here the figure was
  not even accurate, as ISSUE-260801-1310-01's round-3 gate independently established: 52 was the
  binary crate's subtotal, and a bare `cargo test` from the repo root never runs the `core` package at
  all. The reader also spotted that the sentence was a template phrase repeated across sibling records
  rather than a defect local to this one, which is exactly right. Counts are gone in favour of
  `cargo test --workspace` plus a command that demonstrates the difference.

**Readiness gate (cold-reader): FAIL** (round 4)

One blocking finding, and it is a form defect rather than a factual one — the reader verified the
underlying claim is true (a bare `cargo test` never builds the core crate's test binary; `--workspace`
does) before objecting to how the brief demonstrated it:

- **Class 7(b) — the discovery command failed the structural screen.** It was written as
  `cargo test 2>&1 | rg '^     Running'`, and the rubric requires discovery commands to be free of
  redirects and compound commands. The `2>&1` is unambiguous; whether a bare pipeline also counts is
  arguable, and the reader said so rather than overclaiming. Replaced with a comparison of the
  `Executable` lines printed by `cargo test --no-run` versus `cargo test --workspace --no-run` — one
  command each, no redirect, no pipe, and nothing executed, so it is safe to run anywhere.

The same defect was in three other records; all four were fixed in one sweep rather than one gate round
at a time, since it came from a phrase I had propagated.

The reader also confirmed the round-3 count is gone from every live surface, and that the geometry
figures are spec-figures reproduced by round 3's fifteen-pair sweep rather than repo-state counts, so
they need no in-brief discovery command.

**Readiness gate (cold-reader): PASS** (round 5)

No findings. The replacement command was checked for form and then executed as written: the bare
`cargo test --no-run` prints two `Executable` lines, the `--workspace` form prints three, the extra
being the core crate's unit-test binary — and no warning accompanies the bare form, which is what makes
"silently skipping" the right word.

Rather than repeat round 3's fifteen-pair sweep, the reader re-derived the geometry from a fresh angle
on a private server, mirroring what today's `kill.rs` actually does (kill-pane only, no retile):

```
before kill:                 139x26 140x26 139x26 140x26 139x28 140x28
after killing two, no retile: 139x53 140x53 139x28 140x28   → min height 28
after explicit select-layout tiled: 139x40 140x40 139x41 140x41
```

Which is the first criterion's red-before and green-after, measured rather than argued.

**Readiness gate: REOPENED** (round 5)

Reopened by the step-7 adversarial breakdown pass, on a scope mismatch between two things this brief
treated as interchangeable.

The layout guard is **window-level** — "the window must contain a pane owned by this invocation" — but
the brief pointed at the verb's existing safety evaluation as the natural place to capture that answer,
and that evaluation is **target-pane-level**: it resolves registration for the single pane being killed
and returns that one record. It neither identifies the containing window nor enumerates siblings.

The two answers diverge in a case no criterion covered: killing an unowned pane from a marked window
where a *different* pane is owned. The window is eligible; the target's own record says not-owned. An
implementation that reuses the safety verdict as the layout answer skips the retile exactly there — and
would have passed every criterion this record carried, including both positive controls, because each
of those uses a target that is itself owned.

Fixed by stating that the pre-destruction read must enumerate the window's panes rather than lift the
target's verdict, and by adding the mixed-window criterion that separates the two. Re-gating.

**Readiness gate (cold-reader): FAIL** (round 6)

Three blocking findings, all fixed in place before re-gate. The first was introduced by the round-5
reopening edit itself; the other two had survived five prior rounds.

- **Class 4 — the new mixed-window criterion could not be executed at all.** It said to kill an
  *unowned* pane, but the safety evaluator refuses an unregistered target outright, so the kill never
  happens: red before and red after, discriminating nothing. Reproduced live — `kill --target %1` on
  an unregistered pane exits 1 with the "not created by tmux-tools" denial, and only `--any` gets
  past it. This is the same defect round 1 found in the foreign-cwd criterion and fixed there by
  naming `--any`; the reopening edit reintroduced it in a new criterion without noticing. Fixed by
  requiring `--any` and saying why it is mandatory rather than incidental.

  Naming the flag forced an ambiguity into the open that the brief had left implicit: whether `--any`
  suppresses the retile. It does not. The already-settled rule is that `--any` governs destruction
  only and the layout guard is evaluated independently — which the brief stated in one direction
  (a stranger's window is left alone) but not the other (a window the guard approves on its own is
  retiled regardless of the flag). Both directions are now stated. This applies the settled rule
  rather than deciding anything new, but it is the sort of corollary worth reading twice.

- **Class 9 arm B — the unmarked-window criterion's positive control was specified only as "a marked
  window".** That leaves the unmarked fixture's *ownership* free, and the natural reading — a
  hand-arranged window built from raw `tmux split-window` panes — is unowned. So an implementation
  gating on ownership alone and never reading `@tt-tiled` passes it. The reader then checked that
  implementation against the whole criteria set and found it passes every other criterion too,
  meaning layout intent — one of this slice's two named guards — had no test at all. Fixed by
  requiring a mechanical copy differing in the marker alone, with the unmarked window explicitly
  also containing an owned pane.

- **Class 9 arm B — the `--any` foreign-cwd criterion had the same defect.** Its fixture's marker
  state was unspecified while its control differed in both marker and ownership. If the foreign
  window is unmarked, the marker explains the absent retile and an implementation in which `--any`
  *does* reach the layout guard still passes. Fixed by pinning the fixture as marked so ownership is
  the only varying dimension.

Every figure in the brief was re-derived rather than read: the reader re-ran the full 15-pair kill
sweep on an isolated server and reproduced both halves — minimum survivor ≤28 rows without a retile
for every pair, and `139x40 140x40 139x41 140x41` after one, confirming the stated zero margin at
exactly 40. It also confirmed no retile machinery exists anywhere in the tree, so every
retile-asserting criterion is genuinely red at baseline, and verified the last-pane regression guard
holds today by killing a solo pane and watching the window disappear cleanly.

Non-blocking, not acted on: `blocked_by` names only `ISSUE-260802-0128-01` and not
`ISSUE-260801-1310-02`, whose shared predicate this brief calls into. The dependency is real but
transitive through the declared blocker, which is this breakdown's established convention —
`ISSUE-260801-1310-07` omits the same edge for the same reason.

**Readiness gate (cold-reader): PASS** (round 7)

All three round-6 repairs verified closed, each against the code rather than the argument. The
mixed-window scenario is reachable only through `--any` — reproduced live, denied without it and
succeeding with it — and the "grants none, revokes none" claim was checked for coherence against the
PRD, this brief's Out of scope, and criteria 4 and 5 read together, with no contradiction found. A
marker-blind implementation now retiles both windows and so fails criterion 2. An implementation in
which `--any` reaches the layout guard now fails criterion 5.

The 15-pair kill sweep was re-run independently for a second time, rebuilding the six-pane window for
each pair: every pair leaves a minimum survivor ≤28 rows without a retile and every pair retiles to
min exactly 40, with the `[28,28,53,53]` shape appearing at the two pairs the brief names.

**Readiness gate: REOPENED** (round 7)

Reopened immediately by the same report's sharpest non-blocking observation, which is worth more than
its label. The reader measured that at three panes a retile is a **no-op** — killing one pane from a
three-pane tiled window leaves `280x40 280x41`, and an explicit retile produces exactly the same
geometry — while at four and five panes they differ. Criteria 3 and 4 assert a window "still retiles"
without pinning a fixture size, and the only observable available is geometry, so an implementer
choosing the minimal natural fixture would write criteria that are green before and after and prove
nothing. The rubric routes this out of class 9, which is why it came through as an observation rather
than a finding; it is nonetheless the same hazard that bit criterion 1 at round 1, in a brief whose
entire subject is whether a retile happened.

Fixed by hoisting a constraint above the criteria list that binds every "is retiled" assertion:
minimum four panes after the kill, five and kill one where a criterion states no fixture of its own,
asserted against a retile rather than by eye. Also folded in: criterion 5's control must pass `--any`
too, since the dimension list names only window properties and a control without the flag differs
from the fixture in two — letting an implementation where `--any` suppresses the retile pass with the
flag, not ownership, explaining the result.

The PRD was edited alongside, for the same report's observation 4: it carries the "stranger's pane"
sentence without the paragraph that disambiguates it, so a PRD-only reader could take it to mean an
unowned *target* suppresses the retile — which criterion 4 contradicts. The scoping is now explicit
there.

Re-gating at round 8; none of this text has been read by a gate.

**Readiness gate (cold-reader): FAIL** (round 8)

Two blocking findings. The first is the round-7 repair itself — the fix for a non-blocking
observation was unsound, and it took a full sweep to see it.

- **Class 7(c), build-changing — the hoisted constraint's threshold was false.** Round 7 measured one
  kill position per pane count and concluded "at three panes a retile is a no-op, at four and five it
  is not", from which the paragraph derived a minimum of four survivors. Round 8 swept *every* kill
  position at three through six panes and the picture is different: at three panes killing either top
  pane is a no-op but killing the full-width bottom pane discriminates; at four panes killing either
  bottom-row pane is already the tiled layout; at six panes the same. So six panes killing a
  bottom-row pane leaves five survivors — satisfying the stated minimum — with the retile a no-op.
  The paragraph hoisted to prevent exactly that defect was certifying it as safe.

  The property is not survivor count at all. It is which pane is removed relative to the tiled grid,
  which no count-based rule can express. The paragraph also mixed units, counting panes before the
  kill in its evidence and after the kill in its mandate, with no reading on which it was both
  self-consistent and true. Rewritten around the measured property, keeping the two fixtures that
  survive the sweep: five panes killing one discriminates at all five positions, and criterion 1's
  six-panes-kill-two discriminates across all fifteen pairs.

- **Class 9 arm B — criterion 5's control was specified by enumeration rather than as a mechanical
  copy.** The rubric admits only a control produced mechanically from the fixture and differing in
  one dimension; a list of dimensions it must match leaves every unlisted dimension free, and the set
  of those is open. Criterion 2, two criteria earlier, already carried the compliant form, so the
  divergence was not a wording slip. The round-7 edit is itself the proof: its stated reasoning was
  that the list "names only window properties", i.e. it found one unenumerated dimension and closed
  it by name — patching one hole in an open set. Rewritten as a mechanical copy of the fixture
  differing in the recorded cwd alone, same construction path and same flags. The `--any`-on-control
  requirement survives inside that framing, and the reader independently confirmed it introduces no
  confound: the flag only relaxes the two denial arms, so on an owned in-cwd target it changes
  nothing.

The PRD edit from round 7 was verified correct and closing what it claimed, consistent with criteria
4 and 5, story 13, and the marker-read-before-destruction rule. All pre-existing geometry figures
reproduced independently for a third time.

**Readiness gate (cold-reader): PASS** (round 9)

Both round-8 repairs verified closed, by measurement rather than by argument. The reader swept every
kill position at three through six panes and confirmed each descriptive clause in the rewritten
constraint paragraph is exactly right — including the six-pane bottom-row counter-example the
paragraph cites against itself — then re-ran the five-panes-kill-one default under a *second*
construction path (all splits first, one final retile, rather than retiling after each split) and
found it still discriminates at all five positions. Criterion 1's fixture discriminates in all 30
ordered pairs. The paragraph was checked for the unit confusion that sank its predecessor and states
both counts explicitly wherever survivor count matters. It certifies no fixture that fails to
discriminate.

Criterion 5's control now satisfies arm B as a mechanical copy, and the reader confirmed the
`--any`-on-control requirement introduces no confound by reading the evaluator: the flag appears only
inside the two denial conditions, so on an owned in-cwd target it changes nothing. Criterion 2's
control was re-checked and is undisturbed. Fixture-blindness on both negative halves closes
transitively — neither states a pane count, so the discriminating default applies, and "mechanical
copy" plus "kill the corresponding pane in each" forces the negative window onto the same fixture.

Two things to carry forward, neither owed an edit:

- **The round-7 REOPENED note above still contains the superseded rule** — "minimum four panes after
  the kill", and "at three panes a retile is a no-op" as a generalization, which the round-8 sweep
  falsifies. It is history, and the round-8 entry directly beneath corrects it explicitly, so it is
  not stranded spec. It is nonetheless the one place in this record where a skimmer could pick up a
  false rule; read the two entries together or not at all. Left in place deliberately: the record of
  how a plausible fix turned out to be wrong is worth more than a tidy file, and rewriting history to
  match the current answer is how the next reader loses the warning.
- Criterion 1's `≥40` bar has zero margin — the retiled minimum is exactly 40 on tmux 3.7b at 280×82,
  now confirmed across all 30 ordered pairs. A change in tmux's row arithmetic flips that criterion
  without the feature having regressed.
