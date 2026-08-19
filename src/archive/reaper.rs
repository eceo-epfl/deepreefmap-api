//! Periodic cleanup of uploads that stalled and multipart uploads nothing claims.

use chrono::{TimeDelta, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::sync::Arc;
use std::time::Duration;

use crate::archive::store::ArchiveStore;
use crate::common::AppState;
use crate::error::AppResult;
use crate::routes::archive::model as stored_object;

/// Sweep on an interval for as long as the process runs.
pub fn spawn(state: AppState) {
    let Some(store) = state.archive.clone() else {
        return;
    };
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(
            state.config.archive_sweep_seconds.max(1),
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = sweep(&state, &store).await {
                tracing::warn!("Archive sweep failed: {e}");
            }
        }
    });
}

/// One pass: abandon stalled uploads, then abort orphaned multipart uploads.
async fn sweep(state: &AppState, store: &Arc<ArchiveStore>) -> AppResult<()> {
    let now = Utc::now();
    let timeout = TimeDelta::seconds(state.config.archive_upload_timeout_seconds);

    // Pending rows idle past the timeout. Few rows are ever pending, so the idle
    // clock (`last_part_at`, falling back to `created_at`) is evaluated here.
    let pending = stored_object::Entity::find()
        .filter(stored_object::Column::Status.eq(stored_object::STATUS_PENDING))
        .all(&state.db)
        .await?;
    for row in pending {
        if now - row.last_part_at.unwrap_or(row.created_at) < timeout {
            continue;
        }
        if let Some(upload_id) = row.s3_upload_id.as_deref() {
            store.abort_multipart(&row.s3_key, upload_id).await;
        }
        let object_id = row.id;
        let mut update: stored_object::ActiveModel = row.into();
        update.status = Set(stored_object::STATUS_FAILED.to_string());
        update.s3_upload_id = Set(None);
        update.failure = Set(Some(
            "Upload abandoned: no part activity before the timeout".to_string(),
        ));
        update.updated_at = Set(now);
        update.update(&state.db).await?;
        tracing::info!(%object_id, "Abandoned a stalled upload");
    }

    // Belt and braces: multipart uploads in the bucket that no pending row claims,
    // such as one initiated just before a crash. Only past the timeout, so an upload
    // whose row is mid-write is left alone.
    for upload in store.open_uploads().await? {
        let old_enough = upload
            .initiated_at
            .is_none_or(|initiated| now - initiated >= timeout);
        if !old_enough {
            continue;
        }
        let claimed = stored_object::Entity::find()
            .filter(stored_object::Column::S3UploadId.eq(&upload.upload_id))
            .filter(stored_object::Column::Status.eq(stored_object::STATUS_PENDING))
            .one(&state.db)
            .await?
            .is_some();
        if !claimed {
            store.abort_multipart(&upload.key, &upload.upload_id).await;
            tracing::info!(key = %upload.key, "Aborted an orphaned multipart upload");
        }
    }

    Ok(())
}
