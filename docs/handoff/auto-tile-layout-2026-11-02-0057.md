# Handoff: auto-tile epic — 01–05 + 0628 done; timeout fix done on its own branch

> Prepared 2026-11-02 00:57 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> Supersedes `docs/handoff/auto-tile-layout-2026-11-01-2002.md` on **Current state**, **Open questions** (now Q27–Q28; last number used Q28) and **Next actions**. Still standing, not repeated here: the **Dead ends**, **Conventions and gotchas** and **Key decisions** sections of `docs/handoff/auto-tile-layout-2026-11-01-1957.md` (planning CLI commands, receipt rules, order/branch/landing). Its "test safety" gotcha is now obsolete on the epic branch (the suite self-isolates since 01), but it still applies on branches cut from `main`.

## TL;DR
Epic PRD-260801-0656-01 is being implemented on `agent/auto-tile-layout`. Done with receipts: 01, 02, 0628, 03, 04, 05. A standalone bug ISSUE-261102-0142-01 (timeout race) was found, minted, implemented and receipted on its own branch `agent/timeout-bounds-grandchild-pipes` (cut from `main`). Two questions to the user are open (Q27, Q28). Ask them first. Do not start readiness or implementation without the user's go-ahead.

## Current state
- **Epic branch** `agent/auto-tile-layout`: HEAD c95d6bb, clean tree, pushed to the lane as `agent/wip/auto-tile-layout` (same sha).
  - done + receipt: ISSUE-260801-1310-01 (b4d70cd / 0d9228e), -02 (611964c / 8fdda96), ISSUE-260802-0628-01 (cca545a / 309f418), -03 (530057b / 80a0117), -04 (1e8b398 / a3e0041), -05 (cec2ed9 / c95d6bb).
  - The last combined suite on c95d6bb ran with a decoy env: 66 + 142 + 114 passed, 0 failed, no leaks.
  - pending (`needs-triage`): ISSUE-261102-0012-01, ISSUE-260802-0128-01, ISSUE-260801-1310-06, -07, ISSUE-260802-0205-01. Remaining order: 0012, 0128, 06, 07, 0205.
  - The epic branch also holds the 0142 record commits (a5eadc1…e498db6), with the issue file at `ready-for-agent`. They have the same content as on the 0142 branch, so landing both later gives no conflict, and the 0142 merge moves the file to `done/`. Decision: do not rewrite pushed history to remove them.
- **0142 branch** `agent/timeout-bounds-grandchild-pipes`: 7 record commits on `main` 911a05d, fix 298f6f1, receipt 66f4226. Pushed to the lane as both `agent/timeout-bounds-grandchild-pipes` (finished) and `agent/wip/…`. Worktree `/Users/Shared/Data/work/Programming/tools/tmux-tools-timeout-0142` is kept until the branch lands.
  - Not in scope and not done: some early error returns after spawn (failure to take a pipe handle, a `try_wait` error) can still leave the child running. I told the user; they gave no answer.
- **claude-settings repo**: worktree `/Users/Shared/Data/work/Programming/ai/claude-auto-tile-layout`, branch `agent/auto-tile-layout` (cut from `main` bad8a1e), HEAD 9ab04cb, which is the 0628 edit to the installed SKILL.md. It is pushed. Keep it: 0128 also edits the installed copy there. Never edit the live tree `/Users/Shared/Data/work/Programming/ai/claude` (it is on `agent/lane-redesign` and holds unrelated dirty files).
- No worker worktrees, subagents or test servers are left running.

## Key decisions and rationale (maintainer-approved this segment)
- **Q14:** receipt 01 even though `command_output_with_timeout_kills_blocked_process` failed. *Why:* a pre-existing race in code unchanged since `main` (`git log main..HEAD -- core/src/tmux.rs` was empty); it is tracked as its own bug.
- **Q15 (a):** record the race as a draft issue brief (ISSUE-261102-0142-01), not only as a handoff note.
- **Q18–Q20:** for the 0142 timeout: `Timeout` no later than 500 ms after the deadline; the command runs in its own process group on every call and the whole group is killed on timeout; the deadline covers the whole call, including a descendant that holds the pipes after the child exits. These are written in the brief.
- **Q23 (a):** when `agents.toml` cannot be read or parsed, `launch` and `status` print a stderr warning and continue with the built-in registry; `spawn-agent` and `wait-idle` keep failing. *Why:* a broken config file should not break a plain `launch` or the `status` check used to diagnose problems. This is written in the 05 brief.
- **Q24 (a):** 0142 is implemented on its own branch from `main`, with its records. *Why:* the lane rule is one branch per standalone issue, so it can land without waiting for the epic.

## Dead ends / lessons this segment
- **zsh in Bash calls:** `echo =====` fails ("= not found"), and an unmatched glob such as `ls -d /tmp/tt-tests-*` makes the command exit 1 ("no matches found"). Use `echo "---"` and `ls /tmp | grep '^tt-tests-'`. `grep --include=*.rs` also fails as a glob; quote it.
- **`git worktree remove` on an `isolation: worktree` worker worktree** can fail with "locked working tree … claude agent …". The lock pid is this lead session. Once the worker has finished and the tree is clean, run `git worktree unlock <path>`, then remove it.
- **Two parallel workers** both added fields to `CliOptions` and the same helper `all_pane_ids` in `tests/integration.rs`. Merge: keep both fields and delete the duplicate helper. Expect this pattern again.
- **Readiness repairs that only reword text can create a new blocker.** In 05, "inside tmux" made the rule too narrow (`status --target` with `TMUX` unset). Write load/format rules around "renders a pane in concise/json" and not around the tmux environment.
- **macOS zombies:** a killed, unreaped descendant stays visible to `kill -0`. Acceptance checks must accept "exited or zombie" within a bound.

## Conventions (new this segment)
- **Closing check on the epic branch:** `D=$(mktemp -d); TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=$D cargo test --workspace --features test-fixtures --no-fail-fast; ls -A $D` (must be empty). Then check `ls /tmp | grep '^tt-tests-'` (must be empty). Use `--no-fail-fast`; without it, one failing binary hides the rest.
- **On branches cut from `main`** (no harness yet), run the suite against a private server and pass its socket triple through `TMUX`/`TMUX_PANE`. See the 0142 worker prompt pattern: `tmux -L tt-impl-<x>-$$ -f /dev/null new-session -d -s base -x 280 -y 82`, then kill the server afterwards.
- **Worker pattern:** `worker-high`, opus, `isolation: worktree`, one commit per issue that also `git mv`s the record to `docs/issues/done/` with `status: done`. The lead cherry-picks it, runs the closing check, runs `receipt-create` on the cherry-picked sha, commits, pushes `agent/wip/auto-tile-layout`, then removes the worktree and its branch.
- **Readiness pattern:** one `cold-reader` (opus, effort xhigh) per issue, in background. Text-only findings: fix them yourself, then `SendMessage` the same reader for a targeted recheck. A second repair is allowed only when the first repair introduced the blocker. Findings marked `OWNER: maintainer` go to the user as a numbered question with a recommendation.
- **Known flaky tests:** `launch_keeps_pane_alive_after_cmd_exit` and `full_smoke`. The timeout test is flaky on the epic branch until the 0142 branch lands.

## User preferences
- Terse replies by number ("agree with both"). They approved every recommendation in this segment.
- **Q16 and Q21 showed the pattern:** do not start implementation without an explicit "yes". Readiness is also run only when asked.

## Open questions
- **Q27 (SKILL.md for floors):** no record assigns documentation of floors/`below_floor` to the two SKILL.md copies; PRD § Documentation covers only the tile default, which 0128 owns. The recommendation is to add one point to the 0128 brief ("`launch`/`spawn-agent`/`status` report `below_floor`; floors are set per surface in `agents.toml`"). This changes 0128's scope, so it needs approval. The alternative is to leave SKILL.md without floor information.
- **Q28 (next step):** run readiness for ISSUE-261102-0012-01 (prompt incomplete on width change)?

## Next actions
1. Repeat Q27 and Q28 to the user and wait for answers.
2. If Q27 is approved, add the point to `docs/issues/ISSUE-260802-0128-01-tile-default-and-layout-intent.md` (Required change § Documentation, and the reviewer read-check acceptance item). It is still `needs-triage`, so the change is checked at its readiness.
3. If Q28 is yes, spawn a `cold-reader` for readiness of ISSUE-261102-0012-01. Read its `contract.references` first; give the reader the delivered-prerequisite facts (shas above). On PASS: set `status: ready-for-agent`, commit, then `dispatch-create` + `bundle-verify`, commit, push.
4. Push `agent/wip/auto-tile-layout` at least hourly while work is in flight.
5. After the 0142 branch lands upstream, remove its worktree: `git worktree remove /Users/Shared/Data/work/Programming/tools/tmux-tools-timeout-0142` and `git branch -D agent/timeout-bounds-grandchild-pipes`.
