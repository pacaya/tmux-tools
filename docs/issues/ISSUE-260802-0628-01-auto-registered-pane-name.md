---
id: ISSUE-260802-0628-01
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: every pane this tool creates registers @tt-name, auto-derived from the command when --name is absent
prd: PRD-260801-0656-01
terms: [Ownership, Pane registration]
blocked_by: [ISSUE-260801-1310-01]
---

## Agent Brief

**Category:** enhancement
**Summary:** `launch` and `spawn-agent` always register a pane name; without `--name` it is derived
from the command plus a random suffix

**Current behavior:**
`@tt-name` is written only when `--name` is passed. A pane created by a flagless
`tmux-tools launch --cmd <something>` carries `@tt-launched-at` and `@tt-cwd` but no name and no
agent, so it leaves no evidence that this tool created it.

Two things follow from that absence, and both are wrong.

The guarded verbs refuse to touch it. A set of verbs shares one ownership check whose stated rule is
that they act only on panes created by tmux-tools; the check reads `@tt-name` and `@tt-agent` as its
evidence, finds neither, and denies with "not created by tmux-tools". The pane *was* created by
tmux-tools. This is not a protection that was designed — it is the guard failing to recognize its own
output, and the only way past it today is `--any`, which also switches off the unrelated cross-cwd
guard.

`rg -n 'enforce_for_pane' src/` returns the shared helper's definition and one line per guarded verb;
that list, not this paragraph, is the set. Work from it, because the set is not what the shorthand
"the destructive verbs" suggests: most of its members destroy or signal, but `send-enter` submits a
keystroke, which is a different kind of act and the one a reader skims past. It is deliberately in
scope here — see the bounds below.

And the window never becomes eligible for layout. Ownership is the same predicate for tiling as for
destruction, so a window containing only bare-`launch` panes is never one "we own", so nothing
retiles it. Repeating `tmux-tools launch --cmd foo` in one window therefore keeps the compounding
30:70 shrink indefinitely — which is precisely the behavior this epic exists to end, on the
invocation shape the README documents first. `spawn-agent` does not have the problem: it registers
`@tt-agent` unconditionally.

**Desired behavior:**
Every pane either verb creates carries a non-empty `@tt-name`. With `--name`, it is the value
passed, exactly as today. Without, it is derived from the command being launched plus a random
suffix — `npm-a3f2`, `cargo-91bd` — falling back to a fixed stem when no usable word can be taken
from the command.

The consequences are intended, and are the point of the slice:

- Ownership evidence becomes honest, so the shared predicate answers correctly for these panes
  without being loosened. There is still exactly one ownership rule, unchanged in meaning.
- Every guarded verb acts on them without `--any`. **This is a deliberate widening** — see the
  bounds below.
- Their windows become eligible for layout, which is what lets the tiled default reach the flagless
  `launch` path rather than only the agent path.
- They become addressable by `--target <name>`, which they were not before.

**Bounds on the widening.** The verbs are single-target: each resolves one pane, checks it, and acts
on it. There is no sweep verb today and this slice adds none, so nothing newly destroys panes in
bulk. `@tt-cwd` is still written unconditionally, so a pane belonging to another project's run is
still refused without `--any` — only the *reason* for the refusal changes, from "not created by
tmux-tools" to "owned by another cwd". What becomes reachable is one explicitly targeted pane, in
the caller's own cwd, that this tool created.

**On `send-enter` specifically.** Submitting a keystroke is a broader class of act than destroying a
pane, so it deserves its own sentence rather than absorption into "the destructive verbs". Two facts
bound it. First, the same three-part limit applies: single target, caller's own cwd, a pane this tool
created. Second — and this is the part that decides it — `send-enter` is the only guarded way to
deliver **Enter**, and it is far from the only way. (Other guarded verbs do deliver keystrokes —
signalling and escaping are keystrokes too — so the claim is about Enter specifically, which is what
the argument rests on.) Derive the rest rather than believing it: compare
`rg -n 'enforce_for_pane' src/` against `rg -n 'send-keys' src/`. Every verb in the second list and
absent from the first delivers input to any pane on the server without consulting ownership at all,
and the difference between those lists is not a short one — it includes sending arbitrary text,
sending text with a trailing Enter, and running a command. So Enter is already deliverable to a
stranger's pane by several routes. What the guard blocks is one verb's spelling of an act its
siblings perform freely, and only on panes whose creator left no evidence. That is an inconsistency
rather than a protection, and closing it is the intent rather than a side effect.

**Key interfaces:**
- The registration block in each spawning verb, which today writes the name conditionally on the
  flag. Both verbs change; the invariant to establish is "every pane this tool creates carries a
  name", with no exceptions to reason about later.
- **The derivation is delegated, under three bounds.** It must take a recognizable word from the
  command so the name is useful in `list` output; it must produce a value that is valid both as a
  tmux option value and as a `--target` name; and **a collision must never surface as a launch
  failure**. That last bound is the sharp one: the name-setting path validates uniqueness across the
  whole server and returns a hard error when the name is taken, so a derivation with too little
  entropy converts a valid `launch` into a failed one — strictly worse than the bug being fixed.
  Either make collision negligible or handle it by re-deriving; do not let it reach the user. Note
  the failure is not merely a non-zero exit: today the pane is created *before* the name is set, so a
  rejected name leaves an orphan pane behind and the error arrives after the damage. Verify this
  ordering yourself rather than taking it on trust — it is what makes an unhandled collision worse
  than a clean failure, and it also means re-deriving after a rejection is cheap, since the pane
  already exists and only the option write needs retrying.
- **The reported name must come from the registration, not from the flag.** The output rendering
  path currently branches on whether `--name` was passed, so registering a name without threading
  the resolved value through leaves a flagless `launch` printing a pane with no name while the pane
  itself carries one. This is the same shape as the input-versus-read-back problem recorded in
  `ISSUE-260801-1310-04`; here it is about a single field.
- Documentation: README and SKILL both describe `--name` as what makes a pane addressable. That is
  no longer the whole truth. The installed skill copy is a separate file from the in-repo one and
  drifts from it; both need the correction, and the PRD's § Documentation gives its path. Two things
  to check there rather than assume: whether the installed copy is a separate git repository of its
  own, and whether it is hardlinked to another path such that editing one silently updates both.
  Layout and tiling documentation is **not** this record's — `ISSUE-260802-0128-01` owns it.

**Acceptance criteria:**
- [ ] A flagless `launch --cmd <shell>` creates a pane whose `@tt-name` is non-empty. Observable at
      the CLI seam against the private test server: an integration test creates the pane, then reads
      the option back from tmux and asserts non-empty. The same assertion fails before this change,
      because the option is unset — run it against the pre-change binary to confirm it goes red
      rather than assuming it does.
- [ ] The same holds for a flagless `spawn-agent`. This one must **fail** rather than skip when the
      behavior is absent: the existing spawn-agent test is conditional on an agent binary being
      installed, and a test written to that precedent would be green-by-skip in both directions.
      `Registry::load()` honours `XDG_CONFIG_HOME`, so a stub registry entry is one way; asserting
      the registration rather than the agent's readiness is another.
- [ ] Concurrent flagless launches of the **same** command into one window all exit zero and yield
      pairwise-distinct names. This is the collision bound made observable; a derivation keyed only
      on the command word passes every other criterion here and fails this one.

      **Use three panes, and do not raise the count to make the evidence feel stronger.** Tiling is
      out of scope here, so the compounding 30:70 split still applies and the window runs out of room
      quickly: measured on a private server at the harness's fixed 280×82, six concurrent flagless
      launches leave four succeeding and two failing with tmux's no-space error — the split
      arithmetic (280 → 196/83 → 58/24 → 16/7 → 4/2) makes four-succeed deterministic rather than
      incidental. A criterion written at a count above that ceiling can never go green, for a reason
      having nothing to do with name derivation. Three is comfortably inside it and is enough to
      falsify a command-only derivation. If stronger evidence is wanted, add
      panes across *separate windows* rather than more panes in one — uniqueness is validated
      server-wide, so that exercises the same check without competing for width.
- [ ] A flagless `launch --format json` emits a non-null `name`, and that value equals the created
      pane's `@tt-name` read back from tmux. The non-null half is what goes red before this change —
      today the field is null — and it is what catches a fix that registers the name but renders
      from the flag. Assert both halves; the equality alone is satisfied vacuously by the old
      behavior, where neither side has a name.
- [ ] `kill` succeeds, **without** `--any`, on a pane created by a flagless `launch` in the current
      cwd. Before this change the same invocation is denied with a message naming "not created by
      tmux-tools". This is the accepted widening, asserted as a positive control rather than argued.
- [ ] The cross-cwd guard still refuses such a pane, and refuses it *as* a cwd violation: with the
      pane's recorded `@tt-cwd` differing from the caller's, `kill` without `--any` denies with a
      message naming the other cwd. Before this change the same case is also denied, but for the
      wrong reason — so assert the message, not merely the refusal, or the criterion is green in
      both directions.
- [ ] Regression guards, each already true at baseline and therefore proving nothing about this
      change on its own — they exist to catch collateral damage, and should be read that way: an
      explicit `--name foo` still registers exactly `foo`; two panes explicitly given the same name
      still produce the existing collision error; and `--format raw` still emits the bare pane id,
      byte-for-byte.
- [ ] README and the in-repo SKILL no longer imply `--name` is required for a pane to be named or
      addressable. Verify the installed skill copy separately — it is a different file and does not
      inherit repo edits.
- [ ] `cargo test --workspace` passes, run against an **isolated** tmux server. Note `--workspace`:
      cargo tests only the member it is invoked from, so a bare `cargo test` at the root silently
      skips its sibling members — including the one holding the option-setting and
      uniqueness-validation code this slice leans on. Compare the `Executable` lines printed by
      `cargo test --no-run` against those printed by `cargo test --workspace --no-run`; the second
      lists a binary the first does not, and neither executes a test. Read that comparison as
      evidence for the *skipping*, not as an inventory of members — a member with no test targets
      would not appear in either list.

      Be precise about which code sits where, because two things are easy to conflate: the shared
      option-setting machinery is in the core crate, while the per-verb registration blocks this
      slice actually edits are in the binary crate, and relocating those is `ISSUE-260801-1310-04`'s
      work rather than this slice's. The ownership predicate is in the binary crate too and stays
      there for this slice's duration; `ISSUE-260801-1310-02` is what moves it.

      Some tests in this suite are environment-dependent rather than slice-dependent, and the set is
      **open** — do not treat it as a named pair. A failure that survives isolation is not this
      slice's to fix unless this slice introduced it. The PRD's Testing Decisions states this as a
      general rule and names examples rather than an inventory, so meeting an environment-dependent
      failure it does not name is expected, not evidence that the failure is yours.

**Out of scope:**
- Tiling, floors, relocation, and the `@tt-tiled` marker. This slice makes windows *eligible*; it
  changes no placement.
- Moving registration into the core crate. `ISSUE-260801-1310-04` owns that move and carries this
  record's change through it.
- Extracting the ownership predicate. `ISSUE-260801-1310-02` owns that; this record changes what the
  predicate *sees*, not where it lives or how it is spelled.
- Deriving the name from the calling pane's name. Rejected in the PRD: it would read `$TMUX_PANE`,
  the value this epic exists because it is unreliable under concurrent spawns, and would need a
  fallback chain for unnamed parents, human shells, and invocations from outside tmux.
- Any change to `--any`, `--force`, or the cross-cwd rule. The widening in this slice comes entirely
  from panes becoming honestly owned, not from relaxing a check.
- Renaming or backfilling panes that already exist. This applies to panes created after it lands.

## Triage Notes

Minted by the `/to-issues` step-7 breakdown-adversary pass, which found that the PRD's Solution
promised a tiled default for "a flagless `launch`" that no brief delivered — the nine per-record
readiness gates could not see it, because each judged one brief against itself rather than the set
against the code.

The user chose this remedy over three alternatives, and accepted the widening of the guarded verbs
that comes with it. Of the rejected alternatives, the second looser ownership predicate for layout
alone is recorded in the PRD's § Out of Scope with its reason; narrowing the PRD's promise is in
§ Further Notes, listed among the four options considered rather than argued down separately.

**Readiness gate (cold-reader): FAIL** (round 1)

Two blocking findings, both fixed in place before re-gate, plus four non-blocking fixes folded into
the same edit.

- **Class 7 — the widening was enumerated over three verbs and the shared check gates four.** The
  brief and the PRD both said "`kill`, `interrupt`, and `escape`". The enforcement helper has a
  fourth call site: `send-enter`. The reader demonstrated the consequence live on a private server —
  `send-enter` against a flagless-launch pane is refused today, and after setting `@tt-name` by hand
  the same invocation succeeds. So keystroke submission was inside the accepted widening and named
  nowhere. Fixed in both artifacts, with `send-enter` called out on its own rather than folded into
  "the destructive verbs", and the reader's method — enumerate the helper's call sites — written in
  so the count is re-derived rather than trusted.

  Checked while fixing, and it reframes the finding: the sibling verb that sends *arbitrary text* is
  not guarded at all. Writing into any pane on the server is already unrestricted; the guard blocks
  only the closing Enter, and only on panes whose creator left no evidence. That is an inconsistency
  rather than a protection.

- **Class 7 — "the `core` package where the name-registration and ownership code lives" is false for
  ownership.** Registration is in core; the ownership predicate is inline in the binary crate's
  safety evaluator, and this record's own Out of scope says so — the brief contradicted itself two
  sections apart. The `--workspace` instruction survives on the registration half alone. Fixed, and
  the criterion now states where ownership lives for this slice's duration.

Non-blocking, folded in: the concurrency criterion is now pinned at three panes with the ceiling
explained — tiling is out of scope here, so the compounding split still applies and six concurrent
flagless launches at 280×82 leave two failing on tmux's no-space error, meaning a criterion written
at a larger count could never go green for reasons unrelated to naming. The collision bound now
records that the pane is created before the name is set, so an unhandled collision orphans a pane
rather than merely failing. The suite criterion picks up the PRD's known-flaky disposition rule. And
the documentation bullet points at the PRD for the installed copy's path, with the hardlink and
separate-repository questions named as things to check rather than assume.

Everything else was verified red at baseline against a private server, including all six behavioral
criteria: `name=[]` read back from a flagless launch, `"name":null` in JSON, the ownership denial on
`kill`, and — from `/tmp` — a cross-cwd attempt still producing the ownership message rather than one
naming a cwd, which is what the sixth criterion requires.

**Readiness gate (cold-reader): PASS** (round 2)

Both round-1 findings verified closed against the code and live behavior. The four call sites of the
shared enforcement helper were enumerated independently and match; `send` is confirmed absent from
them; and on a private server all four guarded verbs refused a flagless-launch pane while `send`
succeeded on it — then setting `@tt-name` by hand made `send-enter` and `kill` succeed, reproducing
the round-1 demonstration exactly. The PRD's enumeration was checked to agree, and no stale
three-verb wording survives in either artifact. Every other load-bearing premise was re-derived:
pane-created-before-name-set (orphan pane observed directly after a duplicate-name failure), the
server-wide uniqueness check, the renderer branching on the flag, and the three-pane concurrency
figure.

**Readiness gate: REOPENED** (round 2)

Reopened immediately for three corrections from the same report, two of which are mine to own.

- **A figure I wrote was wrong.** The concurrency bullet claimed that at 200×50 "the fourth onward
  fail" — measured, it is the **fifth** onward; four succeed there as at 280×82. The criterion runs
  at 280×82, where the figure reproduces exactly, so nothing built changes. But a false measurement
  in a brief is a false measurement. Replaced with the 280×82 figure alone plus the split arithmetic
  that makes four-succeed deterministic rather than incidental, which is the part actually worth
  knowing.
- **The `send-enter` argument understated its own case.** I wrote that "only the final Enter is
  gated". The reader found Enter is *also* deliverable through the unguarded text-sending verb's
  trailing-Enter form and through the run-a-command verb, neither of which consults ownership — so
  unguarded paths to the same pane already existed. (That round's phrasing put a number on them;
  round 4 replaced it with the derivation now in the brief.) That strengthens
  the argument rather than weakening it — the guard blocks one verb's spelling of an act three others
  perform freely — and both artifacts now say so.
- **The suite criterion conflated two things.** The option-setting and uniqueness machinery is in
  `core`; the per-verb registration blocks this slice edits are in the binary crate. Saying "the
  name-registration code lives in core" was true enough to pass and loose enough to mislead — the
  same two-sections-apart shape as round-1's second finding, milder. Split explicitly.

Re-gating at round 3; none of this text has been read by a gate.

**Readiness gate (cold-reader): FAIL** (round 3)

All three round-2 corrections verified accurate — the reader reproduced the 280×82 concurrency chain
down to the separator arithmetic, confirmed the retracted 200×50 figure's replacement is right,
demonstrated the unguarded keystroke paths live against a pane created by raw `tmux new-window`, and
checked all three code placements in the suite criterion. Every behavioral criterion was executed and
is red at baseline, including the cross-cwd one, whose post-state it also proved reachable by setting
`@tt-name` by hand and watching the denial change to one naming the other cwd.

The failure is authoring form, on facts that are all currently true.

- **Class 7 kind (a) — two occurrence counts asserted as fact with no discovery command.** "**Four**
  verbs share one ownership check", and "At least three unguarded paths reach the same pane". Both
  correct today; both forbidden on brief text under edit regardless, because the count decays and a
  reader treats it as an assertion rather than re-deriving it. My prose hedges — "enumerate the four
  from the code, not from this sentence" — are precisely the decoration the rule disallows: they tell
  a reader to verify without giving them the command, so the number is what survives.

  The first surface is also the one that carried a *wrong* figure at round 1, three instead of four,
  and my round-2 remedy was to refresh the number. That is the one remedy the rule forbids, and it
  left the same defect in place one round later with a correct value in it. Both are now qualitative
  and paired with commands: `rg -n 'enforce_for_pane' src/` for the guarded set, and that compared
  against `rg -n 'send-keys' src/` for the unguarded input paths — the difference between the two
  lists *is* the claim, so a reader derives it rather than counting from prose.

Fixed upstream in the same pass, from the same report's first non-blocking observation: the PRD's
Problem Statement carried the identical off-by-one this record retracted at round 2, claiming the
fourth spawn fails on a 200-column terminal. Measured, four succeed and the fifth fails; the column
chain was slightly off too. Corrected there and marked as measured.

Re-gating at round 4.

**Readiness gate (cold-reader): FAIL** (round 4)

Both round-3 repairs verified compliant and accurate — the reader ran each command, confirmed the
guarded set is exactly what the first returns, and confirmed the difference between the two lists is
exactly the unguarded input-sending verbs, then demonstrated the asymmetry live against a pane created
by raw `tmux new-window`. Two further class 7 kind (a) surfaces, both in the suite criterion, both
fixed.

- **"Two tests in this suite are environment-dependent."** A closed count over an **open** set, and
  the cited source says so explicitly: the PRD states the rule is general and that the pane-creating
  suite has more than one such assertion, naming examples rather than an inventory. My wording also
  claimed the PRD "names both tests and the underlying cause" when it gives a cause for one and only
  an observation for the other. The practical damage is real rather than formal: an agent meeting a
  third environment-dependent failure would, on my wording, conclude it must be theirs. Rewritten to
  say the set is open and that meeting an unnamed one is expected.

- **"this repo is a two-member workspace."** An inventory figure over decaying state. The
  `Executable`-line comparison beside it is not authoritative for it either — that comparison derives
  test *binaries*, and a member with no test targets would appear in neither list. Rewritten to drop
  the count and to say what the comparison is evidence for.

**This one has batch scope, and it is worth stating rather than quietly fixing here.** The same
sentence was templated into five sibling records, all of which now carry pass stamps, so the text is
covered where it stands. Round 3's reader saw it and let it pass as the batch's settled form; round 4
judged it structurally identical to the "Four verbs" surface it had just rejected. Round 4 is right on
the rule as written. The consequence is narrow — those records are stamped and their gates are closed
— but any of them reopened for another reason should expect this sentence to fail, and the corrected
phrasing here is the one to copy.

Non-blocking, fixed anyway: "`send-enter` is the only guarded way to deliver a keystroke" is literally
false, since signalling and escaping are keystrokes and both of those verbs are guarded. The claim the
argument actually needs is about **Enter** specifically. Corrected in the brief and in the PRD, which
carried the same imprecision. Also reworded the round-2 note's surviving present-tense count so it
reads as superseded history rather than a live assertion.

Re-gating at round 5.

**Readiness gate (cold-reader): PASS** (round 5)

All three round-4 repairs verified. The open-set wording matches what the PRD's Testing Decisions
actually says, including its "not the only such test". The workspace count is gone, the surviving
skipping claim is true, and the caveat is correct — a member with no test targets would indeed appear
in neither `Executable` list. And `send-enter` is in fact the only *guarded* verb that delivers Enter:
the reader read the key literal at each `send-keys` site and confirmed the others send `Escape` and
`C-c`, so the corrected claim holds where the original did not.

This round the sweep was exhaustive rather than impressionistic — mechanically extracted every
numeral, number-word, inventory size, occurrence count and repo-state assertion in both sections and
returned a verdict per item across sixty-five surfaces, which is what rounds 3 and 4 each failed to do
and why each of them surfaced a fresh pair the previous had walked past. Nothing in the brief fires.
All six behavioral criteria were re-executed at baseline and are red, with criterion 6's post-state
proved reachable by setting `@tt-name` by hand and watching the denial change to one naming the other
cwd.

Three things left deliberately, recorded so a later round does not re-derive them:

- **The strongest surviving figure is in this journal, not the brief** — round 4's own note that the
  workspace sentence "was templated into five sibling records, all of which now carry pass stamps".
  That is a live count plus a record-state assertion with no command behind it, structurally the shape
  rounds 3 and 4 rejected. It is gate narrative the brief does not rely on, and the reader verified it
  is accurate. Left as written; if this record is ever edited for another reason, that sentence is the
  one to make command-backed.
- **Round 4's own remedy was incomplete.** It claimed to have de-tensed the round-2 note's surviving
  count; the parenthetical landed but the bullet still ends with a present-tense "three others". True,
  and history, but the stated fix did not fully land — noted rather than patched, since patching gate
  history to look tidier is how the next reader loses the trail.
- **The PRD still carries the bare counts this brief retired** — four verbs, at least three siblings.
  The authoring rule binds briefs rather than PRDs, so it is not a finding here, but the next brief
  derived from that section will inherit the phrasing unless it derives the sets itself.
