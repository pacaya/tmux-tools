---
id: ISSUE-261107-1229-01
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: a pane spawned under Orca keeps its Pane registration across an Orca runtime restart, through the stable ptyId and incarnationId
terms: [Pane record, Pane liveness, Pane registration, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p4-registration
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-03
---

## Agent Brief

**Outcome and scope:** After an Orca runtime restart, a pane that `tmux-tools` spawned before the restart still resolves by its name, `kill --target <name>` closes it, and its name stays reserved. This replaces the limit stated in `PRD-261107-0825-01` P4.3.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P4 (names and the registry). A PRD or ADR decision that makes `ptyId` with `incarnationId` the record identity and names the registry directory key must exist before this brief leaves `draft`.

**Current observable:** After an Orca runtime restart, the `runtimeId` and every `term_…` handle change, so the Pane records of panes spawned before the restart are deleted on the next invocation and those panes stay open in Orca unregistered.

**Required change:** To be compiled from the decision named in Required sources.

**Acceptance:**
- [ ] Given a pane spawned with `spawn-agent --name <n>` under Orca, after an Orca runtime restart, `capture --target <n>` reads that pane and `kill --target <n>` closes it.
- [ ] After the restart and before `kill`, `spawn-agent --name <n>` exits 1.

**Out of scope:** Rex and Herdr backends.

## Triage Notes
- 2026-11-07 created as `draft` by maintainer decision Q40: not part of the `PRD-261107-0825-01` release.
