---
id: ISSUE-261114-0707-02
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: the Rex backend passes a live run in a plain Rex terminal, the maintainer confirms it in the Rex app, and it ships as 0.9.0 with a README Rex section
terms: [Host, Host selection, Host state, Rex block]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261114-0707-01
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p9-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: verification
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p2-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p3-placement
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p6-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
---
## Agent Brief

**Outcome and scope:** The `TMUX_TOOLS_LIVE_REX=1` run (PRD-261112-2108-01 P8.4) exists and passes in a plain Rex terminal on the default server, the maintainer confirms the P9.2 items in the Rex app, and the Rex backend ships as the 0.9.0 release of P9.1. This issue owns P8.4, P9.1, P9.2, and Verification items 3–4.

**Required sources:** `tmux-tools:PRD-261112-2108-01` clauses `p8-tests` (items 4 and 6), `p9-rollout` (items 1–3), `contracts` (the `TMUX_TOOLS_LIVE_REX` switch), and `verification`; `p2-selection`, `p3-placement`, `p4-registration`, `p5-io`, `p6-state`, `p7-other-verbs` for the README content; `tmux-tools:PRD-261107-0825-01#p10-tests` for the contract suite.

**Current observable (after ISSUE-261114-0707-01):** `TMUX_TOOLS_LIVE_REX=1` selects no test run. README names no Rex host. The installed `tmux-tools` cannot select `rex`.

**Required change:**
- Add the opt-in live run of P8.4, with the P8.6 teardown and leak assertion.
- The agent starts the run on the agent Mac from a plain Rex terminal (no `$TMUX`, `REX_BLOCK` set) on the default server, which it opens with `rex run` in the maintainer's current Rex session, with `claude` on `PATH`. After the live run passes, the agent runs the P9.2 visual check from that terminal with a build of the release revision, before install: `spawn-agent claude` with no `--split`, and `launch --split v` beside the calling block. It keeps both blocks open until the maintainer replies, then kills both and runs `tmux-tools list` and the Rex block listing. The agent gives the run output and the post-run listings in its completion report to the maintainer, and closes that terminal after the visual check's kill and post-check listings. `## Triage Notes` keeps one lifecycle stamp that the runs passed and the maintainer confirmed on the release revision.
- Write the README Rex section and the `TMUX_TOOLS_HOST` entry (P9.1).
- The release revision includes the 0.9.0 version change, before both runs and the maintainer's confirmation. Install 0.9.0 only after both runs pass on that revision and the maintainer confirms the P9.2 items.
- Approved contracts: PRD-261112-2108-01 `## Contracts` item "Test switches" (`TMUX_TOOLS_LIVE_REX=1`).

**Acceptance:**
- [ ] Given a plain Rex terminal on the default server, when `TMUX_TOOLS_LIVE_REX=1` runs the contract suite, then it passes the P8.4 cases: a Claude agent pane with Host state, a key send and a literal send, a guaranteed multi-line prompt to the Claude pane, and a `--split v` pane beside the caller. Observable at the test run output in the completion report.
- [ ] Given the P9.2 visual check on the release revision, when the maintainer looks at the Rex app, then the `--split v` block shows beside the caller, a no-split block shows as a new tab, focus does not change, and the Claude pane shows its Rex state. Owner: maintainer; timing: while the agent keeps both check blocks open; evidence: the maintainer's reply recorded in `## Triage Notes`; the release and closure need it.
- [ ] Given the finished `TT_TEST_REX=1` and `TMUX_TOOLS_LIVE_REX=1` runs and the visual check on the release revision, when `tmux-tools list` and the Rex block listing run against the default server, then neither shows a block the runs or the visual check created. Observable at their output in the completion report.
- [ ] Given the release, when `tmux-tools --version` runs, then it shows 0.9.0. Observable at the CLI.
- [ ] Given the released README, when it is read, then its Rex section covers selection, `REX_SERVER` and the `server-address` sweep check, P3 placement and session source, the Pane record, the P5.2 scrollback limit, the P6.4 delivery rule, the P6.5 revision floor, and `notify`, and `rex` is listed under `TMUX_TOOLS_HOST`. Observable in `README.md`.

**Verification and release:** 0.9.0 is installed only after `TT_TEST_REX=1` and `TMUX_TOOLS_LIVE_REX=1` both pass on the release revision and the maintainer confirms the P9.2 items in the visual check. This issue's work lands from `agent/rex-live-release`. Rollback is reverting the release; records left under `tmux-tools/rex/` are inert.

**Out of scope:** backend behaviour (ISSUE-261114-0707-01); the Rex teammate shim; harness `rex` guard and poll rows.

## Triage Notes
