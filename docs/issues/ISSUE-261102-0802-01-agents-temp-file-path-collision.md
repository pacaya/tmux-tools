---
id: ISSUE-261102-0802-01
kind: issue
category: bug
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: The core agents tests' temp-file helper gives two parallel tests the same path, so one test deletes the other's file
terms: []
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references: []
---

## Agent Brief

**Outcome and scope:** Every call of the test helper `write_temp_agents_file` (`core/src/agents/mod.rs`,
`#[cfg(test)]`) returns a path that no other call in the same test process returns, so the tests that
use it pass when they run in parallel. Only test code changes.

**Required sources:** none — standalone issue.

**Current observable:** `cargo test -p tmux-tools-core --lib` sometimes fails one of the tests that use
the helper with `Os { code: 2, kind: NotFound }`.

**Required change:**
- The helper's path is unique per call within the process and does not depend on the clock alone (for
  example, add a process-wide atomic counter to the name).
- The helper's paths also stay distinct from paths made by other test processes running at the same
  time in the same `std::env::temp_dir()` (keep the process id in the name).
- The helper keeps its signature and its location under `std::env::temp_dir()`; its callers do not
  change.

**Acceptance:**
- [ ] Given a new unit test in `core/src/agents/mod.rs` that runs 100 rounds in which 8 threads wait on
      a shared `std::sync::Barrier` and then each call `write_temp_agents_file`, when it runs, then in
      every round the 8 returned paths are distinct. On the current helper the test fails, because
      threads released together in the same microsecond get the same path.
- [ ] The new test removes every file it creates.

**Verification and release:** `cargo test --workspace --features test-fixtures --no-fail-fast` passes
with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp
dir>`), and the temp dir is empty afterwards. A failure that survives isolation and that this change did
not introduce is reported, not fixed.

**Out of scope:** Other temp-file helpers; adding a crate dependency; production code.

## Triage Notes
- 2026-11-02: mint passed.
