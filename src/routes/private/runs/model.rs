use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One execution of a pass through the reconstruction pipeline.
///
/// A report of a reconstruction that already ran, not a request to run one. Re-running
/// appends a record rather than overwriting: the repeats are the reproducibility data.
/// The provenance columns come from the run manifest and name the software, taxonomy
/// and weights behind the cover numbers.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "run_record")]
#[crudcrate(
    api_struct = "Run",
    name_singular = "run",
    name_plural = "runs",
    generate_router,
    read::one::body = get_live_one,
    delete::one::body = soft_delete_one,
    delete::many::body = soft_delete_many
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    #[crudcrate(filterable)]
    pub pass_id: Uuid,
    #[crudcrate(filterable, sortable)]
    pub status: String,
    #[crudcrate(filterable, sortable)]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(filterable, sortable)]
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: String,
    /// Relative to the producing device's output root, not a server path.
    pub run_dir_name: String,
    #[crudcrate(filterable)]
    pub gui_version: Option<String>,
    #[crudcrate(filterable)]
    pub library_version: Option<String>,
    #[crudcrate(filterable)]
    pub segmentation_model: Option<String>,
    #[crudcrate(filterable)]
    pub mapping_backend: Option<String>,
    #[crudcrate(filterable)]
    pub taxonomy_version: Option<i32>,
    /// Digest of the class-groups definition, so a claimed version can be checked.
    pub taxonomy_hash: Option<String>,
    /// Repository to upstream revision present at launch. Best-effort: the version
    /// available, not proof it was loaded.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub model_revisions: Option<serde_json::Value>,
    #[crudcrate(filterable)]
    pub preset_name: Option<String>,
    /// Settings that departed from the preset, separating "unchanged" from "unrecorded".
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub preset_deviations: Option<serde_json::Value>,
    #[crudcrate(filterable)]
    pub preset_version: Option<i32>,
    /// Digest of the preset definition, so a claimed version can be checked.
    pub preset_hash: Option<String>,
    /// Wall-clock seconds for the whole run.
    #[crudcrate(sortable)]
    pub run_duration_s: Option<f64>,
    /// Stage name to wall-clock seconds, so a slow run names its slow stage.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub stage_durations: Option<serde_json::Value>,
    /// Stage name to peak resource use, so an out-of-memory run stays explicable.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub stage_peaks: Option<serde_json::Value>,
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
    #[sea_orm(
        belongs_to = "crate::routes::private::passes::model::Entity",
        from = "Column::PassId",
        to = "crate::routes::private::passes::model::Column::Id"
    )]
    Pass,
    #[sea_orm(has_many = "crate::routes::private::cover::model::Entity")]
    CoverRow,
}

impl Related<crate::routes::private::passes::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Pass.def()
    }
}

impl Related<crate::routes::private::cover::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CoverRow.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(Run);
