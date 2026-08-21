use crudcrate::{CRUDResource, EntityToModels};
use sea_orm::entity::prelude::*;

/// An enrolled desktop installation.
///
/// Server-side only, never synced. `token_prefix` and `token_hash` are excluded from
/// every generated model, so no read path can return them.
#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize, EntityToModels,
)]
#[sea_orm(table_name = "device")]
#[crudcrate(
    api_struct = "Device",
    name_singular = "device",
    name_plural = "devices",
    generate_router
)]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    #[crudcrate(primary_key, filterable, exclude(update), on_create = Uuid::new_v4())]
    pub id: Uuid,
    /// Keycloak subject that minted this device's connect code. Audit only: it grants
    /// nothing and attributes nothing. Nullable: subject erasure scrubs it.
    #[crudcrate(filterable, exclude(create, update))]
    pub enrolled_by: Option<String>,
    /// The installation's durable identity, shown as `uploaded_by` on what it pushes.
    /// Renamed by a human through `/api/devices/{id}/rename`, never by the device.
    #[crudcrate(filterable, sortable, fulltext, exclude(create, update))]
    pub name: String,
    /// Non-secret lookup key for the token's secret half.
    #[crudcrate(exclude(create, update, list, one))]
    pub token_prefix: String,
    #[crudcrate(exclude(create, update, list, one))]
    pub token_hash: String,
    #[crudcrate(filterable, exclude(update))]
    pub platform: Option<String>,
    #[crudcrate(filterable, exclude(update))]
    pub gui_version: Option<String>,
    #[crudcrate(filterable, exclude(update))]
    pub library_version: Option<String>,
    /// When a heartbeat last reported a different `gui_version` or `library_version`.
    /// Enrolment does not stamp it, so null reads as unchanged since enrolment.
    #[crudcrate(exclude(create, update), sortable)]
    pub versions_changed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Hardware and driver survey the device reports about itself, stored as sent.
    /// Detail view only.
    #[sea_orm(column_type = "JsonBinary", nullable)]
    #[crudcrate(exclude(create, update, list))]
    pub system_profile: Option<serde_json::Value>,
    /// When the device last reported on itself, so a stale profile reads as stale.
    #[crudcrate(exclude(create, update), sortable)]
    pub profile_reported_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Which `preset-schema.json` revision the installation understands, from its
    /// heartbeat.
    #[crudcrate(filterable, exclude(create, update))]
    pub preset_schema_version: Option<i32>,
    /// The server-chosen default preset, set through `/api/devices/{id}/assign-preset`
    /// and delivered in the heartbeat response.
    #[crudcrate(filterable, exclude(create, update))]
    pub assigned_preset_id: Option<Uuid>,
    #[crudcrate(exclude(create, update), sortable)]
    pub assigned_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The preset the device says it runs under, from its heartbeat. Beside the
    /// assignment, this is the acknowledgement.
    #[crudcrate(exclude(create, update))]
    pub active_preset_name: Option<String>,
    #[crudcrate(exclude(create, update))]
    pub active_preset_version: Option<i32>,
    #[crudcrate(exclude(create, update), sortable)]
    pub active_preset_reported_at: Option<chrono::DateTime<chrono::Utc>>,
    #[crudcrate(exclude(create, update), sortable, on_create = chrono::Utc::now())]
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[crudcrate(exclude(create, update), sortable)]
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Set to revoke. Kept, so a revoked device stays visible in the audit trail.
    #[crudcrate(filterable, sortable, exclude(create))]
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::connect_code::Entity")]
    ConnectCode,
}

impl Related<super::connect_code::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ConnectCode.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
