---
id: ISSUE-260801-1310-03
kind: issue
category: bug
status: ready-for-agent
context: tmux-tools
summary: --split window fails from inside tmux because a pane id is passed to new-window
prd: PRD-260801-0656-01
terms: [Target]
blocked_by: [ISSUE-260801-1310-01]
---

## Agent Brief

**Category:** bug
**Summary:** `--split window` errors with "can't specify pane here" when invoked from inside tmux

**Current behavior:**
`launch --split window` and `spawn-agent --split window` fail whenever they are run from inside
tmux. The launch path resolves the calling pane from `$TMUX_PANE` and then hands that pane id to
tmux's new-window command as its target. tmux rejects a pane id there — a new window is created in
a *session* or at a *window* index, never "in a pane" — and returns `can't specify pane here`.
The command exits non-zero and no pane is created.

Outside tmux the same flag works, because that path targets the managed session instead.

This matters more than its size suggests: `--split window` is the documented way to opt out of the
default placement, and forthcoming work makes it one of the escape hatches from automatic tiling.
An opt-out that errors is not an opt-out.

**Desired behavior:**
`--split window` creates a new window from inside tmux as it does from outside. The window is
created in the session containing the calling pane, so it lands where the caller is rather than
wherever tmux last had focus. The registered name, agent metadata, and output format are unchanged.

**Key interfaces:**
- The launch dispatcher's no-explicit-target branch, which currently reuses the resolved calling
  pane id for both the split and the new-window cases. Only the split case can take a pane id.
- The existing helper that maps a pane id to its containing session is the natural source for the
  new-window target; the explicit-`--target` path already does this. Note the repo carries two
  target spellings for new-window placement — a bare session name and a name with a trailing colon
  — and neither form is covered by a test today, so verify the chosen one against a real server
  rather than by analogy. Whether tmux resolves the two differently is **unconfirmed**: a probe on a
  private server found no case discriminating them for plain and insert-after-current appends. Treat
  it as an open empirical question, not a known difference to design around.
- Failure path: when the calling pane's session cannot be resolved, the command exits non-zero with
  a diagnostic naming the failure. The exact wording is delegated to the implementer — the PRD
  routes `--split window` error text to triage, and this is the record that owns it — bounded only
  by: it must name the failing operation and must not be the tmux passthrough that produced today's
  `can't specify pane here`, which is the bug being fixed.

**Acceptance criteria:**
- [ ] From inside tmux, `launch --cmd <shell> --split window` exits zero and creates a pane in a
      new window. Observable at the CLI seam against the private test server: an integration test
      asserting a successful `--split window` from a `TMUX`/`TMUX_PANE`-set environment passes.
      No such passing test exists before this change.
- [ ] The new window is created in the session containing the calling pane. Asserted at run time by
      comparing the created pane's `#{session_name}` against the calling pane's — not by grepping
      source, which cannot decide what argv the path emits and would in any case miss the defect,
      since the offending call site names a helper rather than the literal tmux command.
- [ ] `spawn-agent --split window` behaves identically. The assertion must **fail** when the fix is
      absent rather than skip: the existing spawn-agent test is conditional on an agent binary being
      installed, and a test written to that precedent would be green-by-skip before and after this
      change. The implementer may choose the mechanism — a registry agent whose binary need not
      launch successfully, or asserting the placement rather than the agent — provided the test
      cannot pass by being skipped.
- [ ] The outside-tmux path is unchanged and its existing tests still pass.

**Out of scope:**
- The tile default, floors, relocation, or any other layout behavior.
- Changing what `--split window` means, or its interaction with `--size` — a size has no effect on
  a new-window placement today and continues not to.
- The `--session`/`--window` scoping paths, which already target correctly.

## Triage Notes

- Reproduced during PRD measurement work on an isolated server: `tmux-tools spawn-agent codex
  --split window` with `TMUX`/`TMUX_PANE` set failed with a tmux error reporting
  `can't specify pane here` for a `new-window` invocation whose target was a `%`-prefixed pane id.
  (Paraphrased — the tool's error format also carries an exit code; this note is narrative, and the
  brief's delegated error-text bound is the specification.)

**Readiness gate (cold-reader): FAIL** (round 1)

One blocking gap, fixed in place before re-gate:

- **Class 9 arm A** — the final criterion grepped for `new-window` "over the tree". It could never
  go red. The defective call site names a helper (`new_window_args`), not the literal string, so it
  is absent from the criterion's output both before and after the fix; the pattern also matched this
  record and the PRD, and "the tree" resolved to three different result sets depending on whether
  ignored files and build artifacts were included. Its first clause was a runtime claim about emitted
  argv that no source grep can decide, and its second named no computable predicate. Removed; the
  session-equality assertion it was groping toward already lives in the second criterion, now
  strengthened to say it is asserted at run time and why source grepping cannot substitute.

Non-blocking, folded into the same edit: the `--split window` failure path is now an explicit
bounded delegation, which also gives the PRD's "settled at triage" error-text item a home — it had
none, and this is the only `--split window` record. The spawn-agent criterion now forbids
passing-by-skip, because the existing spawn-agent test is conditional on an agent binary being
installed. And the two in-repo new-window target spellings (bare session name versus name with a
trailing colon) are flagged as untested and resolved differently by tmux, so the implementer
verifies rather than reasons by analogy.

**Readiness gate (cold-reader): PASS** (round 2)

No blocking findings. The round-1 fixes hold, and the reader re-derived the bug rather than trusting
the record: it reproduced `can't specify pane here` live from a `TMUX`/`TMUX_PANE` environment against
a private server, confirmed the three `new_window_args` call sites match the brief's description of
them, and confirmed no `--split window` integration test exists to be defeated. It also verified the
delegated error-text bound is executable as stated, and that the forbid-passing-by-skip requirement on
the `spawn-agent` criterion is realizable — `Registry::load()` honours `XDG_CONFIG_HOME`, so a stub
registry agent can be injected without installing a real binary.

One correction applied **before** this stamp, in response to the same report: the brief asserted that
tmux "resolves those differently" for the two new-window target spellings. The reader probed both
forms on a private server and could not find a discriminating case, so the claim is not established.
It is now marked unconfirmed. The edit only withdraws an assertion the reader had already found
non-load-bearing — its verdict rested on the brief instructing empirical verification, which is
unchanged — so the pass stands on the weaker text.
