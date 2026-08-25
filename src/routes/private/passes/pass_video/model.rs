use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// Which clips a pass spans, in playing order.
///
/// A join table rather than the desktop application's JSON id list, so the ordering is
/// enforceable and a clip's passes are queryable.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "pass_video")]
#[crudcrate(
    api_struct = "PassVideo",
    name_singular = "pass_video",
    name_plural = "pass_videos",
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
    pub pass_id: Uuid,
    #[crudcrate(filterable)]
    pub video_id: Uuid,
    /// Playing order within the pass, zero-based.
    #[crudcrate(sortable)]
    pub ordinal: i32,
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
        belongs_to = "super::super::model::Entity",
        from = "Column::PassId",
        to = "super::super::model::Column::Id"
    )]
    Pass,
    #[sea_orm(
        belongs_to = "crate::routes::private::videos::model::Entity",
        from = "Column::VideoId",
        to = "crate::routes::private::videos::model::Column::Id"
    )]
    Video,
}

impl Related<super::super::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Pass.def()
    }
}

impl Related<crate::routes::private::videos::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Video.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl crudcrate::validation::Validatable for PassVideoCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        crudcrate::validation::validators::validate_range("ordinal", self.ordinal, Some(0), None)
    }
}

impl crudcrate::validation::Validatable for PassVideoUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        if let Some(Some(ordinal)) = self.ordinal {
            crudcrate::validation::validators::validate_range("ordinal", ordinal, Some(0), None)?;
        }
        Ok(())
    }
}

crate::soft_delete_hooks!(PassVideo, "pass_videos");
crate::ledger_hooks!(PassVideo, "pass_videos");
