use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// Which blob sits at which path inside a run's output directory.
///
/// Videos have no such row: they link to their blob by `content_hash = video_asset.hash`.
/// Read-only router, like [`super::model`]: only the archive endpoints write here.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "run_artifact")]
#[crudcrate(
    api_struct = "RunArtifact",
    name_singular = "run_artifact",
    name_plural = "run_artifacts",
    generate_router
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    #[crudcrate(filterable, exclude(update))]
    pub run_id: Uuid,
    /// Path inside the run directory, validated against traversal on the way in.
    #[crudcrate(filterable, sortable, exclude(update))]
    pub relpath: String,
    pub kind: Option<String>,
    #[crudcrate(sortable)]
    pub size_bytes: Option<i64>,
    #[crudcrate(filterable)]
    pub content_hash: String,
    #[crudcrate(filterable)]
    pub stored_object_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::routes::runs::model::Entity",
        from = "Column::RunId",
        to = "crate::routes::runs::model::Column::Id"
    )]
    Run,
    #[sea_orm(
        belongs_to = "super::model::Entity",
        from = "Column::StoredObjectId",
        to = "super::model::Column::Id"
    )]
    StoredObject,
}

impl Related<crate::routes::runs::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Run.def()
    }
}

impl Related<super::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::StoredObject.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
