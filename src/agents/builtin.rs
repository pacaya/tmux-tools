use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{AccessProfile, AgentSpec};

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
                ready_regex: Some("^▌".to_owned()),
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
            },
        ),
        (
            "claude".to_owned(),
            AgentSpec {
                name: "claude".to_owned(),
                binary: "claude".to_owned(),
                ready_regex: Some("^>".to_owned()),
                access_profiles: BTreeMap::from([
                    (
                        "default".to_owned(),
                        profile(&["--permission-mode", "plan"]),
                    ),
                    ("plan".to_owned(), profile(&["--permission-mode", "plan"])),
                    (
                        "accept-edits".to_owned(),
                        profile(&["--permission-mode", "acceptEdits"]),
                    ),
                    (
                        "bypass".to_owned(),
                        profile(&["--permission-mode", "bypassPermissions"]),
                    ),
                ]),
            },
        ),
        (
            "gemini".to_owned(),
            AgentSpec {
                name: "gemini".to_owned(),
                binary: "gemini".to_owned(),
                ready_regex: Some("^>".to_owned()),
                access_profiles: BTreeMap::from([("default".to_owned(), profile(&[]))]),
            },
        ),
    ])
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
        assert!(agents["claude"].access_profiles.contains_key("plan"));
        assert!(agents["claude"]
            .access_profiles
            .contains_key("accept-edits"));
        assert!(agents["claude"].access_profiles.contains_key("bypass"));
        assert_eq!(
            agents["claude"].access_profiles["default"].args,
            agents["claude"].access_profiles["plan"].args,
        );

        assert!(agents["gemini"].access_profiles.contains_key("default"));
    }
}
