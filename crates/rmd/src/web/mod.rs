//! Local browser workspace: HTTP server, JSON API, and service operations.

pub mod api;
pub mod fingerprint;
pub mod insights;
pub mod pyjson;
pub mod server;
pub mod services;
pub mod workspace;

/// Carries an existing session token to a replacement server. The launcher
/// sets it in the server's environment (never on the command line); the
/// server removes it from its own environment at startup.
pub const TOKEN_ENV: &str = "REMINDER_BROWSER_TOKEN";
