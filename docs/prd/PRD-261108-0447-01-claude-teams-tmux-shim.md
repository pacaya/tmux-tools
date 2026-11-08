---
id: PRD-261108-0447-01
kind: prd
status: draft
scale: epic
stakes: internal
contracts: approved 2026-11-08
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

1. `tmux-tools claude-teams [--claude <program>] [-- <claude args>]` runs in the calling pane. It selects the host by PRD-261107-0825-01 P3 and P3 of this PRD, creates a Team record (P4), and starts the real `claude` as its child process with the environment of item 3. It does not use `exec`, because item 5 needs the Launcher after Claude exits. `--claude` is hidden: `--help` does not show it, and only `spawn-agent` passes it (P9.1).
2. The real `claude` is the result of `command -v <program>` on the caller's `PATH`, before the shim directory is added, where `<program>` is the `--claude` value, else `claude`. It must resolve to a regular executable file; else the verb exits 1 with `claude-teams: claude not found` on stderr and creates no Team record. The Launcher records in the Team record the `command -v` result as it uses it (`ClaudeBinary.resolved`), and the canonical path, the SHA-256, and the `--version` output of that file. It starts Claude by the `command -v` result with no change to `argv[0]`.
3. The child environment is the caller's environment with these changes, and with no other change:
   - `PATH=<team dir>/bin:$PATH`;
   - `TMUX=<team dir>/tmux-fake,<launcher pid>,0`, a path that is not a socket;
   - `TMUX_PANE=%1`, the Shim id of the Leader;
   - `TT_TEAM=<team id>`;
   - `TT_HOST=<selected host>`.
   The Shim is on `PATH` only in this child environment, never globally.
4. The Launcher adds `--teammate-mode tmux` to the Claude arguments unless the caller passes `--teammate-mode <mode>` or `--teammate-mode=<mode>`. It does not set `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` or `--agent-teams`; the caller's settings enable teams.
5. While Claude runs, the Launcher ignores `SIGINT`, `SIGQUIT`, and `SIGTSTP`, and sends `SIGTERM` and `SIGHUP` on to Claude. The Launcher waits with stop reporting: when Claude stops (its own Ctrl-Z handling, or `SIGSTOP`), the Launcher stops itself with `SIGSTOP`, so the caller's shell gets the terminal; when the Launcher gets `SIGCONT`, it sends `SIGCONT` to Claude. When Claude exits with status 0, the Launcher closes every live Teammate pane of its Team through the Host port, deletes their Pane registrations, deletes the team directory, and exits 0. On any other exit, the Launcher closes no pane, keeps the team directory, and exits with Claude's status (128 + signal number for a signal). The Launcher never closes the Leader pane.
6. When `TT_TEAM` is already set, the verb exits 1 with `claude-teams: already inside team <id>` on stderr and creates no Team record.
7. The Launcher fails open. When it cannot set up a Team, it prints `claude-teams: no team: <reason>; starting claude without teammate panes` on stderr, leaves no team directory, and starts the real `claude` of item 2 as its child process with the caller's environment unchanged. It adds `--teammate-mode in-process` unless the caller passes `--teammate-mode`, and it keeps the signal and stop rules of item 5. It exits with Claude's status. A Team cannot be set up when host selection fails (an invalid `TMUX_TOOLS_HOST`, PRD-261107-0825-01 P3.1; an unreachable host CLI or runtime, P3.4), when the calling pane cannot be identified on the selected host (PRD-261107-0825-01 P9.1 on Orca; `TMUX_PANE` unset on tmux, which includes the P3.3 default in a plain, Rex, or Herdr terminal; `<reason>` names the missing variable), when the team directory, a lock, or `team.json` cannot be created, when, with `TT_SHIM_DEBUG=1`, the debug directory or its copy of `team.json` cannot be created (P4.5), or when a host call fails during setup. For `claude-teams` only, these cases replace the exit 1 of PRD-261107-0825-01 P3.1, P3.4, and P9.1 (a missing `ORCA_TERMINAL_HANDLE`), and the Launcher never starts the Managed session of P3.3. Items 2 and 6 are not fail-open: they exit 1.
8. The Launcher never registers the calling pane. The Leader name is the name of the Leader pane's Pane registration, which the Shim reads by the Leader handle at each `split-window` (P6.1); the Team record holds no Leader name. When the Leader pane is not a tmux-tools pane (P7.2) at that time, the Leader has no name: the new Teammate pane gets the fallback name of P6.1 and `parent: null`, and it becomes an Orphan only by the Team rule of P8.3.

### P3 — Host selection inside a team

This clause adds a first rule to PRD-261107-0825-01 P3 and owns that rule.

1. When `TT_TEAM` is set, `TT_HOST` selects the host, before `TMUX_TOOLS_HOST` and before detection. A `TT_HOST` value that is not a host with a backend fails the invocation with exit 1 and names the valid values. `TT_TEAM` set with `TT_HOST` unset fails with exit 1 and names `TT_HOST`. The selection source is `Team`.
2. When `TT_TEAM` is set and its Team record does not exist, the invocation fails with exit 1 and `team not found: <id>` on stderr.
3. Inside a team, the calling pane is the Leader pane handle from the Team record. `TMUX_PANE` is never read inside a team. This applies to every verb that acts on the calling pane, including `self`, `notify` without `--target`, the self-pane refusal, and placement without `--target`.
4. Inside a team on tmux, the tmux backend runs real tmux by the absolute path and with the server socket (`-S`) that the Team record holds (P4.2). It never resolves `tmux` through `PATH` and never reads `$TMUX`.

### P4 — Team record and team directory

1. Each Team has one team directory: `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/teams/<team id>/`. A team id is 8 random lowercase hex digits. The Launcher builds the directory as `teams/.init-<team id>/` (exclusive create; a new id on a collision), creates `leader.lock.new`, takes its exclusive lock, renames it to `leader.lock` (the lock stays held), writes `team.json` and `bin/tmux`, and then renames the directory to `teams/<team id>/` with a rename that fails when the destination exists (`RENAME_EXCL` on macOS, `RENAME_NOREPLACE` on Linux). When the rename fails because the destination exists, the Launcher makes a new id, rewrites `team.json`, and renames again. The Launcher builds the child environment of P2.3 only after the rename. The directory holds:
   - `team.json` — the Team record (`## Contracts`), written by write-to-temp and atomic rename;
   - `team.lock` — the lock that every change of `team.json` and every Shim pane create or close holds (`flock`, exclusive);
   - `leader.lock` — locked exclusively by the Launcher for its whole life, with close-on-exec; the kernel releases it when the Launcher ends;
   - `bin/tmux` — the Shim (P5.1); `bin/` holds no other file;
   - `shim.log` — the Shim log (P5.9), only when `TT_SHIM_DEBUG=1` is not set (item 5).
2. On tmux, the Launcher records in the Team record the absolute path of `command -v tmux` from the caller's `PATH` before the shim directory is added, and the socket path (first comma field) of the caller's `$TMUX`.
3. There is no daemon. The Team record holds the Leader (host handle, Shim id `%1`), the next teammate number, the Launcher pid, the host and `server_id`, the cwd, and the Claude binary (resolved path, canonical path, SHA-256, and version).
4. A Team is running while `leader.lock` is held. Every `claude-teams` start runs garbage collection over `teams/<id>/` and `teams/.init-<id>/` directories. For each one, it opens the existing `leader.lock` without creating it (a directory without `leader.lock` is skipped) and tries the exclusive lock without blocking. It deletes the directory only while it holds that lock itself; a directory whose lock it cannot take is kept. Garbage collection never touches `shim-debug/` (item 5).
5. When `TT_SHIM_DEBUG=1` is in the Launcher's environment, the Launcher creates the debug directory `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/shim-debug/<team id>/` after the rename of item 1 and before it starts Claude, and writes a copy of `team.json` there. When it cannot create the debug directory or write that copy, this is a setup failure of P2.7: the Launcher removes the team directory that it made and fails open. With `TT_SHIM_DEBUG=1`, the Shim writes `shim.log` in the debug directory, not in the team directory. The deletion of P2.5 and the garbage collection of item 4 never touch `shim-debug/`; its directories stay until the user deletes them.

### P5 — Shim command surface

1. The Shim is a symbolic link `bin/tmux` to the canonical path of the running `tmux-tools` binary. `tmux-tools` started with `argv[0]` basename `tmux` runs as the Shim. The Shim with `TT_TEAM` unset exits 1 with `tmux: not inside a tmux-tools team` on stderr.
2. The Shim is a thin front end over the Host port. It never runs tmux itself and builds no host command (PRD-261107-0825-01 P2.2). It acts only on the panes of its own Team: the Leader and the Teammate panes whose registration `team` is `TT_TEAM`. A target outside the Team exits 1 with `tmux: can't find pane: <target>`.
3. Global options before the command: `-S <path>`, `-L <name>`, and `-f <file>` are accepted and ignored. `-V` prints `tmux 3.4` and exits 0. The Shim parses argv with this grammar, for global options and for each command: a word that starts with `-` and is not `-` or `--` is a cluster of one-letter flags; a flag that takes a value takes the rest of the cluster (`-t%2`, `-l30%`) or, when nothing is left, the next word; `--` ends the flags; the first word that is not a flag ends the flags. The words after the flags are the command name (global level), the pane command (`split-window`, `respawn-pane`), the keys (`send-keys`), or the format (`display-message`). As in tmux, a word `;`, or a word that ends in `;` with no `\` before it, is a command separator (item 8). In a word that ends in `\;`, the `\;` is a literal `;`.
4. Targets (`-t`): a Shim id `%N`; the window `@0`; the session `tt-team-<id>`; and `tt-team-<id>:@0`, `tt-team-<id>:@0.%N`, `@0.%N`. A window or session target means the Leader where a pane is needed. Every Team has one fake session `tt-team-<id>` (`session_id` `$0`) with one fake window `@0` (`window_index` 0, `window_name` `team`) that holds all its panes.
5. Format strings (`-F`, `display-message -p`): the Shim expands `#{pane_id}` (Shim id), `#{window_id}`, `#{window_index}`, `#{window_name}`, `#{session_name}`, `#{session_id}`, `#{pane_title}` (stored title or empty), and the 8 fields of Claude's image-support probe: `client_termtype` empty, `allow-passthrough` `off`, `focus-events` `off`, `pane_top` `0`, `pane_left` `0`, `status` `off`, `status-position` `bottom`, `window_bigger` `0`. Every other `#{…}` expands to the empty string. Text outside `#{…}` is copied.
6. Commands with real work:
   - `display-message -p [-t <target>] <format>` prints the expanded format. Without `-p` it exits 0 with no output.
   - `list-panes [-t <target>] [-F <format>]` prints one line per live pane of the Team, Leader first, then Teammate panes by number. The default format is `#{pane_id}`.
   - `split-window [-d] [-h|-v] [-b] [-f] [-l <size>] [-c <dir>] [-e K=V]... [-P] [-F <format>] [-t <target>] [--] [<command>...]` creates a Teammate pane (P6). Without `<command>`, the pane runs the caller's login shell (P6.1). The target must be a pane of the Team (item 2), but the Shim chooses the split target and direction itself (P6.1); `-h`, `-v`, `-b`, `-f`, and `-l` are accepted and ignored. With `-P` it prints the expanded format for the new pane (default `#{pane_id}`).
   - `respawn-pane -k [-c <dir>] [-e K=V]... [-t <target>] [--] <command>...` replaces the command of a Teammate pane (P6.2). Without `-k` it exits 1 with `tmux: respawn-pane needs -k`. The Leader is refused with `tmux: can't respawn leader pane`.
   - `kill-pane [-t <target>]` closes a Teammate pane through the Host port and deletes its Pane registration. The Leader is refused with `tmux: can't kill leader pane`.
   - `send-keys [-l] [-t <target>] <key>...` sends through the `send` rules of the host (PRD-261107-0825-01 P9.4 on Orca); `-l` is `--literal`.
   - `capture-pane -p [-J] [-S <start>] [-E <end>] [-t <target>]` prints the pane through `capture`; `-S` with a negative number or `-` adds history. Without `-p` it exits 1 with `tmux: capture-pane needs -p`.
   - `has-session [-t <target>]` exits 0 for its own session or no target; else exit 1 with `tmux: can't find session: <target>`.
   - `select-pane -T <title> [-t <target>]` stores the title as display data (P6.4). `select-pane` without `-T` is accepted and ignored.
7. Commands accepted and ignored with exit 0 and no output, each with every flag and argument: `set-option`, `select-layout`, `resize-pane`. The size flag `-l` of `split-window` and every size or layout request are dropped and never reach the host, so the Shim never causes exit 3 (PRD-261107-0825-01 P5.3).
8. Errors: `new-session` and `new-window` exit 1 with `tmux: unsupported command: <cmd>`. Every other command name, alias, or `;` separator exits 1 with `tmux: unsupported command: <cmd>`. A flag that item 6 or 7 does not list for that command exits 1 with `tmux: unsupported flag <flag>`. The Shim exits only 0 or 1 and writes errors only to stderr as `tmux: <message>`.
9. Every exit 1 of the Shim is a refusal (`refused`), and its `message` is the stderr message without the `tmux: ` prefix. `shim.log` is JSONL: each line is one JSON object with the keys `time` (RFC 3339 string), `pid` (number), `outcome` (`"ok"` or `"refused"`), `message` (string), and `argv` (array of strings, the complete argv after `argv[0]`). For `ok`, `message` is `""`. The log is `shim.log` in the team directory, or in the debug directory with `TT_SHIM_DEBUG=1` (P4.5). Every refusal appends one line, except when the team directory does not exist (P5.1 with `TT_TEAM` unset, P3.2): then the Shim writes no line. With `TT_SHIM_DEBUG=1`, every invocation appends one line, and each line also has `exit` (number) and `stdout` (string, the complete stdout). Without `TT_SHIM_DEBUG=1`, a line has no `exit` and no `stdout` key.

### P6 — Teammate panes

1. `split-window` holds `team.lock`, takes the next teammate number `k` (from 1), and creates a pane through the Host port. The Shim chooses the split target and direction itself: when the Team has no live Teammate pane, the new pane goes right of the Leader pane on every host. Else, on a host with `tile` (tmux), the new pane goes below the tallest live Teammate pane (most rows; on a tie, the highest number), which the Shim reads with the Host port operation that reads window and pane geometry (PRD-261107-0825-01 P2.1); on a host without `tile` (Orca), it goes below the live Teammate pane with the highest number. It requests no size on any host. When the create fails because the pane has no space (tmux reports `no space for new pane`), the Shim exits 1 with `tmux: no space for a new pane`, and `k` is not used. The new pane gets Shim id `%<k+1>` and name `<leader name>.tm<k>`, where `<leader name>` is read at this `split-window` from the Leader pane's current Pane registration by the Leader handle (P2.8). When the result does not match the name rule of PRD-261107-0825-01 P4.1 or is longer than 100 bytes, or the Leader has no name, the name is `team-<team id>.tm<k>`. When that name is already reserved, `k` advances and the Shim tries again. The reservation and handle rules of PRD-261107-0825-01 P4.4 apply. `-c` and `-e` are applied to the pane command as `cd '<dir>' && env K=V … <command>`, with POSIX shell quoting. When `split-window` has no `<command>`, `<command>` is the login shell `${SHELL:-/bin/sh}` of the caller (the Shim's environment), with the same `cd` and `env K=V`. This placeholder shell stays until `respawn-pane` or `kill-pane`.
2. `respawn-pane -k` holds `team.lock`, creates the new pane below the old pane with the new command, then closes the old pane. The Teammate pane keeps its name and Shim id. The new pane gets every key of the old pane's Pane registration except the handle (PRD-261107-0825-01 P4: every `@tt-*` option on tmux, `@tt-launched-at` included; every Pane record field on a registry host), with the P6.4 values that derive from the command (`claude_agent_name`, `claude_team_name`, `agent`) taken from the new command. Respawn uses only the existing create, registration, and close operations of the Host port on every host; no Host port operation is added. When the create fails, the old pane stays and the Shim exits 1; for lack of space, with the message of item 1.
3. Every Teammate pane command, the placeholder shell of P6.1 included, runs in the Close-on-success wrap: exit 0 closes the pane; any other exit keeps a shell in the pane so the output stays readable. The wrap is the same on every host; no Host port option is added for it. On Orca, the wrap uses `exec` as `launch --bare` does (PRD-261107-0825-01 P9.9). `[OPEN: orca-close-on-clean-exit]`
4. A Teammate pane is a normal Pane registration with these extra fields: `team` (team id), `parent` (the Leader name read at the `split-window` of P6.1, or `null` when the Leader has no name), `shim_id`, `title` (from `select-pane -T`), `claude_agent_name` and `claude_team_name` (from `--agent-name` and `--team-name` in the `respawn-pane` command). `title`, `claude_agent_name`, and `claude_team_name` are display data and never resolve a Target. `agent` is `null` after `split-window` and `claude` after a `respawn-pane` whose command runs the Claude binary of the Team record. The Shim reads the `respawn-pane` command as follows. When the command is one word, it is split into words with POSIX shell quoting limited to blanks, single quotes, double quotes, and backslash, with no expansion; the words `&&`, `||`, `;`, and `|` separate commands. The Claude command is the first command whose first word, after `cd <dir>` commands and after `env` and its `K=V` words, is the Team's `ClaudeBinary.path`, or canonicalizes to it (a word without `/` is resolved through the Leader's `PATH`). `--agent-name <v>`, `--agent-name=<v>`, `--team-name <v>`, and `--team-name=<v>` are read from the words of that command. When the split fails or no Claude command is found, `claude_agent_name` and `claude_team_name` are `null`, `agent` is `null`, and the respawn continues. Ownership of a Teammate pane follows PRD-261107-0825-01 P4.7 and the tmux rule; the Shim's own check is the `team` match of item P5.2.
5. Teammate panes get the host's normal pane environment. The Shim does not give them `TT_TEAM`, `TT_HOST`, the fake `TMUX`, or the shim directory on `PATH`. On Orca, a Teammate pane has the `ORCA_PANE_KEY`, `ORCA_TERMINAL_HANDLE`, and `ORCA_TAB_ID` values that Orca gives its own pane, never the Leader's values. The Shim drops every `-e` setting of these three variables. The P10.6 live run checks that Orca shows each teammate's status on its own pane.

### P7 — Parent on every create

1. `spawn-agent` and `launch` record `parent` when the calling pane is a tmux-tools pane; `parent` is the name of the calling pane. Else `parent` is `null`. The Shim's `split-window` records `parent` per P6.4, and `respawn-pane` keeps the old pane's `parent` (P6.2).
2. A pane is a tmux-tools pane when `self` reports `driven: true` and a non-null `name` for it (PRD-261107-0825-01 P8; inside a team, P3.3).
3. On tmux, `parent` is the pane option `@tt-parent`. On a registry host, it is the Pane record field `parent`. `team`, `shim_id`, `title`, `claude_agent_name`, and `claude_team_name` are stored the same way (`## Contracts`).

### P8 — self, list, and kill

1. `self --format json` adds `team` (team id or `null`), `role` (`leader`, `teammate`, or `null`), and `parent` (`parent` of the calling pane's registration, or `null`). `role` is `leader` when `TT_TEAM` is set; else `teammate` when the calling pane's registration has a `team`; else `null`.
2. `list --format json` rows add `parent`, `team`, `shim_id` (each `null` when absent) and `orphan` (bool). The text table adds a `PARENT` column after `AGENT`; an Orphan row shows `orphan:<parent>`, or `orphan` when `parent` is `null`.
3. A row is `orphan: true` when one of these holds:
   - its `parent` is not `null` and the parent name does not resolve to a live pane, or resolves to a pane whose `launched_at` is later than the row's `launched_at` (`launched_at` is the `@tt-launched-at` option on tmux and the Pane record field on a registry host);
   - it has a `team` and that Team's `leader.lock` is not held or its team directory does not exist.
   `orphan` is reported only; no verb closes a pane because it is an Orphan.
4. `kill --target <name>` acts on a Teammate pane or an Orphan as on any pane. `kill` of a Leader closes no Teammate pane. `prompt` and `send` into a Teammate pane are allowed as on any pane.
5. The Leader pane is an ordinary Pane registration when `spawn-agent` created it. `prompt`, `wait-idle`, `capture`, `send`, `interrupt`, and `kill` on the Leader keep their meaning. The Launcher passes the terminal through to Claude and adds no output of its own while Claude runs. Interrupt keys reach Claude, and the Launcher ignores them (P2.5). On Orca, Host state and the P9.6 delivery determination of PRD-261107-0825-01 for a Leader under the Launcher are verified live. `[OPEN: orca-leader-agent-state]`

### P9 — spawn-agent claude through the launcher

1. On Orca, `spawn-agent` for an Agent profile whose binary basename is `claude` runs the pane command `<tmux-tools path> claude-teams --claude <binary> -- <claude args>`, where `<tmux-tools path>` is the canonical path of the running binary, `<binary>` is the profile's resolved binary (the program that `spawn-agent` starts today), and `<claude args>` are the arguments that `spawn-agent` builds today. The Pane registration, `agent`, Surface, and Keep-alive behavior stay as today.
2. Step 3 of P11 extends item 1 to tmux.
3. `orca claude-teams` is never used. Orca managed-account login is not replicated. Orca shows a plain `claude` leader, not "claude agent teams".

### P10 — Contract tests per Claude version

1. The Claude version key is the first blank-separated word of `ClaudeBinary.version` (`2.1.319` for `2.1.319 (Claude Code)`). The fixture name, the P10.5 match, and the P10.5 warning use this key. A trace fixture per Claude version, `tests/fixtures/claude-teams/<version key>.jsonl`, holds every tmux argv that this Claude version sends in teammate mode. Each line is one JSON object `{"argv":[string],"exit":number,"stdout":string}`: `argv` is the argv after `argv[0]`, `exit` the expected exit code, `stdout` the expected complete stdout. In `argv` and `stdout`, the team id, the cwd, and the Claude binary paths (P10.3) are the placeholders `{{team_id}}`, `{{cwd}}`, and `{{claude}}`. The first fixture is for Claude 2.1.319.
2. The default `cargo test --workspace --features test-fixtures` run replays each fixture through the real binary started as `tmux` (P5.1), inside a Team on the isolated tmux test server, with no Orca. A fake `claude` script on `PATH` replays the fixture and exits 0 or 1. Replay fills the placeholders with the replay Team's values (`{{claude}}` = the path of the fake `claude`); the Shim's stdout must equal the filled `stdout` exactly, and its exit must equal `exit`; the tests check P2.5, P6, P7, and P8 through the CLI (`list --format json`, `self --format json`). One test, in a 120×24 window, starts teammates until a `split-window` exits 1 with `tmux: no space for a new pane`, then respawns teammate `tm1` once. It records in the test output the maximum teammate count and the respawn result. It passes when the failed start creates no pane and no registration, and the respawn either exits 0 or exits 1 with `tmux: no space for a new pane` while the old pane and its registration stay.
3. An opt-in live test (environment switch, never by default) runs the real `claude` under `claude-teams` on the isolated tmux server with `TT_SHIM_DEBUG=1`, makes it spawn one teammate, reads `shim-debug/<team id>/` (P4.5) after Claude exits, and fails when its `shim.log` has a `refused` line or when the Claude version has no fixture. A new Claude version gets its fixture from a `shim.log` written with `TT_SHIM_DEBUG=1`, from this run or from any other run. The fixture tool is the test helper binary `tt-fixture` (`src/bin/tt-fixture.rs`, declared in `Cargo.toml` with `required-features = ["test-fixtures"]` as `tt-fake-tui` is, so the release build does not contain it): `cargo run --features test-fixtures --bin tt-fixture -- <shim.log> > tests/fixtures/claude-teams/<version key>.jsonl`. It parses each JSONL line of `shim.log` (P5.9), takes `argv`, `exit`, and `stdout`, and replaces the team id, the cwd, and the Claude binary paths `ClaudeBinary.resolved` and `ClaudeBinary.path` of that Team (from the `team.json` in the same directory as the given `shim.log`) with the placeholders of P10.1; both Claude paths become `{{claude}}`. Over these four values, it replaces the longest value first.
4. The Shim fails closed: an unknown command or flag exits 1 (P5.8); the Shim never guesses.
5. When the Claude version that the Launcher records has no fixture in the binary's build, the Launcher prints `claude-teams: claude <version key> not contract-tested` on stderr once and continues.
6. The Orca contract run of PRD-261107-0825-01 P10.2 adds: a Team with one teammate whose pane shows in `orca terminal list` and in `tmux-tools list` with `team` and `parent`; Orca reports `agentIdentity` `claude` for the teammate pane (`orca terminal list --json`); Orca shows the teammate's status on its own pane, not on the Leader pane (P6.5); a clean leader exit that closes the teammate; a leader crash that leaves an Orphan that `list` reports. Each run closes every pane it created and ends with no leaked pane (PRD-261107-0825-01 P10.4).

### P11 — Rollout order

1. The Launcher, the Shim, P3, P4, P6, P7, and P8, with the P10.2 tests on tmux. `claude-teams` works by hand on tmux and Orca.
2. P9.1 on Orca, after the `orca-close-on-clean-exit` and `orca-leader-agent-state` live checks pass and P10.6 passes, and after harness PRD-261108-0235-01 rollout steps 2 (hooks) and 3 (text and scripts) are released.
3. P9.1 on tmux, after step 2, and after harness PRD-261108-0235-01 rollout steps 2 (hooks) and 3 (text and scripts) are released.
4. Each step bumps the minor version. README documents `claude-teams`, the fake `TMUX` and shim `PATH` in the Leader's child processes (a `tmux` command run by the Leader or its tools reaches the Shim), the new `list` and `self` fields, and the Claude-version warning. README states that harness PRD-261108-0235-01 rollout steps 2 (hooks) and 3 (text and scripts) must be released before the Launcher is used, because inside a Leader `$TMUX` and `tmux` reach the Shim; the harness hooks must call `tmux-tools self` instead of `tmux display-message`, as the Shim has no compatibility path for `#{@tt-*}` formats and expands them to the empty string (P5.5). README also states the fail-open start of P2.7, that `shim-debug/` directories (P4.5) stay until the user deletes them, and the limit of P6.1: on tmux, when a window has no space for a new pane, a teammate start fails with `tmux: no space for a new pane`.
5. The harness `cl` alias becomes `tmux-tools claude-teams -- --dangerously-skip-permissions` only after step 1 is released and after harness PRD-261108-0235-01 rollout steps 2 (hooks) and 3 (text and scripts) are released. Repository `claude` owns the alias change.

## Codebase fit

- Reuse: the Host port, `HostSelection`, and the tmux and Orca backends (PRD-261107-0825-01 P2, P3, P9; planned in ISSUE-261107-0921-01 and ISSUE-261107-0921-03); the window and pane geometry read of the Host port (today `core/src/layout.rs` `measure`, moved behind the port by ISSUE-261107-0921-01) for P6.1 on tmux; `core/src/registry.rs` `PaneRecord` and P4.4 reservation (planned); `core/src/names.rs` name rule and `@tt-*` keys; `src/cmd/safety.rs` and `names::pane_is_owned`; the `send` key translation (P9.4); `tests/integration.rs` isolated tmux harness.
- Conventions: `CONTEXT.md` glossary; ADR-260919-0146-01 (surfaces); PRD-261107-0825-01 P2.2 (no host commands outside a backend).
- New homes: `src/cmd/claude_teams.rs` (Launcher); `src/shim/mod.rs` (argv parser, dispatch) and `src/shim/format.rs` (format expansion); `core/src/team.rs` (`TeamRecord`, team directory, locks, Orphan test); `argv[0]` dispatch in `src/main.rs`; `tests/claude_teams.rs`; `tests/fixtures/claude-teams/2.1.319.jsonl`; `src/bin/tt-fixture.rs` with its `[[bin]]` entry in `Cargo.toml` (`required-features = ["test-fixtures"]`).
- Planned-record dependencies: `implementation_after` ISSUE-261107-0921-03 (Orca backend, which needs ISSUE-261107-0921-01 and -02); `requires` the PRD-261107-0825-01 clauses in frontmatter. Harness PRD-261108-0235-01 is not an `implementation_after`: its rollout steps 2 and 3 hold only the release of P11 steps 2 and 3 and the alias change of P11.5; the work of P11 steps 2 and 3 may start after step 1.
- Active-plan overlaps: ISSUE-261107-0921-01, -02, -03 (the same host selection, registry, `self`, and `list` code). ISSUE-261107-1229-01 (Orca pane identity across a restart: the Orca Pane record, the Team record fields `server_id` and `TeamLeader.handle`, and the lookup of the Leader's Pane registration by its handle). ISSUE-261107-2103-01 is superseded. Repository `claude`, PRD-261108-0235-01: its hooks read `$TMUX_PANE` and run `tmux display-message`, which inside a Leader reach the Shim.

## Contracts

### Business entities

- **`TeamRecord`** (new; `core/src/team.rs`) — `schema: "tmux-tools.team.v1"`, `id: String` (8 lowercase hex), `host: Host`, `server_id: String` (Orca `runtimeId`; on tmux the server socket path), `leader: TeamLeader`, `next_teammate: u32` (next `k`, starts at 1), `launcher_pid: u32`, `cwd: PathBuf`, `created_at: String` (RFC 3339), `claude: ClaudeBinary`, `tmux: Option<TmuxEndpoint>` (`Some` only on tmux).
- **`TeamLeader`** (new) — `handle: String` (host pane handle: tmux `%id`, Orca `term_…`), `shim_id: String` (always `"%1"`). It has no name field: the Shim reads the Leader name from the Leader's current Pane registration by `handle` at each `split-window` (P6.1).
- **`ClaudeBinary`** (new) — `resolved: PathBuf` (the `command -v` result of P2.2, or of the `--claude` program, as the Launcher used it to start Claude), `path: PathBuf` (canonical), `sha256: String` (lowercase hex of the file at `path`), `version: String` (first line of `--version`).
- **`TmuxEndpoint`** (new) — `path: PathBuf` (absolute real tmux), `socket: PathBuf` (first comma field of the caller's `$TMUX`).
- **`TeamRole`** (new) — enum: `Leader`, `Teammate`.
- **`PaneRecord`** (changed; PRD-261107-0825-01) — adds `parent: Option<String>`, `team: Option<String>`, `shim_id: Option<String>`, `title: Option<String>`, `claude_agent_name: Option<String>`, `claude_team_name: Option<String>`. Absent key = `None`. `schema` stays `"tmux-tools.pane.v1"`.
- **`SelectionSource`** (changed; PRD-261107-0825-01) — adds variant `Team(team_id: String)`.
- **`orphan`** is derived per P8.3 and never stored; no entity. It includes a pane whose `team` names a team directory that does not exist.

### External contracts

- **Verb `claude-teams`** — `tmux-tools claude-teams [--claude <program>] [-- <claude args>]`. Interactive; no `--format`. `--claude <program>` is hidden (not in `--help`); its consumer is `spawn-agent` (P9.1); without it the program is `claude`; the Launcher resolves it with `command -v` on the caller's `PATH`. Exit: Claude's status (128 + signal for a signal); 1 with `claude-teams: <reason>` only for `claude not found` and `already inside team <id>`. Adds `--teammate-mode tmux` with a Team, and `--teammate-mode in-process` on fail-open, unless the caller passes `--teammate-mode`. Fail-open stderr line: `claude-teams: no team: <reason>; starting claude without teammate panes` (P2.7). Behavior per P2.
- **Child environment set by the Launcher** — `PATH=<team dir>/bin:$PATH`; `TMUX=<team dir>/tmux-fake,<launcher pid>,0`; `TMUX_PANE=%1`; `TT_TEAM=<team id>`; `TT_HOST=tmux|orca`.
- **Environment read by host selection** — `TT_TEAM` (team id) and `TT_HOST` (host name); rule P3.1; errors `team not found: <id>`, missing `TT_HOST`, invalid `TT_HOST`.
- **Environment `TT_SHIM_DEBUG`** — `1` logs every Shim call with its `exit` and `stdout` (P5.9); in the Launcher's environment it creates the debug directory, and the Shim then writes `shim.log` there (P4.5).
- **Teammate pane environment on Orca** — `ORCA_PANE_KEY`, `ORCA_TERMINAL_HANDLE`, `ORCA_TAB_ID` are the values of the teammate's own Orca pane, never the Leader's; the Shim drops `-e` settings of these three (P6.5).
- **Team directory** — `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/teams/<team id>/`, built as `teams/.init-<team id>/` and renamed with no replace (P4.1), with `team.json` (`TeamRecord`, JSON keys = field names, `host` lowercase), `team.lock` and `leader.lock` (`flock`), `bin/tmux` (symlink to the canonical `tmux-tools` binary), `shim.log` (JSONL, one object per line: `{"time": string, "pid": number, "outcome": "ok"|"refused", "message": string, "argv": [string]}`; `message` is `""` for `ok`; with `TT_SHIM_DEBUG=1`, every line also has `"exit": number` and `"stdout": string`; P5.9). Without `TT_SHIM_DEBUG=1` only, `shim.log` is in the team directory. Garbage collection deletes a `teams/<id>/` or `teams/.init-<id>/` directory only while it holds that directory's `leader.lock` (P4.4).
- **Debug directory** — `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/shim-debug/<team id>/`, only with `TT_SHIM_DEBUG=1`; holds a copy of `team.json` (written by the Launcher at start) and `shim.log` (same format); never deleted by tmux-tools (P2.5, P4.4); a failure to create it or its `team.json` copy is a P2.7 fail-open; consumer `tt-fixture` and the P10.3 live test (P4.5).
- **Shim CLI (`tmux` subset, consumer: Claude Code)** — P5.3–P5.8: global `-S`/`-L`/`-f` ignored; `-V` → `tmux 3.4`; real work: `display-message`, `list-panes`, `split-window`, `respawn-pane -k`, `kill-pane`, `send-keys`, `capture-pane -p`, `has-session`, `select-pane -T`; ignored: `set-option`, `select-layout`, `resize-pane`, and the `split-window` flags `-h`, `-v`, `-b`, `-f`, `-l` (the Shim chooses split target and direction, P6.1); targets `%N`, `@0`, `tt-team-<id>` and their combinations; format variables of P5.5, other `#{…}` → empty; errors `tmux: unsupported command: <cmd>`, `tmux: unsupported flag <flag>`, `tmux: can't find pane: <t>`, `tmux: can't find session: <t>`, `tmux: can't kill leader pane`, `tmux: can't respawn leader pane`, `tmux: respawn-pane needs -k`, `tmux: capture-pane needs -p`, `tmux: no space for a new pane`, `tmux: not inside a tmux-tools team`; exit 0 or 1 only.
- **tmux pane options (new)** — `@tt-parent`, `@tt-team`, `@tt-shim-id`, `@tt-title`, `@tt-claude-agent-name`, `@tt-claude-team-name`; values as the `PaneRecord` fields.
- **Pane record file** — keys `parent`, `team`, `shim_id`, `title`, `claude_agent_name`, `claude_team_name` added (string or absent).
- **Teammate pane name** — `<leader name>.tm<k>`; fallback `team-<team id>.tm<k>` when invalid, longer than 100 bytes, or no leader name.
- **Changed JSON** — `self --format json` adds `team: string|null`, `role: "leader"|"teammate"|null`, `parent: string|null` (this PRD owns the extension of PRD-261107-0825-01 P8). `list --format json` rows add `parent`, `team`, `shim_id` (`string|null`) and `orphan: bool` (P8.3, including a `team` whose team directory does not exist).
- **Changed text** — `list` table adds column `PARENT` after `AGENT`; an Orphan shows `orphan:<parent>` or `orphan`.
- **Stderr warning** — `claude-teams: claude <version key> not contract-tested` (P10.5).
- **Trace fixture file** — `tests/fixtures/claude-teams/<version key>.jsonl`, JSONL, one object per line: `{"argv": [string], "exit": number, "stdout": string}`; placeholders `{{team_id}}`, `{{cwd}}`, `{{claude}}` in `argv` and `stdout`; producer `tt-fixture`, consumer the P10.2 replay (P10.1–P10.3).
- **Test helper `tt-fixture`** — `cargo run --features test-fixtures --bin tt-fixture -- <shim.log>`; reads a `TT_SHIM_DEBUG=1` `shim.log` and the `team.json` in the same directory, replaces the team id, the cwd, `ClaudeBinary.resolved`, and `ClaudeBinary.path` (both as `{{claude}}`), longest value first, writes the fixture JSONL to stdout; not in the release build (P10.3).
- **Pane command of `spawn-agent claude` on Orca** — `<tmux-tools path> claude-teams --claude <binary> -- <claude args>` (P9.1).

## Verification and release

The epic is done when all of these hold:

1. The P10.2 replay suite passes on tmux in the default test run, and the P10.3 live run on tmux has no `refused` line for Claude 2.1.319.
2. On a live Orca runtime, `spawn-agent claude --name lead` gives a Leader whose teammate shows in `orca terminal list` and in `tmux-tools list --format json` as `lead.tm1` with `team` and `parent: "lead"`; `prompt`, `wait-idle`, and `kill` work on `lead`.
3. A clean `/exit` of the Leader closes every Teammate pane. A killed Leader leaves Teammate panes that `list` reports as `orphan: true`, and `kill --target <name>` closes them.
4. On tmux, after P11 step 3, `spawn-agent claude --name lead` gives the same outcome as item 2: teammate `lead.tm1` in `tmux-tools list --format json` with `team` and `parent: "lead"`; `prompt`, `wait-idle`, and `kill` work on `lead`.
5. After each run, neither `tmux-tools list` nor the host's own pane list shows a leaked pane.
6. `tmux-tools claude-teams` in a terminal with no supported host prints the P2.7 warning, creates no team directory, and starts Claude with `--teammate-mode in-process`; Claude's exit status is the verb's exit status.

Rollback is reverting the release. A previous version ignores the new Pane record fields and `@tt-*` options; team directories left on disk are inert.

## Out of Scope

- Rex and Herdr: the Shim reaches them when their backends exist (later PRDs).
- The `claude --tmux` worktree feature (`new-session`, `switch-client`, `attach-session`).
- Orca managed-account login and the `orca claude-teams` environment changes.
- Harness hook and skill changes (repository `claude`).
- Teammate panes drawn in-process.

## Open Questions

### orca-close-on-clean-exit
- question: Does an Orca pane started with the Close-on-success wrap (`exec` form) close when the command exits 0, and keep a shell when it exits non-zero? The P9.9 login shell must not keep every pane open. Acceptance: a live Orca run shows both outcomes in `orca terminal list`. Evidence so far (Notes, item 1): an Orca pane runs `orca-terminal-host` → `login` → `zsh -l` → the command.
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

### orca-leader-agent-state
- question: With `claude` running as a child of `tmux-tools claude-teams` in an Orca pane, does `terminal show` report `agentIdentity` `claude` and does `orca worktree ps --json` report the agent row with `hasLivePty: true`? Acceptance: a live Orca run records both; else P8.5 states the Leader's Host state is `null` and its multi-line prompt delivery is not guaranteed. Evidence so far (Notes, item 2): Orca reports `agentIdentity` `claude` for the versioned binary started directly in a pane, also with Orca hooks off; a `claude` under a parent process is not checked yet.
- dimension: contracts/integrations
- resolve-by: during-triage
- owner: this PRD

## Notes

Live Orca checks, 2026-11-08 (Orca 1.4.251, Claude 2.1.319):

1. Orca pane environment: `TMUX` unset, `TERM_PROGRAM=Orca`; `ORCA_TERMINAL_HANDLE`, `ORCA_PANE_KEY`, `ORCA_AGENT_PANE`, `ORCA_TAB_ID`, `ORCA_WORKTREE_ID`, `ORCA_WORKSPACE_ID`, `ORCA_CLI_BIN_DIR`, `ORCA_APP_VERSION`, `ORCA_AGENT_HOOK_{PORT,TOKEN,ENDPOINT,TRANSPORT,VERSION,ENV}`, `ORCA_CLAUDE_AGENT_VIEW_PLUGIN_DIR`, and `CLAUDE_CODE_PLUGIN_DIRS` (Orca's `orca-agent-view` plugin). Process chain: `orca-terminal-host` → `login` → `zsh -l` → command.
2. Recognition: `orca terminal list --json` reports `agentIdentity: "claude"` for a pane that starts the versioned path `~/.local/share/claude/versions/2.1.319` directly through `orca terminal split --command`, with Orca hooks on and with `ORCA_AGENT_HOOK_PORT`, `ORCA_AGENT_HOOK_TOKEN`, `ORCA_PANE_KEY`, `CLAUDE_CODE_PLUGIN_DIRS` unset. The mechanism is not the basename and not the hooks. Claude inside tmux inside Orca shows `agentIdentity: null`.
3. `teammateMode: "tmux"` outside tmux: Claude starts a hidden detached server `tmux -L claude-swarm-<leader pid> new-session -d -s claude-swarm -n swarm-view …` and runs the teammate as pane `%0` there; no Orca pane appears. The teammate has `TMUX=…/claude-swarm-<pid>,…`, `TMUX_PANE=%0`, and the Leader's `ORCA_PANE_KEY` and `ORCA_TERMINAL_HANDLE` (P6.5). `TaskStop` ends the teammate and the server. This is the reason for `--teammate-mode in-process` on fail-open (P2.7).
4. Orca's Claude hook (`~/.orca/agent-hooks/claude-hook.sh`, global `~/.claude/settings.json`, 16 events) POSTs each hook event to `127.0.0.1:$ORCA_AGENT_HOOK_PORT` with the pane key from the environment. Hook events from a process with the Leader's `ORCA_PANE_KEY` update the Leader pane's status (P6.5). This interface is private; tmux-tools never calls it.
5. Orca CLI surface used by the backend: `orca terminal list|show|read|send|split|create|close|wait --json`; `split` takes `--terminal <handle> --direction horizontal|vertical --command <text>`. Orca has no pane size control (P6.1).
6. The installed `tmux-tools` 0.3.0 has no `self`, `notify`, or Orca backend; ISSUE-261107-0921-01..03 deliver them first (`implementation_after`).

herdmates (github.com/caioniehues/herdmates, a Herdr plugin of the same launcher + `tmux` symlink shape) found these failure classes: host size semantics differ (#135, #139: inverted `-l`, `resize-pane -x` drift, thin rows without layout), which P6.1 avoids by choosing target and direction and setting no size; a versioned binary basename made the host show teammates as unknown (#134), which Notes item 2 shows does not happen on Orca; launch by typing into a pane hit shell autocorrect (#117), which P6.2 avoids by create-then-close; hook signals routed to the wrong pane (#133), the class of P6.5.

Herdr surface for its later backend: `herdr pane split|run|read|rename|close|resize|get|list|layout|report-metadata`, `herdr agent wait|list`, `herdr notification show`, `herdr api schema --json`; opaque ids (`w1A:p9`, `w1A:t3`); environment `HERDR_ENV=1`, `HERDR_SOCKET_PATH` (NDJSON), `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID`, `HERDR_BIN_PATH`, `HERDR_AGENT`; events `pane.exited`, `pane.closed`.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement; ## Verification and release |
| scope boundary | decided | ## Outcome and scope; ## Out of Scope |
| domain terms | decided | P1 |
| architecture shape | decided | P3; P5.1; P5.2 |
| stack | n/a | brownfield Rust workspace — existing conventions ratified |
| data/schema | decided | P4; P6.4; P7.3; ## Contracts |
| contracts/integrations | deferred | orca-close-on-clean-exit; orca-leader-agent-state |
| UX (if UI) | n/a | CLI only; output per P8 |
| testing/seams | decided | P10 |
