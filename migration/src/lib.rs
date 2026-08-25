pub use sea_orm_migration::prelude::*;

mod m20260817_000001_init;
mod m20260826_000002_ledger;
mod m20260826_000003_harmonise;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260817_000001_init::Migration),
            Box::new(m20260826_000002_ledger::Migration),
            Box::new(m20260826_000003_harmonise::Migration),
        ]
    }
}
