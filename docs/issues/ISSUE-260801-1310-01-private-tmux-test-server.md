---
id: ISSUE-260801-1310-01
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: Run pane-creating integration tests against a private, config-isolated tmux server
terms: [Managed session]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
---

## Agent Brief

**Outcome and scope:** No integration test reaches the developer's tmux server. Every test that talks to tmux does so through the extended `IsolatedTmuxServer` harness (`tests/integration.rs`, behind `test-fixtures`): one private server per test, on its own socket, with no tmux configuration loaded, at a fixed 280×82, torn down when the test ends. Test infrastructure only. No product code changes.

**Required sources:** `tmux-tools:PRD-260801-0656-01`: § Testing Decisions › Sequencing: the isolated-server harness blocks the pane-creating slices (harness shape, `-L`/`-f /dev/null`, both `TMUX` and `TMUX_PANE`, server-level RAII teardown); § How to run the suite (closing check, failures that survive isolation).

**Current observable:**
- `run_bin`, `run_bin_with_timeout`, `run_bin_env`, `run_bin_env_with_timeout`, `run_bin_env_path`, `run_tmux`, `PaneGuard`'s cleanup and the hand-built `TMUX` test (`launch_targets_calling_pane_not_most_recent_client`) run the binary or `tmux` with the inherited environment. The tests that use them create, read and kill panes on the developer's default server. Find them with `rg -n 'Command::new\((BIN|"tmux")\)' tests/integration.rs`.
- `IsolatedTmuxServer` exists only in outside-tmux mode: its `run_cli` removes `TMUX` and `TMUX_PANE`. Its servers are labelled `iso<hex>`, use a temp dir `tt-iso-*`, and start at tmux's detached default size (80×24). Find the harness and the deliberate `TMUX_PANE` removals with `rg -n 'struct IsolatedTmuxServer|env_remove\("TMUX_PANE"\)' tests/`.

**Required change:**
- Extend `IsolatedTmuxServer`. Do not add a second fixture. Each value starts a server with `-L tt-tests-<pid>-<n>` (`<pid>` = test process id, `<n>` = per-process counter) and `-f /dev/null`, with a window of 280×82. One value per test, and its `Drop` kills that server.
- Leak discoverability: the server's `TMUX_TMPDIR` is `<tmp>/tt-tests-<pid>-<n>`, the same name as its socket. `<tmp>` is `/tmp`, or the OS temp dir when `/tmp` is absent. Teardown removes that directory only after the server no longer answers `has-session`. So after a run, every remaining `<tmp>/tt-tests-*` directory gives both the socket name and the `TMUX_TMPDIR` of a server that may have leaked.
- Add an inside-tmux mode beside the existing outside-tmux mode. In inside-tmux mode, the CLI runs with `TMUX` set to the private server's socket triple (socket path, server pid, session id) and `TMUX_PANE` set to a pane the harness owns on that server. Outside-tmux mode keeps removing `TMUX` and `TMUX_PANE` from the CLI (e.g. `launch_created_session_ignores_a_caller_tmux_context` depends on this).
- Scope rule: every `tmux-tools` binary or `tmux` invocation that a test makes against a server goes to that test's private server. CLI calls set both `TMUX` and `TMUX_PANE` to that server (inside-tmux mode), or remove both (outside-tmux mode). One exception: a test whose subject is resolution without `TMUX_PANE` can remove `TMUX_PANE`, but its `TMUX` must name the private server. `tmux -V` availability probes contact no server and are exempt. Migrate the helpers and guards listed above to the harness. Cleanup guards stop a server, not a session.

**Acceptance:**
- [ ] Given a test in inside-tmux mode, when it runs `tmux-tools launch` and holds the created pane, then the pane's `@tt-name` is listed by `list-panes -a` on `tt-tests-<pid>-<n>` (with that server's `TMUX_TMPDIR`), and is not listed on the server that the test process's inherited environment addresses. If that server cannot be reached, the pane is absent. Observable at the CLI seam and with `tmux list-panes`. Fails before this change (no inside-tmux mode, no `tt-tests-*` server).
- [ ] Given `cargo test --workspace --features test-fixtures` run with `TMUX=/nonexistent/tt-decoy,1,0`, `TMUX_PANE=%999999` and `TMUX_TMPDIR` set to an empty directory, when the suite runs, then every test that passes in an ordinary isolated run of the same commit also passes, and the directory is still empty. Fails before this change (the inherited-environment helpers cannot reach a server).
- [ ] Given a test in inside-tmux mode, when it runs `tmux display-message -p '#{config_files}'` with the environment the harness gives the CLI, then the output is `/dev/null`. Fails before this change (that environment addresses the developer's default server, which loaded the developer's configuration).
- [ ] Given a new harness server in either mode, when a test reads `#{window_width}x#{window_height}` of the server's first window, then it is `280x82`. Fails before this change (`80x24`).
- [ ] Given a test that holds a harness server, it observes `<tmp>/tt-tests-<pid>-<n>` exists, `has-session` against that socket and `TMUX_TMPDIR` succeeds, and records the server's `#{pid}`. When the test then drops the harness value, that process no longer exists and the directory is removed. After the full suite run, `find /tmp/ -maxdepth 1 -name 'tt-tests-*'` prints nothing. Fails before this change (no `tt-tests-*` server or directory is ever created).
- [ ] Outside-tmux mode is unchanged: `launch_created_session_ignores_a_caller_tmux_context` and the other existing `IsolatedTmuxServer` tests pass, with the CLI's `TMUX` and `TMUX_PANE` removed.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:**
- A CLI flag or library change for socket or invocation control.
- Removing the suite's serialization guard.
- Isolation from the developer's shell startup files loaded by the keep-alive wrap, and fixing tests that fail under isolation (e.g. `launch_keeps_pane_alive_after_cmd_exit`, `full_smoke`).
- Any layout, tiling, floor or placement behavior.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
