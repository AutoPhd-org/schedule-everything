//! Local browser workspace: HTTP server, JSON API, and service operations.

pub mod api;
pub mod fingerprint;
pub mod insights;
pub mod phone;
pub mod pyjson;
pub mod server;
pub mod services;
pub mod suggestions;
pub mod workspace;

/// Carries an existing session token to a replacement server. The launcher
/// sets it in the server's environment (never on the command line); the
/// server removes it from its own environment at startup.
pub const TOKEN_ENV: &str = "REMINDER_BROWSER_TOKEN";

/// Write `content` to `path` atomically, readable only by its owner.
pub(crate) fn write_private(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&temporary)?, content.as_bytes())?;
    #[cfg(unix)]
    std::fs::set_permissions(&temporary, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::rename(&temporary, path)
}
