use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One entry of the change ledger: a write to a replicated row and how it was decided.
///
/// Written by `/api/sync/push` and the CRUD hooks, never through this router, which
/// only lists.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "change_log")]
#[crudcrate(
    api_struct = "Change",
    name_singular = "change",
    name_plural = "changes",
    generate_router
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, sortable, exclude(create, update))]
    pub seq: i64,
    /// The section, as `contract/sync-contract.json` names it.
    #[crudcrate(filterable, sortable, exclude(update))]
    pub table_key: String,
    #[crudcrate(filterable, exclude(update))]
    pub row_id: Uuid,
    /// The pushing laptop; null for a console entry.
    #[crudcrate(filterable, exclude(update))]
    pub device_id: Option<Uuid>,
    /// The console user behind a console entry.
    #[crudcrate(filterable, exclude(update))]
    pub author: Option<String>,
    #[crudcrate(exclude(update))]
    pub base_seq: i64,
    #[sea_orm(column_type = "JsonBinary")]
    #[crudcrate(exclude(list, update))]
    pub after_image: serde_json::Value,
    /// The fields this entry changed against its base.
    #[sea_orm(column_type = "JsonBinary")]
    #[crudcrate(exclude(update))]
    pub patch: serde_json::Value,
    #[crudcrate(filterable, sortable, exclude(update))]
    pub status: String,
    #[crudcrate(filterable, exclude(update))]
    pub reason: Option<String>,
    #[crudcrate(filterable, exclude(update))]
    pub validate: bool,
    #[crudcrate(sortable, exclude(update))]
    pub projected_seq: Option<i64>,
    #[crudcrate(sortable, exclude(create, update))]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(sortable, exclude(create, update))]
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(filterable, exclude(create, update))]
    pub decided_by: Option<String>,
    #[crudcrate(exclude(create, update))]
    pub decided_seq: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
