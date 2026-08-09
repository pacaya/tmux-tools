# Code Review: extract-library

**Branch:** `extract-library` vs `main`
**Files changed:** 2 files (core/src/agents/builtin.rs, core/src/agents/mod.rs; +109/-2)
**Date:** 2026-06-10
**Reviewers:** Claude Opus (high-effort recall review; single-agent, dual-review report format)
**Scope note:** `tmux-tools-core` is a shared library; the primary downstream consumer `/Users/Shared/Data/work/Programming/SilverBond` was traced and is already written against this new API (`spec.interaction_patterns`, `registry_interaction_pattern(&agents::InteractionPatternSpec)`). It reads spec fields rather than constructing `AgentSpec`, so this change does not break its compilation.

---

## MEDIUM

### M1. `kind` is a stringly-typed field with its valid set duplicated across crate boundaries (FIXED)
**Severity:** MEDIUM — silent cross-crate failure, but only triggers on a coordinated kind-set schema change, not in steady state.
**Files:** `core/src/agents/mod.rs:30-36,208-217,355-358`, `SilverBond/src/driver.rs:20-29,33-40,137-145,1421-1427`
**Description:** The set of valid interaction-pattern kinds (`permission`, `auto_respond`, `subagent_active`, `destructive_warning`) is hardcoded as string literals in three places across two crates: validated via a hand-written match + `bail!` in core (`mod.rs:355-358`), re-mapped in SilverBond's `registry_interaction_pattern` (`driver.rs:137-145`), and reproduced in `InteractionKind::event_type()` (`driver.rs:33-40`). `kind` survives as an unconstrained `String` on both `InteractionPatternConfig` (`mod.rs:208-217`) and `InteractionPatternSpec` (`mod.rs:30-36`). If core accepts a kind SilverBond's match hasn't been updated for, the `_ => return None` at `driver.rs:144` silently drops the pattern — a registry-configured prompt (including the safety-relevant `destructive_warning`) never fires, with no error anywhere.
**Fix:** Introduce a single source of truth in `core` and make the downstream match compiler-enforced.

- **Core:** Define `pub enum InteractionKind` in `core/src/agents/mod.rs` with `#[derive(Deserialize, Clone, Debug, Eq, PartialEq)]` + `#[serde(rename_all = "snake_case")]` and variants `Permission`, `AutoRespond`, `SubagentActive`, `DestructiveWarning`. Change `InteractionPatternConfig.kind` (`mod.rs:208-217`) and `InteractionPatternSpec.kind` (`mod.rs:30-36`) from `String` to `InteractionKind`. Delete the hand-written match + `bail!` at `mod.rs:355-358` (serde now rejects unknown kinds at TOML parse with a contextualized error). Re-export `InteractionKind` from the crate root / `agents` module so consumers can name it.
- **SilverBond:** Replace the string-literal match in `registry_interaction_pattern` (`driver.rs:137-145`) with a match on `agents::InteractionKind` — the compiler then forces exhaustiveness and the silent `_ => return None` arm is removed (genuinely unreachable patterns excepted). SilverBond's own `InteractionKind` (`driver.rs:20-29`) carries data (`AutoRespond { response }`), so it stays as a thin mapping target driven by the core enum rather than being replaced; update `event_type()` (`driver.rs:33-40`) to derive its string from the core enum if practical. Update the one constructed test literal at `driver.rs:1421-1427` (`kind: "auto_respond".to_owned()` → `agents::InteractionKind::AutoRespond`).

### M2. One invalid `kind` in user `agents.toml` fails the entire registry load, and the downstream consumer swallows the error — silently dropping *all* registry customization (FIXED)
**Severity:** MEDIUM — silent + total customization loss, but only reachable via a user-typo'd `agents.toml`, not a default or remote-data path.
**Files:** `core/src/agents/mod.rs:113,272,305,325,342-349,357`, `SilverBond/src/driver.rs:107,156,753,776,786`
**Description:** A typo'd `kind` in any one `[[agent.interaction_patterns]]` entry fails the whole registry: it `bail!`s at `core/src/agents/mod.rs:357` and `?`-propagates via `merge_existing_agent` (`mod.rs:305`) / `agent_from_config` (`mod.rs:325`) up to `Registry::load` (`mod.rs:113`). This is newly reachable on the existing-builtin-override path — before `627d77d`, `merge_existing_agent` returned `()` and never parsed patterns, so overriding a builtin (e.g. `[codex] ready_lines = 4`) could not fail load. SilverBond then swallows the `Err` at `driver.rs:107,156,753,776,786`, silently reverting every agent to hardcoded defaults — losing capabilities, binary overrides, custom agents, and interaction patterns (including the safety-relevant `destructive_warning`) with no diagnostic. Two orthogonal defects: one bad entry nukes the whole registry (core), and the failure is silent (consumer). Note: M1's serde-enum fix does not resolve this — it relocates the whole-load failure from the `bail!` to serde's atomic `toml::from_str` (`mod.rs:272`).
**Fix:** Make the load partial-and-reporting in core, then surface the warnings in SilverBond.

- **Core:** Change `load_with_user_path` (and the public `Registry::load` contract) to return the registry together with a `Vec` of non-fatal load warnings (e.g. `struct LoadWarning { agent: String, detail: String }`), or accept a warning sink. Rework `interaction_patterns_from_config` (`mod.rs:342-349`) and the per-agent merge so an invalid interaction pattern (or invalid agent) is *skipped* with a recorded warning rather than aborting via `?`/`bail!`; valid entries are retained. To keep serde from aborting the whole document atomically (`mod.rs:272`), give M1's `InteractionKind` enum a `#[serde(other)] Unknown` variant; the per-entry validator filters `Unknown` out into a warning instead of failing the parse. Document on `Registry::load` that invalid entries are skipped-with-warning, not fatal.
- **SilverBond:** At the load sites in `src/driver.rs` (`107`, `156`, `753`, `776`, `786`), consume the returned warnings and emit them once (e.g. `tracing::warn!`) at startup so the user learns which agent/pattern was dropped and why — replacing the current silent `.ok()` / `let Ok(...) else { return ... }` fallbacks. The hardcoded-default fallback behavior itself stays; only the silence is removed. Decide whether to dedupe warnings across the multiple load call sites to avoid log spam (load happens in several places).

---

## LOW

### L1. `pattern` regex is not validated at load even though `kind` is, so a bad regex fails silently downstream (FIXED)
**Severity:** LOW — bounded impact (silent feature degradation, user-config-only, no crash/corruption); note the reviewer's "lazy-compile is convention" counterpoint is factually wrong, so LOW rests on bounded impact alone.
**Files:** `core/src/agents/mod.rs:351-368` (`pattern` passed through unchecked at `:361`), `SilverBond/src/tmux_exec.rs:851-861` (silent `.ok()` drop) vs. `tmux_exec.rs:2170-2177` (`ready_regex` errors loudly)
**Description:** `InteractionPatternSpec::try_from` validates `kind` but passes `pattern.pattern` through as an unchecked `String` (`core/src/agents/mod.rs:361`). The consumer compiles it with `Regex::new(...).ok()` inside a `filter_map` (`SilverBond/src/tmux_exec.rs:851-861`), so a typo'd regex is dropped with zero diagnostics and the interaction pattern silently never matches. This diverges from the sibling `ready_regex` path (`tmux_exec.rs:2170-2177`), which surfaces a clear error via `Regex::new(...).with_context()?` — confirming this is a missing validation, not an established lazy convention. `pattern` is only ever used as a regex, never as a literal string.
**Fix:** Validate the regex at config load and skip-with-warning per entry (consistent with M1/M2 — M1 dissolves `try_from` into a trivial infallible copy, and M2 makes load partial, so the check does *not* belong in `try_from` and must not hard-bail).

- In the per-entry load loop introduced by M2 (`core/src/agents/mod.rs`, `interaction_patterns_from_config` area, `~:342-349`), call `regex::Regex::new(&pattern.pattern)`. On `Err`, push a `LoadWarning` (the M2 warning type) naming the agent and the offending pattern, and skip that single pattern; retain valid ones. Discard the compiled value.
- Keep `InteractionPatternSpec.pattern` as a plain `String` so the `Eq`/`PartialEq` derives on `InteractionPatternSpec`, `AgentSpec`, and `Registry` stay intact (a stored `Regex` is not `Eq`/`Hash`). Reuse the `regex` dependency already declared in `core/Cargo.toml`.
- No SilverBond change is strictly required for correctness, but the M2 warning-surfacing path will now also report bad regexes; the existing `.ok()` filter at `tmux_exec.rs:852` can remain as a defensive backstop.

### L2. New public fields added to `AgentSpec` without `#[non_exhaustive]` on a shared library struct (FIXED)
**Severity:** LOW — concrete (a real external constructor exists) but path-dep blast radius is a same-tree recompile error, not a published-SemVer break.
**Files:** `core/src/agents/mod.rs:10-22` (`AgentSpec`), `:29-36` (`InteractionPatternSpec`), `:38-55` (`AgentCapabilities`); `SilverBond/src/driver.rs:1421` (the one external struct-literal construction)
**Description:** `AgentSpec`, `InteractionPatternSpec`, and `AgentCapabilities` are `pub`-exported via `lib.rs:1` without `#[non_exhaustive]`, so adding a field breaks any external crate that constructs them with a struct literal or matches exhaustively. These structs are in active rapid expansion — all three were introduced on this branch and fields were already added after their construction helpers were written (the `capabilities()` helper takes 13 args while `AgentCapabilities` now has 15 fields). External construction is essentially nil: every literal is inside the library crate except one SilverBond *test* at `driver.rs:1421` that builds `agents::InteractionPatternSpec { ... }`. Because `tmux-tools-core` is a path dep (`SilverBond/Cargo.toml:24`), a field addition is a coordinated recompile error, not a version-resolution failure — but given near-certain continued growth, `#[non_exhaustive]` is cheap insurance.
**Fix:** Add `#[non_exhaustive]` to all three structs and provide library-side construction so the (only) external caller has a supported path.

- Add `#[non_exhaustive]` to `AgentSpec` (`mod.rs:10`), `InteractionPatternSpec` (`mod.rs:29`), and `AgentCapabilities` (`mod.rs:38`).
- Derive/keep `Default` on all three (all field types implement `Default`; `AgentCapabilities` already has a hand-written `impl Default` at `mod.rs:57-77` — keep it). Add a small `pub fn new(...)` constructor (or `impl Default` + setters) for `InteractionPatternSpec`, since `#[non_exhaustive]` forbids external struct-literal construction and `..Default::default()` does not help cross-crate.
- **SilverBond:** update the test at `src/driver.rs:1421` to build the spec via the new constructor (e.g. `agents::InteractionPatternSpec::new(...)`) instead of the struct literal, so it compiles under `#[non_exhaustive]`. No production SilverBond change is needed — it only reads these types.
- The library's own builtins (`builtin.rs`) and tests (`mod.rs:574,611`) are same-crate and unaffected by `#[non_exhaustive]`; they may keep their literals.

---

## Conventions Verified (No Issues Found)

- **Result propagation:** `merge_existing_agent` correctly upgraded to return `anyhow::Result<()>`; the `?` is threaded at the single call site (`mod.rs:281`) and the function returns `Ok(())`.
- **Default handling:** `agent_from_config` uses `agent.interaction_patterns.unwrap_or_default()`; `default_send_enter()` returns `true` and is correctly wired via `#[serde(default = "default_send_enter")]`; the `parses_interaction_patterns` test asserts both the defaulted (`send_enter == true`, `response == None`) and explicit (`send_enter == false`, `response == Some("y")`) cases.
- **Merge/replace semantics:** interaction patterns replace wholesale (consistent with the documented scalar-replace merge behavior for builtin overrides); builtins all ship `Vec::new()`, so no information is lost.
- **Downstream `kind` mapping:** the four kinds accepted by core's validator exactly match the four arms in SilverBond's `registry_interaction_pattern` today (no current mismatch — the concern in M1 is future drift).
- **Downstream regex safety:** SilverBond compiles `pattern` with `Regex::new(...).ok()` in a `filter_map` and `auto_respond` uses `response.unwrap_or_default()`, so a missing/invalid value degrades gracefully rather than panicking.
- **Required-field parsing:** `InteractionPatternConfig` leaves `pattern`/`kind`/`description` without serde defaults, so a missing required key yields a contextualized TOML parse error.
- **Test coverage:** all existing `AgentSpec` struct-literal sites were updated; new positive-path test added; no test was deleted or weakened.
