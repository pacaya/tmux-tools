---
id: ISSUE-261102-0142-01
kind: issue
category: bug
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: command_output_with_timeout returns Timeout within 500 ms of its deadline and stops the command's whole process group, even when a descendant keeps stdout or stderr open
terms: []
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references: []
---

## Agent Brief

**Outcome and scope:** `command_output_with_timeout` and its stdin variant `command_output_with_timeout_and_stdin` in `core/src/tmux.rs` (every tmux call goes through them) bound the whole call — waiting for the child and reading its stdout and stderr — by the timeout, and on timeout leave no process of the command running.

**Required sources:** none — standalone issue.

**Current observable:** `tmux::tests::command_output_with_timeout_kills_blocked_process` (a `#!/bin/sh` script running `sleep 5`, timeout 50 ms) intermittently returns `Timeout` only after about 5 s, failing its `elapsed < 1s` check.

**Required change:**
- The spawned command runs in its own process group, on every call.
- When the deadline passes before the child has exited and its stdout and stderr have reached EOF, the call kills the command's whole process group and returns `CommandOutputError::Timeout` no later than 500 ms after the deadline. This also applies when the direct child has already exited but a descendant still holds stdout or stderr open.
- When the child exits and both pipes reach EOF before the deadline, the result (exit status, stdout bytes, stderr bytes) and stdin delivery are unchanged.

**Acceptance:**
- [ ] Given a script that starts `sleep 5` in the background with stdout and stderr inherited, writes its pid to a file, then runs `sleep 5` in the foreground, and the script has written the pid file before the deadline, when `command_output_with_timeout` runs it with a 1 s timeout, then it returns `Timeout` within 1.5 s of the call's start, and after the return neither the script nor the background `sleep` (by pid, `kill -0`) is alive. Fails on every run before this change.
- [ ] Given a script that starts `sleep 5` in the background with stdout and stderr inherited, writes its pid to a file, and exits 0 at once, when `command_output_with_timeout` runs it with a 1 s timeout, then it returns `Timeout` within 1.5 s of the call's start, and the background `sleep` is not alive after the return. Fails before this change.
- [ ] Given `sh -c 'printf out; printf err >&2; exit 3'` and a 5 s timeout, `command_output_with_timeout` returns `Ok` with exit status 3, stdout exactly `out` and stderr exactly `err`.
- [ ] Given `sh -c 'cat; printf done >&2'`, stdin bytes `hello`, and a 5 s timeout, `command_output_with_timeout_and_stdin` returns `Ok` with exit status 0, stdout exactly `hello` and stderr exactly `done`.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server.

**Out of scope:**
- Changing timeout values or which callers use the timeout.
- The auto-tile epic (PRD-260801-0656-01).

## Triage Notes

- 2026-11-02: minted as draft.
- 2026-11-02: mint-check repair applied (maintainer decisions Q18–Q20: 500 ms bound, kill the process group, same deadline when only a descendant holds the pipes).
