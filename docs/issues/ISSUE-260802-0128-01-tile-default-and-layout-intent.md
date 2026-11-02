---
id: ISSUE-260802-0128-01
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: Make tile the default placement inside tmux — ownership-gated retile, the @tt-tiled layout-intent marker, sizing opt-outs, and the documentation change
terms: [Tile, Layout intent, Ownership]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-02
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260802-0628-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-03
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-04
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261102-0012-01
---

## Agent Brief

**Outcome and scope:** Inside tmux, `launch` and `spawn-agent` place panes in a new `tile` mode by default: split, then retile the window with tmux's `tiled` layout when the window was owned before the split, then measure. The tool records the windows it tiles with the `@tt-tiled` window marker, keeps every explicit sizing invocation as it is today, and rewrites the help text, README.md, the repo SKILL.md, the installed SKILL.md copy, and the layout-resolution doc comment to state the new default. Floors, relocation, kill-time retiling, the `rebalance` verb, `status` geometry, and any `prompt` change are excluded.

**Required sources:** `tmux-tools:PRD-260801-0656-01` — § Solution (tiling engages from the second pane), § Layout template, § Ownership (snapshot before split), § Layout intent: the `@tt-tiled` window marker, § Flags, § Module shape, § Reporting surface, § Documentation (installed copy path and release step), § Accepted limits (retile vs in-flight `prompt`), § How to run the suite.

**Current observable:**
- Inside tmux, a flagless `launch` or `spawn-agent` splits the calling pane horizontally and gives the new pane 70% of the width. Repeated placements shrink the caller until tmux refuses with `no space for a new pane`. No placement retiles a window.
- `--split tile` is rejected by clap as an invalid value. No window ever carries `@tt-tiled`.
- `launch --help` and `spawn-agent --help` show `--split <h|v>` and say the implicit default inside tmux "is now a horizontal split".
- README.md and SKILL.md state the 30:70 horizontal split as the flagless default. The installed copy says "70% horizontal split". The doc comment on the flags-to-layout resolution says the new pane is "occupying 70%" of the width.

**Required change:**
- **`tile` value.** The split-direction enum gains `tile`. It is the implicit default inside tmux for both verbs. The flags-to-layout resolution rejects `--split tile --size N` after parsing, before any tmux call, with a diagnostic that names `--split` and `--size`. `--size` stays legal with every other `--split` value.
- **Ownership snapshot.** In tile mode, `place` reads the target window's panes and evaluates the shared core ownership predicate (ISSUE-260801-1310-02) before it splits, and decides eligibility from that snapshot only. Do not restate the predicate.
- **Eligible window.** If the snapshot contains an owned pane: split, register, retile the window with `tiled`, set `@tt-tiled` on the window, then measure.
- **Not eligible.** If the snapshot contains no owned pane: place the pane exactly as today (horizontal split, new pane 70%), with no retile and no marker change.
- **Marker.** `@tt-tiled` is a window option, never a pane option. It is set when the tool tiles a window. It is cleared by unsetting it (never by writing a false-ish value) when a placement with explicit sizing intent targets that window. Placement is gated by ownership only, never by the marker: a later tile placement into an owned window whose marker was cleared tiles it again and sets the marker again.
- **Sizing opt-outs.** `--split h`, `--split v` (with or without `--size`), and `--size N` alone (horizontal split at N%) are placed exactly as today, with no retile. They clear `@tt-tiled` on the targeted window. No invocation that works today is newly rejected.
- **Fresh-window destinations unchanged.** The outside-tmux path and a flagless `--session`/`--window` without `--target` create a new window as today. An explicit `--split tile` with `--session`/`--window` and no `--target` splits into the scoped window, as explicit `h|v` does, and then applies the ownership-gated retile. In every path, including outside tmux, an explicit `--split tile` chooses its destination exactly as an explicit `h|v` does; only the layout step after the split differs.
- **Retile and in-flight prompts.** A retile is never deferred or suppressed for a `prompt` in flight on a pane in the window. ISSUE-261102-0012-01 owns the incomplete report for that case.
- **Layout module.** `place` gains the retile stage. The layout module gains a `rebalance` operation that retiles a window and returns its measured geometry. ISSUE-260801-1310-06 and ISSUE-260802-0205-01 consume it.
- **Output.** Every placement, tiled or opted out, prints `window_id`, `min_pane_cols`, and `min_pane_rows` in `concise` and `json`, measured after the retile. `raw` output is unchanged: the pane id only.
- **Help text.** `launch --help` and `spawn-agent --help` list `tile` among the `--split` values. They no longer say that a horizontal split is the implicit default inside tmux.
- **Documentation.** README.md, the repo SKILL.md, and the installed copy at `/Users/Shared/Data/work/Programming/ai/claude/user/skills/tmux-tools/SKILL.md` state plainly that the default changed. They describe the tiled default, the first-pane rule, the sizing opt-outs, and the rejection of `--split tile --size N`. The doc comment on the flags-to-layout resolution (in `src/` or `core/src/` after ISSUE-260801-1310-04) states the tiled default.
- **Installed copy release step.** Commit the installed-copy edit on branch `agent/auto-tile-layout` in the claude-settings repository `/Users/Shared/Data/work/Programming/ai/claude`. Do not change that repository's live working tree before the merge, because `~/.claude/skills` points into it. Merge the branch only after the tiled binary is installed.

**Acceptance:** Runtime criteria run on the private harness in inside-tmux mode (280×82). `spawn-agent` criteria use the `AgentsConfig` registry with `tt-fake-tui`. Read the marker with `tmux show-options -w -t <window>`: "present" means the listing contains `@tt-tiled`.
- [ ] Given a window that holds only the caller pane, when two flagless `spawn-agent <fake agent>` placements run, then the window holds three panes. Their heights from `tmux list-panes -F '#{pane_height}'` are within one row of each other, and no pane spans the full window height. The second placement's `json` `min_pane_cols`/`min_pane_rows` equal the componentwise minimum that tmux reports for the window. Fails before this change, because all three panes span the full height.
- [ ] Given the same start, when two `launch --cmd <shell>` placements run with no `--name`, no agent, and no layout flags, then the same height result holds. Keep this as a separate test from the criterion above.
- [ ] Given a window that holds only the caller pane (not owned), when one flagless `launch` runs, then the new pane has 70% of the window width, both panes span the full window height, and `@tt-tiled` is absent. Passes before and after this change.
- [ ] Given a window whose panes are registered with a different `@tt-cwd`, when a flagless `launch --target <pane in it>` runs, then the window's other panes keep their geometry and `@tt-tiled` is absent. In the same test, a flagless placement into an owned window retiles that window and sets the marker.
- [ ] Given an owned window, when a flagless placement tiles it, then `@tt-tiled` is present. When a `--split v` placement then targets the window, `@tt-tiled` is absent. When a flagless placement then targets it again, the window is retiled and `@tt-tiled` is present again. The test asserts the marker present before it asserts the marker absent.
- [ ] Given a window with `@tt-tiled` present (asserted first), when a placement with `--split h`, `--split v`, or `--size 40` alone targets one of its panes, then only the target pane's rectangle is divided in that direction (horizontal for `--size` alone, new pane 40% for `--size 40`), all other panes keep their geometry, and `@tt-tiled` is absent afterwards. Test each flag form.
- [ ] Given inside tmux, when `launch --cmd <shell> --split tile` runs into an owned window, then it exits 0 and the window is retiled and marked. Fails before this change, because clap rejects `tile`.
- [ ] Given inside tmux, when `launch --cmd <shell> --split tile --size 50` runs, then it exits non-zero, stderr names both `--split` and `--size`, and the server's pane count does not change.
- [ ] Given session S on the private server, when a flagless `launch --session S --cmd <shell>` runs, then S gains one window W that holds a single pane. When `launch --session S --window <W> --split tile --cmd <shell>` then runs, W holds two panes, S's window count does not change, and `@tt-tiled` is present on W. Fails before this change.
- [ ] Given a tiled placement, when it runs with `--format raw`, then stdout is exactly the pane id followed by a newline.
- [ ] `launch --help` and `spawn-agent --help` each match `rg -w tile` after this change and not before. Each matches `rg 'now a horizontal split'` before this change and not after.
- [ ] `rg -n -i 'tiled' README.md SKILL.md` matches each file. The same search over the installed copy as committed on the branch (`git -C /Users/Shared/Data/work/Programming/ai/claude show agent/auto-tile-layout:user/skills/tmux-tools/SKILL.md`) matches. None of the three copies match before this change.
- [ ] No copy still presents the 70% split as the flagless default. `rg -n 'default is a horizontal split|30:70' README.md SKILL.md` matches each file before this change and neither file after. `rg -n '70% horizontal split'` over the installed copy matches before this change and does not match the branch version after.
- [ ] `rg -n 'occupying 70' src/ core/src/` matches before this change and not after.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change did not introduce is reported, not fixed. "The private harness" is the inside-tmux mode of `IsolatedTmuxServer` from ISSUE-260801-1310-01. A `spawn-agent` criterion must fail, never skip, when the behavior is absent, and must not need an installed vendor agent. Release action: the installed-copy commit on `agent/auto-tile-layout` in `/Users/Shared/Data/work/Programming/ai/claude` is merged only after the tiled binary is installed.

**Out of scope:** Per-surface floors, effective floors, and `below_floor` (ISSUE-260801-1310-05). Relocation to another window (ISSUE-260801-1310-07). Retiling on `kill` and the marker-and-ownership window guard (ISSUE-260801-1310-06). The `rebalance` verb (ISSUE-260802-0205-01). Geometry on `status`. Any `prompt` change (ISSUE-261102-0012-01). The `--split window` fix (ISSUE-260801-1310-03). Predicting geometry from a model of tmux's tiling. Reconciling the two divergent skill-documentation copies.

## Triage Notes

- Scope: the documentation edits stay in this slice (maintainer decision, round-1 readiness gate).
- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
