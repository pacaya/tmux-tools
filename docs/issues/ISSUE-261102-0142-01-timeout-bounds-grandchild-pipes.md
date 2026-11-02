---
id: ISSUE-261102-0142-01
kind: issue
category: bug
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: command_output_with_timeout returns within its timeout even when a descendant of the killed child keeps stdout or stderr open
terms: []
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references: []
---

## Agent Brief

**Outcome and scope:** `command_output_with_timeout` (and the stdin variant it wraps) in `core/src/tmux.rs` returns `CommandOutputError::Timeout` promptly after its deadline even when the timed-out child has started descendants that still hold its stdout or stderr pipe.

**Required sources:** none — standalone issue.

**Current observable:** `tmux::tests::command_output_with_timeout_kills_blocked_process` (a `#!/bin/sh` script running `sleep 5`, timeout 50 ms) intermittently fails its `elapsed < 1s` check: the call returns `Timeout` only after about 5 s. Only the direct child is killed; the surviving `sleep` keeps the pipe open, and the reader joins wait for it.

**Required change:**
- On timeout, no descendant of the spawned command that holds its stdout or stderr delays the return past a small bound after the deadline.
- The non-timeout path (exit status, captured stdout/stderr, stdin delivery) is unchanged.

**Acceptance:**
- [ ] Given a script that starts a long-lived child process (e.g. `sleep 5`) and a 50 ms timeout, when `command_output_with_timeout` runs it, then it returns `Timeout` in under 1 s on every run; checked by running the existing test at least 20 times in a row with no failure. Fails before this change.
- [ ] Given a command that exits normally, then the returned `Output` matches today's status, stdout and stderr.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server.

**Out of scope:**
- Changing timeout values or which callers use the timeout.
- The auto-tile epic (PRD-260801-0656-01).

## Triage Notes

- 2026-11-02: minted as draft; found during the ISSUE-260801-1310-01 closing check.
