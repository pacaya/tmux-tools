---
id: PRD-260918-0323-01
kind: prd
scale: epic
stakes: internal
gate: passed 2026-09-18
contexts: [tmux-tools]
terms: [Surface, Flat surface, Rich surface, Surface-unvalidated pane, Pane state, busy_regex, History mark, Turn observation, Completeness, Bracketed paste transport, Agent profile, Agent registry, Readiness, ready_regex, ready_lines, Idle detection, Pane registration, Target]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: p1-transport
      heading: P1 — One prompt is one submission
    - id: p2-surface
      heading: P2 — Surfaces are a registry concept with a selectable, recorded identity
    - id: p3-state
      heading: P3 — Pane state is a total three-valued function computed in one place
    - id: p4-capture
      heading: P4 — Capture reports its own ceiling
    - id: p5-interrupt
      heading: P5 — Interrupt never quits a pane it meant to interrupt
    - id: p6-resume
      heading: P6 — Resume is surface-owned and fails loudly
    - id: p7-prompt
      heading: P7 — Prompt returns from a pre-submission mark and reports completeness
    - id: p8-smoke
      heading: P8 — Vendor chrome is validated by a version-stamped smoke
    - id: verification
      heading: Verification and release
  references: []
---

# Agent surfaces and pane I/O

## Problem Statement

Three user-visible outcomes are wrong today.

`prompt` reports success and returns nothing. It locates the response by testing whether a
captured line contains the prompt text (src/cmd/prompt.rs:144-179); captured lines wrap at pane
width, so any prompt wider than the pane or containing a newline can never match and the result is
`""` while JSON still reports the full `prompt_sent` and a clean `reason`. Its unit tests all use
one-line prompts.

`capture --all` claims full history and cannot deliver it. A pane reporting `alternate_on=1` has no
tmux scrollback, so `capture-pane -p -S -` returns the pre-launch shell lines plus the visible
screen. SKILL.md:27, SKILL.md:70 and README.md:40 state otherwise.

`interrupt` can terminate the agent it was asked to interrupt. It sends `C-c` unconditionally, and
at least one agent rendering treats `C-c` as *stop* while generating and as *quit* while idle.

Underneath all three: every agent this tool drives has more than one rendering, the renderings
differ in whether scrollback exists and in what readiness chrome they show, and neither the registry
nor a pane's registration records which one is in use.

## Outcome and scope

`prompt`, `capture`, `interrupt` and `spawn-agent` behave correctly against full-screen agent TUIs,
and say so honestly when they cannot. The agent registry gains a **surface** — a named rendering of
an agent — which is selectable when an agent is spawned, recorded on the pane, and read by every
rendering-dependent behavior.

In scope: those four verbs, the launch path, the agent registry schema and its `agents.toml`
deep-merge, pane registration, `core/src/idle.rs` state determination, and the SKILL.md / README.md
statements that are currently false.

Owner: this repository's maintainer. No external consumers.

## Required behavior

### P1 — One prompt is one submission

`prompt` delivers its text through `tmux load-buffer` reading from stdin, then
`tmux paste-buffer -p -d` into a uniquely-named buffer, replacing chunked `send-keys -l`.

- The ceiling this removes is tmux's `send-keys` argument limit. The prompt still reaches the CLI
  as an argv positional and remains bound by the operating system's argv limit; no clause claims
  otherwise, and no new input path is added.
- `-p` is required: it is what makes an embedded newline a newline in the agent's composer rather
  than a submission. tmux brackets the buffer only when the target application has *currently*
  enabled bracketed-paste mode, so the one-submission guarantee is keyed to **live** pane state —
  `bracket_paste_flag`, read immediately before pasting — never to the surface's declaration, which
  records expected capability only. Three outcomes, each reported on stderr for raw and concise
  output and as a structured JSON field; the refusal exits non-zero:
  - live flag set: one-submission delivery, guaranteed.
  - live flag clear and the surface declares no bracketed-paste capability (a plain shell, or a
    pane `launch` created): deliver and report that one-submission delivery is unavailable. This is
    today's behavior.
  - live flag clear while the surface declares the capability: refuse multi-line text before
    `load-buffer` runs. Expectation and reality disagree — the agent is still starting, or the pane
    has fallen through to its keep-alive shell — and delivering would submit fragments.
- tmux's LF-to-CR translation is retained; `-r` is not used. Acceptance is stated over the text the
  target application reports having received, never over raw pane bytes, which this translation
  makes non-identical by construction.
- Prompt text must not persist in the user's paste ring. The buffer is deleted on every path after
  a successful `load-buffer`, including paste failure, and a transport error is reported unchanged
  by that cleanup.
- Text is passed on stdin to `load-buffer`, never written to a temporary file.
- `chunk_literal` and its tests are removed; commit 63e4660 is superseded.

### P2 — Surfaces are a registry concept with a selectable, recorded identity

`AgentSpec` gains a map of named surfaces, shaped like its existing `access_profiles`. Each surface
carries: launch arguments, `ready_regex`, `ready_lines`, `busy_regex`, whether it enables bracketed
paste, the key `interrupt` sends, whether that key quits the agent when the pane is idle, and the
resume-argument syntax and identifier shape P6 needs. Exactly one surface is the default.

Each agent has exactly one surface matching its **pre-surface rendering** — the one it launched
with before surfaces existed. For `claude` and `dsh` that is the rich surface; for an agent with a
single measured rendering (`codex`, `cursor`, `agy`) it is that agent's only surface. `dsh` is not a
built-in agent: its surfaces are declared in `agents.toml`, and this repository documents that
declaration rather than shipping it.

- `ready_regex` and `ready_lines` move onto the surface. They are valid for one rendering only.
- Surfaces are selectable on `spawn-agent` only. `launch --cmd` has no agent profile and therefore
  no surface: its panes resolve to no surface, and no surface flag is added to it.
- `spawn-agent` records the surface it **resolved**, unconditionally — whether or not one was named
  — in pane registration alongside `@tt-access` (core/src/names.rs:12-32). This departs from
  `@tt-access`, which is written only when `--access` is passed (src/cmd/spawn_agent.rs:84-85).
  Recording only on an explicit flag would make "no record" mean two different things, and the most
  common path — spawning on the default with no flag — would resolve to the wrong rendering.
- Surface resolution is total:
  - record present → that surface.
  - record absent → the agent's pre-surface rendering. Given unconditional recording, an absent record
    can only mean a pane created before surfaces existed, which was launched on that rendering.
    Never the current default, which is promotable and would retroactively reinterpret an old pane.
  - no agent at all → none, which behaves as unknown throughout.
- No predicate attempts to detect that an agent has exited. A pane held open by the keep-alive wrap
  shows a shell, which matches neither `ready_regex` nor `busy_regex`, so P3 already returns unknown
  and P5 already refuses. Comparing the pane's current command against the configured binary is not
  a usable test — a live `claude` pane reports its version string and a live `dsh` pane reports
  `node`, so it would classify running agents as gone.
- A recorded surface is launch intent, not live state. Caller-supplied arguments can change the
  rendering out from under it, and pattern mismatch is **not** a safety boundary — a different
  rendering may still match the recorded surface's `busy_regex`, which would let P5 send a key whose
  quit behavior belongs to a rendering that was never recorded. So a pane spawned with
  caller-supplied trailing arguments is recorded as **surface-unvalidated**. Arguments the surface
  itself supplies, including its resume syntax under P6, are validated by construction and do not
  set the mark. Behavior depending on what the pane is *currently* doing reads the pane, not the
  record — see P4.
- The default surface for every agent is its flat, non-alternate-screen rendering
  (`claude --ax-screen-reader`, `dsh --profile dshline`, codex as it already runs) **only once that
  surface ships a `ready_regex` and `busy_regex` measured against the live binary and stamped with
  its version per P8.** An agent whose flat surface has no validated patterns keeps its former
  default until they exist. Idle-only completion is not an acceptable basis for promoting a
  rendering to default.
- Readiness fields in an `agents.toml` written against the pre-surface schema bind to the surface
  matching the agent's pre-surface rendering, the rendering they were calibrated against — never to
  a newly promoted flat default. Such a file continues to load.
- The surface model is recorded as an ADR. Every term this PRD's frontmatter declares is minted in
  CONTEXT.md, and that edit states what the model narrows: **Readiness** (CONTEXT.md:24) becomes one
  value of P3's classification rather than the whole determination, and **Idle detection**
  (CONTEXT.md:30) becomes the fallback used when a surface supplies no patterns, not a parallel
  definition of idleness.

### P3 — Pane state is a total three-valued function computed in one place

`core/src/idle.rs` exposes one pure function taking a capture and a surface and returning idle,
busy, or unknown. No other module derives pane state. The function is total over these cases:

| surface patterns | capture matches | result |
|---|---|---|
| both present | ready only | idle |
| both present | busy only | busy |
| both present | neither | unknown |
| both present | both | unknown |
| `busy_regex` absent | ready | idle |
| `busy_regex` absent | not ready | unknown |
| `ready_regex` absent | busy | busy |
| `ready_regex` absent | not busy | unknown |
| neither present | any | unknown |

Unknown is a distinct outcome, never a synonym for busy. Consumers that need certainty (P5, P7)
branch on it explicitly.

### P4 — Capture reports its own ceiling

When `--all` is requested against a pane that cannot supply scrollback, `capture` says so.

- The determination reads **live pane state** (`alternate_on`), not the recorded surface. A pane
  that entered or left the alternate screen after launch is reported as it currently is.
- Raw and concise stdout are unchanged byte for byte, and carry the notice on stderr. JSON gains a
  structured field; that is an additive change to JSON stdout, and this clause does not claim JSON
  output is unchanged.
- The notice fires only for the structural case. A `--lines N` request that finds fewer than N
  lines stays silent and remains visible through the existing JSON `lines` count.
- SKILL.md:27, SKILL.md:70 and README.md:40 are corrected to state the limit.

### P5 — Interrupt never quits a pane it meant to interrupt

`interrupt` sends the key its target surface declares, and refuses when sending it could quit.

- A surface whose interrupt key quits while idle is refused on **any** result other than positively
  busy — unknown included. The refusal names what to do instead.
- A pane recorded as surface-unvalidated (P2) is refused for any surface carrying a quit hazard,
  whatever its state reads, because the rendering that would receive the key is not the rendering
  whose hazard was declared.
- A surface declaring no quit hazard sends its key on every result, preserving today's behavior for
  shells, builds and unregistered panes.
- Existing `--force` and `--any` guards are unchanged.

### P6 — Resume is surface-owned and fails loudly

`spawn-agent` accepts a session identifier to resume, maps it through the target surface's declared
resume syntax, and validates it against that surface's declared identifier shape before launching.

- A malformed or unrecognized identifier fails with a message and creates no pane. It must not
  launch an agent that lands in an interactive session picker, which a caller cannot distinguish
  from a hung pane.
- The README-documented `dsh` surfaces declare their measured identifier shapes: the rich surface
  (`--profile tui`) accepts `session-<uuid>` or a bare `<uuid>`, which it prefixes itself; the flat
  surface (`--profile dshline`) accepts only `dshline-<uuid>`. Both are stamped with the `dsh`
  version they were read from.
- tmux-tools does not read agents' private on-disk session stores. Resolving "the most recent
  session" is not provided.

### P7 — Prompt returns from a pre-submission mark and reports completeness

`prompt` returns output isolated by history offset, and reports two independent dimensions: whether
the agent was observed to take the turn, and whether what it returned is complete. Both are always
stated; either may be degraded, including both at once.

- **Mark.** The history mark is taken **before submission** and the result begins there. The mark is
  never advanced: `busy_regex` marks when the agent started, not where its echo of the prompt ended,
  and advancing to it would discard any response content emitted before the busy footer rendered.
  The result therefore always includes the agent's echo of the prompt, on every surface and every
  path. No echo-exclusion is promised, and none is attempted.
- **Turn observation.** Whether `busy_regex` was observed between submission and settle is reported
  as its own field. It says the agent took the turn; it makes no claim about where content begins.
- **Completeness.** A result is incomplete when the pane cannot supply everything after the anchor.
  Two conditions qualify, and either alone is enough: the pane has no scrollback per P4's live
  check, or history has evicted lines. Eviction is read from the pane's `history_collected`
  counter when the running tmux provides it: the counter is read at the mark and again at settle,
  and any increase is eviction. When the counter is unavailable, the pane counts as evicted whenever
  `history_size >= history_limit - history_limit / 10`. tmux frees the oldest tenth of history in
  one step when the limit is reached, so an evicting pane never sits below that floor; a pane that
  is merely near its limit is reported incomplete even if nothing was lost. Support for the counter
  is detected at runtime, never inferred from a version string.
- An empty result is never returned without both dimensions stated. Raw and concise carry them on
  stderr; JSON carries them as structured fields.
- `history-limit` is a session option in tmux and cannot be set per pane. When `launch` or
  `spawn-agent` creates a session, it sets that session's `history-limit` to 50000 before the first
  pane's command can emit output (lines evicted before the limit applies are unrecoverable).
  tmux-tools never changes `history-limit` on a session it did not create, and never sets it
  globally. Cost is roughly 70 bytes per stored line, stated so the value can be re-chosen. README
  documents the equivalent `~/.tmux.conf` setting for panes created in other sessions; truncation in
  those panes is reported through Completeness, never silent. tmux-tools does not write to that file.

### P8 — Vendor chrome is validated by a version-stamped smoke

Behavior depending on a third-party binary's rendering is validated by a separate, manually-run
smoke, never by `cargo test`.

- Per agent and surface: flat surface still non-alternate-screen, a large paste still arriving as
  one submission, `ready_regex` and `busy_regex` still matching their states, and whether the
  interrupt key quits when idle.
- It records the agent version validated, as `agents.toml` already does for codex and claude.
- It is the source of the validated patterns P2 requires before a flat surface becomes a default.
- Live agent binaries do not run in `cargo test`.

## Verification and release

**Primary seam — the CLI binary against a real tmux server.** `tests/integration.rs` already drives
`CARGO_BIN_EXE_tmux-tools` serialized behind its mutex; every behavior above is expressible there.

**Fixture — `tt-fake-tui`.** A second `[[bin]]`, reachable from tests as `CARGO_BIN_EXE_tt-fake-tui`,
that enables bracketed paste, reports the text it received, renders a configurable ready/busy
footer, and optionally switches to the alternate screen. The existing suite launches `bash`, which
has none of these properties, which is why none of the problems above are currently reachable. It is
gated behind a cargo feature via `required-features` so a normal install does not ship it; the
project's documented test command enables that feature, and a bare `cargo test` reporting fewer
tests is an accepted cost.

**New code seam — pane state classification** (P3). Pure, tmux-free, exercised directly against the
truth table.

Representative scenarios, each independently observable at the primary seam:

- A payload larger than tmux's `send-keys` ceiling, containing newlines, is reported by the fixture
  as one submission whose text matches what was sent.
- A transport failure after `load-buffer` leaves no prompt buffer in tmux, and reports the original
  error.
- A fixture pane on the alternate screen returns unchanged raw and concise stdout from
  `capture --all`, plus the notice on stderr and the field in JSON.
- Each row of P3's truth table.
- `interrupt` against a quit-hazard surface is refused while the fixture is idle **and** while its
  state is unknown, and sends the key while it is busy.
- `prompt` against a fixture that emits output before its busy footer appears returns that output,
  and reports the turn as observed. The same fixture with its busy footer suppressed returns the
  same content and reports the turn as unobserved.
- `prompt` against a fixture whose bracketed-paste mode is off, when the surface declares the
  capability, refuses multi-line text and leaves no buffer loaded; with a surface declaring no
  capability, the same text is delivered and reported as one-submission unavailable.
- `prompt` reports incomplete both when the fixture is on the alternate screen and when history has
  evicted lines, including together; eviction is detected on the counter path and on the floor
  path.
- `launch` or `spawn-agent` into a session tmux-tools did not create leaves that session's
  `history-limit` unchanged.
- `spawn-agent` with a malformed resume identifier fails without creating a pane.
- `spawn-agent` with no surface named records the surface it resolved, and a later verb reads that
  record rather than falling back.
- `interrupt` against a quit-hazard surface on a pane spawned with caller-supplied trailing
  arguments is refused while the fixture renders its busy footer.

Release is a version bump of a local CLI; no deploy step, and no rollback beyond git revert. P8's
smoke is re-run and re-stamped when a driven agent's version changes.

## Out of Scope

- Cursor and Antigravity surfaces. Neither was investigated for a flat rendering; they keep their
  current single-surface behavior until someone measures them.
- Reading agents' own transcript stores (`~/.claude/projects`, `~/.dsh/sessions`) as a history
  source.
- `pipe-pane` stream recording as an alternative history anchor.
- A file or stdin input path for `prompt` itself.
- Anchoring on rendered paste placeholders. A composer's rendering of pasted text is the agent's
  choice and is version-specific chrome; core/src/agents/builtin.rs already declines to bake that
  class of pattern into the binary.
- Making `capture --all` an error rather than a warning.
- Live agent binaries in CI.

## Dimension Scan

| Dimension | Verdict | Citation |
|---|---|---|
| problem/success | decided | ## Problem Statement, ## Outcome and scope |
| scope boundary | decided | ## Out of Scope |
| domain terms | decided | P2 — terms minted in CONTEXT.md, with stated narrowing of Readiness and Idle detection |
| architecture shape | decided | P2 — surface model, recorded as an ADR |
| stack | n/a | brownfield — Rust/clap/tmux are the existing conventions and this epic introduces none; the test fixture adds a bin target, not a dependency |
| data/schema | decided | P2 — registry schema, pane registration, and `agents.toml` backward compatibility |
| contracts/integrations | decided | P6 — `dsh` identifier shapes |
| UX (if UI) | decided | P2 — surface selection and the default-promotion bar; P4, P7 — stderr notices |
| testing/seams | decided | ## Verification and release |
| NFRs | decided | P7 — history-limit memory cost stated so the value can be re-chosen |
| ops envelope | n/a | local CLI; no deploy target, no environments, no monitoring |
