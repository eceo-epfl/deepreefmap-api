//! Connect codes and device tokens.

use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt;
use sha2::{Digest, Sha256};

/// Version tag on a connect code. Clients match on it, so a payload change is a new tag.
const CONNECT_CODE_PREFIX: &str = "drm1.";

/// Public prefix of every device token: `drmd_<prefix>_<secret>`.
const DEVICE_TOKEN_PREFIX: &str = "drmd_";

const PREFIX_HEX_LEN: usize = 16;
const SECRET_HEX_LEN: usize = 64;

fn random_hex(n_bytes: usize) -> String {
    use std::fmt::Write as _;
    let mut rng = rand::rng();
    (0..n_bytes).fold(String::with_capacity(n_bytes * 2), |mut out, _| {
        let _ = write!(out, "{:02x}", rng.random::<u8>());
        out
    })
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// SHA-256 hex of an input.
#[must_use]
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// A connect code as pasted, and the digest stored for it.
pub struct MintedConnectCode {
    pub code: String,
    pub code_hash: String,
}

/// Mint a connect code embedding `base_url`.
///
/// The address travels inside the code, so a client needs no configuration to reach
/// the server that issued it.
#[must_use]
pub fn mint_connect_code(base_url: &str) -> MintedConnectCode {
    let secret = random_hex(32);
    let payload = serde_json::json!({ "url": base_url, "code": secret });
    let encoded = URL_SAFE_NO_PAD.encode(payload.to_string());
    MintedConnectCode {
        code: format!("{CONNECT_CODE_PREFIX}{encoded}"),
        // Over the secret alone: a server reachable at two names must still
        // recognise its own codes.
        code_hash: sha256_hex(&secret),
    }
}

/// Recover the secret from a pasted connect code.
///
/// Accepts the whole `drm1.…` string or a bare secret, since operators paste both.
#[must_use]
pub fn parse_connect_code(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let secret = match trimmed.strip_prefix(CONNECT_CODE_PREFIX) {
        Some(encoded) => {
            let decoded = URL_SAFE_NO_PAD.decode(encoded).ok()?;
            let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
            json.get("code")?.as_str()?.to_string()
        }
        None => trimmed.to_string(),
    };
    (is_lower_hex(&secret) && secret.len() == SECRET_HEX_LEN).then_some(secret)
}

/// A minted device token. `raw_token` reaches the device once; only the prefix and
/// hash are persisted.
pub struct MintedDeviceToken {
    pub raw_token: String,
    pub token_prefix: String,
    pub token_hash: String,
}

/// Mint a device token: `drmd_<16-hex-prefix>_<64-hex-secret>`.
#[must_use]
pub fn mint_device_token() -> MintedDeviceToken {
    let prefix = random_hex(8);
    let secret = random_hex(32);
    MintedDeviceToken {
        raw_token: format!("{DEVICE_TOKEN_PREFIX}{prefix}_{secret}"),
        token_prefix: prefix,
        token_hash: hash_secret(&secret),
    }
}

/// Split a raw device token into its lookup prefix and secret.
///
/// Returns `None` for anything not shaped like a minted token, so a JWT falls through
/// to the other authentication path without touching the database or argon2.
#[must_use]
pub fn split_device_token(raw_token: &str) -> Option<(&str, &str)> {
    let rest = raw_token.strip_prefix(DEVICE_TOKEN_PREFIX)?;
    let (prefix, secret) = rest.split_once('_')?;
    if prefix.len() != PREFIX_HEX_LEN
        || secret.len() != SECRET_HEX_LEN
        || !is_lower_hex(prefix)
        || !is_lower_hex(secret)
    {
        return None;
    }
    Some((prefix, secret))
}

/// Argon2id at the OWASP baseline (m = 19 MiB, t = 2, p = 1). Pinned so a change to
/// the crate defaults cannot shift the work factor.
fn argon2() -> Argon2<'static> {
    let params = Params::new(19_456, 2, 1, None).expect("static argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Argon2id PHC hash of a token secret. Salted, so lookup goes by prefix.
///
/// # Panics
///
/// Panics if argon2 rejects a valid secret and salt, which cannot happen.
#[must_use]
pub fn hash_secret(secret: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(secret.as_bytes(), &salt)
        .expect("argon2 hashing of a token secret cannot fail")
        .to_string()
}

/// Verify a secret against its stored PHC hash. `false` on a malformed hash.
#[must_use]
pub fn verify_secret(secret: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    argon2().verify_password(secret.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_connect_code_round_trip() {
        let minted = mint_connect_code("https://example.org/api");
        let secret = parse_connect_code(&minted.code).expect("minted code parses");
        assert_eq!(sha256_hex(&secret), minted.code_hash);
    }

    #[test]
    fn test_mint_connect_code_embeds_url() {
        let minted = mint_connect_code("https://example.org/api");
        let encoded = minted.code.strip_prefix(CONNECT_CODE_PREFIX).unwrap();
        let decoded = URL_SAFE_NO_PAD.decode(encoded).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(json["url"], "https://example.org/api");
    }

    #[test]
    fn test_parse_connect_code_accepts_bare_secret() {
        let minted = mint_connect_code("https://example.org/api");
        let secret = parse_connect_code(&minted.code).unwrap();
        assert_eq!(parse_connect_code(&secret), Some(secret));
    }

    #[test]
    fn test_parse_connect_code_rejects_malformed() {
        for input in ["", "drm1.", "drm1.!!!!", "not-a-code", "drm1.YWJj"] {
            assert_eq!(parse_connect_code(input), None, "accepted {input:?}");
        }
    }

    #[test]
    fn test_parse_connect_code_rejects_short_secret_in_envelope() {
        let payload = serde_json::json!({ "url": "https://example.org", "code": "abc123" });
        let code = format!(
            "{CONNECT_CODE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        assert_eq!(parse_connect_code(&code), None);
    }

    #[test]
    fn test_split_device_token_verifies_secret() {
        let minted = mint_device_token();
        let (prefix, secret) = split_device_token(&minted.raw_token).expect("token splits");
        assert_eq!(prefix, minted.token_prefix);
        assert!(verify_secret(secret, &minted.token_hash));
        assert!(!verify_secret("wrong", &minted.token_hash));
    }

    #[test]
    fn test_split_device_token_rejects_jwt_and_malformed() {
        assert!(split_device_token("eyJhbGciOiJSUzI1NiJ9.e30.sig").is_none());
        assert!(split_device_token("drmd_short_secret").is_none());
        assert!(split_device_token("drmd_ZZZZZZZZZZZZZZZZ_ZZZZ").is_none());
    }
}
