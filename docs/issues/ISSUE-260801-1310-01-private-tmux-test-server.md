---
id: ISSUE-260801-1310-01
kind: issue
category: enhancement
status: ready-for-agent
context: tmux-tools
summary: Run pane-creating integration tests against a private, config-isolated tmux server
prd: PRD-260801-0656-01
terms: [Managed session]
---

## Agent Brief

**Category:** enhancement
**Summary:** Move every pane-creating integration test onto a private tmux server with its own socket and no user config, at a fixed window size

**Current behavior:**
The integration suite shells out to the built binary and drives it against the developer's
**default** tmux server, isolating runs only by unique pane names and a serialization guard.
That is safe today because the pane-creating verbs only ever split — a pane-scoped operation
that cannot disturb anything outside the pane it creates.

A targeted test already builds a `TMUX` environment triple by hand — find the precedent with
`rg -n '\.env\(' tests/` — but derives it from the default server. The general binary-spawning
helper sets no environment, so the child process simply inherits the developer's `TMUX` and
`TMUX_PANE`; that inheritance is why both must be overridden rather than only `TMUX`.

**Desired behavior:**
Every test that creates or rearranges panes runs against a tmux server the test owns: an
independent socket, started with no user configuration file, at a fixed 280×82 window size.
Running the suite leaves the developer's own tmux server untouched.

Two things make this a prerequisite rather than a cleanup. The forthcoming tile default turns
these same tests into **window-scoped** operations, so a suite run from inside a working session
would rearrange it, live agents included. And the tmux-behavior tests that follow require
creating sixteen panes and retiling them, which cannot be done safely on a session someone is using.

Config isolation matters independently of socket isolation: tmux still loads configuration on a
private socket, and a user-set tiled-column option would change the grid and make "deterministic
geometry" false. Suppressing the **user** configuration file is the requirement. Whether to also
neutralise a system-level configuration file is delegated to the implementer — starting the server
with no user config is sufficient for this issue; going further is acceptable but not owed, and
either choice must leave the geometry assertions stable.

**Key interfaces:**
- The helper that spawns the built binary — must accept an environment so it can set the target
  server. It currently accepts none.
- Target resolution reads `TMUX_PANE` first and consults `TMUX` only in its absence, so the
  environment must carry **both**: setting `TMUX` alone leaves a suite started from a developer
  pane pointing at that pane's id while aiming commands at the private socket. The existing
  targeted test already removes `TMUX_PANE` for this reason.
- The helper that invokes `tmux` directly for assertions and cleanup — must address the private
  socket rather than plain `tmux`.
- The RAII cleanup guards — must tear down a *server*, not a session.

**Acceptance criteria:**
- [ ] Pane-creating integration tests execute against a tmux server on a socket the suite names
      itself, started with no user configuration loaded. `rg -n 'tt-tests-' tests/` returns the
      socket name; no matches before this change.
- [ ] A test asserts, **while it holds a pane it created**, that the pane is present on the suite's
      private socket and absent from the default server — not merely that the default server's pane
      inventory is restored afterwards. An after-the-fact equality check would pass today, because
      the current suite already creates panes on the default server and cleans them up; it verifies
      teardown, not isolation. Observable at the CLI seam.
- [ ] That server is created at a fixed 280×82, so pane-geometry assertions do not depend on the
      developer's terminal size.
- [ ] The binary-spawning helper passes an environment setting both `TMUX` and `TMUX_PANE`; a test
      asserting the binary places a pane on the private server and not the default one passes. Such
      a test does not exist before this change.
- [ ] The suite tears its server down. Both halves are required **within one suite invocation**: a
      test asserts the private server is reachable by name while the suite holds a pane on it, and
      after the full run a `has-session` against that same socket fails to connect. The mid-run half
      is not decoration — on its own the post-run half is satisfied today by any socket name in this
      scheme, because nothing creates a server under one
      (`tmux -L tt-tests-doesnotexist has-session` already errors with "No such file or directory").
      Only the pair distinguishes "torn down" from "never existed".
- [ ] `cargo test --workspace` passes. `--workspace` is required, not stylistic: a bare `cargo test`
      from the repo root tests only the binary package and skips the `core` package's unit tests
      entirely — see Triage Notes for the command that shows the difference.

**Out of scope:**
- Exposing socket or invocation control as a CLI flag — the environment variable is sufficient and
  the library already has an invocation override for in-process callers.
- Migrating tests that create no panes; they have no blast radius to contain.
- Any layout, tiling, floor, or placement behavior — this issue changes test infrastructure only.
- Removing the suite's serialization guard.

## Triage Notes

- Scale snapshot (non-contractual): `wc -l tests/integration.rs` → ~1k lines (2026-08-01)

- **`cargo test` from the repo root does not run the whole workspace.** The root `Cargo.toml`
  declares `members = [".", "core"]`, and cargo tests only the member it is invoked from — so a bare
  `cargo test` builds and runs the binary package and the integration suite while silently skipping
  the `core` package's unit tests. Compare the `Executable` lines printed by `cargo test --no-run`
  against those printed by `cargo test --workspace --no-run`: only the second builds the core crate's
  `unittests src/lib.rs`. Neither command executes a test, so both are safe to run anywhere. This
  matters beyond bookkeeping — the epic's pure logic (layout, floor resolution, candidate ordering)
  lands in `core`, so a criterion written as bare `cargo test` would not execute the tests those
  slices add.

- **The redirection premise is verified end-to-end, not inferred.** Running the existing, unmodified
  suite with `TMUX` (socket path, server pid, session id) and `TMUX_PANE` pointed at a server started
  as `tmux -L <socket> -f /dev/null new-session -d -x 280 -y 82` put every pane it created on that
  private server. The mechanism this issue is built on therefore needs no new CLI flag and no library
  change — only the helpers threading the environment through. Measured 2026-08-02 with tmux 3.7b.

- **Suite behavior under isolation.** The PRD's Testing Decisions section is the authority on how to
  run the suite and what a surviving failure means; it also names the integration test observed to
  fail intermittently even under isolation. Run instead from a busy shared tmux server, the
  pane-creating tests fail readily and inconsistently (`no such pane: %N` during registration, and a
  bottom-N trim assertion off by one) — this issue's motivating hazard reproducing, not a source
  defect in scope here. The residual flake under isolation is worth an implementer's attention while
  they are already inside these helpers, but this issue does not commit to fixing it.

**Readiness gate (cold-reader): FAIL** (round 1)

Two blocking gaps, both fixed in place before re-gate:

- **Class 9 arm A** — the original first criterion compared the default server's pane-id inventory
  before and after a suite run. Every test in the current suite tears down what it creates, so that
  equality already holds at baseline: the criterion verified cleanup, not isolation, and could not
  distinguish "never touched the default server" from "touched it and cleaned up". It also risked
  false reds from unrelated pane activity on the machine. Replaced with a socket-name observable
  verified red at gate time, plus a during-run presence/absence assertion.
- **Class 7 kind (a)** — "builds a `TMUX` triple by hand in one targeted test" and "the existing
  targeted test" asserted an occurrence count and a uniqueness claim as fact, with no discovery
  command. Reworded qualitatively and paired with `rg -n '\.env\(' tests/`; the count was accurate
  but the authoring rule is unconditional on brief text under edit.

Non-blocking, folded into the same edit: the user-vs-system config-isolation scope is now an
explicit bounded delegation; the final criterion is pinned to the named socket rather than being
vacuously true against a socket that does not yet exist; and "passes no environment at all" is
corrected — the helper sets none, so the child inherits the developer's `TMUX`/`TMUX_PANE`, which
is precisely why both must be overridden.

The out-of-scope carve-out for "tests that create no panes" currently covers the empty set: every
test in the suite creates panes. Harmless, left as written for the case where a non-pane test is
added later.

**Readiness gate (cold-reader): FAIL** (round 2)

One blocking gap, fixed in place before re-gate:

- **Class 9 arm A — the teardown criterion was vacuous read on its own.** "No server is alive on the
  socket named by the first criterion" is already true today for every name in that scheme, because
  nothing creates a server under one; round 1's fix pinned the name but not the vacuity. It was sound
  only as a compound with the socket-name criterion, and per-criterion evaluation is the standard the
  gate applies. Rewritten to require both halves within one suite invocation — present mid-run,
  absent after — which is what distinguishes "torn down" from "never existed".

A second finding, that `cargo test` is red at baseline in two pane-creating tests, was **not** a brief
defect. The reader hedged that its own environment was a busy shared default server with ~20
concurrent sessions, and that hedge was correct — what it observed was this issue's motivating hazard
reproducing under it.

**Readiness gate (cold-reader): FAIL** (round 3)

Two class-7(a) findings, both against my own Triage Notes rather than the brief, and both correct:

- **A wrong count, stated as measurement.** I recorded the isolated baseline as a test count read off
  a truncated tail of the run output, having never seen the core crate's result line. The reader
  counted `#[test]` attributes across the tree and observed that my figure was exactly the binary
  crate's subtotal — inferring, correctly, that the core crate's had been dropped. Investigating it
  surfaced something worth more than the correction: a bare `cargo test` from the repo root **does not
  run the core package at all**, because cargo tests only the member you invoke it from. Since this
  epic's pure logic lands in `core`, every brief's final criterion was written to skip precisely the
  tests those slices add. All of them now say `--workspace`, and the bullet above gives a command that
  shows the difference without executing anything.
- **Figures beside a command that cannot express their qualifier.** "Fully green … when the suite does
  not share a tmux server" sat next to a bare `cargo test`, which expresses no such condition, and the
  redirection claim cited no command at all. Both now carry the literal isolation command sequence
  instead of prose, and the counts are gone in favour of a command that shows the difference.

Correcting these also caught a flaw in my own verification: the earlier "default server untouched"
check was run with `TMUX` still exported, so it queried the private server rather than the default
one. Re-checked properly — the default server holds only long-lived sessions predating this work.

**Readiness gate (cold-reader): FAIL** (round 4)

Four class-7(a) findings, and the irony is worth recording: the paragraph narrating the round-3 fix to
a bare-count violation reintroduced three bare counts of its own — an uncommanded `#[test]` tally
(which was also wrong), the 62-versus-110 pair, and a three-runs-two-green outcome claim. All are now
gone in favour of commands, or delegated to the PRD's Testing Decisions, which is the single place
this epic states how to run the suite.

The fourth finding is sharper and I have adopted it rather than argued it: my discovery command used
`2>&1` and a pipe, and the rubric's screen for discovery commands bars redirects and compound
commands. The reader flagged it as a policy question — is a read-only `cmd | rg` a barred compound
command, or is the screen aimed only at destructive, networked, and eval constructs? — and noted no
worked example in the rubric uses a pipe, which leans strict. Rather than litigate it, the bullet now
uses `cargo test --no-run` versus `cargo test --workspace --no-run` and asks the reader to compare the
`Executable` lines. That is a single command with no redirect, no pipe, and no test execution, so it
is safe to run anywhere and settles the question by not raising it.

The reader also confirmed the load-bearing workspace claim two independent ways before reporting any
of this, and verified the code premises this brief rests on — `TMUX_PANE` read before `TMUX` in
`core/src/target.rs`, `run_bin` setting no environment, `run_tmux` invoking plain `tmux`, and the
existing guard tearing down a session rather than a server.

**Readiness gate (cold-reader): PASS** (round 5)

No findings. All four round-4 defects are confirmed gone from live text, and the replacement
demonstration was checked for form as well as substance — pipe-free, redirect-free, and executing no
tests. Every discovery command and code premise was re-derived independently, down to `tmux -V`
reporting 3.7b and the default server holding only sessions that predate this work.

Two things the reader did that are worth keeping in the record:

- It **declined to execute** `cargo test --workspace`, because doing so would run the pane-creating
  tests, and said so explicitly rather than letting the criterion pass as silently verified. It ran
  `cargo test --workspace --lib` instead — safe, no panes — and reported what that does and does not
  establish. Flagging an unexecuted check is the right call over quietly assuming it.
- It surfaced, without firing, that `terms: [Managed session]` is declared in frontmatter but never
  cited in the prose. Harmless metadata slack, left as is.
