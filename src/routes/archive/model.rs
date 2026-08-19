use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One archived blob, keyed by content hash.
///
/// The hash is imohash, the same sampled identity a device gives a clip at ingest, so
/// a blob and its `video_asset` meet on a value both sides already hold. Integrity of
/// the bytes in flight is S3's job: every part is checked against the `ETag` it answers.
///
/// Written only by the archive endpoints, so the generated router is read-only: the
/// console browses, nothing edits, and nothing deletes.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "stored_object")]
#[crudcrate(
    api_struct = "StoredObject",
    name_singular = "stored_object",
    name_plural = "stored_objects",
    generate_router
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    /// imohash of the file, 32 lowercase hex. The join key to `video_asset.hash`.
    #[crudcrate(filterable, exclude(update))]
    pub content_hash: String,
    #[crudcrate(sortable, exclude(update))]
    pub size_bytes: i64,
    /// `video` or `artifact`, deciding the key layout.
    #[crudcrate(filterable, exclude(update))]
    pub kind: String,
    /// `pending`, `failed` or `complete`.
    #[crudcrate(filterable, sortable)]
    pub status: String,
    #[crudcrate(exclude(update))]
    pub s3_key: String,
    /// Live multipart upload handle. Internal plumbing, so no read model carries it.
    #[crudcrate(exclude(list, one))]
    pub s3_upload_id: Option<String>,
    pub part_size_bytes: Option<i64>,
    #[crudcrate(filterable, exclude(update))]
    pub uploaded_by_device_id: Option<Uuid>,
    /// Keycloak subject, when a person uploaded through the console.
    #[crudcrate(filterable, exclude(update))]
    pub uploaded_by: Option<String>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Last part activity, so the reaper measures idleness rather than age.
    #[crudcrate(exclude(create, update), sortable)]
    pub last_part_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When S3 assembled the parts into the finished object.
    #[crudcrate(exclude(create, update), sortable)]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Why `status` is `failed`, for the operator.
    #[crudcrate(exclude(create, update))]
    pub failure: Option<String>,
}

/// Status values, written and matched in one vocabulary.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_COMPLETE: &str = "complete";

pub const KIND_VIDEO: &str = "video";
pub const KIND_ARTIFACT: &str = "artifact";

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::run_artifact::Entity")]
    RunArtifact,
}

impl Related<super::run_artifact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RunArtifact.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
