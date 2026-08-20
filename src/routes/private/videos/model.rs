use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One input clip, identified by content hash. Paths are device-local and absent.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "video_asset")]
#[crudcrate(
    api_struct = "Video",
    name_singular = "video",
    name_plural = "videos",
    generate_router,
    read::one::body = get_live_one,
    delete::one::body = soft_delete_one,
    delete::many::body = soft_delete_many
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    /// The sampled imohash a device computes at ingest. Nullable, because an
    /// unreadable clip still deserves a row, and unique when present. Also what
    /// the archive keys this clip's blob on, so a stored object and this row
    /// meet without either side reading the whole file.
    #[crudcrate(filterable)]
    pub hash: Option<String>,
    #[crudcrate(filterable, sortable, fulltext)]
    pub file_name: String,
    #[crudcrate(sortable)]
    pub size_bytes: Option<i64>,
    #[crudcrate(sortable)]
    pub duration_s: Option<f64>,
    pub fps: Option<f64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    #[crudcrate(filterable)]
    pub codec: Option<String>,
    #[crudcrate(filterable, sortable)]
    pub captured_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Where `captured_at` came from: a container stamp and an mtime differ in trust.
    pub captured_source: Option<String>,
    /// Tri-state `yes`/`no`/`unknown`: a camera that recorded none differs from unread.
    #[crudcrate(filterable)]
    pub gravity: String,
    #[crudcrate(filterable)]
    pub gps: String,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The conflict key last-write-wins resolves on. Server-stamped: `on_update` only
    /// fires for a field the update model excludes, and a client-set stamp could pin a
    /// row against every later push. `/api/sync/push` writes it directly instead.
    #[crudcrate(sortable, exclude(create, update), on_create = chrono::Utc::now(), on_update = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The tombstone. Written only by the delete route, which administrators alone
    /// reach, and by `/api/sync/push`. Accepting it on create or update would let a
    /// member delete through a plain edit.
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Provenance, stamped from the credential by `/api/sync/push`. Never accepted
    /// from a client, which would otherwise forge another device.
    #[crudcrate(filterable, exclude(create, update))]
    pub device_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable)]
    pub server_seq: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::routes::private::passes::pass_video::Entity")]
    PassVideo,
}

impl Related<crate::routes::private::passes::pass_video::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PassVideo.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(Video);
