---
id: ISSUE-260918-0525-06
kind: issue
category: bug
status: done
brief_contract: agent-brief.v2
context: tmux-tools
summary: prompt returns output from a pre-submission history mark and always reports turn observation and completeness; sessions `launch` or `spawn-agent` create get history-limit 50000
terms: [History mark, Turn observation, Completeness, busy_regex, Surface]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p7-prompt
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p4-capture
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p3-state
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-01
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-02
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-03
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-04
---

## Agent Brief

**Outcome and scope:** `prompt` never returns an empty or silently truncated result without
saying so, and the sessions `launch` or `spawn-agent` create keep enough history for that to be achievable.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p7-prompt`;
`tmux-tools:PRD-260918-0323-01#p4-capture` (the live no-scrollback check that completeness reuses);
`tmux-tools:PRD-260918-0323-01#p3-state` (the state `prompt` settles on).

**Current observable:** A prompt wider than the pane, or containing a newline, returns `""` while
JSON reports a clean `reason`.

**Required change:** P7 as written.
- Take the history mark at the submission point before submission and never advance it. The result
  begins there and always includes the agent's echo and the turn's output. Output from before
  submission is excluded wherever that removes neither; where a redraw places the echo or turn output
  above the submission point (a footer or composer redrawn in place, or the alternate screen), they
  are kept even if earlier output comes with them. Remove `extract_after_prompt`.
- `prompt` settles when P3's classifier returns idle, or by idle detection only when the resolved
  surface supplies no patterns or the pane resolves to no surface. With patterns present, unknown
  (neither or both matching) is never treated as idle. `wait-idle` keeps its current behavior.
- Report turn observation (`busy_regex` seen between submission and settle) and completeness as
  independent fields: on stderr for raw/concise, structured in JSON, on every run.
- Completeness is incomplete when P4's live check finds no scrollback, or when history has evicted
  lines. Eviction: when the running tmux provides `history_collected`, read it at the mark and at
  settle and treat any increase as eviction; otherwise treat
  `history_size >= history_limit - history_limit / 10` as eviction. Detect counter support at
  runtime, never from a version string.
- When `launch` or `spawn-agent` creates a session, set that session's `history-limit` to 50000
  before the launched command can emit output. Never change `history-limit` on a session they did
  not create, and never set it globally. README documents the equivalent `~/.tmux.conf` line for
  other sessions, its roughly 70-bytes-per-line cost, and that truncation there is reported through
  completeness.

**Acceptance:**
- [ ] Given a multi-line prompt wider than the pane, when `prompt` runs against the fixture, then the
  result contains the fixture's response and a recognizable echo of the prompt, observable at the CLI.
- [ ] Given distinctive normal-screen output above the submission point that the turn does not
  redraw, when `prompt` runs, then that output is absent from the result, observable at the CLI.
- [ ] Given a normal-screen fixture that redraws its composer in place so the echo starts above the
  submission point, when `prompt` runs, then the result contains the echo and the turn's output,
  observable at the CLI.
- [ ] Given a fixture on the alternate screen, when `prompt` runs, then the result still contains the
  recognizable echo of the prompt, observable at the CLI.
- [ ] Given a fixture that emits output before its busy footer appears, when `prompt` runs, then that
  output is returned and the turn is reported observed. With the busy footer suppressed, the same
  content is returned and the turn is reported unobserved.
- [ ] Given a surface with both patterns and a fixture holding a capture that matches neither, and
  separately both, past `--idle-seconds`, when `prompt` runs, then it does not settle with reason
  idle, observable in the CLI result.
- [ ] Given a fixture on the alternate screen; a pane whose `history_collected` increases between mark
  and settle; a pane without the counter whose `history_size` is at or above the 90% floor; and
  alternate screen together with eviction, when `prompt` runs, then each is reported incomplete, on
  stderr in raw and concise format and in the JSON field.
- [ ] Given a normal-screen fixture whose output fits in history, with the counter unchanged, or
  without the counter and below the floor, when `prompt` runs, then it is reported complete, on
  stderr and in JSON.
- [ ] Given a fixture that prints nothing after the mark, when `prompt` runs, then the result is
  empty in raw, concise and JSON, and both dimensions are still stated on stderr and in JSON.
- [ ] Given `launch` or `spawn-agent` creating a session for a command that immediately prints more
  lines than the server's global `history-limit` (and fewer than 50000), when `capture --all` runs on
  that pane, then the command's first line is present.
- [ ] Given a session tmux-tools did not create, when `launch` or `spawn-agent` places a pane in it,
  then that session's `history-limit` is unchanged, observable via tmux.

**Verification and release:** Integration tests at the CLI seam with `tt-fake-tui`; both eviction
paths (counter and floor) are exercised whether or not the host tmux provides `history_collected`.
The session-creation retention test runs on a test-owned, config-isolated tmux server: every
tmux-touching command it runs (the CLI under test and the test's own setup, inspection and teardown)
runs with `TMUX` and `TMUX_PANE` removed and `TMUX_TMPDIR` set to a test-owned temp dir, with
`-f /dev/null` or equivalent where the test starts tmux itself. Teardown, including on failure, kills
only that server via an explicit `-S` socket or `-L` label and removes the temp dir; the test never
touches the default or current server. Full workspace
suite green under the documented test command.

**Out of scope:** Excluding the echo; `pipe-pane` anchoring; reading agent transcript stores;
writing to `~/.tmux.conf`; changing `wait-idle`'s settle behavior; transport (`ISSUE-260918-0525-03`).

## Context Pack — canonical at claim (2026-09-19T10:27:05Z)

**Approval:** dispatch-20260919T102524Z (/Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/approvals/ISSUE-260918-0525-06.json) · issue tmux-tools:ISSUE-260918-0525-06@0fdde4686e59ed4068c335305a8aef18321942e6 · sha256 3b9bdd20e808761664556678bd592e7bbf6ad9e253238c05324792f2fa094bfb

**Contract bundle:** /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-06 · manifest /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-06/manifest.json

**Required source objects:**
- tmux-tools:PRD-260918-0323-01#p3-state · revision d0e7bb2df25555d69a488e5feea30c055ab07c88 · sha256 5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-06/objects/5a416456cdbf417d93cf1cf17af464fbffe97c0511e3dcac6f472eb9ac9d0435.md
- tmux-tools:PRD-260918-0323-01#p4-capture · revision d0e7bb2df25555d69a488e5feea30c055ab07c88 · sha256 40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-06/objects/40c395669c2ab6b627bacde2e0124382d55ab505d66bfe28a458a326f74c1648.md
- tmux-tools:PRD-260918-0323-01#p7-prompt · revision d0e7bb2df25555d69a488e5feea30c055ab07c88 · sha256 3dadcd27ca9820bdc0fec44db65600b508a0e4ae19c505799aa1c22058f4b67a · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-06/objects/3dadcd27ca9820bdc0fec44db65600b508a0e4ae19c505799aa1c22058f4b67a.md

**Public seam:** observable at `the CLI seam (integration tests with tt-fake-tui; session history-limit observable via tmux)`

**Mode:** canonical

## Code Review

Review: `issue-260918-0525-06-code-review-20260919-110659.md` — Outcome: ACCEPTED

## Triage Notes

- 2026-09-19: `ready-for-agent`; dispatch readiness passed against the corrected sources. The PREMISE_CHANGED park is cleared: the brief was reminted against `PRD-260918-0323-01#p7-prompt` as corrected in e27ccf2, which supersedes the per-pane `history-limit` and `history_size + visible_rows` obligations. Review H1/H2 are moot; H4 is superseded by the session-scoped rule; H3, M1, M2, L1, L2 are carried into the brief.
- Prior implementation is parked on `wip/ISSUE-260918-0525-06` (a540e03); reuse it where it satisfies this brief.

## Resolution

- Commit: `fix: prompt returns output from a pre-submission history mark and always reports turn observation and completeness; sessions `launch` or `spawn-agent` create get history-limit 50000 (ISSUE-260918-0525-06)`
- Route: deepseek. The route-picker classified it as code-primary. Implementation and correction batches 1–3 all ran in the same resumed DeepSeek session.
- TDD: red-green at the CLI seam (integration tests with `tt-fake-tui`; isolated `-L` tmux servers for session-creation tests).
- Review: ACCEPTED after 3 of 3 correction batches (`issue-260918-0525-06-code-review-20260919-110659.md`); H1, M1–M3 and L1–L6 are FIXED.
  - L1 needed batches 2 and 3. Batch 1's untargeted `set-option` followed the tmux client's resolution context. Batch 2's `tmux-tools-pending-*` private name prefix-matched the managed name.
  - Reviewers disagreed on L6. Claude verified it fixed. Codex held it STILL_BROKEN because the rename-failure path disarms the guard after a `kill-session` whose result is ignored, and the empty-id path does the same before the guard exists. The orchestrator adjudicated it fixed. The defect, a leak through `?` returns, is removed, and every post-creation exit path attempts the kill. The only alternative to the explicit ignored-result kill is the same best-effort kill in `Drop`, and a failing `kill-session` means tmux is unreachable, so no cleanup could succeed.
- Suite: `cargo test --workspace --features test-fixtures` 222 passed, 0 failed. `cargo fmt --check` is clean. `cargo clippy --workspace --all-targets --features test-fixtures` exits 0 with one advisory `clippy::unnecessary_map_or` at `src/cmd/prompt.rs:435`.
- Noted, out of scope and not introduced here: outside tmux, `capture` with no target fails on tmux 3.7c ("list-windows returned an empty pane id") because `#{window_active_pane_id}` expands empty.
- Incident: during the batch-1 recheck a reviewer repro ran without `unset TMUX`. It sent a stray `hello` turn to the dispatching session's live pane `%0`, which answered it harmlessly. Nothing else on the live server was touched.
- Date: 2026-09-19T12:26:25Z
