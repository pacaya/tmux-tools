# Handoff: auto-tile epic — planning done, implementing in dispatch order

> Prepared 2026-11-01 19:57 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> First handoff of this strand. `docs/handoff/agent-surfaces-pane-io-2026-09-18-0004.md` is a different, stale strand — ignore its next-action.

## TL;DR
Epic PRD-260801-0656-01 (auto-tile layout) was re-triaged against v0.2.0, reminted, and is now being implemented on branch `agent/auto-tile-layout`. ISSUE-260801-1310-02 is **done** (commit 611964c + receipt 8fdda96). ISSUE-260801-1310-01 was being implemented by a subagent in worktree `.claude/worktrees/agent-a8fc3bc51ed5131fc` when this session ended — its result never reached this session. **First action: inspect that worktree.**

## Goal
Deliver the 11 issues of PRD-260801-0656-01 in the agreed order on one epic branch, then land it `--no-ff` with per-issue commits intact (lane rules in `~/.claude/CLAUDE.md` § Publishing through the lane).

## Current state
- Branch `agent/auto-tile-layout` (cut from `main` 911a05d; lane has no `development`). HEAD 8fdda96, pushed to lane as `agent/wip/auto-tile-layout`. The epic branch itself is not yet pushed under its own name.
- All 11 briefs are `agent-brief.v3`, mint-passed. Status per `grep -H '^status:' docs/issues/ISSUE-2608*.md docs/issues/ISSUE-2611*.md docs/issues/done/ISSUE-260801-1310-02*`:
  - ISSUE-260801-1310-02 — **done** (`docs/issues/done/`), receipt `.planning-contracts/receipts/ISSUE-260801-1310-02.implementation.json`.
  - ISSUE-260801-1310-01 — `ready-for-agent`, bundle `.planning-contracts/bundles/ISSUE-260801-1310-01/`; **in progress** in worktree (see TL;DR). Unknown whether the subagent committed.
  - The other 9 — `needs-triage`; readiness runs just-in-time before each claim.
- Dispatch order (maintainer-approved): 01, 02, 0628, 03, 04, 05, 261102-0012-01, 0128, 06, 07, 0205 (also the order of the PRD `issues:` list). Hard edges are in each brief's `contract.references` (`implementation_after`).

## Key decisions and rationale
All maintainer decisions Q1–Q12 (2026-11-02) are recorded in the owning records — read them there, not here:
- PRD-260801-0656-01 (§ Floor per surface, § Flags, § Reclamation on kill, § Module shape, § Documentation, § Accepted limits, § Sequencing/How to run the suite) — `git show 8b12369 5188a3c 3fbe302 -- docs/prd CONTEXT.md`.
- PRD-260918-0323-01 § P7 Completeness (width-change condition) and § Verification.
- CONTEXT.md (Floor, Agent profile, Completeness, Layout intent, Ownership).
Session-level choices not in records:
- **Order/branch/landing** — approved by maintainer: order above, branch from `main`, `--no-ff` landing, no squash (v0.2.0 was squashed; lane rule forbids it).
- **Implementation via isolated worktrees** — subagents (`worker-high`, opus, `isolation: worktree`) implement one issue each; lead reviews diff, fast-forwards/merges into the epic branch. *Why:* parallel issues without build/commit collisions.

## Dead ends — things tried that did not work
- `receipt-create --implementation <short sha>` → `implementation-revision-invalid`: needs the full 40-char sha.
- `receipt-create` with the issue's `status: done` + move to `docs/issues/done/` in a *separate later* commit → `historical-reference-unresolved` ("historical object is unavailable … at <impl sha>:docs/issues/done/…"). The **implementation commit itself must contain the issue record moved to `docs/issues/done/` with `status: done`** (as v0.2.0 did). Fixed by folding the record change into the implementation commit (`git reset --soft HEAD~1; git add -A docs/issues; git commit --amend --no-edit`) before pushing.
- `git mv` after an unstaged `sed` edit commits the rename without the edit — stage the edit explicitly.

## Conventions and gotchas observed
- Planning CLI: `P=/Users/Shared/Data/work/Programming/ai/claude/user/skills/domain-modeling/scripts/planning-contracts.sh`; always `--registry planning-contracts.json --mode canonical`.
  - Dispatch: set brief `status: ready-for-agent`, commit, then `TS=dispatch-$(date -u +%Y%m%dT%H%M%SZ); "$P" dispatch-create --registry planning-contracts.json --mode canonical --issue tmux-tools:<ID> --gate-id $TS --artifact-id $TS`, then `"$P" bundle-verify --mode canonical --bundle .planning-contracts/bundles/<ID> --approval .planning-contracts/approvals/<ID>.json`, commit `.planning-contracts`.
  - Receipt: `"$P" receipt-create --registry planning-contracts.json --mode canonical --issue tmux-tools:<ID> --implementation $(git rev-parse <impl>) --receipt-id receipt-$(date -u +%Y%m%dT%H%MZ)`, commit.
- Triage procedure: `~/.claude/skills/triage/READINESS-GATE.md` (readiness = one `cold-reader`, opus, effort xhigh, per issue; findings format `~/.claude/skills/domain-modeling/FINDINGS-FORMAT.md`). Correction loop: one consolidated repair + one targeted recheck; second repair only if the first introduced a blocker.
- **Test safety:** until ISSUE-260801-1310-01 lands, the integration suite drives `launch` against the runner's own pane. Never run it on the default tmux server. Run with a private server: `tmux -L tt-impl-$$ -f /dev/null new-session -d -s base -x 280 -y 82`, export `TMUX="<socket_path>,<pid>,<session id without $>"` and `TMUX_PANE=<pane id>`, kill the server after.
- Closing check for every slice: `cargo test --workspace --features test-fixtures` (bare `--workspace` compiles out the fixture tests). Known machine-dependent flakes (report, don't fix): `launch_keeps_pane_alive_after_cmd_exit`, `full_smoke`.
- `cargo clippy` has one pre-existing warning (`unnecessary_map_or`, `src/cmd/prompt.rs:435`) — not ours.
- Installed SKILL.md copy = `~/.claude/skills/tmux-tools/SKILL.md` → symlink into `/Users/Shared/Data/work/Programming/ai/claude` (separate repo, live tree on `agent/lane-redesign`). Edits go on branch `agent/auto-tile-layout` **in that repo**, never the live tree; landed once after the epic binary is installed (Q10).
- Worker-raised "Q1/Q2…" numbers in subagent replies are their own, not session question numbers.

## User preferences and working style
- Per `~/.claude/CLAUDE.md`: discuss non-trivial decisions with evidence + alternatives + a recommendation; ASD-STE100 style; number open questions continuously (last used: Q12 — next new question is **Q13**); fix wording-only findings yourself, ask only for behaviour/data/scope/settled-decision changes; `Changes prior decision` items each as their own numbered question.
- User answers tersely by number ("Q1 agree"). They approved every recommendation so far.

## Open questions
None.

## Next actions
1. Inspect the ISSUE-260801-1310-01 worktree: `git -C .claude/worktrees/agent-a8fc3bc51ed5131fc log --oneline -3; git -C .claude/worktrees/agent-a8fc3bc51ed5131fc status --short`. If there is one commit tagged `(ISSUE-260801-1310-01)` on top of 2af30de and a clean tree: review the diff against the bundle's acceptance; if uncommitted/partial work, decide whether to finish it in that worktree or re-dispatch a fresh subagent with the same brief.
2. Re-run the closing check yourself on the 01 work from a private tmux server (see gotchas) and also with `TMUX`/`TMUX_PANE` unset (after 01 the suite must self-isolate).
3. Bring 01 into the epic branch (it is based on 2af30de; epic HEAD is 8fdda96 → `git cherry-pick` or merge). Make the implementation commit include `docs/issues/done/ISSUE-260801-1310-01-…md` with `status: done`, then `receipt-create`, commit, push to `agent/wip/auto-tile-layout`. Remove the worktree and its branch.
4. Run readiness (cold-reader) for ISSUE-260802-0628-01 and ISSUE-260801-1310-03 (both only need 01); on PASS → `ready-for-agent` → dispatch-create → implement in worktrees (03 and 0628 can run in parallel; 04 needs both 03 and 0628).
5. Continue the order. Push `agent/wip/auto-tile-layout` at least hourly.
