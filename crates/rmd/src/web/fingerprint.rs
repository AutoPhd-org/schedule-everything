//! Identify the code a browser server runs so a stale server can be replaced.
//!
//! A running server keeps the code it started with. After an update, an old
//! server would pair the new frontend with old API commands, so the launcher
//! compares the server's fingerprint with its own: both hash the `rmd`
//! executable on disk (and the bundled frontend), so a newer binary's
//! fingerprint differs from what an old server reports.

use std::io::Read;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::web::server::Assets;

/// SHA-256 of the running executable's bytes (empty if it cannot be read).
fn executable_digest() -> Vec<u8> {
    let mut digest = Sha256::new();
    if let Ok(path) = std::env::current_exe()
        && let Ok(mut file) = std::fs::File::open(path)
    {
        let mut buffer = vec![0u8; 1 << 16];
        while let Ok(read) = file.read(&mut buffer) {
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
    }
    digest.finalize().to_vec()
}

/// Hash the executable's digest and every bundled frontend file (path and
/// content).
fn fingerprint_of(executable: &[u8], assets: &[(String, Vec<u8>)]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"rmd\0");
    digest.update(executable);
    for (path, content) in assets {
        digest.update(path.as_bytes());
        digest.update(b"\0");
        digest.update(Sha256::digest(content));
    }
    hex::encode(digest.finalize())[..16].to_string()
}

/// Short hex fingerprint of the running build (binary + embedded frontend),
/// computed once per process.
pub fn code_fingerprint() -> String {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let mut assets: Vec<(String, Vec<u8>)> = Assets::iter()
                .filter_map(|path| Assets::get(&path).map(|file| (path.into_owned(), file.data.into_owned())))
                .collect();
            assets.sort();
            fingerprint_of(&executable_digest(), &assets)
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_tracks_the_binary_and_the_frontend() {
        let assets = vec![("index.html".to_string(), b"<main></main>".to_vec())];
        let original = fingerprint_of(b"binary v1", &assets);
        assert_eq!(original.len(), 16);
        assert!(original.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(fingerprint_of(b"binary v1", &assets), original);
        assert_ne!(fingerprint_of(b"binary v2", &assets), original);
        let changed = vec![("index.html".to_string(), b"<main id=app></main>".to_vec())];
        assert_ne!(fingerprint_of(b"binary v1", &changed), original);
        let renamed = vec![("other.html".to_string(), b"<main></main>".to_vec())];
        assert_ne!(fingerprint_of(b"binary v1", &renamed), original);
    }

    #[test]
    fn running_build_has_a_stable_fingerprint() {
        let first = code_fingerprint();
        assert_eq!(first.len(), 16);
        assert_eq!(code_fingerprint(), first);
    }
}
