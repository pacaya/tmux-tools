//! Desktop notifications on tmux: the tmux-wrapped OSC 777 sequence, written to the
//! tty of the target pane, which tmux unwraps through passthrough to its client.

use anyhow::{anyhow, Context, Result};
use std::fs::OpenOptions;
use std::io::Write;

use super::{runner, FIELD_SEP};
use crate::host::Delivery;

/// Write `ESC P tmux; ESC ESC ]777;notify;<T>;<B> BEL ESC \` to the tty of `pane`.
/// Without an attached client on the pane's session nothing is written and the
/// notification is not delivered.
pub(super) fn notify(pane: &str, title: &str, body: &str) -> Result<Delivery> {
    let format = format!("#{{session_attached}}{FIELD_SEP}#{{pane_tty}}");
    let line = runner::run_checked(&["display-message", "-p", "-t", pane, &format])?;
    let (attached, tty) = line
        .trim_end_matches('\n')
        .split_once(FIELD_SEP)
        .ok_or_else(|| anyhow!("unexpected tmux pane description for {pane}: {line:?}"))?;
    let attached: u64 = attached
        .parse()
        .with_context(|| format!("failed to parse session_attached {attached:?} of {pane}"))?;
    if attached == 0 {
        return Ok(Delivery::NotDelivered);
    }
    if tty.is_empty() {
        return Err(anyhow!("tmux reports no tty for pane {pane}"));
    }

    let sequence = wrapped_sequence(title, body);
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Never make the pane's tty the controlling terminal of this process.
        options.custom_flags(libc::O_NOCTTY);
    }
    let mut file = options
        .open(tty)
        .with_context(|| format!("failed to open the tty {tty} of pane {pane}"))?;
    file.write_all(sequence.as_bytes())
        .with_context(|| format!("failed to write the notification to {tty} of pane {pane}"))?;
    Ok(Delivery::Delivered)
}

/// The tmux passthrough wrapping of the OSC 777 notify sequence for `title` and `body`,
/// each sanitised by [`sanitise`]. No standalone BEL follows it.
fn wrapped_sequence(title: &str, body: &str) -> String {
    format!(
        "\x1bPtmux;\x1b\x1b]777;notify;{};{}\x07\x1b\\",
        sanitise(title),
        sanitise(body)
    )
}

/// Drop bytes 0x00–0x1F and 0x7F, and turn `;` (the OSC 777 field separator) into `,`.
fn sanitise(value: &str) -> String {
    value
        .chars()
        .filter(|char| !matches!(char, '\u{00}'..='\u{1f}' | '\u{7f}'))
        .map(|char| if char == ';' { ',' } else { char })
        .collect()
}
