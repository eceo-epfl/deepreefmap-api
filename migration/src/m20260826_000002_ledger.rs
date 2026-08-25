use sea_orm_migration::prelude::*;

/// The change ledger, and the validation stamp it projects.
///
/// `change_log` records every write to a replicated row; the tables are the projection
/// of the applied entries. `seq` draws from `sync_seq`, so one cursor orders rows and
/// entries alike. `projected_seq` is the `server_seq` the row took when the entry was
/// applied, which a device sends back as `base_seq`.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Tables a curator validates. `preset` is server-authored and needs no stamp.
const VALIDATED_TABLES: [&str; 8] = [
    "site",
    "campaign",
    "transect",
    "video_asset",
    "transect_pass",
    "pass_video",
    "run_record",
    "cover_row",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS change_log (
                seq            BIGINT PRIMARY KEY DEFAULT nextval('sync_seq'),
                table_key      TEXT NOT NULL,
                row_id         UUID NOT NULL,
                -- The pushing laptop, or NULL for a console entry.
                device_id      UUID REFERENCES device(id),
                -- The console user, for a console entry. Subject erasure sets it NULL.
                author         TEXT,
                -- The row's server_seq the author last saw; 0 for a new row.
                base_seq       BIGINT NOT NULL DEFAULT 0,
                after_image    JSONB NOT NULL,
                -- The fields this entry changed against its base.
                patch          JSONB NOT NULL DEFAULT '{}'::jsonb,
                status         TEXT NOT NULL,
                reason         TEXT,
                -- A console entry that stamps the row validated.
                validate       BOOLEAN NOT NULL DEFAULT FALSE,
                -- The server_seq the row took when this entry was applied.
                projected_seq  BIGINT,
                created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                -- Set when a proposal is accepted or dismissed, so a pull finds it.
                decided_at     TIMESTAMPTZ,
                decided_by     TEXT,
                decided_seq    BIGINT,
                CONSTRAINT change_log_status CHECK (status IN
                    ('applied', 'superseded', 'proposed', 'rejected', 'dismissed'))
            );
            CREATE INDEX IF NOT EXISTS change_log_row_idx
                ON change_log (table_key, row_id, seq);
            CREATE INDEX IF NOT EXISTS change_log_proposed_idx
                ON change_log (seq) WHERE status = 'proposed';
            CREATE INDEX IF NOT EXISTS change_log_device_idx
                ON change_log (device_id, seq);
            CREATE INDEX IF NOT EXISTS change_log_decided_idx
                ON change_log (device_id, decided_seq) WHERE decided_seq IS NOT NULL;
            ",
        )
        .await?;

        // The projection of a console entry that carried `validate`.
        for table in VALIDATED_TABLES {
            db.execute_unprepared(&format!(
                r"
                ALTER TABLE {table}
                    ADD COLUMN IF NOT EXISTS validated_at TIMESTAMPTZ,
                    ADD COLUMN IF NOT EXISTS validated_by TEXT;
                "
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        for table in VALIDATED_TABLES {
            db.execute_unprepared(&format!(
                "ALTER TABLE {table} DROP COLUMN IF EXISTS validated_at, \
                 DROP COLUMN IF EXISTS validated_by"
            ))
            .await?;
        }
        db.execute_unprepared("DROP TABLE IF EXISTS change_log")
            .await?;
        Ok(())
    }
}
