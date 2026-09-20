---
id: ISSUE-260918-0525-05
kind: issue
category: enhancement
status: done
claimed_by: implement-issue@Mac-mini-4
claimed_at: 2026-09-19T06:22:54Z
context: tmux-tools
summary: spawn-agent resumes a session through the surface's declared syntax and fails loudly on a malformed identifier
terms: [Surface]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p6-resume
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p2-surface
    - relation: blocked_by
      repository: tmux-tools
      record: ISSUE-260918-0525-02
---

## Agent Brief

**Outcome and scope:** `spawn-agent` takes a session identifier to resume, maps it through the
target surface's resume syntax, and validates it against that surface's identifier shape before
creating any pane.

**Required sources:** `tmux-tools:PRD-260918-0323-01#p6-resume`;
`tmux-tools:PRD-260918-0323-01#p2-surface` (surface-supplied arguments do not set the
surface-unvalidated mark).

**Current observable:** `spawn-agent` has no resume input. A resume can only be requested as
caller-supplied trailing arguments, which are passed through without validation.

**Required change:** P6 as written. Fill in resume syntax and identifier shape on the built-in
surfaces that support resume, and add them to README's documented `dsh` surfaces, with P6's
measured `dsh` identifier shapes. The resume argument the surface supplies does not mark
the pane surface-unvalidated.

**Acceptance:**
- [ ] Given a malformed identifier, when `spawn-agent` runs with it, then it fails with a message and
  no pane is created, observable at the CLI and in `tmux list-panes`.
- [ ] Given a well-formed identifier, when `spawn-agent` runs with it, then the launch command
  carries the surface's resume syntax, observable in the arguments a stub agent binary reports it
  received, and the pane is not marked surface-unvalidated, observable in pane registration.
- [ ] Given README's documented `dsh` surfaces loaded as `agents.toml`, and an identifier outside the
  rich surface's settled shape, when `spawn-agent dsh` runs with it, then it fails with a message and
  no pane is created, observable at the CLI and in `tmux list-panes`, with no live `dsh` binary.

**Verification and release:** Integration tests at the CLI seam, with a stub registry entry where a
real agent binary would be needed; full workspace suite green under the documented test command.

**Out of scope:** Resolving the most recent session; reading `~/.claude/projects` or
`~/.dsh/sessions`.

## Context Pack — canonical at claim (2026-09-19T06:22:54Z)

**Approval:** dispatch-20260919T043728Z · /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/approvals/ISSUE-260918-0525-05.json · issue tmux-tools:ISSUE-260918-0525-05@f193466b9e5e8102a5c84c413c458150caff071f · sha256 e4e8382d7c5c8faa6b2652becdf45b8d9696d5373d7bf43f08ebc39ba9a951e7

**Contract bundle:** /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-05 · manifest /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-05/manifest.json

**Required source objects:**
- tmux-tools:PRD-260918-0323-01#p6-resume · revision fc2b3c1c1301ae23ffdaa0002a388c3ef2464697 · sha256 e90bf434162cb4c6fec5dadce8855712f30be73c4c16fe34c0ee1dd90ad102e9 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-05/objects/e90bf434162cb4c6fec5dadce8855712f30be73c4c16fe34c0ee1dd90ad102e9.md
- tmux-tools:PRD-260918-0323-01#p2-surface · revision fc2b3c1c1301ae23ffdaa0002a388c3ef2464697 · sha256 89e022e32cffd813184bc0c6e201a2e28a09fa2312ddf3c7d27c2898edd794d0 · object /Users/Shared/Data/work/Programming/tools/tmux-tools/.planning-contracts/bundles/ISSUE-260918-0525-05/objects/89e022e32cffd813184bc0c6e201a2e28a09fa2312ddf3c7d27c2898edd794d0.md

**Public seam:** observable at `the spawn-agent CLI (exit and message), tmux list-panes, the argv a stub agent binary reports, and pane registration (surface-unvalidated mark)`

**Mode:** canonical

## Code Review

Review: `issue-260918-0525-05-code-review-20260919-070012.md` — Outcome: ACCEPTED

## Resolution

- **Commit:** `feat: spawn-agent resumes a session through the surface's declared syntax and fails loudly on a malformed identifier (ISSUE-260918-0525-05)`
- **Route:** DeepSeek. The work is code-primary: registry resume declarations, `spawn-agent` validation, CLI tests, and supporting README and SKILL documentation. One fresh DeepSeek session did the implementation and a second fresh session did the correction.
- **TDD:** red-green at the `spawn-agent` CLI seam (exit and message, `tmux list-panes`, argv reported by a stub agent binary, pane registration).
- **Review:** dual review (Claude and Codex). Claude reported no findings. Codex reported four. CODEX-1 (cursor and agy have no resume) was dropped per PRD-260918-0323-01 Out of Scope. CODEX-2 (codex session names) was dropped because free-form names cannot be shape-validated without losing P6's fail-loudly property. CODEX-3 (malformed-ID tests) was dropped because the pane and stub-log assertions catch a regression. CODEX-4, a race in the argv stub, was kept as L1. The single correction fixed L1 with a sentinel line. Both reviewers verified it fixed. Outcome: ACCEPTED.
- **Suite:** `cargo test --workspace --features test-fixtures`: 171 passed, 0 failed. Clippy (all targets) and `cargo fmt --check` clean.
- **Date:** 2026-09-19 (UTC)
