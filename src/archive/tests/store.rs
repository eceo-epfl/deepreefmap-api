use super::*;

fn part(number: i32, size: i64) -> UploadedPart {
    UploadedPart {
        part_number: number,
        etag: "receipt".to_string(),
        size_bytes: size,
    }
}

#[test]
fn test_validate_parts_complete_sequence() {
    assert!(validate_parts(&[part(1, 8), part(2, 3)], 11, 8).is_ok());
}

#[test]
fn test_validate_parts_rejects_missing_or_truncated_parts() {
    assert!(validate_parts(&[part(1, 8)], 11, 8).is_err());
    assert!(validate_parts(&[part(1, 7), part(2, 4)], 11, 8).is_err());
    assert!(validate_parts(&[part(1, 8), part(3, 3)], 11, 8).is_err());
}

#[tokio::test]
async fn test_upload_part_stream_preserves_bytes_without_checksum_trailers() {
    use axum::{
        Router,
        http::{HeaderMap, StatusCode},
        routing::put,
    };
    use std::sync::{Arc, Mutex};

    let captured = Arc::new(Mutex::new(None));
    let received = captured.clone();
    let app = Router::new().route(
        "/bucket/object",
        put(move |headers: HeaderMap, body: axum::body::Bytes| {
            let received = received.clone();
            async move {
                *received.lock().unwrap() = Some((headers, body));
                (StatusCode::OK, [("etag", "\"receipt\"")])
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let store = ArchiveStore::new(&ArchiveConfig {
        endpoint_url: endpoint,
        bucket: "bucket".to_string(),
        access_key: "test".to_string(),
        secret_key: "test".to_string(),
        prefix: "test".to_string(),
    });
    let body = ByteStream::from_body_1_x(http_body_util::Full::new(bytes::Bytes::from_static(
        b"reef",
    )));
    let checksum = "lJgbRHlHweavXYvh4mLdfg==";
    let result = store
        .upload_part("object", "upload", 1, 4, Some(checksum.to_string()), body)
        .await;
    server.abort();
    assert_eq!(result.unwrap(), "receipt");
    let held = captured.lock().unwrap();
    let (headers, bytes) = held.as_ref().unwrap();
    assert_eq!(bytes.as_ref(), b"reef");
    assert_eq!(headers["content-md5"], checksum);
    assert!(!headers.contains_key("x-amz-trailer"));
    assert!(
        headers
            .get("content-encoding")
            .is_none_or(|value| value != "aws-chunked")
    );
}
