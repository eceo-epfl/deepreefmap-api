use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A named rig: a body, a lens mode, a housing, a resolution.
///
/// Holds no measurements of its own; those are its calibrations. The name is what a
/// preset and a run record carry, and what a device resolves a profile file by, so it
/// is unique across the registry rather than per anything.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "camera_profile")]
#[crudcrate(
    api_struct = "CameraProfile",
    name_singular = "camera_profile",
    name_plural = "camera_profiles",
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
    /// Resolved by a device against its own profile directory, so it follows the
    /// library's own charset: letters, numbers, underscores and hyphens.
    #[crudcrate(filterable, sortable, fulltext)]
    pub name: String,
    #[crudcrate(fulltext)]
    pub description: String,
    /// Which calibration laptops run this rig under. None follows the newest, which
    /// is what a profile does until a curator deploys a particular measurement.
    /// Excluded from create: a profile has no calibrations at the moment it is made.
    #[crudcrate(filterable, exclude(create))]
    pub current_calibration_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The conflict key last-write-wins resolves on. Server-stamped: `on_update` only
    /// fires for a field the update model excludes, and a client-set stamp could pin a
    /// row against every later push. `/api/sync/push` writes it directly instead.
    #[crudcrate(sortable, exclude(create, update), on_create = chrono::Utc::now(), on_update = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The tombstone. Written only by the delete route, which administrators alone
    /// reach, and by `/api/sync/push`.
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The device that published the profile, where one did. Never accepted from a
    /// client, which would otherwise forge another device.
    #[crudcrate(filterable, exclude(create, update))]
    pub device_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable)]
    pub server_seq: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::calibration::Entity")]
    CameraCalibration,
}

impl Related<super::calibration::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CameraCalibration.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// The charset the library validates a profile name against before it will load one.
#[must_use]
pub fn name_is_resolvable(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl crudcrate::validation::Validatable for CameraProfileUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(name)) = &self.name {
            crudcrate::validation::validators::validate_required("name", name)?;
            if !name_is_resolvable(name) {
                return Err(crudcrate::validation::ValidationError::new(
                    "name",
                    "letters, numbers, underscores and hyphens only",
                ));
            }
        }
        Ok(())
    }
}

impl crudcrate::validation::Validatable for CameraProfileCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_required("name", &self.name)?;
        if !name_is_resolvable(&self.name) {
            return Err(crudcrate::validation::ValidationError::new(
                "name",
                "letters, numbers, underscores and hyphens only: a device resolves a \
                 profile by this name on its own disk",
            ));
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(CameraProfile, "camera_profiles");
crate::ledger_hooks!(CameraProfile, "camera_profiles");
