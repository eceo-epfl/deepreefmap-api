use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// One swim along a transect: a time-trimmed window over one or more clips.
///
/// `begin_s` and `end_s` are offsets into the clips played back to back. A `GoPro`
/// splits at about 4 GB, so a swim often spans clips and a clip often holds swims.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "transect_pass")]
#[crudcrate(
    api_struct = "Pass",
    name_singular = "pass",
    name_plural = "passes",
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
    /// Nullable: footage is not always laid against a tape, and such a run is unscaled.
    #[crudcrate(filterable)]
    pub transect_id: Option<Uuid>,
    #[crudcrate(filterable)]
    pub campaign_id: Option<Uuid>,
    /// The curator's survey event, assigned in the console. Deliberately outside the
    /// sync contract, so a device re-pushing this pass can never clobber it.
    #[crudcrate(filterable)]
    pub survey_group_id: Option<Uuid>,
    pub begin_s: f64,
    pub end_s: f64,
    /// Null means the direction was never recorded, which no code stands for.
    #[crudcrate(filterable)]
    pub direction: Option<String>,
    #[crudcrate(filterable)]
    pub upside_down: bool,
    /// Empty means unnamed, which clients render as their generated default.
    #[crudcrate(filterable, fulltext)]
    pub label: String,
    #[crudcrate(fulltext)]
    pub notes: String,
    /// Diver's assessment, on the scale the field spreadsheets map onto.
    #[crudcrate(filterable, sortable)]
    pub quality: Option<String>,
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
        belongs_to = "crate::routes::private::transects::model::Entity",
        from = "Column::TransectId",
        to = "crate::routes::private::transects::model::Column::Id"
    )]
    Transect,
    #[sea_orm(
        belongs_to = "crate::routes::private::campaigns::model::Entity",
        from = "Column::CampaignId",
        to = "crate::routes::private::campaigns::model::Column::Id"
    )]
    Campaign,
    #[sea_orm(
        belongs_to = "crate::routes::private::pass_groups::model::Entity",
        from = "Column::SurveyGroupId",
        to = "crate::routes::private::pass_groups::model::Column::Id"
    )]
    PassGroup,
    #[sea_orm(has_many = "super::pass_video::Entity")]
    PassVideo,
    #[sea_orm(has_many = "crate::routes::private::runs::model::Entity")]
    RunRecord,
}

impl Related<crate::routes::private::transects::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Transect.def()
    }
}

impl Related<crate::routes::private::campaigns::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Campaign.def()
    }
}

impl Related<crate::routes::private::pass_groups::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PassGroup.def()
    }
}

impl Related<super::pass_video::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PassVideo.def()
    }
}

impl Related<crate::routes::private::runs::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::RunRecord.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

crate::soft_delete_hooks!(Pass);
