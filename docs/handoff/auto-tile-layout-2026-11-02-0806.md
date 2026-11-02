# Handoff: auto-tile epic complete, v0.3.0 installed; bug 0802 at needs-triage

> Prepared 2026-11-02 08:06 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> Supersedes `docs/handoff/auto-tile-layout-2026-11-02-0441.md` on **TL;DR**, **Current state**, **Open questions** (Q41–Q50 are answered; the last number used is **Q50**, so the next new question is **Q51**) and **Next actions**. Its **Dead ends** still stand. The **Conventions** sections of `docs/handoff/auto-tile-layout-2026-11-02-0057.md` (closing check, worker and readiness patterns) and `docs/handoff/auto-tile-layout-2026-11-01-1957.md` (planning CLI commands, receipt rules) still stand.

## TL;DR
Epic PRD-260801-0656-01 is complete: every issue has a receipt. The 0142 branch is merged into the epic, the version is 0.3.0, the binary is installed here, and `agent/auto-tile-layout` (3d092c2) is on the lane for the user to merge into `development` on the control Mac. A new standalone test-only bug, ISSUE-261102-0802-01, is at `needs-triage` on its own branch. Nothing is running. Next: ask the user whether to run readiness for 0802.

## Current state
- **Done, on `agent/auto-tile-layout` = lane `agent/auto-tile-layout` = lane `agent/wip/auto-tile-layout` = 3d092c2:**
  - ISSUE-260801-1310-06 (kill-time reclamation): implementation e59baf4, receipt c12a1ae.
  - ISSUE-260801-1310-07 (overflow relocation): 9027bee / b857e68.
  - ISSUE-260802-0205-01 (`rebalance` verb): 2bcc055 / 9f866a7.
  - 0142 merged with `--no-ff`: 1bcf23f. The stale `ready-for-agent` copy of its brief was removed in that merge commit.
  - Version 0.3.0: 3d092c2. Full suite on 3d092c2 (decoy env): 67 + 200 + 133 passed, 0 failed, no leaks.
  - CONTEXT.md has no "planned" markers left for this PRD (1aecfc2, 2aef3dc).
- **Installed:** `~/.cargo/bin/tmux-tools`, `tmux-tools 0.3.0`, built with `cargo install --path . --locked` from 3d092c2.
- **claude-settings:** branch `agent/auto-tile-layout` at 1da0982 (0205's installed SKILL.md edit, on top of 55807bb for 0128) is pushed to its lane. Per Q10 it lands after the binary install, so it can now land with the epic. The worktree `/Users/Shared/Data/work/Programming/ai/claude-auto-tile-layout` is still there. Never edit the live tree `/Users/Shared/Data/work/Programming/ai/claude`. It has an uncommitted `codex/config.toml` change that is not ours.
- **`completion-verify` gives `landing-binding-required`** for 06, 07, 0205 and 0142. This is expected: history advanced past the receipt-bound revisions. The landing binding (`landing-create`) is made when the branch lands on `development`.
- **Bug ISSUE-261102-0802-01** (`docs/issues/ISSUE-261102-0802-01-agents-temp-file-path-collision.md`): `needs-triage`, mint passed. Branch `agent/agents-temp-file-collision` (cut from 3d092c2), HEAD d97bece, pushed as lane `agent/wip/agents-temp-file-collision`. The current checkout is this branch. This handoff is committed on it too.
- **0142 cleanup is pending:** after `development` contains it, run `git worktree remove /Users/Shared/Data/work/Programming/tools/tmux-tools-timeout-0142 && git branch -D agent/timeout-bounds-grandchild-pipes`.
- No subagents, worker worktrees, test servers or panes of ours are running.

## Key decisions and rationale (maintainer-approved this segment)
- **Q43:** `relocated` stays in the pane-gone result. It is `true` only when a `join-pane`/`break-pane` of the new pane succeeded before the pane was gone. *Why:* only fields that describe an unmeasurable window are left out; facts about what the tool did are kept (as with `name`). The list of four omitted fields is unchanged.
- **Q44:** a refused `join-pane` while the pane exists is a rejected trial. A failed `break-pane` while the pane exists leaves the pane where it is, retiles that window, and exits 0 with `below_floor: true`. Other step failures fail the verb. *Why:* PRD § Solution says no invocation starts failing, and the same reason as Q35. Both rules are in PRD § Reporting surface and § Placement and overflow (dc4e7c1).
- **Q42/Q45/Q46:** readiness may run in parallel with an implementation. Implementations that edit the same module (`core/src/layout.rs`) run one after the other.
- **User request after Q47:** merge everything onto the epic here, build and install the binary here (the app is needed on this Mac); the user merges into `development` on the control Mac.
- **Q49:** version 0.3.0. *Why:* new verb and changed default placement; `--version` shows the installed build.
- **Q50 (option A):** install first. The flaky helper becomes the separate bug 0802. *Why:* test-only, it does not affect the binary.

## Dead ends / lessons this segment
- **A PRD edit made after an issue's dispatch blocks that issue's receipt.** `receipt-create` compares the whole PRD (`clauses: []`) with the approved hash and gives `stale-approval ... required-source content changed`. It does this even when the edited sections are ones the issue does not require. `dispatch-refresh` cannot fix it after the brief moved to `done/` (`historical-reference-unresolved`). What worked for 06: move the brief back with `status: ready-for-agent`, commit; `dispatch-create`, `bundle-verify`, commit; move it to `done/` with `status: done`, commit; then `receipt-create` on that last commit (7cf54c4..c12a1ae). **Rule from now on:** after any PRD edit, run `dispatch-create` again for every `ready-for-agent` or in-progress issue whose approval binds that PRD, at once.
- **`git worktree remove` of an Agent-tool worktree can say "locked"**, with the lock owned by this session's own `claude` pid. If the agent has finished and `git -C <wt> status -s` is empty, `git worktree unlock <wt>` and then remove it.
- **`tmux display -p -t =<session>` prints an empty string in tmux 3.8.** Use `=<session>:`. The 07 worker found that a focus test compared `""` with `""`.
- **Temp-file names from `SystemTime::now().as_nanos()` collide on macOS.** The clock has 1 µs resolution (nanos always end in `000`). This is bug 0802. The 8-thread barrier test failed 3/3 on the current helper.
- **Merging the 0142 branch left a duplicate brief.** The epic had `docs/issues/ISSUE-261102-0142-01-…md` (`ready-for-agent`) and the merge added `docs/issues/done/…` (`done`). Remove the stale copy in the merge.

## Conventions (new this segment)
- **Mutation checks:** the workers changed production code on purpose (removed `-d`, skipped the guard) to show that each new test catches what it must catch. Ask for this when a baseline failure alone (for example "unrecognized subcommand") could hide weak tests.
- **A standalone bug gets its own branch** `agent/<slug>`, pushed as `agent/wip/<slug>` until it is done. Do not add commits to an epic branch after it is published.
- **The "outside tmux" test** is `TMUX` unset or empty: `core::target::in_tmux()` (added by 0205). Use it, not `var_os("TMUX").is_none()`.

## User preferences
- Terse answers by number ("Q50 agree", "agree with both"). The user approved every recommendation in this segment.
- The user likes running readiness in parallel with implementation when the issues do not conflict.
- Explicit yes is needed before readiness and before implementation.

## Open questions
- None are open. The next new question is **Q51**.

## Next actions
1. Ask Q51: run readiness for ISSUE-261102-0802-01? Recommend yes. It is small and test-only (one `cold-reader`, opus, xhigh). Its baseline is the current helper `write_temp_agents_file` in `core/src/agents/mod.rs`.
2. If yes and PASS: `ready-for-agent`, commit, `dispatch-create` + `bundle-verify`, commit, push `agent/wip/agents-temp-file-collision`. Implement only after an explicit yes. `worker-medium` is enough. When done, push it as `agent/agents-temp-file-collision` for landing.
3. After the user lands the epic on `development`: remove the 0142 worktree and branch (command above). Then delete the local `agent/auto-tile-layout` and cut new work from `development`. If 0802 is not landed yet, cut a fresh `agent/agents-temp-file-collision` from `development` and cherry-pick only its own commits (`git log --oneline 3d092c2..agent/agents-temp-file-collision`). Do not rebase the old branch (lane rule).
4. Push any work in flight to `agent/wip/<topic>` at least hourly.
