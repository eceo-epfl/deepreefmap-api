use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A named reef location. Transect names are unique within a site.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "site")]
#[crudcrate(
    api_struct = "Site",
    name_singular = "site",
    name_plural = "sites",
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
    #[crudcrate(filterable, sortable, fulltext)]
    pub country: Option<String>,
    #[crudcrate(filterable, fulltext)]
    pub region: Option<String>,
    #[crudcrate(fulltext)]
    pub description: String,
    /// Representative point, not a boundary.
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
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
    /// Sync ordering, stamped by trigger. Never accepted from a client.
    #[crudcrate(exclude(create, update), sortable)]
    pub server_seq: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::routes::private::transects::model::Entity")]
    Transect,
}

impl Related<crate::routes::private::transects::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Transect.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl crudcrate::validation::Validatable for SiteCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_required("name", &self.name)?;
        if let Some(latitude) = self.latitude {
            crate::common::validate::latitude("latitude", latitude)?;
        }
        if let Some(longitude) = self.longitude {
            crate::common::validate::longitude("longitude", longitude)?;
        }
        Ok(())
    }
}

impl crudcrate::validation::Validatable for SiteUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(name)) = &self.name {
            crudcrate::validation::validators::validate_required("name", name)?;
        }
        if let Some(Some(latitude)) = self.latitude {
            crate::common::validate::latitude("latitude", latitude)?;
        }
        if let Some(Some(longitude)) = self.longitude {
            crate::common::validate::longitude("longitude", longitude)?;
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(Site);
