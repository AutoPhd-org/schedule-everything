//! Identify the code a browser server runs so a stale server can be replaced
//! (port of `web/fingerprint.py`). OWNER: web agent.

/// Short hex fingerprint of the running build (binary + embedded frontend).
pub fn code_fingerprint() -> String {
    todo!("web agent: port web/fingerprint.py")
}
