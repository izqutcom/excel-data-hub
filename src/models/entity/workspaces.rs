use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "workspaces")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub owner_id: i32,
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub created_at: DateTime<Utc>,
    #[sea_orm(column_type = "TimestampWithTimeZone")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::OwnerId",
        to = "super::users::Column::Id"
    )]
    Owner,
    #[sea_orm(has_many = "super::files::Entity")]
    Files,
    #[sea_orm(has_many = "super::excel_data::Entity")]
    ExcelData,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl Related<super::files::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Files.def()
    }
}

impl Related<super::excel_data::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ExcelData.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
