//! Signatures on archive fetch links.
//!
//! `/archive/{id}/download` requires a bearer, but the URL it answers must work in a
//! plain browser navigation, which carries no header. So the URL carries a short-lived
//! HMAC over the object id and an expiry instead: read-only, single-object, and
//! validated by the API itself, so the object store still never faces a client.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

/// How long a fetch link stays valid. Long enough for the click that follows the
/// mint, short enough that a shared link goes stale in minutes.
pub const FETCH_TTL_SECONDS: i64 = 5 * 60;

fn mac(secret: &[u8], object_id: Uuid, expires: i64) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(format!("archive-fetch:{object_id}:{expires}").as_bytes());
    mac
}

/// The signature for one object and expiry, as lowercase hex.
#[must_use]
pub fn sign(secret: &[u8], object_id: Uuid, expires: i64) -> String {
    use std::fmt::Write;
    mac(secret, object_id, expires)
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Whether `sig` is the live signature for this object. Constant-time on the
/// signature bytes, and false for anything expired or unreadable.
#[must_use]
pub fn verify(secret: &[u8], object_id: Uuid, expires: i64, sig: &str, now: i64) -> bool {
    if expires < now {
        return false;
    }
    let Some(bytes) = hex_bytes(sig) else {
        return false;
    };
    mac(secret, object_id, expires).verify_slice(&bytes).is_ok()
}

fn hex_bytes(sig: &str) -> Option<Vec<u8>> {
    if !sig.len().is_multiple_of(2) {
        return None;
    }
    (0..sig.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&sig[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"minioadmin";

    fn object() -> Uuid {
        Uuid::parse_str("2f5d0f39-9f5f-4b1e-9be0-6ce574c4f5aa").expect("a valid uuid")
    }

    #[test]
    fn test_a_live_signature_verifies() {
        let sig = sign(SECRET, object(), 1_000);
        assert!(verify(SECRET, object(), 1_000, &sig, 999));
        assert!(verify(SECRET, object(), 1_000, &sig, 1_000));
    }

    #[test]
    fn test_expiry_is_refused() {
        let sig = sign(SECRET, object(), 1_000);
        assert!(!verify(SECRET, object(), 1_000, &sig, 1_001));
    }

    #[test]
    fn test_tampering_is_refused() {
        let sig = sign(SECRET, object(), 1_000);
        assert!(!verify(SECRET, object(), 2_000, &sig, 999), "moved expiry");
        assert!(
            !verify(SECRET, Uuid::new_v4(), 1_000, &sig, 999),
            "other object"
        );
        assert!(!verify(b"other", object(), 1_000, &sig, 999), "other key");
        assert!(!verify(SECRET, object(), 1_000, "zz", 999), "not hex");
        assert!(!verify(SECRET, object(), 1_000, "", 999), "empty");
    }
}
