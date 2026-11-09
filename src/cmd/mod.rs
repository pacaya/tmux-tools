pub mod capture;
pub mod claude_teams;
pub mod escape;
pub mod execute;
pub mod interrupt;
pub mod kill;
pub mod launch;
pub mod list;
pub mod notify;
pub mod prompt;
pub mod rebalance;
pub mod safety;
pub mod self_;
pub mod send;
pub mod send_enter;
pub mod spawn_agent;
pub mod status;
pub mod wait_idle;

use serde::Serialize;
use tmux_tools_core::host::{self, Host};

/// A verb's `--format json` object result with the `host` that served it.
#[derive(Serialize)]
pub(crate) struct Hosted<T> {
    host: Host,
    #[serde(flatten)]
    result: T,
}

/// `result` with the current host, for a verb's `--format json` object result.
pub(crate) fn hosted<T: Serialize>(result: T) -> Hosted<T> {
    Hosted {
        host: host::current().host(),
        result,
    }
}
