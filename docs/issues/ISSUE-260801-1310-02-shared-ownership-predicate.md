---
id: ISSUE-260801-1310-02
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Extract the pane-ownership predicate so layout and the destructive verbs share one definition
prd: PRD-260801-0656-01
terms: [Ownership, Pane registration]
---

## Agent Brief

**Category:** enhancement
**Summary:** Lift the ownership rule out of the safety evaluator into a shared function, with no behavior change

**Current behavior:**
Whether a pane belongs to this tool is decided inside the safety evaluator that guards the
destructive verbs. The rule has two parts: a pane is owned when `@tt-name` **or** `@tt-agent` is
set; and separately, when the pane's recorded `@tt-cwd` **and** the current working directory are
both known, they must match. That second check is deliberately **fail-open** — a pane with no
recorded cwd, or an invocation that cannot read its own cwd, passes.

That logic is reachable only through the safety path, which also performs self-pane and override
handling. Forthcoming layout work needs the same ownership question answered without any of the
destructive-verb machinery around it.

**Desired behavior:**
Ownership is a single named predicate that both the safety evaluator and future callers use. Its
semantics are exactly today's, including fail-open on an absent cwd on either side. The safety
evaluator delegates to it rather than restating it.

The predicate is **public and lives in the core crate**. Both the safety evaluator (binary crate)
and the forthcoming layout module (core crate) must answer this question identically, and the
dependency runs one way — the binary depends on core, never the reverse. A predicate left in the
binary crate would be unreachable from core, and one private to a core module would force the
binary to restate it; either defeats the purpose of this issue.

This is a pure refactor: no verb changes behavior, no new flag, no new persisted field.

**Key interfaces:**
- `pane_is_owned` — takes the pane's registration record plus the current working directory and
  returns whether the pane is owned by this invocation. The current working directory is
  **optional**: when it is not known, the cwd comparison is skipped and the pane is owned on the
  strength of its registration alone. The same holds when the pane has no recorded cwd. Both arms
  fail **open**, matching today's behavior; a signature that cannot represent an absent current cwd
  would push that arm back out to the caller and reintroduce the duplication.
- The safety evaluator — keeps its self-pane check, its override handling, and its two distinct
  denial messages; only the ownership determination moves.
- **How one boolean still yields two messages.** Read literally, "a predicate returning owned/not-owned"
  and "keep both denial messages" look contradictory: an unregistered pane and a registered pane from
  another cwd both collapse to not-owned, and the evaluator would have no way to pick a message. They
  are not contradictory, because the predicate's cwd argument is optional and fails **open**. Calling
  it twice decomposes the reason — once with no current cwd, which tests registration alone and
  distinguishes "not created by tmux-tools"; then, if that passes, once with the real cwd, where a
  false result can only mean the cwd mismatched. Any equivalent decomposition is fine; what is not
  fine is reintroducing a field-presence test in the evaluator to tell the cases apart, which the
  criteria below forbid. The existing message-content assertions pin both strings, so a wrong shape
  goes red rather than silently merging the two denials.

**Acceptance criteria:**
- [ ] A public `pane_is_owned` exists in the core crate: `rg -n 'pub fn pane_is_owned' core/src`
      returns its definition; no matches before this change.
- [ ] The safety evaluator **calls** it, as a call expression rather than a mention:
      `rg -n 'pane_is_owned\s*\(' src/cmd/safety.rs` returns at least one match; no matches before
      this change. This is a separate criterion from the one above because a definition-anchored
      pattern cannot see a call site — a `fn pane_is_owned` search would be satisfied by a core-crate
      predicate that nothing calls.
- [ ] The obvious restatement is gone: `rg -n '(name|agent)\.is_some' src/` returns exactly one match
      before this change — the evaluator's inline expression — and none after. Treat this as a **cheap
      necessary signal, not a proof.** `.as_ref().is_some()`, `!x.is_none()`, `Option::is_some(&x)`,
      `matches!`, a local trait method, a helper closure — all are ordinary Rust, all are the same
      two-field presence test, and none of them match this pattern. The space of spellings is open, so
      no regex closes it. Five gate rounds were spent discovering that; the criterion below is what
      actually decides the issue.
- [ ] **Reviewer check — the real acceptance test.** By reading `evaluate` in `src/cmd/safety.rs`,
      confirm both: (a) its ownership answer is the return value of the **core** predicate, on a live
      path — not dead code that merely mentions it, and not a same-named local function shadowing the
      import, which is the failure mode a call-site grep cannot distinguish; and (b) no equivalent
      field-presence test survives anywhere in `src/`, under any spelling. This is a bounded check on
      one ~35-line function plus one grep-assisted sweep, not open-ended judgement — but it is manual,
      and the brief says so rather than dressing it up as mechanical.
- [ ] The predicate returns owned for a pane with `@tt-name` or `@tt-agent` set and no recorded
      cwd (fail-open preserved), and for one whose recorded cwd matches the current directory.
- [ ] The predicate returns owned when the **current** working directory is unknown, even though
      the pane records a cwd that would not have matched — mirroring the existing safety test for an
      unknown current cwd, but asserted at the predicate itself rather than only through the verb.
- [ ] It returns not-owned for a pane with neither `@tt-name` nor `@tt-agent`, and for one whose
      recorded cwd differs from the current directory.
- [ ] The existing safety unit tests pass unmodified, including the ones covering an owned pane
      with no recorded cwd and an unknown current cwd.
- [ ] `cargo test --workspace` passes and no verb's observable behavior changes. `--workspace` is
      required, not stylistic: a bare `cargo test` from the repo root tests only the binary package
      and skips the `core` package entirely — which is where this issue's new predicate and its unit
      tests live, so the bare form would not execute them.

**Out of scope:**
- Changing the ownership rule itself — tightening it to require `@tt-cwd`, or making it
  fail-closed. The point of this issue is that layout and the destructive verbs answer the
  question identically; changing the answer is a separate decision that was explicitly rejected.
- Making unnamed, agentless `launch` panes owned. `ISSUE-260802-0628-01` does that, and does it by
  registering a name on every pane this tool creates — so the panes become owned under the predicate
  extracted here, with the predicate itself untouched. Nothing about the rule changes in either
  record; do not anticipate that change here, and do not treat the panes it affects as a special
  case.
- Any layout, tiling, or placement behavior.
- Moving the safety evaluator itself between crates.

## Triage Notes

**Readiness gate (cold-reader): FAIL** (round 1)

Three blocking gaps, all fixed in place before re-gate:

- **Class 9 arm A** — the acceptance observable pre-matched. `rg -n --pcre2 --multiline
  '(?s)pub fn [a-z_]*own[a-z_]*\s*\(' core/src src` returned `core/src/tmux.rs: pub fn
  run_checked_owned`, because `[a-z_]*own[a-z_]*` swallows `run_checked_` + `own` + `ed`. The
  criterion was green at baseline and could never go red. Replaced with `rg -n 'fn pane_is_owned'
  core/src src`, verified to return no matches at gate time.
- **Class 4 — crate placement.** The brief did not pin which crate owns the predicate, and its two
  referenced artifacts pointed opposite ways: the glossary anchored Ownership to the binary crate's
  safety evaluator while the PRD placed the predicate inside the core layout module. The binary
  depends on core and never the reverse, so only a public core-crate predicate is reachable from
  both callers. Fixed in the brief, and upstream in both artifacts — the PRD now states the
  predicate is public in core rather than layout-internal, and the glossary anchor now names the
  shared predicate with the safety evaluator delegating to it.
- **Class 4 — unfalsifiable fail-open arm.** The prose pinned fail-open on an absent cwd on either
  side, but no criterion exercised the *unknown current cwd* arm, so a fail-closed predicate taking
  a non-optional cwd would have satisfied every criterion while forcing the layout caller to
  re-implement the arm outside it. Added an explicit optional-cwd contract to Key interfaces and a
  criterion asserting it at the predicate.

**Readiness gate (cold-reader): FAIL** (round 2)

One blocking gap, fixed in place before re-gate:

- **Class 9 arm A — the delegation half of the first criterion was unobservable.** The criterion
  claimed `rg -n 'fn pane_is_owned' core/src src` would return "its definition in the core crate and
  its call from the safety evaluator". It cannot: the `fn ` anchor matches a definition, and a call
  site never contains the substring `fn pane_is_owned`. Since the remaining criteria either test the
  predicate in isolation or assert end-to-end evaluator behavior — identical whether the evaluator
  delegates or keeps its inline boolean — an implementation that added the predicate to core and
  changed `safety.rs` not at all satisfied the whole set while removing none of the duplication the
  issue exists to remove. Split into three criteria: the definition in core, a call site in
  `safety.rs`, and the disappearance of the inline `registered.name.is_some() || …` expression, each
  verified against the tree at gate time (no matches, no matches, and one match at
  `src/cmd/safety.rs:57` respectively).

Round-1 fixes re-verified as holding: the `run_checked_owned` false positive is gone, PRD § Module
shape and CONTEXT.md now both place the predicate in core, and the unknown-current-cwd arm is pinned
by its own criterion.

**Readiness gate (cold-reader): FAIL** (round 3)

The round-2 trio was still defeatable, demonstrated with constructed counterexamples rather than
argued:

- **Class 9 arm A.** The call-site criterion matched any textual occurrence of the identifier, so a
  bare comment — `// see pane_is_owned in core (future work)` — turned it green with no call. And the
  inline-removal criterion was a literal-order string match, which a semantics-preserving operand
  reorder (`agent.is_some() || name.is_some()`) defeats outright. Together: add a predicate nothing
  calls, drop a comment, reorder two operands, and all three criteria pass while the duplication
  survives untouched.

Fixed by moving both patterns onto things a rewrite cannot dodge. The call-site check now requires a
call **expression** (`pane_is_owned\s*\(`). The removal check now targets the **field reads**
(`registered\.(name|agent)\.is_some`) rather than the boolean joining them: any restatement of the
rule must read those two fields, in any order, whether written with `||` or as a branch. Both verified
against the tree at gate time — no matches and one match respectively.

**Readiness gate (cold-reader): FAIL** (round 4)

Broken again, by two more constructed counterexamples: `let r = &input.registered;` before the same
boolean defeats a receiver-anchored pattern, and a dead `#[allow(dead_code)]` function whose body
merely contains the call text satisfies the call-site check while `evaluate` decides ownership itself
through a locally-defined trait method.

Four rounds of this have made the real lesson clear: each fix anchored the observable to *vocabulary*
— an identifier, a receiver, an operator — and vocabulary is exactly what a semantics-preserving
rewrite is free to change. The pattern is now anchored to the two things the rule cannot be written
without, the **field names** `name` and `agent` being tested for presence, and scoped to the whole
binary crate (`rg -n '(name|agent)\.is_some' src/`, exactly one match today) so relocating the
duplicate is not an escape either.

The dead-call-site evasion is answered by composition rather than by a cleverer regex: once no
ownership logic survives anywhere in `src/`, an evaluator wired to a dead call has nothing left to
decide with. The brief now says plainly that the commands are necessary conditions and that a reviewer
confirms reachability from `evaluate` by reading it — rather than pretending a grep establishes it.

**Readiness gate (cold-reader): FAIL** (round 5)

Broken a fourth time, and this round settled the question rather than continuing it. The evasions were
no longer contrived: `.as_ref().is_some()`, `!x.is_none()`, and UFCS `Option::is_some(&x)` are
ordinary Rust that any competent implementer might write **without any intent to evade** — unlike the
decoy comment and operand reorder of earlier rounds. Worse, a same-file `fn pane_is_owned` with the
identical signature, defined and called locally and never importing core's, satisfies the call-site
grep too and reads at a skim exactly like real delegation.

So round 4's composition argument was unsound: it claimed a reviewer had little left to check because
no ownership logic could survive anywhere in `src/`, and that premise was false.

**The conclusion is that this property is not grep-checkable, and the brief now says so.** The
observable space here is an open vocabulary — every spelling of a two-field presence test — which is
exactly the diagnosis round 4 reached about identifiers and operators and then applied one level too
shallow. Five rounds of tightening a regex against an open vocabulary is the wrong move repeated.

The criterion is now honest: the grep stays as a cheap necessary signal explicitly labelled as not a
proof, and the acceptance test is a bounded reviewer check over one ~35-line function, covering both
live-path delegation and the absence of any equivalently-spelled duplicate. The reader's own judgment
on the earlier reviewer concession was that it is legitimate in form — control-flow reachability
genuinely is not grep-checkable, the delegated question is crisp and binary, and the rubric routes
procedure-based criteria to classes 4 and 5 rather than 9 — but that the "hard to fake anyway"
framing propped it up with a claim that did not hold. That framing is gone.

**Readiness gate (cold-reader): PASS** (round 6)

No findings. The judgment the round turned on: the reviewer-check criterion is **not** a class-9
problem in disguise — it is class 9 correctly declining jurisdiction and classes 4 and 5 correctly
picking it up, per the rubric's own carve-out that criteria whose observable is a procedure stay with
those classes. It then clears both rather than merely landing there: nothing it depends on is unbuilt
(it reads code that exists now), and its seam is named explicitly (`evaluate` in `src/cmd/safety.rs`,
measured at 36 lines against the brief's "~35").

The reader also re-derived why no mechanical criterion can close this, rather than accepting the
record's account of five rounds it did not witness: `evaluate`'s `Allow`/`Deny` return is identical
whether ownership delegates or is restated inline, so no behavioral assertion through its public
surface can force delegation to be exercised — and no closed regex can rule out an open vocabulary of
equivalent spellings, nor a same-named local function shadowing the import. A bounded, named manual
check is the right instrument for that residual.

All three grep criteria were executed at baseline and are red as claimed (no matches, no matches, and
exactly `src/cmd/safety.rs:57`), as were both cited existing safety tests and the `--workspace` claim.

**Readiness gate: REOPENED** (round 6)

Reopened by the step-7 adversarial breakdown pass. It read the brief literally and concluded the
requirements are unsatisfiable: a predicate returning one boolean cannot tell an unregistered pane from
a registered pane in the wrong cwd, so keeping both denial messages appears to force either a merged
denial or duplicate ownership logic — and the criteria forbid the latter outright.

The objection is answerable, and the answer was in `## Triage Notes` rather than in the brief, which is
exactly the gap. The predicate's cwd argument is optional and fails open, so calling it twice — first
with no cwd to test registration alone, then with the real one — decomposes the reason without
restating any field test. That reasoning is now hoisted into Key interfaces where an implementer will
actually meet it, along with the constraint that any equivalent decomposition is fine but a
field-presence test in the evaluator is not.

Worth noting the shape of this one: six cold-reader rounds never flagged it, because each was checking
whether the criteria could be *gamed* rather than whether they could be *satisfied at all*. Re-gating.

Non-blocking, recorded: the bool-vs-reason interface tension (the evaluator's two denial messages
are pinned by existing message assertions, so a wrong shape goes red); and `cargo test` in the
final criterion is non-hermetic until the private-server slice lands — this record declares no
`blocked_by` because it is a pure refactor needing no tmux server of its own, but an agent running
the full suite from inside a live session will rearrange it.

Second edit under the same reopened state, from the user's adjudication of the adversary's B3
finding: the out-of-scope line disclaiming unnamed, agentless `launch` panes now names
`ISSUE-260802-0628-01` as what makes them owned, and says how — by registering a name on every pane
this tool creates, leaving the predicate extracted here untouched. Without that pointer the line
reads as a permanent exclusion, which it no longer is, and an implementer could reasonably conclude
those panes need special handling in the predicate. They do not. No acceptance criterion changed.

**Readiness gate (cold-reader): PASS** (round 7)

No blocking findings. The reader re-derived every figure rather than reading it, ignore-blind: the
three grep criteria are red at baseline exactly as claimed (nothing named `pane_is_owned` anywhere in
code; `(name|agent)\.is_some` matching one line, `src/cmd/safety.rs:57`), with `git status
--porcelain --ignored src core` empty, so no ignored tree hides a hit that would flip them. `evaluate`
measures 36 lines against the brief's hedged "~35". Both cited safety tests exist at the lines the
brief relies on.

It also traced the reopening edit through the real evaluator rather than accepting the argument.
Calling the predicate with no cwd reproduces today's `owned` expression exactly, so a false there is
denial A; calling it again with the real cwd can only be false when both cwds are known and differ,
which is denial B. Every existing test was walked against that decomposition — including the two
message-content assertions that pin the literal strings — and all hold. Formatting denial B needs the
recorded cwd string, which is reachable without any presence test, so the decomposition does not
collide with the criterion forbidding one.

One cross-record hazard recorded here rather than in the brief, since it is not this slice's to fix:
criterion 3's pattern is unanchored and would also match an unrelated identifier ending in `name` or
`agent`. `ISSUE-260802-0628-01` rewrites the launch name-rendering path, and if it spells a
name-presence test as `args.name.is_some()` this criterion's "none after" half goes red for a reason
outside this slice. The brief already calls that grep a cheap necessary signal rather than a proof,
and the bounded reviewer check is the actual acceptance test, so the criterion degrades to noise
rather than to a false pass. The constraint belongs on the record that could cause it.
