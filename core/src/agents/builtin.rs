use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{AccessProfile, AgentCapabilities, AgentSpec, ResumeSpec, Surface};

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
                // No shipped ready_regex until the P8 smoke measured one: Codex's input
                // glyph `▌`/`›` is on screen idle *and* generating, so it cannot
                // discriminate. Its reliable idle/busy signal is the status line —
                // `· Ready · Context …` vs `· Working ·` — which the P8 smoke
                // (src/bin/tt-vendor-smoke.rs) measured against codex-cli 0.155.1
                // and stamped below. The status line sits above the composer, so
                // ready_lines = 2. Re-run the smoke and re-stamp the version when codex
                // changes.
                surfaces: BTreeMap::from([(
                    "default".to_owned(),
                    Surface {
                        ready_regex: Some("· Ready · Context".to_owned()),
                        ready_lines: Some(2),
                        busy_regex: Some("· Working ·".to_owned()),
                        // Measured live: codex enables bracketed paste, and C-c quits it
                        // while idle. With a busy pattern declared, `interrupt` can still
                        // fire while generating.
                        bracketed_paste: true,
                        quit_when_idle: true,
                        validated_version: Some("codex-cli 0.155.1".to_owned()),
                        // Resume is a subcommand: `codex resume <session-id>`, where the
                        // session id is a UUID. With no id codex opens an interactive
                        // session picker, which P6 refuses. Read from codex-cli 0.155.1.
                        resume: resume("resume {id}", UUID_SHAPE),
                        ..surface(&[])
                    },
                )]),
                pre_surface_rendering: Some("default".to_owned()),
                default_surface: Some("default".to_owned()),
            },
        ),
        (
            "claude".to_owned(),
            AgentSpec {
                name: "claude".to_owned(),
                binary: "claude".to_owned(),
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
                // Two renderings. `rich` is today's full-screen TUI and the pre-surface
                // rendering; `flat` is the non-alternate-screen screen-reader rendering.
                // Neither ships readiness patterns: native chrome alone can't
                // discriminate idle from busy — Claude Code's prompt glyph `❯` is empty
                // in both states, and the permission-mode footer's `← for agents` suffix
                // is present while generating too. `claude --ax-screen-reader` has no
                // native ready/busy chrome at all, so the flat surface cannot satisfy
                // P2's promotion bar (a measured pair) and claude keeps `rich` as its
                // default. The status-line patterns the optional
                // examples/claude-statusline/ setup emits stay opt-in, declared by the
                // user in ~/.config/tmux-tools/agents.toml, where an agent-level
                // `ready_regex` binds to the rich pre-surface rendering. With no patterns
                // readiness falls back to idle detection, which is reliable here — the
                // pane animates while Claude works and goes static only when the turn
                // (including background subagents) is truly complete.
                //
                // The native declarations below (bracketed paste, quit hazard, resume)
                // are measured against claude 2.1.278 by the P8 smoke
                // (src/bin/tt-vendor-smoke.rs); re-run it and re-stamp
                // `validated_version` when claude changes.
                surfaces: BTreeMap::from([
                    (
                        "rich".to_owned(),
                        Surface {
                            // The full-screen TUI enables bracketed paste; the declared
                            // capability is expectation only and the live flag is read before
                            // pasting. Measured live: C-c does not quit claude while idle.
                            bracketed_paste: true,
                            quit_when_idle: false,
                            validated_version: Some("claude 2.1.278".to_owned()),
                            // Resume: `claude --resume <session-id>` (also `-r`), where the
                            // session id is a UUID. With no id claude opens an interactive
                            // session picker, which P6 refuses. Read from claude 2.1.278.
                            resume: resume("--resume {id}", UUID_SHAPE),
                            ..Surface::default()
                        },
                    ),
                    (
                        "flat".to_owned(),
                        Surface {
                            // No ready_regex/busy_regex: `--ax-screen-reader` has no native
                            // idle/busy chrome. See the comment above.
                            bracketed_paste: true,
                            quit_when_idle: false,
                            validated_version: Some("claude 2.1.278".to_owned()),
                            // `--ax-screen-reader` selects the rendering; it does not change
                            // resume, so the flat surface declares the same syntax and shape.
                            resume: resume("--resume {id}", UUID_SHAPE),
                            ..surface(&["--ax-screen-reader"])
                        },
                    ),
                ]),
                pre_surface_rendering: Some("rich".to_owned()),
                default_surface: Some("rich".to_owned()),
            },
        ),
        (
            "cursor".to_owned(),
            AgentSpec {
                name: "cursor".to_owned(),
                binary: "cursor-agent".to_owned(),
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
                // cursor's TUI input box renders 4 non-blank lines from the bottom
                // (mode row + Composer status row + cwd row sit below it), so
                // ready_lines = 4. The placeholder, anchored with `\s*$`, is present
                // only in the idle state. See ~/.config/tmux-tools/agents.toml for the
                // full rationale and the validated cursor-agent version.
                surfaces: BTreeMap::from([(
                    "default".to_owned(),
                    Surface {
                        ready_regex: Some(
                            "→ (Add a follow-up|Plan, search, build anything)\\s*$".to_owned(),
                        ),
                        ready_lines: Some(4),
                        ..Surface::default()
                    },
                )]),
                pre_surface_rendering: Some("default".to_owned()),
                default_surface: Some("default".to_owned()),
            },
        ),
        (
            "agy".to_owned(),
            AgentSpec {
                name: "agy".to_owned(),
                binary: "agy".to_owned(),
                interaction_patterns: Vec::new(),
                // Antigravity CLI (`agy`). Its bottom-most non-blank line reads
                // `? for shortcuts` when idle and `esc to cancel` while generating —
                // a clean idle/busy discriminator on the bottom line, so ready_lines
                // stays at the default. agy has NO interactive read-only mode, so there
                // is no `read-only` profile — only the two autonomy levels it supports.
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
                surfaces: BTreeMap::from([(
                    "default".to_owned(),
                    Surface {
                        ready_regex: Some("\\? for shortcuts".to_owned()),
                        ..Surface::default()
                    },
                )]),
                pre_surface_rendering: Some("default".to_owned()),
                default_surface: Some("default".to_owned()),
            },
        ),
    ])
}

/// The measured identifier shape of the session IDs `claude` and `codex` accept: a
/// canonical UUID. Both open an interactive session picker when no identifier is
/// supplied, which P6's validation exists to avoid.
const UUID_SHAPE: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";

/// A resume declaration for a surface, read from the live binary's `--help`.
fn resume(syntax: &str, id_regex: &str) -> Option<ResumeSpec> {
    Some(ResumeSpec {
        syntax: syntax.to_owned(),
        id_regex: id_regex.to_owned(),
    })
}

/// A surface with the shipped defaults (no patterns, no bracketed-paste capability,
/// interrupt `C-c`, no quit hazard, no resume syntax) and the given launch arguments.
fn surface(args: &[&str]) -> Surface {
    Surface {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        ..Surface::default()
    }
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

    #[test]
    fn every_builtin_has_exactly_one_default_surface() {
        for (name, agent) in all() {
            let default = agent
                .default_surface()
                .unwrap_or_else(|| panic!("{name} should have a default surface"));
            assert!(
                agent.surfaces.contains_key(default.name),
                "{name} default surface {} should exist",
                default.name
            );
            assert!(
                agent.default_surface.is_some(),
                "{name} should name its default surface"
            );
            assert!(
                agent.pre_surface().is_some(),
                "{name} should have a pre-surface rendering"
            );
        }
    }

    #[test]
    fn single_surface_agents_carry_their_measured_readiness_fields() {
        let agents = all();

        // codex's only surface is flat and is both its pre-surface rendering and its
        // default; it carries the P8 smoke's measured status-line patterns.
        assert_eq!(
            agents["codex"].surfaces["default"].ready_regex.as_deref(),
            Some("· Ready · Context")
        );
        assert_eq!(agents["codex"].surfaces["default"].ready_lines, Some(2));
        assert_eq!(
            agents["codex"].surfaces["default"].busy_regex.as_deref(),
            Some("· Working ·")
        );
        assert_eq!(agents["codex"].default_surface.as_deref(), Some("default"));
        assert_eq!(
            agents["codex"].pre_surface_rendering.as_deref(),
            Some("default")
        );

        // cursor keeps its 4-line scan depth and idle placeholder (out of scope for
        // this smoke; ISSUE-260918-0525-02 shipped these values).
        assert_eq!(
            agents["cursor"].surfaces["default"].ready_regex.as_deref(),
            Some("→ (Add a follow-up|Plan, search, build anything)\\s*$")
        );
        assert_eq!(agents["cursor"].surfaces["default"].ready_lines, Some(4));

        // agy keys off its bottom-line footer.
        assert_eq!(
            agents["agy"].surfaces["default"].ready_regex.as_deref(),
            Some("\\? for shortcuts")
        );
        assert_eq!(agents["agy"].surfaces["default"].ready_lines, None);
    }

    #[test]
    fn claude_keeps_rich_as_default_and_flat_has_no_native_patterns() {
        let claude = &all()["claude"];

        assert_eq!(claude.surfaces.len(), 2);
        // `claude --ax-screen-reader` has no native ready/busy chrome, so the flat
        // surface declares no patterns and does not satisfy P2's promotion bar.
        // Claude therefore keeps `rich` as its default and pre-surface rendering.
        assert_eq!(claude.default_surface.as_deref(), Some("rich"));
        assert_eq!(claude.pre_surface_rendering.as_deref(), Some("rich"));
        assert_eq!(claude.default_surface().unwrap().name, "rich");
        assert_eq!(claude.pre_surface().unwrap().name, "rich");

        let flat = &claude.surfaces["flat"];
        assert_eq!(flat.args, vec!["--ax-screen-reader".to_owned()]);
        assert_eq!(flat.ready_regex, None);
        assert_eq!(flat.busy_regex, None);
        assert_eq!(flat.ready_lines, None);
        assert!(
            !flat.patterns_validated(),
            "the flat surface cannot be promoted without a measured pattern pair"
        );

        // The rich surface declares no patterns either; rich surfaces are not required
        // to carry any.
        assert_eq!(claude.surfaces["rich"].ready_regex, None);
        assert!(!claude.surfaces["rich"].patterns_validated());
    }

    #[test]
    fn in_scope_builtin_surfaces_ship_measured_declarations() {
        let agents = all();

        let expected = [
            ("claude", "rich", true, false, "claude 2.1.278"),
            ("claude", "flat", true, false, "claude 2.1.278"),
            ("codex", "default", true, true, "codex-cli 0.155.1"),
        ];
        for (name, surface_name, bracketed_paste, quit_when_idle, version) in expected {
            let surface = &agents[name].surfaces[surface_name];
            assert_eq!(
                surface.bracketed_paste, bracketed_paste,
                "{name}.{surface_name} bracketed-paste declaration"
            );
            assert_eq!(
                surface.quit_when_idle, quit_when_idle,
                "{name}.{surface_name} quit-when-idle declaration"
            );
            assert_eq!(
                surface.validated_version.as_deref(),
                Some(version),
                "{name}.{surface_name} validated-version stamp"
            );
            assert_eq!(
                surface.interrupt_key, "C-c",
                "{name}.{surface_name} ships C-c as the interrupt key"
            );
        }

        // cursor and agy are out of scope and keep ISSUE-260918-0525-02's values: a
        // single unvalidated surface with no busy pattern and no stamp.
        for name in ["cursor", "agy"] {
            let surface = &agents[name].surfaces["default"];
            assert!(!surface.bracketed_paste, "{name} bracketed-paste unchanged");
            assert!(!surface.quit_when_idle, "{name} quit hazard unchanged");
            assert_eq!(surface.validated_version, None, "{name} stays unstamped");
            assert!(surface.busy_regex.is_none(), "{name} keeps no busy_regex");
            assert!(!surface.patterns_validated());
        }

        // Resume syntax is read from the live binary; cursor and agy keep
        // ISSUE-260918-0525-02's values (no measured resume declaration).
        for (name, agent) in agents {
            for (surface_name, surface) in &agent.surfaces {
                let expected_resume = matches!(
                    (name.as_str(), surface_name.as_str()),
                    ("claude", "rich") | ("claude", "flat") | ("codex", "default")
                );
                assert_eq!(
                    surface.resume.is_some(),
                    expected_resume,
                    "{name}.{surface_name} resume declaration moved unexpectedly"
                );
            }
        }
    }

    #[test]
    fn builtin_resume_declarations_match_their_measured_syntax() {
        let agents = all();
        let uuid = "0123abcd-4567-89ab-cdef-0123456789ab";

        // claude resumes by flag; both renderings share the CLI's UUID shape.
        for surface_name in ["rich", "flat"] {
            let resume = agents["claude"].surfaces[surface_name]
                .resume
                .as_ref()
                .unwrap_or_else(|| panic!("claude.{surface_name} should declare resume"));
            assert_eq!(resume.syntax, "--resume {id}");
            assert!(
                regex::Regex::new(&resume.id_regex).unwrap().is_match(uuid),
                "claude.{surface_name} resume shape must accept a UUID"
            );
            assert!(!regex::Regex::new(&resume.id_regex)
                .unwrap()
                .is_match("nope"));
        }

        // codex resumes by subcommand.
        let codex = agents["codex"].surfaces["default"]
            .resume
            .as_ref()
            .expect("codex.default should declare resume");
        assert_eq!(codex.syntax, "resume {id}");
        assert!(regex::Regex::new(&codex.id_regex).unwrap().is_match(uuid));

        // cursor and agy were not measured, so they declare no resume syntax.
        assert!(agents["cursor"].surfaces["default"].resume.is_none());
        assert!(agents["agy"].surfaces["default"].resume.is_none());
    }
}
