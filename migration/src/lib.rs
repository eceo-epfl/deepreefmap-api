pub use sea_orm_migration::prelude::*;

mod m20260817_000001_init;
mod m20260921_000001_performance;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260817_000001_init::Migration), Box::new(m20260921_000001_performance::Migration)]
    }
}
