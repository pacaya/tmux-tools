---
id: ISSUE-261107-2103-01
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: on Orca, spawn-agent claude starts Claude through orca claude-teams, so a driven Claude lead gets Orca teammate panes
terms: [Pane record, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p9-orca
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-03
---

## Agent Brief

**Outcome and scope:** Under Orca, `spawn-agent claude` starts the driven Claude session with `orca claude-teams` instead of plain `claude`. Teammates that the driven session spawns open as Orca panes, not in-process. `prompt`, `wait-idle`, `capture` and `kill` keep working on the driven pane.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P9 (Orca backend). `PRD-261107-0825-01` lists Claude Code teammate panes as out of scope, so a PRD or ADR decision that brings `orca claude-teams` launch into scope must exist before this brief leaves `draft`.

**Current observable:** Under Orca, `spawn-agent claude` starts plain `claude`. With `teammateMode: "auto"` and no tmux environment, teammates of the driven session run in-process.

**Required change:** To be compiled from the decision named in Required sources. That decision must cover: the environment changes `orca claude-teams` makes (`agentTeams.prepareLaunch` deletes and adds variables and can select a managed account), and Orca's first-prompt delivery through stdin after start (`promptInjectionMode: "stdin-after-start"`) against `tmux-tools prompt`.

**Acceptance:**
- [ ] Given a live Orca runtime, a session started with `spawn-agent claude --name <n>` spawns one named teammate, and `orca terminal list` shows a pane for that teammate.
- [ ] On that session, `prompt --target <n>` submits, `wait-idle --target <n>` reaches done, and `kill --target <n>` closes the pane with no leaked teammate pane.

**Out of scope:** Rex and Herdr backends; a tmux shim launcher for Rex or Herdr; `teammateMode` in harness settings.

## Triage Notes
- 2026-11-07 created as `draft` by maintainer decision Q55b: needs a live test before it becomes the default; not part of the `PRD-261107-0825-01` release.
