---
id: ISSUE-261107-0921-02
kind: issue
category: enhancement
status: needs-triage
brief_contract: agent-brief.v3
context: tmux-tools
summary: self and notify verbs, exit 3 for unsupported optional verbs, and the host, host_state and submit JSON fields, on tmux
terms: [Host, Host capabilities, Host state, Pane registration, Pane state]
contract:
  schema: planning-contracts.record.v1
  clauses:
    - id: acceptance
      heading: Agent Brief
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p3-selection
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p5-verb-classes
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p6-host-state
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p7-notify
    - relation: requires
      repository: tmux-tools
      record: PRD-261107-0825-01
      clause: p8-self
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
      clause: contracts
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-261107-0921-01
---

## Agent Brief

**Outcome and scope:** Step 2 of P11, on tmux: the `self` and `notify` verbs, capability gating with exit 3, and the new JSON fields.

**Required sources:** `tmux-tools:PRD-261107-0825-01` — P3.5 (`host` in JSON); P5.3–P5.5 (exit 3, silent auto-tile skip, required verbs never exit 3); P6.1, P6.3, P6.4 (Host state values, `host_state`, `submit` without `submit_confirmation`); P7.1, P7.2, P7.4 (`notify`); P8 (`self`); P1 (the term `Host state`); P11.2; `## Contracts`.

**Current observable:** `tmux-tools self` and `tmux-tools notify` are unknown subcommands. `--format json` results carry no `host`, `host_state` or `submit`. No verb exits 3.

**Required change:**
- Add the host state and notify operations to the port. On tmux, host state is always `null` and the `notify` capability is true. Notify on tmux follows P7.2.
- Add `src/cmd/self_.rs` and `src/cmd/notify.rs` per P7 and P8.
- Every `--format json` result carries `host`. `status`, `list`, `wait-idle` and `prompt` carry `host_state`, and `prompt` carries `submit: "assumed"` (P6.4).
- Capabilities are read on each invocation. `rebalance`, `--split tile` and `--size` exit 3 per P5.3 when their capability is false. The automatic tile after a spawn is skipped per P5.4 when `tile` is false.
- A test-only capability override, compiled only with the `test-fixtures` feature, can set any tmux capability false for one invocation, so tests can reach exit 3, the P5.4 skip and P7.4 on tmux.
- Approved contracts: `PRD-261107-0825-01` `## Contracts` — `HostCapabilities.notify`, Verb `notify`, Verb `self`, Changed JSON results, Exit code 3.
- Add `Host state` to `CONTEXT.md` with its code home.

**Acceptance:**
- [ ] Given a pane in the isolated tmux server that `spawn-agent` or `launch --name` created, when `self --format json` runs in that pane, then stdout is `{"host":"tmux","driven":true,"name":<its name>,"agent":<its agent or null>}`, observable at stdout.
- [ ] Given an unregistered pane, `self --format json` reports `driven: false` and `name: null`. Given no `$TMUX`, it reports `host: "tmux", driven: false`.
- [ ] Given a pane whose session has an attached client, when `notify --title T --body B --target <name>` runs, then the bytes written to the client's tty contain `ESC P tmux; ESC ESC ]777;notify;T;B BEL ESC \` and the verb exits 0, observable at that tty and the exit code.
- [ ] Given a pane whose session has no attached client, `notify --title T --body B --target <name>` exits 0 and stderr is `notify: not delivered on tmux`.
- [ ] Given a session with an attached client and the override setting `notify` false, `notify --title T --body B --target <name>` exits 0, stderr is `notify: not delivered on tmux`, and the bytes written to the client's tty contain no `]777;notify;` sequence.
- [ ] Given `TMUX_PANE` unset, `notify --title T --body B` without `--target` exits 1 and stderr names `TMUX_PANE` (P7.1).
- [ ] Given a pane whose session has an attached client, `notify --title T --body B` without `--target`, run in that pane, exits 0 and the bytes written to the client's tty contain the P7.2 sequence.
- [ ] Given `--title 'a;b'` and a body that contains ESC and BEL, the delivered sequence carries the title `a,b` and the body without bytes 0x00–0x1F or 0x7F (the `notify-tmux.sh` sanitisation that P7.2 adopts).
- [ ] Given the override sets `rebalance` false, `rebalance` exits 3, stderr is `unsupported: rebalance on tmux`, and the window layout is unchanged. Given `tile` false, `launch --split tile` exits 3, stderr is `unsupported: --split tile on tmux`, and no pane is created. Given `size` false, `launch --size <n>` exits 3, stderr is `unsupported: --size on tmux`, and no pane is created. `spawn-agent --split tile` and `spawn-agent --size <n>` behave the same way.
- [ ] Given a window that holds a pane registered from the launch's cwd and has no `@tt-tiled` option: with `tile` true, a flagless `launch` run in that window retiles it and sets `@tt-tiled`; with the override setting `tile` false, the same launch exits 0, prints nothing about tiling on stderr, does not retile, and the window still has no `@tt-tiled` option.
- [ ] For every verb that supports `--format json`, the result has `host: "tmux"`. `status`, `list`, `wait-idle` and `prompt` results have `host_state: null`, and `prompt` results have `submit: "assumed"`.
- [ ] Every assertion that existed in `tests/integration.rs` before this issue still passes; only the added JSON keys differ.

**Verification and release:** `cargo test --workspace --features test-fixtures --no-fail-fast` passes with the decoy environment (`TMUX=/nonexistent/tt-decoy,1,0 TMUX_PANE=%999999 TMUX_TMPDIR=<empty temp dir>`), and the temp dir is empty afterwards. The override is absent from a build without `test-fixtures`. No release of its own.

**Out of scope:** `TMUX_TOOLS_HOST` and host detection beyond today's tmux behaviour; the Orca backend; changes to the harness `notify-tmux.sh` and other hooks.

## Triage Notes
- 2026-11-07 dispatch readiness: not promoted; stays `needs-triage`. Unresolved maintainer choices: Q36 (notify write path, P7.2), Q37 (notify client when several are attached), Q38 (standalone BEL after the notify sequence), Q39 (where `list --format json` carries `host` and `host_state`, P3.5 / `## Contracts`).
