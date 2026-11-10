# Context: tmux-tools

The ubiquitous language for driving tmux panes from LLM agents. Single context; this file is
the whole glossary.

## Hosts

**Host** [VO] — the terminal program whose panes the tool drives: `tmux`, `orca`, or `herdr` (herdr-core terminals driven through the `hc` CLI). Every verb reaches its host only through the Host port, a pane-level interface; no module outside a [[Host backend]] builds host commands. _Avoid_: terminal, multiplexer. ↔ core/src/host/mod.rs (`Host`, the `HostPort` trait)

**Host backend** [adapter] — the implementation of the Host port for one [[Host]]: the tmux backend, the Orca backend, and the Herdr backend. Host facts live in their backend: on tmux the [[Managed session]], [[Bracketed paste transport]], the `@tt-*` storage of [[Pane registration]], and [[Layout intent]] as `@tt-tiled`; on Orca every `orca` CLI invocation and the [[Pane record]] reconciliation; on Herdr every `hc <command> --json` invocation (never `herdr-app`, never `hc connect`), the `tt.*` label storage of [[Pane registration]], and the `send` key-name map. _Avoid_: driver, plugin. ↔ core/src/host/tmux/ (`TmuxHost`), core/src/host/orca.rs (`OrcaHost`), core/src/host/herdr.rs (`HerdrHost`)

**Host selection** [VO] — the [[Host]] chosen for one invocation and the source of that choice: `TMUX_TOOLS_HOST` when set (`tmux`, `orca` or `herdr`; any other value is an error naming all three), else `$TMUX` set → tmux, else `ORCA_TERMINAL_HANDLE` set → orca, else `HC_TERMINAL_ID` set → herdr, else tmux. Under tmux the host stays tmux unless `TMUX_TOOLS_HOST=herdr`. Herdr is unreachable when `hc` is not on `PATH` or `hc server status --json` fails. An unreachable selected host fails the invocation naming the host and the source; it never falls back. _Avoid_: host detection, mode. ↔ core/src/host/mod.rs (`HostSelection`, `SelectionSource`)

**Host capabilities** [VO] — the optional features a [[Host backend]] supports: `native_state`, `submit_confirmation`, `notify`, `tile`, `rebalance`, `size`. The tmux backend supports `notify`, `tile`, `rebalance` and `size`; the Orca and Herdr backends support `native_state` and `submit_confirmation`. On Herdr `--session` and `--window` also exit 3. Capabilities are read on each invocation: an optional verb or option (`rebalance`, `--split tile`, `--size`) on a host that lacks it exits 3 with `unsupported: <verb> on <host>`, and the automatic tile after a spawn is skipped silently on a host without `tile`. _Avoid_: features, flags. ↔ core/src/host/mod.rs (`HostCapabilities`, `Unsupported`)

**Host state** [VO] — the agent state a [[Host]] itself reports for a pane: `working`, `blocked`, `done` or `idle`, or `null` when it reports none. Separate from [[Pane state]]; the tmux backend always reports `null`, the Orca backend maps the pane's `orca worktree ps` agent row (it never reports `idle`), and the Herdr backend maps the program's own OSC 7501 word (`status` of `hc get`): `idle`, `working`, `blocked` and `done` to themselves, `error` to `blocked`, anything else or a finished terminal to `null`. `status`, `list`, `wait-idle` and `prompt` report it as `host_state` in JSON; `status` also reports the host's exact word as `host_word` (always `null` on tmux and Orca). _Avoid_: agent status, native state. ↔ core/src/host/mod.rs (`HostState`, `HostPort::host_state`)

## Panes and sessions

**Managed session** [entity] — the `tmux-tools` tmux session this tool creates and reuses when invoked from outside tmux. _Avoid_: default session, our session. ↔ core/src/host/tmux/session.rs

**Target** [VO] — a resolved reference to one pane, parsed from a registered name, a pane id (`%3`), or a window id (`@7`) on tmux; on Orca from a pane name or a `term_…` handle; on Herdr from a pane name or an hc address `t<n>` — a registered name wins, a value is an address only when no registered name matches, and a dead name never resolves. A verb without `--target` acts on the calling pane (on Herdr the terminal named by `HC_TERMINAL_ID`). _Avoid_: selector. ↔ core/src/target.rs

**Pane registration** [VO] — what this tool records about a pane it created. On tmux it is the `@tt-*` pane user-options; on a registry host (Orca) it is the pane's [[Pane record]]; on Herdr it is the hc labels `tt.name`, `tt.agent`, `tt.access`, `tt.launched_at`, `tt.cwd`, `tt.surface`, `tt.surface_unvalidated` (`"true"` or `"false"`), `tt.parent`, `tt.team`, `tt.shim_id`, `tt.title`, `tt.claude_agent_name` and `tt.claude_team_name` on the [[hc terminal]], plus `where` set to the pane name (no Pane record). The fields: `@tt-name`, `@tt-agent`, `@tt-access`, `@tt-launched-at`, `@tt-cwd`, `@tt-surface`, `@tt-surface-unvalidated`, `@tt-parent` (the [[Parent]]), and on a [[Teammate pane]] `@tt-team` (team id), `@tt-shim-id` (its [[Shim id]]), `@tt-title`, `@tt-claude-agent-name` and `@tt-claude-team-name` (display data that never resolves a [[Target]]); on a registry host the [[Pane record]] fields `parent`, `team`, `shim_id`, `title`, `claude_agent_name` and `claude_team_name`. `@tt-surface` records the surface `spawn-agent` resolved and is written unconditionally; `@tt-surface-unvalidated` is present when caller-supplied trailing arguments could have changed the rendering out from under that record. `@tt-name` is set on every pane either spawning verb creates: `--name` when given (it must match `[A-Za-z0-9._-]+` and not be `.` or `..`, on every host), otherwise auto-derived from the command. A host title is display only and never resolves a [[Target]]. _Avoid_: metadata, tags. ↔ core/src/names.rs (fields, `validate_name`), core/src/host/tmux/registration.rs (`@tt-*` storage), core/src/registry.rs (Pane records)

**Pane record** [entity] — the registry file holding the [[Pane registration]] of one pane on a host that cannot store custom data on a pane: `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/<host>/<server_id>/<name>.json`, where on Orca `server_id` is the runtime's `runtimeId`. A spawn reserves the name by exclusive create before the pane exists, then writes the handle by atomic rename. An Orca restart makes a new `runtimeId`, so every earlier record directory is deleted and those panes stay open unregistered. _Avoid_: registry entry, state file. ↔ core/src/registry.rs (`PaneRecord`)

**Pane liveness** [VO] — the state of a [[Pane record]] checked against the host's pane list: `live` (its handle is listed), `dead` (not listed on a complete list: the record is deleted and reported once), or `orphan-reservation` (no handle and older than the 120-second spawn timeout: deleted). A handle not listed on an incomplete list stays and resolves to `pane list incomplete: <name>`; a younger reservation is a spawn in progress. _Avoid_: pane status, health. ↔ core/src/registry.rs (`PaneLiveness`)

**Keep-alive wrap** [service] — keeping a pane open after its launched command exits so its output stays readable: on tmux the command is wrapped as `(cmd); exec $SHELL`; on Orca the login shell that runs the command stays after it exits, and `--bare` runs `exec <cmd>`; on Herdr the command is wrapped as on tmux, and `--bare` runs the command so the [[hc terminal]] finishes with it. _Avoid_: shell wrapper, persistence. ↔ src/cmd/launch.rs (`keep_alive`)

**hc terminal** [entity] — one herdr-core terminal, addressed by `t<n>`; on Herdr a pane is an hc terminal. It shows in the Herdr window only through its `herdr.space` and `herdr.tab` labels. A finished hc terminal that carries `tt.name` is dead: the next invocation that resolves, lists or spawns that name removes it by address. _Avoid_: herdr pane, session. ↔ core/src/host/herdr.rs

**hc name** [VO] — the `--name` of an [[hc terminal]] this tool creates: `tt-` and the first 16 lowercase hex characters of SHA-256 of the UTF-8 tmux-tools pane name. hc refuses a name a live terminal holds, which keeps pane names unique among live terminals; terminals are always resolved and removed by address, never by hc name. _Avoid_: hc label, terminal title. ↔ core/src/host/herdr.rs (`hc_name`)

## Agents and readiness

**Agent profile** [aggregate] — one named registry entry: the binary to run, its [[Surface]] map, its access profiles, and its capabilities. _Avoid_: agent config, agent definition. ↔ core/src/agents/mod.rs

**Agent registry** [repository] — the built-in profiles deep-merged with the user's `agents.toml`. _Avoid_: agent list, catalogue. ↔ core/src/agents/mod.rs

**Access profile** [VO] — a named argument set granting an agent a level of filesystem authority, e.g. `read-only`, `workspace-write`, `full-access`. _Avoid_: permission, sandbox mode. ↔ core/src/agents/mod.rs

**Surface** [VO] — one named **rendering** of an agent: the launch arguments that select it and every rendering-dependent behavior (readiness patterns, bracketed-paste capability, interrupt key and its quit hazard, resume syntax), plus the agent version those declarations were measured against, and its [[Floor]]. An agent carries a map of surfaces; `spawn-agent --surface <name>` selects one, and the resolved name is recorded in [[Pane registration]]. _Avoid_: mode, profile, variant. ↔ core/src/agents/mod.rs

**Default surface** [VO] — the surface `spawn-agent` resolves when no `--surface` is given, named by `default_surface`. Exactly one per agent after the `agents.toml` deep-merge. It is promotable: a flat surface becomes the default only once it ships a `ready_regex`/`busy_regex` pair carrying a [[Validated version]]; until then the agent keeps its [[Pre-surface rendering]] as the default. When an `agents.toml` explicitly names a `default_surface` that fails that bar, the fallback is reported through the registry load warning rather than applied silently. _Avoid_: primary surface, current surface. ↔ core/src/agents/mod.rs

**Validated version** [VO] — the agent-binary version the manual smoke measured a [[Surface]]'s declared patterns, bracketed-paste capability and quit hazard against, stamped beside those declarations. A flat surface is promoted to [[Default surface]] only when its `ready_regex`/`busy_regex` pair carries that stamp. _Avoid_: tested version, smoke stamp. ↔ core/src/agents/mod.rs

**Pre-surface rendering** [VO] — the surface matching the rendering an agent launched with before surfaces existed, named by `pre_surface_rendering`. Readiness fields written in the agent-level (pre-surface) `agents.toml` shape bind here, and total pane resolution falls back here when a pane carries no [[Surface]] record. It is a fixed fact, not the promotable [[Default surface]]. _Avoid_: legacy surface, original surface. ↔ core/src/agents/mod.rs

**Flat surface** [VO] — a non-alternate-screen rendering, so the pane keeps tmux scrollback. `claude --ax-screen-reader` and `dsh --profile dshline` are flat. A flat rendering with no native idle/busy chrome cannot produce a measured pattern pair, so it stays unpromoted and the agent keeps its [[Pre-surface rendering]] as the [[Default surface]] (`claude --ax-screen-reader` is the case). _Avoid_: line mode, plain surface. ↔ core/src/agents/mod.rs

**Rich surface** [VO] — a full-screen (alternate-screen) rendering. For `claude` and `dsh` it is the [[Pre-surface rendering]]; it has no tmux scrollback. _Avoid_: full surface, TUI surface. ↔ core/src/agents/mod.rs

**Surface-unvalidated pane** [entity] — a pane carrying a [[Surface]] record that caller-supplied trailing arguments may have invalidated, marked `@tt-surface-unvalidated`. Arguments the surface supplies itself are validated by construction and do not set the mark. _Avoid_: unchecked pane, unverified pane. ↔ core/src/names.rs

**Readiness** [service] — one value of [[Pane state]]: the classification that an agent has finished its turn, derived from the resolved [[Surface]]'s `ready_regex`/`busy_regex` by the single classifier. On a host with `native_state`, [[Host state]] decides first when it is not `null` (unless `--until` or `--idle-seconds` is given); Readiness decides only when it is `null`. It is no longer the whole determination; when the classifier cannot decide — no patterns, no surface, or `unknown` — the wait falls back to [[Idle detection]], and a `busy` classification suppresses it. _Avoid_: done, completion, finished. ↔ core/src/idle.rs

**Pane state** [VO] — the total three-valued classification of a pane capture against a resolved [[Surface]]'s patterns: `idle`, `busy`, or `unknown`. `unknown` is distinct from `busy` and consumers needing certainty branch on it explicitly. Derived in exactly one place. _Avoid_: status, readiness value. ↔ core/src/idle.rs

**ready_regex** [VO] — the per-[[Surface]] pattern whose presence means idle-and-ready, matched against the pane's bottom non-blank lines. _Avoid_: prompt pattern, done marker. ↔ core/src/agents/mod.rs

**ready_lines** [VO] — how many bottom non-blank lines `ready_regex` is tested against; `0` means scan the whole visible pane. Per [[Surface]]. _Avoid_: scan depth, tail size. ↔ core/src/agents/mod.rs

**busy_regex** [VO] — the per-[[Surface]] pattern whose presence means the agent is generating. Matched against the whole capture, and never a synonym for `unknown`. _Avoid_: working marker, activity pattern. ↔ core/src/agents/mod.rs

**Idle detection** [service] — the fallback completion signal used when the classifier cannot decide (no patterns, no [[Surface]], or `unknown`), and only while [[Host state]] is `null`: the visible pane capture unchanged for a configured duration. A `busy` [[Pane state]] suppresses it. It is not a parallel definition of idleness alongside [[Readiness]]. _Avoid_: quiescence, settling. ↔ core/src/idle.rs

**Exited-agent pane** [entity] — a pane whose agent process has exited but which the keep-alive wrap holds open as a shell; it still carries `@tt-agent` and still consumes window geometry. _Avoid_: zombie pane, dead pane, stale pane. ↔ src/cmd/launch.rs

## Pane I/O

**Bracketed paste transport** [service] — delivering one prompt as one submission through `tmux load-buffer` from stdin followed by `paste-buffer -p -d`, keyed to the pane's live `bracket_paste_flag` rather than a [[Surface]]'s declared capability. _Avoid_: paste mode, chunked send. ↔ core/src/host/tmux/pane_io.rs (transport), src/cmd/prompt.rs (delivery decision)

**History mark** [VO] — the pane-history offset taken before a prompt is submitted, from which `prompt`'s result begins: on tmux the cursor row's absolute line, on Orca the `orca terminal read` cursor. Never advanced: the result always includes the agent's echo of the prompt. _Avoid_: anchor, watermark. ↔ src/cmd/prompt.rs

**Turn observation** [VO] — whether `busy_regex` was observed, or [[Host state]] `working` was seen, between submission and settle. It reports that the agent took the turn and makes no claim about where content begins. _Avoid_: turn detection, completion. ↔ src/cmd/prompt.rs

**Completeness** [VO] — whether everything after the [[History mark]] can be supplied. On tmux it is `false` when the pane has no scrollback (per live `alternate_on`), history has evicted lines, or the pane's width changed between the mark and settle. On Orca it is always `false`: Orca reports no pane width and no full-screen state. Reported independently of [[Turn observation]]. _Avoid_: truncation, fidelity. ↔ src/cmd/prompt.rs

## Layout and geometry

**Tile** [VO] — the placement mode that applies tmux's `tiled` layout to a window after each spawn; the implicit default inside tmux. _Avoid_: grid, auto-layout. ↔ core/src/layout.rs

**Floor** [VO] — the minimum pane geometry a [[Surface]] needs for its readiness signal to remain matchable, declared per surface as `min_cols`/`min_rows` and resolved from the pane's `@tt-agent` and `@tt-surface`. _Avoid_: minimum size, threshold, limit. ↔ core/src/agents/mod.rs

**Effective floor** [VO] — a window's floor: the componentwise maximum over the floors contributed by the panes in it; panes with no agent contribute none but are still measured. _Avoid_: window floor, combined minimum. ↔ core/src/layout.rs

**Ceiling** [VO] — the pane count at which a window's effective floor stops being satisfiable; derived from live window size, never stored. _Avoid_: max panes, pane cap, limit. ↔ core/src/layout.rs

**Advisory floor** [VO] — the property that a floor binds only panes this tool places: direct `tmux split-window` calls bypass it and concurrent spawns can overshoot it. _Avoid_: soft limit, best-effort floor. ↔ core/src/layout.rs

**Overflow window** [entity] — the window a pane is moved to when placing it would push its origin below the effective floor. _Avoid_: spillover window, secondary window. ↔ core/src/layout.rs

**Layout intent** [VO] — whether a window is tool-tiled or human-arranged, recorded as the `@tt-tiled` window option (unset when cleared, so "cleared" and "never set" are one state). A record of what the tool did, not a permission to act: it gates teardown (`kill` retiling), the `rebalance` verb, and overflow-destination eligibility, never placement — which is gated by [[Ownership]] alone, since the first tiling placement into any window necessarily finds no marker and is what sets one. _Avoid_: tiled flag, managed layout. ↔ core/src/layout.rs, core/src/host/tmux/window.rs (`@tt-tiled` storage)

## Ownership

**Ownership** [service] — the predicate deciding whether a pane is this tool's, read from the [[Pane registration]] of the pane's host (on Orca, the pane's [[Pane record]] exists; on Herdr, the `tt.name` label is present and `tt.cwd` is the evidence for the cwd clause): `@tt-name` or `@tt-agent` present, and where both the pane's `@tt-cwd` and the current working directory are known, they match; absence on either side passes. The rule is stable; what changes is the evidence it reads — once every created pane registers a name ([[Pane registration]]), the first clause holds for all of them, including a `launch` with no flags, which today it does not. _Avoid_: managed, ours, created-by. ↔ core/src/names.rs `pane_is_owned` (public in core; the safety evaluator in src/cmd/safety.rs delegates to it — PRD-260801-0656-01 · ISSUE-260801-1310-02)

**@tt-cwd** [VO] — the working directory of the invocation that *created* a pane — run identity, not where the agent works. _Avoid_: agent cwd, working directory. ↔ core/src/names.rs

## Teams

**Team** [aggregate] — one run of `tmux-tools claude-teams`: one [[Leader]] and its [[Teammate pane]]s. It is running while its [[Launcher]] holds the team directory's `leader.lock`. _Avoid_: swarm, session. ↔ core/src/team.rs

**Team record** [entity] — the file that holds the state of one [[Team]]: `team.json` in `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-tools/teams/<team id>/`, with the Leader handle, the next teammate number, the Launcher pid, the host and `server_id`, the cwd, the Claude binary, and on tmux the real tmux and its socket. _Avoid_: team state, team file. ↔ core/src/team.rs (`TeamRecord`)

**Leader** [entity] — the pane in which `claude-teams` runs, and the Claude process it starts. Inside a team it is the calling pane of every verb; its name is the name of its own [[Pane registration]], read at each `split-window`. _Avoid_: lead agent, main pane. ↔ core/src/team.rs (`TeamLeader`)

**Teammate pane** [entity] — a pane that the [[Shim]] creates for a [[Team]]: a normal [[Pane registration]] named `<leader name>.tm<k>` (fallback `team-<team id>.tm<k>`) with `team`, `parent` and `shim_id`. _Avoid_: worker pane, swarm pane. ↔ src/shim.rs

**Launcher** [service] — the `claude-teams` verb: it creates the [[Team]], starts the real `claude` as its child with the Shim first on the child's `PATH`, closes the Teammate panes on a clean exit, and fails open to in-process teammates when it cannot set up a Team. _Avoid_: wrapper, runner. ↔ src/cmd/claude_teams.rs

**Shim** [adapter] — the `tmux` executable in the team directory (`bin/tmux`, a link to `tmux-tools`) that serves Claude's tmux calls through the Host port, only for the panes of its own Team, and refuses every command and flag outside its surface. _Avoid_: fake tmux, tmux wrapper. ↔ src/shim.rs

**Shim id** [VO] — the `%N` pane id that Claude sees for a pane of its Team: `%1` for the Leader, `%<k+1>` for teammate `k`. _Avoid_: fake pane id. ↔ src/shim.rs, `@tt-shim-id`

**Parent** [VO] — the name of the tmux-tools pane from which a pane was created (`spawn-agent`, `launch`, or the Shim's `split-window`); `null` when the calling pane is not a tmux-tools pane. _Avoid_: creator, owner. ↔ src/cmd/self_.rs (`calling_pane_name`), `@tt-parent`

**Orphan** [VO] — a pane whose [[Parent]] is not live (or names a pane launched later), or a [[Teammate pane]] whose [[Team]] has no running [[Launcher]]. Derived by `list`, never stored; no verb closes a pane for being one. _Avoid_: stale pane, leaked pane. ↔ src/cmd/list.rs (`orphans`)

**Close-on-success wrap** [service] — the Teammate pane command wrap `exec sh -c '<command> || exec "${SHELL:-/bin/sh}" -l'`: exit 0 closes the pane, any other exit keeps a login shell in it so the output stays readable. The same on every host. _Avoid_: keep-alive wrap (that one keeps the pane on every exit). ↔ src/shim.rs (`close_on_success`)
