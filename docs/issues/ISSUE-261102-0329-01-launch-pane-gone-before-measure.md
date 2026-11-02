---
id: ISSUE-261102-0329-01
kind: issue
category: bug
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: launch and spawn-agent exit 0 and omit window geometry when their new pane no longer exists at a placement step after the split
terms: [Floor]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0128-01
---

## Agent Brief

**Outcome and scope:** When `launch` or `spawn-agent` created its pane but that pane no longer exists at a later placement step in `place` (`core/src/layout.rs`: registration, retile, `@tt-tiled` marker, measurement), the verb still succeeds and reports the pane without window geometry.

**Required sources:** `tmux-tools:PRD-260801-0656-01` — § Reporting surface (the pane-gone exception to "nothing conditionally omitted").

**Current observable:** `launch --bare --cmd "sleep 0.3" --name <n> --format json` sometimes exits 1 with `cannot measure the window of pane %N … can't find pane: %N`. A pane that is gone before registration fails the verb in the same way.

**Required change:**
- When a tmux call at a placement step after the split fails because the new pane no longer exists, `place` stops the remaining steps and `launch` and `spawn-agent` exit 0.
- stdout: `raw` is unchanged (the pane id and a newline). `concise` and `json` keep every other field they print today and leave out `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor` (JSON: the keys are absent).
- stderr gets one line: `warning: pane <id> no longer exists; window geometry not measured`.
- Any other failure at those steps (the pane still exists, or tmux output cannot be parsed) fails the verb as today.

**Acceptance:** Runtime criteria run on the private harness (`IsolatedTmuxServer`). "Gone-pane wrapper at step S" is a test-only tmux wrapper that kills the new pane and then answers the first tmux call of step S that targets it with tmux's `can't find pane: <id>` error and exit 1.
- [ ] Given the gone-pane wrapper at measurement, when `launch --cmd <shell> --format json` runs, then it exits 0, stdout is JSON with `pane_id` and without `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor`, and stderr contains the warning line. Fails before this change.
- [ ] With the same wrapper, `launch --format concise` exits 0 and prints none of those four keys, and `launch --format raw` prints exactly the pane id and a newline.
- [ ] Given the gone-pane wrapper at registration, when `launch --cmd <shell> --format json` runs, then it exits 0 with the same stdout and stderr result. Fails before this change.
- [ ] Given an owned window and the gone-pane wrapper at retile, when a flagless `launch --cmd <shell> --format json` runs into it, then it exits 0 with the same stdout and stderr result. Fails before this change.
- [ ] Given an owned window and the gone-pane wrapper at the `@tt-tiled` marker step, when a flagless `launch --cmd <shell> --format json` runs into it, then it exits 0 with the same stdout and stderr result. Fails before this change.
- [ ] Given the gone-pane wrapper at measurement, when `spawn-agent <fake agent> --format json` runs with the `AgentsConfig` registry and `tt-fake-tui`, then it exits 0 with the same stdout and stderr result. Fails before this change.
- [ ] Given a pane that still exists and a wrapper that answers the measurement `list-panes` with output that cannot be parsed, when `launch --format json` runs, then it exits non-zero.
- [ ] Given a pane that still exists and a wrapper that makes the measurement `list-panes` exit 1 with an error other than `can't find pane: <id>`, when `launch --format json` runs, then it exits non-zero.
- [ ] Given a pane that still exists and a wrapper that makes the first registration tmux call exit 1 with an error other than `can't find pane: <id>`, when `launch --format json` runs, then it exits non-zero.
- [ ] `launch_bare_pane_closes_when_cmd_exits` passes unmodified.

**Verification and release:** `cargo test --workspace --features test-fixtures --no-fail-fast` passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir>`), and the temp dir is empty afterwards. A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:** A failure of the split itself; `status`; the meaning of `below_floor` when it is reported.

## Triage Notes
