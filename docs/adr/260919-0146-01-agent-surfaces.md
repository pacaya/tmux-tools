---
id: ADR-260919-0146-01
status: accepted
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: claude-rich-launch
      heading: Claude rich surface launch
  references: []
---

# Agent surfaces: a named rendering with a recorded identity

`ready_regex` and `ready_lines` were properties of an *agent*, but they are valid for one
*rendering* of that agent. Claude Code's full-screen TUI and its `--ax-screen-reader` rendering show
different chrome; `dsh`'s `--profile tui` is on the alternate screen while `--profile dshline` is
not. A pattern calibrated against one rendering silently fails, or false-matches, against another —
and nothing on a pane recorded which rendering it launched with, so no consumer could tell. An
agent therefore gains a map of named **surfaces**, `spawn-agent` selects and records one, and pane
state becomes a single classifier.

## Decision

`AgentSpec` (`core/src/agents/mod.rs`) gains `surfaces: BTreeMap<String, Surface>`. A `Surface`
carries launch arguments, `ready_regex`, `ready_lines`, `busy_regex`, whether it enables bracketed
paste, the key `interrupt` sends, whether that key quits when idle, and the resume-argument syntax
and identifier shape P6 needs. `ready_regex`/`ready_lines` are removed from `AgentSpec`.

Two agent-level fields name surfaces:

- **`default_surface`** — the surface `spawn-agent` resolves when no `--surface` is given. Exactly
  one per agent.
- **`pre_surface_rendering`** — the surface matching the **pre-surface rendering**, the rendering
  the agent launched with before surfaces existed. Readiness fields written in the agent-level
  (pre-surface) `agents.toml` shape bind here, and total pane resolution falls back here when a pane
  carries no surface record.

`pre_surface_rendering` is the field the P2 brief left unnamed. It is named after the PRD's own term
("pre-surface rendering") rather than after `default` or `legacy`, because the two concepts must
diverge: `default_surface` is promotable — P8's flat-default promotion moves it — while the
pre-surface rendering is a fixed fact about an agent that a promotable default would otherwise
retroactively reinterpret. An `agents.toml` may name a different `default_surface` and leave
`pre_surface_rendering` alone; the pre-surface readiness override then keeps governing the
pre-surface rendering and not the new default.

Surfaces merge by name and field like `access_profiles`, and every agent ends a load with exactly
one valid default and one valid pre-surface rendering (`normalize_surfaces`). An agent declared
without surfaces loads as one surface built from its agent-level readiness fields; that surface is
both its pre-surface rendering and its default, so a pre-surface `agents.toml` continues to load.

Built-ins: `claude` ships `rich` (its own interface, the pre-surface rendering and default; launch arguments per `## Claude rich surface launch`) and `flat`
(`--ax-screen-reader`); `codex`, `cursor` and `agy` each ship a single `default` surface carrying
today's readiness fields. No built-in surface ships validated `ready_regex`/`busy_regex` for a flat
rendering, bracketed-paste capability, a quit hazard, or resume syntax yet — those come from
`ISSUE-260918-0525-03`/`-05`/`-07`. Because the flat surfaces have no validated patterns, no
agent's default moves in this change.

`spawn-agent` gains `--surface <name>` (and only `spawn-agent`). It resolves the named surface or
the default, orders arguments `binary + surface.args + access.args + caller args`, and records the
resolved surface **unconditionally** as the `@tt-surface` tmux pane option. A pane whose surface
came with caller-supplied trailing arguments is additionally marked `@tt-surface-unvalidated`,
because those arguments can change the rendering out from under the record and pattern mismatch is
not a safety boundary. Arguments the surface supplies itself do not set the mark.

Pane-state determination moves behind one pure function, `core/src/idle.rs::classify(capture,
patterns) -> PaneState {Idle, Busy, Unknown}`, total over P2's truth table. Readiness consumers
compile a resolved surface's patterns once and classify each poll. When the classifier cannot decide
— no patterns, no surface, or `Unknown` — they fall back to idle detection; a `Busy` classification
suppresses it.

## Claude rich surface launch

1. The built-in `claude` `rich` surface has the launch arguments `--settings` `{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}` (two argv items). A pane started on it shows Claude's own interface in classic scroll mode on every host. The proof is `#{alternate_on}` `0` on tmux, `how: history` from `hc read --json` on Herdr, and on Orca (which reports no alternate-screen state) both argv items in the `claude` process command line of the pane.
2. `rich` stays the name, the `default_surface`, and the `pre_surface_rendering` of `claude`. The `flat` surface and the default-surface promotion rule do not change.
3. `CONTEXT.md` **Rich surface** and **Flat surface** are reworded so that full screen does not separate them: a rich surface shows the agent's own interface (prompt, status line, footer) and can be full screen (`dsh --profile tui`) or not (`claude` `rich`); a flat surface is a reduced rendering without that interface and is never full screen.
4. The P8 smoke (`tt-vendor-smoke`) launches the `claude` `rich` surface, fails when the pane is on the alternate screen, measures the `rich` declarations (bracketed paste, quit hazard, resume) against the current `claude`, and re-stamps its `validated_version`. The built-in `rich` surface declares no `ready_regex`/`busy_regex`, so the smoke notes that and does not fail on it.
5. README states the `rich` launch arguments, and that a `CLAUDE_CODE_NO_FLICKER=1` user setting does not apply to panes that `spawn-agent claude` starts on `rich`.
6. Out of scope: Claude sessions that tmux-tools does not start with a surface (a human `claude`, and agent-team teammates that Claude Code starts). They keep the user setting.

Reason: Claude is full screen only when `CLAUDE_CODE_NO_FLICKER=1` is set. A full-screen pane has no tmux scrollback, and Herdr reads it as `how: stitched`, where the History mark does not find the reply. The user settings file overrides the process environment, so the override goes through `--settings`, which merges with the user settings.

## Evidence

- P2 and P3 of `PRD-260918-0323-01`, the binding requirement text
  (`.planning-contracts/bundles/ISSUE-260918-0525-02/objects/`).
- The live renderings that motivated the split: measured in
  `docs/handoff/agent-surfaces-pane-io-2026-09-18-0004.md` — `claude` default `alternate_on=1`,
  `claude --ax-screen-reader` `alternate_on=0`; `dsh --profile tui` `alternate_on=1`,
  `dsh --profile dshline` `alternate_on=0`.
- Live probe, claude 2.1.322, 2026-11-10, user settings with `CLAUDE_CODE_NO_FLICKER=1`: plain `claude` and `env CLAUDE_CODE_NO_FLICKER=0 claude` give `alternate_on=1`; `claude --settings '{"env":{"CLAUDE_CODE_NO_FLICKER":"0"}}'` gives `alternate_on=0` with the user status line still drawn. In a private hc core, the full-screen pane reads `how: stitched` and the classic pane reads `how: history`.
- `ISSUE-260918-0525-02` acceptance items 1–9 are covered by the CLI-seam integration tests in
  `tests/integration.rs` (fixture-backed), the truth-table unit tests in `core/src/idle.rs`, and the
  built-in registry tests in `core/src/agents/builtin.rs`.

## Consequences

- Readiness is narrowed to one value of a three-valued classification rather than the whole
  determination, and idle detection is narrowed to the completion fallback for the cases the
  classifier cannot decide (no patterns, no surface, or `Unknown`) — suppressed by `Busy`, never a
  parallel definition of idleness. `CONTEXT.md` states both.
- A pane recorded with no surface is unambiguous: the tool records the resolved surface on every
  pane it spawns, so an absent record can only mean a pane created before surfaces existed, and it
  resolves to the pre-surface rendering. A record naming a surface the registry no longer has is
  likewise not reinterpreted — it resolves to no surface, which behaves as unknown.
- Every recorded surface is launch intent, not live state. Consumers acting on what the pane is
  *currently* doing must read the pane, not the record; the surface-unvalidated mark is the
  signal that the record may no longer describe the rendering.
- `dsh` is not a built-in. Its surfaces are documented in `README.md` for declaration in
  `agents.toml` (rich `--profile tui`, flat `--profile dshline`).
