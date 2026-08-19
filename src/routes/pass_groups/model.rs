use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// A survey event: passes a curator groups by hand in the console.
///
/// Campaigns describe logistics, not analysis, so one expedition's passes can belong to
/// two events and one event can span expeditions. Console-authored only: the table is
/// not in the sync contract, and devices never see or write it.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "pass_group")]
#[crudcrate(
    api_struct = "PassGroup",
    name_singular = "pass_group",
    name_plural = "pass_groups",
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
    /// Where the event sits on a timeline (`2024 spring`), and what orders a series.
    #[crudcrate(filterable, sortable)]
    pub period_label: Option<String>,
    #[crudcrate(fulltext)]
    pub description: String,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(sortable, exclude(create, update), on_create = chrono::Utc::now(), on_update = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The tombstone. Written only by the delete route, which administrators alone
    /// reach. Accepting it on create or update would let a member delete through a
    /// plain edit.
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "crate::routes::passes::model::Entity")]
    TransectPass,
}

impl Related<crate::routes::passes::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TransectPass.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(PassGroup);
