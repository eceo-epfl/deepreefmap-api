use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One input clip, identified by content hash. Paths are device-local and absent.
///
/// Which camera of the rig it came from, where that camera sat, and the review verdict
/// are the clip's, not the pass's: one swim is filmed by several cameras at once.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "video_asset")]
#[crudcrate(
    api_struct = "Video",
    name_singular = "video",
    name_plural = "videos",
    generate_router,
    require_scope,
    deny_unknown_fields,
    create::one::post = ledger_created,
    create::many::post = ledger_created_many,
    update::one::post = ledger_updated,
    update::many::post = ledger_updated_many,
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
    /// The camera's name on the rig, as the field team labels it: `GoPro_3`, `cam1`.
    #[crudcrate(filterable, sortable)]
    pub camera_label: Option<String>,
    /// Where the camera sat relative to the diver: `left`, `centre` or `right`.
    #[crudcrate(filterable)]
    pub rig_position: Option<String>,
    /// Mounted inverted, which the reconstruction has to know and no probe can tell.
    #[crudcrate(filterable)]
    pub upside_down: bool,
    /// `unreviewed`, `usable` or `excluded`.
    #[crudcrate(filterable, sortable)]
    pub review: String,
    #[crudcrate(fulltext)]
    pub notes: String,
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
    /// Stamped by the console; from then on a laptop's change is a proposal.
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(filterable, exclude(create, update))]
    pub validated_by: Option<String>,
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

impl crudcrate::validation::Validatable for VideoUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        for (field, value, vocabulary) in [
            (
                "rig_position",
                self.rig_position.as_ref().and_then(|v| v.as_deref()),
                &crate::contract::vocab::RIG_POSITION,
            ),
            (
                "review",
                self.review.as_ref().and_then(|v| v.as_deref()),
                &crate::contract::vocab::VIDEO_REVIEW,
            ),
        ] {
            if let Some(code) = value
                && !vocabulary.codes().contains(&code)
            {
                return Err(crudcrate::validation::ValidationError::new(
                    field,
                    format!("must be one of {}", vocabulary.codes().join(", ")),
                ));
            }
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(Video, "videos");
crate::ledger_hooks!(Video, "videos");
