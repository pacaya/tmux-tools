---
id: ISSUE-261110-0019-01
kind: issue
category: enhancement
status: in-progress
claimed_by: implement-issue@agentmac
claimed_at: 2026-11-10T09:43:56Z
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
- Implement the PRD-261109-2223-01 P5.1 first-changed-line History mark and its Completeness rule in the Herdr backend, and cover it in the fake-`hc` contract tests. The fake `hc get --json` reports a `rows` value that the test sets. Tests in `tests/contract/herdr.rs` that encode the superseded line-count/last-line mark (`a_moved_anchor_line_makes_the_prompt_result_incomplete`, and the `anchored` expectation in `herdr_private_core_claude_completeness_and_capture_all`) are replaced by P5.1 expectations.
- Add the clause item 1 launch arguments to the built-in `claude` `rich` surface.
- Extend `tt-vendor-smoke` per clause item 4, then run it and re-stamp the `rich` `validated_version`.
- Reword `CONTEXT.md` per clause item 3 and README per clause item 5.

**Acceptance:**
- [ ] Given the built-in registry, when the `claude` `rich` surface resolves, then its launch arguments are `--settings` and `{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}`, and `rich` is the `default_surface` and the `pre_surface_rendering`, observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given user settings with `CLAUDE_CODE_NO_FLICKER=1` and a tmux host, when `spawn-agent claude` starts a pane with no `--surface`, then `#{alternate_on}` for that pane is `0` and Claude's status line is on the screen, observable at `tmux display -p` and `tmux-tools capture`.
- [ ] Given the same settings and an Orca host, in a new case that the implementer adds to the PRD-261107-0825-01 P10 opt-in Orca live run (`tests/contract/orca.rs`) and that the maintainer runs with `TMUX_TOOLS_LIVE_ORCA=1` from inside an Orca terminal (Q186), when `spawn-agent claude` starts a pane with no `--surface` and no caller-supplied trailing arguments, then the `claude` process of that pane has both launch argv items on its command line and that `--settings` is the only `--settings` on it, observable at `ps -o args` for that process; the maintainer's result is recorded in `## Triage Notes`, and until then the item is maintainer-run, pending.
- [ ] Given the same settings and a private hc core (`TT_TEST_HERDR=1`), when `spawn-agent claude` starts a pane and `prompt --format json` sends a prompt that makes Claude print a unique token, then `hc read --json` for that pane reports `how: history`, the result contains the token and no line before the first changed line, and Completeness is true, observable at `prompt --format json`.
- [ ] Given the fake `hc` in the default suite, observable at `prompt --format json` in `cargo test --workspace --features test-fixtures`: when the read after settle rewrites the last lines of the mark read and adds lines (a classic-scroll agent), then the result starts at the first changed line and Completeness is true; when it only adds lines after the end of the mark read (a shell), then the result is the added lines and Completeness is true; when the first changed line is less than the number of lines of the mark read minus `rows`, then the result starts there and Completeness is false; when the two reads are equal, or the read after settle is the mark read with lines removed from its end, then the result is empty and Completeness follows the same P5.1 test; when either read reports `complete: false` with `how: history`, then Completeness follows the same P5.1 test and not that flag.
- [ ] Given the smoke, when `cargo run --features vendor-smoke --bin tt-vendor-smoke` runs with the current `claude`, then its output shows, for the `claude` `rich` pane in classic scroll mode, that the pane is not on the alternate screen and that each `rich` declaration of PRD-260918-0323-01 P8 and clause item 4 (bracketed paste with a large paste arriving as one submission, the interrupt-key quit hazard, resume) was measured and holds, and that `rich` declares no `ready_regex`/`busy_regex` (noted, not failed); the smoke fails when any of them does not hold; and `validated_version` of `rich` equals that `claude --version` only after all of them hold, observable at the smoke output and `core/src/agents/builtin.rs`.
- [ ] Given the change, when `CONTEXT.md` and `README.md` are read, then they state clause items 3 and 5, observable in those files.

**Verification and release:** ships in the ISSUE-261109-2336-02 release. Rollback: revert the issue commit.

**Out of scope:** the `flat` surface; Claude sessions that tmux-tools does not start with a surface (clause item 6); the P5.1 `how: stitched`/`how: screen` rule (ISSUE-261109-2336-02).

## Context Pack — canonical at claim (2026-11-10T09:43:56Z)

**Contract approval:** `review-20261110T094310Z` · canonical · revision check complete

**Issue binding:** `tmux-tools:ISSUE-261110-0019-01` · revision `04b9a45e539824f0ea203998bc36e59af64396cd` · sha256 `490c2cb33f19aca3e9a4d8a982252760616252b4146c258685c5b164d3c2f13b`

**Required source objects:**
- `tmux-tools:ADR-260919-0146-01#claude-rich-launch` · revision `04b9a45e539824f0ea203998bc36e59af64396cd` · sha256 `3a975c52416451effe1e72d26d03992af00f4e9d32677dfde5847d3508784ccc` · `objects/3a975c52416451effe1e72d26d03992af00f4e9d32677dfde5847d3508784ccc.md`
- `tmux-tools:PRD-260918-0323-01#p8-smoke` · revision `04b9a45e539824f0ea203998bc36e59af64396cd` · sha256 `e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2` · `objects/e073a2a24ce3b5d9f9a1da38858e4f693c2f3ed2ce9a45c7698f024c39ca8bb2.md`
- `tmux-tools:PRD-261107-0825-01#p10-tests` · revision `04b9a45e539824f0ea203998bc36e59af64396cd` · sha256 `a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb` · `objects/a28eb660fd94942cad51db5869d5ba74757bff5b2ac3317f6e44699676aa3fcb.md`
- `tmux-tools:PRD-261109-2223-01#p5-io` · revision `04b9a45e539824f0ea203998bc36e59af64396cd` · sha256 `16c605a4a5c69459aa491661c76fc4bea358946393d5482093ab186588d012d3` · `objects/16c605a4a5c69459aa491661c76fc4bea358946393d5482093ab186588d012d3.md`

**Bundle manifest:** `.planning-contracts/bundles/ISSUE-261110-0019-01/manifest.json`

**Test seam:** `cargo test --workspace --features test-fixtures` (registry, fake-`hc` P5.1 cases); `tmux display -p` + `tmux-tools capture` (tmux); `ps -o args` in the P10 Orca live run; `prompt --format json` with `TT_TEST_HERDR=1` (Herdr); `cargo run --features vendor-smoke --bin tt-vendor-smoke` (smoke)

## Triage Notes

- 2026-11-10 `in-progress → ready-for-human` (attempt claimed by `implement-issue@agentmac` at 2026-11-10T05:36:39Z). Blocker, maintainer decision: with `rich` in classic scroll (`hc read --json` reports `how: history`), the PRD-261109-2223-01 P5.1 History mark (line count + last line of the full read before submission) still misses the reply. Claude redraws its input box, separator and status line below new output, so the reply lands above the recorded line count and the anchor moves; `prompt --format json` returns only that bottom block with `complete: false`. Acceptance item 4 (Herdr) cannot pass without a P5.1 change (owner PRD-261109-2223-01 / ISSUE-261109-2336-02). All other runnable items passed live; the Orca item is not run (no Orca runtime). WIP: `wip/ISSUE-261110-0019-01-20261110T060742Z` @ `7676983e30bd993fb6d0a645016e528568a1337d`.
- 2026-11-10 `ready-for-human → needs-triage`: maintainer Q185 replaced the P5.1 History mark with the first-changed-line rule (PRD-261109-2223-01 P5.1), owned by this issue. Implementation WIP: `agent/wip/ISSUE-261110-0019-01` @ `7676983`. Q186: the maintainer runs the Orca live case from an Orca terminal.
- 2026-11-10 Orca acceptance evidence (Q186, maintainer-run from an Orca terminal against WIP `7676983`): `TMUX_TOOLS_LIVE_ORCA=1 TMUX_TOOLS_LIVE_ORCA_ONLY="claude rich" cargo test --features test-fixtures --test integration orca_contract_suite -- --nocapture` → `orca contract: claude rich: classic-scroll launch arguments (ADR-260919-0146-01)`, `test orca_contract::orca_contract_suite ... ok`, 1 passed.
- 2026-11-10 `in-progress → ready-for-human` (attempt claimed by `implement-issue@agentmac` at 2026-11-10T09:03:47Z). Blocker, maintainer decision: real `hc` 0.1.19 reports `how: history` but `complete: false` for a classic-scroll Claude pane from its first turn on (verified on a private core: 12 lines `complete: true` at first idle; 16 lines `complete: false` after one turn on a 120-row screen, nothing scrolled off; only `--lines 0` reports `complete: true`). P5.1 Completeness requires `complete: true` on both reads, so the Herdr acceptance item "Completeness is true" cannot pass; every other check of that item passes live. Owner: PRD-261109-2223-01 P5.1. WIP: `wip/ISSUE-261110-0019-01-20261110T091944Z` @ `7dfacc2b0656aa6eb610e8c1f7ba99a4b0c8158b`.
- 2026-11-10 `ready-for-human → needs-triage`: maintainer Q188 removed hc's `complete` flag from the P5.1 Completeness rule (PRD-261109-2223-01 P5.1). Implementation WIP: `agent/wip/ISSUE-261110-0019-01` @ `7dfacc2`.
