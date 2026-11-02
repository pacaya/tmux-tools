---
id: ISSUE-260802-0628-01
kind: issue
category: enhancement
status: ready-for-agent
brief_contract: agent-brief.v3
context: tmux-tools
summary: every pane this tool creates registers @tt-name, auto-derived from the command when --name is absent
terms: [Ownership, Pane registration]
contract:
  schema: planning-contracts.record.v1
  clauses: []
  references:
    - relation: requires
      repository: tmux-tools
      record: PRD-260801-0656-01
    - relation: implementation_after
      repository: tmux-tools
      record: ISSUE-260801-1310-01
---

## Agent Brief

**Outcome and scope:** Every pane `launch` or `spawn-agent` creates carries a non-empty `@tt-name`:
the `--name` value when passed, otherwise a name derived from the launched command plus a random
suffix (`npm-a3f2`), with a fixed stem when the command yields no usable word. Both verbs report the
registered name. The ownership predicate, `--any`, `--force`, and the cross-cwd rule do not change;
the guarded verbs' resulting reach is the widening accepted in PRD § Persistent shape — one resolved
pane (explicit `--target`, or implicit within the caller's scope) in the caller's own cwd that this
tool created. No placement or tiling change.

**Required sources:** tmux-tools:PRD-260801-0656-01 — § Persistent shape (always-registered name,
derivation, accepted widening), § Ownership (predicate unchanged), § Documentation (installed skill
copy and its release step), § Out of Scope (no lineage-derived names), § How to run the suite
(closing check).

**Current observable:**
- A flagless `launch --cmd <cmd>` pane has no `@tt-name`; concise prints `pane=<id>` with no name,
  and `--format json` prints `"name":null`.
- A flagless `spawn-agent` pane has no `@tt-name`; concise and json render the name as absent.
- `kill` without `--any` on a flagless-`launch` pane is denied with "not created by tmux-tools",
  from the creating cwd and from any other cwd alike.

**Required change:**
- Both spawning verbs always write `@tt-name`. With `--name`, the value passed, unchanged. Without it,
  a name derived from the command being launched (the user's command, for `spawn-agent` the agent's
  resolved command line — never the keep-alive wrap) plus a random suffix; a fixed stem when no
  usable word can be taken. A derived name must resolve back to its pane through `--target <name>`.
- Derivation: the stem is the basename of the command's first word (for `spawn-agent`, the first
  word of the resolved agent command line — never the keep-alive wrap), reduced to characters valid
  in a tmux option value and a `--target` name. A first word that yields no letter or digit is
  unusable and gets the fixed stem `pane`.
- Every pane either verb creates without `--name` also carries `@tt-launched-at`, and `@tt-cwd` when
  the cwd is readable, beside the derived `@tt-name` (PRD § Persistent shape).
- The random suffix alone must make concurrent same-command collisions negligible (`names::set`
  checks uniqueness then writes, non-atomically). Re-deriving after a rejected write is only an
  addition for a name already taken.
- Concise and json output of both verbs render the registered name, not the `--name` flag value.
  `--format raw` stays the bare pane id.
- README.md, the repo SKILL.md, and the installed SKILL.md copy stop implying `--name` is required
  for a pane to be named or addressable, and state that a name is derived when `--name` is absent.
  The installed copy is `/Users/Shared/Data/work/Programming/ai/claude/user/skills/tmux-tools/SKILL.md`
  (the same file `~/.claude/skills/tmux-tools/SKILL.md` resolves to), in the separate repository
  `/Users/Shared/Data/work/Programming/ai/claude` (the claude-settings repo). Its edit is committed
  on branch `agent/auto-tile-layout` in that repo and pushed to its lane, without modifying the live
  working tree the symlink resolves to.

**Acceptance:**
- [ ] Given the private test server, when `launch --cmd <shell>` runs with no `--name`, then the
      created pane's `@tt-name` read back from tmux is non-empty (unset before this change),
      observable at the CLI binary plus `tmux show-options`.
- [ ] Given the private test server, when `launch --cmd 'cargo --version'` runs with no `--name`, then the
  pane's `@tt-name` starts with `cargo-`; fails before this change.
- [ ] Given the private test server, when a flagless `launch` runs, then the created pane read with
      `tmux show-options -p` contains `@tt-name`, `@tt-launched-at` and `@tt-cwd`. Observable at the
      CLI binary plus tmux.
- [ ] Given the private test server and the `tt-fake-tui` registry below: (a) a flagless
      `spawn-agent` of the `tt-fake-tui` agent gives an `@tt-name` starting with `tt-fake-tui-`;
      (b) a `launch --cmd` whose first word yields no letter or digit gives an `@tt-name` starting
      with `pane-`. In both cases a guarded verb run with `--target <reported name>` from the
      creating cwd acts on exactly that pane. Observable at the CLI binary plus tmux.
- [ ] Given an `agents.toml` registry injected through `XDG_CONFIG_HOME` with a `tt-fake-tui` agent
      (test-fixtures feature, no installed vendor agent), when `spawn-agent <that agent>` runs with no
      `--name`, then the created pane's `@tt-name` is non-empty; the test fails, never skips, when the
      behavior is absent. Observable at the CLI binary plus tmux.
- [ ] Given that registry, when a flagless `spawn-agent --format json` runs, then `name` is non-null
      and equals the pane's `@tt-name` read back from tmux; and a flagless concise `spawn-agent`
      prints `name=<v>` where `<v>` equals its pane's `@tt-name`. Observable at stdout plus tmux.
- [ ] Given a flagless `launch --format json`, then `name` is non-null and equals the created pane's
      `@tt-name` read back from tmux; and a flagless concise `launch` prints `name=<v>` where `<v>`
      equals its pane's `@tt-name`. Observable at stdout plus tmux.
- [ ] Given three concurrent flagless `launch` invocations of the same command into one window of
      the private server, then all three exit 0 and the three created panes' `@tt-name` values are
      pairwise distinct. Use three. Observable at exit codes plus tmux.
- [ ] Given a pane created by a flagless `launch` from cwd A, when `kill --target <its pane id>`
      runs from cwd A without `--any`, then it exits 0 and the pane is gone (denied with "not created
      by tmux-tools" before this change). Observable at exit code plus tmux.
- [ ] Given a pane created by a flagless `launch` from cwd A, when `kill` targeting it runs from a
      different cwd B without `--any`, then it is denied with a message naming cwd A as the other
      cwd, not "not created by tmux-tools". Observable at exit code and stderr.
- [ ] Preserved: `launch --name foo` registers exactly `foo`; a second `launch --name foo` while the
      first pane lives fails with the existing name-collision error; `launch --format raw` stdout is
      the bare pane id, byte-for-byte as before.
- [ ] Reviewer read check over README.md, the repo SKILL.md, and the installed SKILL.md copy on its
      `agent/auto-tile-layout` branch: none states or implies that `--name` is required for a pane to
      be named or addressable, and the `launch` and `spawn-agent` entries say a name is derived when
      `--name` is absent.

**Verification and release:** Behavioral criteria run as integration tests through the CLI binary
against the private tmux server from ISSUE-260801-1310-01. `cargo test --workspace --features
test-fixtures` passes against an isolated tmux server (PRD-260801-0656-01 § How to run the suite). A
failure that survives isolation and that this change did not introduce is reported, not fixed.
Release action: the installed SKILL.md edit is committed on branch `agent/auto-tile-layout` in the
claude-settings repo and pushed to its lane; this issue closes on that branch commit. The control Mac
lands that branch once, after the whole auto-tile epic's binary is installed (PRD-260801-0656-01
§ Documentation).

**Out of scope:**
- Tiling, floors, relocation, and the `@tt-tiled` marker.
- Moving registration into the core crate (ISSUE-260801-1310-04).
- Extracting the ownership predicate (ISSUE-260801-1310-02).
- Layout and tiling documentation (ISSUE-260802-0128-01).
- Names derived from the calling pane's name (lineage).
- Any change to `--any`, `--force`, or the cross-cwd rule.
- Renaming or backfilling panes that already exist.

## Triage Notes

- 2026-11-02: reminted to agent-brief.v3 after dispatch readiness; prior gate rounds in Git history.
- 2026-11-02: mint-check repair applied.
