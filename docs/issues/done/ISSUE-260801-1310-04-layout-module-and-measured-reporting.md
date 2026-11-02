---
id: ISSUE-260801-1310-04
kind: issue
category: enhancement
status: done
brief_contract: agent-brief.v3
context: tmux-tools
summary: Core layout module owning placement, and measured destination geometry in the spawning verbs' output
terms: [Pane registration]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: requires
      repository: tmux-tools
      record: ADR-260919-0146-01
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p2-surface
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p7-prompt
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0628-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-03
---

## Agent Brief

**Outcome and scope:** A `layout` module in the core crate becomes the single owner of placement for `launch` and `spawn-agent`. Its placement operation `place` does split → register → measure and returns a landing report. Both verbs print the measured destination geometry in `concise` and `json`. Where panes land does not change: a flagless placement inside tmux is still the horizontal split that gives the new pane 70%. The tile default, ownership gating, `@tt-tiled`, sizing opt-outs, floors and documentation are excluded (see Out of scope).

**Required sources:**
- `tmux-tools:PRD-260801-0656-01`: § Module shape (layout module, `place`, type move), § Ownership (registration before measurement; agent and resolved surface are inputs to `place`), § Persistent shape (registration field sets, auto-derived `@tt-name`), § Reporting surface (field names and meaning), § How to run the suite.
- `tmux-tools:ADR-260919-0146-01`: the surface record in pane registration.
- `tmux-tools:PRD-260918-0323-01#p2-surface`: `@tt-surface` written unconditionally; the surface-unvalidated mark.
- `tmux-tools:PRD-260918-0323-01#p7-prompt`: `history-limit` on sessions that tmux-tools creates.

**Current observable:** `launch` and `spawn-agent` output only the pane registration: `raw` is the pane id, and `concise`/`json` show name and pane fields. Neither verb reports the window the pane landed in or that window's geometry. `Split`, `Layout` and `resolve_layout` are in `src/cmd/launch.rs`.

**Required change:**
- **Module and type move.** Add `pub mod layout;` to `core/src/lib.rs`. Move the split-direction enum `Split`, the resolved-layout type `Layout` and the flags-to-layout resolution into it. This is a move: the binary crate keeps no second definition. Both verbs get placement only through this module. The later stages (retile, `rebalance`, `floor_violations`, overflow) do not need stubs in this slice.
- **`place` inputs.** `place` receives the target and the full registration to write. It does not read registration back from the pane. The registration includes the pane's agent and resolved surface, which are absent for `launch`.
- **Registration inside `place`.** `place` writes the whole registration after the split and before it reads geometry. It never writes an unset field, not even as an empty value. The written sets must equal the sets each verb writes today:
  - `launch`: `@tt-name`, `@tt-launched-at`, and `@tt-cwd` (the invoking process's cwd, written only when readable).
  - `spawn-agent`: the `launch` set, plus `@tt-agent` and `@tt-surface` (always), `@tt-access` (only with `--access`), and `@tt-surface-unvalidated` (only when trailing caller arguments are present). See `src/cmd/spawn_agent.rs`.
- **Auto-name.** Move the `@tt-name` derivation and collision handling from ISSUE-260802-0628-01 into `place` without change. The landing report includes the name actually written, and both verbs print that name.
- **Destinations.** `place` covers every destination path that `launch_pane` covers today:
  - a split inside tmux, from the calling pane or from `--target`, or into a `--session`/`--window` scope with an explicit `--split h|v`;
  - a new window for `--split window` or for a `--session`/`--window` target;
  - the managed `tmux-tools` session outside tmux.
- **Binary-crate dependencies.** `place` depends on two helpers in the binary crate. Resolve each one in one of two ways:
  - The timestamp helper `rfc3339_utc_now` (`src/util.rs`): move it to core, or let the caller compute the timestamp and pass it in the registration.
  - Managed-session creation `ensure_managed_session_atomic` (`src/cmd/launch.rs`): move it to core with the split, or keep it with the caller.
  The `history-limit` behavior of `PRD-260918-0323-01#p7-prompt` does not change.
- **Landing report and output.** The landing report contains the destination window's stable id and the smallest pane width and height measured across every pane in that window. In `concise` and `json`, both verbs add these values. The JSON field names are exactly `window_id`, `min_pane_cols` and `min_pane_rows`. The implementer chooses the `concise` key spelling and order, with two limits: the minima must not be spelled `min_cols`/`min_rows`, and the window field must not be spelled `window`. `raw` output does not change.

**Acceptance:**
- [ ] Given this change, `rg -n '^pub mod layout;' core/src/lib.rs` returns one match, and `rg -n 'enum Split\b|enum Layout\b' src/` returns no matches. Both checks give the opposite result before this change.
- [ ] Given this change, `src/` contains no definition of `Split` or `Layout` (any item kind) and no flags-to-layout resolution. A reviewer check by reading code confirms that `launch` and `spawn-agent` create panes only through `layout::place`, with no split or new-window placement call left in `src/`, for every path listed under Destinations. Fails while placement stays in the binary crate.
- [ ] Given the private harness in inside-tmux mode, when `launch --cmd <shell>` runs with no layout flags, then `tmux list-panes -t <window> -F '#{pane_left} #{pane_top} #{pane_width} #{pane_height}'` shows a horizontal split: the new pane has 70% of the window width, and both panes span the full window height. This is a preservation check: it passes before and after this change.
- [ ] Given the private harness, when `launch --format json` runs, and when `spawn-agent <fake agent> --format json` runs, then each JSON object contains `window_id`, `min_pane_cols` and `min_pane_rows`, observable at the CLI seam. `window_id` equals `tmux display -p -t <pane_id> '#{window_id}'`. The two minima equal the componentwise minimum of `tmux list-panes -t <window_id> -F '#{pane_width} #{pane_height}'`. Fails before this change because the fields are absent.
- [ ] Given `launch --format concise` and `spawn-agent <fake agent> --format concise`, then each invocation's stdout contains the window id that `tmux display -p -t <that invocation's pane_id> '#{window_id}'` reports, and the componentwise minimum of `tmux list-panes -t <that window_id> -F '#{pane_width} #{pane_height}'`.
- [ ] Given `launch` and `spawn-agent` with `--format raw`, then stdout is exactly the pane id followed by a newline.
- [ ] Given the private harness, read registration with `tmux show-options -p -t <pane_id>`. The set of `@tt-*` keys for each pane is exactly as follows, and no key that is absent is present with an empty value:
  - flagless `launch`: `@tt-name` (non-empty), `@tt-launched-at`, `@tt-cwd`;
  - `spawn-agent` with no `--access` and no trailing arguments: those keys plus `@tt-agent` and `@tt-surface`; `@tt-access` and `@tt-surface-unvalidated` are both absent;
  - `spawn-agent --access <profile> <agent> -- <arg>`: those keys plus `@tt-access` and `@tt-surface-unvalidated`, both present.
- [ ] Given a flagless `launch` and a flagless `spawn-agent`, then the name in `concise` and `json` output equals the `@tt-name` value that tmux reports for the created pane.
- [ ] Given the harness in outside-tmux mode, when `launch --format json` runs, then `tmux display -p -t <window_id> '#{session_name}'` returns `tmux-tools`. Given `launch --session <s> --format json` with no `--target`, then the reported window is a new window in session `<s>`.
- [ ] The existing `history-limit` integration tests (`rg -n '^fn \w*(history_limit|created_session|managed_session|pending_session)\w*' tests/integration.rs`) and the acceptance tests of ISSUE-260802-0628-01 pass without modification.
- [ ] Every `spawn-agent` criterion above runs without an installed vendor agent. It uses the `AgentsConfig` registry, injected through `XDG_CONFIG_HOME` with `tt-fake-tui` (`tests/integration.rs`), under the `test-fixtures` feature. It fails, and never skips, when the behavior is absent.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). If a failure survives isolation and this change did not introduce it, report it. Do not fix it.

**Out of scope:**
- The tile placement mode, the default change, the ownership snapshot, tiling from the second pane, `@tt-tiled`, the sizing opt-outs, and all documentation changes. These belong to ISSUE-260802-0128-01.
- Floors, effective-floor resolution, `below_floor`, `relocated` and relocation. These belong to ISSUE-260801-1310-05 and ISSUE-260801-1310-07.
- Retiling on `kill`, the `rebalance` verb, and geometry on `status`.
- Predicting geometry from a model of tmux's tiling algorithm.

## Triage Notes

- Split from the original record; tile default, ownership gating, @tt-tiled marker and docs → ISSUE-260802-0128-01.
- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied.
