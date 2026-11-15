---
id: ISSUE-261114-0816-01
kind: issue
category: bug
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: on tmux, capture --lines 0 gives the visible screen, as it does on Herdr and Rex, instead of an empty result
terms: [Host, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references: []
---

## Agent Brief

**Outcome and scope:** On host `tmux`, `capture --lines 0` gives the visible screen of the target pane. This is the same result that Herdr gives (PRD-261109-2223-01 P5 item 1) and Rex gives (PRD-261112-2108-01 P5 item 1). This issue changes only the tmux result.

**Required sources:** none — standalone issue.

**Current observable:** On host `tmux`, `tmux-tools capture --target <pane> --lines 0 --format json` exits 0 with `"output": ""` and `"lines": 0`. Raw format prints nothing.

**Required change:**
- `capture --lines 0` on tmux gives the same text as `capture` with no `--lines` and no `--all`, in every format.
- The README `capture` row states that `--lines 0` gives the visible screen on tmux and Herdr.

**Acceptance:**
- [ ] Given a tmux pane whose visible screen shows a known marker line, when `capture --target <pane> --lines 0 --format json` runs, then `output` contains the marker line, and `output` and `lines` are equal to the `output` and `lines` of `capture --target <pane> --format json`. Observable at the `capture` JSON in the tmux integration test.
- [ ] Given the change, when the existing tmux integration checks of `capture --lines N` with N > 0 and of `capture --all` run, then they pass with no edits. Observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given the change, when the README `capture` row is read, then it states that `--lines 0` gives the visible screen on tmux and Herdr. Observable in `README.md`.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. No separate release; the change ships with the next tmux-tools release.

**Out of scope:** the Herdr, Rex, and Orca `--lines 0` results, and the Rex README text (ISSUE-261114-0707-02); `--lines N` for N > 0; `--all`; the no-scrollback notice.

## Triage Notes
