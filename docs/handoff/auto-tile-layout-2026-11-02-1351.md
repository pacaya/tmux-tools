# Handoff: bug 0802 ready-for-agent; Q52 (implement it?) open

> Prepared 2026-11-02 13:51 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> Supersedes `docs/handoff/auto-tile-layout-2026-11-02-0806.md` only on the 0802 status, **Open questions** and **Next actions**. Everything else in it still stands: epic state, the decisions Q43–Q50, dead ends, conventions, the 0142 cleanup, and the claude-settings branch. Read it first.

## TL;DR
The auto-tile epic is complete and published. `agent/auto-tile-layout` is at 3d092c2 on the lane, waiting for the user to merge it into `development` on the control Mac, and `tmux-tools 0.3.0` is installed. The standalone test-only bug ISSUE-261102-0802-01 passed readiness (Q51 yes) and is `ready-for-agent` with its dispatch bundle. Q52 is open: implement 0802 now? Ask it first.

## Current state
- **ISSUE-261102-0802-01** (`docs/issues/ISSUE-261102-0802-01-agents-temp-file-path-collision.md`): `ready-for-agent` (5c96e6c), bundle at 1d283e0. The readiness reader returned a clean `PASS — readiness`, SCALE PASS, with no findings.
- The branch `agent/agents-temp-file-collision` is checked out at 1d283e0 and pushed as lane `agent/wip/agents-temp-file-collision`. Clean tree. It holds only 0802 planning commits and the two handoff commits on top of 3d092c2.
- No subagents, worktrees, test servers or panes of ours are running.

## Open questions
- **Q52 (implementation of 0802):** implement it now? The recommendation is yes: one `worker-medium` (opus) in a worktree (`isolation: worktree`). This is test-only, so medium effort is enough, as it was for 0157. The next new question is **Q53**.

## Next actions
1. Ask Q52 and wait for an explicit yes.
2. If yes: spawn the worker on branch `agent/agents-temp-file-collision`. Its contract is the bundle `.planning-contracts/bundles/ISSUE-261102-0802-01/` and the approval `.planning-contracts/approvals/ISSUE-261102-0802-01.json`. Tell it:
   - Each new test must fail on the current helper. The barrier test failed 3/3 in a scratch run.
   - Make one commit with subject `fix(test): unique temp agents file per call (ISSUE-261102-0802-01)`. In the same commit, `git mv` the brief to `docs/issues/done/` and set `status: done`.
   - Run the closing check with the decoy environment and the leak check (see the 0057 handoff § Conventions).
3. Cherry-pick the worker's commit, run the closing check yourself, then `receipt-create` on the cherry-picked sha and commit. Push it as `agent/agents-temp-file-collision` (the landing branch) and as `agent/wip/agents-temp-file-collision`. Remove the worktree and its branch. If the remove says "locked" by this session's pid, run `git worktree unlock` first.
4. Then follow Next actions 3–4 of the 0806 handoff: the post-landing cleanup, and the fresh-branch rule if 0802 lands after the epic.
