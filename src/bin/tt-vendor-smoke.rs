//! P8: the manually-run, version-stamped smoke for the vendor surfaces.
//!
//! Behavior that depends on a third-party agent's rendering is validated here,
//! never under `cargo test`. For each in-scope `claude`, `codex` and
//! README-documented `dsh` surface this smoke:
//!
//! - compares the live `--version` with the surface's `validated_version` stamp by
//!   exact version-token equality;
//! - launches the agent on that surface and checks a flat surface is still
//!   non-alternate-screen;
//! - checks the declared `bracketed_paste` against the pane's live
//!   `bracket_paste_flag`;
//! - establishes idleness independently of the declared `ready_regex` (the pane
//!   must hold its native boot chrome and stay unchanged for a fixed window), then
//!   checks each declared `ready_regex`/`busy_regex` matches its state; the busy
//!   phase samples every frame of the generating turn and fails if `ready_regex`
//!   matches at any point between the first and last busy frame (rather than only
//!   matching at the turn's completion);
//! - checks a large (~5 KB) multi-line paste still arrives as one submission:
//!   positive composer evidence (a full-size collapse placeholder, or the end
//!   sentinel in the composer region of an inline-expanding surface) must be
//!   present and no turn may start across a sampled stability window, with
//!   transcript growth and busy frames as independent turn evidence;
//! - confirms a positive stable idle, then sends the declared `interrupt_key` and
//!   checks whether the agent quits, comparing the result with `quit_when_idle`.
//!
//! It exits non-zero on any mismatch. The declarations are read from the shipped
//! built-in registry (claude, codex) and from the `agents.toml` block the README
//! documents for `dsh`, so the smoke validates exactly what ships. Run it with:
//!
//! ```sh
//! cargo run --features vendor-smoke --bin tt-vendor-smoke
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use tmux_tools_core::agents::{Registry, Surface};
use tmux_tools_core::idle::{
    bottom_non_blank_lines, classify, PaneState, SurfacePatterns, DEFAULT_READY_SCAN_LINES,
};

/// How long a pane's capture must stay unchanged before it counts as idle. Used to
/// establish idleness independently of any declared `ready_regex`.
const IDLE_STABLE: Duration = Duration::from_secs(3);
/// How long the composer must stay unchanged after a paste before the paste check
/// concludes no turn started.
const PASTE_STABLE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(250);

/// How a surface renders a large paste, and therefore what counts as positive evidence
/// that the *whole* payload reached the composer as one unit.
#[derive(Clone, Copy, Eq, PartialEq)]
enum PasteEvidence {
    /// The agent collapses the paste to a placeholder reporting how many lines it
    /// holds, e.g. claude's `[Pasted text #1 +100 lines]`.
    Lines,
    /// The agent collapses the paste to a placeholder reporting its character count,
    /// e.g. codex's `[Pasted Content 5013 chars]`.
    Chars,
    /// The agent expands the paste inline, so the payload's end sentinel must appear in
    /// the composer region, e.g. `dsh --profile dshline`.
    InlineSentinel,
}

/// Every `claude`, `codex` and README-documented `dsh` surface this smoke validates.
/// `flat` marks the non-alternate-screen rendering, whose check is the only one that
/// applies to flat surfaces alone. `boot_evidence` is the surface's native startup
/// chrome: the smoke waits for it and then for a content-stable window, so idleness is
/// never inferred from the declared `ready_regex`. `paste_evidence` is how that surface
/// renders a collapsed or inline large paste. `note` is printed when the surface
/// declares no patterns.
struct InScope {
    agent: &'static str,
    surface: &'static str,
    flat: bool,
    boot_evidence: &'static str,
    paste_evidence: PasteEvidence,
    note: Option<&'static str>,
}

const IN_SCOPE: &[InScope] = &[
    InScope {
        agent: "claude",
        surface: "rich",
        flat: false,
        boot_evidence: r"Claude Code v",
        paste_evidence: PasteEvidence::Lines,
        note: Some(
            "claude's rich TUI declares no readiness patterns; readiness falls back to idle detection",
        ),
    },
    InScope {
        agent: "claude",
        surface: "flat",
        flat: true,
        boot_evidence: r"Claude Code v",
        paste_evidence: PasteEvidence::Lines,
        note: Some(
            "claude --ax-screen-reader has no native ready/busy chrome; the status-line patterns are opt-in via examples/claude-statusline, so this surface stays unpromoted",
        ),
    },
    InScope {
        agent: "codex",
        surface: "default",
        flat: true,
        boot_evidence: r"OpenAI Codex",
        paste_evidence: PasteEvidence::Chars,
        note: None,
    },
    InScope {
        agent: "dsh",
        surface: "rich",
        flat: false,
        boot_evidence: r"DeepSeek Harness",
        paste_evidence: PasteEvidence::Lines,
        note: Some("dsh --profile tui declares no readiness patterns"),
    },
    InScope {
        agent: "dsh",
        surface: "flat",
        flat: true,
        boot_evidence: r"dshline",
        paste_evidence: PasteEvidence::InlineSentinel,
        note: None,
    },
];

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => {
            println!("\nRESULT: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("\nRESULT: FAIL");
            std::process::ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("smoke error: {error:#}");
            println!("\nRESULT: FAIL");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Kills the smoke's private tmux server however the run ends.
struct Server {
    socket: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

fn run() -> Result<bool> {
    if Command::new("tmux").arg("-V").output().is_err() {
        bail!("tmux is required");
    }

    let registry = load_registry()?;
    let server = Server {
        socket: format!("tt-vendor-smoke-{}", std::process::id()),
    };
    let _ = tmux(&server, &["kill-server"]);

    let mut failures: Vec<String> = Vec::new();

    for scope in IN_SCOPE {
        println!("\n=== {}.{} ===", scope.agent, scope.surface);
        match check_surface(&server, &registry, scope) {
            Ok(mut surface_failures) => failures.append(&mut surface_failures),
            Err(error) => {
                let message = format!(
                    "{}.{}: could not be checked: {error:#}",
                    scope.agent, scope.surface
                );
                println!("    FAIL {message}");
                failures.push(message);
            }
        }
    }

    println!("\n--- summary ---");
    if failures.is_empty() {
        println!("all {} surfaces validated", IN_SCOPE.len());
        Ok(true)
    } else {
        for failure in &failures {
            println!("FAIL {failure}");
        }
        println!("{} check(s) failed", failures.len());
        Ok(false)
    }
}

/// Load the built-in registry plus the `dsh` surfaces the README documents, extracted
/// verbatim so the smoke validates exactly the shipped declaration.
fn load_registry() -> Result<Registry> {
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let readme = std::fs::read_to_string(&readme_path)
        .with_context(|| format!("failed to read {}", readme_path.display()))?;
    let dsh_toml = extract_dsh_block(&readme)?;

    let config_path =
        std::env::temp_dir().join(format!("tt-vendor-smoke-dsh-{}.toml", std::process::id()));
    std::fs::write(&config_path, dsh_toml)
        .with_context(|| format!("failed to write {}", config_path.display()))?;
    let (registry, warnings) = Registry::load_with_user_path(Some(&config_path))?;
    let _ = std::fs::remove_file(&config_path);

    if !warnings.is_empty() {
        bail!("the README dsh declaration loaded with warnings: {warnings:?}");
    }

    Ok(registry)
}

/// The `toml` fence under the README's `#### \`dsh\` surfaces` heading.
fn extract_dsh_block(readme: &str) -> Result<String> {
    let heading = readme
        .find("#### `dsh` surfaces")
        .ok_or_else(|| anyhow!("README does not document the dsh surfaces"))?;
    let after_heading = &readme[heading..];
    let fence_open = after_heading
        .find("```toml")
        .ok_or_else(|| anyhow!("the dsh section has no ```toml block"))?
        + "```toml".len();
    let body = &after_heading[fence_open..];
    let fence_close = body
        .find("```")
        .ok_or_else(|| anyhow!("the dsh toml block is not closed"))?;
    Ok(body[..fence_close].to_owned())
}

fn check_surface(server: &Server, registry: &Registry, scope: &InScope) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    let agent = registry
        .get(scope.agent)
        .ok_or_else(|| anyhow!("agent {} is not in the registry", scope.agent))?;
    let surface = agent
        .surfaces
        .get(scope.surface)
        .ok_or_else(|| anyhow!("surface {} has no surface {}", scope.agent, scope.surface))?;

    check_version(&agent.binary, surface, &mut failures);

    let session = format!("smoke-{}-{}", scope.agent, scope.surface);

    // --- launch and independent idle ----------------------------------------
    let argv = launch_argv(agent, surface);
    let cmd = argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    tmux_new_session(server, &session, &cmd)?;
    let patterns = compile_patterns(scope.agent, scope.surface, surface)?;
    let boot = Regex::new(scope.boot_evidence)
        .with_context(|| format!("smoke boot evidence for {}.{}", scope.agent, scope.surface))?;

    let Some(idle_capture) = wait_for_stable_idle(server, &session, &boot)? else {
        failures.push(format!(
            "{}.{}: never reached a content-stable idle window (boot evidence {:?})",
            scope.agent, scope.surface, scope.boot_evidence
        ));
        kill_session(server, &session);
        return Ok(failures);
    };

    // --- live rendering checks ----------------------------------------------
    let alternate_on = display(server, &session, "#{alternate_on}")?;
    if scope.flat {
        if alternate_on.trim() == "0" {
            pass("flat surface is non-alternate-screen");
        } else {
            failures.push(format!(
                "{}.{}: flat surface has alternate_on={alternate_on:?}, expected 0",
                scope.agent, scope.surface
            ));
        }
    } else {
        println!("    INFO alternate_on={alternate_on} (rich surface: no requirement)");
    }

    let bracket_paste_flag = display(server, &session, "#{bracket_paste_flag}")?;
    let live_bracketed_paste = bracket_paste_flag.trim() == "1";
    if live_bracketed_paste == surface.bracketed_paste {
        pass(&format!(
            "bracketed_paste={} matches live flag={}",
            surface.bracketed_paste, live_bracketed_paste
        ));
    } else {
        failures.push(format!(
            "{}.{}: declared bracketed_paste={} but live bracket_paste_flag={}",
            scope.agent, scope.surface, surface.bracketed_paste, live_bracketed_paste
        ));
    }

    // --- declared patterns against their states ------------------------------
    if let Some(patterns) = &patterns {
        let state = classify(&idle_capture, patterns);
        if state == PaneState::Idle {
            pass("ready_regex matches the independently-established idle capture (and busy_regex does not)");
        } else {
            failures.push(format!(
                "{}.{}: declared idle patterns classify the stable idle capture as {state:?}",
                scope.agent, scope.surface
            ));
        }

        if patterns.busy_regex.is_some() {
            check_busy(server, &session, scope, patterns, &mut failures)?;
        }
    } else if let Some(note) = scope.note {
        println!("    INFO no declared patterns: {note}");
    } else {
        println!("    INFO no declared patterns");
    }

    // --- quit-when-idle ------------------------------------------------------
    // Send the interrupt key only after a positive, independently established idle
    // window; a timeout here is a failure, not a pass.
    match wait_for_stable_idle(server, &session, &boot) {
        Ok(Some(_)) => {
            tmux_ok(
                server,
                &["send-keys", "-t", &session, &surface.interrupt_key],
            )?;
            sleep(Duration::from_secs(3));
            let quit_observed = !session_alive(server, &session);
            if quit_observed == surface.quit_when_idle {
                pass(&format!(
                    "interrupt_key {} quits idle={} matches quit_when_idle",
                    surface.interrupt_key, quit_observed
                ));
            } else {
                failures.push(format!(
                    "{}.{}: declared quit_when_idle={} but sending {} while idle quit={}",
                    scope.agent,
                    scope.surface,
                    surface.quit_when_idle,
                    surface.interrupt_key,
                    quit_observed
                ));
            }
        }
        Ok(None) => failures.push(format!(
            "{}.{}: could not confirm idle before the quit check",
            scope.agent, scope.surface
        )),
        Err(error) => failures.push(format!(
            "{}.{}: quit check could not confirm idle: {error:#}",
            scope.agent, scope.surface
        )),
    }

    // --- large paste as one submission --------------------------------------
    // A quit-when-idle surface's pane is gone; relaunch it for the paste check.
    let paste_session = if session_alive(server, &session) {
        session.clone()
    } else {
        let fresh = format!("{session}-paste");
        tmux_new_session(server, &fresh, &cmd)?;
        fresh
    };
    match wait_for_stable_idle(server, &paste_session, &boot) {
        Ok(Some(_)) => {
            check_large_paste(server, &paste_session, scope, &patterns, &mut failures)?;
        }
        Ok(None) => failures.push(format!(
            "{}.{}: paste-check pane never reached a content-stable idle window",
            scope.agent, scope.surface
        )),
        Err(error) => failures.push(format!(
            "{}.{}: paste check could not confirm idle: {error:#}",
            scope.agent, scope.surface
        )),
    }

    kill_session(server, &session);
    kill_session(server, &paste_session);
    Ok(failures)
}

fn check_version(binary: &str, surface: &Surface, failures: &mut Vec<String>) {
    let live = Command::new(binary).arg("--version").output();
    let live_text = match live {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned(),
        Ok(output) => {
            failures.push(format!(
                "{binary}: --version failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            return;
        }
        Err(error) => {
            failures.push(format!("{binary}: could not run --version: {error}"));
            return;
        }
    };

    let Some(declared) = surface.validated_version.as_deref() else {
        failures.push(format!(
            "{binary}: surface has no validated_version stamp (live version {live_text:?})"
        ));
        return;
    };

    if versions_match(declared, &live_text) {
        pass(&format!(
            "validated_version {declared:?} matches live {live_text:?}"
        ));
    } else {
        failures.push(format!(
            "{binary}: validated_version {declared:?} does not match live {live_text:?}"
        ));
    }
}

/// The version token of `text`, e.g. `2.1.278 (Claude Code)` -> `2.1.278` and
/// `dsh 0.1.5-rc.1` -> `0.1.5-rc.1`.
fn version_token(text: &str) -> Option<String> {
    let regex = Regex::new(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?(?:-[0-9A-Za-z.]+)?").ok()?;
    regex.find(text).map(|found| found.as_str().to_owned())
}

/// Whether the stamp's version token equals the live `--version` output's token
/// exactly. Substring matching is deliberately avoided: live `2.1.27` must not match
/// a stamp of `claude 2.1.278`, and live `0.1.5` must not match `dsh 0.1.5-rc.1`.
fn versions_match(declared: &str, live: &str) -> bool {
    match (version_token(declared), version_token(live)) {
        (Some(declared), Some(live)) => declared == live,
        _ => false,
    }
}

/// Poll until the capture matches the surface's native `boot_evidence` and has then
/// stayed byte-identical for `IDLE_STABLE`. This establishes idleness without
/// consulting the declared `ready_regex`, so the later pattern checks are not
/// circular. `Ok(None)` on timeout.
fn wait_for_stable_idle(server: &Server, session: &str, boot: &Regex) -> Result<Option<String>> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last: Option<String> = None;
    let mut since = Instant::now();

    loop {
        let capture = capture_pane(server, session)?;
        if last.as_deref() != Some(capture.as_str()) {
            last = Some(capture.clone());
            since = Instant::now();
        }
        if boot.is_match(&capture) && since.elapsed() >= IDLE_STABLE {
            return Ok(Some(capture));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        sleep(POLL);
    }
}

/// Submit a generating prompt and sample every frame until the turn returns to a
/// stable idle. Fails if `busy_regex` never matches, if `ready_regex` matches any
/// frame where `busy_regex` also matches (a premature-ready frame), or if the turn
/// never settles.
fn check_busy(
    server: &Server,
    session: &str,
    scope: &InScope,
    patterns: &SurfacePatterns,
    failures: &mut Vec<String>,
) -> Result<()> {
    // Submit a prompt that keeps the agent generating long enough to observe the
    // busy pattern. Codex needs a beat between the literal text and Enter.
    tmux_ok(
        server,
        &[
            "send-keys",
            "-l",
            "-t",
            session,
            "Count slowly from 1 to 40, one number per line.",
        ],
    )?;
    sleep(Duration::from_millis(500));
    let _ = tmux_ok(server, &["send-keys", "-t", session, "Enter"]);

    let deadline = Instant::now() + Duration::from_secs(90);
    // Every sampled frame from submission to the independently-established turn end,
    // as `(busy, ready)`. The turn end is byte-stability (`IDLE_STABLE`), never a
    // `ready_regex` match, so a premature ready frame cannot end the loop.
    let mut frames: Vec<(bool, bool)> = Vec::new();
    let mut last: Option<String> = None;
    let mut since = Instant::now();
    let mut completed = false;

    loop {
        let capture = capture_pane(server, session)?;
        let busy = patterns
            .busy_regex
            .as_ref()
            .is_some_and(|regex| regex.is_match(&capture));
        let ready = patterns
            .ready_regex
            .as_ref()
            .is_some_and(|regex| ready_matches(&capture, regex, patterns.ready_scan_lines));
        frames.push((busy, ready));

        if last.as_deref() != Some(capture.as_str()) {
            last = Some(capture.clone());
            since = Instant::now();
        }
        // The turn ends when the pane has held still for the window and is not busy.
        // A byte-stable busy pane (e.g. a static permission footer) never completes.
        if since.elapsed() >= IDLE_STABLE && !busy {
            completed = true;
            break;
        }

        if Instant::now() >= deadline {
            break;
        }
        sleep(POLL);
    }

    let busy_seen = frames.iter().any(|(busy, _)| *busy);
    let premature_ready = ready_matched_during_generation(&frames);

    let problem = if !busy_seen {
        Some("declared busy_regex never matched a generating frame within 90s".to_owned())
    } else if !completed {
        Some("the generating turn never returned to a byte-stable, non-busy state".to_owned())
    } else if premature_ready {
        Some(
            "ready_regex matched between the first and last busy frame — that frame would complete wait-idle mid-turn"
                .to_owned(),
        )
    } else {
        None
    };

    match problem {
        Some(detail) => failures.push(format!("{}.{}: {detail}", scope.agent, scope.surface)),
        None => pass(
            "busy_regex matched generating frames; ready_regex first matched only after the last busy frame",
        ),
    }

    Ok(())
}

/// Whether `ready_regex` matched during the generating window — from the first busy
/// frame through the last. A match there lets `wait-idle` complete mid-turn, so it
/// fails; a match before the first busy frame is the pane's pre-turn idle chrome, and
/// one after the last busy frame is the turn's actual completion. Frames are
/// `(busy, ready)` in sample order.
fn ready_matched_during_generation(frames: &[(bool, bool)]) -> bool {
    let first_busy = frames.iter().position(|(busy, _)| *busy);
    let last_busy = frames.iter().rposition(|(busy, _)| *busy);
    match (first_busy, last_busy) {
        // A frame where both match is inside this window too, and classify() would call
        // it Unknown rather than Idle — still a premature match, still a failure.
        (Some(first), Some(last)) => frames[first..=last].iter().any(|(_, ready)| *ready),
        _ => false,
    }
}

/// How many bottom non-blank lines make up the composer region. Paste evidence is only
/// accepted there: a fragmented paste echoes submitted lines into the transcript above
/// the composer, where they must not count.
const COMPOSER_SCAN_LINES: usize = 8;

/// The payload's terminal marker. An inline-expanding surface renders it in the
/// composer; a collapsed placeholder stands in for it.
const PASTE_SENTINEL: &str = "END-MARKER-9Z";

/// Whether the composer region proves the whole large payload arrived as one unit.
/// Exactly one collapse placeholder covering ~the whole payload, or the end sentinel on
/// an inline-expanding surface, is required; anything else (no evidence, several
/// fragments, a too-small placeholder) is a failure.
fn composer_paste_evidence(
    composer: &str,
    kind: PasteEvidence,
    payload_lines: usize,
    payload_bytes: usize,
) -> std::result::Result<(), String> {
    match kind {
        PasteEvidence::Lines => {
            let placeholder = Regex::new(r"\[(?:Pasted text|paste) #\d+ \+(\d+) lines\]").unwrap();
            let counts = placeholder_counts(&placeholder, composer);
            match counts.as_slice() {
                [count] if *count + 2 >= payload_lines => Ok(()),
                _ => Err(format!(
                    "the composer region holds {counts:?} collapsed line-count placeholder(s), not one covering the whole {payload_lines}-line payload"
                )),
            }
        }
        PasteEvidence::Chars => {
            let placeholder = Regex::new(r"\[Pasted Content (\d+) chars\]").unwrap();
            let counts = placeholder_counts(&placeholder, composer);
            match counts.as_slice() {
                [count] if *count + 16 >= payload_bytes => Ok(()),
                _ => Err(format!(
                    "the composer region holds {counts:?} collapsed char-count placeholder(s), not one covering the whole {payload_bytes}-byte payload"
                )),
            }
        }
        PasteEvidence::InlineSentinel => {
            if composer.contains(PASTE_SENTINEL) {
                Ok(())
            } else {
                Err("the payload's end sentinel is not in the composer region".to_owned())
            }
        }
    }
}

fn placeholder_counts(regex: &Regex, composer: &str) -> Vec<usize> {
    regex
        .captures_iter(composer)
        .filter_map(|captures| captures.get(1))
        .filter_map(|matched| matched.as_str().parse().ok())
        .collect()
}

/// Paste a genuinely large multi-line payload and require positive evidence that the
/// whole thing reached the composer as one unit, with no turn started. A collapsed
/// placeholder reporting the full size, or the payload's end sentinel inside the
/// composer region on an inline-expanding surface, is that evidence. A missing
/// placeholder, a too-small one, an observed busy frame, transcript growth, or an
/// unsettled pane is a failure.
fn check_large_paste(
    server: &Server,
    session: &str,
    scope: &InScope,
    patterns: &Option<SurfacePatterns>,
    failures: &mut Vec<String>,
) -> Result<()> {
    // ~5 KB / 101 lines: large enough to exercise P8's large-paste guarantee.
    let mut lines: Vec<String> = (1..=100)
        .map(|index| format!("line-{index:03}-{}", "x".repeat(40)))
        .collect();
    lines.push(PASTE_SENTINEL.to_owned());
    let payload = lines.join("\n");
    let payload_lines = lines.len();
    let payload_bytes = payload.len();

    let payload_path = std::env::temp_dir().join(format!(
        "tt-vendor-smoke-paste-{}-{}-{}.txt",
        std::process::id(),
        scope.agent,
        scope.surface
    ));
    {
        let mut file = std::fs::File::create(&payload_path)
            .with_context(|| format!("failed to create {}", payload_path.display()))?;
        file.write_all(payload.as_bytes())?;
    }

    let history_before = history_size(server, session)?;
    tmux_ok(
        server,
        &[
            "load-buffer",
            "-b",
            "tt-vendor-smoke",
            payload_path.to_str().unwrap(),
        ],
    )?;
    tmux_ok(
        server,
        &[
            "paste-buffer",
            "-p",
            "-d",
            "-b",
            "tt-vendor-smoke",
            "-t",
            session,
        ],
    )?;

    // Sample a stability window: the composer update settles, and nothing else may
    // change. A turn would keep the pane animating and/or classify it Busy.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last: Option<String> = None;
    let mut since = Instant::now();
    let mut busy_seen = false;
    let mut settled = false;

    loop {
        let capture = capture_pane(server, session)?;
        if last.as_deref() != Some(capture.as_str()) {
            last = Some(capture.clone());
            since = Instant::now();
        }
        if let Some(patterns) = patterns {
            if patterns
                .busy_regex
                .as_ref()
                .is_some_and(|regex| regex.is_match(&capture))
            {
                busy_seen = true;
            }
        }

        if busy_seen {
            break;
        }
        if since.elapsed() >= PASTE_STABLE {
            settled = true;
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep(POLL);
    }

    let history_after = history_size(server, session)?;
    let _ = std::fs::remove_file(&payload_path);

    let final_capture = last.unwrap_or_default();
    // The composer region only — never the whole scrollback.
    let composer = bottom_non_blank_lines(&final_capture, COMPOSER_SCAN_LINES).join("\n");

    let mut check_failures = Vec::new();

    // Positive evidence that the whole payload is one composer unit.
    if let Err(detail) = composer_paste_evidence(
        &composer,
        scope.paste_evidence,
        payload_lines,
        payload_bytes,
    ) {
        check_failures.push(detail);
    }

    // Independent turn evidence, alongside the collapse/inline evidence above.
    if busy_seen {
        check_failures.push("a busy frame was observed (a turn started)".to_owned());
    }
    if history_after > history_before {
        check_failures.push(format!(
            "the transcript grew during the paste (history_size {history_before} -> {history_after})"
        ));
    }
    if !settled {
        check_failures.push("the pane never settled after the paste".to_owned());
    }
    if let Some(patterns) = patterns {
        if classify(&final_capture, patterns) != PaneState::Idle {
            check_failures.push("the pane does not classify as idle after the paste".to_owned());
        }
    }

    if check_failures.is_empty() {
        pass("large paste arrived as one submission (whole payload in the composer, no turn started)");
    } else {
        for failure in check_failures {
            failures.push(format!("{}.{}: {failure}", scope.agent, scope.surface));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// tmux plumbing (a private server, so the maintainer's panes are untouched)
// ---------------------------------------------------------------------------

fn tmux(server: &Server, args: &[&str]) -> std::process::Output {
    Command::new("tmux")
        .arg("-L")
        .arg(&server.socket)
        .args(args)
        .output()
        .expect("failed to spawn tmux")
}

fn tmux_ok(server: &Server, args: &[&str]) -> Result<String> {
    let output = tmux(server, args);
    if !output.status.success() {
        bail!(
            "tmux {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn tmux_new_session(server: &Server, session: &str, cmd: &str) -> Result<()> {
    tmux_ok(
        server,
        &[
            "new-session",
            "-d",
            "-s",
            session,
            "-x",
            "220",
            "-y",
            "50",
            cmd,
        ],
    )?;
    Ok(())
}

fn kill_session(server: &Server, session: &str) {
    let _ = tmux(server, &["kill-session", "-t", session]);
}

fn session_alive(server: &Server, session: &str) -> bool {
    tmux(server, &["has-session", "-t", session])
        .status
        .success()
}

fn capture_pane(server: &Server, session: &str) -> Result<String> {
    tmux_ok(server, &["capture-pane", "-t", session, "-p", "-S", "-"])
}

fn display(server: &Server, session: &str, format: &str) -> Result<String> {
    tmux_ok(server, &["display-message", "-p", "-t", session, format])
}

/// The pane's scrollback length, used as independent transcript-growth evidence.
fn history_size(server: &Server, session: &str) -> Result<usize> {
    let raw = display(server, session, "#{history_size}")?;
    raw.trim()
        .parse()
        .with_context(|| format!("tmux reported a non-numeric history_size: {raw:?}"))
}

fn compile_patterns(agent: &str, surface: &str, spec: &Surface) -> Result<Option<SurfacePatterns>> {
    let ready = match &spec.ready_regex {
        Some(pattern) => Some(
            Regex::new(pattern)
                .with_context(|| format!("invalid ready_regex for {agent}.{surface}"))?,
        ),
        None => None,
    };
    let busy = match &spec.busy_regex {
        Some(pattern) => Some(
            Regex::new(pattern)
                .with_context(|| format!("invalid busy_regex for {agent}.{surface}"))?,
        ),
        None => None,
    };
    if ready.is_none() && busy.is_none() {
        return Ok(None);
    }
    Ok(Some(SurfacePatterns {
        ready_regex: ready,
        ready_scan_lines: spec.ready_lines.unwrap_or(DEFAULT_READY_SCAN_LINES),
        busy_regex: busy,
    }))
}

/// Whether `ready_regex` matches any of the bottom `ready_scan_lines` non-blank lines.
fn ready_matches(capture: &str, ready: &Regex, ready_scan_lines: usize) -> bool {
    bottom_non_blank_lines(capture, ready_scan_lines)
        .iter()
        .any(|line| ready.is_match(line))
}

fn launch_argv(agent: &tmux_tools_core::agents::AgentSpec, surface: &Surface) -> Vec<String> {
    let mut argv = vec![agent.binary.clone()];
    argv.extend(surface.args.iter().cloned());
    if let Some(profile) = agent.access_profiles.get("default") {
        argv.extend(profile.args.iter().cloned());
    }
    argv
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn pass(message: &str) {
    println!("    PASS {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_token_extracts_the_version_from_stamps_and_live_output() {
        assert_eq!(
            version_token("2.1.278 (Claude Code)").as_deref(),
            Some("2.1.278")
        );
        assert_eq!(version_token("claude 2.1.278").as_deref(), Some("2.1.278"));
        assert_eq!(
            version_token("codex-cli 0.155.1").as_deref(),
            Some("0.155.1")
        );
        assert_eq!(version_token("0.1.5-rc.1").as_deref(), Some("0.1.5-rc.1"));
        assert_eq!(version_token("no version here"), None);
    }

    #[test]
    fn versions_match_requires_exact_tokens() {
        // Matching pairs, including the agent-name prefix on the stamp.
        assert!(versions_match("claude 2.1.278", "2.1.278 (Claude Code)"));
        assert!(versions_match("codex-cli 0.155.1", "codex-cli 0.155.1"));
        assert!(versions_match("dsh 0.1.5-rc.1", "0.1.5-rc.1"));
    }

    #[test]
    fn versions_match_rejects_prefix_adjacent_and_prerelease_mismatches() {
        // Prefix-adjacent: live `2.1.27` must not match the `2.1.278` stamp.
        assert!(!versions_match("claude 2.1.278", "2.1.27 (Claude Code)"));
        // Prerelease suffix: live `0.1.5` must not match `0.1.5-rc.1`.
        assert!(!versions_match("dsh 0.1.5-rc.1", "0.1.5"));
        // Different prerelease ordinal.
        assert!(!versions_match("dsh 0.1.5-rc.1", "0.1.5-rc.2"));
        // A stamp with no version at all never matches.
        assert!(!versions_match("dsh", "0.1.5-rc.1"));
    }

    #[test]
    fn premature_ready_is_only_the_generating_window() {
        // Pre-turn idle (ready), then busy, then completion (ready): neither the
        // pre-turn idle nor the completion may be treated as premature.
        assert!(!ready_matched_during_generation(&[
            (false, true),
            (true, false),
            (true, false),
            (false, true),
        ]));

        // A mid-turn frame where ready matches and busy does not — the hazard
        // classify() maps to Idle — is followed by another busy frame: premature.
        assert!(ready_matched_during_generation(&[
            (true, false),
            (false, true),
            (true, false),
        ]));

        // A frame where both match (classify() -> Unknown) is also premature.
        assert!(ready_matched_during_generation(&[
            (true, false),
            (true, true),
            (false, true),
        ]));

        // Ready only ever matching after the last busy frame is the turn's completion.
        assert!(!ready_matched_during_generation(&[
            (true, false),
            (false, false),
            (false, true),
        ]));

        // No busy frame at all is handled by the separate busy-seen failure.
        assert!(!ready_matched_during_generation(&[(false, true)]));
    }

    #[test]
    fn composer_paste_evidence_requires_one_full_unit() {
        // One collapse placeholder covering the whole 101-line payload passes.
        assert!(composer_paste_evidence(
            "[Pasted text #1 +100 lines]",
            PasteEvidence::Lines,
            101,
            5013
        )
        .is_ok());
        // dsh spells it differently and reports the full line count.
        assert!(
            composer_paste_evidence("[paste #1 +101 lines]", PasteEvidence::Lines, 101, 5013)
                .is_ok()
        );
        // Fragmented into several placeholders fails.
        assert!(composer_paste_evidence(
            "[Pasted text #1 +20 lines][Pasted text #2 +20 lines]",
            PasteEvidence::Lines,
            101,
            5013
        )
        .is_err());
        // A single placeholder covering only part of the payload fails.
        assert!(composer_paste_evidence(
            "[Pasted text #1 +19 lines]",
            PasteEvidence::Lines,
            101,
            5013
        )
        .is_err());
        // A dropped paste leaves no evidence at all.
        assert!(composer_paste_evidence("", PasteEvidence::Lines, 101, 5013).is_err());

        // codex's character-count placeholder must cover the payload bytes.
        assert!(composer_paste_evidence(
            "[Pasted Content 5013 chars]",
            PasteEvidence::Chars,
            101,
            5013
        )
        .is_ok());
        assert!(composer_paste_evidence(
            "[Pasted Content 100 chars]",
            PasteEvidence::Chars,
            101,
            5013
        )
        .is_err());

        // Inline surfaces need the end sentinel in the composer region.
        assert!(
            composer_paste_evidence("END-MARKER-9Z", PasteEvidence::InlineSentinel, 101, 5013)
                .is_ok()
        );
        assert!(composer_paste_evidence(
            "some other composer text",
            PasteEvidence::InlineSentinel,
            101,
            5013
        )
        .is_err());
    }
}
