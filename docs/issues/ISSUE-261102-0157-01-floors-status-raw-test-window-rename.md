---
id: ISSUE-261102-0157-01
kind: issue
category: bug
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: The unparsable-agents.toml floors test compares two status --format raw outputs that differ when tmux renames the window between the calls
terms: []
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references: []
---

## Agent Brief

**Outcome and scope:** The integration test `unparsable_agents_toml_warns_and_launch_and_status_still_report_floors` (`tests/integration.rs`) gives the same result on every run. It still fails when `status --format raw` output or stderr depends on whether `agents.toml` can be parsed. Only test code changes.

**Required sources:** none — standalone issue.

**Current observable:** The test sometimes fails at its `raw stdout must not change` assertion: the two back-to-back `status --target <pane> --format raw` calls print different window names (`0.tmux`, then `0.bash`). tmux's `automatic-rename` renamed the window between the calls. The test passes when run alone.

**Required change:**
- Before the two `status --format raw` calls, the test turns off `automatic-rename` on the window that holds the launched pane (`tmux set-window-option -t <pane> automatic-rename off` on the isolated server), so the window name cannot change between the calls.
- The test's assertions stay unchanged: raw stdout and raw stderr with the unparsable `agents.toml` equal those with no `agents.toml`.

**Acceptance:**
- [ ] Given a scenario in which tmux `automatic-rename` changes the launched pane's window name between the two `status --target <pane> --format raw` calls (the implementer chooses how to cause the rename), when `cargo test --features test-fixtures --test integration unparsable_agents_toml_warns_and_launch_and_status_still_report_floors` runs, then the current test fails at `raw stdout must not change` and the changed test passes.
- [ ] The stdout and stderr equality assertions are unchanged.

**Verification and release:** `cargo test --workspace --features test-fixtures --no-fail-fast` passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir>`), and the temp dir is empty afterwards. A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:** Changing `IsolatedTmuxServer` server-wide options; changing `status` output; other tests that compare raw output.

## Triage Notes
