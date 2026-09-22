use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE TABLE performance_observation (
                    id UUID PRIMARY KEY,
                    device_id UUID NOT NULL REFERENCES device(id),
                    run_id UUID UNIQUE REFERENCES run_record(id) ON DELETE SET NULL,
                    observation JSONB NOT NULL,
                    stage_peaks JSONB NOT NULL DEFAULT '{}'::jsonb,
                    source TEXT NOT NULL DEFAULT 'device',
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                );
                CREATE INDEX performance_observation_device_idx
                    ON performance_observation(device_id);
                INSERT INTO performance_observation
                    (id, device_id, run_id, observation, stage_peaks, source, created_at)
                SELECT r.id, r.device_id, r.id,
                    CASE WHEN r.performance_observation IS NOT NULL THEN r.performance_observation
                    ELSE jsonb_build_object(
                        'version', 0,
                        'settings', jsonb_build_object(
                            'processing_width', r.processing_width,
                            'processing_height', r.processing_height,
                            'fps', r.fps,
                            'preprocess_batch_size', r.preprocess_batch_size,
                            'mapping_backend', r.mapping_backend,
                            'segmentation_model', r.segmentation_model,
                            'preset_name', r.preset_name,
                            'preset_version', r.preset_version,
                            'preset_hash', r.preset_hash),
                        'basis', 'unknown',
                        'status', CASE WHEN r.status = 'succeeded' THEN 'completed' ELSE r.status END,
                        'duration_s', r.run_duration_s,
                        'recorded_at', r.started_at)
                    END,
                    COALESCE(r.stage_peaks, '{}'::jsonb),
                    'run',
                    COALESCE(r.started_at, r.created_at)
                FROM run_record r
                WHERE r.device_id IS NOT NULL
                    AND (r.stage_peaks IS NOT NULL OR r.performance_observation IS NOT NULL)
                ON CONFLICT DO NOTHING;
                "#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE performance_observation")
            .await?;
        Ok(())
    }
}
