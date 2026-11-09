//! Pane output for a library consumer: ANSI snapshots and a live output stream. It is
//! internal to the crate: the library exposes no stream API.
#![allow(dead_code)]

use std::path::PathBuf;
use std::pin::Pin;
use std::process::Command;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as AnyhowContext, Result};
use tokio::io::{AsyncRead, ReadBuf};

use crate::host;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureAnsiOpts {
    /// Start line (negative = history). None means visible pane only.
    pub start: Option<i64>,
    /// End line. None means bottom of pane.
    pub end: Option<i64>,
}

pub fn capture_ansi(pane_id: &str, opts: CaptureAnsiOpts) -> Result<String> {
    host::current().capture_ansi(pane_id, opts.start, opts.end)
}

// FUTURE SEAM: a host event stream (on tmux, control mode with its multiplexed
// output events) can replace the FIFO approach for zero-overhead multiplexing.
pub async fn stream_pane(pane_id: &str) -> Result<impl AsyncRead + Send + 'static> {
    // NOTE: stream_pane uses a FIFO that the observed pane's output is piped into.
    // Cross-user FIFO permissions are fragile when the host invocation switches users.
    // SilverBond's execution path uses snapshot capture (capture_ansi), which is
    // unaffected by user-switching. Live-stream-to-UI is deprioritized in
    // favor of terminal-attach observability.
    let fifo_path = unique_fifo_path()?;
    make_fifo(&fifo_path)?;

    let shell_command = format!("cat > {}", shell_quote(&fifo_path.to_string_lossy()));
    if let Err(error) = host::current().start_output_pipe(pane_id, &shell_command) {
        let _ = std::fs::remove_file(&fifo_path);
        return Err(error);
    }

    let file = match tokio::fs::File::open(&fifo_path).await {
        Ok(file) => file,
        Err(error) => {
            let _ = host::current().stop_output_pipe(pane_id);
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
        let _ = host::current().stop_output_pipe(&self.pane_id);
        let _ = std::fs::remove_file(&self.fifo_path);
    }
}
