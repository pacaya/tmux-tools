---
id: ISSUE-260801-1310-04
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Core layout module owning placement, and measured destination geometry in the spawning verbs' output
prd: PRD-260801-0656-01
terms: [Pane registration]
blocked_by: [ISSUE-260801-1310-01, ISSUE-260802-0628-01]
---

## Agent Brief

**Category:** enhancement
**Summary:** Move placement into a core layout module and have both spawning verbs report the geometry tmux actually produced — no change to where panes land

**Current behavior:**
Placement logic lives in the binary crate: the split-direction argument type, the resolved-layout
type, and the function that turns flags into a layout all sit in the launch command module, and the
spawning verbs build their tmux split arguments there. `kill` will later need the same layout rules
and cannot reach them without depending on a sibling verb.

Neither verb reports anything about the window a pane landed in. Output is the pane id (`raw`), or a
short line / JSON object built from the pane's own registration. Nothing describes the resulting
geometry, so a test — or a caller — has no way to ask what the placement actually produced except by
issuing its own tmux queries.

**Desired behavior:**
A layout module in the **core** crate becomes the single owner of placement. The split-direction and
resolved-layout types move there from the binary crate — a move, not a copy; the binary keeps no
second definition. Placing a clap-derived enum in the core crate is precedented by the existing
output-format enum, which is already used directly as a CLI argument type.

The module exposes a placement operation performing **split → register → measure**, returning where
the pane landed: the destination window's stable id and the smallest pane width and height measured
in that window.

**Where panes land does not change in this slice.** A flagless placement inside tmux is still the
horizontal split giving the new pane 70%. This slice moves the code, establishes the measurement, and
puts the measurement in the output; the tile default, ownership gating, the layout-intent marker, and
the documentation change all belong to the slice that follows.

Two orderings are fixed here because later slices depend on them:

*Registration before measurement.* `@tt-agent`, `@tt-cwd`, and `@tt-name` are written immediately
after the split and before geometry is read, so the new pane already carries its identity when the
window is measured. A later slice resolves per-agent floors from exactly that identity. All three are
unconditional by the time this slice runs — `@tt-name` became so in `ISSUE-260802-0628-01`, which is
a blocker for that reason.

*The whole registration is an input, not a read-back.* The placement operation receives what to write
from its caller rather than querying the pane, because at the moment placement needs it the pane has
not been tagged yet. **This is more than the agent name, and the existing seam does not already carry
it.** Today's shared chokepoint stops at pane creation: `launch_pane` takes a `name` and discards it
(`let _ = name;`), and each verb registers afterwards in its own block with a different field set —
`launch` writes name, launched-at, and cwd; `spawn-agent` additionally writes agent and access. So the
operation takes a registration record covering every `@tt-*` field either verb writes, with the fields
a given verb does not set left empty.

One consequence to plan for rather than discover: the launched-at timestamp is produced by a helper in
the **binary** crate. Core cannot call it, so either the helper moves to core alongside the layout
module, or the timestamp is computed by the caller and passed in with the rest of the registration.
Either is acceptable; picking neither leaves the operation unable to register at all.

Both verbs report the destination geometry in `concise` and `json`; `raw` continues to emit only the
pane id. The reported minima are named distinctly from the per-agent floor fields that arrive in a
later slice — these are geometry a window *has*, not a minimum an agent *requires*, and one pair of
identifiers meaning both would invert depending on which the reader had in mind. The window field is
named for the window **id**, not `window`, because `status` already uses `window` for the window name.

**Key interfaces:**
- A new layout module in the core crate. The split-direction enum, the resolved-layout type, and the
  flags-to-layout resolution move into it. Both spawning verbs obtain placement through it.
- Its surface in this slice: a placement operation (target + the registration to write → landing
  report) and the flags-to-layout resolution. The retile and overflow stages arrive later; the shape
  should accommodate them without a second rewrite, but this slice is not required to stub them.
- The existing shared helper covers the split only. Moving registration behind the same seam is part
  of this slice's work, not something the current chokepoint already provides — verify by reading it
  rather than assuming, since its `name` parameter is accepted and dropped.
- The two verbs' `concise` and `json` renderers gain the destination window id and the two measured
  minima. `raw` renderers are not touched.
- The **JSON field names are fixed by the PRD**, not delegated: `min_pane_cols`, `min_pane_rows`, and
  `window_id`, spelled exactly so. § Reporting surface settles them with its reasons — the minima are
  named distinctly from the registry's `min_cols`/`min_rows` because those are a floor an agent
  *requires* while these are geometry a window *has*, and the window field is `window_id` rather than
  `window` because `status` already uses `window` for the window name.
- Only the **concise** format's key spelling and ordering is delegated, which is what the PRD's
  § Settled at triage list actually defers. Bounded the same way: the two minima must not collide with
  `min_cols`/`min_rows`, and the window field must not be spelled `window`.

**Acceptance criteria:**
- [ ] The layout module lives in the core crate: `rg -n '^pub mod layout;' core/src/lib.rs` returns a
      match, and returns none before this change.
- [ ] The split-direction and resolved-layout types **moved** rather than being duplicated:
      `rg -n 'enum Split\b|enum Layout\b' src/` returns no matches after this change, and returns the
      two definitions in the launch command module before it.
- [ ] A flagless placement inside tmux still produces today's arrangement — a horizontal split giving
      the new pane 70% of the width, both panes spanning the full window height. Observable at the CLI
      seam against the private test server. This criterion is a **regression guard**: it passes both
      before and after this change, and is the assertion the next slice deliberately flips.
- [ ] Both verbs emit the destination window's stable id and the smallest measured pane width and
      height in `concise` and `json`. In `json` the fields are named exactly `window_id`,
      `min_pane_cols`, and `min_pane_rows`, per the PRD. Observable at the CLI seam: a test parsing the
      JSON output finds all three under those names, and the reported window id equals the one tmux
      reports for the created pane independently. The test fails before this change because the fields
      are absent — `rg -n 'min_pane_cols|min_pane_rows' src/ core/src/` returns nothing today. (Do not
      widen that pattern to include `window_id`: it already matches an unrelated local binding in
      `core/src/target.rs`, so the widened form is green at baseline and checks nothing.)
- [ ] The reported minima equal what tmux reports independently for the same window — asserted by
      querying tmux for every pane's width and height in the destination window and comparing the
      componentwise minimum against the command's output, rather than against hard-coded numbers.
- [ ] `raw` output for both verbs is byte-for-byte unchanged.
- [ ] Registration happens **inside** the placement operation, not after it in each verb: the panes
      both verbs create still carry exactly the `@tt-*` fields they carry at this record's baseline —
      `launch` a name, launched-at and cwd; `spawn-agent` those plus agent and access — asserted at the
      CLI seam by querying the created pane's options. Note that the name is now **unconditional** on
      both verbs, auto-derived when `--name` is absent: `ISSUE-260802-0628-01` is a blocker precisely
      so that the registration this slice moves is already in its final shape. Do not reintroduce the
      flag-conditional form while relocating it. This is a regression guard on the observable
      registration, paired with the structural criteria above; without it the operation could be
      called with a registration it never writes and every other criterion would still pass.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Both halves matter.
      `--workspace`: this repo is a two-member workspace and a bare `cargo test` from the root tests
      only the binary package, silently skipping the `core` package — which is where this slice's new
      layout module and its unit tests land, so the bare form would not execute them. Isolation: point
      `TMUX` (socket path, server pid, session id) and `TMUX_PANE` at a server started as
      `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`, or use this repo's private harness
      once it exists. A run sharing the developer's tmux server is not a valid baseline — the
      pane-creating tests fail there, which is the hazard ISSUE-260801-1310-01 exists to remove. If a
      failure survives on an isolated server it is **not** this slice's to fix: report it and carry
      on, unless this slice introduced it.

**Out of scope:**
- The tile placement mode, the default change, and every rule gating a retile — ownership snapshot,
  tiling-from-the-second-pane, the `@tt-tiled` marker, and the sizing opt-outs.
- The documentation change; the 30:70 default is still the truth after this slice, so restating it
  would be false.
- Per-agent floors, effective-floor resolution, `below_floor`, and relocation.
- Retiling on `kill`; geometry on `status`.
- Predicting geometry from a model of tmux's tiling algorithm — explicitly rejected; this slice
  measures what tmux produced.

## Triage Notes

- The PRD records that agents tolerate being resized immediately after launch (`codex`, `cursor`,
  and `claude` all reach ready after a 280×82 → 139×40 resize mid-startup), which is what makes
  split-then-retile safe in the following slice and why prediction was rejected.

**Readiness gate (cold-reader): FAIL** (round 1)

The blocking finding was a scope verdict, and the fix reshaped this record:

- **Class 6 — epic-in-issue-clothing, prong (b).** The record bundled a code move, a default change,
  three ownership rules, a durable marker with its own lifecycle, four flag opt-outs, a new reporting
  surface, and edits to three documentation files across two repositories — nine acceptance criteria
  spanning changes that share no single reviewable decision. Split, with the user's approval, into
  this record (layout module + measured reporting, demoable with no tiling at all) and
  ISSUE-260802-0128-01 (tile default, ownership gating, marker lifecycle, opt-outs, docs). The
  documentation change went to the second, where the statement it makes becomes true.

Findings that applied to the material now in this record, fixed here:

- **Class 9 arm B** — the record had no criterion that could catch a regression in *unchanged*
  placement. The flagless-70% criterion is now stated explicitly as a regression guard that passes
  before and after, with a note that the next slice flips it, so a reader does not mistake a
  green-before criterion for a broken one.
- The measured-minima criterion originally had no independent check. It now compares against tmux's
  own report of the same window rather than against literals, which is the same principle the PRD
  applies when it rejects pinning a forecast against hard-coded numbers.
- The type-move criterion was added: without it, a copy left behind in the binary crate would satisfy
  every other criterion while leaving two definitions of the split-direction enum in the tree.

Findings that moved with the split are recorded in ISSUE-260802-0128-01's own triage notes.

**Readiness gate (cold-reader): FAIL** (round 2)

The class-6 split was verified as clean — the reader checked the boundary against the sibling record in
both directions, confirmed `blocked_by` runs one way, and could not decompose this record's criteria
into independently-demoable batches. Every `rg` observable was executed and confirmed red at baseline.

One blocking gap, fixed in place before re-gate:

- **Class 4 — `cargo test` passes in full was unverifiable as written.** The reader ran it and hit a
  reproducible failure in a test belonging to an unrelated feature, leaving an AFK agent with no way to
  tell whether to ignore it, fix it, or treat itself as blocked. Re-measured: that failure does not
  reproduce on an isolated tmux server — it is the shared-server hazard this epic's first slice exists
  to remove, reproducing under the reader. The criterion never said *where* to run the suite, which is
  the actual defect. It now names the isolated harness and states that a failure surviving on an
  isolated server is out of scope unless this slice introduced it. (This paragraph originally cited a
  test count here; the figure was wrong, and the PRD's Testing Decisions now owns how the suite is run.)

**Readiness gate (cold-reader): FAIL** (round 3)

One blocking gap, fixed in place before re-gate:

- **Class 1 — I misattributed a deferral to the PRD.** The brief said the PRD "routes to triage" the
  JSON field names for this slice's three new fields. It does not. The PRD's § Settled at triage list
  covers concise-key spelling and ordering, `status`'s JSON names, and `--split window` error text —
  not the spawning verbs' JSON names, which § Reporting surface **fixes** as `min_pane_cols`,
  `min_pane_rows`, and `window_id`, each with a stated reason, and which the Dimension Scan marks
  `UX: decided`. An AFK agent reading only the brief would have concluded the names were free-form
  subject to two negative bounds, and could legitimately have chosen different ones. The reader asked
  which artifact was right; the PRD is — it settled these deliberately, and `window_id` in particular
  exists to avoid colliding with `status`'s existing `window` field. The brief now requires the PRD's
  literal names and confines the delegation to the concise format, which is what was actually deferred.

Two things the reader confirmed rather than took on trust: the core-crate `Format` enum precedent is
real and used as a CLI argument type, and `status` does already use `window` for the window name — the
premise `window_id` exists to respect.

Also corrected here: the final criterion now says `cargo test --workspace`, after a sibling record's
gate established that the bare form does not run the `core` package at all — which is exactly where
this slice's layout module lands.

**Readiness gate (cold-reader): PASS** (round 4)

No findings. Both round-3 fixes reproduce clean: the brief and the PRD now agree that
`min_pane_cols`, `min_pane_rows`, and `window_id` are fixed names and only the concise format's
spelling is delegated, and the `--workspace` claim was verified by comparing what
`cargo test --no-run` and `cargo test --workspace --no-run` build. Every baseline command was executed,
including the widened `window_id` pattern, which the reader confirmed matches only the unrelated local
binding in `core/src/target.rs` — exactly what the criterion's "do not widen" parenthetical warns about.

Two notes on the exchange. The reader pushed back on a correction I sent it — I had told it the brief's
workspace command used a pipe and needed re-reading, and it was right that this record never carried
one; the piped form was in three sibling records and in its own verification command, not here. Good
catch, and I was wrong to broadcast that correction without checking this record's text.

It also flagged, without firing, that this record's round-2 narrative still cited a test count. It read
that as exempt journal history, which is defensible. I corrected it anyway before stamping, because the
figure was not merely uncommanded but **wrong** — leaving a false number in the record invites a later
reader to believe it. The edit only withdraws an incorrect claim from narrative; no criterion or
specification text changed.

**Readiness gate: REOPENED** (round 4)

Reopened by the step-7 adversarial breakdown pass, which reads all nine records plus the PRD together
and caught a plan-vs-code collision no per-record gate had reason to look for.

The brief said the layout operation performs "split → register → measure" and treated the existing
shared seam as movable wholesale. That seam is smaller than claimed: `launch_pane` is shared, but it
takes a `name` and **discards it** (`let _ = name;`), and all registration happens afterwards in each
verb's own block with a different field set — `launch` writes name/launched-at/cwd, `spawn-agent` adds
agent and access. Verified by reading both call sites. So an implementer could have moved the split,
left registration where it is, satisfied every criterion, and silently broken the ordering guarantee
two later slices depend on. Compounding it, the launched-at helper lives in the **binary** crate, so a
core-resident operation cannot call it at all — a wall that would have been hit mid-implementation with
no guidance in the brief.

Fixed by making the registration record an explicit input covering every field either verb writes,
naming the timestamp problem with both acceptable resolutions, warning that the existing helper covers
only the split, and adding a criterion asserting the created panes still carry exactly today's `@tt-*`
fields. Re-gating.

Second edit under the same reopened state, from the user's adjudication of the same pass's B3
finding. `ISSUE-260802-0628-01` makes `@tt-name` unconditional on both verbs, and it is now a
blocker here rather than a sibling. The two were initially both unblocked behind the test-harness
slice, which left a real ordering hazard: this record's registration-preservation criterion pinned
`launch` to writing a name "only with `--name`", so whichever landed second would have contradicted
the other. Ordering them removes the contradiction and means the registration this slice relocates
is already in its final shape — a `@tt-*` field set that changes right after being moved is the
worst of both. The criterion now says the name is unconditional and warns against reintroducing the
flag-conditional form while relocating it.

**Readiness gate (cold-reader): PASS** (round 5)

No blocking findings. The reader reproduced all three code claims the reopening edit rests on rather
than trusting them: `launch_pane` takes `name: Option<&str>` and opens with `let _ = name;`; the two
verbs register afterwards with different field sets; and `rfc3339_utc_now` lives in the binary crate
while `core/src/lib.rs` declares no module that could reach it. It also re-derived the two precedent
claims — a core-crate clap enum already serving as a CLI argument type, and `status` already using
`window` for the window *name*, which is why the new field is `window_id`.

Baselines were measured, not assumed: on a private server a flagless `launch --format json` emits
none of the three new fields, and a flagless launch produces `83x82` and `196x82` panes both at top
0 — a horizontal split at 70%, both full height — confirming the regression criterion is honestly
green at baseline rather than exempt by assertion.

Two things worth carrying forward, neither blocking:

- No criterion can falsify this slice's headline structural requirement, that registration happens
  *inside* the placement operation. Criterion 7 is green whether registration moved or stayed put.
  That is deliberate — the ordering has no observable consequence until the per-agent-floor slice,
  and the PRD forbids asserting which internal function was called — but it means the exact hazard
  that reopened this record at round 4 remains detectable only by review, not by the criteria.
- The `spawn-agent` halves of criteria 4 and 7 can pass by being skipped: the existing spawn-agent
  coverage is conditional on an agent binary being installed. `ISSUE-260802-0628-01` carries an
  explicit warning about this and names two workarounds; this record does not. Worth reading that
  record's criteria before writing these.

The reader also flagged a scope judgment it made and invited an overrule: it treated text covered by
the round-4 PASS as already-stamped and only the reopening edits as under edit, on the reasoning that
the rubric's already-stamped rows would otherwise be dead letters in any re-gate round. Under the
harsher reading two kind-(a) figure surfaces would flip to blocking. Both are true as written — it
verified them anyway — so the verdict is the same either way.
