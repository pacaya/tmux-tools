//! Minimal, feature-gated test fixture for the integration suite.
//!
//! `tmux-tools` tests need a pane whose *live* state the test controls. `bash`
//! cannot enter the alternate screen on demand, cannot enable bracketed paste,
//! and cannot report the text it received, so this fixture does all three. It
//! exposes a line-oriented control channel on the pty:
//!
//! ```text
//! alt          enter the alternate screen (`\x1b[?1049h`)
//! normal       leave the alternate screen (`\x1b[?1049l`)
//! ready        render the configured ready footer
//! busy         render the configured busy footer
//! lines N      emit N numbered output lines (builds pane history)
//! bracket on   enable bracketed paste (`\x1b[?2004h`)
//! bracket off  disable bracketed paste (`\x1b[?2004l`)
//! ```
//!
//! Any other line, and anything between bracketed-paste markers, is composed
//! (see *Submissions* below). The fixture runs until its stdin closes.
//!
//! ## Bracketed paste
//!
//! Unless `--no-bracketed-paste` is given, the fixture enables bracketed paste
//! on startup (`\x1b[?2004h`), which flips tmux's live `bracket_paste_flag` for
//! the pane. A `tmux paste-buffer -p` then arrives wrapped in `\x1b[200~` /
//! `\x1b[201~`; the fixture appends everything between the markers to its
//! composer. With `--no-bracketed-paste` the flag stays clear and the same paste
//! arrives as ordinary bytes. The pty is put into non-canonical mode so the
//! closing marker is delivered without waiting for a newline and the markers can
//! be framed exactly.
//!
//! ## Submissions
//!
//! The fixture models a composer: pasted and typed bytes accumulate until a CR
//! (Enter) arrives, at which point the composed text is *submitted*. An
//! ordinary input line that names a control command is dispatched instead of
//! submitted; a bracketed paste is never treated as a command, even if its text
//! happens to spell one. An Enter on an empty composer submits nothing, but is
//! still reported on the pane as `tt-fake-tui: enter`, so a caller can observe
//! that Enter was delivered.
//!
//! Every submission is reported two ways:
//!
//! - a one-line summary on the pane, `tt-fake-tui: submission #N <bytes> bytes`,
//!   so tests can wait for it with `capture-pane`; and
//! - with `--report <PATH>`, the exact composed bytes appended to that file as a
//!   framed record: a `"submission <byte-len>\n"` header followed by the bytes.
//!   This is the fixture's own report of what it received, independent of how
//!   tmux renders or translates those bytes.
//!
//! A surface selects the footer text through its launch arguments:
//! `--ready <TEXT>` and `--busy <TEXT>`. The footer renders as
//! `tt-fake-tui ready: <TEXT>` / `tt-fake-tui busy: <TEXT>`, so two surfaces
//! can carry distinguishable, independently matchable footers. There is only
//! ever **one** footer row: `ready`/`busy` redraw it in place (moving the
//! cursor back up and clearing the row) rather than appending, so a busy→ready
//! transition leaves no stale busy line in a `capture-pane`. With
//! `--busy-on-submit`, a submission renders the busy footer after its event
//! line, modelling an agent that emits output before its busy footer appears.
//! With `--busy-on-paste`, the busy footer appears as soon as the prompt is
//! pasted and is restored to ready on submit, so the busy state is visible only
//! during the Enter delivery's frames. With `--echo-submit`, a submission
//! redraws the composer/footer row (when there is one, so the echo starts above
//! the submission point) with the submitted text before printing the event line,
//! modelling an agent that echoes the prompt. With `--quiet-submit`, a
//! submission prints nothing on the pane (its report file still records it), so
//! a test can exercise an empty `prompt` result. With `--alt-turn`, a submission
//! emits its output on the alternate screen and leaves it again before settling.
//! Without `--ready` no footer line is emitted, preserving the fixture's
//! original output for the capture tests.
//!
//! ## Control keys
//!
//! A control byte (other than CR/LF) is reported as a `key` event: a
//! `tt-fake-tui: key C-c` line on the pane, and a `key <byte-len>` record in
//! the `--report` file carrying the raw byte. This is how tests observe which
//! key `interrupt` actually sent. The tty's `isig` is disabled so an interrupt
//! character (`C-c`) is delivered as a byte instead of signalling the fixture.
//!
//! It is declared with `required-features = ["test-fixtures"]` in `Cargo.toml`,
//! so a normal `cargo build` / `cargo install` never produces it; integration
//! tests reach it as `CARGO_BIN_EXE_tt-fake-tui` when that feature is enabled.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};

const ENTER_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049h";
const LEAVE_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049l";
const ENABLE_BRACKETED_PASTE: &[u8] = b"\x1b[?2004h";
const DISABLE_BRACKETED_PASTE: &[u8] = b"\x1b[?2004l";
const BRACKET_START: &[u8] = b"\x1b[200~";
const BRACKET_END: &[u8] = b"\x1b[201~";
/// Return the cursor to the start of the current line and erase it, so the next
/// write replaces the row rather than appending to it.
const REWRITE_CURRENT_ROW: &str = "\r\x1b[2K";
/// Move the cursor up one row (back onto an active footer) before rewriting it.
const MOVE_TO_FOOTER_ROW: &str = "\x1b[1A";

fn main() -> io::Result<()> {
    // The control channel is a pty. Non-canonical mode delivers the bracketed-paste
    // close marker immediately (canonical mode would hold it until a newline), and
    // `-echo` stops the terminal from echoing input back over the single footer row we
    // redraw in place. `-isig` keeps an interrupt byte like `C-c` from signalling the
    // fixture, so it arrives as data and can be reported. `min 1 time 0` makes a read
    // return as soon as one byte is ready.
    let _ = std::process::Command::new("stty")
        .args(["-icanon", "-echo", "-isig", "min", "1", "time", "0"])
        .status();

    let mut ready_footer = None;
    let mut busy_footer = None;
    let mut busy_on_submit = false;
    let mut busy_on_paste = false;
    let mut echo_submit = false;
    let mut quiet_submit = false;
    let mut alt_turn = false;
    let mut report_path = None;
    let mut bracketed_paste = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ready" => ready_footer = args.next(),
            "--busy" => busy_footer = args.next(),
            "--busy-on-submit" => busy_on_submit = true,
            "--busy-on-paste" => busy_on_paste = true,
            "--echo-submit" => echo_submit = true,
            "--quiet-submit" => quiet_submit = true,
            "--alt-turn" => alt_turn = true,
            "--report" => report_path = args.next(),
            "--no-bracketed-paste" => bracketed_paste = false,
            _ => {}
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    out.write_all(if bracketed_paste {
        ENABLE_BRACKETED_PASTE
    } else {
        DISABLE_BRACKETED_PASTE
    })?;

    let mut fixture = Fixture {
        footer: Footer::default(),
        receiver: Receiver::default(),
        commands: Commands {
            ready_footer,
            busy_footer,
            busy_on_submit,
            busy_on_paste,
            echo_submit,
            quiet_submit,
            alt_turn,
        },
        report: match report_path {
            Some(path) => Some(OpenOptions::new().create(true).append(true).open(path)?),
            None => None,
        },
        on_alternate: false,
        bracketed_paste,
    };

    report_state(
        &mut out,
        &mut fixture.footer,
        "ready on the normal screen",
        fixture.commands.ready_footer.as_deref(),
    )?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut buffer = [0_u8; 4096];
    let mut events = Vec::new();

    loop {
        let read = stdin.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        fixture.receiver.feed(&buffer[..read], &mut events);
        for event in events.drain(..) {
            fixture.handle(event, &mut out)?;
        }
    }

    if fixture.on_alternate {
        out.write_all(LEAVE_ALTERNATE_SCREEN)?;
        out.flush()?;
    }

    Ok(())
}

/// The fixture's composer and control state.
struct Fixture {
    footer: Footer,
    receiver: Receiver,
    commands: Commands,
    report: Option<File>,
    on_alternate: bool,
    bracketed_paste: bool,
}

impl Fixture {
    fn handle(&mut self, event: InputEvent, out: &mut impl Write) -> io::Result<()> {
        match event {
            InputEvent::Paste(text) => {
                self.receiver.composer.extend_from_slice(&text);
                self.receiver.composer_from_paste = true;
                // Model an agent that shows its busy footer as soon as the prompt arrives,
                // then settles to ready on submit. The footer is visible only during the
                // Enter delivery's frames, not during the wait loop.
                if self.commands.busy_on_paste {
                    if let Some(footer) = self.commands.busy_footer.clone() {
                        self.footer.render(out, &format!("busy: {footer}"))?;
                    }
                }
            }
            InputEvent::Text(text) => self.receiver.composer.extend_from_slice(&text),
            InputEvent::Control(byte) => self.control(out, byte)?,
            InputEvent::Submit => self.submit(out)?,
        }
        Ok(())
    }

    /// A control byte arrived; report which key it was without composing it.
    fn control(&mut self, out: &mut impl Write, byte: u8) -> io::Result<()> {
        self.footer
            .event(out, &format!("tt-fake-tui: key {}", control_key_name(byte)))?;
        write_report(&mut self.report, "key", &[byte])
    }

    /// A CR arrived: dispatch a control command, or submit the composed text.
    fn submit(&mut self, out: &mut impl Write) -> io::Result<()> {
        let composer = std::mem::take(&mut self.receiver.composer);
        let from_paste = std::mem::replace(&mut self.receiver.composer_from_paste, false);

        // An ordinary input line drives the control channel; a bracketed paste does
        // not, even when its text happens to spell a command.
        if !from_paste {
            if let Ok(line) = std::str::from_utf8(&composer) {
                match line.trim() {
                    "alt" => {
                        if !self.on_alternate {
                            out.write_all(ENTER_ALTERNATE_SCREEN)?;
                            self.on_alternate = true;
                        }
                        report_state(
                            out,
                            &mut self.footer,
                            "on the alternate screen",
                            self.commands.ready_footer.as_deref(),
                        )?;
                        return Ok(());
                    }
                    "normal" => {
                        if self.on_alternate {
                            out.write_all(LEAVE_ALTERNATE_SCREEN)?;
                            self.on_alternate = false;
                        }
                        report_state(
                            out,
                            &mut self.footer,
                            "on the normal screen",
                            self.commands.ready_footer.as_deref(),
                        )?;
                        return Ok(());
                    }
                    "ready" => {
                        if let Some(text) = &self.commands.ready_footer {
                            self.footer.render(out, &format!("ready: {text}"))?;
                        }
                        return Ok(());
                    }
                    "busy" => {
                        if let Some(text) = &self.commands.busy_footer {
                            self.footer.render(out, &format!("busy: {text}"))?;
                        }
                        return Ok(());
                    }
                    "bracket on" => {
                        self.set_bracketed_paste(out, true)?;
                        return Ok(());
                    }
                    "bracket off" => {
                        self.set_bracketed_paste(out, false)?;
                        return Ok(());
                    }
                    _ => {
                        if let Some(count) = line.trim().strip_prefix("lines ") {
                            if let Ok(count) = count.trim().parse::<u32>() {
                                self.emit_lines(out, count)?;
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        // An empty composer is not a submission: an Enter with nothing composed
        // (including the retry Enters `prompt` sends when a pane does not visibly
        // change) delivers no text to the agent. It is still reported on the pane
        // so a caller can observe that Enter arrived, unless the fixture is quiet.
        if composer.is_empty() {
            if self.commands.quiet_submit {
                return Ok(());
            }
            return self.footer.event(out, "tt-fake-tui: enter");
        }

        self.receiver.submission_count += 1;
        let summary = format!(
            "tt-fake-tui: submission #{} {} bytes",
            self.receiver.submission_count,
            composer.len()
        );

        if self.commands.alt_turn {
            // Model a turn emitted on the alternate screen that leaves it before settle:
            // the output has no scrollback even though `alternate_on` is false at settle.
            out.write_all(ENTER_ALTERNATE_SCREEN)?;
            out.flush()?;
            self.on_alternate = true;
        }

        if self.commands.echo_submit {
            // Echo the submitted text where the composer row was: clearing the footer
            // leaves the cursor on that row, which sits above the submission point, so
            // the echo starts above it even though the pre-submission capture saw only
            // the old footer.
            self.footer.clear(out)?;
            out.write_all(&composer)?;
            if !composer.ends_with(b"\n") {
                out.write_all(b"\n")?;
            }
            out.flush()?;
        }

        if self.commands.quiet_submit {
            // Record the exact bytes but print nothing on the pane.
            write_report(&mut self.report, "submission", &composer)?;
        } else if self.commands.echo_submit {
            report(out, &summary)?;
            write_report(&mut self.report, "submission", &composer)?;
        } else {
            self.footer.event(out, &summary)?;
            write_report(&mut self.report, "submission", &composer)?;
        }

        if self.commands.alt_turn {
            out.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(800));
            out.write_all(LEAVE_ALTERNATE_SCREEN)?;
            self.on_alternate = false;
            report(out, "tt-fake-tui: alt-turn done")?;
            return Ok(());
        }

        // Model an agent turn: the submission output is written first, then the busy
        // footer is rendered below it. `prompt` must not advance its history mark to
        // this footer, or the output above it would be discarded.
        if self.commands.busy_on_submit {
            if let Some(text) = self.commands.busy_footer.clone() {
                self.footer.render(out, &format!("busy: {text}"))?;
            }
        }
        if self.commands.busy_on_paste {
            // Restore the ready footer once the turn is submitted, so the wait loop sees a
            // settled pane while the delivery frames still hold the brief busy footer.
            if let Some(text) = self.commands.ready_footer.clone() {
                self.footer.render(out, &format!("ready: {text}"))?;
            }
        }
        Ok(())
    }

    /// Emit `count` numbered output lines, building pane history for the eviction tests.
    fn emit_lines(&mut self, out: &mut impl Write, count: u32) -> io::Result<()> {
        for index in 1..=count {
            writeln!(out, "tt-fake-tui: line {index}")?;
        }
        out.flush()
    }

    fn set_bracketed_paste(&mut self, out: &mut impl Write, enabled: bool) -> io::Result<()> {
        if enabled == self.bracketed_paste {
            return Ok(());
        }
        out.write_all(if enabled {
            ENABLE_BRACKETED_PASTE
        } else {
            DISABLE_BRACKETED_PASTE
        })?;
        out.flush()?;
        self.bracketed_paste = enabled;
        Ok(())
    }
}

/// The fixture's own report of a submission, framed so arbitrary bytes are
/// unambiguous: `"<kind> <byte-len>\n"` followed by exactly that many bytes.
fn write_report(report: &mut Option<File>, kind: &str, bytes: &[u8]) -> io::Result<()> {
    if let Some(file) = report {
        writeln!(file, "{kind} {}", bytes.len())?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    Ok(())
}

/// The single footer row, tracked so an event line can be written above it
/// without the next footer redraw erasing the wrong row.
#[derive(Default)]
struct Footer {
    current: Option<String>,
}

impl Footer {
    fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.current.is_some() {
            write!(out, "{MOVE_TO_FOOTER_ROW}{REWRITE_CURRENT_ROW}")?;
            self.current = None;
        }
        Ok(())
    }

    fn render(&mut self, out: &mut impl Write, text: &str) -> io::Result<()> {
        self.clear(out)?;
        report(out, text)?;
        self.current = Some(text.to_owned());
        Ok(())
    }

    /// Write a non-footer line, keeping any active footer directly below it.
    fn event(&mut self, out: &mut impl Write, line: &str) -> io::Result<()> {
        let saved = self.current.take();
        if saved.is_some() {
            write!(out, "{MOVE_TO_FOOTER_ROW}{REWRITE_CURRENT_ROW}")?;
        }
        report(out, line)?;
        if let Some(text) = saved {
            report(out, &text)?;
            self.current = Some(text);
        }
        Ok(())
    }
}

struct Commands {
    ready_footer: Option<String>,
    busy_footer: Option<String>,
    busy_on_submit: bool,
    busy_on_paste: bool,
    echo_submit: bool,
    quiet_submit: bool,
    alt_turn: bool,
}

/// One parsed input event.
enum InputEvent {
    /// The exact bytes between a bracketed-paste start and end marker.
    Paste(Vec<u8>),
    /// A run of ordinary (non-bracketed) bytes, without a terminator.
    Text(Vec<u8>),
    /// A control byte (`C-a`..`C-z` and friends) that is not CR/LF.
    Control(u8),
    /// A CR or LF that ends the current ordinary input line.
    Submit,
}

/// A byte-oriented parser for the pty input. It frames bracketed pastes and
/// splits the remaining input into text runs and CR/LF terminators. A trailing
/// partial marker is held back until the next chunk so a marker split across
/// reads is still recognized.
#[derive(Default)]
struct Receiver {
    pending: Vec<u8>,
    paste: Vec<u8>,
    in_paste: bool,
    /// Text composed but not yet submitted.
    composer: Vec<u8>,
    /// Whether the current composer content includes a bracketed paste.
    composer_from_paste: bool,
    submission_count: u64,
}

impl Receiver {
    fn feed(&mut self, data: &[u8], events: &mut Vec<InputEvent>) {
        self.pending.extend_from_slice(data);
        loop {
            if self.in_paste {
                if let Some(index) = find_subslice(&self.pending, BRACKET_END) {
                    self.paste.extend_from_slice(&self.pending[..index]);
                    self.pending.drain(..index + BRACKET_END.len());
                    self.in_paste = false;
                    events.push(InputEvent::Paste(std::mem::take(&mut self.paste)));
                } else {
                    let keep = partial_marker_suffix_len(&self.pending, BRACKET_END);
                    if self.pending.len() > keep {
                        let split = self.pending.len() - keep;
                        self.paste.extend_from_slice(&self.pending[..split]);
                        self.pending.drain(..split);
                    }
                    break;
                }
            } else if let Some(index) = find_subslice(&self.pending, BRACKET_START) {
                let ordinary: Vec<u8> = self.pending.drain(..index).collect();
                self.feed_ordinary(&ordinary, events);
                self.pending.drain(..BRACKET_START.len());
                self.in_paste = true;
                self.paste.clear();
            } else {
                let keep = partial_marker_suffix_len(&self.pending, BRACKET_START);
                if self.pending.len() > keep {
                    let split = self.pending.len() - keep;
                    let ordinary: Vec<u8> = self.pending.drain(..split).collect();
                    self.feed_ordinary(&ordinary, events);
                }
                break;
            }
        }
    }

    fn feed_ordinary(&mut self, data: &[u8], events: &mut Vec<InputEvent>) {
        let mut run = Vec::new();
        for &byte in data {
            if byte == b'\r' || byte == b'\n' {
                if !run.is_empty() {
                    events.push(InputEvent::Text(std::mem::take(&mut run)));
                }
                events.push(InputEvent::Submit);
            } else if byte < 0x20 {
                // A control key (e.g. `C-c` from `interrupt`) is not composer input.
                if !run.is_empty() {
                    events.push(InputEvent::Text(std::mem::take(&mut run)));
                }
                events.push(InputEvent::Control(byte));
            } else {
                run.push(byte);
            }
        }
        if !run.is_empty() {
            events.push(InputEvent::Text(run));
        }
    }
}

/// The tmux `send-keys` spelling of a control byte, e.g. `0x03` -> `C-c`.
fn control_key_name(byte: u8) -> String {
    if (0x01..=0x1a).contains(&byte) {
        let letter = (b'a' + (byte - 1)) as char;
        format!("C-{letter}")
    } else {
        format!("C-{byte:#04x}")
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The length of the longest suffix of `bytes` that is a proper prefix of
/// `marker`, i.e. how many trailing bytes must be held back in case the rest of
/// the marker arrives in the next read. Holding back a fixed count instead would
/// strand ordinary bytes behind a partial marker that never completes.
fn partial_marker_suffix_len(bytes: &[u8], marker: &[u8]) -> usize {
    let max = (marker.len() - 1).min(bytes.len());
    (1..=max)
        .rev()
        .find(|&len| bytes.ends_with(&marker[..len]))
        .unwrap_or(0)
}

fn report_state(
    out: &mut impl Write,
    footer: &mut Footer,
    state: &str,
    ready_footer: Option<&str>,
) -> io::Result<()> {
    footer.clear(out)?;
    report(out, state)?;
    if let Some(text) = ready_footer {
        footer.render(out, &format!("ready: {text}"))?;
    }
    Ok(())
}

fn report(out: &mut impl Write, state: &str) -> io::Result<()> {
    writeln!(out, "tt-fake-tui: {state}")?;
    out.flush()
}
