---
id: ISSUE-261107-0921-01
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: every verb reaches tmux only through a pane-level Host port whose only backend is tmux, with no change in behaviour
terms: [Host, Host backend, Host capabilities, Managed session, Bracketed paste transport, Layout intent, Tile]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p2-host-port
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p1-definitions
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p11-rollout
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p10-tests
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: contracts
---

## Agent Brief

**Outcome and scope:** Step 1 of P11: the `Host` port of P2 exists with the tmux backend as its only implementation, and every verb keeps its current output and exit behaviour.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P2 (the port, its operations, what stays in `core/src/layout.rs` and in the tmux backend, the public API); P1 (the terms `Host`, `Host backend`, `Host capabilities`); P11.1 (zero behaviour change); P10.1, only the sentence "The Host port is tested only through the CLI binary" (ISSUE-261107-0921-03 owns the rest of P10); `## Contracts` (`Host`, `HostCapabilities`).

**Current observable:** `src/cmd/*` and the `core` modules build tmux argv themselves and call `core/src/tmux.rs`; `tmux-tools-core` exports `tmux`, `stream`, `TmuxInvocation`, `with_invocation` and `set_global_invocation`.

**Required change:**
- Add the P2.1 port in `core/src/host/mod.rs` with every P2.1 operation except host state and notify, which ISSUE-261107-0921-02 adds. Move `core/src/tmux.rs` and all tmux command construction into `core/src/host/tmux/`.
- Approved contracts: `PRD-261107-0825-01` `## Contracts` — `Host` with the `Tmux` variant only (ISSUE-261107-0921-03 adds `Orca`), and `HostCapabilities` with all its fields. The tmux backend reports `tile`, `rebalance` and `size` true and the other fields false.
- The public API of `tmux-tools-core` follows P2.6, except `HostSelection`, which ISSUE-261107-0921-03 adds: the P2.6 tmux paths are removed, and `agents`, `format`, `idle`, `layout`, `names` and `target` stay public. Tests in `tests/integration.rs` that call removed core paths drive the CLI binary instead (P10.1). Their expected values do not change.
- `src/bin/tt-vendor-smoke.rs` keeps its own tmux commands (P2.2 exception).
- Add `Host`, `Host backend` and `Host capabilities` to `CONTEXT.md` with their code homes.

**Acceptance:**
- [ ] Given the isolated tmux server, when the existing suite runs, then every existing test passes and no expected value in `tests/integration.rs` changed (P2.4), observable at `cargo test --workspace --features test-fixtures --no-fail-fast` and in the diff.
- [ ] `core/src/tmux.rs` does not exist. In `src/cmd/`, `src/main.rs`, `src/util.rs` and `core/src/` outside `core/src/host/tmux/`, no code starts the tmux process, builds a tmux argv in any form (full subcommand name or alias), or names a tmux option, including `@tt-*` pane options and the `@tt-tiled` window option; Outside `core/src/host/tmux/`, code refers to no item of that module except the tmux backend type: every match of `rg -n 'tmux::' src/cmd src/main.rs src/util.rs core/src --glob '!core/src/host/tmux/**'` names only the tmux backend type, and `rg -n '"@tt-' src/cmd src/main.rs src/util.rs core/src --glob '!core/src/host/tmux/**'` finds no match. `src/bin/tt-vendor-smoke.rs` is exempt (P2.2); `src/bin/tt-fake-tui.rs` builds no tmux command.
- [ ] At no public path of `tmux-tools-core` (the crate root or any public module) can a caller reach `TmuxInvocation`, `with_invocation`, `set_global_invocation`, the tmux command runner, `DEFAULT_TMUX_COMMAND_TIMEOUT`, `stream::capture_ansi`, or any other item of `core/src/host/tmux/` except the tmux backend type. No public module named `stream` exists, and no `tmux` module exists at the crate root. The `Host` port, `Host`, `HostCapabilities`, the tmux backend type, and the modules `agents`, `format`, `idle`, `layout`, `names` and `target` are public, observable in `cargo doc -p tmux-tools-core` output.

**Verification and release:** `cargo test --workspace --features test-fixtures --no-fail-fast` passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir>`), and the temp dir is empty afterwards. A failure that survives isolation and that this change did not introduce is reported, not fixed. No release of its own.

**Out of scope:** Host selection and `TMUX_TOOLS_HOST`; new verbs, JSON fields and exit 3; the Orca backend; moving `SilverBond` to the port.

## Triage Notes
