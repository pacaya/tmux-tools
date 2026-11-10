---
id: ISSUE-261110-0019-01
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: panes that spawn-agent claude starts on the rich surface run in classic scroll mode, not full screen, on every host
terms: [Surface, Rich surface, Flat surface, Default surface, Validated version, History mark, Completeness]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: ADR-260919-0146-01
      clause: claude-rich-launch
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p8-smoke
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261109-2223-01
      clause: p5-io
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261109-2336-01
---
## Agent Brief

**Outcome and scope:** A pane that `tmux-tools spawn-agent claude` starts on the `rich` surface (the default) shows Claude's own interface in classic scroll mode on tmux, Orca, and Herdr, and the P8 smoke fails when it does not. On Herdr, `prompt` to such a pane returns the reply. This issue owns `ADR-260919-0146-01#claude-rich-launch` items 1–5 and the PRD-261109-2223-01 P5.1 first-changed-line History mark and its Completeness rule.

**Required sources:** `tmux-tools:ADR-260919-0146-01#claude-rich-launch`; `tmux-tools:PRD-260918-0323-01#p8-smoke` for the smoke and the `validated_version` stamp; `tmux-tools:PRD-261107-0825-01#p10-tests` for the Orca live run; `tmux-tools:PRD-261109-2223-01#p5-io` (P5.1) for the Herdr History mark.

**Current observable:** with `CLAUDE_CODE_NO_FLICKER=1` in `~/.claude/settings.json`, a `spawn-agent claude` pane is on the alternate screen (`#{alternate_on}` is `1`), and on Herdr `hc read --json` reports `how: stitched` for it.

**Required change:**
- Implement the PRD-261109-2223-01 P5.1 first-changed-line History mark and its Completeness rule in the Herdr backend, and cover it in the fake-`hc` contract tests.
- Add the clause item 1 launch arguments to the built-in `claude` `rich` surface.
- Extend `tt-vendor-smoke` per clause item 4, then run it and re-stamp the `rich` `validated_version`.
- Reword `CONTEXT.md` per clause item 3 and README per clause item 5.

**Acceptance:**
- [ ] Given the built-in registry, when the `claude` `rich` surface resolves, then its launch arguments are `--settings` and `{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}`, and `rich` is the `default_surface` and the `pre_surface_rendering`, observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given user settings with `CLAUDE_CODE_NO_FLICKER=1` and a tmux host, when `spawn-agent claude` starts a pane with no `--surface`, then `#{alternate_on}` for that pane is `0` and Claude's status line is on the screen, observable at `tmux display -p` and `tmux-tools capture`.
- [ ] Given the same settings and an Orca host, in a new case of the PRD-261107-0825-01 P10 opt-in Orca live run (run from inside an Orca terminal), when `spawn-agent claude` starts a pane with no `--surface` and no caller-supplied trailing arguments, then the `claude` process of that pane has both launch argv items on its command line and that `--settings` is the only `--settings` on it, observable at `ps -o args` for that process.
- [ ] Given the same settings and a private hc core (`TT_TEST_HERDR=1`), when `spawn-agent claude` starts a pane and `prompt --format json` sends a prompt that makes Claude print a unique token, then `hc read --json` for that pane reports `how: history`, the result contains the token and no line before the first changed line, and Completeness is true, observable at `prompt --format json`.
- [ ] Given the fake `hc` in the default suite, when the read after settle rewrites the last lines of the mark read and adds lines (a classic-scroll agent), then the `prompt` result starts at the first changed line and Completeness is true; when the first changed line is above the last `rows` lines of the mark read, then the result starts there and Completeness is false, observable at `prompt --format json` in `cargo test --workspace --features test-fixtures`.
- [ ] Given the smoke, when `cargo run --features vendor-smoke --bin tt-vendor-smoke` runs with the current `claude`, then its output shows, for the `claude` `rich` pane in classic scroll mode, that the pane is not on the alternate screen and that each `rich` declaration of PRD-260918-0323-01 P8 and clause item 4 (bracketed paste with a large paste arriving as one submission, the interrupt-key quit hazard, resume) was measured and holds, and that `rich` declares no `ready_regex`/`busy_regex` (noted, not failed); the smoke fails when any of them does not hold; and `validated_version` of `rich` equals that `claude --version` only after all of them hold, observable at the smoke output and `core/src/agents/builtin.rs`.
- [ ] Given the change, when `CONTEXT.md` and `README.md` are read, then they state clause items 3 and 5, observable in those files.

**Verification and release:** ships in the ISSUE-261109-2336-02 release. Rollback: revert the issue commit.

**Out of scope:** the `flat` surface; Claude sessions that tmux-tools does not start with a surface (clause item 6); the P5.1 `how: stitched`/`how: screen` rule (ISSUE-261109-2336-02).

## Context Pack — canonical at claim (2026-11-10T05:36:39Z)

**Contract approval:** `review-20261110T053552Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261110-0019-01` · revision `8a41937a17fe420ff6ffc8dbcf7a9cdb5e4bb9c8` · sha256 `b2703f21b023d9d11b9aac0b9755442e113fb467a6c2b20a0a3ffc2e2f59a8b1`

**Required source objects:**
- `tmux-tools:ADR-260919-0146-01#claude-rich-launch` · revision `8a41937a17fe420ff6ffc8dbcf7a9cdb5e4bb9c8` · sha256 `3a975c52416451effe1e72d26d03992af00f4e9d32677dfde5847d3508784ccc` · `objects/3a975c52416451effe1e72d26d03992af00f4e9d32677dfde5847d3508784ccc.md`
- `tmux-tools:PRD-260918-0323-01#p8-smoke` · revision `8a41937a17fe420ff6ffc8dbcf7a9cdb5e4bb9c8` · sha256 `e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2` · `objects/e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2.md`
- `tmux-tools:PRD-261107-0825-01#p10-tests` · revision `8a41937a17fe420ff6ffc8dbcf7a9cdb5e4bb9c8` · sha256 `a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb` · `objects/a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261110-0019-01/manifest.json`

**Test seam:** `cargo test --workspace --features test-fixtures` (registry); `tmux display -p` + `tmux-tools capture` (tmux); `ps -o args` in the P10 Orca live run; `prompt --format json` with `TT_TEST_HERDR=1` (Herdr); `cargo run --features vendor-smoke --bin tt-vendor-smoke` (smoke)

## Triage Notes

- 2026-11-10 `in-progress → ready-for-human` (attempt claimed by `implement-issue@agentmac` at 2026-11-10T05:36:39Z). Blocker, maintainer decision: with `rich` in classic scroll (`hc read --json` reports `how: history`), the PRD-261109-2223-01 P5.1 History mark (line count + last line of the full read before submission) still misses the reply. Claude redraws its input box, separator and status line below new output, so the reply lands above the recorded line count and the anchor moves; `prompt --format json` returns only that bottom block with `complete: false`. Acceptance item 4 (Herdr) cannot pass without a P5.1 change (owner PRD-261109-2223-01 / ISSUE-261109-2336-02). All other runnable items passed live; the Orca item is not run (no Orca runtime). WIP: `wip/ISSUE-261110-0019-01-20261110T060742Z` @ `7676983e30bd993fb6d0a645016e528568a1337d`.
- 2026-11-10 `ready-for-human → needs-triage`: maintainer Q185 replaced the P5.1 History mark with the first-changed-line rule (PRD-261109-2223-01 P5.1), owned by this issue. Implementation WIP: `agent/wip/ISSUE-261110-0019-01` @ `7676983`. Q186: the maintainer runs the Orca live case from an Orca terminal.
