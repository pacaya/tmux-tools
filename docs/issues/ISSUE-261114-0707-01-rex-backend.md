---
id: ISSUE-261114-0707-01
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: tmux-tools runs every required verb on Rex through a rex Host backend over the rex CLI, tested against a fake rex and a private Rex server
terms: [Host, Host backend, Host selection, Host capabilities, Host state, Pane registration, Pane record, Pane state, Target, Ownership, Parent, Rex block, History mark, Completeness, Keep-alive wrap, Surface, Readiness, Idle detection, Turn observation]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: implementation_after
      repository: tmux-tools
      record: PRD-261109-2223-01
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p1-definitions
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

**Outcome and scope:** With host `rex` selected, every required verb of PRD-261107-0825-01 P5.1 works on Rex blocks as PRD-261112-2108-01 P1–P7 state, and the default suite plus the `TT_TEST_REX=1` private-server suite prove it. This issue owns P1–P7, P8.1–P8.3, P8.5, P8.6, the P9.3 cut point, P9.4, and Verification items 1–2. It does not land the branch. The live run, README, version bump, release, and the P9.3 landing order are ISSUE-261114-0707-02.

**Required sources:** `tmux-tools:PRD-261112-2108-01` clauses `p1-definitions` through `p8-tests`, `p9-rollout` (items 3–4 only), `contracts`, and `verification` (items 1–2). These are the rules being implemented. Also the upstream clauses they adopt: `tmux-tools:PRD-261107-0825-01` `p2-host-port`, `p3-selection`, `p4-registration`, `p5-verb-classes`, `p6-host-state`, `p7-notify`, `p8-self`, `p9-orca`, `p10-tests`; `tmux-tools:PRD-261108-0447-01` `p2-launcher`, `p7-parent`; `tmux-tools:PRD-260918-0323-01` `p1-transport`, `p3-state`, `p4-capture`, `p7-prompt`; `tmux-tools:PRD-261109-2223-01` `p5-io` (the first-changed-line rule that moves to a shared module) and `p6-state`.

**Current observable:** `TMUX_TOOLS_HOST=rex tmux-tools list` exits 1 and does not name `rex` as a valid value. A plain Rex terminal (no `$TMUX`, `REX_BLOCK` set) does not select `rex`.

**Required change:**
- First, move the first-changed-line rule out of `core/src/host/herdr.rs` into one shared module under `core/src/host/` that both backends call (P5.2), with the Herdr tests unchanged except for the P8.2 invalid-value exception.
- Then add the `rex` backend and wire it in: selection P2, placement P3, the Rex Pane record with atomic server-directory publication, the address-check sweep, and spawn recovery P4, capture, keys, and send P5, Host state and `prompt` P6, and the other verbs P7, including the `claude-teams` fail-open of P7.6. The tmux, Orca, and Herdr results do not change (P9.4); the Orca sweep keeps PRD-261107-0825-01 P4.3.
- Add `tests/contract/fake_rex.py`, `tests/contract/rex.rs`, and the `TT_TEST_REX=1` suite (P8.2, P8.3, P8.5, P8.6).
- Update `CONTEXT.md` as P1 lists.
- Work on branch `agent/rex-backend`, cut as P9.3 states.
- Approved contracts: PRD-261112-2108-01 `## Contracts`, all items except the `TMUX_TOOLS_LIVE_REX=1` test switch (ISSUE-261114-0707-02).

**Acceptance:**
- [ ] Given a fake `rex` and `REX_BLOCK` set with no `$TMUX` and no `ORCA_TERMINAL_HANDLE`, when `tmux-tools self --format json` runs, then it reports `host: "rex"`; given `TMUX_TOOLS_HOST=bogus`, the invocation exits 1 and names `tmux`, `orca`, `rex`, and `herdr`. Observable at stdout, stderr, and the exit status.
- [ ] Given a fake `rex`, `REX_SERVER` set, `REX_SESSION` set, and a calling block, when `launch --split v --name N` runs with a 129-character `N`, then the fake `rex` receives one `split --split=below -b <calling block>` with `--focus=false`, `--label` equal to the first 128 characters of `N`, `--cwd`, `--json`, `--autostart=false`, and `-S <REX_SERVER>`, and no `--keep-open`. Observable at the fake `rex` call log.
- [ ] Given a fake block `block:<uuid>` with no registration and no registered name equal to that value, when `capture --target block:<uuid>` runs, then the fake `rex` captures that block; given a registered name `N` whose record holds block B, `capture --target N` makes the fake `rex` capture B. Observable at the CLI output and the fake `rex` call log.
- [ ] Given a fake `rex`, when `rebalance`, `--split tile`, `--size`, `--session`, or `--window` is used, then the verb exits 3 with the PRD-261107-0825-01 P5.3 message and the fake `rex` receives no create. Observable at the exit status, stderr, and call log.
- [ ] Given a create that times out and one block in the target session with the matching label, when the spawn runs, then the record gains that block id; when another reservation in the same server directory has a name with the same first 128 characters, the spawn instead exits 1 with `spawn outcome unknown: <name> (session <session id>)` and creation is not repeated; when the create exits 3 or 4, the reservation is deleted. Observable at the exit status, stderr, the call log, and the registry directory.
- [ ] Given one fake server reached once with `REX_SERVER` unset and once with its explicit address, when both invocations read the registry, then one `rex/<instance_id>/` directory holds both records and neither is deleted; given a second directory whose stored address fails, times out after 2 seconds, or reports a different `instance_id`, or that has no readable `server-address`, then that directory is deleted; given a second live server, its directory survives. Observable at the registry directory.
- [ ] Given a new server directory being published while a sweep from another live server runs, when both finish, then the reservation survives and a second spawn of the same name is refused. Observable at the registry directory and the spawn's exit status.
- [ ] Given a fake block whose scrollback is full (or whose width changes, or whose alternate screen is on) between the History mark and settle, when `prompt --format json` runs, then the result is the last `rows` lines of the capture after settle, with `rows` from the `size` after settle, and Completeness is false. Observable at `prompt --format json`.
- [ ] Given a fake block whose Host state before the send is not `working` and that gets a `program_status` record after the send with state `done` (and, in another case, raw `error`), when `prompt` runs, then `submit` is `confirmed`; given only an older `idle` record, the settle does not end on it and `submit` is `accepted`. Observable at `prompt --format json`.
- [ ] Given `api_revision` 11, when `prompt` runs, then it exits 1 with `rex api revision 11 is older than 12` and the fake `rex` receives no paste or send; given a multi-line prompt to a block without mode 2004 on, it is refused before any transmission. Observable at stderr, the exit status, and the call log.
- [ ] Given host `rex` and `REX_BLOCK` set, when `notify` runs, then it exits 0 with `notify: not delivered on rex` on stderr; when `claude-teams` runs with a fake `claude`, then stderr carries `claude-teams: no team: no teammate shim on rex; starting claude without teammate panes`. Observable at stderr and the exit status.
- [ ] Given the tmux, Orca, and Herdr contract tests, when the default suite runs, then they pass unchanged, except that `an_unreachable_herdr_names_the_host_and_the_selection_source` uses a value that is not a host as its invalid `TMUX_TOOLS_HOST` and checks that stderr names `tmux`, `orca`, `rex`, and `herdr` (P8.2). Observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given `TT_TEST_REX=1` with a real `rex` on `PATH`, when the contract suite runs, then every required verb passes against a private server, a create that times out does not exit 4, and afterwards the private server is stopped, its directory is gone, the default server's `rex ls --json` is unchanged, the operator's `tmux-tools/rex/` registry is unchanged, and no created block remains. Observable at the test run and those checks.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes. `TT_TEST_REX=1 cargo test --workspace --features test-fixtures` passes on the agent Mac. No release from this issue.

**Out of scope:** the `TMUX_TOOLS_LIVE_REX=1` live run, README, version bump, and install (ISSUE-261114-0707-02); the Rex teammate shim; harness `rex` guard and poll rows; Rex event streams, `rex block stream`, `rex resize`, and block identity across a Rex server restart.

## Triage Notes
