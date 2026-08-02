---
id: PRD-260801-0656-01
scale: epic
stakes: internal
gate: passed 2026-08-01
contexts: [tmux-tools]
terms: [Tile, Floor, Effective floor, Ceiling, Advisory floor, Overflow window, Layout intent, Ownership, Exited-agent pane, Pane registration, Agent profile, Readiness]
issues: [ISSUE-260801-1310-01, ISSUE-260801-1310-02, ISSUE-260802-0628-01, ISSUE-260801-1310-03, ISSUE-260801-1310-04, ISSUE-260802-0128-01, ISSUE-260801-1310-05, ISSUE-260801-1310-06, ISSUE-260801-1310-07, ISSUE-260802-0205-01]
---

# Auto-tile layout for concurrent agent panes

## Problem Statement

When an orchestrating agent spawns several sub-agents, every one of them lands in the
same pane and shreds it.

`launch` and `spawn-agent` pin each split to the calling pane, resolved from `$TMUX_PANE`.
Sub-agents inherit their orchestrator's environment, so every sub-agent reports the *same*
calling pane — the orchestrator's. The implicit default gives the new pane 70% of the
width, which means the caller keeps 30% of what it had left, compounding: measured on a
200-column terminal the orchestrator drops to 59 columns, then 17, then 5, then 1 — four
spawns succeed and the fifth fails outright with `no space for a new pane`.

The visible symptom is cosmetic. The expensive one is not. Readiness detection reads the
pane and matches a per-agent regex, and agent TUIs *reflow* rather than truncate when a
pane shrinks. Measured against the live registry profiles, each agent has a geometry below
which its readiness signal simply stops appearing:

| agent | narrowest width still ready | shortest height still ready |
|---|---|---|
| `codex` | **57** (fails at 56) | ≤ 8 |
| `cursor` | 35 (fails at 34) | ≤ 8 |
| `claude` | ≤ 25 | **15** (fails at 14) |

The two binding constraints are complementary. `codex` fails on **width**, because its
`· Ready · Context` status line wraps. `claude` fails on **height**, because its profile
sets `ready_lines = 0` — a whole-pane scan — and `capture` reads only the visible pane, so
a short pane pushes the `✓ done | 🤖` sentinel out of view. A floor guarding one dimension
would miss the other regardless of which was chosen.

Below those geometries `wait-idle` and `prompt` fall through to idle heuristics or burn
their full timeout, and the failure is attributed to a slow agent rather than to a small
pane. So a layout problem degrades the completion signal this tool exists to provide, and
it degrades it quietly.

tmux offers no protection. Its own floor is a pane width of **1** — it will happily build a
3-column pane and only refuse somewhere past the eighth split. By the time tmux complains,
readiness has been broken for several panes already.

The user watching those panes has a second, related problem: there is no arrangement to
watch. Panes end up at whatever ragged sizes the split history produced, and killing one
does not reclaim its space into a sensible shape.

## Solution

`launch` and `spawn-agent` place panes into a tiled arrangement, and never themselves push
a window below the geometry its agents need to report readiness from.

After each spawn the containing window is retiled, then **measured**. If any pane has
fallen below the floor, the newly created pane is moved to another window and the origin is
retiled back. When a pane is killed from a window this tool tiled, the window is retiled so
the survivors reclaim the space.

The tool never forecasts geometry — it applies the layout and reads what tmux actually
produced. This is possible because agents tolerate being resized immediately after launch
(measured: `codex`, `cursor`, and `claude` all reach ready after being resized from 280×82
to 139×40 mid-startup), so a pane may be created, resized, and moved without damaging its
readiness signal.

**This changes the default.** Today a flagless `launch` or `spawn-agent` inside tmux makes a
horizontal 30:70 split, and that is documented in README.md and SKILL.md. It will tile
instead — that is the feature, not a side effect. What does *not* change: no invocation
starts failing, no flag changes meaning, and any command expressing an explicit direction or
size is placed exactly as it is today. Driver skills need no edits because none of them pass
layout flags.

**Tiling engages from the second pane.** Because ownership is judged from the window as it
stood before the split (§ Ownership), the first pane this tool puts into a window it did not
already own is placed **exactly as it is placed today** — a horizontal split giving the new
pane 70% — and no retile follows. That window becomes eligible once it contains a pane we own,
so the second and later spawns tile.

This holds for **both** verbs, including `launch` with no flags at all. It does so only because
every pane this tool creates now registers a name (§ Persistent shape, `ISSUE-260802-0628-01`).
Without that, a repeated bare `launch --cmd …` would leave no ownership evidence, its window would
never become eligible, and the compounding 30:70 shrink would persist indefinitely on the very
invocation shape this paragraph promises to fix.

This is the intended shape rather than a gap. A window holding one agent has no crowding to
relieve; the compounding shrink this epic exists to fix begins at the second pane. It also
makes the change strictly additive at the boundary people meet first: the very first spawn into
your own shell window behaves precisely as it does now, and tiling appears only when a second
agent would otherwise have started the shrink.

From the watching human's side, several agents are legible side by side, an arrangement that
stays as even as tmux can make it while agents come and go, and an explicit report when a
window has filled up and work moved elsewhere.

## User Stories

1. As an orchestrating agent, I want each sub-agent I spawn to land in a pane sized like
   the others, so that my own pane is not reduced to a sliver by the third spawn.
2. As an orchestrating agent, I want spawning to keep working past the third sub-agent, so
   that a wave of parallel work does not fail with `no space for a new pane`.
3. As an orchestrating agent, I want every pane to clear the geometry its own agent needs,
   so that `wait-idle` returns on readiness rather than on timeout.
4. As an orchestrating agent, I want the floor enforced on both width and height, so that a
   `codex` pane is never too narrow and a `claude` pane is never too short.
5. As an orchestrating agent, I want to be told when my pane was moved to a different
   window, so that I can report accurately where work is happening.
6. As an orchestrating agent, I want placement based on the geometry tmux actually produced
   rather than a forecast, so that placement cannot drift from reality.
7. As an orchestrating agent, I want the pane I just created to be immediately usable for
   `prompt` and `capture`, so that I can proceed without a settling delay.
8. As a human overseeing a run, I want several agents visible at once, so that I can compare
   their progress at a glance.
9. As a human overseeing a run, I want to confirm my skill edits actually execute correctly,
   so that I need to *watch* agents work rather than read their output after the fact.
10. As a human overseeing a run, I want no pane shrunk below a readable size, so that what I
    am watching is worth watching.
11. As a human overseeing a run, I want the arrangement kept as even as tmux allows as agents
    are added, so that I do not have to manually resize panes mid-run.
12. As a human overseeing a run, I want killed agents' space reclaimed into larger surviving
    panes, so that the window does not stay cramped at the previous wave's density.
13. As a human overseeing a run, I want windows created by a different project's run left
    alone, so that concurrent work elsewhere is not reshaped by this one.
14. As a human overseeing a run, I want the destination window reported rather than switched
    to, so that a background spawn never moves my focus.
15. As a human running many agents, I want overflow panes packed into an existing tiled window
    that has room, so that I do not accumulate a trail of one-pane windows.
16. As a human who arranged a window by hand, I want that arrangement preserved when a pane in
    it is killed, so that opting out of tiling actually lasts.
17. As a driver skill, I want the improved placement without changing any of my commands, so
    that upgrading requires no edits to me.
18. As a driver skill author, I want the tool's own skill documentation to describe the real
    default, so that agents reading it do not pass flags that opt out of the behavior they want.
19. As a user with a specific arrangement in mind, I want an explicit direction or size to
    disable automatic tiling entirely, so that I can lay panes out by hand.
20. As a user, I want an explicit `--size` to keep meaning exactly what it means today, so that
    a documented invocation does not start failing.
21. As a user on a smaller display, I want the ceiling to follow from my actual window size,
    so that a limit calibrated on a large monitor does not produce unreadable panes on a laptop.
22. As a user, I want to tune an agent's floor in the registry, so that I can trade legibility
    for density for a specific agent, and record why.
23. As a user of a custom or unmeasured agent, I want a documented, deliberately conservative
    default floor, so that I know the number is an assumption rather than a measurement of my agent.
24. As a maintainer, I want the floor values traceable to a measurement, so that a change to an
    agent's TUI can be re-measured rather than re-argued.
25. As a maintainer, I want the tool's dependence on tmux's tiling behavior verified against the
    real tmux binary, so that a tmux release that changes it fails the suite.
26. As a maintainer, I want the layout rules in one module shared by every verb that needs them,
    so that `kill` does not have to depend on `launch`.
27. As a maintainer, I want the test suite to run against a private tmux server with no user
    config, so that running tests neither rearranges my session nor inherits my tmux options.
28. As a maintainer, I want tests to run at a fixed window size, so that geometry assertions are
    deterministic rather than dependent on my terminal.
29. As a maintainer, I want the decisions the tool makes reported in its output, so that tests
    can assert on them without reaching inside the implementation.
30. As a user diagnosing a problem, I want to inspect whether a window is packed below its floor,
    so that I can explain why readiness detection started misbehaving.
31. As a user, I want `--split window` to work from inside tmux, so that the documented opt-out
    is actually usable.

## Implementation Decisions

### Layout template

tmux's own `tiled` layout, applied to the window after each spawn. It is **row-first** — it
holds pane width roughly constant while adding rows — which suits a fleet where `codex` binds
on width. It does **not** produce uniformly sized panes: at counts that are not rectangular,
tmux stretches the final row across the full width (at 3 panes on a 280-column window:
139×40, 140×40, 280×41). The contract this epic offers is "tmux's `tiled` arrangement," not
"every pane identical."

### Floor: per agent, in the registry

Each agent profile gains optional `min_cols` and `min_rows` beside its `ready_regex`/
`ready_lines`.

Resolution is componentwise and reads the **resolved** profile — the builtin deep-merged with the
user's `agents.toml` — so a partially configured profile is well defined:

| pane | floor |
|---|---|
| `@tt-agent` set, resolved profile declares both | the declared pair |
| `@tt-agent` set, resolved profile declares one | declared value for that axis, the global default for the other |
| `@tt-agent` set, resolved profile declares neither | the default pair |
| `@tt-agent` set, agent absent from the registry | the default pair |
| no `@tt-agent` (plain `launch`, or a foreign pane) | contributes no floor — nothing to protect |

Because the table reads the resolved profile, a user who overrides **one** axis of a seeded builtin
keeps the builtin's other axis: setting `min_cols = 50` for `codex` leaves its `min_rows` at the
shipped 9, not at the global default of 16. The per-field deep merge already produces this; the
alternative — filling the unset axis from the global default — would silently raise `codex`'s height
floor from 9 to 16 because someone tuned its width. The global default applies only where the
resolved profile declares nothing at all for that axis.

Contributing a floor and being measured against one are different things. A pane with no agent
contributes nothing to its window's effective floor, but it still occupies geometry, so it still
counts when the window is measured — it can push an agent pane below the floor even though it has
none of its own.

Measured values seed the shipped profiles — `codex` 58×9, `cursor` 36×9, `claude` 26×16, each
one step above the cliff in the Problem Statement. The default for everything else is **58×16**,
the componentwise maximum of those. That default is an explicit assumption, not a measurement of
any particular agent: it is the most conservative thing we can assert for a TUI nobody has tested,
and it is documented as such.

A window's **effective floor** is the componentwise maximum over the floors of the panes actually
in it. In a mixed window — an orchestrator beside driven `cursor` and `codex` panes — this
collapses to the strictest agent present.

These values are *derived from* the readiness rules: `codex` fails at 56 columns because its
status line wraps, `claude` fails at 14 rows because `ready_lines = 0` scans a pane that no longer
shows the sentinel. Note the coupling is real but not enforced — the shipped builtins carry
`ready_regex: None`, and the measured rules live in user configuration, which deep-merges
independently. Changing `ready_regex` or `ready_lines` for an agent therefore invalidates its
measured floor, and the registry documentation says so. (The coupling is uneven across the shipped
profiles: `cursor` and `agy` ship a `ready_regex` in the builtins — `cursor` ships `ready_lines = 4`
too — while `codex` and `claude` ship none and take their measured rules from user configuration.)

There is no per-invocation floor override. A flag would let one caller admit panes below the floor
an existing pane was placed under, in a window they share, with no record of which floor applied to
what. Configuration lives in the registry, where it is durable and attributable.

No pane-count cap. How many agents run concurrently is the orchestrator's decision, and the
orchestrating skills already declare their own limits.

### Ceiling: measured, never predicted

The tool does not forecast geometry. It splits, retiles, reads the resulting pane sizes, and acts
on what it finds. An earlier design predicted geometry from `rows = ceil(sqrt(n))`; that is
rejected. Prediction was only ever needed to avoid resizing a freshly launched agent, and
measurement showed that constraint does not exist.

Rejecting prediction also removes three latent defects: tmux's `tiled-layout-max-columns` window
option changes the grid and the formula ignored it; the formula had to be maintained as a parallel
model of an undocumented algorithm; and the test pinning it compared the tool's forecast against
hard-coded literals, so it could never detect the tmux change it existed to catch.

The ceiling therefore has no stored value. It is wherever the effective floor stops being satisfied
on the window at hand — with a 58×16 effective floor, 16 panes on a 280×82 window, 9 on a
200-column one, 4 at 120 columns.

### Ownership

A pane is **ours** when `@tt-name` or `@tt-agent` is set and, when both the pane's `@tt-cwd` and
the current working directory are known, they match. This mirrors `src/cmd/safety.rs` exactly,
including its fail-open behavior when a cwd was never recorded, so layout and the destructive verbs
answer "is this ours" the same way. A window is eligible for retiling, and as an overflow
destination, when it contains such a pane.

Because `@tt-name` is now written on every pane this tool creates (§ Persistent shape), the
predicate's first clause holds for **all** of them — including a `launch` with no `--name` and no
agent. That is what makes the tiled default reach the flagless `launch` path rather than only the
agent path: the first such pane makes its window eligible, so the second and later ones tile. There
is exactly one ownership predicate, unchanged in meaning; it is the registration that got honest,
not the rule that got looser.

`@tt-cwd` records the working directory of the **invocation that created the pane**, not where the
agent works: `spawn-agent --cwd /tmp/scratch` still records the caller's directory. That is the
right identity for this purpose — it answers "which project's run owns this pane," which is what
story 13 needs.

**Ownership is evaluated against the window as it stood before this invocation's split.** `place`
snapshots the window's panes and ownership first, decides eligibility from that snapshot, and only
then splits. Without the snapshot, splitting into a stranger's window would tag a pane with our cwd
and thereby manufacture the authority to retile that window — putting a pane somewhere would become
the act that grants permission to reshape it. A window that was not ours before the split receives
the pane and is left un-retiled.

Registration (`@tt-agent`, `@tt-cwd`, `@tt-name`) happens immediately after the split and **before**
the retile, so the new pane carries its identity when geometry is measured. The pane's agent is also
passed into `place` as an input, so its floor is known without reading it back.

An earlier design justified a weaker predicate by capturing the prior layout string for restoration.
That is dropped: tmux refuses to apply a saved layout to a window holding more panes than when it was
captured (`have 4 panes but need 3`), which is exactly and only the situation a spawn creates.

### Layout intent: the `@tt-tiled` window marker

A window this tool tiles carries a `@tt-tiled` window option. Transitions are explicit:

- **Set** when the tool tiles a window.
- **Cleared** — unset, not set to a false-ish value — when a split expressing explicit sizing intent
  (`--split h|v`, or `--size`) targets that window. The caller has taken manual control. Readers test
  presence, so "cleared" and "never set" are the same state and there is only one absent-marker case
  to reason about.
- **Absent** means the tool will not *rearrange what it did not arrange*: `kill` does not retile the
  window, and it is not an overflow destination.

The marker is a **record of what the tool did, not a precondition for doing it.** It gates teardown
(`kill`) and overflow-destination eligibility; it never gates placement. Placement-time tiling is
gated by ownership alone — as it must be, since the very first tiling placement into a window
necessarily finds no marker and is what sets one. The consequence is deliberate: after a
`--split v` clears the marker, a later flagless placement into that owned window tiles it again and
re-marks it. The alternative would make one manual split opt a window out permanently, which is a
durability nobody asked for; a caller who wants the arrangement kept simply keeps passing an explicit
direction, and each such invocation re-clears the marker.

Without a durable marker the documented opt-out would survive placement but not teardown — a window
arranged by hand would be flattened by the first `kill` in it, because nothing at kill time
distinguishes a hand-built window from a tiled one. Restricting overflow destinations to marked
windows keeps "absent means never touch" true in both of the directions the marker governs.

At `kill`, the marker and ownership are read **before** the pane is destroyed. Pane options vanish
with the pane, so a pane holding the window's only ownership evidence would otherwise make the window
un-retileable exactly when reclamation is due.

### Flags

The split-direction argument gains a `tile` value, the implicit default inside tmux for both
`launch` and `spawn-agent`.

**Any explicit sizing intent opts out of tiling**, and is placed exactly as it is today: `--split h|v`
with or without `--size`, and — because `--size` is meaningless except for a directional split —
`--size N` on its own, which continues to mean the horizontal split at that percentage that README.md
and SKILL.md document today. No invocation that works today is newly rejected, and tiling never
discards a size.

**`--split tile --size N` is rejected at parse**, with a diagnostic naming the conflict. It is the one
combination the three rules above cannot jointly satisfy: `--size` means "opt out and split at this
percentage" while `--split tile` means "tile," and a tiled layout has no percentage to honour.
Accepting it would force a silent winner. Rejecting is safe precisely because `--split tile` does not
exist today, so no working invocation starts failing — the rule is unreachable except by a caller who
opted into the new value explicitly. The conflict is value-dependent (`--size` is legal with every
other `--split` value), so it is an explicit post-parse validation rather than a declarative
`conflicts_with`, which cannot express it.

Two honest qualifications. `--split window --size N` ignores the size today, because the new-window
path has nothing to size; that stays true, so "nothing is silently discarded" describes the tile
interaction, not that pre-existing flag combination. And opting out changes *placement* only — an
explicit invocation still emits the new output fields below and still clears `@tt-tiled` on the window
it targets, because that is how the window records that a human took manual control.

`--split window` keeps its meaning but is **currently broken** from inside tmux: the launch path
passes the calling *pane* id to `new-window -t`, which tmux rejects with `can't specify pane here`.
Since this epic makes that flag a documented opt-out, fixing it is in scope.

Two destinations are unaffected by the tile default because they create a fresh window rather than
splitting: the outside-tmux path, and `--session`/`--window` without `--target`. Both produce a
single-pane window, which is already at its own floor; tiling applies from the second pane onward if
the window later becomes a target.

### Placement and overflow

Split into the target window, retile, measure. If every pane clears the effective floor, done. If
not, move the newly created pane and retile the origin back.

The move uses **`join-pane -d`** when the destination is an existing window and **`break-pane -d`**
when a new window must be created. `break-pane` cannot pack into an existing window — it makes the
moved pane the sole pane of a fresh one — and both commands select the destination window unless
`-d` is passed, which would violate "reported, not selected." Pane user-options survive either move
(verified), so registration and ownership travel with the pane.

Because geometry is never forecast, candidate capacity is established by **trial, not prediction**.
Eligible marked windows are tried in ascending index order: the pane is joined, the window retiled and
measured, and if it now breaches its effective floor the pane is moved on. A failed candidate is
**retiled again after the pane leaves**, so a rejected trial restores the arrangement it had rather
than the ragged one pane removal produces. The candidate set is the session's eligible marked windows,
so the search is finite by construction; when every candidate breaches, the pane goes to a fresh window.

A fresh fallback window is tiled and marked `@tt-tiled` after the move. This is not automatic:
`@tt-tiled` is a *window* option, and only *pane* options travel with a relocated pane, so
`break-pane` yields a window that inherits global window options and carries no marker until one is
set. Without that step the new window would be invisible to every later overflow search.

Freed capacity in the caller's own window needs no preference rule: that window is the split target
and is always tried first.

The destination is reported, not selected.

Relocating the new pane restores a window that *this spawn* pushed below the floor. It does not repair
a window that was already below it — see Accepted limits.

### Reclamation on kill

`kill` retiles a marked, owned window whenever a pane is removed. An earlier design retiled only when
the row count dropped, justified by "nothing changes geometrically otherwise" — that premise is false,
because going from six panes to five converts three equal rows into two equal rows plus a full-width
straggler. Retiling can only enlarge surviving panes, so it cannot breach the floor.

Killing the window's **last** pane is not a retile case: tmux destroys the window with the pane, so
there is no window left to rearrange and no survivors to reclaim anything.

**`--any` does not reach the layout guard.** The override governs *destruction* — it is what lets a
caller kill a pane that is not theirs or was created from another cwd — and it stops there. The retile
decision is evaluated separately and still requires the window to be marked and to hold a pane this
invocation owns, so `kill --any` on a stranger's pane destroys that pane and leaves the stranger's
window arranged as it was. Widening `--any` to the layout guard would make the override's blast radius
jump from one pane to a whole window the caller never had any claim on, which is precisely what story
13 rules out; and because the tool's own panes never need `--any`, nothing legitimate loses reclamation.

Read that sentence carefully: it is scoped to the stranger's *window*, not to the killed pane. The
override grants no layout permission and **revokes none**. When the guard answers yes on its own —
the window is marked and holds a pane this invocation owns — the retile happens whether or not
`--any` was passed, including when the pane being killed is itself unowned. That combination is not
exotic: an unowned pane cannot be killed without the override at all, so it is the only way the case
arises. Taking the sentence to mean "an unowned target suppresses the retile" is the natural
misreading and the wrong one.

### Module shape

A new `layout` module in the core crate becomes the single owner of these rules, with the
split-direction and resolved-layout types moving there from the binary crate. Three verbs need it —
the two spawning verbs and `kill` — and three consumers is the signal to put a shared floor beneath
them rather than have sibling verbs import from each other.

Its public surface is `place`, which takes the target and the incoming pane's agent identity and
performs the snapshot-split-register-retile-measure-move sequence, returning where the pane landed;
`rebalance`, which retiles a window and returns its measured geometry; and `floor_violations`, which
reports panes below a given effective floor. The tmux calls and the overflow trial stay inside.

The **ownership predicate is public in the core crate**, not layout-internal. Both the layout module
(core) and the destructive verbs' safety evaluator (binary) must answer "is this pane ours"
identically, and the dependency runs one way — the binary depends on core, never the reverse — so a
predicate private to layout, or left in the binary, would force one of the two callers to restate it.
Restating it is the duplication this shared definition exists to remove.

Placing a clap-derived enum in the core crate is precedented — the output-format enum already lives
there and is used directly as a CLI argument type — so this introduces no new dependency and no new
pattern. The repo has no ADR directory and no ADR practice to respect; this PRD is the record of the
decision, and an ADR home can be seeded later if the repo grows one.

### Persistent shape

The epic introduces no new store. It reuses the existing tmux pane user-options (`@tt-name`,
`@tt-agent`, `@tt-access`, `@tt-launched-at`, `@tt-cwd`) as the ownership source of truth. Which of
them a pane carries depends on how it was made: `spawn-agent` writes agent and access, `launch` writes
neither. `@tt-name`, `@tt-launched-at`, and `@tt-cwd` are written by both paths — `@tt-cwd`
unconditionally, independent of `--name`, failing open when the cwd is unreadable, which mirrors
`safety.rs`'s own fail-open. It adds one **window** option (`@tt-tiled`) and two optional per-agent
fields to the existing registry file (`min_cols`, `min_rows`). No database, no new file, no new
dependency.

`@tt-name` is written on **every** pane this tool creates, not only when `--name` is passed. Absent
the flag, the name is auto-derived from the command being launched plus a random suffix
(`npm-a3f2`, `cargo-91bd`, falling back to a fixed stem when no usable command word exists).
`ISSUE-260802-0628-01` owns this; before it, `launch` set `@tt-name` only under `--name`.

That earlier behavior was not a deliberate protection — it was the ownership guard failing to
recognize its own output. `src/cmd/safety.rs` states the rule it means to enforce as "panes created
by tmux-tools"; an unnamed `launch` pane *is* created by tmux-tools, and the check simply had no
evidence of it. Always registering a name makes the recorded identity honest, and the predicate then
answers correctly without being loosened.

**This does widen the guarded verbs, and the widening is accepted deliberately.** Four verbs share
the ownership check — `kill`, `interrupt`, `escape`, and `send-enter` — and on a pane that `launch`
created without `--name` all four change from refusing ("not created by tmux-tools") to acting. The
blast radius is bounded on three sides: the verbs are strictly single-target — the epic adds no sweep
and none exists today; `@tt-cwd` is still written, so the cross-cwd guard is untouched; and `--any`
remains the only way past either check for genuinely foreign panes. What becomes reachable is one
explicitly targeted pane, in the caller's own cwd, that this tool created.

`send-enter` is worth naming separately, because "the destructive verbs" is the natural shorthand
for this set and it hides a verb that submits input rather than destroying a pane. Two facts settle
it. The three-part bound above applies to it unchanged. And `send-enter` is the only *guarded* way to
deliver **Enter** while being far from the only way — other guarded verbs deliver keystrokes too,
since signalling and escaping are keystrokes; the claim is about Enter, which is what the argument
rests on. At least three sibling verbs — sending
arbitrary text, sending text with a trailing Enter, and the run-a-command verb — reach the same pane
without consulting ownership at all, none of them appearing among the enforcement helper's call
sites. Enter can therefore already be delivered to any pane on the server. What the guard blocks is
one verb's spelling of an act three other verbs perform freely, and only on panes whose creator left
no evidence — an inconsistency rather than a protection, and removing it is intended.

### Reporting surface

`launch` and `spawn-agent` report the pane's **final** landing state in both `concise` and `json`
formats (`raw` continues to emit only the pane id): `min_pane_cols` and `min_pane_rows` are the
smallest pane dimensions measured in the destination **window**, `window_id` is the stable tmux window id,
`relocated` records whether a floor breach forced a move, and `below_floor` says the destination is
*still* below its effective floor — which happens only when no window can satisfy it. The field is
`window_id` rather than `window` because `status` already uses `window` for the window *name*.

Every field describes the window, never the individual pane, so a window whose effective floor is
empty — no agent panes in it at all — still reports its measured minima, with `below_floor: false`.
One rule for all cases, no per-pane exception and nothing conditionally omitted.

The reported names are deliberately distinct from the registry's `min_cols`/`min_rows`. Those are the
floor an agent *requires*; these are the geometry a window *has*. Reusing one pair of identifiers for a
required minimum and an observed minimum would invert the meaning depending on which section you were
reading.

`status` gains the target window's measured geometry and floor verdict in `concise` and `json`, so a
window crowded by a raw `tmux split-window` or by a concurrent overshoot can be inspected without
spawning into it. Its `raw` output stays **byte-for-byte unchanged** — it is a passthrough of a tmux
format string, so anything parsing it would break, and the same rule already governs `raw` on the
spawning verbs. `list` is unchanged; its table is already eight columns.

`status` is also where a *relocated* pane's origin window is inspected. The spawning verbs'
`below_floor` describes the window the pane finally landed in, which after a successful relocation is
not the crowded one — the crowded origin is reported by pointing `status` at it.

Because these are measurements rather than forecasts, they are also the assertion surface for the tests.

### Documentation

The 30:70 default is documented in the repo README, in the tool's skill documentation, and in the
layout-resolution doc comment. All are rewritten in the same change, and the change of default is
stated plainly rather than implied.

The skill documentation exists in two separately version-controlled copies which have drifted in both
directions — 63 lines present only in this repo's `SKILL.md`, 88 only in the installed copy at
`/Users/Shared/Data/work/Programming/ai/claude/user/skills/tmux-tools/SKILL.md`, which is what agents
load via `~/.claude/skills/tmux-tools/`. **Both** are updated; leaving the loaded copy stale would
actively teach agents that the old default still holds.

### Accepted limits

The floor is **advisory, not an invariant**. Agents that call tmux directly to split panes bypass it
entirely — this already happens in practice, and a 58-column pane created that way is observable in a
live session today. Measurement reads every pane in the window, not only owned ones, so a raw split
still *consumes* geometry and can push agent panes below the floor; it simply contributes no floor of
its own and is never prevented.

Concurrent spawns can overshoot: each is a separate short-lived process, so *k* simultaneous spawns can
each measure before the others have split. This is tolerated rather than locked. A lock would only
serialize this tool against itself and could not cover direct tmux splits, so it cannot make the floor an
invariant regardless. The overshoot is **not** self-correcting — no later operation moves panes out of an
over-full window; a subsequent spawn places itself elsewhere and leaves the crowding in place. It is
surfaced through `status` and `below_floor`.

That left detection with no remedy reachable through the tool, which is tolerable for a human (who can
run `tmux select-layout tiled` by hand) and awkward for an agent, the primary caller — an agent would
have to shell out to raw tmux, the very bypass that makes the floor advisory. ISSUE-260802-0205-01 adds
a `rebalance` verb wrapping the layout module's existing operation, under the same marker-and-ownership
guards `kill` uses. It repairs the *ragged* case; it still cannot repair an *over-full* one, because
retiling redistributes geometry and cannot remove panes.

Measuring rather than forecasting costs additional tmux round-trips per spawn — the retile, the
measurement, and on breach a move plus a second retile. That does **not** touch the figure the README
advertises: sub-10ms cold start describes the *binary's* startup, the thing that replaced a 200-500ms
Python interpreter start, and this epic adds no work to process startup at all.

What it adds lands on `launch` and `spawn-agent` only — the two least frequent verbs, and the two
already dominated by the agent process booting inside the new pane, which costs orders of magnitude
more than a handful of tmux calls. The hot-loop verbs the README's argument is actually about (`send`,
`capture`, `wait-idle`) are untouched. So no benchmark gates this epic. If placement latency ever does
become a complaint, the measurement to make is `hyperfine` against those two verbs with and without the
retile — not a cold-start benchmark, which would measure the wrong thing.

Panes whose agent has exited are held open by the keep-alive wrapper and count toward geometry like any
other pane. They are not reclaimed automatically: doing so would destroy the crash output the wrapper
exists to preserve. Driver skills already end by killing their pane.

### Unrelated correction

The `cursor` profile's readiness scan depth stays at 4 rows. Raising it was considered to tolerate footer
wrapping; measurement shows `cursor` stays ready down to 35 columns, well below any width this design
permits, so the change would buy nothing and would widen the surface for false positives.

## Testing Decisions

A good test here asserts **externally observable behavior** — the geometry tmux actually produced, the
window a pane actually landed in, the values the command actually printed — never which internal function
was called. Because the tool measures rather than forecasts, the values it reports are the same values a
test independently checks.

### Sequencing: the isolated-server harness blocks the pane-creating slices

Pane-creating tests move onto a private server started as `tmux -L tt-tests-<pid> -f /dev/null` at a fixed
280×82. Both flags matter: `-L` gives an independent socket so the suite cannot touch the developer's
session, and `-f /dev/null` prevents the developer's `~/.tmux.conf` from being loaded — a global
`tiled-layout-max-columns` would otherwise change the grid and make "deterministic geometry" false. (The
suite header today names a *session* `ttests-<pid>`; this is a *socket*, hence the distinct name.)

This must land before any slice that creates or rearranges panes, for two reasons:

- Making tiling the default converts existing tests from pane-scoped to window-scoped operations. Today's
  suite drives `launch` against the test runner's own pane and isolates only by unique pane *name*, so a
  suite run from inside a working session would rearrange it, live agents included.
- The tmux-behavior tests below require it — verifying tiling means creating sixteen panes and retiling
  them, which cannot be done safely on the developer's session.

It does **not** block slices needing no tmux server: floor resolution, overflow-candidate ordering, and the
flag-parsing contract.

The work is larger than reusing an existing helper: the suite's hand-built `TMUX` triple is derived from the
*default* server, `run_tmux` always invokes plain `tmux`, and `run_bin` takes no environment. Setting `TMUX`
to a private server's socket triple redirects tmux itself, so no new CLI flag is needed — but the helpers
must thread it through, and they must **also** control `TMUX_PANE`: target resolution reads `TMUX_PANE`
first and consults `TMUX` only in its absence, so a suite started from inside a developer pane would keep
that pane's `%N` while aiming commands at the private socket. The existing regression test already calls
`env_remove("TMUX_PANE")` for exactly this reason. The RAII guards must tear down a server rather than a
session.

### How to run the suite, and what a surviving failure means

Every slice in this epic closes with `cargo test --workspace`, run against an isolated tmux server.
Both halves are load-bearing and neither is stylistic.

`--workspace`: the repo is a two-member workspace (`members = [".", "core"]`) and cargo tests only the
member it is invoked from, so a bare `cargo test` at the root runs the binary package and the
integration suite while **silently skipping `core`** — no warning, no skipped-test line, nothing to
notice. Since this epic's pure logic (the layout module, floor resolution, overflow-candidate
ordering) all lands in `core`, the bare form would not execute the tests these slices add. Compare the
`Executable` lines printed by `cargo test --no-run` against those printed by
`cargo test --workspace --no-run`; the second lists a `core` binary the first does not, and neither
runs a test. Deliberately no test counts here: this epic adds tests to both members, so any number
written down is wrong by the time the first slice lands — the missing *executable* is the durable
signal.

Isolation: point `TMUX` (socket path, server pid, session id) and `TMUX_PANE` at a server started as
`tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82`, or use the private harness once the first
slice lands. A run sharing the developer's tmux server is not a valid baseline — the pane-creating
tests fail there readily and inconsistently, which is the hazard the harness slice exists to remove.

**A failure that survives isolation is not the current slice's to fix** unless that slice introduced
it: report it and carry on. This is not hypothetical, and the case we have is instructive.

`launch_keeps_pane_alive_after_cmd_exit` fails for some developers and passes for others on the same
commit — observed on 2026-08-02 both as an intermittent failure and as a deterministic one, on clean
private servers. The cause is not tmux: the keep-alive wrap execs `$SHELL`, so the test runs the
developer's **interactive shell startup** inside the pane, and a prompt that redraws on init (a
`powerlevel10k` zsh, for instance) can overwrite the pane's visible content before `capture` reads it.

That exposes a second isolation axis this epic does *not* close. The harness slice suppresses tmux's
user configuration; it does nothing about the shell rc files the keep-alive wrap then loads. Anyone
seeing this failure should suspect their shell prompt before suspecting their change. Fixing it —
pinning the wrap's shell for tests, or making the assertion robust to a redraw — is real work and is
out of scope here; it is recorded so the next person does not re-derive it.

It is also **not the only such test**. `full_smoke` has been observed failing on an `execute`-verb
timeout on a clean private server, in a session where `launch_keeps_pane_alive_after_cmd_exit` passed.
So treat the rule above as general rather than as a carve-out for one named test: the pane-creating
suite has more than one assertion whose outcome depends on the machine and shell it runs under, and
the epic closes none of them.

Without this rule an AFK agent hitting it has no way to tell whether it is blocked, whether it broke
something, or whether to go fix an unrelated test.

### Primary seam: the CLI binary against that server

The existing suite already shells out to the built binary and asserts on stdout and exit codes. Every
behavior in this epic is observable there: the tile default, floor enforcement, `join-pane` vs `break-pane`
relocation, the ordered multi-candidate trial including retile-after-rejection and the fresh-window
fallback with its marker, the shared ownership predicate, `@tt-tiled` set/clear
transitions and their gating of kill-time retiling, focus preservation via `-d`, the sizing opt-out, and the
`--split window` fix.

### tmux-behavior tests, not a prediction pin

Because nothing is forecast, there is no formula to pin. What must be verified against the real tmux binary
is that applying `tiled` at a given pane count yields panes clearing the effective floor, that `join-pane`
and `break-pane` land panes where expected with user-options intact and without stealing focus, and that the
tool's reported measurements match what tmux reports independently.

The agent readiness floors are **data, not behavior**: they come from the measurement recorded in the Problem
Statement and are re-measured when an agent's TUI changes, like the `agents.toml` comments citing *validated
against cursor-agent 2026.05.28*. They are not asserted in the suite, because that would require launching
real agents in CI.

### Secondary: pure unit tests

Effective-floor resolution over a set of panes and a registry, floor comparison, and overflow-candidate
ordering are pure, and are unit-tested inline in the layout module, as `core/src/idle.rs` already tests its
helpers. These test the tool's own logic; they make no claim about tmux.

### Prior art

The suite's serialization guard, per-pid unique naming, and RAII cleanup guards are the model for the new
tests; the existing test that hand-builds a `TMUX` triple is the direct precedent for the private-server
harness.

### Explicitly not built

No trait or port abstracting tmux geometry queries. `tmux::with_invocation` already redirects tmux calls and
the CLI seam covers the integration path.

## Out of Scope

- **Predicting geometry** from a model of tmux's tiling algorithm. Rejected on measurement.
- **Cross-process locking** between concurrent spawns. Cannot cover direct tmux splits.
- **Repairing an over-full window** after a concurrent overshoot or a raw split. Surfaced, not fixed.
- **Automatic reclamation of exited-agent panes.** Would destroy preserved crash output.
- **A per-invocation floor override.** Floors live in the registry.
- **A pane-count cap** separate from the floor. Concurrency limits belong to orchestrating skills.
- **Saved-layout capture or a restore verb.** tmux cannot restore across a pane-count increase.
- **A persistent global configuration file.** The registry already exists.
- **Lineage-derived pane names.** The auto-derived `@tt-name` (§ Persistent shape) comes from the
  command, not from the calling pane's name. Deriving a child name from its parent would read
  `$TMUX_PANE` — the value this epic exists because it is unreliable under concurrent spawns — and
  would need a fallback chain for unnamed parents, human shells, and invocations from outside tmux.
  The lineage idea pays off for agent swarms, and swarms go through `spawn-agent`, which already
  registers unconditionally.
- **Widening ownership for layout alone.** A second, looser predicate used only for tiling
  eligibility would deliver the same reach as the auto-name, at the cost of two ownership notions
  that can drift. Rejected for that reason; `ISSUE-260801-1310-02` exists to collapse ownership to
  one predicate, not to fork it.
- **Searching outside the current session for an overflow destination.** Candidates are the session's
  eligible marked windows; beyond those, a fresh window.
- **Unifying the two divergent copies of the skill documentation.** Both are updated; reconciling them
  permanently is its own work.
- **Exposing socket or invocation control as a CLI flag.** The environment variable suffices.
- **Migrating driver skills.** None pass layout flags; the default changes under them by design.
- **Asserting agent readiness floors in the test suite.** Measured data, not behavior.
- **Geometry columns in `list`.** `status` carries the inspection surface.

### Settled at triage, not here

Deliberately left to the issues, where the code is in front of the implementer: exact concise-format key
spelling and ordering; the `status` JSON field names for geometry and floor verdict; and the precise
error text for a `--split window` failure. These are naming and encoding choices with no design content,
and pinning them here would only create a second place to keep them correct.


## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement · ## Solution |
| scope boundary | decided | ## Out of Scope · § Settled at triage |
| domain terms | decided | CONTEXT.md — glossary seeded for this epic; `contexts:`/`terms:` populated |
| architecture shape | decided | § Module shape — this PRD is the record; repo has no ADR practice |
| stack | n/a | brownfield — existing Rust/clap/anyhow/serde conventions ratified, no new dependencies |
| data/schema | decided | § Persistent shape · § Ownership · § Layout intent |
| contracts/integrations | decided | § Flags · § Placement and overflow · § Documentation · ## Testing Decisions § tmux-behavior tests |
| UX | decided | § Flags · § Reporting surface · ## Solution (default change stated) |
| testing/seams | decided | ## Testing Decisions |
| NFRs | decided | § Accepted limits — the README's sub-10ms figure is binary startup, which this epic does not touch; added round-trips land on the two least frequent verbs |
| ops envelope | n/a | installed from source; no CI or release pipeline. The out-of-repo doc copy is an edit target, not a release step |

## Further Notes

**Documentation drift.** The two copies of the tool's skill documentation have diverged in both directions —
88 lines present only in the installed copy, 63 only in this repo's. Both are updated by this epic, but the
drift will recur on the next feature and is a latent source of agents acting on stale facts about their own
tools. It deserves its own issue.

**Why the failure was invisible.** The compounding shrink has been present for as long as the 70% default has,
and the reason it read as cosmetic is that its real cost surfaced somewhere else entirely — as readiness
timeouts, attributed to slow agents rather than to small panes. The existing suite's own header comment records
the mechanism, noting that concurrent splits against the same pane run into tmux's size constraints.

**Gate amendment, 2026-08-02.** The `/to-issues` readiness gate surfaced five decisions this PRD had left
implicit, each of which the briefs could not be written without. The user adjudicated all five and they are
recorded above rather than in the issues alone: partial floor overrides read the *merged* profile so a
builtin's untouched axis survives (§ Floor); `@tt-tiled` is a record of what the tool did — it gates teardown
and overflow but never placement, so a flagless spawn re-tiles a window whose marker a manual split cleared
(§ Layout intent); `--split tile --size N` is rejected at parse (§ Flags); `--any` governs destruction only and
does not reach the layout guard (§ Reclamation on kill); and `status` gains its new fields in `concise`/`json`
with `raw` byte-for-byte unchanged, and is the surface on which a relocated pane's crowded *origin* is
inspected (§ Reporting surface). No new deferral was created and no approved decision was overturned. The
same gate also split the tile-default slice in two, which is recorded in the `issues:` list.

**Breakdown-adversary amendment, 2026-08-02.** The `/to-issues` step-7 adversarial pass over the whole
breakdown found what nine per-record readiness gates could not, because those gates ask whether a brief can be
*gamed* and this one asks whether the plan collides with the code. Three findings were brief-level and fixed in
place: the shared launch seam is smaller than one brief assumed (it stops at pane creation and discards the
name, and the timestamp helper is binary-crate-only, so registration cannot simply ride along); one ownership
boolean cannot by itself choose between the two safety denial messages, which the predicate's optional,
fail-open cwd argument resolves by being called twice; and `kill`'s existing safety check answers a
*target-pane* question while the layout guard asks a *window* one, so a mixed window — unowned target, owned
sibling — needs the window's panes enumerated rather than the target's verdict lifted.

The fourth was structural and reached this PRD. The Solution promised that "a flagless `launch` … will tile
instead", but `launch` without `--name` registered neither `@tt-name` nor `@tt-agent`, so its panes were never
"ours", so such a window never became eligible and a repeated bare `launch --cmd …` would have kept the
compounding 30:70 shrink forever. `spawn-agent` was unaffected, so the motivating agent-swarm case worked and
the shortfall sat exactly on the invocation shape the README documents first. Of the four ways out — narrow the
promise, widen ownership for layout only, always register, or leave it — the user chose to always register, and
accepted the resulting change to the destructive verbs. `ISSUE-260802-0628-01` owns it; § Persistent shape
records the widening and its bounds, and the two rejected alternatives are in § Out of Scope with their
reasons.

**Measurement provenance.** The floors, the resize-tolerance result, and the `join-pane`/`break-pane` semantics
were measured on an isolated tmux server against `codex --sandbox read-only`, `cursor-agent`, and
`claude --permission-mode dontAsk`, using each profile's live `ready_regex` and `ready_lines` from
`~/.config/tmux-tools/agents.toml`, on 2026-08-01 with tmux 3.7b.
