use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One measurement of one camera profile.
///
/// Versions of a profile coexist rather than overwrite, as a preset's do: a housing
/// change or a firmware update invalidates the last measurement without invalidating
/// the runs made under it. `document` is the profile JSON the pipeline reads, stored
/// as the desktop writes it, and is what a device materialises into its own profile
/// directory on pull. The columns beside it are that document's own facts, lifted out
/// so the console can sort and filter on them without opening every document.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "camera_calibration")]
#[crudcrate(
    api_struct = "CameraCalibration",
    name_singular = "camera_calibration",
    name_plural = "camera_calibrations",
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
    #[crudcrate(filterable, sortable)]
    pub camera_profile_id: Uuid,
    #[crudcrate(filterable, sortable)]
    pub version: i32,
    /// The profile JSON itself, opaque to the registry beyond the shape check on
    /// write. Detail view only: a page of calibrations does not carry every document.
    #[sea_orm(column_type = "JsonBinary")]
    #[crudcrate(exclude(list))]
    pub document: serde_json::Value,
    #[crudcrate(filterable, sortable)]
    pub image_width: Option<i32>,
    #[crudcrate(filterable, sortable)]
    pub image_height: Option<i32>,
    /// What COLMAP reported when this was measured. The one number that says whether
    /// a calibration is worth trusting.
    #[crudcrate(filterable, sortable)]
    pub reprojection_error_px: Option<f64>,
    #[crudcrate(filterable, sortable)]
    pub registered_frames: Option<i32>,
    /// The clip it was measured from, by name. Empty where nobody recorded one.
    #[crudcrate(fulltext)]
    pub source_clip: String,
    #[crudcrate(filterable, sortable)]
    pub calibrated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(fulltext)]
    pub description: String,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The conflict key last-write-wins resolves on. Server-stamped, as everywhere.
    #[crudcrate(sortable, exclude(create, update), on_create = chrono::Utc::now(), on_update = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The device that measured it, where a device published it.
    #[crudcrate(filterable, exclude(create, update))]
    pub device_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable)]
    pub server_seq: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::model::Entity",
        from = "Column::CameraProfileId",
        to = "super::model::Column::Id"
    )]
    CameraProfile,
}

impl Related<super::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CameraProfile.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// Refuse a document the pipeline could not load.
///
/// Not a full schema: the registry stores what the desktop writes and the desktop
/// reads it back. These are the keys `CameraProfile.load` reaches for without
/// guarding, so a document missing one fails on a field laptop mid-run instead of
/// here.
pub fn validate_document(document: &serde_json::Value) -> Result<(), String> {
    let map = document
        .as_object()
        .ok_or_else(|| "document must be a JSON object".to_string())?;
    for key in ["name", "distorted", "rectified_pinhole"] {
        if !map.contains_key(key) {
            return Err(format!("document is missing {key}"));
        }
    }
    let distorted = map["distorted"]
        .as_object()
        .ok_or_else(|| "distorted must be an object".to_string())?;
    if !distorted.contains_key("params") {
        return Err("document is missing distorted.params".to_string());
    }
    let rectified = map["rectified_pinhole"]
        .as_object()
        .ok_or_else(|| "rectified_pinhole must be an object".to_string())?;
    for key in ["image_size", "K"] {
        if !rectified.contains_key(key) {
            return Err(format!("document is missing rectified_pinhole.{key}"));
        }
    }
    Ok(())
}

fn document_error(why: String) -> crudcrate::validation::ValidationError {
    crudcrate::validation::ValidationError::new("document", why)
}

impl crudcrate::validation::Validatable for CameraCalibrationCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_range("version", self.version, Some(1), None)?;
        validate_document(&self.document).map_err(document_error)
    }
}

impl crudcrate::validation::Validatable for CameraCalibrationUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(version)) = self.version {
            crudcrate::validation::validators::validate_range("version", version, Some(1), None)?;
        }
        if let Some(Some(document)) = &self.document {
            validate_document(document).map_err(document_error)?;
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(CameraCalibration, "camera_calibrations");
crate::ledger_hooks!(CameraCalibration, "camera_calibrations");
