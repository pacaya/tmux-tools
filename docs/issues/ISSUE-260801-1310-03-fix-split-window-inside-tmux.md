---
id: ISSUE-260801-1310-03
kind: issue
category: bug
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: new-window placements must land in the caller's, the target's, or the scoped session for every session name, including numeric ones
terms: [Target]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-01
---

## Agent Brief

**Outcome and scope:** `launch --split window` and `spawn-agent --split window` create a new window in the session that contains the calling pane, `--target <pane> --split window` creates it in the target pane's session, and `--session`/`--window` with no `--target` (with `--split window` or no `--split`, both of which create a new window) create it in the scoped session, for every session name tmux allows, including tmux's default numeric names.

**Required sources:** `tmux-tools:PRD-260801-0656-01` — § Flags (scope of the `--split window` fix: calling pane, `--target <pane>`, and `--session`/`--window` without `--target`, every session name); § Settled at triage (this issue owns the `--split window` error text); § How to run the suite (closing check).

**Current observable:**
- From inside tmux (`TMUX` and `TMUX_PANE` set), with no `--target`, `--session` or `--window`, `launch --split window` and `spawn-agent --split window` exit non-zero with tmux's `can't specify pane here`, and create no pane.
- `--target <pane> --split window`, when the target pane's session has a numeric name that is also an index in use (e.g. session `0`), exits 1 with `create window failed: index 0 in use`.
- `--session 0 --split window` with no `--target`, when session `0` already has window `0`, fails the same way.

**Required change:**
- The `-t` given to `new-window` must resolve to the destination session (the calling pane's, the target pane's, or the session scoped by `--session`/`--window`) for every session name tmux allows. Neither a pane id nor a bare `#{session_name}` meets this: tmux reads a bare numeric name as a window index. `#{session_id}`, the form the outside-tmux path already uses, meets it.
- All three cases use the same rule: the calling pane (no explicit target), `--target <pane>`, and the session scoped by `--session`/`--window` without `--target`.
- The registered name, agent metadata (`@tt-agent`, `@tt-surface`, `@tt-access`), and output format are unchanged.
- When the destination session cannot be resolved, the command exits non-zero and creates no pane. The exact wording is delegated to the implementer, bounded only by: it must name the failing operation, and it must not be the tmux passthrough `can't specify pane here`.

**Acceptance:**
- [ ] Given the inside-tmux harness mode (ISSUE-260801-1310-01) and a calling pane in a session with a non-numeric name, when `launch --cmd <shell> --split window` runs, then it exits zero, and the created pane is in a window other than the calling pane's window, and its `#{session_id}` equals the calling pane's. Observable at the CLI seam and by `tmux display -p -t <created pane>` on the private server. Fails before this change.
- [ ] Given a calling pane in a session with a numeric name (tmux's default, e.g. `0`) that already has a window at that index, when `launch --cmd <shell> --split window` runs, then it exits zero and the created pane's `#{session_id}` equals the calling pane's. Fails before this change.
- [ ] Given a target pane in session `0` that already has window `0`, and a caller that is not in that session, when `launch --cmd <shell> --target <that pane> --split window` runs, then it exits zero and the created pane's `#{session_id}` equals the target pane's. Fails before this change with `index 0 in use`.
- [ ] Given a target pane in a session with a non-numeric name, when `launch --cmd <shell> --target <that pane> --split window` runs, then it exits zero and the created pane's `#{session_id}` equals the target pane's.
- [ ] Given the inside-tmux harness mode, an `agents.toml` injected through `XDG_CONFIG_HOME` with the `AgentsConfig` fixture, and an agent whose binary is `tt-fake-tui`, when `spawn-agent <agent> --split window` runs from a calling pane in a session with a numeric name, then it exits zero, the created pane's `#{session_id}` equals the calling pane's, and the pane carries `@tt-agent` for that agent. The test runs under `--features test-fixtures`, needs no installed vendor agent, and fails (does not skip) when the fix is absent.
- [ ] Given session `0` that already has window `0`, when `launch --cmd <shell> --session 0 --split window` runs with no `--target`, then it exits zero and the created pane's `#{session_id}` equals session `0`'s. Fails before this change with `index 0 in use`.
- [ ] Given session `0` that already has window `0`, when `launch --cmd <shell> --session 0` runs with no `--split` and no `--target`, then it exits zero, session `0` gains one window, and the created pane's `#{session_id}` equals session `0`'s. Fails before this change.
- [ ] Given session `0` with windows `0` and `1`, when `launch --cmd <shell> --session 0 --window 1 --split window` runs with no `--target`, then it exits zero and the created pane's `#{session_id}` equals session `0`'s.
- [ ] The outside-tmux path is unchanged: `cargo test --workspace --features test-fixtures` passes.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:**
- The tile default, floors, relocation, `@tt-tiled`, new output fields, or any other layout behavior.
- Changing what `--split window` means, or its interaction with `--size`: a size has no effect on a new-window placement today and continues to have none.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
