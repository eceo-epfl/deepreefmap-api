//! The S3 client and every operation the archive performs against it.
//!
//! One client, built at startup from [`ArchiveConfig`]. Handlers and background tasks
//! go through these wrappers, so SDK errors surface as [`AppError`] uniformly and no
//! call site assembles its own bucket or key.

use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use std::time::Duration;

use crate::config::ArchiveConfig;
use crate::error::{AppError, AppResult};

/// How long a presigned URL stays valid. Short: the client asked seconds ago, and a
/// client still uploading when it lapses re-initiates for fresh ones.
pub const PRESIGN_TTL_SECONDS: u64 = 15 * 60;
const PRESIGN_TTL: Duration = Duration::from_secs(PRESIGN_TTL_SECONDS);

/// Fixed part size for multipart uploads. The last part may be smaller.
pub const PART_SIZE_BYTES: i64 = 32 * 1024 * 1024;

/// An upload another initiate may resume: which parts are done, and its age.
pub struct OpenUpload {
    pub key: String,
    pub upload_id: String,
    pub initiated_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct ArchiveStore {
    client: aws_sdk_s3::Client,
    /// Signs the URLs handed to clients. The same client as `client` unless a public
    /// endpoint is configured: a presigned URL embeds its endpoint, and the address a
    /// client reaches the store at need not be the one this server uses.
    presign_client: aws_sdk_s3::Client,
    bucket: String,
    /// Leading segment of every key this store writes.
    pub prefix: String,
}

impl ArchiveStore {
    #[must_use]
    pub fn new(config: &ArchiveConfig) -> Self {
        let client = client_against(config, &config.endpoint_url);
        let presign_client = match config.public_endpoint_url.as_deref() {
            Some(public) => client_against(config, public),
            None => client.clone(),
        };
        Self {
            client,
            presign_client,
            bucket: config.bucket.clone(),
            prefix: config.prefix.clone(),
        }
    }

    pub async fn create_multipart(&self, key: &str) -> AppResult<String> {
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| internal("CreateMultipartUpload", &e))?;
        created
            .upload_id()
            .map(ToString::to_string)
            .ok_or_else(|| AppError::Internal("S3 answered no upload id".to_string()))
    }

    /// Abort an upload, tolerating one that is already gone.
    pub async fn abort_multipart(&self, key: &str, upload_id: &str) {
        if let Err(e) = self
            .client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
        {
            tracing::debug!(key, "AbortMultipartUpload: {}", DisplayErrorContext(&e));
        }
    }

    /// Part numbers already uploaded, for a resuming client to skip.
    pub async fn uploaded_part_numbers(&self, key: &str, upload_id: &str) -> AppResult<Vec<i32>> {
        Ok(self
            .uploaded_parts(key, upload_id)
            .await?
            .into_iter()
            .map(|(number, _)| number)
            .collect())
    }

    /// Every uploaded part with its `ETag`, across every `ListParts` page, sorted.
    ///
    /// S3 is the authority on what was uploaded. A resuming client only holds
    /// `ETag`s for the parts it sent itself, so assembly must never depend on a
    /// client's list.
    pub async fn uploaded_parts(
        &self,
        key: &str,
        upload_id: &str,
    ) -> AppResult<Vec<(i32, String)>> {
        let mut parts = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let page = self
                .client
                .list_parts()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(upload_id)
                .set_part_number_marker(marker)
                .send()
                .await
                .map_err(|e| internal("ListParts", &e))?;
            parts.extend(
                page.parts()
                    .iter()
                    .filter_map(|part| Some((part.part_number()?, part.e_tag()?.to_string()))),
            );
            if page.is_truncated() != Some(true) {
                break;
            }
            marker = page.next_part_number_marker().map(ToString::to_string);
        }
        parts.sort_unstable_by_key(|(number, _)| *number);
        Ok(parts)
    }

    pub async fn presign_upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: i32,
    ) -> AppResult<String> {
        let presigned = self
            .presign_client
            .upload_part()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .presigned(presign_config()?)
            .await
            .map_err(|e| internal("presign UploadPart", &e))?;
        Ok(presigned.uri().to_string())
    }

    pub async fn presign_get(&self, key: &str) -> AppResult<String> {
        let presigned = self
            .presign_client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presign_config()?)
            .await
            .map_err(|e| internal("presign GetObject", &e))?;
        Ok(presigned.uri().to_string())
    }

    /// Assemble the upload from S3's own part list, never a client's.
    ///
    /// Returns the number of parts assembled, zero meaning nothing was
    /// uploaded and there is nothing to complete.
    pub async fn complete_multipart(&self, key: &str, upload_id: &str) -> AppResult<usize> {
        let parts = self.uploaded_parts(key, upload_id).await?;
        if parts.is_empty() {
            return Ok(0);
        }
        let completed: Vec<CompletedPart> = parts
            .iter()
            .map(|(number, etag)| {
                CompletedPart::builder()
                    .part_number(*number)
                    .e_tag(etag)
                    .build()
            })
            .collect();
        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(completed))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| internal("CompleteMultipartUpload", &e))?;
        Ok(parts.len())
    }

    /// Every multipart upload open under this store's prefix.
    pub async fn open_uploads(&self) -> AppResult<Vec<OpenUpload>> {
        let mut found = Vec::new();
        let mut key_marker: Option<String> = None;
        let mut id_marker: Option<String> = None;
        loop {
            let page = self
                .client
                .list_multipart_uploads()
                .bucket(&self.bucket)
                .prefix(format!("{}/", self.prefix))
                .set_key_marker(key_marker)
                .set_upload_id_marker(id_marker)
                .send()
                .await
                .map_err(|e| internal("ListMultipartUploads", &e))?;
            for upload in page.uploads() {
                let (Some(key), Some(upload_id)) = (upload.key(), upload.upload_id()) else {
                    continue;
                };
                found.push(OpenUpload {
                    key: key.to_string(),
                    upload_id: upload_id.to_string(),
                    initiated_at: upload
                        .initiated()
                        .and_then(|t| chrono::DateTime::from_timestamp(t.secs(), 0)),
                });
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            key_marker = page.next_key_marker().map(ToString::to_string);
            id_marker = page.next_upload_id_marker().map(ToString::to_string);
        }
        Ok(found)
    }
}

fn client_against(config: &ArchiveConfig, endpoint_url: &str) -> aws_sdk_s3::Client {
    let credentials = aws_sdk_s3::config::Credentials::new(
        config.access_key.clone(),
        config.secret_key.clone(),
        None,
        None,
        "deepreefmap",
    );
    let s3_config = aws_sdk_s3::config::Builder::new()
        .behavior_version(aws_config::BehaviorVersion::latest())
        // MinIO ignores the region but sigv4 needs one to sign under.
        .region(aws_config::Region::new("us-east-1"))
        .endpoint_url(endpoint_url)
        .credentials_provider(credentials)
        // MinIO serves no virtual-host buckets.
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_config)
}

fn presign_config() -> AppResult<PresigningConfig> {
    PresigningConfig::expires_in(PRESIGN_TTL)
        .map_err(|e| AppError::Internal(format!("presigning config: {e}")))
}

/// SDK error with its full source chain, logged by [`AppError::Internal`] and never
/// shown to the caller.
fn internal<E: std::error::Error>(operation: &str, error: &E) -> AppError {
    AppError::Internal(format!("{operation}: {}", DisplayErrorContext(error)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(public_endpoint_url: Option<&str>) -> ArchiveConfig {
        ArchiveConfig {
            endpoint_url: "http://minio.internal:9000".to_string(),
            public_endpoint_url: public_endpoint_url.map(String::from),
            bucket: "reef".to_string(),
            access_key: "key".to_string(),
            secret_key: "secret".to_string(),
            prefix: "test".to_string(),
        }
    }

    /// Presigned URLs are all a client ever touches, so they carry the public
    /// address. Presigning is local computation: no endpoint is contacted here.
    #[tokio::test]
    async fn test_presigned_urls_carry_the_public_endpoint() {
        let store = ArchiveStore::new(&config(Some("http://localhost:9000")));
        let get = store.presign_get("test/videos/imohash/abc").await.unwrap();
        assert!(
            get.starts_with("http://localhost:9000/reef/test/videos/imohash/abc?"),
            "{get}"
        );
        let part = store
            .presign_upload_part("test/videos/imohash/abc", "upload-1", 2)
            .await
            .unwrap();
        assert!(part.starts_with("http://localhost:9000/reef/"), "{part}");
        assert!(part.contains("partNumber=2"), "{part}");
    }

    #[tokio::test]
    async fn test_presigning_defaults_to_the_primary_endpoint() {
        let store = ArchiveStore::new(&config(None));
        let url = store.presign_get("test/videos/imohash/abc").await.unwrap();
        assert!(url.starts_with("http://minio.internal:9000/reef/"), "{url}");
    }
}
