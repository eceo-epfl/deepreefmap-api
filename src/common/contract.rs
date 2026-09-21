//! Range negotiation of the metadata contract.
//!
//! Each side declares a range of versions it can read, and one exchange runs at
//! `min(client_max, server_max)`. A response body carries that agreed version, never this
//! server's own maximum: the client tests the body for equality against what it asked for,
//! so a maximum it cannot read would refuse a sound exchange. The server's own range
//! travels in a response header instead, where it is diagnosis rather than protocol.
//!
//! Both request headers are optional, and a malformed value counts as absent. Contract 1
//! is the only one that ever shipped without them, and a truncating proxy must not brick a
//! working laptop.

use std::sync::LazyLock;

use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::AppError;
use crate::routes::private::sync::schema::{CONTRACT_VERSION, MIN_CONTRACT_VERSION};

/// Versions the client can read: digits, or `min-max`.
pub const CONTRACT_HEADER: &str = "deepreefmap-contract";
/// Sections the client can read: lowercase names, comma separated, order irrelevant.
pub const SECTIONS_HEADER: &str = "deepreefmap-sections";

/// This server's range, as the value every response carries.
static SERVER_RANGE: LazyLock<HeaderValue> = LazyLock::new(|| {
    HeaderValue::from_str(&format!("{MIN_CONTRACT_VERSION}-{CONTRACT_VERSION}"))
        .expect("digits and a dash are a valid header value")
});

/// What the client declared it can read.
#[derive(Debug, Clone)]
pub struct ClientContract {
    pub min: u32,
    pub max: u32,
    /// None means the client did not narrow: serve what a pre-negotiation server would.
    pub sections: Option<Vec<String>>,
}

impl Default for ClientContract {
    fn default() -> Self {
        Self {
            min: 1,
            max: 1,
            sections: None,
        }
    }
}

impl ClientContract {
    /// The version this exchange runs at, which is what a response body stamps.
    #[must_use]
    pub fn agreed(&self) -> u32 {
        self.max.min(CONTRACT_VERSION)
    }

    /// Whether the client can read a section. An un-narrowed client reads every one.
    #[must_use]
    pub fn speaks(&self, section: &str) -> bool {
        self.sections
            .as_ref()
            .is_none_or(|declared| declared.iter().any(|name| name == section))
    }

    /// Whether the two ranges share no version at all.
    fn disjoint(&self) -> bool {
        self.min > CONTRACT_VERSION || self.max < MIN_CONTRACT_VERSION
    }

    fn from_headers(headers: &HeaderMap) -> Self {
        let text = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
        let (min, max) = text(CONTRACT_HEADER)
            .and_then(parse_range)
            .unwrap_or((1, 1));
        Self {
            min,
            max,
            sections: text(SECTIONS_HEADER).and_then(parse_sections),
        }
    }
}

/// A bare `1` is the range `1-1`, and `min` may not exceed `max`.
fn parse_range(raw: &str) -> Option<(u32, u32)> {
    let (min, max) = raw.split_once('-').unwrap_or((raw, raw));
    let min: u32 = min.parse().ok()?;
    let max: u32 = max.parse().ok()?;
    (min <= max).then_some((min, max))
}

/// One malformed name discards the whole header, since a partial list would narrow the
/// pull to whatever survived truncation.
fn parse_sections(raw: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for name in raw.split(',') {
        let legible = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !legible {
            return None;
        }
        out.push(name.to_string());
    }
    Some(out)
}

/// Resolve the client's declared range onto the request, or refuse a disjoint one.
///
/// A 400, because the client's error reader understands `{"error": ...}` and reads a 409 as
/// a row conflict. Refused here rather than in a handler, so no document is parsed under a
/// version it was not written for.
pub async fn contract_gate(mut request: Request, next: Next) -> Response {
    let contract = ClientContract::from_headers(request.headers());
    if contract.disjoint() {
        return AppError::BadRequest(format!(
            "This registry speaks metadata contract {MIN_CONTRACT_VERSION}-{CONTRACT_VERSION} \
             and the client declared {}-{}. Update whichever is older before syncing.",
            contract.min, contract.max
        ))
        .into_response();
    }
    request.extensions_mut().insert(contract);
    next.run(request).await
}

/// Add this server's own range to a response, whatever its status.
///
/// Wraps the gate, so the gate's own refusal names both ranges: the client's in the body,
/// the server's here.
pub async fn stamp_contract(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        HeaderName::from_static(CONTRACT_HEADER),
        SERVER_RANGE.clone(),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("a legible header name"),
                HeaderValue::from_str(value).expect("a legible header value"),
            );
        }
        map
    }

    #[test]
    fn test_absent_headers_are_contract_one_and_no_narrowing() {
        let contract = ClientContract::from_headers(&HeaderMap::new());
        assert_eq!((contract.min, contract.max), (1, 1));
        assert!(contract.sections.is_none());
        assert!(contract.speaks("cover_rows"));
    }

    #[test]
    fn test_bare_version_is_a_single_point_range() {
        assert_eq!(parse_range("3"), Some((3, 3)));
    }

    #[test]
    fn test_inverted_and_unparseable_ranges_are_rejected() {
        assert_eq!(parse_range("4-2"), None);
        assert_eq!(parse_range("1-x"), None);
        assert_eq!(parse_range(""), None);
        assert_eq!(parse_range("1 - 2"), None);
    }

    #[test]
    fn test_malformed_range_reads_as_absent() {
        let contract = ClientContract::from_headers(&headers(&[(CONTRACT_HEADER, "nonsense")]));
        assert_eq!((contract.min, contract.max), (1, 1));
        assert!(!contract.disjoint());
    }

    #[test]
    fn test_malformed_section_list_reads_as_absent() {
        assert_eq!(parse_sections("sites,,videos"), None);
        assert_eq!(parse_sections("Sites"), None);
        assert_eq!(parse_sections("sites, videos"), None);
        let contract = ClientContract::from_headers(&headers(&[(SECTIONS_HEADER, "sites videos")]));
        assert!(contract.sections.is_none());
    }

    #[test]
    fn test_narrowed_client_speaks_only_what_it_declared() {
        let contract = ClientContract::from_headers(&headers(&[(SECTIONS_HEADER, "sites,runs")]));
        assert!(contract.speaks("sites"));
        assert!(contract.speaks("runs"));
        assert!(!contract.speaks("cover_rows"));
    }

    #[test]
    fn test_agreed_version_is_the_lower_maximum() {
        let newer = ClientContract {
            min: 1,
            max: CONTRACT_VERSION + 5,
            sections: None,
        };
        assert_eq!(newer.agreed(), CONTRACT_VERSION);
        assert!(!newer.disjoint());
    }

    #[test]
    fn test_a_range_above_this_server_is_disjoint() {
        let ahead = ClientContract {
            min: CONTRACT_VERSION + 1,
            max: CONTRACT_VERSION + 2,
            sections: None,
        };
        assert!(ahead.disjoint());
    }
}
