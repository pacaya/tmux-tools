---
id: ISSUE-261109-2336-01
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: tmux-tools runs every required verb on Herdr through a herdr Host backend over the hc CLI, tested against a fake hc and a private hc core
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Target, Ownership, Parent, Keep-alive wrap, History mark, Completeness, hc terminal, hc name]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p1-definitions
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
      record: PRD-261107-0825-01
      clause: p2-host-port
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p3-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p4-registration
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p5-verb-classes
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p6-host-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p7-notify
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p8-self
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p9-orca
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p2-launcher
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p7-parent
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p1-transport
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p3-state
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p4-capture
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p7-prompt
---
## Agent Brief

**Outcome and scope:** With host `herdr` selected, every required verb of PRD-261107-0825-01 P5.1 works on hc terminals as PRD-261109-2223-01 P1–P7 state, and the default suite plus the `TT_TEST_HERDR=1` private-core suite prove it. This issue owns P1–P7, P8.1–P8.3, P8.5, and P9.3. The live run, README, version bump, and release are ISSUE-261109-2336-02.

**Required sources:** `tmux-tools:PRD-261109-2223-01` clauses `p1-definitions` through `p8-tests`, `p9-rollout` (item 3 only), and `contracts`. These are the rules being implemented. Also the upstream clauses they adopt: `tmux-tools:PRD-261107-0825-01` `p2-host-port`, `p3-selection`, `p4-registration`, `p5-verb-classes`, `p6-host-state`, `p7-notify`, `p8-self`, `p9-orca`, `p10-tests`; `tmux-tools:PRD-261108-0447-01` `p2-launcher`, `p7-parent`; `tmux-tools:PRD-260918-0323-01` `p1-transport`, `p3-state`, `p4-capture`, `p7-prompt`.

**Current observable:** `TMUX_TOOLS_HOST=herdr tmux-tools list` exits 1 and names only `tmux` and `orca` as valid values. A plain Herdr terminal selects `tmux`.

**Required change:**
- First, change the shared code without changing tmux or Orca behaviour:
  - calling-pane lookups go through the Host port (P2.6);
  - creation dispatch no longer depends on Pane records (P3.4);
  - `status --format json` gains `host_word` on every host (P6.3).
- Then add the `herdr` backend and wire it in: selection P2, placement P3, label registration and dead-name removal P4, keys and capture P5, Host state and prompt P6, and the other verbs P7, including the `claude-teams` fail-open of P7.7.
- Add `tests/contract/fake_hc.py` and the `TT_TEST_HERDR=1` suite (P8.2, P8.3, P8.5).
- Update `CONTEXT.md` as P1 lists.
- Approved contracts: PRD-261109-2223-01 `## Contracts`, all items except `TMUX_TOOLS_LIVE_HERDR=1` (ISSUE-261109-2336-02).

**Triage step (before readiness):** run private-core live checks for the PRD's during-triage items `herdr-history-mark`, `herdr-bracketed-paste`, and `herdr-keys-charset`, and put each result to the maintainer. Write each approved result into P5.1, P5.2, P6.6, and P8.4 of PRD-261109-2223-01. An item the maintainer does not approve keeps its PRD default: the P5.1 default, printable ASCII, and the P6.6 refusal. The implementation works to the clauses as recorded at readiness.

**Acceptance:**
- [ ] Given a fake `hc` and `HC_TERMINAL_ID` set with no `$TMUX`, when `tmux-tools self --format json` runs, then it reports `host: "herdr"`, observable at the CLI's stdout.
- [ ] Given a fake `hc` and a calling terminal with `herdr.space=S` and `herdr.tab=T`, when `launch --split v --name A.b` runs, then the fake `hc` receives one `run` with `--outlive-parent --no-notify`, `--name tt-<16 hex of SHA-256("A.b")>`, the labels `herdr.space=S`, `herdr.tab=T`, `where=A.b`, and `tt.name=A.b`, and no `--rm`. With no `--split`, the `run` has `herdr.space=S` and no `herdr.tab`. Observable at the fake `hc` call log.
- [ ] Given a fake `hc`, when `rebalance`, `--split tile`, `--size`, `--session`, or `--window` is used, then the verb exits 3 with the P5.3 message and the fake `hc` receives no `run`, observable at the exit status, stderr, and call log.
- [ ] Given a finished terminal that carries `tt.name=X`, when `launch --bare --name X` runs, then the old terminal is removed by address and the spawn succeeds; when `kill --target X` runs instead, it exits 1 and never acts on another terminal. Observable at the exit status and the fake `hc` call log.
- [ ] Given a fake `hc` whose terminal reports `working` before and after a send with an unchanged `turn`, when `prompt` runs, then `submit` is `accepted`, not `confirmed`, observable at `prompt --format json`.
- [ ] Given a multi-line prompt on Herdr to a pane that P6.6, as recorded at readiness, refuses, when `prompt` runs, then it exits 1 and the fake `hc` receives no `send`, observable at the exit status and call log.
- [ ] Given a Claude pane in a private core, when `prompt --format json` and `capture --all` run, then Completeness, the `capture --all` read, and the no-scrollback notice match P5.1 as recorded at readiness, observable at the CLI output.
- [ ] Given `send` with an argument containing a character outside the typeable set, when it runs, then it exits 1 with `cannot type on herdr: U+<XXXX>` and the fake `hc` receives no `keys`, observable at stderr and the call log.
- [ ] Given a fake `hc` and a binary named `claude`, when `spawn-agent claude` runs on Herdr, then stderr carries `claude-teams: no team: no teammate shim on herdr; starting claude without teammate panes` and no team directory exists, observable at stderr and the state directory.
- [ ] Given the tmux and Orca contract tests, when the default suite runs, then they pass, and their only output change is the added `host_word: null` in `status`, observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given `TT_TEST_HERDR=1` with a real `hc` on `PATH`, when the contract suite runs, then every required verb passes against `HC_SESSION=tt-tests-<pid>`. Afterwards `~/.local/state/herdr-core/` holds no `tt-tests-*` directory, and the default core's `hc ls --all --json` is unchanged. Observable at the test run and those two checks.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. `TT_TEST_HERDR=1 cargo test --workspace --features test-fixtures` passes on the agent Mac with `hc` 0.1.19 or later. No release from this issue.

**Out of scope:** the `TMUX_TOOLS_LIVE_HERDR=1` live run, README, version bump, and install (ISSUE-261109-2336-02); the Herdr teammate shim; harness rows for `hc`; `herdr-app`.
