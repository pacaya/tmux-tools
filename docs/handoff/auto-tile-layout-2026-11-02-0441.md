# Handoff: auto-tile epic — 0012, 0157, 0128, 0329 done; 06 next

> Prepared 2026-11-02 04:41 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> Supersedes `docs/handoff/auto-tile-layout-2026-11-02-0057.md` on **Current state**, **Open questions** (Q27–Q40 are answered; Q41 is open; the last number used is Q41) and **Next actions**. Its **Dead ends**, **Conventions** (closing check, worker and readiness patterns) and **Key decisions** still stand. The **Conventions and gotchas** section of `docs/handoff/auto-tile-layout-2026-11-01-1957.md` also still stands (planning CLI commands, receipt rules).

## TL;DR
Epic PRD-260801-0656-01 is implemented on `agent/auto-tile-layout` (HEAD 16ba631, clean, pushed to the lane as `agent/wip/auto-tile-layout`). This segment finished 0012, 0157 (new), 0128 and 0329 (new), each with a receipt. The remaining issues are 06, 07 and 0205, all `needs-triage`. Q41 is open: run readiness for ISSUE-260801-1310-06? Ask it first. Do not start readiness or implementation without the user's answer.

## Current state
- **Done with receipts, on the epic branch** (implementation / receipt):
  - 01, 02, 0628, 03, 04, 05: see the predecessor handoff.
  - ISSUE-261102-0012-01 (`prompt` incomplete on width change): 67824a1 / be3d595.
  - ISSUE-261102-0157-01 (floors test pins the window name before its raw `status` comparison): febd542 / 0de15b2.
  - ISSUE-260802-0128-01 (tile default, `@tt-tiled`, `rebalance` op): d4b3874 / b4744bc.
  - ISSUE-261102-0329-01 (`launch`/`spawn-agent` succeed when the pane is gone after the split): de1345e / 16ba631.
- **Pending (`needs-triage`), in this order:** ISSUE-260801-1310-06 (kill-time reclamation), ISSUE-260801-1310-07 (overflow relocation), ISSUE-260802-0205-01 (rebalance verb).
- **Planning edits this segment:**
  - PRD-260801-0656-01 § Reporting surface has the pane-gone exception (5f88160) and the `name` rule (dfe8940).
  - CONTEXT.md: the "planned" markers for delivered terms are removed (6a1a5c3). Layout intent, Ceiling and Overflow window stay "planned".
- **Last full suite on de1345e (decoy env):** 67 + 166 + 119 passed, 0 failed, no leaks.
- **claude-settings repo:** branch `agent/auto-tile-layout` at 55807bb (0128's installed SKILL.md edit) is pushed to its lane. The worktree `/Users/Shared/Data/work/Programming/ai/claude-auto-tile-layout` is kept, because 07 or 0205 may edit the installed copy. Never edit the live tree `/Users/Shared/Data/work/Programming/ai/claude`.
- **0142 branch:** unchanged. `agent/timeout-bounds-grandchild-pipes` is at 66f4226 and waits to land. Its worktree is `/Users/Shared/Data/work/Programming/tools/tmux-tools-timeout-0142`.
- No worker worktrees, subagents or test servers are running.

## Key decisions and rationale (maintainer-approved this segment)
- **Q27:** the 0128 brief also requires both SKILL.md copies to state that `launch`/`spawn-agent`/`status` report `below_floor`, and that floors are set per surface with `min_cols`/`min_rows` in `agents.toml`. *Why:* no other record put floors in SKILL.md.
- **Q31/Q32:** the intermittent floors-test failure (tmux `automatic-rename` changed the window name between two raw `status` calls) became bug 0157 and was fixed before 0128. *Why:* a known-flaky entry could hide a real floors failure.
- **Q34:** 0128 was receipted although `launch_bare_pane_closes_when_cmd_exits` failed once. *Why:* this race comes from older code (measurement since 04), the same reasoning as Q14. It is tracked as 0329.
- **Q35–Q38, Q40:** this rule is now in PRD § Reporting surface. If the new pane is gone at any step after the split (registration, retile, marker, measurement), `launch` and `spawn-agent` exit 0, print the pane id, and leave out `window_id`, `min_pane_cols`, `min_pane_rows` and `below_floor`. stderr gets one warning line. `name` shows the chosen name (`--name`, else the derived name). Any other failure at those steps fails the command.
  - *Why:* a pane that the tool created correctly must not give exit 1.
  - *Rejected:* making the test sleep longer (users still get exit 1); `null` fields (consumers still break, and the JSON is less clear); keeping `spawn-agent` failing (the race makes that check unreliable; `wait-idle` is the reliable check).
  - Q37 was a `Changes prior decision` of "nothing conditionally omitted", and it was approved.
- **Q39:** 0329 was done before 06, because 06 and 07 build on the same `place` path.

## Dead ends / lessons this segment
- **The tmux error text differs per step when a pane is gone** (tmux 3.8). `set-option -p` gives `no such pane: %N`, `set-option -w` gives `no such window: %N`, and `select-layout`/`list-panes` give `can't find pane: %N`. Never match one error text. 0329 checks whether the pane still exists with `list-panes -a -F '#{pane_id}'` after a typed `CommandFailed`.
- **Test wrappers that fake a tmux error can hide real behavior.** Write wrappers that kill the pane and then pass the call to real tmux.
- **Under heavy machine load** (load average 120–140), tests can fail on the 5 s tmux timeout. Re-run the test alone before you report a regression.
- **A `tt-tests-<pid>-*` directory in `/tmp`** during the closing check can belong to a parallel worker's live run. Check with `ps -p <pid>` before you call it a leak.
- **Readiness keeps finding existing tests that a default change breaks** (0128: two floors tests needed `--split h --size 70`). Ask readers to look for this on 06 and 07.
- **Mint checks reject acceptance that cannot fail on the baseline** (0157 "passes 20× in a row"; the 0012 "conditions combine" check). Write criteria that fail before the change.

## Conventions (new this segment)
- **Two workers may run in parallel** on different issues (0157 and 0128). Cherry-pick in the approved order. Fix conflicts by hand.
- **`worker-medium` is enough for a small test-only fix.** Use `worker-high` for features.
- **Glossary upkeep:** after an issue lands, remove its "planned" markers in CONTEXT.md, as a separate `docs(context)` commit.
- **PRD changes:** run `"$P" impact --registry planning-contracts.json --mode canonical --target tmux-tools:PRD-260801-0656-01`. Nonterminal issues are checked again at their own readiness. Done issues stay closed.

## User preferences
- Terse answers by number ("agree with both", "approve and agree with all"). The user approved every recommendation in this segment.
- Explicit go-ahead is needed for readiness and for implementation. A "yes" to "readiness and then implement" covers both.

## Open questions
- **Q41 (next step):** run readiness for ISSUE-260801-1310-06? The recommendation is yes, because it is next in the order.

## Next actions
1. Ask Q41 and wait for the answer.
2. If yes: spawn a `cold-reader` (opus, effort xhigh, background) for the readiness of `docs/issues/ISSUE-260801-1310-06-kill-time-reclamation.md`.
   - Read its `contract.references` first.
   - Give it the delivered shas above and the new PRD pane-gone rule (5f88160, dfe8940).
   - Ask it to look for existing tests that the change would break.
   - On PASS: set `ready-for-agent`, commit, run `dispatch-create` and `bundle-verify`, commit, and push.
3. Implement only after an explicit yes. Use the worker pattern from the predecessor handoff.
4. Push `agent/wip/auto-tile-layout` at least hourly while work is in flight.
5. After the 0142 branch lands upstream: `git worktree remove /Users/Shared/Data/work/Programming/tools/tmux-tools-timeout-0142 && git branch -D agent/timeout-bounds-grandchild-pipes`.
