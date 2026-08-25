use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A survey line: two georeferenced end points plus the tape length used to scale the
/// reconstruction. `length_m` is the tape reading, not the geodesic distance.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "transect")]
#[crudcrate(
    api_struct = "Transect",
    name_singular = "transect",
    name_plural = "transects",
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
    pub site_id: Option<Uuid>,
    /// Unique per site, not globally.
    #[crudcrate(filterable, sortable, fulltext)]
    pub name: String,
    #[crudcrate(fulltext)]
    pub description: String,
    /// End points are nullable: the historical lines mostly have none.
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
    /// Accuracy is per end point, as the field records have it.
    pub start_accuracy_m: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lon: Option<f64>,
    pub end_accuracy_m: Option<f64>,
    #[crudcrate(filterable, sortable)]
    pub length_m: Option<f64>,
    #[crudcrate(filterable, sortable)]
    pub depth_m: Option<f64>,
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
        belongs_to = "crate::routes::private::sites::model::Entity",
        from = "Column::SiteId",
        to = "crate::routes::private::sites::model::Column::Id"
    )]
    Site,
    #[sea_orm(has_many = "crate::routes::private::passes::model::Entity")]
    TransectPass,
}

impl Related<crate::routes::private::sites::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Site.def()
    }
}

impl Related<crate::routes::private::passes::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TransectPass.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl crudcrate::validation::Validatable for TransectCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_required("name", &self.name)?;
        if let Some(start_lat) = self.start_lat {
            crate::common::validate::latitude("start_lat", start_lat)?;
        }
        if let Some(start_lon) = self.start_lon {
            crate::common::validate::longitude("start_lon", start_lon)?;
        }
        if let Some(end_lat) = self.end_lat {
            crate::common::validate::latitude("end_lat", end_lat)?;
        }
        if let Some(end_lon) = self.end_lon {
            crate::common::validate::longitude("end_lon", end_lon)?;
        }
        Ok(())
    }
}

impl crudcrate::validation::Validatable for TransectUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(name)) = &self.name {
            crudcrate::validation::validators::validate_required("name", name)?;
        }
        if let Some(Some(start_lat)) = self.start_lat {
            crate::common::validate::latitude("start_lat", start_lat)?;
        }
        if let Some(Some(start_lon)) = self.start_lon {
            crate::common::validate::longitude("start_lon", start_lon)?;
        }
        if let Some(Some(end_lat)) = self.end_lat {
            crate::common::validate::latitude("end_lat", end_lat)?;
        }
        if let Some(Some(end_lon)) = self.end_lon {
            crate::common::validate::longitude("end_lon", end_lon)?;
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(Transect, "transects");
crate::ledger_hooks!(Transect, "transects");
