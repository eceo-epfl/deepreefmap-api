use sea_orm_migration::prelude::*;

/// The catalogue as the field data needs it.
///
/// A site name is unique within its country, not the world. A transect may lack
/// coordinates. A pass records the day it was swum and no longer a curated group:
/// a survey event is the passes of one transect in one campaign. A clip records the
/// camera and rig position it came from, whether it was mounted upside down, and a
/// review verdict. A run records the scale it was computed at.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r"
            DROP INDEX IF EXISTS site_name_lower_idx;
            CREATE UNIQUE INDEX IF NOT EXISTS site_country_name_lower_idx
                ON site (LOWER(COALESCE(country, '')), LOWER(name)) WHERE deleted_at IS NULL;

            ALTER TABLE transect
                ALTER COLUMN start_lat DROP NOT NULL,
                ALTER COLUMN start_lon DROP NOT NULL,
                ALTER COLUMN end_lat DROP NOT NULL,
                ALTER COLUMN end_lon DROP NOT NULL;

            DROP INDEX IF EXISTS transect_pass_survey_group_idx;
            ALTER TABLE transect_pass
                DROP COLUMN IF EXISTS survey_group_id,
                DROP COLUMN IF EXISTS upside_down,
                ADD COLUMN IF NOT EXISTS surveyed_on DATE;

            ALTER TABLE video_asset
                ADD COLUMN IF NOT EXISTS camera_label TEXT,
                ADD COLUMN IF NOT EXISTS rig_position TEXT,
                ADD COLUMN IF NOT EXISTS upside_down BOOLEAN NOT NULL DEFAULT FALSE,
                ADD COLUMN IF NOT EXISTS review TEXT NOT NULL DEFAULT 'unreviewed',
                ADD COLUMN IF NOT EXISTS notes TEXT NOT NULL DEFAULT '',
                ADD CONSTRAINT video_rig_position CHECK (rig_position IS NULL OR
                    rig_position IN ('left', 'centre', 'right')),
                ADD CONSTRAINT video_review CHECK (review IN
                    ('unreviewed', 'usable', 'excluded'));

            ALTER TABLE run_record
                ADD COLUMN IF NOT EXISTS camera_profile TEXT,
                ADD COLUMN IF NOT EXISTS pixel_size_m DOUBLE PRECISION,
                ADD COLUMN IF NOT EXISTS scale_type TEXT,
                ADD COLUMN IF NOT EXISTS transect_length_m DOUBLE PRECISION,
                ADD COLUMN IF NOT EXISTS crop_width_m DOUBLE PRECISION,
                ADD COLUMN IF NOT EXISTS preset_id UUID REFERENCES preset(id);

            DROP TABLE IF EXISTS pass_group;
            ",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r"
            ALTER TABLE run_record
                DROP COLUMN IF EXISTS camera_profile,
                DROP COLUMN IF EXISTS pixel_size_m,
                DROP COLUMN IF EXISTS scale_type,
                DROP COLUMN IF EXISTS transect_length_m,
                DROP COLUMN IF EXISTS crop_width_m,
                DROP COLUMN IF EXISTS preset_id;
            ALTER TABLE video_asset
                DROP CONSTRAINT IF EXISTS video_rig_position,
                DROP CONSTRAINT IF EXISTS video_review,
                DROP COLUMN IF EXISTS camera_label,
                DROP COLUMN IF EXISTS rig_position,
                DROP COLUMN IF EXISTS upside_down,
                DROP COLUMN IF EXISTS review,
                DROP COLUMN IF EXISTS notes;
            ALTER TABLE transect_pass
                DROP COLUMN IF EXISTS surveyed_on,
                ADD COLUMN IF NOT EXISTS upside_down BOOLEAN NOT NULL DEFAULT FALSE;
            DROP INDEX IF EXISTS site_country_name_lower_idx;
            CREATE UNIQUE INDEX IF NOT EXISTS site_name_lower_idx
                ON site (LOWER(name)) WHERE deleted_at IS NULL;
            ",
        )
        .await?;
        Ok(())
    }
}
