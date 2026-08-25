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
    /// Nullable: footage is not always laid against a tape, and such a run is unscaled.
    #[crudcrate(filterable)]
    pub transect_id: Option<Uuid>,
    #[crudcrate(filterable)]
    pub campaign_id: Option<Uuid>,
    pub begin_s: f64,
    pub end_s: f64,
    /// Null means the direction was never recorded, which no code stands for.
    #[crudcrate(filterable)]
    pub direction: Option<String>,
    /// The day the swim happened, from the clip's capture stamp unless corrected.
    #[crudcrate(filterable, sortable)]
    pub surveyed_on: Option<chrono::NaiveDate>,
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
    /// Stamped by the console; from then on a laptop's change is a proposal.
    #[crudcrate(filterable, sortable, exclude(create, update))]
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(filterable, exclude(create, update))]
    pub validated_by: Option<String>,
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

fn validate_window(
    begin_s: Option<f64>,
    end_s: Option<f64>,
) -> Result<(), crudcrate::validation::ValidationError> {
    if let Some(begin) = begin_s
        && begin < 0.0
    {
        return Err(crudcrate::validation::ValidationError::new(
            "begin_s",
            "must not be negative",
        ));
    }
    if let (Some(begin), Some(end)) = (begin_s, end_s)
        && end <= begin
    {
        return Err(crudcrate::validation::ValidationError::new(
            "end_s",
            "must be after begin_s",
        ));
    }
    Ok(())
}

fn validate_code(
    field: &str,
    value: Option<&str>,
    vocabulary: &crate::contract::vocab::Vocabulary,
) -> Result<(), crudcrate::validation::ValidationError> {
    match value {
        Some(code) if !vocabulary.codes().contains(&code) => {
            Err(crudcrate::validation::ValidationError::new(
                field,
                format!("must be one of {}", vocabulary.codes().join(", ")),
            ))
        }
        _ => Ok(()),
    }
}

impl crudcrate::validation::Validatable for PassCreate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        validate_window(Some(self.begin_s), Some(self.end_s))?;
        validate_code(
            "direction",
            self.direction.as_deref(),
            &crate::contract::vocab::PASS_DIRECTION,
        )?;
        validate_code(
            "quality",
            self.quality.as_deref(),
            &crate::contract::vocab::PASS_QUALITY,
        )
    }
}

impl crudcrate::validation::Validatable for PassUpdate {
    fn validate(&self) -> Result<(), crudcrate::validation::ValidationError> {
        validate_window(self.begin_s.flatten(), self.end_s.flatten())?;
        validate_code(
            "direction",
            self.direction.as_ref().and_then(|v| v.as_deref()),
            &crate::contract::vocab::PASS_DIRECTION,
        )?;
        validate_code(
            "quality",
            self.quality.as_ref().and_then(|v| v.as_deref()),
            &crate::contract::vocab::PASS_QUALITY,
        )
    }
}

crate::soft_delete_hooks!(Pass, "passes");
crate::ledger_hooks!(Pass, "passes");
