use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A field expedition, named after the archive folders (`2025_10_eritrea`). Visits
/// many sites, so passes point at it rather than the reverse.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "campaign")]
#[crudcrate(
    api_struct = "Campaign",
    name_singular = "campaign",
    name_plural = "campaigns",
    generate_router,
    read::one::body = get_live_one,
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
    pub begin_date: Option<chrono::NaiveDate>,
    #[crudcrate(filterable, sortable)]
    pub end_date: Option<chrono::NaiveDate>,
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
pub enum Relation {
    #[sea_orm(has_many = "crate::routes::private::passes::model::Entity")]
    TransectPass,
}

impl Related<crate::routes::private::passes::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TransectPass.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(Campaign);
