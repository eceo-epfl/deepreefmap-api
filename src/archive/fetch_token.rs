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
    signature_over(secret, &format!("archive-fetch:{object_id}:{expires}"))
}

fn signature_over(secret: &[u8], claim: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(claim.as_bytes());
    mac
}

fn hex(mac: Hmac<Sha256>) -> String {
    use std::fmt::Write;
    mac.finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// The claim a run bundle link is signed over: one run, one group of its files.
fn bundle_claim(run_id: Uuid, purpose: &str, expires: i64) -> String {
    format!("archive-bundle:{run_id}:{purpose}:{expires}")
}

/// The signature for one run's output bundle, as lowercase hex.
#[must_use]
pub fn sign_bundle(secret: &[u8], run_id: Uuid, purpose: &str, expires: i64) -> String {
    hex(signature_over(secret, &bundle_claim(run_id, purpose, expires)))
}

/// Whether `sig` is the live signature for this bundle.
#[must_use]
pub fn verify_bundle(
    secret: &[u8],
    run_id: Uuid,
    purpose: &str,
    expires: i64,
    sig: &str,
    now: i64,
) -> bool {
    if expires < now {
        return false;
    }
    let Some(bytes) = hex_bytes(sig) else {
        return false;
    };
    signature_over(secret, &bundle_claim(run_id, purpose, expires))
        .verify_slice(&bytes)
        .is_ok()
}

/// The signature for one object and expiry, as lowercase hex.
#[must_use]
pub fn sign(secret: &[u8], object_id: Uuid, expires: i64) -> String {
    hex(mac(secret, object_id, expires))
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
    fn test_a_bundle_signature_is_bound_to_its_run_and_group() {
        let sig = sign_bundle(SECRET, object(), "Results", 1_000);
        assert!(verify_bundle(SECRET, object(), "Results", 1_000, &sig, 999));
        assert!(!verify_bundle(SECRET, object(), "frames", 1_000, &sig, 999));
        assert!(!verify_bundle(SECRET, Uuid::new_v4(), "Results", 1_000, &sig, 999));
        assert!(!verify_bundle(SECRET, object(), "Results", 1_000, &sig, 1_001));
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
