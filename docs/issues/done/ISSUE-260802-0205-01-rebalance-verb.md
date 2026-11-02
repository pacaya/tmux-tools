---
id: ISSUE-260802-0205-01
kind: issue
category: enhancement
status: done
brief_contract: agent-brief.v3
context: tmux-tools
summary: A rebalance verb so a crowded window can be retiled through the tool instead of raw tmux
terms: [Tile, Layout intent, Ownership, Effective floor, Advisory floor]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0128-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-05
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-06
---

## Agent Brief

**Outcome and scope:** A `rebalance` verb retiles one window through the tool, under the same
marker-and-ownership guard `kill` uses, and reports the window's measured geometry and floor verdict.
It wraps existing layout-module capability; it adds no placement, floor, relocation, or kill behavior.

**Required sources:** tmux-tools:PRD-260801-0656-01 — § Layout template, § Layout intent (marker
gates `rebalance`), § Ownership (the predicate), § Module shape (`rebalance` operation in core),
§ Floor: per surface, in the registry (effective floor), § Reporting surface (field names and
meanings), § Documentation (doc copies and the installed-copy release step), § Accepted limits
(ragged vs over-full), § How to run the suite (closing check).

**Current observable:** `tmux-tools rebalance --help` exits 2 (`unrecognized subcommand`).

**Required change:**
- New verb `rebalance` with the common target arguments (`--target`, `--session`, `--window`,
  `--format`). The resolved pane's containing window is the window acted on; inside tmux with no
  selector, that is the calling pane's window.
- Outside tmux (`TMUX` unset or empty, as target resolution judges it) with no `--target`,
  `--session` or `--window`, the verb refuses with a non-zero exit before target resolution runs, so
  it never creates the managed session.
- Guard: the shared window guard owned by ISSUE-260801-1310-06 in the core layout module (`@tt-tiled`
  present on the window, and at least one pane in the window owned per the shared ownership predicate
  for this invocation's cwd). Reuse it; do not build a second marker read or ownership scan. A
  window failing either half is refused, non-zero exit, geometry untouched, with a diagnostic that
  names the failed guard (layout intent / `@tt-tiled` vs ownership).
- On pass: retile through the layout module's `rebalance` operation (ISSUE-260802-0128-01), then
  report the window's geometry and verdict. The effective floor is resolved per surface from each
  pane's `@tt-agent`/`@tt-surface` (ISSUE-260801-1310-05).
- Output: `concise` and `json` carry `window_id`, `min_pane_cols`, `min_pane_rows`, `below_floor`,
  with the spellings and meanings the spawning verbs use; `raw` prints the window id only. Exit 0 on a
  completed rebalance, including when the window is still below its effective floor.
- Docs: the verb is documented in `README.md` § Verb Summary, the repo `SKILL.md`, and the installed
  copy `/Users/Shared/Data/work/Programming/ai/claude/user/skills/tmux-tools/SKILL.md`.

**Acceptance:** "Retiled" means the window's pane geometry equals what an explicit
`tmux select-layout tiled` of the same panes produces; "ragged" means it differs from that geometry.
- [ ] Given the built binary, when `tmux-tools rebalance --help` runs, then it exits 0, and
      `tmux-tools --help` lists `rebalance`, observable at the CLI.
- [ ] Given a marked, owned window on the private server made ragged by a raw `tmux split-window`,
      when `rebalance` targets it, then it exits 0 and the window is retiled, observable via
      `tmux list-panes`.
- [ ] Given that same scenario, the reported `window_id`, `min_pane_cols` and `min_pane_rows` (json)
      equal the window id and the smallest pane width and height that `tmux list-panes` reports after
      the call; `concise` carries the same four fields; `raw` stdout is exactly the window id.
- [ ] Given a ragged window without `@tt-tiled` that holds an owned pane, when `rebalance` targets
      it, then it exits non-zero and every pane's geometry is unchanged; in the same test, a ragged,
      marked, owned window is rebalanced and is retiled. Observable via exit code and `tmux list-panes`.
- [ ] Given a ragged, marked window in which no pane is owned for the invoking cwd, when `rebalance`
      targets it, then it exits non-zero and every pane's geometry is unchanged; in the same test, a
      ragged, marked, owned window is rebalanced and is retiled.
- [ ] Given the two refusals above, their stderr diagnostics differ, and each names its guard (layout
      intent / `@tt-tiled`, ownership).
- [ ] Given a marked, owned window holding a `tt-fake-tui` agent pane whose surface floor (fixture
      registry) its tiled pane cannot meet, when `rebalance` runs, then it exits 0 and reports
      `below_floor: true` (json).
- [ ] Given a roomy marked, owned window holding a `tt-fake-tui` agent pane whose surface floor its
      tiled pane meets, when `rebalance` runs, then it exits 0 and reports `below_floor: false` (json).
- [ ] Given the CLI runs outside tmux with no `--target`, `--session` or `--window`, once with `TMUX`
      unset and once with `TMUX` set to the empty string, when `rebalance` runs, then each run exits
      non-zero and no tmux session exists afterwards that did not
      exist before (in particular no `tmux-tools` session), observable via `tmux list-sessions` on the
      private server; in the same test, outside tmux, `rebalance --session <S>` on a marked, owned,
      ragged window exits 0 and the window is retiled.
- [ ] Given this change, `sed -n '/^## Verb Summary/,/^## [^V]/p' README.md | grep -c '`rebalance`'`,
      `grep -c rebalance SKILL.md`, and
      `git -C /Users/Shared/Data/work/Programming/ai/claude show agent/auto-tile-layout:user/skills/tmux-tools/SKILL.md | grep -c rebalance`
      each print a count of at least 1 (each prints 0 or fails before this change).

**Verification and release:** All runtime criteria run at the CLI seam on the private tmux harness
(PRD-260801-0656-01 § Sequencing), inside-tmux mode unless stated. Agent-pane criteria use the
`AgentsConfig` registry injected through `XDG_CONFIG_HOME` with `tt-fake-tui`, under the
`test-fixtures` feature; they need no installed vendor agent and fail, never skip, when the behavior
is absent. `cargo test --workspace --features test-fixtures` passes against an isolated tmux server
(PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change
did not introduce is reported, not fixed. Release step: the installed `SKILL.md` edit is committed on
branch `agent/auto-tile-layout` in the claude-settings repository
(`/Users/Shared/Data/work/Programming/ai/claude`) and pushed to its lane; this issue closes on that
branch commit. The control Mac lands that branch once, after the whole auto-tile epic's binary is
installed. Do not modify that repository's live working tree before then.

**Out of scope:**
- Moving panes out of an over-full window, or killing exited-agent panes to reclaim geometry.
- A layout template argument; the verb applies the epic's one template.
- Setting or clearing `@tt-tiled`.
- An override flag past either guard.
- Rebalancing more than one window per invocation.
- Deferring the retile for in-flight `prompt` calls; width-change incompleteness is owned by
  ISSUE-261102-0012-01.
- Any change to placement, floors, relocation, or kill-time reclamation.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied; second targeted repair (live working tree rule).
