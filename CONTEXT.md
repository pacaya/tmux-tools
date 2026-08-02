# Context: tmux-tools

The ubiquitous language for driving tmux panes from LLM agents. Single context; this file is
the whole glossary.

## Panes and sessions

**Managed session** [entity] — the `tmux-tools` tmux session this tool creates and reuses when invoked from outside tmux. _Avoid_: default session, our session. ↔ core/src/target.rs

**Target** [VO] — a resolved reference to one pane, parsed from a registered name, a pane id (`%3`), or a window id (`@7`). _Avoid_: selector, handle. ↔ core/src/target.rs

**Pane registration** [VO] — the `@tt-*` tmux pane user-options recording what this tool knows about a pane: `@tt-name`, `@tt-agent`, `@tt-access`, `@tt-launched-at`, `@tt-cwd`. `@tt-name` follows `--name` today; it becomes unconditional on every pane either spawning verb creates, auto-derived from the command when the flag is absent (*planned — PRD-260801-0656-01 · ISSUE-260802-0628-01*). _Avoid_: metadata, tags. ↔ core/src/names.rs

**Keep-alive wrap** [service] — wrapping a launched command as `(cmd); exec $SHELL` so the pane outlives the command and its output stays readable. _Avoid_: shell wrapper, persistence. ↔ src/cmd/launch.rs

## Agents and readiness

**Agent profile** [aggregate] — one named registry entry: the binary to run, its access profiles, and its readiness rules; plus its geometry floor (*planned — PRD-260801-0656-01 · ISSUE-260801-1310-05*). _Avoid_: agent config, agent definition. ↔ core/src/agents/mod.rs

**Agent registry** [repository] — the built-in profiles deep-merged with the user's `agents.toml`. _Avoid_: agent list, catalogue. ↔ core/src/agents/mod.rs

**Access profile** [VO] — a named argument set granting an agent a level of filesystem authority, e.g. `read-only`, `workspace-write`, `full-access`. _Avoid_: permission, sandbox mode. ↔ core/src/agents/mod.rs

**Readiness** [service] — the determination that an agent has finished its turn, made by matching its `ready_regex` against the pane. _Avoid_: done, completion, finished. ↔ core/src/idle.rs

**ready_regex** [VO] — the per-agent pattern whose presence means idle-and-ready, matched against the pane's bottom non-blank lines. _Avoid_: prompt pattern, done marker. ↔ core/src/agents/mod.rs

**ready_lines** [VO] — how many bottom non-blank lines `ready_regex` is tested against; `0` means scan the whole visible pane. _Avoid_: scan depth, tail size. ↔ core/src/idle.rs

**Idle detection** [service] — the fallback completion signal: the visible pane capture unchanged for a configured duration. _Avoid_: quiescence, settling. ↔ core/src/idle.rs

**Exited-agent pane** [entity] — a pane whose agent process has exited but which the keep-alive wrap holds open as a shell; it still carries `@tt-agent` and still consumes window geometry. _Avoid_: zombie pane, dead pane, stale pane. ↔ src/cmd/launch.rs

## Layout and geometry

**Tile** [VO] — the placement mode that applies tmux's `tiled` layout to a window after each spawn; the implicit default inside tmux. _Avoid_: grid, auto-layout. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

**Floor** [VO] — the minimum pane geometry an agent needs for its readiness signal to remain matchable, declared per agent as `min_cols`/`min_rows`. _Avoid_: minimum size, threshold, limit. ↔ core/src/agents/mod.rs *(planned — PRD-260801-0656-01)*

**Effective floor** [VO] — a window's floor: the componentwise maximum over the floors contributed by the panes in it; panes with no agent contribute none but are still measured. _Avoid_: window floor, combined minimum. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

**Ceiling** [VO] — the pane count at which a window's effective floor stops being satisfiable; derived from live window size, never stored. _Avoid_: max panes, pane cap, limit. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

**Advisory floor** [VO] — the property that a floor binds only panes this tool places: direct `tmux split-window` calls bypass it and concurrent spawns can overshoot it. _Avoid_: soft limit, best-effort floor. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

**Overflow window** [entity] — the window a pane is moved to when placing it would push its origin below the effective floor. _Avoid_: spillover window, secondary window. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

**Layout intent** [VO] — whether a window is tool-tiled or human-arranged, recorded as the `@tt-tiled` window option (unset when cleared, so "cleared" and "never set" are one state). A record of what the tool did, not a permission to act: it gates teardown (`kill` retiling) and overflow-destination eligibility, never placement — which is gated by [[Ownership]] alone, since the first tiling placement into any window necessarily finds no marker and is what sets one. _Avoid_: tiled flag, managed layout. ↔ core/src/layout.rs *(planned — PRD-260801-0656-01)*

## Ownership

**Ownership** [service] — the predicate deciding whether a pane is this tool's: `@tt-name` or `@tt-agent` present, and where both the pane's `@tt-cwd` and the current working directory are known, they match; absence on either side passes. The rule is stable; what changes is the evidence it reads — once every created pane registers a name ([[Pane registration]]), the first clause holds for all of them, including a `launch` with no flags, which today it does not. _Avoid_: managed, ours, created-by. ↔ src/cmd/safety.rs (inline in the safety evaluator today; moving to a shared public predicate in core, with the evaluator delegating to it — *planned, PRD-260801-0656-01 · ISSUE-260801-1310-02*)

**@tt-cwd** [VO] — the working directory of the invocation that *created* a pane — run identity, not where the agent works. _Avoid_: agent cwd, working directory. ↔ core/src/names.rs
