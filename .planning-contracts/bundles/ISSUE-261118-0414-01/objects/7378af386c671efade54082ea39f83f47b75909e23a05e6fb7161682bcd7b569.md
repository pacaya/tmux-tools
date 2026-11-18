---
id: ISSUE-261118-0414-01
kind: issue
category: bug
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: on Orca and Rex, the send refusal of a key that gives the NUL byte names the key as the user wrote it (C-Space, M-C-@), not a rebuilt C-<char>
terms: [Host, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references: []
---

## Agent Brief

**Outcome and scope:** On hosts `orca` and `rex`, when `send` refuses a key argument because it gives the NUL byte, the stderr names the argument exactly as the user wrote it: `cannot send <argument>: it is the NUL byte, which <Host> cannot carry as text`, with `Orca` on Orca and `Rex` on Rex (maintainer decision Q331). This changes the Orca text on purpose. Only the key part of the message changes; which arguments are refused, the exit code, and the "nothing is sent" rule do not change.

**Required sources:** none — standalone issue (maintainer decision Q331; no PRD clause owns this refusal text).

**Current observable:** `tmux-tools send --target <pane> C-Space` on Orca or Rex gives `cannot send C- : it is the NUL byte, …` (a space where the key name must be). `send --target <pane> M-C-@` gives `cannot send C-@: …` (the `M-` modifier is lost).

**Required change:** The NUL refusal shared by the Orca and Rex backends (`core/src/host/orca/keys.rs`) uses the `send` argument as given in place of the rebuilt `C-<char>`.

**Acceptance:**
- [ ] Given Orca and a registered pane, when `send --target <pane> C-Space` runs, then it exits 1, sends nothing, and its stderr contains `cannot send C-Space: it is the NUL byte, which Orca cannot carry as text`. Observable in `tests/contract/orca.rs`.
- [ ] Given Rex and a registered pane, when `send --target <pane> C-Space` runs, then it exits 1, sends nothing, and its stderr contains `cannot send C-Space: it is the NUL byte, which Rex cannot carry as text`. Observable in `tests/contract/rex.rs`.
- [ ] Given Orca and Rex, when `send --target <pane> M-C-@` runs, then it exits 1, sends nothing, and its stderr contains `cannot send M-C-@: it is the NUL byte, which <Host> cannot carry as text` with that host's name. Observable in `tests/contract/orca.rs` and `tests/contract/rex.rs`.
- [ ] Given Orca and Rex, when `send --target <pane> C-@` runs, then the stderr is still `cannot send C-@: it is the NUL byte, which <Host> cannot carry as text`. Observable in `tests/contract/orca.rs` and `tests/contract/rex.rs`.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. Until ISSUE-261118-0414-02 lands, run it with every `TT_*`, `TMUX`, `TMUX_PANE`, `REX_*`, `HC_TERMINAL_ID`, and `ORCA_*` variable removed from the caller environment. No separate release; the change ships with the next tmux-tools release.

**Out of scope:** Which keys give the NUL byte; the Herdr `send` refusals; the key-hint text of other refused characters.

## Triage Notes
- 2026-11-18: draft → needs-triage (mint check passed) → ready-for-agent (dispatch readiness passed).
