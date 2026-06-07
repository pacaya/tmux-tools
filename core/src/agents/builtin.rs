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
                // Claude Code's prompt glyph is `❯`, sits above a status/footer
                // block, and is present (empty) while generating, so it can't
                // signal readiness. The permission-mode footer (bottom line)
                // ends with `← for agents` only when idle; that suffix is dropped
                // while generating. Falls back to idle detection if absent.
                ready_regex: Some("← for agents\\s*$".to_owned()),
                ready_lines: Some(2),
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
            "gemini".to_owned(),
            AgentSpec {
                name: "gemini".to_owned(),
                binary: "gemini".to_owned(),
                ready_regex: Some("^>".to_owned()),
                ready_lines: None,
                access_profiles: BTreeMap::from([("default".to_owned(), profile(&[]))]),
                capabilities: capabilities(
                    true, true, true, true, true, false, true, true, false, false, false, false,
                    true,
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

        assert_eq!(agents.len(), 3);
        assert!(agents.contains_key("codex"));
        assert!(agents.contains_key("claude"));
        assert!(agents.contains_key("gemini"));

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

        assert!(agents["gemini"].access_profiles.contains_key("default"));
    }
}
