---
id: ISSUE-261118-0414-02
kind: issue
category: bug
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: the test harness clears every host and team variable a test does not set on purpose from each CLI it starts (default, private-server, and live runs), so the default suite passes unchanged inside tmux, Orca, Herdr, Rex, and team sessions
terms: [Host, Team]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references: []
---

## Agent Brief

**Outcome and scope:** `cargo test --workspace --features test-fixtures` passes unchanged when it runs inside a tmux pane, an Orca terminal, a Herdr terminal, a Rex terminal, or a `claude-teams` Leader session (maintainer decision Q332). For every CLI process that a test starts, the test harness removes each variable in this set that the test does not set on purpose: every `TT_*` variable, `TMUX`, `TMUX_PANE`, every `REX_*` variable, `HC_TERMINAL_ID`, and every `ORCA_*` variable. A test that needs one of these variables sets it on that command. The rule also applies to the opt-in private-server runs (`TT_TEST_HERDR=1`, `TT_TEST_REX=1`) and the opt-in live runs; a live run passes on purpose only the caller variables its PRD clause requires: `ORCA_TERMINAL_HANDLE` and `ORCA_WORKTREE_ID` on Orca (PRD-261107-0825-01 P10.2), `HC_TERMINAL_ID` on Herdr (PRD-261109-2223-01 P8.4), and `REX_BLOCK` and `REX_SESSION` on Rex (PRD-261112-2108-01 P8.4, P3.1).

**Required sources:** none — standalone issue (maintainer decision Q332).

**Current observable:** In a `claude-teams` Leader session (`TT_TEAM` and `TT_HOST` set), Orca fake-host tests in `tests/contract/orca.rs` fail (for example `a_nul_key_is_refused_naming_orca` and `literal_escape_is_sent_as_text_on_orca`).

**Required change:** Each place in the test harness that builds the environment of a spawned CLI removes the variable set above, except the variables that the test sets on purpose. A variable that the suite itself reads as an opt-in switch is not passed to the CLI because of that switch.

**Acceptance:**
- [ ] Given a shell where `TT_TEAM`, `TT_HOST`, `TMUX`, `TMUX_PANE`, `REX_BLOCK`, `REX_SERVER`, `REX_SESSION`, `HC_TERMINAL_ID`, `ORCA_TERMINAL_HANDLE`, and `ORCA_WORKTREE_ID` are set to values that name no reachable host or team, when `cargo test --workspace --features test-fixtures` runs, then it passes with the same tests passing as in a shell where none of them is set. Observable in the `cargo test` result.
- [ ] Given the Orca fake-host harness and a caller environment with `TT_TEAM` and `TT_HOST` set, when a test starts the CLI without setting them, then the CLI process sees neither variable. Observable in an integration test.
- [ ] Given a test that sets one of these variables on its command, when it starts the CLI, then the CLI sees that value. Observable in the existing tests that set `TMUX`, `TMUX_PANE`, `ORCA_TERMINAL_HANDLE`, or `TT_SHIM_REAL`, which pass unchanged.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes in a plain shell and in the shell of the first acceptance item. Test-only change; no release.

**Out of scope:** Rex fake-host tests that fail on a loaded machine because a fake `rex` call takes longer than its timeout.

## Triage Notes
- 2026-11-18: draft → needs-triage (mint check passed) → ready-for-agent (dispatch readiness passed).
