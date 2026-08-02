---
id: ISSUE-260802-0128-01
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Make tile the default placement inside tmux — ownership-gated retile, the @tt-tiled layout-intent marker, sizing opt-outs, and the documentation change
prd: PRD-260801-0656-01
terms: [Tile, Layout intent, Ownership]
blocked_by: [ISSUE-260801-1310-02, ISSUE-260802-0628-01, ISSUE-260801-1310-03, ISSUE-260801-1310-04]
---

## Agent Brief

**Category:** enhancement
**Summary:** A flagless placement inside tmux tiles the window it lands in, gated on ownership and recorded with a durable marker

**Current behavior:**
Inside tmux, a flagless `launch` or `spawn-agent` splits the calling pane horizontally and gives the
new pane 70% of the width. Because sub-agents inherit their orchestrator's environment, they all
resolve the *same* calling pane, so the caller's share compounds down — on a 200-column terminal, 60
columns, then 18, then 5 — until tmux refuses with `no space for a new pane`. Panes also end at
whatever ragged sizes the split history produced.

The layout module now owns placement and reports the geometry it produced, but it does not change it.

**Desired behavior:**
A new placement mode, `tile`, becomes the implicit default inside tmux for both verbs. After the
split, the containing window is retiled with tmux's own tiled layout, then measured — the measurement
seam already exists and is unchanged.

Note what tiling does and does not promise. tmux's `tiled` layout is **row-first** and does not
produce uniformly sized panes: at counts that are not rectangular it stretches the final row across
the full width. At three panes on a 280×82 window the result is 139×40, 140×40, 280×41. The contract
is "tmux's tiled arrangement", not "every pane identical" — so assertions about evenness belong on
**row heights**, not widths.

Three rules govern when a retile happens:

*Ownership is judged from the window as it stood **before** this invocation's split.* The window's
panes and their ownership are snapshotted first, using the shared ownership predicate; eligibility is
decided from that snapshot; only then does the split happen. Without this, splitting into a window we
do not own would tag a pane with our identity and thereby manufacture the authority to reshape that
window — putting a pane somewhere would become the act that grants permission to rearrange it.

*Consequently, tiling engages from the second pane.* The first pane this tool places into a window it
did not already own is placed exactly as before this slice — horizontal split, 70% — with no retile.
That window becomes eligible once it holds a pane we own, so the second and later placements tile.
This is intended: a window with one agent has no crowding to relieve.

*Layout intent is durable, and it is a record rather than a permission.* A window this tool tiles is
marked with a `@tt-tiled` **window** option. It is set when the tool tiles a window, and **unset** —
not set to a false-ish value — when a placement expressing explicit sizing intent targets that
window. Readers test presence, so "cleared" and "never set" are one state.

The marker gates **teardown and overflow**: `kill` does not retile an unmarked window, and an
unmarked window is not an overflow destination. It does **not** gate placement, which is gated by
ownership alone — as it must be, since the first tiling placement into any window necessarily finds
no marker and is what sets one. The consequence is deliberate: after a `--split v` clears the marker,
a later flagless placement into that owned window tiles it again and re-marks it. The alternative
would make one manual split opt a window out permanently; a caller who wants an arrangement kept
keeps passing an explicit direction, and each such invocation re-clears the marker.

Any explicit sizing intent opts out of tiling and is placed exactly as today: `--split h|v` with or
without `--size`, and `--size N` alone — which continues to mean the horizontal split at that
percentage the docs describe today, because a size is meaningless except for a directional split. No
invocation that works today is newly rejected. Opting out changes *placement* only: the invocation
still emits the reporting fields and still clears `@tt-tiled` on the window it targets.

**One combination is rejected: `--split tile --size N`**, at parse, with a diagnostic naming the
conflict. `--size` means "opt out and split at this percentage" while `--split tile` means "tile",
and a tiled layout has no percentage to honour; accepting it would force a silent winner. This is
safe precisely because `--split tile` does not exist before this slice, so no working invocation
starts failing. The conflict is value-dependent — `--size` stays legal with every other `--split`
value — so it is an explicit post-parse validation, not a declarative `conflicts_with`, which cannot
express it.

Two destinations are unaffected because they create a fresh window rather than splitting: the
outside-tmux path, and `--session`/`--window` without `--target`.

Finally, the default change is **documented**. The README, the repo's skill documentation, and the
separately version-controlled installed copy of that skill documentation (path recorded in the PRD's
§ Documentation) all describe the 70% split as the flagless default today. All three are rewritten in
this slice, and the change of default is stated plainly rather than implied. The installed copy is
what agents actually load; leaving it stale would actively teach them that the old default holds.

There is a fourth copy, inside the code: the layout-resolution function's doc comment states the same
70% default in prose. It is the copy the next person to edit that function will read, so it is
rewritten too.

**Key interfaces:**
- The layout module gains the retile stage in its placement operation, and a rebalance operation that
  retiles a window and returns its measured geometry. Rebalance is separate because the kill verb
  needs retiling without placing anything.
- The split-direction enum gains a `tile` value alongside the existing directional and new-window
  values, and it becomes the implicit default inside tmux.
- The flags-to-layout resolution gains the post-parse rejection of `--split tile --size N`. Its
  diagnostic text is delegated to the implementer, bounded by: it must name both flags.
- The ownership question is answered by the shared core predicate, not by a restatement.
- Marker reads and writes are on **window** options; pane options do not carry them.

**Acceptance criteria:**
- [ ] With no layout flags inside tmux, placing a second pane into a window this tool already owns
      retiles the window. Observable at the CLI seam against the private test server on its fixed
      280×82 window: after two flagless spawns the window holds three panes whose **heights** are
      within one row of each other and none of which spans the full window height. Before this change
      all three panes span the full height, so the assertion is red.
- [ ] The same holds when **both** placements are bare `launch --cmd <shell>` invocations — no
      `--name`, no agent, no layout flags. This is the shape the PRD's Solution promises and the one
      the breakdown adversary found undelivered, and it is separately assertable because it depends
      on `ISSUE-260802-0628-01` having made such panes owned: run it and the first criterion against
      a tree with that record reverted, and this one goes red while the first stays green. Do not
      fold the two together — an implementation that tiles only for named or agent panes passes the
      first and fails this one, which is exactly the distinction worth keeping.
- [ ] The first placement into a window the tool did not previously own is a horizontal split giving
      the new pane 70%, with the window left un-retiled — both panes still span the full window
      height. This is a regression guard: green before and after, and it is what makes
      "tiling engages from the second pane" falsifiable rather than a restatement of the first
      criterion.
- [ ] A window containing no pane owned by this invocation is never retiled — verified by placing
      into a window whose panes carry a foreign `@tt-cwd` and asserting its pre-existing pane geometry
      is unchanged. The same test must also place into an **owned** window and assert that one *is*
      retiled, so an implementation that never retiles anything cannot pass.
- [ ] `@tt-tiled` is present on a window after the tool tiles it and absent from a window it has not
      tiled: querying the option on the window returns a value only after a tiling placement, and
      returns none before this change because the option does not exist.
- [ ] Clearing the marker **unsets** it rather than writing a false-ish value — asserted as a sequence
      in one test: tile the window with a flagless placement, **assert the marker is present**, then
      apply a `--split v` placement, then assert querying the option reports it as unset,
      indistinguishable from a window that was never tiled. The present-first assertion is what makes
      this non-vacuous: `@tt-tiled` does not exist today, so "unset after a `--split v`" is already
      true of every window in the current tree, and an implementation that never marks or clears
      anything would satisfy the unset half trivially.
- [ ] A flagless placement into that same owned window afterwards tiles it again and the marker is
      present again — one manual split does not opt the window out permanently.
- [ ] A placement carrying `--split h`, `--split v`, or `--size N` alone leaves the window's pane
      geometry as the pre-slice behavior produces (a preservation guard), and clears `@tt-tiled` on
      the targeted window — the clearing half asserted on a window whose marker the same test first
      established is **present**, for the reason given in the criterion above. Without that, the
      clearing half is green today on any window.
- [ ] `--size N` with no `--split` still produces a horizontal split at N percent and is not rejected.
- [ ] `--split tile` alone is accepted and tiles — it fails before this change, because the value does
      not exist — while `--split tile --size 50` exits non-zero with a diagnostic naming both flags and
      creates no pane.
- [ ] **All three copies state the tiled default** — the presence half, which must cover the installed
      copy too and not only the repo's: `rg -n -i 'tiled' README.md SKILL.md` matches each file, and
      the same search over the installed copy at the path recorded in the PRD's § Documentation
      matches it as well. None of the three match before this change. Without naming the installed
      copy here, its only check is the removal half below, which is satisfied by deleting the sentence
      or replacing it with text that never mentions tiling.
- [ ] No copy still presents the 70% split as the flagless default:
      `rg -n 'default is a horizontal split' README.md SKILL.md` matches each file before this change
      and neither after; and over the installed copy at the path recorded in the PRD's § Documentation,
      `rg -n '70% horizontal split'` matches before this change and not after. The presence half for
      all three copies is the criterion above; this one is only about the old claim going away. The
      installed copy is outside this repository and is edited in place — it is an explicit edit
      target, not a release step.
- [ ] The **fourth** copy — the layout-resolution function's own doc comment — no longer states the
      70% split as the implicit in-tmux default either. It is prose describing this exact decision,
      it sits directly above the function this slice changes, and it is the copy most likely to be
      read by whoever next edits that code. Find it with `rg -n 'occupying 70' src/`, which matches
      before this change and must not after. Note this target is inside the code rather than in a
      documentation file, so the three doc-file criteria above cannot reach it, and the module may
      have moved crates by the time this slice runs — search rather than assuming the path.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Both halves matter.
      `--workspace`: this repo is a two-member workspace and a bare `cargo test` from the root tests
      only the binary package, silently skipping the `core` package — where the layout module this
      slice extends lives. Compare the `Executable` lines printed by `cargo test --no-run` against
      those printed by `cargo test --workspace --no-run`; neither executes a test. Isolation: point
      `TMUX` (socket path, server pid, session id) and `TMUX_PANE` at a server started as
      `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`, or use this repo's private harness
      once it exists. A run sharing the developer's tmux server is not a valid baseline — the
      pane-creating tests fail there, which is the hazard ISSUE-260801-1310-01 exists to remove. A
      failure that survives isolation is **not this slice's to fix** unless this slice introduced it:
      report it and carry on. The PRD's Testing Decisions section owns that rule and names the known
      case, including why it reproduces for some developers and not others.

**Out of scope:**
- Per-agent floors, effective-floor resolution, and the `below_floor` verdict — a breach is not yet
  detected or acted on here.
- Relocating a pane to another window on breach.
- Retiling on `kill`, and the marker's role in gating it — this slice establishes the marker;
  the kill slice consumes it.
- Geometry on the `status` verb.
- Predicting geometry from a model of tmux's tiling algorithm — explicitly rejected; this slice
  applies the layout and reads what tmux produced.
- Reconciling the two divergent copies of the skill documentation permanently. Both are updated here;
  the drift itself is its own work.

## Triage Notes

Carved out of ISSUE-260801-1310-04 in response to that record's round-1 gate, which fired failure
class 6 (epic-in-issue-clothing), prong (b): the original bundled a code move, the default change,
the ownership rules, the marker lifecycle, the flag opt-outs, the reporting surface, and edits to
three documentation files across two repositories under nine acceptance criteria with no single
reviewable decision. The user approved the split. The layout module and measured reporting stayed in
`-04`; everything that changes *where a pane lands* is here, together with the documentation change —
which belongs to whichever slice makes the new statement true, and that is this one.

The round-1 findings that travelled with this material, all fixed in the text above:

- **Class 9 arm A — false-after-the-change geometry assertion.** The original criterion asserted
  "near-equal pane **widths**" after two flagless spawns. That is false after the change, by a fact
  this epic's own PRD records: `tiled` is row-first and leaves a full-width straggler at
  non-rectangular counts — 139×40, 140×40, 280×41 at three panes on 280 columns. Rewritten onto row
  heights, with the pre-change state (all panes full height) named so the assertion is verifiably red
  before.
- **Class 9 arm B — no positive control on the foreign-window criterion.** "A foreign window is never
  retiled" is satisfied by an implementation that never retiles anything. The criterion now requires
  the same test to assert an owned window *is* retiled.
- **Class 4 — `@tt-tiled` "cleared" was ambiguous** between unset and set-to-false, which decides
  whether readers test presence or parse a value, and therefore whether "cleared" and "never set" are
  the same state. Pinned to unset.
- **Class 4 — three rules could not be jointly satisfied for `--split tile --size N`:** "any explicit
  sizing intent opts out", "`--split tile` tiles", and "no combination is newly rejected". Resolved by
  the user: reject that one combination at parse. Recorded upstream in the PRD's § Flags, whose
  "nothing is newly rejected" now reads "no invocation that works today is newly rejected".
- **Class 4 — the marker's scope was under-specified.** "Absent means never touch" read as covering
  placement, which contradicts the marker being set *by* a placement, and left open what a flagless
  spawn does after a manual split cleared it. Resolved by the user: the marker records what the tool
  did and gates teardown and overflow only; placement is gated by ownership, so a later flagless
  placement re-tiles and re-marks. Recorded upstream in the PRD's § Layout intent.
**Readiness gate (cold-reader): FAIL** (round 1)

One blocking gap, fixed in place before re-gate, and one scope question declined:

- **Class 7(a) — decaying-state counts beside discovery commands.** Two documentation criteria
  asserted literal occurrence counts ("returns one match in each file", "returns 2"). The numbers were
  correct at gate time — the reader executed all three doc greps and they matched — but the authoring
  rule forbids counts unconditionally in favour of qualitative polarity, precisely because a count
  decays the moment anyone edits a sentence nearby. Two of the four `rg` criteria in this same brief
  already complied; the other two now do.

- **Class 6 prong (b) — declined, with the reasoning recorded.** The reader observed that the two
  documentation criteria are independently shippable today: they need no tmux server, no layout
  module, and nothing else in this brief, so "rewrite the docs to say tile-by-default" could be its
  own commit right now. That is mechanically true. It is nonetheless not a slice, because shipping it
  alone would make the documentation **false** — it would tell agents the flagless default tiles while
  the code still splits 70:30, which is a worse failure than the staleness this epic is fixing. A
  standalone documentation record would have to be `blocked_by` this one and would then land in the
  same commit anyway, which the reader itself concedes. The rule the split follows is that the
  documentation change belongs to whichever slice makes its statement true, and that is this one. The
  reader surfaced this for a maintainer call rather than asserting it, which is the correct protocol;
  the call is to keep them together.

- **Non-blocking, folded in:** the documentation criterion's `rg` was unscoped ("over the repo"),
  which matched this record and the PRD as well as the docs, and it named no check at all for the
  out-of-repo installed copy — the one agents actually load. Both are now explicit commands with
  their before-counts verified at gate time.

**Readiness gate (cold-reader): FAIL** (round 2)

One blocking gap, and it is the round-1 finding recurring in a place my own fix pass missed:

- **Class 7(a) — a bare count beside a discovery command.** Round 1 removed literal occurrence counts
  from the two documentation criteria; the final `cargo test` criterion carried the same shape
  ("Measured green … 52 unit and 10 integration tests") and I did not catch it while fixing the
  others. The rule is unconditional on brief text under edit regardless of accuracy — and the figure
  was not accurate either, as ISSUE-260801-1310-01's round-3 gate established: 52 was the binary
  crate's subtotal, and a bare `cargo test` from the repo root never runs the `core` package at all.
  Replaced with `cargo test --workspace` and a command that shows the difference. The reader swept
  every other numeric surface in the record and confirmed each is either a PRD-provenanced spec figure
  or inert narrative inside Triage Notes.

Everything else was re-derived rather than trusted. The reader built the binary, ran two flagless
`launch` invocations against a private 280×82 server, and measured `24x82`, `58x82`, `196x82` — all
three panes at full window height, confirming the first criterion's stated baseline is genuinely red.
It also confirmed `--split tile` is currently rejected by clap, and that querying an unset `@tt-tiled`
returns the same result before ever setting it and after `set -u`, which is what makes "cleared and
never set are one state" checkable.

**Readiness gate (cold-reader): FAIL** (round 3)

Two blocking gaps, both fixed in place before re-gate. The first came from applying the
leaves-alone lens systematically, which is why it surfaced two instances the earlier rounds missed:

- **Class 9 arm A — the marker-clearing criteria were vacuous, twice.** "After a `--split v`, querying
  `@tt-tiled` reports it unset" is **already true of every window in the tree**, because the option
  does not exist yet; an ordinary `tmux split-window` leaves it just as absent. So an implementation
  that never marks or clears anything satisfied both the clearing criterion and the clearing half of
  the sizing-opt-out criterion. Fixed by requiring the same test to first tile the window and assert
  the marker is **present**, then apply the explicit-flag placement, then assert unset — the same
  pairing the foreign-window criterion already had, which the reader re-checked and confirmed genuine.
- **Class 4 — no disposition for a failure surviving isolation.** The criterion said where to run the
  suite but not what to do when it fails for reasons outside the slice. It now states the rule and
  points at the PRD's Testing Decisions, which owns it.

The second finding arrived with a root-cause diagnosis worth more than the finding. The reader hit
`launch_keeps_pane_alive_after_cmd_exit` failing **deterministically** on two independent private
sockets — where for me it had been an occasional flake — and traced it: the keep-alive wrap execs
`$SHELL`, so the test runs the developer's interactive shell startup inside the pane, and a redrawing
prompt overwrites the content before `capture` reads it. That explains the environment-dependence, and
exposes an isolation axis this epic does not close — the harness suppresses tmux's user config but not
the shell rc files the wrap then loads. Recorded in the PRD.

The reader also self-reported that its first suite run accidentally used the inherited shared server
before it exported a private socket, and confirmed it left nothing behind. Volunteering that is the
right instinct — it is exactly the mistake this epic's first slice exists to make impossible.

**Readiness gate (cold-reader): FAIL** (round 4)

Both round-3 fixes verified closed by re-execution, not by reading: the marker sequence now fails at
the present-check before it ever reaches the clear-check, and the disposition rule was exercised for
real — the reader hit `full_smoke` failing on an unrelated `execute` timeout under isolation, a
*different* test from the one the PRD names, which is good evidence the rule is written generally
rather than scoped to one known case. That second case is now recorded upstream too.

One new gap, the same shape as round 3's in a place neither of us had looked:

- **Class 9 — the installed documentation copy had only a removal check.** The repo copies get an
  explicit presence check (`rg -i 'tiled'`) alongside the removal one; the installed copy got the
  removal half plus a prose clause saying it should state the tiled default "instead". Prose is not a
  check: deleting the sentence, or replacing it with text that never mentions tiling, satisfies every
  named command. And the installed copy is the one agents actually load, so it is the worst place for
  the weaker assertion. The presence criterion now covers all three copies explicitly; the reader
  confirmed the search is genuinely red against the installed path today.

That makes three separate instances in this record of the same defect — absence proven, presence
assumed — found across three rounds. The pattern is now stated plainly in the criteria themselves so a
fourth instance would be visible on reading rather than needing a gate round to surface it.

**Readiness gate (cold-reader): PASS** (round 5)

No findings, after a systematic per-criterion sweep of all twelve bullets against both lenses rather
than another opportunistic hunt. No fourth instance of the absence-without-presence shape exists; the
reader named the three most plausible remaining candidates, examined each, and explained why each is
genuinely paired, self-witnessing, or ordinary implementation freedom rather than a masking defect.
Reporting *where it looked and found nothing* is what makes that a useful negative result.

The blocker lens came back clean for a structural reason worth recording: this record's blocker
explicitly lists every one of this slice's surfaces in its own Out-of-scope — the tile mode, the
default change, the ownership snapshot, the marker, the sizing opt-outs — so the two records partition
rather than overlap, and nothing here arrives pre-guaranteed.

Every baseline was re-derived live rather than read: two flagless launches producing `24x82 / 58x82 /
196x82` (all full height, and a faithful reproduction of the compounding shrink this epic exists to
fix), `@tt-tiled` reporting `invalid option`, `--split tile` rejected by clap, `--size 40` yielding
exactly 40%, and all three documentation copies matching their stated before-state. The reader also
confirmed the installed copy is a genuinely separate git repository, which is the premise behind
treating it as an explicit edit target.

**Readiness gate: REOPENED** (round 5)

Reopened by the step-7 adversarial breakdown pass, which found the gap this record sat closest to
without owning. The PRD's Solution promises that "a flagless `launch` or `spawn-agent` … will tile
instead", but every criterion here was satisfiable by tiling only for panes carrying `@tt-name` or
`@tt-agent` — and a bare `launch` registered neither, so repeating it would have kept the compounding
30:70 shrink forever, on the invocation shape the README documents first. The round-5 reader had in
fact measured that exact baseline (`24x82 / 58x82 / 196x82` from two flagless launches) and read it
as a faithful reproduction of the bug, which it is; what no single-record gate could see is that
nothing in the set turned those panes into ones the tool would retile.

The user's remedy is `ISSUE-260802-0628-01` — register a name on every pane this tool creates —
which is now a blocker, since without it the new criterion cannot go green. That criterion is
deliberately separate from the first rather than folded into it: an implementation that tiles only
for named or agent panes passes the first and fails the new one, and collapsing them would discard
the distinction the adversary paid to find.

**Readiness gate (cold-reader): PASS** (round 6)

No blocking findings. The reader confirmed the reopening edit does what it claims on all three counts
it was asked about. The new criterion is genuinely separable: criterion 1's precondition is
satisfiable today via `spawn-agent` or `launch --name`, so an implementation deciding eligibility
from the *invocation* rather than from the pane-registration snapshot passes it and fails the new
one. The blocker is genuinely required — bare-launch panes were measured carrying only `@tt-cwd` and
`@tt-launched-at`, and the shared predicate needs a name or an agent, with the PRD forbidding a
second looser predicate. And every criterion was executed for its baseline: three full-height panes
at `24x82 / 58x82 / 196x82`, `@tt-tiled` reporting `invalid option`, `--split tile` rejected by clap
with a message naming only `--split`, and zero `tiled` matches in each of the three documentation
copies.

It also verified the documentation inventory is exact rather than assumed — the installed copy and
the path under `~/.claude/skills` are the *same inode*, so there is no fourth loaded copy, and that
file is tracked in a different git repository from this one.

**One edit postdates this reader's read**, disclosed here rather than folded in silently: the same
report noted that the PRD names a fourth 70%-default statement — the layout-resolution function's own
doc comment — which the three documentation criteria structurally cannot reach, since they search
documentation files and this one is inside the code. A criterion for it is now present, and its
command was confirmed red at baseline (`rg -n 'occupying 70' src/` → one match). The addition is a
fourth item in an existing enumeration and introduces no new decision; a re-gate round for it would
have cost more than it bought. Flagged so the next reader knows which text this stamp did not cover.

Non-blocking, not acted on: criterion 1 does not pin whether its two placements are named or agent
spawns, so an implementer writing both as bare launches would make the new criterion's revert
demonstration yield two reds instead of one red and one green. The union of the two criteria pins the
same behavior either way, so no build forks on it.
