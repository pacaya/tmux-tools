---
id: ISSUE-260801-1310-02
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: Extract the pane-ownership predicate so layout and the destructive verbs share one definition
terms: [Ownership, Pane registration]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
---

## Agent Brief

**Outcome and scope:** Ownership is one public predicate in the core crate, `pane_is_owned`, and the
safety evaluator (`evaluate`, `src/cmd/safety.rs`) delegates to it. Pure refactor: no verb changes
behavior, no new flag, no new persisted field.

**Required sources:** tmux-tools:PRD-260801-0656-01 — § Ownership (the rule), § Module shape
(predicate public in core), § How to run the suite (closing check).

**Current observable:** `evaluate` decides ownership inline; no core-crate predicate exists.

**Required change:**
- `pane_is_owned` is public in the core crate. Inputs: the pane's registration record
  (`names::Registered`) and an optional current cwd. It returns owned when `@tt-name` or `@tt-agent`
  is set and, when both the recorded `@tt-cwd` and the current cwd are known, they match. An absent
  cwd on either side passes. No other registration field (including `@tt-surface`) affects the result.
- `evaluate` keeps its self-pane check, `--force`/`--any` handling, check order, and both denial
  messages ("not created by tmux-tools …" and "owned by another cwd (<recorded cwd>) …"). Its
  ownership answer is the return value of the core predicate. To select the message, call the
  predicate with no current cwd (registration alone), then with the real cwd; any equivalent
  decomposition is allowed, but no code in `src/` may decide whether a pane was created by tmux-tools
  or is owned by testing whether `@tt-name`/`@tt-agent` is set, except by calling the core
  `pane_is_owned`. Code reading those fields for other purposes (printing a name, resolving an agent)
  is unaffected.

**Acceptance:**
- [ ] `rg -n 'pub fn pane_is_owned' core/src` returns the definition.
- [ ] `rg -n 'pane_is_owned\s*\(' src/cmd/safety.rs` returns a call.
- [ ] `rg -n '(name|agent)\.is_some' src/` returns no match that decides ownership from a `Registered`
      value's `name` or `agent`.
- [ ] Reviewer check by reading `evaluate`: its ownership answer is the live return value of the
      **core** `pane_is_owned` (not dead code, not a same-named local function), and no other ownership decision
      from `@tt-name`/`@tt-agent` presence survives anywhere in `src/` under any spelling.
- [ ] Given a registration with `@tt-agent` set, `@tt-name` unset and no recorded cwd, when
      `pane_is_owned` is called with a known current cwd, then it returns owned.
- [ ] Given a registration with `@tt-name` set, `@tt-agent` unset and no recorded cwd, when
      `pane_is_owned` is called with a known current cwd, then it returns owned.
- [ ] Given a registration with `@tt-name` set and a recorded cwd equal to the current cwd, when
      `pane_is_owned` is called, then it returns owned.
- [ ] Given a registration with `@tt-name` set and a recorded cwd, when `pane_is_owned` is called with
      no current cwd, then it returns owned.
- [ ] Given a registration with neither `@tt-name` nor `@tt-agent` and a recorded cwd equal to the
      current cwd, when `pane_is_owned` is called, then it returns not owned.
- [ ] Given a registration with `@tt-name` set and a recorded cwd different from the known current
      cwd, when `pane_is_owned` is called, then it returns not owned.
- [ ] The existing unit tests in `src/cmd/safety.rs` pass unmodified, including both denial-message
      assertions.

**Verification and release:** Core unit tests for `pane_is_owned` cover the six cases above.
`cargo test --workspace --features test-fixtures` passes against an isolated tmux server
(PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change
did not introduce is reported, not fixed. No release action.

**Out of scope:**
- Changing the ownership rule (requiring `@tt-cwd`, failing closed, or reading other fields).
- Making unnamed, agentless `launch` panes owned; ISSUE-260802-0628-01 does that through registration,
  with this predicate unchanged.
- Layout, tiling, or placement behavior.
- Moving the safety evaluator between crates.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
