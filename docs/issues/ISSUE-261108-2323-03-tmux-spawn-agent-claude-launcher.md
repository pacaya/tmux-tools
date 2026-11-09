---
id: ISSUE-261108-2323-03
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: on tmux, spawn-agent claude starts Claude through tmux-tools claude-teams, so a driven Leader's teammates are tmux-tools panes
terms: [Pane registration, Surface, Keep-alive wrap, Team, Leader, Teammate pane, Launcher, Parent]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p8-output
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p9-spawn-agent
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p11-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: verification
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: claude
      record: PRD-261108-0235-01
      clause: p9-rollout
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261108-2323-02
    - relation: runtime_dependency
      repository: claude
      record: ISSUE-261109-0201-02
    - relation: runtime_dependency
      repository: claude
      record: ISSUE-261109-0201-04
---

## Agent Brief

**Outcome and scope:** Step 3 of `PRD-261108-0447-01` P11: on tmux, `spawn-agent claude --name lead` starts Claude through the Launcher, with the same outcome as on Orca: teammate `lead.tm1` with `team` and `parent: "lead"`, and `prompt`, `wait-idle`, and `kill` work on `lead`.

**Required sources:** `tmux-tools:PRD-261108-0447-01` — P8.4, and P8.5 except its Orca live check (Leader verbs); P9.1, P9.2 (the pane command of P9.1, extended to tmux); P11.3, P11.4 (release conditions, minor bump); `## Contracts` item "Pane command of `spawn-agent claude` on Orca" (the same command on tmux by P9.2); `## Verification and release` items 4, 5 (tmux run). `tmux-tools:PRD-261107-0825-01` — P10.1, P10.4 (tmux contract suite, no leaked pane). `claude:PRD-261108-0235-01` — P9 rollout steps 2 (hooks) and 3 (text and scripts) (release condition).

**Current observable:** On tmux, `spawn-agent claude --name lead` starts the profile's `claude` binary directly in the pane, so a teammate of that session is not a tmux-tools pane.

**Required change:**
- On tmux, for an Agent profile whose binary basename is `claude`, `spawn-agent` runs the pane command of P9.1 inside the normal tmux Keep-alive wrap. Pane registration, `agent`, and Surface stay as today.
- Bump the minor version.
- Approved contracts: `PRD-261108-0447-01` `## Contracts` — Pane command of `spawn-agent claude` on Orca, applied to tmux by P9.2.

**Acceptance:**
- [ ] Given the isolated tmux server and a fake `claude` that starts one teammate through the Shim, when `spawn-agent claude --name lead` runs, then the `lead` pane command is `<canonical tmux-tools path> claude-teams --claude <binary> -- <claude args>` inside the tmux Keep-alive wrap, where `<binary>` is the program and the args are those that `spawn-agent` started before this change, and `list --format json` shows `lead` with `agent: "claude"` and the same Surface that `spawn-agent claude --name lead` gave before this change, and `lead.tm1` with `team` and `parent: "lead"`.
- [ ] On that Leader, `prompt --target lead` and `wait-idle --target lead` give the same result as for the same fake `claude` started without the Launcher; `kill --target lead` closes `lead` and removes its row from `list --format json`. The run closes `lead.tm1` itself and ends with no leaked pane.
- [ ] Given a profile whose binary basename is not `claude`, the `spawn-agent` pane command is unchanged.

**Verification and release:** The default `cargo test --workspace --features test-fixtures` run passes. Release only when the P11.3 conditions hold: step 2 (`ISSUE-261108-2323-02`) is released, and harness `PRD-261108-0235-01` rollout steps 2 (`claude:ISSUE-261109-0201-02`, hooks) and 3 (`claude:ISSUE-261109-0201-04`, text and scripts) are released. Rollback is reverting the release.

**Out of scope:** Orca behavior (`ISSUE-261108-2323-02`); Launcher, Shim, and Team behavior (`ISSUE-261108-2323-01`); harness hook, skill, and `cl` alias changes (repository `claude`).

## Triage Notes

- 2026-11-09: dispatch readiness passed; `needs-triage` → `ready-for-agent`. `implementation_after` `ISSUE-261108-2323-02` is `done` (installed `tmux-tools` 0.6.2). Release conditions (P11.3) hold: harness `claude:ISSUE-261109-0201-02` and `-04` are `done`.
- The existing test `spawn_agent_claude_on_tmux_runs_the_binary_directly` asserts the step-2 tmux behavior; this issue's first acceptance item replaces it.
