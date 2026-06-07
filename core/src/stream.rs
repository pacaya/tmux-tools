use std::path::PathBuf;
use std::pin::Pin;
use std::process::Command;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use tokio::io::{AsyncRead, ReadBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureAnsiOpts {
    /// Start line (negative = history). None means visible pane only.
    pub start: Option<i64>,
    /// End line. None means bottom of pane.
    pub end: Option<i64>,
}

pub fn capture_ansi(pane_id: &str, opts: CaptureAnsiOpts) -> Result<String> {
    let args = capture_ansi_args(pane_id, opts);
    crate::tmux::run_checked_owned(&args)
}

// FUTURE SEAM: tmux control-mode (tmux -CC / %output multiplex)
// When implementing, replace the pipe-pane fifo approach with a control-mode
// client that parses structured %output events for zero-overhead multiplexing.
pub async fn stream_pane(pane_id: &str) -> Result<impl AsyncRead + Send + 'static> {
    // NOTE: stream_pane uses a FIFO that the observed pane writes into via `pipe-pane`.
    // Cross-user FIFO permissions are fragile when TmuxInvocation switches users.
    // SilverBond's execution path uses snapshot capture (capture_ansi / capture-pane),
    // which is unaffected by user-switching. Live-stream-to-UI is deprioritized in
    // favor of terminal-attach observability.
    let fifo_path = unique_fifo_path()?;
    make_fifo(&fifo_path)?;

    let shell_command = format!("cat > {}", shell_quote(&fifo_path.to_string_lossy()));
    if let Err(error) = crate::tmux::run_checked(&["pipe-pane", "-t", pane_id, &shell_command]) {
        let _ = std::fs::remove_file(&fifo_path);
        return Err(error);
    }

    let file = match tokio::fs::File::open(&fifo_path).await {
        Ok(file) => file,
        Err(error) => {
            let _ = crate::tmux::run(&["pipe-pane", "-t", pane_id]);
            let _ = std::fs::remove_file(&fifo_path);
            return Err(error)
                .with_context(|| format!("failed to open fifo {}", fifo_path.display()));
        }
    };

    Ok(PaneStream {
        file,
        fifo_path,
        pane_id: pane_id.to_owned(),
    })
}

fn capture_ansi_args(pane_id: &str, opts: CaptureAnsiOpts) -> Vec<String> {
    let mut args = vec![
        "capture-pane".to_owned(),
        "-e".to_owned(),
        "-p".to_owned(),
        "-t".to_owned(),
        pane_id.to_owned(),
    ];

    if let Some(start) = opts.start {
        args.push("-S".to_owned());
        args.push(start.to_string());
    }
    if let Some(end) = opts.end {
        args.push("-E".to_owned());
        args.push(end.to_string());
    }

    args
}

fn unique_fifo_path() -> Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?;
    Ok(std::env::temp_dir().join(format!(
        "tmux-tools-stream-{}-{}.fifo",
        std::process::id(),
        now.as_nanos()
    )))
}

fn make_fifo(path: &PathBuf) -> Result<()> {
    let status = Command::new("mkfifo")
        .arg(path)
        .status()
        .with_context(|| format!("failed to run mkfifo {}", path.display()))?;

    if !status.success() {
        return Err(anyhow!(
            "mkfifo {} failed with exit code {}",
            path.display(),
            status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

struct PaneStream {
    file: tokio::fs::File,
    fifo_path: PathBuf,
    pane_id: String,
}

impl AsyncRead for PaneStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buf)
    }
}

impl Drop for PaneStream {
    fn drop(&mut self) {
        let _ = crate::tmux::run(&["pipe-pane", "-t", &self.pane_id]);
        let _ = std::fs::remove_file(&self.fifo_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_ansi_default_builds_visible_pane_args() {
        assert_eq!(
            capture_ansi_args("%7", CaptureAnsiOpts::default()),
            vec!["capture-pane", "-e", "-p", "-t", "%7"]
        );
    }

    #[test]
    fn capture_ansi_range_builds_history_args() {
        assert_eq!(
            capture_ansi_args(
                "%7",
                CaptureAnsiOpts {
                    start: Some(-100),
                    end: Some(-1),
                },
            ),
            vec![
                "capture-pane",
                "-e",
                "-p",
                "-t",
                "%7",
                "-S",
                "-100",
                "-E",
                "-1",
            ]
        );
    }
}
