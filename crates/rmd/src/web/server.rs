//! Local HTTP server for the browser workspace (port of `web/server.py`).
//! OWNER: web agent.

use std::path::Path;

/// Serve the workspace for config root `root` on `port` (0 = pick a free
/// port); blocks until shut down. Returns the exit code.
pub fn run(_root: &Path, _port: u16) -> i32 {
    todo!("web agent: port web/server.py")
}
