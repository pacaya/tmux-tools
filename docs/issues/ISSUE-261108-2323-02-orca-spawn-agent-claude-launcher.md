---
id: ISSUE-261108-2323-02
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: on Orca, spawn-agent claude starts Claude through tmux-tools claude-teams, so a driven Leader's teammates are tmux-tools panes, proved by the live Orca contract run
terms: [Host, Host state, Pane registration, Surface, Keep-alive wrap, Team, Leader, Teammate pane, Launcher, Parent, Orphan]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p6-teammate-panes
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
      clause: p10-tests
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
      clause: p6-host-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p9-orca
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
      record: ISSUE-261108-2323-01
    - relation: runtime_dependency
      repository: claude
      record: ISSUE-261109-0201-02
    - relation: runtime_dependency
      repository: claude
      record: ISSUE-261109-0201-04
---

## Agent Brief

**Outcome and scope:** Step 2 of `PRD-261108-0447-01` P11: on Orca, `spawn-agent claude --name lead` starts Claude through the Launcher, so the teammates of the driven Leader are tmux-tools panes `lead.tm<k>`, and `prompt`, `wait-idle`, and `kill` work on `lead`. The live Orca contract run proves this. `spawn-agent claude` on tmux does not change in this issue.

**Required sources:** `tmux-tools:PRD-261108-0447-01` — P9.1, P9.3 (pane command on Orca; no `orca claude-teams`); P6.5 live-run sentence and P8.5 last sentence (live Orca checks); P10.6 (Orca contract run additions); P11.2, P11.4 (release conditions, minor bump); `## Contracts` item "Pane command of `spawn-agent claude` on Orca"; `## Verification and release` items 2, 3 (Orca), 5 (Orca runs). `tmux-tools:PRD-261107-0825-01` — P6 (Host state, waits, submission), P9.6 and P9.10 (prompt delivery, Host state mapping on Orca), P10.2–P10.4 (opt-in live Orca suite, no leaked pane). `claude:PRD-261108-0235-01` — P9 rollout steps 2 (hooks) and 3 (text and scripts) (release condition), P9.6 (verification sessions before that release).

**Current observable:** On Orca, `spawn-agent claude --name lead` starts the profile's `claude` binary directly in the pane; no `lead.tm<k>` row appears in `list --format json`.

**Required change:**
- On Orca, for an Agent profile whose binary basename is `claude`, `spawn-agent` runs the pane command of P9.1. Pane registration, `agent`, Surface, and Keep-alive behavior stay as today.
- Add the P10.6 items to the opt-in live Orca contract run.
- Bump the minor version.
- Approved contracts: `PRD-261108-0447-01` `## Contracts` — Pane command of `spawn-agent claude` on Orca.

**Acceptance:**
- [ ] Given a fake `orca` CLI that records its calls and an Agent profile whose binary basename is `claude`, when `spawn-agent claude --name lead` runs, then the recorded pane-create `--command` is `<canonical tmux-tools path> claude-teams --claude <binary> -- <claude args>`, where `<binary>` is the program and the args are those that `spawn-agent` started before this change, and the fake `orca` records no `orca claude-teams` call; `list --format json` shows `lead` with `agent: "claude"` and the same Surface as before.
- [ ] Given a profile whose binary basename is not `claude`, the recorded pane command is unchanged.
- [ ] On tmux, `spawn-agent claude --name lead` gives the same pane command as before this change.
- [ ] The opt-in live Orca contract run passes with the P10.6 additions: `spawn-agent claude --name lead` gives teammate `lead.tm1` in `orca terminal list --json` with `agentIdentity: "claude"`, and in `tmux-tools list --format json` with `team` and `parent: "lead"`; the Leader has `lead.tm1` print its `ORCA_PANE_KEY`, `ORCA_TERMINAL_HANDLE`, and `ORCA_TAB_ID`, each of the three values equals the value of the `lead.tm1` pane's row in `orca terminal list --json` (`ORCA_TERMINAL_HANDLE` = `handle`, `ORCA_TAB_ID` = `tabId`, `ORCA_PANE_KEY` = `<tabId>:<leafId>`, PRD-261107-0825-01 P9.10), and the `ORCA_PANE_KEY` and `ORCA_TERMINAL_HANDLE` values differ from the `lead` pane's values; in `orca worktree ps --json`, the agent row whose `paneKey` is the teammate pane's key reports the teammate's status, and the Leader's row does not; `prompt`, `wait-idle`, and `kill` work on `lead`; a clean Leader `/exit` closes `lead.tm1`; a killed Leader leaves `lead.tm1` with `orphan: true`, and `kill --target lead.tm1` closes it; at the end neither list shows a pane that the run created.
- [ ] In the same live run, the `host_state` of `lead` in `list --format json` and `wait-idle --format json`, and the result of a multi-line `prompt --target lead` (delivered, or refused before any transmission), equal what `PRD-261108-0447-01` P8.5 states after `orca-leader-agent-state` is resolved at this issue's dispatch triage.

**Verification and release:** The default suite passes. Before review, the implementing agent runs the opt-in live Orca contract run from a Claude session inside an Orca terminal on the agent Mac (repository registered as an Orca worktree; Orca version and `orca status` capabilities named in the handoff). Each check is reported with its commands and result in the review handoff. The issue does not close until the run passes; closing does not wait for the harness rollout. Every live Orca run before harness steps 2 and 3 are released, including the full P10.6 run and the dispatch-triage live check for `orca-leader-agent-state`, starts the Leader's Claude with `disableAllHooks: true` (harness P9.6). Release only when all P11.2 conditions hold: the `orca-close-on-clean-exit` and `orca-leader-agent-state` live checks pass, P10.6 passes, and harness `PRD-261108-0235-01` rollout steps 2 (`claude:ISSUE-261109-0201-02`, hooks) and 3 (`claude:ISSUE-261109-0201-04`, text and scripts) are released. Implementation may start before the harness steps are released. Rollback is reverting the release.

**Out of scope:** `spawn-agent claude` on tmux (P9.2); Launcher, Shim, and Team behavior (`ISSUE-261108-2323-01`); Orca managed-account login; harness hook, skill, and `cl` alias changes (repository `claude`).

## Triage Notes
- Supersedes `ISSUE-261107-2103-01`.
- 2026-11-09: `orca-leader-agent-state` resolved live into `PRD-261108-0447-01` P8.5 (Notes item 9); dispatch readiness passed; `needs-triage` → `ready-for-agent`.
- Release waits on harness `claude:ISSUE-261109-0201-04` (text and scripts) release; implementation does not wait.
