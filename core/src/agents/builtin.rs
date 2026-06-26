use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{AccessProfile, AgentCapabilities, AgentSpec};

pub fn all() -> &'static BTreeMap<String, AgentSpec> {
    static BUILTINS: OnceLock<BTreeMap<String, AgentSpec>> = OnceLock::new();
    BUILTINS.get_or_init(build_all)
}

fn build_all() -> BTreeMap<String, AgentSpec> {
    BTreeMap::from([
        (
            "codex".to_owned(),
            AgentSpec {
                name: "codex".to_owned(),
                binary: "codex".to_owned(),
                // No shipped ready_regex: Codex's input glyph changed from `▌` to `›`
                // (always on screen, idle *and* generating, so it can't discriminate), and
                // its only reliable idle/busy signal is the `· Ready ·` vs `· Working ·`
                // status line — which is version-sensitive chrome we'd rather not bake into
                // the binary. With `None`, readiness falls back to idle/timeout out of the
                // box; users who want the faster status-line signal set a `ready_regex`
                // (e.g. `· Ready · Context`, debounced by `--ready-stable-seconds`) in their
                // ~/.config/tmux-tools/agents.toml. See README "Detecting readiness".
                ready_regex: None,
                ready_lines: None,
                interaction_patterns: Vec::new(),
                access_profiles: BTreeMap::from([
                    ("default".to_owned(), profile(&["--sandbox", "read-only"])),
                    ("read-only".to_owned(), profile(&["--sandbox", "read-only"])),
                    (
                        "workspace-write".to_owned(),
                        profile(&["--sandbox", "workspace-write"]),
                    ),
                    (
                        "full-access".to_owned(),
                        profile(&[
                            "--sandbox",
                            "danger-full-access",
                            "--ask-for-approval",
                            "never",
                        ]),
                    ),
                ]),
                capabilities: capabilities(
                    false, false, false, true, true, true, true, true, false, false, false, false,
                    true,
                ),
            },
        ),
        (
            "claude".to_owned(),
            AgentSpec {
                name: "claude".to_owned(),
                binary: "claude".to_owned(),
                // No shipped ready_regex (same call as the codex builtin). Claude
                // Code's prompt glyph `❯` is empty in both states, and the
                // permission-mode footer's `← for agents` suffix is present while
                // generating too (verified on claude 2.1.193) — so native chrome
                // can't discriminate idle from busy, and a regex on it reports a
                // premature "done". Readiness falls back to idle/timeout out of the
                // box, which is reliable here: the pane animates while Claude works
                // (spinner + per-second task-row timers) and goes static only when the
                // turn — including any background subagents — is truly complete. Users
                // who want a faster, precise signal install the optional custom status
                // line and set `ready_regex = "(✓ done|⏸ waiting) \\| 🤖"` with
                // `ready_lines = 0` (whole-pane scan, uncapped by subagent rows) in
                // ~/.config/tmux-tools/agents.toml. See README "Detecting readiness".
                ready_regex: None,
                ready_lines: None,
                interaction_patterns: Vec::new(),
                // Mirrors the codex vocabulary (read-only / workspace-write /
                // full-access) so one `--access` value works across agents, and
                // maps each tier onto Claude's permission modes.
                access_profiles: BTreeMap::from([
                    (
                        "default".to_owned(),
                        profile(&["--permission-mode", "plan"]),
                    ),
                    (
                        "read-only".to_owned(),
                        profile(&["--permission-mode", "plan"]),
                    ),
                    (
                        "workspace-write".to_owned(),
                        profile(&["--permission-mode", "acceptEdits"]),
                    ),
                    (
                        "full-access".to_owned(),
                        profile(&["--dangerously-skip-permissions"]),
                    ),
                ]),
                capabilities: capabilities(
                    true, true, true, true, true, true, true, false, true, true, true, true, true,
                ),
            },
        ),
        (
            "cursor".to_owned(),
            AgentSpec {
                name: "cursor".to_owned(),
                binary: "cursor-agent".to_owned(),
                // cursor's TUI input box renders 4 non-blank lines from the bottom
                // (mode row + Composer status row + cwd row sit below it), so
                // ready_lines = 4. The placeholder, anchored with `\s*$`, is present
                // only in the idle state. See ~/.config/tmux-tools/agents.toml for the
                // full rationale and the validated cursor-agent version.
                ready_regex: Some(
                    "→ (Add a follow-up|Plan, search, build anything)\\s*$".to_owned(),
                ),
                ready_lines: Some(4),
                interaction_patterns: Vec::new(),
                // Access-profile names mirror the codex vocabulary (read-only /
                // workspace-write / full-access) so one `--access` value works across
                // agents, plus a cursor-specific `plan` tier.
                access_profiles: BTreeMap::from([
                    ("default".to_owned(), profile(&["--mode", "ask"])),
                    ("read-only".to_owned(), profile(&["--mode", "ask"])),
                    ("plan".to_owned(), profile(&["--mode", "plan"])),
                    (
                        "workspace-write".to_owned(),
                        profile(&["--sandbox", "enabled"]),
                    ),
                    (
                        "full-access".to_owned(),
                        profile(&["--force", "--sandbox", "disabled"]),
                    ),
                ]),
                capabilities: capabilities(
                    false, false, false, false, false, false, false, false, false, false, false,
                    false, false,
                ),
            },
        ),
        (
            "agy".to_owned(),
            AgentSpec {
                name: "agy".to_owned(),
                binary: "agy".to_owned(),
                // Antigravity CLI (`agy`). Its bottom-most non-blank line reads
                // `? for shortcuts` when idle and `esc to cancel` while generating —
                // a clean idle/busy discriminator on the bottom line, so ready_lines
                // stays at the default. agy has NO interactive read-only mode, so there
                // is no `read-only` profile — only the two autonomy levels it supports.
                ready_regex: Some("\\? for shortcuts".to_owned()),
                ready_lines: None,
                interaction_patterns: Vec::new(),
                access_profiles: BTreeMap::from([
                    ("default".to_owned(), profile(&[])),
                    ("workspace-write".to_owned(), profile(&[])),
                    (
                        "full-access".to_owned(),
                        profile(&["--dangerously-skip-permissions"]),
                    ),
                ]),
                capabilities: capabilities(
                    false, false, false, false, false, false, false, false, false, false, false,
                    false, false,
                ),
            },
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
fn capabilities(
    prompt_refinement: bool,
    branch_choice: bool,
    loop_verdict: bool,
    structured_output: bool,
    session_reuse: bool,
    native_json_schema: bool,
    model_selection: bool,
    reasoning_config: bool,
    system_prompt: bool,
    budget_limit: bool,
    turn_limit: bool,
    cost_reporting: bool,
    web_search: bool,
) -> AgentCapabilities {
    AgentCapabilities {
        worker_execution: true,
        prompt_refinement,
        branch_choice,
        loop_verdict,
        structured_output,
        session_reuse,
        native_json_schema,
        model_selection,
        reasoning_config,
        system_prompt,
        budget_limit,
        turn_limit,
        cost_reporting,
        tool_allowlist: system_prompt,
        web_search,
    }
}

fn profile(args: &[&str]) -> AccessProfile {
    AccessProfile {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_expected_builtin_agents_and_profiles() {
        let agents = all();

        assert_eq!(agents.len(), 4);
        assert!(agents.contains_key("codex"));
        assert!(agents.contains_key("claude"));
        assert!(agents.contains_key("cursor"));
        assert!(agents.contains_key("agy"));
        // Gemini was removed (replaced by the Antigravity CLI).
        assert!(!agents.contains_key("gemini"));

        assert!(agents["codex"].access_profiles.contains_key("default"));
        assert!(agents["codex"].access_profiles.contains_key("read-only"));
        assert!(agents["codex"]
            .access_profiles
            .contains_key("workspace-write"));
        assert!(agents["codex"].access_profiles.contains_key("full-access"));
        assert_eq!(
            agents["codex"].access_profiles["default"].args,
            agents["codex"].access_profiles["read-only"].args,
        );

        assert!(agents["claude"].access_profiles.contains_key("default"));
        assert!(agents["claude"].access_profiles.contains_key("read-only"));
        assert!(agents["claude"]
            .access_profiles
            .contains_key("workspace-write"));
        assert!(agents["claude"].access_profiles.contains_key("full-access"));
        assert_eq!(
            agents["claude"].access_profiles["default"].args,
            agents["claude"].access_profiles["read-only"].args,
        );

        // claude ships no ready_regex (like codex): native footer chrome can't
        // discriminate idle from busy (the `← for agents` suffix shows while generating
        // too), so a built-in regex would report a premature "done". Readiness falls back
        // to idle detection; the custom-status-line signal is an opt-in user config.
        assert_eq!(agents["claude"].ready_regex, None);
        assert_eq!(agents["claude"].ready_lines, None);
        assert_eq!(agents["codex"].ready_regex, None);

        // Cursor mirrors the codex vocabulary plus a `plan` tier.
        assert!(agents["cursor"].access_profiles.contains_key("default"));
        assert!(agents["cursor"].access_profiles.contains_key("read-only"));
        assert!(agents["cursor"].access_profiles.contains_key("plan"));
        assert!(agents["cursor"]
            .access_profiles
            .contains_key("workspace-write"));
        assert!(agents["cursor"].access_profiles.contains_key("full-access"));
        assert_eq!(agents["cursor"].binary, "cursor-agent");

        // agy has no read-only tier (no interactive read-only mode).
        assert!(agents["agy"].access_profiles.contains_key("default"));
        assert!(agents["agy"]
            .access_profiles
            .contains_key("workspace-write"));
        assert!(agents["agy"].access_profiles.contains_key("full-access"));
        assert!(!agents["agy"].access_profiles.contains_key("read-only"));
    }
}
