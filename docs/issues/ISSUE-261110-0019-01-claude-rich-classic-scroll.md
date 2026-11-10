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
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261109-2336-01
---
## Agent Brief

**Outcome and scope:** A pane that `tmux-tools spawn-agent claude` starts on the `rich` surface (the default) shows Claude's own interface in classic scroll mode on tmux, Orca, and Herdr, and the P8 smoke fails when it does not. This issue owns `ADR-260919-0146-01#claude-rich-launch` items 1–5.

**Required sources:** `tmux-tools:ADR-260919-0146-01#claude-rich-launch`; `tmux-tools:PRD-260918-0323-01#p8-smoke` for the smoke and the `validated_version` stamp; `tmux-tools:PRD-261107-0825-01#p10-tests` for the Orca live run.

**Current observable:** with `CLAUDE_CODE_NO_FLICKER=1` in `~/.claude/settings.json`, a `spawn-agent claude` pane is on the alternate screen (`#{alternate_on}` is `1`), and on Herdr `hc read --json` reports `how: stitched` for it.

**Required change:**
- Add the clause item 1 launch arguments to the built-in `claude` `rich` surface.
- Extend `tt-vendor-smoke` per clause item 4, then run it and re-stamp the `rich` `validated_version`.
- Reword `CONTEXT.md` per clause item 3 and README per clause item 5.

**Acceptance:**
- [ ] Given the built-in registry, when the `claude` `rich` surface resolves, then its launch arguments are `--settings` and `{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}`, and `rich` is the `default_surface` and the `pre_surface_rendering`, observable at `cargo test --workspace --features test-fixtures`.
- [ ] Given user settings with `CLAUDE_CODE_NO_FLICKER=1` and a tmux host, when `spawn-agent claude` starts a pane with no `--surface`, then `#{alternate_on}` for that pane is `0` and Claude's status line is on the screen, observable at `tmux display -p` and `tmux-tools capture`.
- [ ] Given the same settings and an Orca host, in a new case of the PRD-261107-0825-01 P10 opt-in Orca live run (run from inside an Orca terminal), when `spawn-agent claude` starts a pane with no `--surface`, then the `claude` process of that pane has both launch argv items on its command line, observable at `ps -o args` for that process.
- [ ] Given the same settings and a private hc core (`TT_TEST_HERDR=1`), when `spawn-agent claude` starts a pane and `prompt --format json` sends a prompt that makes Claude print a unique token, then `hc read --json` for that pane reports `how: history`, and the result contains the token and does not contain the text above the History mark, observable at `prompt --format json`.
- [ ] Given the smoke, when `cargo run --features vendor-smoke --bin tt-vendor-smoke` runs with the current `claude`, then its output shows, for the `claude` `rich` pane in classic scroll mode, that the pane is not on the alternate screen and that each `rich` declaration of PRD-260918-0323-01 P8 and clause item 4 (bracketed paste with a large paste arriving as one submission, the interrupt-key quit hazard, resume) was measured and holds, and that `rich` declares no `ready_regex`/`busy_regex` (noted, not failed); the smoke fails when any of them does not hold; and `validated_version` of `rich` equals that `claude --version` only after all of them hold, observable at the smoke output and `core/src/agents/builtin.rs`.
- [ ] Given the change, when `CONTEXT.md` and `README.md` are read, then they state clause items 3 and 5, observable in those files.

**Verification and release:** ships in the ISSUE-261109-2336-02 release. Rollback: revert the issue commit.

**Out of scope:** the `flat` surface; Claude sessions that tmux-tools does not start with a surface (clause item 6); the P5.1 `how: stitched`/`how: screen` rule (ISSUE-261109-2336-02).
