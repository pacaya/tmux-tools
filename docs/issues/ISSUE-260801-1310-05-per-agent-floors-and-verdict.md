---
id: ISSUE-260801-1310-05
kind: issue
category: enhancement
status: draft
brief_contract: agent-brief.v3
context: tmux-tools
summary: Per-surface geometry floors in the registry, effective-floor resolution, below_floor verdict, and status geometry
terms: [Floor, Effective floor, Advisory floor, Surface, Pre-surface rendering, Agent profile, Readiness]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: requires
      repository: tmux-tools
      record: ADR-260919-0146-01
    - relation: requires
      repository: tmux-tools
      record: PRD-260918-0323-01
      clause: p2-surface
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-04
---

## Agent Brief

**Outcome and scope:** Each surface in the agent registry may declare a geometry floor (`min_cols`/`min_rows`). A pane's floor resolves from its `@tt-agent` and `@tt-surface`; a window's effective floor is the componentwise maximum over its panes. `launch`, `spawn-agent` and `status` report whether the window is below that floor. This issue detects and reports only. It does not relocate, refuse or repair.

**Required sources:**
- `tmux-tools:PRD-260801-0656-01`: § Floor: per surface, in the registry (resolution table, seeds, default pair, effective floor, coupling note); § Module shape (`floor_violations`); § Reporting surface (fields, window-scoped rule, `status`); § Accepted limits (advisory floor); § How to run the suite.
- `tmux-tools:ADR-260919-0146-01`: the surface model, merge by name and field, `pre_surface_rendering`, and how agent-level fields bind.
- `tmux-tools:PRD-260918-0323-01#p2-surface`: total surface resolution from a pane registration.

**Current observable:** `Surface` and `SurfaceConfig` (`core/src/agents/mod.rs`) have no floor fields. `SurfaceConfig` and `AgentConfig` accept unknown keys, so `min_cols`/`min_rows` written in `agents.toml` today are silently ignored. `launch` and `spawn-agent` output has no `below_floor` field. `status` reports no window geometry and no floor verdict.

**Required change:**
- Floor fields:
  - `Surface` and `SurfaceConfig` gain optional `min_cols` and `min_rows`.
  - `merge_surface` merges them per field, as it merges the other surface fields.
  - Agent-level `min_cols`/`min_rows` in the pre-surface `agents.toml` shape bind to the agent's `pre_surface_rendering`, as agent-level `ready_regex`/`ready_lines` do. This applies both when the file merges over a built-in and when it declares a new agent without surfaces. They bind to `pre_surface_rendering`, never to a `default_surface` that differs from it.
- Seeded built-in surfaces:
  - `codex` `default`: 58×9.
  - `cursor` `default`: 36×9.
  - `claude` `rich`: 26×16.
  - `claude` `flat` and every other surface are unseeded.
  - The global default pair is 58×16.
- Pane floor resolution:
  - Get the surface through the registry's `resolve_surface` from `@tt-agent` and `@tt-surface`:
    - A recorded surface resolves to that surface.
    - An absent record resolves to `pre_surface_rendering`.
  - Read the floor from the merged surface. An axis the surface declares keeps its declared value. An axis it does not declare takes the global default for that axis.
  - An unknown agent, or a recorded surface the registry does not have, gives the default pair.
  - A pane with no `@tt-agent` contributes no floor, but it still counts when the window is measured.
- Effective floor and verdict:
  - The effective floor is the componentwise maximum over the floors of the panes in the window.
  - `below_floor` is true when the window's measured `min_pane_cols` or `min_pane_rows` is below the matching axis of the effective floor.
  - A window with no floor-contributing pane reports `below_floor: false`.
  - Resolution and verdict are pure, given measured geometries, pane registrations and a registry. They live in the core layout module that ISSUE-260801-1310-04 creates.
  - This issue builds `floor_violations` in that module (PRD-260801-0656-01 § Module shape: "reports panes below a given effective floor"). `below_floor` derives from `floor_violations`.
- For the new pane, placement takes the pane's agent and resolved surface as inputs (PRD-260801-0656-01 § Ownership). It does not read them back from tmux.
- `launch` and `spawn-agent` add `below_floor` for the destination window in `concise` and `json`. `raw` is unchanged.
- `status` adds the measured geometry and the verdict of the window that holds its resolved pane:
  - The fields appear in `concise` and `json` only.
  - The JSON names are `min_pane_cols`, `min_pane_rows` and `below_floor`.
  - The concise key spelling is the implementer's choice, but the minima must not be spelled `min_cols`/`min_rows` (PRD-260801-0656-01 § Reporting surface keeps reported names distinct from registry names).
  - `status --format raw` stays byte-for-byte unchanged, and so does the not-in-tmux output.
- README.md's agent-registry documentation must state:
  - The surface fields `min_cols`/`min_rows`, and that agent-level values bind to the pre-surface rendering.
  - That the 58×16 default is an assumption, not a measurement.
  - That each shipped floor was measured against the readiness rules of one surface: `codex` `default`, `cursor` `default`, and `claude` `rich` with the opt-in status-line pattern.
  - That a change to that surface's `ready_regex`/`ready_lines` makes its floor invalid, and that the tool does not enforce this coupling.
  - That the 2026-08-01 measurement recorded no agent versions, and that `validated_version` does not cover floors.

**Acceptance:**
- [ ] Given an `agents.toml` that declares `min_cols`/`min_rows` under a surface table, when the registry loads it, then that surface's resolved floor uses the declared values. Observable at the registry loaded from `agents.toml` text. Fails before this change, because the keys are ignored.
- [ ] Given an `agents.toml` that sets only `min_cols = 50` on `codex`'s `default` surface, when its floor resolves, then it is 50×9, not 50×16. The mirror case (only `min_rows`) keeps the shipped 58 columns. Observable at the same seam.
- [ ] Given a surface with no shipped floor (for example a new agent's surface) that declares one axis only, when its floor resolves, then the other axis is the global default.
- [ ] Given an `agents.toml` in the pre-surface shape that sets agent-level `min_cols`/`min_rows` for an agent whose `default_surface` and `pre_surface_rendering` differ, when it loads, then the values apply to the `pre_surface_rendering` surface only, and the default surface's floor is unchanged.
- [ ] Given an `agents.toml` that declares a new agent with no surface tables and only an agent-level `min_cols`, when it loads, then that agent's floor resolves to the declared columns × 16.
- [ ] Given an agent whose `default_surface` and `pre_surface_rendering` differ and declare different floors, when the floor of a pane with that `@tt-agent` and no `@tt-surface` resolves, then it is the `pre_surface_rendering` floor.
- [ ] Given `claude` panes with no user floor, when floors resolve, then a pane recorded `@tt-surface=flat` gets 58×16 and a pane recorded `rich` gets 26×16. A `claude` pane with no `@tt-surface` resolves to the `rich` (pre-surface) floor 26×16.
- [ ] Given a pane whose `@tt-agent` the registry does not know, or whose `@tt-surface` names a surface the agent does not have, when its floor resolves, then it is 58×16, with no error.
- [ ] Given a set of panes that includes a pane with no `@tt-agent`, when the effective floor is computed, then it is the componentwise maximum of the agent panes' floors only. When the agentless pane is the window's smallest pane and is below that floor, the verdict is below floor. This is unit-tested without a tmux server.
- [ ] Given the built-in registry, then `codex.default` is 58×9, `cursor.default` 36×9, `claude.rich` 26×16, and `claude.flat` resolves to 58×16.
- [ ] Given placements on the private test server, when `launch` and `spawn-agent` place into a destination window below its effective floor, then `--format json` reports `below_floor: true` and `concise` shows the verdict. When they place into a window with room, the result is `below_floor: false`. Observable at the CLI seam. Both directions are asserted for both verbs. No such field exists before this change. The `spawn-agent` cases use an injected `agents.toml` (via `XDG_CONFIG_HOME`) whose agent runs `tt-fake-tui` under the `test-fixtures` feature. They need no installed vendor agent, and they fail (never skip) when the behavior is absent.
- [ ] Given a window that holds no `@tt-agent` pane and is crowded below 58 columns, when `launch --format json` places into it, then the output has the `below_floor` field with value `false`. Observable at the CLI seam.
- [ ] Given a window that holds a pane registered with `@tt-agent` and is crowded by direct `tmux split-window` calls, with no spawn involved, when `status` points at it, then it reports the window below floor. When `status` points at a roomy window that holds the same registration, it reports the window not below floor. Observable at the CLI seam in `concise` and `json`. In `json` the fields are named `min_pane_cols`, `min_pane_rows` and `below_floor`.
- [ ] `status --format raw` output is byte-for-byte identical before and after this change for the same pane.
- [ ] README.md's agent-registry documentation states: the surface fields `min_cols`/`min_rows`, and that agent-level values bind to the pre-surface rendering; that the 58×16 default is an assumption; the surface each shipped floor was measured against, and that the coupling with `ready_regex`/`ready_lines` is not enforced; that the measurement recorded no agent versions, and that `validated_version` does not cover floors. Structural check on README.md. Fails before this change.

**Verification and release:** `cargo test --workspace --features test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A failure that survives isolation and that this change did not introduce is reported, not fixed.

**Out of scope:**
- Relocating a pane, or refusing a placement, when the floor is breached.
- A per-invocation floor override flag.
- A pane-count cap.
- Asserting the measured readiness cliffs in the suite, or re-measuring floors.
- Geometry columns in `list`.
- Changing where panes land. The tile default belongs to ISSUE-260802-0128-01.
- Enforcing the coupling between floor and readiness rules.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness (floors per surface, maintainer Q1/Q2); prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied.
