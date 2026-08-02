---
id: ISSUE-260801-1310-05
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Per-agent geometry floors in the registry, effective-floor resolution, below_floor verdict, and status geometry
prd: PRD-260801-0656-01
terms: [Floor, Effective floor, Ceiling, Advisory floor, Agent profile, Readiness]
blocked_by: [ISSUE-260801-1310-04]
---

## Agent Brief

**Category:** enhancement
**Summary:** Give each agent a declared minimum pane geometry, resolve a window's effective floor, and report when a window is below it

**Current behavior:**
Tiling produces whatever geometry the pane count and window size imply, and nothing checks whether
the result is still large enough for the agents in it. Readiness detection matches a per-agent
regex against the pane, and agent TUIs reflow rather than truncate as panes shrink, so below a
certain geometry the readiness signal stops appearing and `wait-idle`/`prompt` fall through to idle
heuristics or burn their full timeout — a failure that reads as a slow agent rather than a small
pane.

The measured cliffs, from the PRD: `codex` is ready at 57 columns and fails at 56; `cursor` is
ready at 35 and fails at 34; `claude` is ready down to 25 columns but fails below 15 **rows**,
because its profile scans the whole visible pane for its sentinel. The two binding constraints are
complementary — one agent binds width, another binds height — so a single-dimension floor would
miss one of them whichever dimension was chosen.

**Desired behavior:**
Each agent profile gains optional `min_cols` and `min_rows` beside its readiness rules. Resolution is
componentwise and reads the **resolved** profile — the builtin deep-merged with the user's
`agents.toml` — so a partially configured profile is well defined:

| pane | floor |
|---|---|
| agent set, resolved profile declares both | the declared pair |
| agent set, resolved profile declares one | declared value for that axis, the global default for the other |
| agent set, resolved profile declares neither | the default pair |
| agent set, agent absent from the registry | the default pair |
| no agent recorded | contributes no floor |

Reading the *resolved* profile is what makes a partial user override of a **seeded builtin** behave:
a user who sets only `min_cols` for `codex` keeps the builtin's `min_rows` of 9, because the existing
per-field deep merge has already supplied it before floor resolution runs. The global default fills
an axis only where the resolved profile declares nothing for it. Filling from the global default
instead would silently raise `codex`'s height floor from 9 to 16 because someone tuned its width —
the merge already does the right thing, and this issue must not undo it by resolving floors against
the builtin table separately from the merge.

Shipped profiles are seeded from the measurements, each one step above its cliff: `codex` 58×9,
`cursor` 36×9, `claude` 26×16. The default for everything else is **58×16**, the componentwise
maximum of those — deliberately conservative, and documented as an assumption rather than a
measurement of any particular agent.

A window's **effective floor** is the componentwise maximum over the floors its panes contribute.
Contributing a floor and being measured against one are different: a pane with no agent contributes
nothing but still occupies geometry, so it still counts when the window is measured and can push an
agent pane below the floor while having none of its own.

Both spawning verbs gain a `below_floor` verdict in their output, describing the destination window
— true when the window's measured minima fall below its effective floor. The `status` verb gains
the target window's measured geometry and the same verdict, so a window crowded by a direct
`tmux split-window` or by concurrent spawns can be inspected without placing into it.

`status` gains those fields in `concise` and `json` only. Its `raw` output is a passthrough of a tmux
format string, so anything parsing it would break; it stays byte-for-byte unchanged, the same rule the
spawning verbs' `raw` output already follows.

The floor is **advisory**: it is not enforced against panes this tool did not place, and concurrent
spawns can overshoot it because each is a separate short-lived process measuring before the others
have split. That overshoot is not repaired later; it is made visible.

**Key interfaces:**
- The agent profile shape and its user-config counterpart gain two optional numeric fields, merged
  by the existing per-field deep merge so a user may set one without restating the other. Floor
  resolution consumes the **merged** profile; it must not consult the builtin table separately, which
  is what would reintroduce the global default over a builtin's untouched axis.
- The layout module gains effective-floor resolution over a set of panes plus the registry, and a
  floor-violation report. Both are pure given measured geometries and a registry, and are
  unit-testable without a tmux server.
- The registry documentation records that these values are derived from each agent's readiness
  rules, so changing an agent's `ready_regex` or `ready_lines` invalidates its measured floor. Note
  the coupling is real but unenforced: the shipped builtins differ in whether they carry readiness
  rules at all, and user configuration merges over them independently.

**Acceptance criteria:**
- [ ] An agent profile may declare `min_cols` and/or `min_rows` in user configuration and the
      resolved profile reflects them.
- [ ] A user configuration declaring only `min_cols` for a **seeded builtin** leaves that agent's
      `min_rows` at the builtin's shipped value, not at the global default — unit-tested against
      `codex`, whose shipped floor is 58×9: overriding its width alone must not raise its height floor
      to 16. The mirror case (declaring only `min_rows`) holds equally.
- [ ] A user configuration declaring only one axis for an agent with **no** shipped floor resolves the
      other axis to the global default.
- [ ] A pane whose recorded agent is absent from the registry resolves to the default pair rather
      than erroring or contributing nothing.
- [ ] A pane with no recorded agent contributes no floor but is still counted when the window is
      measured — verified by a case where an agentless pane shrinks an agent pane below its floor
      and the window reports below floor.
- [ ] A window's effective floor equals the componentwise maximum over its panes' contributions;
      unit-tested with a mixed set including an agentless pane, without a tmux server.
- [ ] Both spawning verbs report `below_floor` for the destination window in `concise` and `json`.
      Observable at the CLI seam against the private test server: placing enough panes to breach on
      a deliberately small window reports below floor, **and** a placement into a window with room
      reports not-below-floor — both directions, so an implementation hard-coding either answer fails.
      No such field exists before this change.
- [ ] `status` reports the target window's measured geometry and floor verdict in `concise` and
      `json`. Observable at the CLI seam: pointing `status` at a window crowded by direct
      `tmux split-window` calls — no spawn involved — reports it below floor, and at a roomy window
      reports it not below floor. Neither is expressible before this change.
- [ ] `status --format raw` is byte-for-byte unchanged.
- [ ] The shipped defaults are 58×16, with `codex` 58×9, `cursor` 36×9, `claude` 26×16.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Both halves matter.
      `--workspace`: this repo is a two-member workspace and a bare `cargo test` from the root tests
      only the binary package, silently skipping the `core` package's unit tests — which is exactly
      where this slice's floor-resolution and effective-floor logic lands. Isolation: point `TMUX`
      (socket path, server pid, session id) and `TMUX_PANE` at a server started as
      `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`, or use this repo's private harness
      once it exists. A run sharing the developer's tmux server is not a valid baseline — the
      pane-creating tests fail there, which is the hazard ISSUE-260801-1310-01 exists to remove.

**Out of scope:**
- Relocating a pane when the floor is breached — this issue detects and reports only.
- A per-invocation floor override flag; floors live in the registry so one caller cannot change the
  envelope of panes it shares a window with.
- A pane-count cap separate from the floor.
- Asserting the measured readiness cliffs in the test suite — they are data, re-measured when an
  agent's TUI changes, and asserting them would require launching real agents.
- Geometry columns in `list`; `status` carries the inspection surface.

## Triage Notes

**Readiness gate (cold-reader): FAIL** (round 1)

Two blocking gaps, both fixed in place before re-gate:

- **Class 4 — partial floor override on a seeded builtin was undecided.** The resolution table said
  "declared value for that axis, default for the other", which contradicts the registry's existing
  per-field deep merge: a user setting only `min_cols` for `codex` would have had its height floor
  silently raised from the shipped 9 to the global default of 16, purely because they tuned its width.
  Resolved by the user: resolution reads the **merged** profile, so a builtin's untouched axis
  survives; the global default fills an axis only where the resolved profile declares nothing.
  Recorded upstream in the PRD's § Floor and pinned here by two criteria (builtin and no-builtin
  cases) plus an explicit constraint in Key interfaces.
- **Class 9 arm A — the `status` criterion was a source grep** for "the verdict field name in the
  status output path". It decided nothing about behavior: a field defined and never populated
  satisfies it, the field name was never fixed so the pattern was unstated, and the location "the
  status output path" names no computable scope. Replaced with a CLI-seam observable that crowds a
  window with direct `tmux split-window` calls — which is also the scenario the field exists for —
  asserted in both directions.

Non-blocking, folded into the same edit: the `below_floor` criterion had no positive control and was
satisfiable by hard-coding `true`; both spawning-verb and `status` criteria now assert both
directions. And `status`'s output-format scope was unstated — its `raw` output is a passthrough of a
tmux format string, so it is now pinned byte-for-byte unchanged, matching the rule the spawning verbs
already follow.

**Readiness gate (cold-reader): PASS** (round 2)

Both round-1 fixes verified against the code rather than taken on the record's word. The merge premise
in particular was checked at `core/src/agents/mod.rs` — every optional scalar merges replace-if-present
with no refill-from-default step, and the existing `user_ready_lines_merges_onto_builtin` test already
exercises that shape — so a partial override of a seeded builtin does keep the builtin's other axis,
and the floor table's first row (not its second) is what `codex` hits. The premise is true of the code,
not aspirational.

One **delegable** finding, left as the implementer's call: floor-table row 3 (registered agent whose
resolved profile declares neither axis) has no acceptance criterion of its own. The other four rows are
each pinned. Low-stakes — the behavior falls out of the partial-override criteria and the merge's own
fallback — so it is test-coverage thinness rather than an undecided question.

The reader's own `cargo test` run was red in three pane-creating tests; that is its shared tmux server,
not this tree. The criterion now names the isolated harness explicitly, added before this stamp in
response to that observation.

**Readiness gate (cold-reader): REOPENED** (round 2)

Reopened by the author, not by a finding against this record. ISSUE-260801-1310-01's round-3 gate
established that a bare `cargo test` from the repo root does **not** run the `core` package — cargo
tests only the workspace member it is invoked from, so `cargo test` runs 62 tests where
`cargo test --workspace` runs 110. This record's floor resolution, effective-floor computation, and
floor-violation report are all specified as pure logic unit-tested in the core crate, so its final
criterion as stamped would not have executed a single test this slice adds. Criterion corrected to
`cargo test --workspace` with the reason stated inline, and the isolation command spelled out rather
than described. Re-gating.

**Readiness gate (cold-reader): PASS** (round 3)

The workspace claim was verified independently and more cleanly than I had done it — via
`cargo test --workspace -- --list`, which compiles the test binaries without executing them and so
needs no tmux isolation at all: 110 tests against 62 for the bare form, with the 48-test difference
matching `cargo test -p tmux-tools-core --lib` exactly.

Two **delegable** findings, neither blocking:

- The final criterion lacked the disposition clause its sibling record carries — what to do about a
  failure that survives isolation. The reader hit exactly that case while gating: the integration test
  `launch_keeps_pane_alive_after_cmd_exit` failed once and passed on immediate retry on a clean
  private server. Rather than copy the clause into six briefs, the rule now lives once in the PRD's
  Testing Decisions, which every brief already cites as its seam authority, and it names that test as
  a known intermittent so an AFK agent recognises it. That is an upstream edit, so this record's
  gated text is unchanged.
- Floor-table row 3 (registered agent whose resolved profile declares neither axis) still has no
  criterion of its own, carried forward from round 2. The row *is* expressed in the brief's table, so
  nothing is stranded in the notes; only its test coverage is inferential. Left as the implementer's
  call.
