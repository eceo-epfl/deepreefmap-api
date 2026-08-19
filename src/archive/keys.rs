//! Object key layout, derived here and nowhere else.
//!
//! Videos are content-addressed, so two devices holding the same clip land on one
//! object. Run artefacts keep their directory shape under the run's id.

use uuid::Uuid;

/// Hex length of an imohash.
pub const CONTENT_HASH_LEN: usize = 32;

/// Key for a video blob: `{prefix}/videos/imohash/{32-hex}`.
#[must_use]
pub fn video_key(prefix: &str, content_hash: &str) -> String {
    format!("{prefix}/videos/imohash/{content_hash}")
}

/// Key for a run artefact: `{prefix}/runs/{run_id}/{relpath}`, or `None` when the
/// relpath could escape the run's directory.
#[must_use]
pub fn artifact_key(prefix: &str, run_id: Uuid, relpath: &str) -> Option<String> {
    if !relpath_is_safe(relpath) {
        return None;
    }
    Some(format!("{prefix}/runs/{run_id}/{relpath}"))
}

/// Whether a client-supplied relpath stays inside its run directory.
///
/// Clients are untrusted, so anything that could traverse is refused: absolute paths,
/// backslashes, and `..` or empty components.
#[must_use]
pub fn relpath_is_safe(relpath: &str) -> bool {
    if relpath.is_empty() || relpath.starts_with('/') || relpath.contains('\\') {
        return false;
    }
    relpath
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Whether a claimed hash is exactly 32 lowercase hex characters.
///
/// imohash is 128 bits. Length is the whole check: the value is an identity a device
/// asserts, never a proof, so nothing here can or should validate it further.
#[must_use]
pub fn is_content_hash(claimed: &str) -> bool {
    claimed.len() == CONTENT_HASH_LEN
        && claimed
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn test_video_key_layout() {
        assert_eq!(video_key("dev", HASH), format!("dev/videos/imohash/{HASH}"));
    }

    #[test]
    fn test_artifact_key_layout() {
        let run_id: Uuid = "6f5902ac-2373-4c4d-bf3d-1a3b0d3b0d3b".parse().unwrap();
        assert_eq!(
            artifact_key("dev", run_id, "ortho/cover.json").unwrap(),
            format!("dev/runs/{run_id}/ortho/cover.json")
        );
    }

    #[test]
    fn test_traversal_relpaths_are_refused() {
        let run_id = Uuid::new_v4();
        for relpath in [
            "",
            "/etc/passwd",
            "../sibling/file",
            "a/../../b",
            "a/..",
            "..",
            "a//b",
            "a/./b",
            r"a\b",
            r"..\..\b",
        ] {
            assert!(
                artifact_key("dev", run_id, relpath).is_none(),
                "accepted {relpath:?}"
            );
        }
    }

    #[test]
    fn test_plain_relpaths_are_accepted() {
        for relpath in [
            "run_manifest.json",
            "ortho/ortho.png",
            "a/b/c.npz",
            "..a/b..",
        ] {
            assert!(relpath_is_safe(relpath), "refused {relpath:?}");
        }
    }

    #[test]
    fn test_content_hash_shape() {
        assert!(is_content_hash(HASH));
        assert!(!is_content_hash(&HASH[..31]));
        assert!(!is_content_hash(&HASH.to_uppercase()));
        assert!(!is_content_hash(&format!("{}g", &HASH[..31])));
    }
}
