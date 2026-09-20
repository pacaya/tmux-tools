# Handoff: agent surfaces and pane I/O — PRD gated, issue breakdown awaiting approval

> Prepared 2026-09-18 00:04 from a Claude Code session in `/Users/Shared/Data/work/Programming/tools/tmux-tools`.
> Audience: a fresh Claude instance with no memory of the prior conversation. Read top to bottom.
> First handoff for this strand.

## TL;DR

A `/grilling` session established, by live measurement, that three `tmux-tools` verbs lie to their
callers; the settled response is `docs/prd/PRD-260918-0323-01-agent-surfaces-and-pane-io.md`, which
is written, has survived both gate layers, and carries `gate: passed 2026-09-18`. `/to-issues` is
mid-run: it stopped at **step 4**, having proposed a seven-packet breakdown and asked the user to
approve the granularity and dependency structure. **The single next action is to get that approval
and then mint the issue records.** Nothing is committed — the PRD is untracked.

## Goal

Fix three defects in `tmux-tools` where a verb reports success while delivering something else, and
remove the structural cause: every agent this tool drives has more than one *rendering*, the
renderings differ in whether tmux scrollback exists and what readiness chrome they show, and neither
the registry nor a pane's registration recorded which one was in use.

The user's stated constraint throughout: decisions are theirs, grounded in verified evidence rather
than the assistant's defaults. They rejected assumption-driven recommendations repeatedly and asked
for objective critique of their own proposals (see *User preferences*).

## Current state

| Strand | Status | Artifact |
|---|---|---|
| Investigation of the two reported bugs | **done** | Findings summarized below under *Dead ends* and *Reference snippets*; not in any record |
| PRD authored, both gate layers passed, stamped | **done** | `docs/prd/PRD-260918-0323-01-agent-surfaces-and-pane-io.md` (323 lines, untracked) |
| `/to-issues` steps 0–3 | **done** | Pre-check passed; per-repo mode; seven packets drafted (below) |
| `/to-issues` step 4 — user approval of breakdown | **in progress — BLOCKED ON USER** | Question asked, not answered |
| `/to-issues` step 5 — mint `ISSUE-*` records | **pending** | Flat `docs/issues/`, ID grammar `ISSUE-YYMMDD-HHMM-NN` from one `date -u` for the batch, `-NN` in dependency order |
| `/to-issues` step 6 — mint-time brief check | **pending** | `triage/READINESS-GATE.md` in `mint` mode per issue |
| `/to-issues` step 7 — breakdown adversary | **pending, and its trigger IS met** | Shared write ownership of the `tt-fake-tui` fixture across packets 1/2/3/6, plus packet 6 depending on two siblings |
| Git | **nothing committed** | Branch `fix/prompt-send-keys-size-ceiling`, working tree otherwise clean |

### The proposed breakdown, exactly as presented to the user

1. **Surface model, selection, and pane-state classification** — blocked by: none — covers P2 (except default promotion), P3. Adds the surface map to `AgentSpec`, declares rich+flat surfaces for claude/dsh/codex, `spawn-agent --surface`, unconditional recording of the resolved surface, total resolution, the surface-unvalidated mark, the three-valued classifier in `core/src/idle.rs`. Introduces the `tt-fake-tui` fixture. Carries the ADR and CONTEXT.md minting. Defaults do not move.
2. **Honest capture ceiling** — blocked by: 1 — covers P4.
3. **Bracketed-paste transport** — blocked by: 1 — covers P1.
4. **State-aware interrupt** — blocked by: 1 — covers P5.
5. **Surface-aware resume** — blocked by: 1 — covers P6.
6. **Prompt contract and launch history-limit** — blocked by: 1, 2, 3 — covers P7.
7. **Vendor smoke and flat-default promotion** — blocked by: 1, 3, 4 — covers P8 and P2's promotion clause.

Three things were flagged to the user as genuinely arguable and may come back changed: packet 1 is
the largest and could be read as two; the 2→1 edge exists *only* because the fixture is born in 1
and flips if capture-honesty should land first; and 2 could merge into 6 as one honest-reporting
packet.

## Key decisions and rationale

The PRD's clauses P1–P8 are the authoritative record of what must be built — **read it, don't rely
on this list**. What follows is only the rationale that did not survive into the record, because
planning-record conventions in `~/.claude/CLAUDE.md` exclude proof-that-the-work-deserves-to-exist.

- **The user's reported bug #1 was wrong, and that matters for anyone re-reading the branch.**
  "Long input pastes only the last chunk" is false; chunked `send-keys -l` delivers completely.
  The appearance comes from Claude Code rendering a large paste as `[Pasted text #N]` chips.
  The real defect is `extract_after_prompt` (`src/cmd/prompt.rs:144-179`).
  - *Why it matters:* the branch is named `fix/prompt-send-keys-size-ceiling` and commit `63e4660`
    fixes a real but different problem. The PRD supersedes it rather than reverting it.
- **Transport moves to `load-buffer` + `paste-buffer -p -d` from stdin.**
  - *Why:* it removes tmux's argv ceiling rather than staying under it, and `-p` is the only thing
    that makes an embedded newline a newline in an agent composer instead of a submission.
  - *Alternatives rejected:* keeping chunked `send-keys` (works only by grace of the agent's own
    paste-burst heuristic); a temp file for `load-buffer` (puts prompt text on disk).
- **Surfaces became a first-class registry concept rather than per-agent argument edits.**
  - *Why:* `ready_regex`/`ready_lines` are valid for exactly one rendering. Flipping `dsh` to
    `--profile dshline` while `~/.config/tmux-tools/agents.toml:183` still holds tui chrome would
    launch fine and never report ready — a silent readiness failure of the class being fixed.
- **`prompt` never advances its history mark.** Marked before submission, never moved; the result
  always includes the agent's echo.
  - *Why:* both gate layers independently found that advancing to the `busy_regex` transition
    discards response bytes emitted before the busy footer rendered. A bulky result that says it is
    bulky beats a clean result that quietly lost the first lines.
- **No predicate tries to detect that an agent exited.**
  - *Why:* comparing `pane_current_command` to the configured binary misfires on two of three
    agents — a live claude pane reports its version string (`2.1.276`), a live dsh pane reports
    `node`. An exited agent's shell matches neither pattern, so P3 already returns unknown and P5
    already refuses. The safe behavior falls out without the predicate.
- **Panes spawned with caller-supplied trailing args are marked surface-unvalidated.**
  - *Why:* pattern mismatch is not a safety boundary — a different rendering may still match the
    recorded surface's `busy_regex`, letting `interrupt` send a key whose quit behavior belongs to a
    rendering that was never recorded.
- **The `Dimension Scan` section was kept against an adversary `DELETE` finding.**
  - *Why:* `to-spec/SPEC-GATE.md` owns that table and the gate stamp depends on it, and
    `docs/prd/PRD-260801-0656-01-auto-tile-layout.md` already carries one. Repo precedent and the
    owning contract agree. **If the adversary raises this again, it is settled — do not re-litigate.**
- **`gate-retain` was skipped.** It requires `--registry`; this repo has no coordination registry and
  `~/.claude/CLAUDE.md` forbids requiring one for ordinary single-repository work. Precedent:
  `PRD-260801-0656-01` carries a frontmatter stamp and no registry artifacts. Git is the snapshot.

## Files touched

| Path | Change |
|---|---|
| `docs/prd/PRD-260918-0323-01-agent-surfaces-and-pane-io.md` | **Created.** The whole deliverable. 323 lines, untracked, `gate: passed 2026-09-18` |
| `docs/handoff/` | **Created** (directory did not exist) |

No source file was modified. No experiment touched the repo.

## Dead ends — things tried that did not work

- **Chasing "only the last chunk is pasted" as a delivery bug.** 32,320 bytes in five
  `send-keys -l` chunks arrived byte-identical at a raw-mode pty sink, and a live Claude pane
  answered correctly over the whole payload (`FIRST=M00000 LAST=M00319 COUNT=320`). Insight: the
  pane is a *rendering*, not a transcript, and must never be used to verify what was sent.
- **`history_size` arithmetic as a truncation guard.** With `history-limit 50`, H₀=22 then 201 lines
  emitted: `history_size` went 22→48, so `H_now − H₀` reported 26. An 8× undercount, silent. Worse,
  the obvious guard `history_size == history_limit` **does not fire** — a saturating pane sits at
  48/50 while actively evicting. Hence the predicate in P7 uses `+ visible_rows`.
- **`pane_bracketed_paste` as a format variable.** Does not exist. The real one is
  **`bracket_paste_flag`** (`man tmux`, "Pane bracketed paste flag"), verified live across panes.
- **Escape as a universal stop key.** The user proposed it; measured false. On `dsh --profile
  dshline`, Escape is a no-op both idle and busy — it neither clears the composer nor stops
  generation. `C-u` clears; `C-c` stops while busy and **quits while idle**.
- **`pipe-pane` byte-offset anchoring.** Rejected, not disproven — structurally better than history
  offsets, but its main cost (dshline's spinner redraw volume) was never measured, and recommending
  an unmeasured design was declined. Listed in the PRD's Out of Scope.
- **`tmux-tools prompt` with a multi-line prompt returned empty output** while driving Codex during
  the gate — a live reproduction of the P7 defect this PRD fixes. Work around it by reading the pane
  with `tmux-tools capture` after the wait, not by trusting `prompt`'s return.

## Conventions and gotchas observed

- **Record homes and grammar.** `ADR-260809-1542-01` adopted the domain-modeling conventions
  wholesale. IDs are `<KIND>-YYMMDD-HHMM-NN` in **UTC** from `date -u`, never a repo scan and never
  the next sequential number. Issues live flat in `docs/issues/` — that is conformant, confirmed by
  the ADR's evidence section.
- **`PLANNING_CONTRACTS_CLI`** canonicalizes to
  `/Users/Shared/Data/work/Programming/ai/claude/user/skills/domain-modeling/scripts/planning-contracts.sh`.
  Quote that exact path; never invoke a bare `planning-contracts.sh`. `CONTRACT_MODE=legacy` here.
- **`CONTEXT.md` is the entire glossary** — single bounded context, no separate domain-model dir.
- **`tmux-tools kill` refuses across cwd.** It compares the pane's `@tt-cwd` to the current working
  directory. A `cd` into `docs/prd` earlier made a teardown fail with
  `refusing to kill pane %32 owned by another cwd`. `cd` back to the repo root, or pass `--any`.
- **Run-scoped native runtime identity is mandatory** (`~/.claude/CLAUDE.md`): resolve via
  `command -v`, canonicalize, require a regular executable, record path + SHA-256 + `--version`
  before *and* after every native invocation. See *Reference snippets* for this run's values.
- **Never `Read` a subagent's `.output` file** — it is the full JSONL transcript.

## User preferences and working style

- They want decisions surfaced as prose with rationale and honest alternatives, **never**
  `AskUserQuestion` — its canned options anchor and truncate the thinking. Recommend, then wait.
- They explicitly ask for critical pushback on their own proposals and have been right to: the
  `--ax-screen-reader` / `dshline` insight was theirs, and correcting their Escape hypothesis with
  measurements was received well. Do not soften findings to agree with them.
- They answer tersely — "agreed", "agreed with all". Treat that as a real decision and proceed; do
  not re-confirm.
- They notice slop. Two of the gate's sharpest findings existed because earlier drafts asserted
  things that were merely plausible. Measure before claiming.

## Open questions

1. Does the seven-packet granularity and dependency structure hold, or should packet 1 be split,
   the 2→1 edge flipped, or 2 merged into 6? *(This is the live blocker — the question was asked and
   is unanswered.)*
2. Should the PRD be committed now, and if so on this branch or a fresh one? The branch name
   `fix/prompt-send-keys-size-ceiling` no longer describes the scope, and the PRD supersedes that
   branch's own commit.
3. `dsh-tui-resume-id` — does the dsh rich surface need the prefixed identifier its flat surface
   needs, or a bare uuid as `agents.toml` documents? Ledger entry says `resolve-by: during-triage`,
   so it rides; do not resolve it to unblock minting.

## Next actions

1. Ask the user the step-4 question again if they have not answered it — it is quoted verbatim in
   *Current state* above. Wait for approval before minting anything.
2. Once approved, run `date -u` **once** and mint the batch into flat `docs/issues/` as
   `ISSUE-YYMMDD-HHMM-NN-<slug>.md`, assigning `-NN` in dependency order so `blocked_by` can
   reference IDs minted earlier in the same run.
3. Each issue: `status: draft`, `kind: issue`, `context: tmux-tools`, and
   `contract.references` declaring `requires` against `PRD-260918-0323-01` and the exact clause ids
   (`p1-transport`, `p2-surface`, `p3-state`, `p4-capture`, `p5-interrupt`, `p6-resume`,
   `p7-prompt`, `p8-smoke`). Write `## Agent Brief` per `triage/AGENT-BRIEF.md`. Never append a
   reciprocal list to the PRD.
4. Run the mint-time brief check (`triage/READINESS-GATE.md`, `mint` mode) per issue; `PASS — mint`
   flips `draft` → `needs-triage`.
5. Run step 7's breakdown adversary — **its trigger is met**, so it is not skippable. Vehicle is
   `/codex-researcher` in a fresh read-only session per `to-spec/PLAN-ADVERSARY.md`; verify Codex
   identity before and after and tear the pane down.
6. Ask the user about committing (open question 2).

## Reference snippets

Runtime identities recorded for this run (re-resolve and re-verify before any new native call):

```
claude  /Users/agent/.local/bin/claude -> /Users/agent/.local/share/claude/versions/2.1.274
        sha256 3509913f9d1576316c8845b88837f8fd3bbbcf26625833ac82cfb6b8985da94a   2.1.274
dsh     /Users/agent/.npm-global/bin/dsh -> .../lib/node_modules/@deepseek-ai/dsh/lib/bin.js
        sha256 0ff7f1d72c4e0cbe14001709c81e20a04b70464118a7f78568952988e28f2ac5   0.1.5-rc.1
codex   /Users/agent/.local/bin/codex -> .../releases/0.154.0-aarch64-apple-darwin/bin/codex
        sha256 4f85982624b3898c8991cb80c0981b2aa71070e3537046c9a95950318a95afcc   codex-cli 0.154.0
tmux 3.7c
```

The measurements the PRD deliberately does not carry, in case they need re-deriving:

```
claude (default TUI)          alternate_on=1  history_size=5, never grows
claude --ax-screen-reader     alternate_on=0  history 89 after a 120-line answer; -S - recovered 121 of 121
dsh --profile tui             alternate_on=1  history_size=0
dsh --profile dshline         alternate_on=0  history 98; -S - recovered ROW-001..ROW-120 (39 visible)
codex                         alternate_on=0  real scrollback

load-buffer + paste-buffer -p : 514,799 bytes / 5,200 lines -> ONE composer entry, 10ms
                                raw-sink check: 514,799 in, 514,799 out; 5,199 LF in -> 5,199 CR out
paste-buffer -p into bash     : no bracket markers emitted (tmux brackets only if the app asked)
per-pane history-limit        : `tmux set-option -p -t <pane> history-limit 50000` works on 3.7c
                                50,000 lines ~= 3.5 MB tmux server RSS (~70 bytes/line)
```

dshline footers, the basis for its `ready_regex` / `busy_regex` (must be re-validated by P8's smoke
before use):

```
idle:  ● ready · deepseek-flash · ↑7.9k ↓543 · CR 9.7% · ▏░░░░░░░ 8.2k/1.0M · alt-enter newline · ctrl-d quit
busy:  ◝  thinking · turn 4s · deepseek-flash · ↑16k ↓1.4k · CR 55.4% · ▏░░░░░░░ 9.0k/1.0M · ctrl-c stop · ctrl-d quit
```

Gate evidence is not persisted as files (no registry, and `PLAN-ADVERSARY.md` forbids prompt/report
pairs). The adversary's Codex session is resumable at
`codex resume 01a0b28c-fc36-7c91-b62e-d0322da1a80a` if its reasoning is needed; scratch files from
that review are at `/tmp/codex-research/codex-{prompt,response}-0c26bff4-*.md` and will be reaped by
macOS `/tmp` cleanup.
