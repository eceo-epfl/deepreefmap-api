use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// Benthic cover in long format, one row per class group per run.
///
/// Shaped after the desktop application's collated export, so ingest is a mapping and
/// not a translation. These rows make cover across sites and seasons a query.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "cover_row")]
#[crudcrate(
    api_struct = "CoverRow",
    name_singular = "cover_row",
    name_plural = "cover_rows",
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
    pub run_id: Uuid,
    /// Level of the class hierarchy the group sits at.
    #[crudcrate(filterable, sortable)]
    pub level: String,
    #[crudcrate(filterable, sortable, fulltext)]
    pub class_group: String,
    /// `per_pass` for one pass observed, `pooled` for the count-weighted estimate.
    #[crudcrate(filterable)]
    pub estimator: String,
    #[crudcrate(sortable)]
    pub fraction: f64,
    #[crudcrate(sortable)]
    pub point_count: Option<f64>,
    pub denominator: Option<f64>,
    /// Which cloud the fractions were measured on: the choice changes their meaning.
    #[crudcrate(filterable)]
    pub metric_source: Option<String>,
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
        belongs_to = "crate::routes::private::runs::model::Entity",
        from = "Column::RunId",
        to = "crate::routes::private::runs::model::Column::Id"
    )]
    Run,
}

impl Related<crate::routes::private::runs::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Run.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(CoverRow);
