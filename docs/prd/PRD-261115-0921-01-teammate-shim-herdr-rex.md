---
id: PRD-261115-0921-01
kind: prd
status: active
scale: epic
stakes: internal
gate: passed 2026-11-15
contracts: approved 2026-11-15
contexts: [tmux-tools]
terms: [Host, Host backend, Host selection, Pane registration, Team, Team record, Leader, Teammate pane, Launcher, Shim, Shim id, Parent, Orphan, Close-on-success wrap, hc terminal, hc name]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: problem
      heading: Problem Statement
    - id: outcome
      heading: Outcome and scope
    - id: p1-slices
      heading: P1 — Slices
    - id: p2-launcher
      heading: P2 — Launcher and host selection on Herdr and Rex
    - id: p3-create
      heading: P3 — Teammate creation and placement
    - id: p4-environment
      heading: P4 — Teammate environment
    - id: p5-herdr-respawn
      heading: P5 — Respawn on Herdr
    - id: p6-herdr-close
      heading: P6 — Close on success on Herdr
    - id: p7-tests
      heading: P7 — Contract tests
    - id: p8-live
      heading: P8 — Live runs
    - id: p9-rollout
      heading: P9 — Rollout and release
    - id: contracts
      heading: Contracts
    - id: verification
      heading: Verification and release
  references:
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
      record: PRD-261108-0447-01
      clause: p2-launcher
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p3-team-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p4-team-record
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p5-shim-surface
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p6-teammate-panes
    - relation: requires
      repository: tmux-tools
      record: PRD-261108-0447-01
      clause: p7-parent
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
      clause: contracts
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
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: contracts
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
      clause: p7-other-verbs
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: p8-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261112-2108-01
      clause: contracts
---

# PRD-261115-0921-01 — Teammate shim on Herdr and Rex

## Problem Statement

On Herdr and Rex, `tmux-tools claude-teams` (the `cl` alias, and every `spawn-agent claude`) cannot set up a Team: it fails open (PRD-261109-2223-01 P7.7, PRD-261112-2108-01 P7.6) and Claude runs its teammates in-process. Teammates get no pane, no name, no Ownership, and no `parent`, and the leak check cannot see them.

## Outcome and scope

`claude-teams` started in a Herdr terminal or a Rex block creates a Team, and every teammate that Claude starts is a Teammate pane on that host with the PRD-261108-0447-01 behavior: named `<leader name>.tm<k>`, listed with `team` and `parent`, closed on a clean Leader exit, reported as an Orphan after a Leader crash. Everything in PRD-261108-0447-01 applies on `herdr` and `rex` unless a clause below says otherwise. This PRD owns the Herdr and Rex values of the Team record, `TT_HOST`, the Leader handle, teammate creation, placement, environment, respawn, close-on-success, their tests, their live runs, and their releases. The tmux and Orca behavior does not change.

## Required behavior

### P1 — Slices

1. The work is two slices with separate landings. The **Herdr slice** covers every clause below for host `herdr` and lands on branch `agent/teammate-shim-herdr`. The **Rex slice** covers every clause below for host `rex` and lands on its own branch, cut from `development` after ISSUE-261114-0707-02 (Rex live run and release) has landed.
2. A slice changes no behavior of the other host. Until the Rex slice lands, PRD-261112-2108-01 P7.6 (fail-open on `rex`) stays in force.

### P2 — Launcher and host selection on Herdr and Rex

1. `TT_HOST` accepts `herdr` (Herdr slice) and `rex` (Rex slice) in addition to `tmux` and `orca` (PRD-261108-0447-01 P3.1). The invalid-value error names every accepted value. The Launcher sets `TT_HOST` to the selected host.
2. The Leader handle (`TeamLeader.handle`) is the value of `HC_TERMINAL_ID` on Herdr and of `REX_BLOCK` on Rex. When the variable is unset or empty, the Launcher fails open per PRD-261108-0447-01 P2.7 with `<reason>` `HC_TERMINAL_ID is unset` or `REX_BLOCK is unset`.
3. `TeamRecord.server_id` is the backend's `server_identity()` value: the Herdr core id, or the Rex `instance_id`. `TeamRecord.tmux` is `None` on both hosts.
4. The slice removes its host's fail-open reason `no teammate shim on herdr` (PRD-261109-2223-01 P7.7) or `no teammate shim on rex` (PRD-261112-2108-01 P7.6), and updates that clause and the matching stderr entry in that PRD's `## Contracts` to point to this PRD. Every other P2.7 fail-open case applies unchanged.
5. `spawn-agent claude` on the host keeps its pane command of PRD-261108-0447-01 P9.1; with the slice released, it gives a Leader with Teammate panes.

### P3 — Teammate creation and placement

1. The Shim creates a Teammate pane through the Host port's registered-create path on every host whose backend creates panes only together with their registration (Herdr), and on every host with Pane records; it uses the create-then-register path only on the remaining hosts. A Herdr teammate create therefore never calls `create_split`.
2. Herdr: the teammate is created with the `herdr.space` and `herdr.tab` labels of its split target (the Leader or a Teammate pane), so it joins the Leader's tab and Herdr chooses its position (PRD-261109-2223-01 P3.1–P3.2). The split direction has no effect.
3. Rex: Rex has no `tile`. The first teammate goes right of the Leader; each later teammate goes below the live Teammate pane with the highest number (the no-`tile` rule of PRD-261108-0447-01 P6.1).
4. Rex: when Rex refuses a split for lack of space, the Shim exits 1 with `tmux: no space for a new pane` and does not use `k` [OPEN: rex-no-space]. When the probe finds such a refusal, the Rex backend must report it as a definite create failure that keeps no name reservation, so that a retry with the same `k` works; this changes PRD-261112-2108-01 P4.4 (which sends unknown create errors to recovery) and needs maintainer approval at the Rex slice triage.
5. The Shim's `capture-pane` chooses its history path by whether the host addresses lines, not by registration storage: on Herdr, as on Orca, `-S -` reads all history and a negative `-S` reads that many history lines plus the visible screen (PRD-261108-0447-01 P5.6), through the backend's `All`, `Visible`, and `Tail` capture ranges.

### P4 — Teammate environment

1. The Shim drops every `split-window` and `respawn-pane` `-e` setting of the host's per-pane variables: `HC_TERMINAL_ID` and `HC_SCOPE` on Herdr; `REX_BLOCK`, `REX_SESSION`, and `REX_SERVER` on Rex. Other `-e` settings pass as PRD-261108-0447-01 P6.1 states.
2. A Teammate pane has its own host values of `HC_TERMINAL_ID` (Herdr) or `REX_BLOCK` (Rex), never the Leader's, and does not have `TT_TEAM`, `TT_HOST`, the fake `TMUX`, or the shim directory first on `PATH` (PRD-261108-0447-01 P6.5).

### P5 — Respawn on Herdr

1. Herdr refuses a second live terminal with the same name, so `respawn-pane -k` on Herdr creates the new terminal under the transitional name `<name>.respawn-<pid>`, closes the old terminal, then writes the name `<name>` on the new terminal, which sets both `tt.name` and `where` (PRD-261109-2223-01 P4.1). When the create fails, the old terminal and its registration stay (PRD-261108-0447-01 P6.2).
2. The new terminal's hc name stays the hc name of the transitional name. Name uniqueness among live Herdr terminals is by the `tt.name` label: when a Herdr create uses a name that a live terminal holds as `tt.name`, the create fails as taken, exits 1, and creates no terminal (the backend's live-name lookup before `hc run`); the Shim then advances `k` (PRD-261108-0447-01 P6.1). The Herdr slice updates PRD-261109-2223-01 P4.2 and its `## Contracts` entry for the hc terminal fields to name this respawn exception and point to this clause.

### P6 — Close on success on Herdr

1. A Herdr Teammate terminal starts with `hc run … --rm`, in addition to the PRD-261109-2223-01 P3.4 flags. With the Close-on-success wrap, its program exits only after exit 0, so Herdr removes the terminal right after it exits 0, and `tmux-tools list` does not show it. After any other exit the wrap keeps a login shell, and the terminal stays.
2. Every other terminal that the Herdr backend creates keeps PRD-261109-2223-01 P3.4: `--rm` is never used. The slice updates P3.4 and the hc-flags entry in that PRD's `## Contracts` to state this one exception and point to this clause.

### P7 — Contract tests

1. Herdr: the default `cargo test --workspace --features test-fixtures` run tests the real binary as `claude-teams` and as `tmux` against the fake `hc` (`tests/contract/fake_hc.py`), with a fake `claude` that replays the trace fixtures of `tests/fixtures/claude-teams/` (PRD-261108-0447-01 P10.2). It covers P2, P3.1–P3.2, P3.5 (`capture-pane -p -S -10` returns history lines plus the visible screen), P4.1, P5 (including `tt.name` and `where` after a respawn, and a create of a respawned teammate's name that is refused as taken), P6, and the PRD-261108-0447-01 P8 output (`list` and `self` fields, Orphan).
2. Rex: the `TT_TEST_REX=1` suite (PRD-261112-2108-01 P8) covers P2, P3.1, P3.3, P4.1, and the PRD-261108-0447-01 P8 output on the private Rex server, with the same fake `claude` and fixtures. When P3.4 is kept, a test with the fake `rex` that answers a split with the error found by the `rex-no-space` probe checks that the Shim exits 1 with `tmux: no space for a new pane`, creates no registration, does not use `k`, and that the next `split-window` succeeds with that `k`.
3. Run every suite with `TT_TEAM` and `TT_HOST` unset.

### P8 — Live runs

Each slice passes one live run on its host before its release. The run starts `spawn-agent claude --name lead`, records the Claude runtime identity, has the Leader start one teammate, and checks:

1. the teammate shows in `tmux-tools list --format json` as `lead.tm1` with `team` and `parent: "lead"`, and in the host's own list (`hc ls --all --json` on Herdr; on Rex the block listing of PRD-261112-2108-01 P4.3: `rex ls --json` and `rex block ls -s <session> --json` for each listed session);
2. the teammate prints its own `HC_TERMINAL_ID` or `REX_BLOCK`, which differs from the Leader's (P4.2);
3. Herdr: the teammate shows in the Leader's tab in the Herdr window (maintainer check);
4. a clean `/exit` of the Leader closes the teammate; on Herdr, the teammate terminal is removed (P6.1);
5. a killed Leader leaves the teammate, which `list` reports as `orphan: true`, and `kill --target <name>` closes it;
6. Rex, when P3.4 is kept: in a small visible Rex window, the Leader starts teammates until a split is refused; the refused start exits 1 with `tmux: no space for a new pane`, and no new block or registration exists;
7. no leaked pane in `tmux-tools list` or in the host listing of item 1 after the run.

The Herdr run needs the maintainer's Herdr window, with the Herdr core started from the GUI session.

### P9 — Rollout and release

1. Each slice ships as its own minor release, after its P7 suite and P8 live run pass. Before the release, the slice merges `origin/development` into its branch and takes the next minor version above the version there, so it never reuses the version of another release (such as ISSUE-261114-0707-02).
2. Each release adds to the README host section: `claude-teams` sets up a Team on that host, where teammates appear (Herdr: in the Leader's tab, placed by Herdr; Rex: right of the Leader, then below the newest teammate), and that on Herdr a teammate terminal is removed when it exits 0.
3. The `cl` alias does not change.

## Codebase fit

- Reuse: the Launcher and Shim (`src/cmd/claude_teams.rs`, `src/shim.rs`), the Team record (`core/src/team.rs`), host selection (`core/src/host/mod.rs`), the Herdr backend (`core/src/host/herdr.rs`: `create_registered`, `placement`, `server_identity`), the Rex backend (`core/src/host/rex.rs`: `server_identity`, `uses_pane_records`), the fake `hc` (`tests/contract/fake_hc.py`), the Rex private-server suite, the trace fixtures (`tests/fixtures/claude-teams/`).
- Sites that change: `claude_teams.rs` Leader match and fail-open arms (`Host::Herdr`, `Host::Rex`); the `TT_HOST` parse and its error text in `core/src/host/mod.rs`; in `src/shim.rs` the create-path branch (`uses_pane_records()` → P3.1), the dropped-variable list (`DROPPED_ENV` → per host, P4.1), the respawn transitional-name branch (P5), the `capture-pane` history branch (`uses_pane_records()` → P3.5), and the `no space` mapping for Rex (P3.4, with the Rex backend's create-error classification in `core/src/host/rex.rs` when a refusal is found); the Herdr `run_terminal` flags for teammate registrations (P6.1); `team.rs` doc comments of `server_id` and `tmux`.
- Conventions: CONTEXT.md terms (Team, Team record, Leader, Teammate pane, Launcher, Shim, Orphan, Close-on-success wrap) unchanged; ADR-260809-1542 (domain-modeling conventions).
- New homes: none; Herdr-slice tests extend `tests/contract/herdr.rs` and `tests/claude_teams.rs`; Rex-slice tests extend the Rex private-server suite.
- Planned-record dependencies: none at PRD level; the Rex-slice issue declares `implementation_after` ISSUE-261114-0707-02 (P1.1).
- Active-plan overlaps: ISSUE-261114-0707-02 (Rex live run and release) touches the README and the version, which the Rex slice also changes; the Rex slice starts after it lands.

## Contracts

### Business entities

- **`TeamRecord`** (changed; `core/src/team.rs`) — `host: Host` adds values `herdr` and `rex`; `server_id: String` is the Herdr core id on Herdr and the Rex `instance_id` on Rex; `tmux: Option<TmuxEndpoint>` is `None` on both. No field is added or removed; `schema` stays `"tmux-tools.team.v1"`.
- **`TeamLeader`** (changed) — `handle: String` is the `HC_TERMINAL_ID` value on Herdr and the `REX_BLOCK` value on Rex.

### External contracts

- **Environment `TT_HOST`** (changed) — accepted values `tmux`, `orca`, plus `herdr` (Herdr slice) and `rex` (Rex slice); the invalid-value error names every value accepted at that time (after the Herdr slice: `tmux`, `orca`, `herdr`). The Launcher's child environment sets `TT_HOST=<selected host>`.
- **`claude-teams` fail-open reasons** (changed) — added: `HC_TERMINAL_ID is unset` (Herdr), `REX_BLOCK is unset` (Rex); removed per slice: `no teammate shim on herdr`, `no teammate shim on rex`. Line format unchanged: `claude-teams: no team: <reason>; starting claude without teammate panes`.
- **Teammate pane environment** (changed) — the Shim drops `-e` settings of `HC_TERMINAL_ID` and `HC_SCOPE` on Herdr and of `REX_BLOCK`, `REX_SESSION`, `REX_SERVER` on Rex; each teammate has its host's own values.
- **Herdr Teammate terminal** (changed) — created by `hc run --bg --outlive-parent --no-notify --rm --name <hc name> --cwd <cwd>` with the `tt.*` registration labels (PRD-261109-2223-01 P4.1, including `tt.team`, `tt.parent`, `tt.shim_id`) and the split target's `herdr.space` and `herdr.tab` labels.
- **Herdr respawn name** (changed) — transitional name `<name>.respawn-<pid>` (`tt.name` and `where`), replaced by `<name>` in both labels after the old terminal closes; the hc name stays that of the transitional name.
- **Rex Teammate block placement** (changed) — first teammate `rex split --split=right -b <leader block>`, later teammates `--split=below -b <highest-numbered live teammate block>`.
- **Shim error on Rex** (changed) — `tmux: no space for a new pane` when Rex refuses a split for lack of space (kept only if the `rex-no-space` probe finds such a refusal).
- Unchanged: the Shim `tmux` subset, the team directory and `shim.log`, `list`/`self` JSON, the trace fixture format, and the `spawn-agent claude` pane command.

## Verification and release

The epic is done when:

1. The Herdr P7.1 tests pass in the default run, and the Herdr P8 live run passes in the maintainer's Herdr window.
2. The Rex P7.2 tests pass with `TT_TEST_REX=1`, and the Rex P8 live run passes in a Rex tab.
3. `cl` in a Herdr terminal and in a Rex block prints no `claude-teams: no team` line and gives teammates as Teammate panes.
4. The tmux and Orca suites pass unchanged.

Rollback is reverting the slice's release: the previous version fails open on that host, and Team directories left on disk are inert.

## Out of Scope

- Harness rule text, guard rows, and setup-guide rows for `herdr` and `rex` (repository `claude`; a separate issue), including the rule that the Herdr core starts only from the GUI session.
- A tmux-tools warning when the Herdr core runs outside the GUI session.
- Split-direction control on Herdr (needs `herdr-app`; out of scope per PRD-261109-2223-01).
- A geometry-based (`tile`) placement on Rex.

## Open Questions

### rex-no-space
- question: which Rex exit status and error text mean "no space for a new block" in a visible window, so P3.4 can map it?
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: the Rex slice triage: a direct `rex split` loop in a new small visible Rex window, before P3.4 is built. An observed refusal gives the exit status and text that P3.4 maps (and the P3.4 change to PRD-261112-2108-01 P4.4 goes to the maintainer). When no refusal is observed, the question stays open and the triage returns the choice for P3.4 and P8.6 to the maintainer.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; ## Verification and release |
| scope boundary | decided | ## Outcome and scope; P1; ## Out of Scope |
| domain terms | decided | CONTEXT.md terms reused; no new term |
| architecture shape | n/a | brownfield — Host port and Shim of PRD-261107-0825-01 and PRD-261108-0447-01 ratified |
| stack | n/a | brownfield — Rust workspace unchanged |
| data/schema | decided | ## Contracts — Business entities (Team record values only) |
| contracts/integrations | deferred | P3.4; open-questions: rex-no-space |
| UX (if UI) | n/a | CLI only; host window placement in P3.2–P3.3 |
| testing/seams | decided | P7; P8 |
