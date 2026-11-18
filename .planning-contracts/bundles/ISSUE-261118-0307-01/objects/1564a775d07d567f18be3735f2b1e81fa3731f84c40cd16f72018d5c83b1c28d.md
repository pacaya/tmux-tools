---
id: ISSUE-261118-0307-01
kind: issue
category: bug
status: in-progress
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-18T03:28:09Z
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

**Outcome and scope:** On host `herdr`, every text that tmux-tools delivers arrives in the target exactly as given, so a leading `/<command>` runs as a command in Claude or Codex (PRD-261109-2223-01 P6.5). A character that `send` cannot type on Herdr is refused with its key name, the command that sends it as a key, and a pointer to `tmux-tools send --help` (PRD-261109-2223-01 P5.2). `tmux-tools send --help` documents keys and the per-host typeable rule. The Orca and Rex `send` refusals do not change, except that the shared NUL-byte refusal names the host that refuses the key (maintainer decision Q324).

**Required sources:** PRD-261109-2223-01 `p5-io` (P5.2 stderr texts) and `p6-state` (P6.5 untagged delivery).

**Current observable:**
- A Claude session in a Herdr terminal, while working, runs `tmux-tools prompt --target <claude pane> 'reply only FIVE'`; the target receives `<hc from="t2w tt-…">reply only FIVE</hc>`. The same path delivers `/exit` as a chat message, and the command does not run. `execute` uses the same `hc send` path.
- `tmux-tools send --target <pane> --literal $'\e'` on Herdr exits 1 with only `cannot type on herdr: U+001B`.
- `tmux-tools send --target <pane> C-@` on Rex exits 1 with `cannot send C-@: it is the NUL byte, which Orca cannot carry as text`.

**Required change:**
- Every `hc send` that tmux-tools runs on Herdr (`prompt` and the `execute` text+Enter path) runs without `HC_TERMINAL_ID` in its environment. No other `hc` call changes its environment.
- Herdr `send` refusals per P5.2: a character outside the typeable set that has a key name (U+001B `Escape`, U+000D `Enter`, U+0009 `Tab`, U+007F `BSpace`, otherwise U+0001–U+001A `C-a`–`C-z`) gives the P5.2 key-hint stderr. Every other character keeps `cannot type on herdr: U+<XXXX>`, and the key-name refusal `cannot type on herdr: <argument as written>` does not change.
- `tmux-tools send --help` states: an argument in tmux key grammar (`Escape`, `Enter`, `C-c`, …) is sent as a key; `--literal` text must be typeable on the host; on Herdr the typeable set is the P5.2 set, stated in the README Herdr section; tmux, Orca, and Rex send any text.
- The README Herdr section states the key-hint refusal and the untagged `hc send` delivery.
- The `send` refusal of a key that gives the NUL byte (`C-@`, `C-Space`, and the other control forms of U+0000) names the host that refuses it: `cannot send <key>: it is the NUL byte, which <Host> cannot carry as text`, with `Orca` on Orca and `Rex` on Rex. The Orca text does not change.

**Acceptance:**
- [ ] Given the fake `hc` records the environment of each call, when `prompt --target <pane> '/exit'` and `execute --target <pane> '<cmd>'` run with `HC_TERMINAL_ID` set, then each `hc send` call has no `HC_TERMINAL_ID` in its environment, and every other `hc` call of these verbs still has it. Observable in `tests/contract/herdr.rs`.
- [ ] Given Herdr and a pane named `probe`, when `send --target probe --literal` is run with U+001B, U+000D, U+0009, U+007F, U+0001, and U+001A, then each exits 1, sends nothing, and gives the stderr `cannot type on herdr: U+001B (Escape). Send it as a key: tmux-tools escape --target <pane>, or tmux-tools send --target <pane> Escape. See: tmux-tools send --help`, and the matching P5.2 form for `Enter`, `Tab`, `BSpace`, `C-a`, and `C-z`. The stderr contains the literal text `<pane>` (P5.2), not `probe`. `send --target probe --literal '!'` still gives `cannot type on herdr: U+0021`, and `send --target probe KP0` (no `--literal`) still gives `cannot type on herdr: KP0`. Observable in `tests/contract/herdr.rs`.
- [ ] Given Orca and Rex, when `send --target <pane> --literal $'\e'` runs, then it exits 0 and the fake host receives the byte 0x1B. Observable in `tests/contract/orca.rs` and `tests/contract/rex.rs`.
- [ ] Given Rex, when `send --target <pane> C-@` runs, then it exits 1, sends nothing, and gives the stderr `cannot send C-@: it is the NUL byte, which Rex cannot carry as text`; `send --target <pane> C-Space` exits 1, sends nothing, and its stderr names `Rex` and not `Orca`. Given Orca, the same two commands exit 1, send nothing, and give the same refusals with `Orca`. Observable in `tests/contract/rex.rs` and `tests/contract/orca.rs`.
- [ ] Given the change, when `tmux-tools send --help` runs, then it contains the four statements of the help item above. Observable in an integration test.
- [ ] Live, in Herdr, run by the implementer before closure (a failed check, or a check that did not run, keeps the issue open): a Claude session in a Herdr terminal launches a second Claude pane with `tmux-tools launch`, and while it is working it runs `tmux-tools prompt --target <that pane> '/exit'`. The second Claude session ends and prints its resume line; its transcript has no `<hc` text. Afterwards neither `tmux-tools list` nor `hc ls --all --json` shows a terminal the check created. Record the result in the implementation report.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. No separate release; the change ships with the next tmux-tools release.

**Out of scope:** `send --enter` behavior on Herdr (still key by key per P5.2); the Herdr typeable set itself; Herdr `notes` configuration; driver skills in repository `claude`.

## Triage Notes
- 2026-11-18: draft → needs-triage (mint check passed) → ready-for-agent (dispatch readiness passed).
- 2026-11-18: in-progress → needs-triage (maintainer decision Q324 adds the host-named NUL refusal; no PRD clause owns that text) → ready-for-agent (mint check and dispatch readiness passed); prior approval `claim-20261118T031858Z` superseded.

## Context Pack — canonical at claim (2026-11-18T03:28:09Z)

**Contract approval:** `redispatch-20261118T032318Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261118-0307-01` · revision `56a417582492ffba671e06367dbf4735dded29d7` · sha256 `45e938d99302ee8c9ddefcd827dd905893c3001db0448f01c70af806df742101`

**Required source objects:**
- `tmux-tools:PRD-261109-2223-01#p5-io` · revision `2e382603c515128133a49b6a4d308eff38ec9275` · sha256 `250ba4cc552405f78a1c4de7091c19dacf45dfdcc6db2057a3ffa1e151669b05` · `objects/250ba4cc552405f78a1c4de7091c19dacf45dfdcc6db2057a3ffa1e151669b05.md`
- `tmux-tools:PRD-261109-2223-01#p6-state` · revision `2e382603c515128133a49b6a4d308eff38ec9275` · sha256 `604326f0dde762ee44f6150b83bbb3c42d38239a03388f681790b5eebab02694` · `objects/604326f0dde762ee44f6150b83bbb3c42d38239a03388f681790b5eebab02694.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261118-0307-01/manifest.json`

**Test seam:** `tests/contract/herdr.rs`, `tests/contract/orca.rs`, `tests/contract/rex.rs` (fake-host contract tests), a `send --help` integration test, and the live Herdr `prompt '/exit'` check
