# Context: tmux-tools

The ubiquitous language for driving tmux panes from LLM agents. Single context; this file is
the whole glossary.

## Panes and sessions

**Managed session** [entity] — the `tmux-tools` tmux session this tool creates and reuses when invoked from outside tmux. _Avoid_: default session, our session. ↔ core/src/target.rs

**Target** [VO] — a resolved reference to one pane, parsed from a registered name, a pane id (`%3`), or a window id (`@7`). _Avoid_: selector, handle. ↔ core/src/target.rs

**Pane registration** [VO] — the `@tt-*` tmux pane user-options recording what this tool knows about a pane: `@tt-name`, `@tt-agent`, `@tt-access`, `@tt-launched-at`, `@tt-cwd`, `@tt-surface`, `@tt-surface-unvalidated`. `@tt-surface` records the surface `spawn-agent` resolved and is written unconditionally; `@tt-surface-unvalidated` is present when caller-supplied trailing arguments could have changed the rendering out from under that record. `@tt-name` follows `--name` today; it becomes unconditional on every pane either spawning verb creates, auto-derived from the command when the flag is absent (*planned — PRD-260801-0656-01 · ISSUE-260802-0628-01*). _Avoid_: metadata, tags. ↔ core/src/names.rs

**Keep-alive wrap** [service] — wrapping a launched command as `(cmd); exec $SHELL` so the pane outlives the command and its output stays readable. _Avoid_: shell wrapper, persistence. ↔ src/cmd/launch.rs

## Agents and readiness

**Agent profile** [aggregate] — one named registry entry: the binary to run, its [[Surface]] map, its access profiles, and its capabilities; plus its geometry floor (*planned — PRD-260801-0656-01 · ISSUE-260801-1310-05*). _Avoid_: agent config, agent definition. ↔ core/src/agents/mod.rs

**Agent registry** [repository] — the built-in profiles deep-merged with the user's `agents.toml`. _Avoid_: agent list, catalogue. ↔ core/src/agents/mod.rs

**Access profile** [VO] — a named argument set granting an agent a level of filesystem authority, e.g. `read-only`, `workspace-write`, `full-access`. _Avoid_: permission, sandbox mode. ↔ core/src/agents/mod.rs

**Surface** [VO] — one named **rendering** of an agent: the launch arguments that select it and every rendering-dependent behavior (readiness patterns, bracketed-paste capability, interrupt key and its quit hazard, resume syntax), plus the agent version those declarations were measured against. An agent carries a map of surfaces; `spawn-agent --surface <name>` selects one, and the resolved name is recorded in [[Pane registration]]. _Avoid_: mode, profile, variant. ↔ core/src/agents/mod.rs

**Default surface** [VO] — the surface `spawn-agent` resolves when no `--surface` is given, named by `default_surface`. Exactly one per agent after the `agents.toml` deep-merge. It is promotable: a flat surface becomes the default only once it ships a `ready_regex`/`busy_regex` pair carrying a [[Validated version]]; until then the agent keeps its [[Pre-surface rendering]] as the default. When an `agents.toml` explicitly names a `default_surface` that fails that bar, the fallback is reported through the registry load warning rather than applied silently. _Avoid_: primary surface, current surface. ↔ core/src/agents/mod.rs

**Validated version** [VO] — the agent-binary version the manual smoke measured a [[Surface]]'s declared patterns, bracketed-paste capability and quit hazard against, stamped beside those declarations. A flat surface is promoted to [[Default surface]] only when its `ready_regex`/`busy_regex` pair carries that stamp. _Avoid_: tested version, smoke stamp. ↔ core/src/agents/mod.rs

**Pre-surface rendering** [VO] — the surface matching the rendering an agent launched with before surfaces existed, named by `pre_surface_rendering`. Readiness fields written in the agent-level (pre-surface) `agents.toml` shape bind here, and total pane resolution falls back here when a pane carries no [[Surface]] record. It is a fixed fact, not the promotable [[Default surface]]. _Avoid_: legacy surface, original surface. ↔ core/src/agents/mod.rs

**Flat surface** [VO] — a non-alternate-screen rendering, so the pane keeps tmux scrollback. `claude --ax-screen-reader` and `dsh --profile dshline` are flat. A flat rendering with no native idle/busy chrome cannot produce a measured pattern pair, so it stays unpromoted and the agent keeps its [[Pre-surface rendering]] as the [[Default surface]] (`claude --ax-screen-reader` is the case). _Avoid_: line mode, plain surface. ↔ core/src/agents/mod.rs

**Rich surface** [VO] — a full-screen (alternate-screen) rendering. For `claude` and `dsh` it is the [[Pre-surface rendering]]; it has no tmux scrollback. _Avoid_: full surface, TUI surface. ↔ core/src/agents/mod.rs

**Surface-unvalidated pane** [entity] — a pane carrying a [[Surface]] record that caller-supplied trailing arguments may have invalidated, marked `@tt-surface-unvalidated`. Arguments the surface supplies itself are validated by construction and do not set the mark. _Avoid_: unchecked pane, unverified pane. ↔ core/src/names.rs

**Readiness** [service] — one value of [[Pane state]]: the classification that an agent has finished its turn, derived from the resolved [[Surface]]'s `ready_regex`/`busy_regex` by the single classifier. It is no longer the whole determination; when the classifier cannot decide — no patterns, no surface, or `unknown` — the wait falls back to [[Idle detection]], and a `busy` classification suppresses it. _Avoid_: done, completion, finished. ↔ core/src/idle.rs

**Pane state** [VO] — the total three-valued classification of a pane capture against a resolved [[Surface]]'s patterns: `idle`, `busy`, or `unknown`. `unknown` is distinct from `busy` and consumers needing certainty branch on it explicitly. Derived in exactly one place. _Avoid_: status, readiness value. ↔ core/src/idle.rs

**ready_regex** [VO] — the per-[[Surface]] pattern whose presence means idle-and-ready, matched against the pane's bottom non-blank lines. _Avoid_: prompt pattern, done marker. ↔ core/src/agents/mod.rs

**ready_lines** [VO] — how many bottom non-blank lines `ready_regex` is tested against; `0` means scan the whole visible pane. Per [[Surface]]. _Avoid_: scan depth, tail size. ↔ core/src/agents/mod.rs

**busy_regex** [VO] — the per-[[Surface]] pattern whose presence means the agent is generating. Matched against the whole capture, and never a synonym for `unknown`. _Avoid_: working marker, activity pattern. ↔ core/src/agents/mod.rs

**Idle detection** [service] — the fallback completion signal used when the classifier cannot decide (no patterns, no [[Surface]], or `unknown`): the visible pane capture unchanged for a configured duration. A `busy` [[Pane state]] suppresses it. It is not a parallel definition of idleness alongside [[Readiness]]. _Avoid_: quiescence, settling. ↔ core/src/idle.rs

**Exited-agent pane** [entity] — a pane whose agent process has exited but which the keep-alive wrap holds open as a shell; it still carries `@tt-agent` and still consumes window geometry. _Avoid_: zombie pane, dead pane, stale pane. ↔ src/cmd/launch.rs

## Pane I/O

**Bracketed paste transport** [service] — delivering one prompt as one submission through `tmux load-buffer` from stdin followed by `paste-buffer -p -d`, keyed to the pane's live `bracket_paste_flag` rather than a [[Surface]]'s declared capability. _Avoid_: paste mode, chunked send. ↔ src/cmd/prompt.rs *(planned — PRD-260918-0323-01 · ISSUE-260918-0525-03)*

**History mark** [VO] — the pane-history offset taken before a prompt is submitted, from which `prompt`'s result begins. Never advanced: the result always includes the agent's echo of the prompt. _Avoid_: anchor, watermark. ↔ src/cmd/prompt.rs *(planned — PRD-260918-0323-01 · ISSUE-260918-0525-06)*

**Turn observation** [VO] — whether `busy_regex` was observed between submission and settle. It reports that the agent took the turn and makes no claim about where content begins. _Avoid_: turn detection, completion. ↔ src/cmd/prompt.rs *(planned — PRD-260918-0323-01 · ISSUE-260918-0525-06)*

**Completeness** [VO] — whether everything after the [[History mark]] can be supplied: `false` when the pane has no scrollback (per live `alternate_on`) or history has evicted lines. Reported independently of [[Turn observation]]. _Avoid_: truncation, fidelity. ↔ src/cmd/prompt.rs *(planned — PRD-260918-0323-01 · ISSUE-260918-0525-06)*

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
