---
id: PRD-261108-0447-01
kind: prd
status: draft
scale: epic
stakes: internal
contracts: pending
contexts: [tmux-tools]
terms: [Host, Host backend, Host selection, Pane record, Pane registration, Pane liveness, Ownership, Target, Keep-alive wrap, Surface, Team, Team record, Leader, Teammate pane, Launcher, Shim, Shim id, Parent, Orphan, Close-on-success wrap]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: problem
      heading: Problem Statement
    - id: outcome
      heading: Outcome and scope
    - id: p1-definitions
      heading: P1 — Definitions
    - id: p2-launcher
      heading: P2 — The claude-teams verb
    - id: p3-team-selection
      heading: P3 — Host selection inside a team
    - id: p4-team-record
      heading: P4 — Team record and team directory
    - id: p5-shim-surface
      heading: P5 — Shim command surface
    - id: p6-teammate-panes
      heading: P6 — Teammate panes
    - id: p7-parent
      heading: P7 — Parent on every create
    - id: p8-output
      heading: P8 — self, list, and kill
    - id: p9-spawn-agent
      heading: P9 — spawn-agent claude through the launcher
    - id: p10-tests
      heading: P10 — Contract tests per Claude version
    - id: p11-rollout
      heading: P11 — Rollout order
    - id: contracts
      heading: Contracts
    - id: verification
      heading: Verification and release
  references:
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-03
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p1-definitions
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
      record: PRD-261107-0825-01
      clause: contracts
---

# Claude teams through a tmux-tools tmux shim

## Problem Statement

Claude Code draws agent-team teammates as panes only through a `tmux` executable. On Orca, the only way to get teammate panes is `orca claude-teams`, whose panes are not tmux-tools panes: they have no name, no Ownership, no parent, and the leak check cannot see them. On tmux, teammate panes that Claude draws through real tmux have the same gaps. A driven Claude leader started by `spawn-agent claude` gets in-process teammates on Orca.

## Outcome and scope

`tmux-tools claude-teams` starts Claude so that every teammate pane is a tmux-tools pane on the host that the caller runs under. A tmux-tools-owned `tmux` shim translates Claude's tmux calls into Host port operations (PRD-261107-0825-01 P2). The first host is Orca; tmux follows in this epic. Rex and Herdr get the shim when their backends exist. Claude is not started with `orca claude-teams`. This PRD owns the launcher, the shim, the team record, the host-selection rule inside a team, the `parent` field on every create, and the `team`, `parent`, `shim_id`, and `orphan` output. This PRD supersedes ISSUE-261107-2103-01.

## Required behavior

### P1 — Definitions

- **Team:** one run of `tmux-tools claude-teams`: one leader and its teammate panes.
- **Team record:** the file that holds the state of one Team (P4).
- **Leader:** the pane in which `claude-teams` runs, and the Claude process that it starts.
- **Teammate pane:** a pane that the Shim creates for a Team.
- **Launcher:** the `claude-teams` verb (P2).
- **Shim:** the `tmux` executable in the team directory that serves Claude's tmux calls (P5).
- **Shim id:** the `%N` pane id that Claude sees for a pane of its Team.
- **Parent:** the name of the tmux-tools pane from which a pane was created (P7).
- **Orphan:** a pane whose Parent is not live, or a Teammate pane whose Team has no running Launcher (P8.3).
- **Close-on-success wrap:** the pane command wrap that closes the pane when the command exits 0 and keeps a shell in the pane on any other exit (P6.3).

Every term in this clause is added to `CONTEXT.md` with its code home. The `CONTEXT.md` entry Pane registration adds the fields of P6.4 and P7.

### P2 — The claude-teams verb

1. `tmux-tools claude-teams [-- <claude args>]` runs in the calling pane. It selects the host by PRD-261107-0825-01 P3 and P3 of this PRD, creates a Team record (P4), and starts the real `claude` as its child process with the environment of item 3. It does not use `exec`, because item 5 needs the Launcher after Claude exits.
2. The real `claude` is the result of `command -v claude` on the caller's `PATH`, before the shim directory is added. It must resolve to a regular executable file; else the verb exits 1 with `claude-teams: claude not found` on stderr and creates no Team record. The Launcher records the canonical path and the `--version` output of that file in the Team record.
3. The child environment is the caller's environment with these changes, and with no other change:
   - `PATH=<team dir>/bin:$PATH`;
   - `TMUX=<team dir>/tmux-fake,<launcher pid>,0`, a path that is not a socket `[ASSUMED: three comma fields like real TMUX; Claude reads only the first]`;
   - `TMUX_PANE=%1`, the Shim id of the Leader;
   - `TT_TEAM=<team id>`;
   - `TT_HOST=<selected host>`.
   The Shim is on `PATH` only in this child environment, never globally.
4. The Launcher adds `--teammate-mode tmux` to the Claude arguments unless the caller passes `--teammate-mode <mode>` or `--teammate-mode=<mode>`. It does not set `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` or `--agent-teams`; the caller's settings enable teams. `[ASSUMED: launcher does not enable the teams feature]`
5. While Claude runs, the Launcher ignores `SIGINT`, `SIGQUIT`, and `SIGTSTP`, and sends `SIGTERM` and `SIGHUP` on to Claude. When Claude exits with status 0, the Launcher closes every live Teammate pane of its Team through the Host port, deletes their Pane registrations, deletes the team directory, and exits 0. On any other exit, the Launcher closes no pane, keeps the team directory, and exits with Claude's status (128 + signal number for a signal). The Launcher never closes the Leader pane.
6. When `TT_TEAM` is already set, the verb exits 1 with `claude-teams: already inside team <id>` on stderr and creates no Team record. `[ASSUMED: nested teams refused]`
7. When the calling pane cannot be identified on the selected host (PRD-261107-0825-01 P9.1 on Orca; `TMUX_PANE` unset on tmux), the verb exits 1 and names the missing variable.

### P3 — Host selection inside a team

This clause adds a first rule to PRD-261107-0825-01 P3 and owns that rule.

1. When `TT_TEAM` is set, `TT_HOST` selects the host, before `TMUX_TOOLS_HOST` and before detection. A `TT_HOST` value that is not a host with a backend fails the invocation with exit 1 and names the valid values. `TT_TEAM` set with `TT_HOST` unset fails with exit 1 and names `TT_HOST`. The selection source is `Team`.
2. When `TT_TEAM` is set and its Team record does not exist, the invocation fails with exit 1 and `team not found: <id>` on stderr.
3. Inside a team, the calling pane is the Leader pane handle from the Team record. `TMUX_PANE` is never read inside a team. This applies to every verb that acts on the calling pane, including `self`, `notify` without `--target`, the self-pane refusal, and placement without `--target`.
4. Inside a team on tmux, the tmux backend runs real tmux by the absolute path and with the server socket (`-S`) that the Team record holds (P4.2). It never resolves `tmux` through `PATH` and never reads `$TMUX`.
5. The fake `TMUX` value never selects the tmux host.

### P4 — Team record and team directory

1. Each Team has one team directory: `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/teams/<team id>/`. A team id is 8 random lowercase hex digits; the Launcher creates the directory with exclusive create and makes a new id on a collision. `[ASSUMED: 8 hex digits]` The directory holds:
   - `team.json` — the Team record (`## Contracts`), written by write-to-temp and atomic rename;
   - `team.lock` — the lock that every change of `team.json` and every Shim pane create or close holds (`flock`, exclusive);
   - `leader.lock` — locked exclusively by the Launcher for its whole life, with close-on-exec; the kernel releases it when the Launcher ends;
   - `bin/tmux` — the Shim (P5.1); `bin/` holds no other file;
   - `shim.log` — the Shim debug log (P5.9).
2. On tmux, the Launcher records in the Team record the absolute path of `command -v tmux` from the caller's `PATH` before the shim directory is added, and the socket path (first comma field) of the caller's `$TMUX`.
3. There is no daemon. The Team record holds the Leader (name, host handle, Shim id `%1`), the next teammate number, the Launcher pid, the host and `server_id`, the cwd, and the Claude binary path and version.
4. A Team is running while `leader.lock` is held. Every `claude-teams` start deletes each team directory whose `leader.lock` is not held. `[ASSUMED: garbage collection at launcher start only]`

### P5 — Shim command surface

1. The Shim is a symbolic link `bin/tmux` to the canonical path of the running `tmux-tools` binary. `tmux-tools` started with `argv[0]` basename `tmux` runs as the Shim. The Shim with `TT_TEAM` unset exits 1 with `tmux: not inside a tmux-tools team` on stderr. `[ASSUMED: multi-call symlink, not a separate bin]`
2. The Shim is a thin front end over the Host port. It never runs tmux itself and builds no host command (PRD-261107-0825-01 P2.2). It acts only on the panes of its own Team: the Leader and the Teammate panes whose registration `team` is `TT_TEAM`. A target outside the Team exits 1 with `tmux: can't find pane: <target>`.
3. Global options before the command: `-S <path>`, `-L <name>`, and `-f <file>` are accepted and ignored. `-V` prints `tmux 3.4` and exits 0. `[ASSUMED: version string tmux 3.4]`
4. Targets (`-t`): a Shim id `%N`; the window `@0`; the session `tt-team-<id>`; and `tt-team-<id>:@0`, `tt-team-<id>:@0.%N`, `@0.%N`. A window or session target means the Leader where a pane is needed. Every Team has one fake session `tt-team-<id>` (`session_id` `$0`) with one fake window `@0` (`window_index` 0, `window_name` `team`) that holds all its panes.
5. Format strings (`-F`, `display-message -p`): the Shim expands `#{pane_id}` (Shim id), `#{window_id}`, `#{window_index}`, `#{window_name}`, `#{session_name}`, `#{session_id}`, `#{pane_title}` (stored title or empty), and the 8 fields of Claude's image-support probe: `client_termtype` empty, `allow-passthrough` `off`, `focus-events` `off`, `pane_top` `0`, `pane_left` `0`, `status` `off`, `status-position` `bottom`, `window_bigger` `0`. Every other `#{…}` expands to the empty string. Text outside `#{…}` is copied.
6. Commands with real work:
   - `display-message -p [-t <target>] <format>` prints the expanded format. Without `-p` it exits 0 with no output.
   - `list-panes [-t <target>] [-F <format>]` prints one line per live pane of the Team, Leader first, then Teammate panes by number. The default format is `#{pane_id}`.
   - `split-window [-d] [-h|-v] [-b] [-f] [-l <size>] [-c <dir>] [-e K=V]... [-P] [-F <format>] [-t <target>] [--] [<command>...]` creates a Teammate pane (P6). With `-P` it prints the expanded format for the new pane (default `#{pane_id}`).
   - `respawn-pane -k [-c <dir>] [-e K=V]... [-t <target>] [--] <command>...` replaces the command of a Teammate pane (P6.2). Without `-k` it exits 1 with `tmux: respawn-pane needs -k`. The Leader is refused with `tmux: can't respawn leader pane`.
   - `kill-pane [-t <target>]` closes a Teammate pane through the Host port and deletes its Pane registration. The Leader is refused with `tmux: can't kill leader pane`.
   - `send-keys [-l] [-t <target>] <key>...` sends through the `send` rules of the host (PRD-261107-0825-01 P9.4 on Orca); `-l` is `--literal`.
   - `capture-pane -p [-J] [-S <start>] [-E <end>] [-t <target>]` prints the pane through `capture`; `-S` with a negative number or `-` adds history. Without `-p` it exits 1 with `tmux: capture-pane needs -p`.
   - `has-session [-t <target>]` exits 0 for its own session or no target; else exit 1 with `tmux: can't find session: <target>`.
   - `select-pane -T <title> [-t <target>]` stores the title as display data (P6.4). `select-pane` without `-T` is accepted and ignored.
7. Commands accepted and ignored with exit 0 and no output: `set-option` (every flag), `select-layout`, `resize-pane`. The size flag `-l` of `split-window` and every size or layout request are dropped and never reach the host, so the Shim never causes exit 3 (PRD-261107-0825-01 P5.3).
8. Errors: `new-session` and `new-window` exit 1 with `tmux: unsupported command: <cmd>`. Every other command name, alias, or `;` separator exits 1 with `tmux: unsupported command: <cmd>`. A flag that item 6 or 7 does not list for that command exits 1 with `tmux: unsupported flag <flag>`. The Shim exits only 0 or 1 and writes errors only to stderr as `tmux: <message>`.
9. Every refusal (item 2, 6, or 8) appends one line to `shim.log`: RFC 3339 time, pid, `refused`, the message, and the argv as one JSON array. With `TT_SHIM_DEBUG=1`, every invocation appends a line with outcome `ok` or `refused`. `[ASSUMED: log format and TT_SHIM_DEBUG]`

### P6 — Teammate panes

1. `split-window` holds `team.lock`, takes the next teammate number `k` (from 1), and creates a pane through the Host port next to the target pane: `-h` is a horizontal split, `-v` (the default) a vertical split. The new pane gets Shim id `%<k+1>` and name `<leader name>.tm<k>`. When the result does not match the name rule of PRD-261107-0825-01 P4.1 or is longer than 100 bytes, or the Leader has no name, the name is `team-<team id>.tm<k>`. When that name is already reserved, `k` advances and the Shim tries again. `[ASSUMED: 100-byte limit]` The reservation and handle rules of PRD-261107-0825-01 P4.4 apply. `-c` and `-e` are applied to the pane command as `cd '<dir>' && env K=V … <command>`, with POSIX shell quoting. `[ASSUMED: -c/-e applied by command wrapping, not by a new port option]`
2. `respawn-pane -k` holds `team.lock`, creates the new pane next to the old pane with the new command, then closes the old pane. The Teammate pane keeps its name and Shim id; its Pane registration gets the new handle by write-to-temp and atomic rename. When the create fails, the old pane stays and the Shim exits 1. `[ASSUMED: respawn = create then close, through existing port operations, on every host]`
3. Every Teammate pane command runs in the Close-on-success wrap: exit 0 closes the pane; any other exit keeps a shell in the pane so the output stays readable (tmux `remain-on-exit failed`). On Orca, the wrap uses `exec` as `launch --bare` does (PRD-261107-0825-01 P9.9). `[OPEN: orca-close-on-clean-exit]`
4. A Teammate pane is a normal Pane registration with these extra fields: `team` (team id), `parent` (Leader name, or `null` when the Leader has no name), `shim_id`, `title` (from `select-pane -T`), `claude_agent_name` and `claude_team_name` (from `--agent-name` and `--team-name` in the `respawn-pane` command). `title`, `claude_agent_name`, and `claude_team_name` are display data and never resolve a Target. `agent` is `null` after `split-window` and `claude` after a `respawn-pane` whose command runs the Claude binary of the Team record. Ownership of a Teammate pane follows PRD-261107-0825-01 P4.7 and the tmux rule; the Shim's own check is the `team` match of item P5.2.
5. Teammate panes get the host's normal pane environment. The Shim does not give them `TT_TEAM`, `TT_HOST`, the fake `TMUX`, or the shim directory on `PATH`.

### P7 — Parent on every create

1. `spawn-agent`, `launch`, and the Shim's `split-window` and `respawn-pane` record `parent` when the calling pane is a tmux-tools pane; `parent` is the name of the calling pane. Else `parent` is `null`.
2. A pane is a tmux-tools pane when `self` reports `driven: true` and a non-null `name` for it (PRD-261107-0825-01 P8; inside a team, P3.3).
3. On tmux, `parent` is the pane option `@tt-parent`. On a registry host, it is the Pane record field `parent`. `team`, `shim_id`, `title`, `claude_agent_name`, and `claude_team_name` are stored the same way (`## Contracts`).

### P8 — self, list, and kill

1. `self --format json` adds `team` (team id or `null`), `role` (`leader`, `teammate`, or `null`), and `parent` (`parent` of the calling pane's registration, or `null`). `role` is `leader` when `TT_TEAM` is set; else `teammate` when the calling pane's registration has a `team`; else `null`.
2. `list --format json` rows add `parent`, `team`, `shim_id` (each `null` when absent) and `orphan` (bool). The text table adds a `PARENT` column after `AGENT`; an Orphan row shows `orphan:<parent>`, or `orphan` when `parent` is `null`.
3. A row is `orphan: true` when one of these holds:
   - its `parent` is not `null` and the parent name does not resolve to a live pane, or resolves to a pane whose `launched_at` is later than the row's `launched_at`;
   - it has a `team` and that Team's `leader.lock` is not held or its team directory does not exist.
   `orphan` is reported only; no verb closes a pane because it is an Orphan.
4. `kill --target <name>` acts on a Teammate pane or an Orphan as on any pane. `kill` of a Leader closes no Teammate pane. `[ASSUMED: kill of a leader does not cascade]`
5. The Leader pane is an ordinary Pane registration when `spawn-agent` created it. `prompt`, `wait-idle`, `capture`, `send`, `interrupt`, and `kill` on the Leader keep their meaning. The Launcher passes the terminal through to Claude and adds no output of its own while Claude runs. Interrupt keys reach Claude, and the Launcher ignores them (P2.5). On Orca, Host state and the P9.6 delivery determination of PRD-261107-0825-01 for a Leader under the Launcher are verified live. `[OPEN: orca-leader-agent-state]`

### P9 — spawn-agent claude through the launcher

1. On Orca, `spawn-agent` for an Agent profile whose binary basename is `claude` runs the pane command `<tmux-tools path> claude-teams -- <claude args>`, where `<tmux-tools path>` is the canonical path of the running binary and `<claude args>` are the arguments that `spawn-agent` builds today. The Pane registration, `agent`, Surface, and Keep-alive behavior stay as today. `[ASSUMED: the binary-basename test selects Claude]`
2. Step 3 of P11 extends item 1 to tmux.
3. `orca claude-teams` is never used. Orca managed-account login is not replicated. Orca shows a plain `claude` leader, not "claude agent teams".

### P10 — Contract tests per Claude version

1. A trace fixture per Claude version, `tests/fixtures/claude-teams/<version>.jsonl`, holds every tmux argv that this Claude version sends in teammate mode, with the expected exit code and output pattern. The first fixture is for Claude 2.1.319.
2. The default `cargo test --workspace --features test-fixtures` run replays each fixture through the real binary started as `tmux` (P5.1), inside a Team on the isolated tmux test server, with no Orca. A fake `claude` script on `PATH` replays the fixture and exits 0 or 1; the tests check P2.5, P6, P7, and P8 through the CLI (`list --format json`, `self --format json`).
3. An opt-in live test (environment switch, never by default) runs the real `claude` under `claude-teams` on the isolated tmux server with `TT_SHIM_DEBUG=1`, makes it spawn one teammate, and fails when `shim.log` has a `refused` line or when the Claude version has no fixture. A new Claude version gets its fixture from this run.
4. The Shim fails closed: an unknown command or flag exits 1 (P5.8); the Shim never guesses.
5. When the Claude version that the Launcher records has no fixture in the binary's build, the Launcher prints `claude-teams: claude <version> not contract-tested` on stderr once and continues. `[ASSUMED: warn, do not refuse]`
6. The Orca contract run of PRD-261107-0825-01 P10.2 adds: a Team with one teammate whose pane shows in `orca terminal list` and in `tmux-tools list` with `team` and `parent`; a clean leader exit that closes the teammate; a leader crash that leaves an Orphan that `list` reports. Each run closes every pane it created and ends with no leaked pane (PRD-261107-0825-01 P10.4).

### P11 — Rollout order

1. The Launcher, the Shim, P3, P4, P6, P7, and P8, with the P10.2 tests on tmux. `claude-teams` works by hand on tmux and Orca.
2. P9.1 on Orca, after the `orca-close-on-clean-exit` and `orca-leader-agent-state` live checks pass and P10.6 passes.
3. P9.1 on tmux, after step 2.
4. Each step bumps the minor version. README documents `claude-teams`, the fake `TMUX` and shim `PATH` in the Leader's child processes (a `tmux` command run by the Leader or its tools reaches the Shim), the new `list` and `self` fields, and the Claude-version warning.

## Codebase fit

- Reuse: the Host port, `HostSelection`, and the tmux and Orca backends (PRD-261107-0825-01 P2, P3, P9; planned in ISSUE-261107-0921-01 and ISSUE-261107-0921-03); `core/src/registry.rs` `PaneRecord` and P4.4 reservation (planned); `core/src/names.rs` name rule and `@tt-*` keys; `src/cmd/safety.rs` and `names::pane_is_owned`; the `send` key translation (P9.4); `tests/integration.rs` isolated tmux harness.
- Conventions: `CONTEXT.md` glossary; ADR-260919-0146-01 (surfaces); PRD-261107-0825-01 P2.2 (no host commands outside a backend).
- New homes: `src/cmd/claude_teams.rs` (Launcher); `src/shim/mod.rs` (argv parser, dispatch) and `src/shim/format.rs` (format expansion); `core/src/team.rs` (`TeamRecord`, team directory, locks, Orphan test); `argv[0]` dispatch in `src/main.rs`; `tests/claude_teams.rs`; `tests/fixtures/claude-teams/2.1.319.jsonl`.
- Planned-record dependencies: `implementation_after` ISSUE-261107-0921-03 (Orca backend, which needs ISSUE-261107-0921-01 and -02); `requires` the PRD-261107-0825-01 clauses in frontmatter.
- Active-plan overlaps: ISSUE-261107-0921-01, -02, -03 (ready-for-agent; the same host selection, registry, `self`, and `list` code). ISSUE-261107-2103-01 (draft) is superseded. Repository `claude`, PRD-261108-0235-01: open questions `teammate-child-signal` and `orca-teammate-panes`; its hooks read `$TMUX_PANE` and run `tmux display-message`, which inside a Leader reach the Shim.

## Contracts

Pending maintainer approval. The proposed batch is in the run reply and is not part of this draft.

### Business entities

Pending.

### External contracts

Pending.

## Verification and release

The epic is done when all of these hold:

1. The P10.2 replay suite passes on tmux in the default test run, and the P10.3 live run on tmux has no `refused` line for Claude 2.1.319.
2. On a live Orca runtime, `spawn-agent claude --name lead` gives a Leader whose teammate shows in `orca terminal list` and in `tmux-tools list --format json` as `lead.tm1` with `team` and `parent: "lead"`; `prompt`, `wait-idle`, and `kill` work on `lead`.
3. A clean `/exit` of the Leader closes every Teammate pane. A killed Leader leaves Teammate panes that `list` reports as `orphan: true`, and `kill --target <name>` closes them.
4. After each run, neither `tmux-tools list` nor the host's own pane list shows a leaked pane.

Rollback is reverting the release. A previous version ignores the new Pane record fields and `@tt-*` options; team directories left on disk are inert.

## Out of Scope

- Rex and Herdr: the Shim reaches them when their backends exist (later PRDs).
- The `claude --tmux` worktree feature (`new-session`, `switch-client`, `attach-session`).
- Orca managed-account login and the `orca claude-teams` environment changes.
- Harness hook and skill changes (repository `claude`).
- Teammate panes drawn in-process.

## Open Questions

### orca-close-on-clean-exit
- question: Does an Orca pane started with the Close-on-success wrap (`exec` form) close when the command exits 0, and keep a shell when it exits non-zero? The P9.9 login shell must not keep every pane open. Acceptance: a live Orca run shows both outcomes in `orca terminal list`.
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

### orca-leader-agent-state
- question: With `claude` running as a child of `tmux-tools claude-teams` in an Orca pane, does `terminal show` report `agentIdentity` `claude` and does `orca worktree ps --json` report the agent row with `hasLivePty: true`? Acceptance: a live Orca run records both; else P8.5 states the Leader's Host state is `null` and its multi-line prompt delivery is not guaranteed.
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; ## Verification and release |
| scope boundary | decided | ## Outcome and scope; ## Out of Scope |
| domain terms | decided | P1 |
| architecture shape | decided | P3; P5.1; P5.2 |
| stack | n/a | brownfield Rust workspace — existing conventions ratified |
| data/schema | decided | P4; P6.4; P7.3; ## Contracts (pending) |
| contracts/integrations | deferred | orca-close-on-clean-exit; orca-leader-agent-state |
| UX (if UI) | n/a | CLI only; output per P8 |
| testing/seams | decided | P10 |
