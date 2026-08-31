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
    /// The processing configuration the run used: resolution and fps set the memory
    /// regime, batch size gates VRAM. Integer fps, as the preset schema defines it.
    #[crudcrate(filterable)]
    pub processing_width: Option<i32>,
    #[crudcrate(filterable)]
    pub processing_height: Option<i32>,
    #[crudcrate(filterable)]
    pub fps: Option<i32>,
    #[crudcrate(filterable)]
    pub preprocess_batch_size: Option<i32>,
    #[crudcrate(filterable)]
    pub taxonomy_version: Option<i32>,
    /// Digest of the class-groups definition, so a claimed version can be checked.
    #[crudcrate(filterable)]
    pub taxonomy_hash: Option<String>,
    /// Repository to upstream revision present at launch. Best-effort: the version
    /// available, not proof it was loaded. Detail view only.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    #[crudcrate(exclude(list))]
    pub model_revisions: Option<serde_json::Value>,
    #[crudcrate(filterable)]
    pub preset_name: Option<String>,
    /// Settings that departed from the preset, separating "unchanged" from "unrecorded".
    /// Detail view only.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    #[crudcrate(exclude(list))]
    pub preset_deviations: Option<serde_json::Value>,
    #[crudcrate(filterable)]
    pub preset_version: Option<i32>,
    /// Digest of the preset definition, so a claimed version can be checked.
    #[crudcrate(filterable)]
    pub preset_hash: Option<String>,
    /// Wall-clock seconds for the whole run.
    #[crudcrate(sortable)]
    pub run_duration_s: Option<f64>,
    /// Stage name to wall-clock seconds, so a slow run names its slow stage. Detail
    /// view only.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    #[crudcrate(exclude(list))]
    pub stage_durations: Option<serde_json::Value>,
    /// Stage name to peak resource use, so an out-of-memory run stays explicable.
    /// Detail view only.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    #[crudcrate(exclude(list))]
    pub stage_peaks: Option<serde_json::Value>,
    /// The scale the cover was measured at: the camera profile, the tape length and
    /// crop width the run used, the metres per pixel that gave, and how the scale
    /// was established.
    #[crudcrate(filterable)]
    pub camera_profile: Option<String>,
    pub pixel_size_m: Option<f64>,
    #[crudcrate(filterable)]
    pub scale_type: Option<String>,
    pub transect_length_m: Option<f64>,
    pub crop_width_m: Option<f64>,
    /// The preset row the run ran under, where the device knew it.
    #[crudcrate(filterable)]
    pub preset_id: Option<Uuid>,
    /// Which measurement of the lens rectified this run's frames, where the device
    /// knew one. The run's own outputs carry the document; this resolves it to what
    /// the console holds.
    #[crudcrate(filterable)]
    pub camera_calibration_id: Option<Uuid>,
    /// The device session this run was processed in: the queue it was ordered from,
    /// which groups the runs that went through the pipeline together. A correlation
    /// key, not a foreign key -- a session is one workstation's cart and has no row
    /// here.
    #[crudcrate(filterable)]
    pub batch_id: Option<Uuid>,
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

crate::soft_delete_hooks!(Run, "runs");
crate::ledger_hooks!(Run, "runs");
