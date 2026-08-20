use sea_orm::entity::prelude::*;

/// A single-use onboarding code.
///
/// No CRUD router: listing these would list live credentials.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "connect_code")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// SHA-256 of the secret half, looked up by exact equality.
    pub code_hash: String,
    /// Nullable: subject erasure scrubs it.
    pub created_by: Option<String>,
    pub note: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub used_by_device_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::model::Entity",
        from = "Column::UsedByDeviceId",
        to = "super::model::Column::Id"
    )]
    Device,
}

impl Related<super::model::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Device.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
