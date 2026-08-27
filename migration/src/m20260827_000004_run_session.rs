use sea_orm_migration::prelude::*;

/// Which session a run was processed in.
///
/// A session is one workstation's queue and has no table here: the registry holds
/// survey facts, not a laptop's cart. What is a fact is that these runs went through
/// the pipeline together, which is what groups them in the console. So the run
/// carries the session's id as a correlation key and nothing references it -- there
/// is deliberately no foreign key, because the row it names lives on the device.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
                ALTER TABLE run_record ADD COLUMN IF NOT EXISTS batch_id UUID;
                CREATE INDEX IF NOT EXISTS run_record_batch_id_idx
                    ON run_record (batch_id) WHERE batch_id IS NOT NULL;
                ",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
                DROP INDEX IF EXISTS run_record_batch_id_idx;
                ALTER TABLE run_record DROP COLUMN IF EXISTS batch_id;
                ",
            )
            .await?;
        Ok(())
    }
}
