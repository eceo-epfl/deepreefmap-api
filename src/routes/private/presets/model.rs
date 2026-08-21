use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A named settings document the server defines and devices download.
///
/// Versions of one name coexist rather than overwrite, so a run's provenance
/// (`preset_name`, `preset_version`) keeps naming the settings it actually ran with.
/// Written through the console only: `presets` is a pull-only sync section.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "preset")]
#[crudcrate(
    api_struct = "Preset",
    name_singular = "preset",
    name_plural = "presets",
    generate_router,
    require_scope,
    deny_unknown_fields,
    delete::one::body = soft_delete_one,
    delete::many::body = soft_delete_many
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    #[crudcrate(filterable, sortable, fulltext)]
    pub name: String,
    #[crudcrate(filterable, sortable)]
    pub version: i32,
    /// The settings document itself, opaque to the registry. Detail view only: a page
    /// of presets does not carry every document.
    #[sea_orm(column_type = "JsonBinary")]
    #[crudcrate(exclude(list))]
    pub settings: serde_json::Value,
    #[crudcrate(fulltext)]
    pub description: String,
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
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Settings are stored as sent apart from what `preset_schema` refuses: values the
/// desktop could not apply, and paths that never leave one machine.
fn validate_settings(
    settings: &serde_json::Value,
) -> Result<(), crudcrate::validation::ValidationError> {
    crate::contract::preset_schema::validate_settings(settings)
        .map_err(|why| crudcrate::validation::ValidationError::new("settings", why))
}

impl crudcrate::validation::Validatable for PresetCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_required("name", &self.name)?;
        crudcrate::validation::validators::validate_range("version", self.version, Some(1), None)?;
        validate_settings(&self.settings)
    }
}

impl crudcrate::validation::Validatable for PresetUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(name)) = &self.name {
            crudcrate::validation::validators::validate_required("name", name)?;
        }
        if let Some(Some(version)) = self.version {
            crudcrate::validation::validators::validate_range("version", version, Some(1), None)?;
        }
        if let Some(Some(settings)) = &self.settings {
            validate_settings(settings)?;
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(Preset);
