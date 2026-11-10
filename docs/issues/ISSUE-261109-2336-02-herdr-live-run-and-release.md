---
id: ISSUE-261109-2336-02
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: the Herdr backend passes a live run in the maintainer's Herdr window and ships as a minor release with a README Herdr section
terms: [Host, Host state, Host selection]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p9-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: contracts
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: verification
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p2-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p3-placement
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p5-io
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p6-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p10-tests
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261109-2336-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261110-0019-01
---
## Agent Brief

**Outcome and scope:** The `TMUX_TOOLS_LIVE_HERDR=1` run (PRD-261109-2223-01 P8.4) exists and passes in the maintainer's Herdr window, the maintainer confirms the window result, and the Herdr backend ships as the P9.1 release. This issue owns P8.4, P9.1, P9.2, Verification items 3–4, the P5.1 rule for `how: stitched` and `how: screen` reads (added after ISSUE-261109-2336-01 closed), and the Claude 2.1.322 shim fixture.

**Required sources:** `tmux-tools:PRD-261109-2223-01` clauses `p8-tests` (items 4–5), `p9-rollout`, `contracts` (the `TMUX_TOOLS_LIVE_HERDR` switch), and `verification`; `p2-selection`, `p3-placement`, `p4-registration`, `p5-io`, `p6-state`, `p7-other-verbs` for the README content; `tmux-tools:PRD-261107-0825-01#p10-tests` for the contract suite; `tmux-tools:PRD-261108-0447-01#p10-tests` for the per-version Claude fixture.

**Current observable (after ISSUE-261110-0019-01):** `prompt` to a full-screen pane on Herdr, which hc reads as `how: stitched`, returns the read after settle from its first changed line, not the whole read. `cl` with Claude 2.1.322 prints `claude-teams: claude 2.1.322 not contract-tested`. `TMUX_TOOLS_LIVE_HERDR=1` selects no test run. README names no Herdr host. The installed `tmux-tools` cannot select `herdr`.

**Required change:**
- First, implement the PRD-261109-2223-01 P5.1 rule: when the read after settle reports `how: stitched` or `how: screen`, the `prompt` result is that whole read, and Completeness is false.
- Capture `tests/fixtures/claude-teams/2.1.322.jsonl` per PRD-261108-0447-01 P10.3 (live test from a tmux pane without Team environment, then `tt-fixture`), so the release build contract-tests Claude 2.1.322.
- Add the opt-in live run of P8.4, with the P8.5 teardown and leak assertion.
- Run it in a plain Herdr terminal (no `$TMUX`, `HC_TERMINAL_ID` set) that the agent starts with `hc run` and the maintainer's current `herdr.space` and `herdr.tab` labels. The agent kills and removes that terminal after the run.
- Write the README Herdr section and the `TMUX_TOOLS_HOST` entry (P9.1).
- Bump the minor version, keep a backup of the previous binary, and install.
- Approved contracts: PRD-261109-2223-01 `## Contracts` item "Test switches" (`TMUX_TOOLS_LIVE_HERDR=1`).

**Acceptance:**
- [ ] Given the fake `hc` in the default suite, when the read after settle reports `how: stitched` (and, in a second case, `how: screen`) and its first lines are the same as in the mark read, then the `prompt` result is that whole read and Completeness is false, observable at `prompt --format json` in `cargo test --workspace --features test-fixtures`.
- [ ] Given a full-screen Claude pane in a private core (`TT_TEST_HERDR=1`) that `launch --cmd claude` starts with the user setting `CLAUDE_CODE_NO_FLICKER=1`, so `hc read --json` reports `how: stitched`, when `prompt --format json` sends a prompt that makes Claude print a unique token, then the result contains that token and Completeness is false, observable at `prompt --format json`.
- [ ] Given the release build and Claude 2.1.322, when `tmux-tools claude-teams` starts, then stderr has no `not contract-tested` line, and the default suite replays `2.1.322.jsonl`, observable at stderr and `cargo test`.
- [ ] Given a plain Herdr terminal in the maintainer's space and tab, when `TMUX_TOOLS_LIVE_HERDR=1` runs the contract suite, then it passes the P8.4 cases, observable at the test run output read with `hc read`.
- [ ] Given that run, when the maintainer looks at the Herdr window on the control Mac, then the `--split v` terminal is in the caller's tab, the no-split terminal is a new tab, and the Claude pane shows Herdr's agent state. Owner: maintainer; timing: during the run; evidence: the maintainer's reply recorded in `## Triage Notes`; closure needs it.
- [ ] Given the finished runs, when `tmux-tools list` and `hc ls --all --json` run against the default core, then neither shows a terminal the runs created, observable at their output.
- [ ] Given the release, when `tmux-tools --version` runs, then it shows the bumped minor version, observable at the CLI.
- [ ] Given the released README, when it is read, then its Herdr section covers selection, P3 placement, the `tt.*` and `herdr.*` labels, the P5.2 and P6.6 limits as recorded at release, and `notify`, and `herdr` is listed under `TMUX_TOOLS_HOST`, observable in `README.md`.

**Verification and release:** `TT_TEST_HERDR=1` and `TMUX_TOOLS_LIVE_HERDR=1` both pass on the release revision (P9.2). Rollback: copy the backed-up previous binary over `~/.cargo/bin/tmux-tools`.

**Out of scope:** backend behaviour other than the P5.1 `how: stitched`/`how: screen` result rule (ISSUE-261109-2336-01; the P5.1 first-changed-line mark is ISSUE-261110-0019-01); the Herdr teammate shim; harness rows for `hc`.
