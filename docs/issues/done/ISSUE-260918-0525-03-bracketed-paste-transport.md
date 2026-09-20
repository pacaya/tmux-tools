---
id: ISSUE-260918-0525-03
kind: issue
category: bug
status: done
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-09-19T04:41:52Z
context: tmux-tools
summary: prompt delivers text through load-buffer + paste-buffer -p, keyed to the live bracketed-paste flag, replacing chunked send-keys
terms: [Bracketed paste transport, Surface]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p1-transport
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-01
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-02
---

## Agent Brief

**Outcome and scope:** One `prompt` is one submission whenever the target has bracketed paste
enabled. When it hasn't, `prompt` reports that honestly or refuses.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p1-transport`.

**Current observable:** `prompt` sends text as chunked `send-keys -l` without bracketing, so an
embedded newline reaches the agent as a keypress.

**Required change:** P1 as written. Transport is stdin `load-buffer` into a uniquely named buffer,
then `paste-buffer -p -d`. It is keyed to `bracket_paste_flag` read right before pasting, with P1's
three reported outcomes. The buffer is deleted on every path after a successful `load-buffer`, and
the original error is kept. No temp file, no `-r`. Remove `chunk_literal` and its tests. Extend
`tt-fake-tui` to enable bracketed paste (switchable off) and report the text it received. Declare
bracketed-paste capability on `claude`'s rich surface; other
surfaces' capability is recorded by `ISSUE-260918-0525-07`.

**Acceptance:**
- [ ] Given a payload larger than tmux's `send-keys` ceiling and containing newlines, when `prompt`
  sends it to a fixture with bracketed paste on, then the fixture reports exactly one submission
  whose text matches what was sent, compared as the fixture reports it rather than as raw pane
  bytes; `prompt` reports one-submission delivery guaranteed on stderr (raw and concise) and in the
  JSON field, and `tmux list-buffers` shows no prompt buffer.
- [ ] Given a transport failure after `load-buffer`, when `prompt` runs, then no prompt buffer
  remains in tmux and the original error is reported, observable at the CLI and in
  `tmux list-buffers`.
- [ ] Given a fixture with bracketed paste off and a surface declaring the capability, when `prompt`
  sends multi-line text, then it refuses with a non-zero exit and reports the refusal on stderr (raw
  and concise) and in the JSON field, the fixture reports no text received, and
  `tmux list-buffers` shows no prompt buffer.
- [ ] Given `tt-fake-tui` with bracketed paste off in a pane with no surface, when `prompt` sends the
  same text, then the fixture reports receiving it and `prompt` reports one-submission delivery
  unavailable on stderr (raw and concise) and in the JSON field.

**Verification and release:** Integration tests at the CLI seam with `tt-fake-tui`; full workspace
suite green under the documented test command. Supersedes commit 63e4660.

**Out of scope:** Where `prompt` output starts and how completeness is reported (P7,
`ISSUE-260918-0525-06`); a file or stdin input path for `prompt`; live-agent validation (P8).
## Context Pack — canonical at claim (2026-09-19T04:41:52Z)

**Approval:** dispatch-20260919T043330Z · /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/approvals/ISSUE-260918-0525-03.json · issue tmux-tools:ISSUE-260918-0525-03@dc5f83fa926098239be9bcff62fd9e62498a2b79 · sha256 eaa04dda64819f5809a208df1aee312aa11dc3b6786dbf6427ff3a42dc5ac63f

**Contract bundle:** /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-03 · manifest /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-03/manifest.json

**Required source objects:**
- tmux-tools:PRD-260918-0323-01#p1-transport · revision 61a74879a6f1fe53be714833274b2d256c28c7a5 · sha256 8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-03/objects/8842bfd61bf171639a253a44e316076a470c48844b93a7069d7cdc6e9c9c1d18.md

**Public seam:** observable at `the CLI seam (tmux-tools prompt) against tt-fake-tui, plus tmux list-buffers`

**Mode:** canonical

## Code Review

Review: `issue-260918-0525-03-code-review-20260919-051302.md` — Outcome: ACCEPTED

## Resolution

- **Commit:** `fix: prompt delivers text through load-buffer + paste-buffer -p, keyed to the live bracketed-paste flag, replacing chunked send-keys (ISSUE-260918-0525-03)`
- **Route:** DeepSeek. The work is code-primary: the `prompt` transport, tmux stdin plumbing, fixture, and CLI tests. One DeepSeek session carried the implementation and both corrections by resume.
- **TDD:** red-green at the CLI seam (`tmux-tools prompt` against `tt-fake-tui` on a private tmux server, plus `tmux list-buffers`).
- **Review:** dual review (Claude and Codex) kept three findings after adjudication (M1–M3). CODEX-3 was dropped: Codex withdrew it and Claude disputed it. The repeat-Enter half of CODEX-2 was ruled out of scope because the Enter dispatch is unchanged from base. The first correction fixed M1–M3. Its M2 repair introduced M4, a buffer leak when the post-load flag read fails, which the allowed second correction fixed. Both reviewers verified all four fixed. Outcome: ACCEPTED.
- **Beyond the brief:** the implementer made the pre-existing flaky `launch_keeps_pane_alive_after_cmd_exit` deterministic (`echo` in place of `printf`), and applied clippy's `io::Error::other` idiom in `core/src/tmux.rs`.
- **Suite:** `cargo test --workspace --features test-fixtures --no-fail-fast` 163 passed, 0 failed. Bare `cargo test --workspace` 134 passed, 0 failed.
- **Date:** 2026-09-19 (UTC)
