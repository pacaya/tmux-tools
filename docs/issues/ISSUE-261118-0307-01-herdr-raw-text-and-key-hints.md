---
id: ISSUE-261118-0307-01
kind: issue
category: bug
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: on Herdr, prompt and execute deliver text untagged so a leading /command runs, and a refused control character names its key and the command that sends it
terms: [Host, Host backend, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p6-state
---

## Agent Brief

**Outcome and scope:** On host `herdr`, every text that tmux-tools delivers arrives in the target exactly as given, so a leading `/<command>` runs as a command in Claude or Codex (PRD-261109-2223-01 P6.5). A character that `send` cannot type on Herdr is refused with its key name, the command that sends it as a key, and a pointer to `tmux-tools send --help` (PRD-261109-2223-01 P5.2). `tmux-tools send --help` documents keys and the per-host typeable rule. The Orca and Rex `send` refusals do not change.

**Required sources:** PRD-261109-2223-01 `p5-io` (P5.2 stderr texts) and `p6-state` (P6.5 untagged delivery).

**Current observable:**
- A Claude session in a Herdr terminal, while working, runs `tmux-tools prompt --target <claude pane> 'reply only FIVE'`; the target receives `<hc from="t2w tt-…">reply only FIVE</hc>`. The same path delivers `/exit` as a chat message, and the command does not run. `execute` uses the same `hc send` path.
- `tmux-tools send --target <pane> --literal $'\e'` on Herdr exits 1 with only `cannot type on herdr: U+001B`.

**Required change:**
- Every `hc send` that tmux-tools runs on Herdr (`prompt` and the `execute` text+Enter path) runs without `HC_TERMINAL_ID` in its environment. No other `hc` call changes its environment.
- Herdr `send` refusals per P5.2: a character outside the typeable set that has a key name (U+001B `Escape`, U+000D `Enter`, U+0009 `Tab`, U+007F `BSpace`, otherwise U+0001–U+001A `C-a`–`C-z`) gives the P5.2 key-hint stderr. Every other character keeps `cannot type on herdr: U+<XXXX>`, and the key-name refusal `cannot type on herdr: <argument as written>` does not change.
- `tmux-tools send --help` states: an argument in tmux key grammar (`Escape`, `Enter`, `C-c`, …) is sent as a key; `--literal` text must be typeable on the host; on Herdr the typeable set is the P5.2 set, stated in the README Herdr section; tmux, Orca, and Rex send any text.
- The README Herdr section states the key-hint refusal and the untagged `hc send` delivery.

**Acceptance:**
- [ ] Given the fake `hc` records the environment of each call, when `prompt --target <pane> '/exit'` and `execute --target <pane> '<cmd>'` run with `HC_TERMINAL_ID` set, then each `hc send` call has no `HC_TERMINAL_ID` in its environment, and every other `hc` call of these verbs still has it. Observable in `tests/contract/herdr.rs`.
- [ ] Given Herdr and a pane named `probe`, when `send --target probe --literal` is run with U+001B, U+000D, U+0009, U+007F, U+0001, and U+001A, then each exits 1, sends nothing, and gives the stderr `cannot type on herdr: U+001B (Escape). Send it as a key: tmux-tools escape --target <pane>, or tmux-tools send --target <pane> Escape. See: tmux-tools send --help`, and the matching P5.2 form for `Enter`, `Tab`, `BSpace`, `C-a`, and `C-z`. The stderr contains the literal text `<pane>` (P5.2), not `probe`. U+0021 still gives `cannot type on herdr: U+0021`, and `KP0` still gives `cannot type on herdr: KP0`. Observable in `tests/contract/herdr.rs`.
- [ ] Given Orca and Rex, when `send --target <pane> --literal $'\e'` runs, then it exits 0 and the fake host receives the byte 0x1B. Observable in `tests/contract/orca.rs` and `tests/contract/rex.rs`.
- [ ] Given the change, when `tmux-tools send --help` runs, then it contains the four statements of the help item above. Observable in an integration test.
- [ ] Live, in Herdr, run by the implementer before closure (a failed check, or a check that did not run, keeps the issue open): a Claude session in a Herdr terminal launches a second Claude pane with `tmux-tools launch`, and while it is working it runs `tmux-tools prompt --target <that pane> '/exit'`. The second Claude session ends and prints its resume line; its transcript has no `<hc` text. Afterwards neither `tmux-tools list` nor `hc ls --all --json` shows a terminal the check created. Record the result in the implementation report.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. No separate release; the change ships with the next tmux-tools release.

**Out of scope:** the Orca NUL refusal text (`cannot send C-@: … Orca cannot carry as text`, also printed on Rex); `send --enter` behavior on Herdr (still key by key per P5.2); the Herdr typeable set itself; Herdr `notes` configuration; driver skills in repository `claude`.

## Triage Notes
