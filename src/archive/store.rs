//! The S3 client and every operation the archive performs against it.
//!
//! One client, built at startup from [`ArchiveConfig`]. Handlers and background tasks
//! go through these wrappers, so SDK errors surface as [`AppError`] uniformly and no
//! call site assembles its own bucket or key.

use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

use crate::archive::imohash;
use crate::config::ArchiveConfig;
use crate::error::{AppError, AppResult};

/// Fixed part size for multipart uploads. The last part may be smaller.
pub const PART_SIZE_BYTES: i64 = 32 * 1024 * 1024;

/// An upload another initiate may resume: which parts are done, and its age.
pub struct OpenUpload {
    pub key: String,
    pub upload_id: String,
    pub initiated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct UploadedPart {
    pub part_number: i32,
    pub etag: String,
    pub size_bytes: i64,
}

pub struct ArchiveStore {
    client: aws_sdk_s3::Client,
    bucket: String,
    /// Leading segment of every key this store writes.
    pub prefix: String,
    /// Keys the archive fetch links. The S3 secret, because it is already secret and
    /// already shared by every replica, so signing needs no extra deployment secret.
    fetch_secret: Vec<u8>,
}

impl ArchiveStore {
    #[must_use]
    pub fn new(config: &ArchiveConfig) -> Self {
        Self {
            client: client_against(config, &config.endpoint_url),
            bucket: config.bucket.clone(),
            prefix: config.prefix.clone(),
            fetch_secret: config.secret_key.as_bytes().to_vec(),
        }
    }

    /// The key archive fetch links are signed with.
    #[must_use]
    pub fn fetch_secret(&self) -> &[u8] {
        &self.fetch_secret
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
            .map(|part| part.part_number)
            .collect())
    }

    /// Every uploaded part with its `ETag`, across every `ListParts` page, sorted.
    ///
    /// S3 is the authority on what was uploaded. A resuming client only holds
    /// `ETag`s for the parts it sent itself, so assembly must never depend on a
    /// client's list.
    pub async fn uploaded_parts(&self, key: &str, upload_id: &str) -> AppResult<Vec<UploadedPart>> {
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
            parts.extend(page.parts().iter().filter_map(|part| {
                Some(UploadedPart {
                    part_number: part.part_number()?,
                    etag: part.e_tag()?.trim_matches('"').to_string(),
                    size_bytes: part.size()?,
                })
            }));
            if page.is_truncated() != Some(true) {
                break;
            }
            marker = page.next_part_number_marker().map(ToString::to_string);
        }
        parts.sort_unstable_by_key(|part| part.part_number);
        Ok(parts)
    }

    /// Stream one part's bytes into the upload, answering the `ETag` S3 stored it
    /// under (bare, without quotes). The body passes through unbuffered, so memory
    /// stays bounded by the transport's own chunks, not the part size.
    ///
    /// `content_length` is required: a streamed body is unsized and S3 will not
    /// take an `UploadPart` without a length.
    pub async fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: i32,
        content_length: i64,
        content_md5: Option<String>,
        body: ByteStream,
    ) -> AppResult<String> {
        let started = std::time::Instant::now();
        let uploaded = self
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .content_length(content_length)
            .set_content_md5(content_md5)
            .body(body)
            .send()
            .await
            .map_err(|e| internal("UploadPart", &e))?;
        tracing::debug!(
            key,
            part_number,
            content_length,
            elapsed_ms = started.elapsed().as_millis(),
            "Archive part stored"
        );
        uploaded
            .e_tag()
            .map(|etag| etag.trim_matches('"').to_string())
            .ok_or_else(|| AppError::Internal("S3 answered a part without an ETag".to_string()))
    }

    /// The stored object as a stream, with its size, for handing to a response body.
    pub async fn download(&self, key: &str) -> AppResult<(i64, ByteStream)> {
        let got = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| internal("GetObject", &e))?;
        let size = got
            .content_length()
            .ok_or_else(|| AppError::Internal("GetObject answered no size".to_string()))?;
        Ok((size, got.body))
    }

    /// Assemble the upload from S3's own part list, never a client's.
    ///
    /// Returns the number of parts assembled, zero meaning nothing was
    /// uploaded and there is nothing to complete.
    pub async fn complete_multipart(
        &self,
        key: &str,
        upload_id: &str,
        size: i64,
        part_size: i64,
    ) -> AppResult<usize> {
        let parts = self.uploaded_parts(key, upload_id).await?;
        if parts.is_empty() {
            return Ok(0);
        }
        validate_parts(&parts, size, part_size)?;
        let completed: Vec<CompletedPart> = parts
            .iter()
            .map(|part| {
                CompletedPart::builder()
                    .part_number(part.part_number)
                    .e_tag(&part.etag)
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

    /// The stored object's size in bytes, or `None` when no object sits at `key`.
    pub async fn object_size(&self, key: &str) -> AppResult<Option<i64>> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(head) => head
                .content_length()
                .map(Some)
                .ok_or_else(|| AppError::Internal("HeadObject answered no size".to_string())),
            Err(e)
                if e.as_service_error().is_some_and(
                    aws_sdk_s3::operation::head_object::HeadObjectError::is_not_found,
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(internal("HeadObject", &e)),
        }
    }

    /// `len` bytes of the object at `key`, starting at `start`.
    pub async fn get_range(&self, key: &str, start: u64, len: u64) -> AppResult<Vec<u8>> {
        let got = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .range(format!("bytes={start}-{}", start + len - 1))
            .send()
            .await
            .map_err(|e| internal("GetObject", &e))?;
        let data = got
            .body
            .collect()
            .await
            .map_err(|e| internal("GetObject body", &e))?
            .into_bytes();
        if data.len() as u64 != len {
            return Err(AppError::Internal(format!(
                "GetObject answered {} bytes for a {len} byte range",
                data.len()
            )));
        }
        Ok(data.to_vec())
    }

    /// Remove an object, tolerating one that is already gone. Verification cleanup
    /// only: no route exposes deletion.
    pub async fn delete_object(&self, key: &str) {
        if let Err(e) = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            tracing::debug!(key, "DeleteObject: {}", DisplayErrorContext(&e));
        }
    }

    /// The imohash of the stored object, computed the way a device computes it:
    /// whole under the sampling threshold, three windows above it.
    pub async fn computed_imohash(&self, key: &str, size: i64) -> AppResult<String> {
        let size = u64::try_from(size)
            .map_err(|_| AppError::Internal(format!("Cannot hash a {size} byte object")))?;
        if size < imohash::SAMPLE_THRESHOLD {
            let data = self.get_range(key, 0, size).await?;
            return Ok(imohash::hash_whole(&data));
        }
        let mut samples = Vec::with_capacity(3);
        for (start, len) in imohash::sample_windows(size) {
            samples.push(self.get_range(key, start, len).await?);
        }
        Ok(imohash::hash_sampled(
            size,
            &samples[0],
            &samples[1],
            &samples[2],
        ))
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
        .request_checksum_calculation(aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired)
        // MinIO ignores the region but sigv4 needs one to sign under.
        .region(aws_config::Region::new("us-east-1"))
        .endpoint_url(endpoint_url)
        .credentials_provider(credentials)
        // MinIO serves no virtual-host buckets.
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(s3_config)
}

/// SDK error with its full source chain, logged by [`AppError::Internal`] and never
/// shown to the caller.
fn internal<E: std::error::Error>(operation: &str, error: &E) -> AppError {
    let detail = format!("{}", DisplayErrorContext(error));
    tracing::error!(operation, error = %detail, "Archive storage request failed");
    let code = if detail.contains("trailing checksum is not supported") {
        "archive_storage_incompatible"
    } else if detail.contains("NoSuchUpload") {
        "archive_upload_missing"
    } else if detail.contains("BadDigest") {
        "archive_integrity"
    } else {
        "archive_storage_unavailable"
    };
    AppError::Archive {
        code,
        message: format!("Archive storage failed during {operation}"),
    }
}

fn validate_parts(parts: &[UploadedPart], size: i64, part_size: i64) -> AppResult<()> {
    let count = (size + part_size - 1) / part_size;
    if i64::try_from(parts.len()).ok() != Some(count) {
        return Err(AppError::Conflict(
            "The upload is missing parts".to_string(),
        ));
    }
    for (index, part) in parts.iter().enumerate() {
        let number = i64::try_from(index).expect("part count fits i64") + 1;
        let expected = part_size.min(size - (number - 1) * part_size);
        if i64::from(part.part_number) != number || part.size_bytes != expected {
            return Err(AppError::Conflict(
                "The upload has an invalid part size or sequence".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/store.rs"]
mod tests;
