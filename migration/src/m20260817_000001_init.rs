use sea_orm_migration::prelude::*;

/// Clean-slate schema for the metadata registry.
///
/// Mirrors the desktop application's survey catalogue
/// (`deepreefmap_gui/survey/models/`), plus a `site`/`campaign` hierarchy above
/// transects, sync columns, device enrolment, curated pass groups, server-defined
/// presets and the blob archive.
///
/// `updated_at` is the client's clock and what conflicts resolve on. `server_seq` is
/// the server's, from one shared sequence, so a pull cursor is a single scalar across
/// all tables. Deletes are tombstones: a hard delete resurrects on the next push from
/// a laptop that never learnt of it.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// Columns shared by every syncable table, inlined so they stay ordinary and indexable.
const SYNC_COLUMNS: &str = r"
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at   TIMESTAMPTZ,
    device_id    UUID,
    server_seq   BIGINT NOT NULL DEFAULT 0
";

/// Tables carrying the sync columns, in foreign-key order.
const SYNCABLE_TABLES: [&str; 9] = [
    "site",
    "campaign",
    "transect",
    "video_asset",
    "transect_pass",
    "pass_video",
    "run_record",
    "cover_row",
    "preset",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // One sequence for all tables, so a cursor is a single scalar.
        db.execute_unprepared("CREATE SEQUENCE IF NOT EXISTS sync_seq AS BIGINT START 1")
            .await?;

        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS site (
                id           UUID PRIMARY KEY,
                name         TEXT NOT NULL,
                country      TEXT,
                region       TEXT,
                description  TEXT NOT NULL DEFAULT '',
                latitude     DOUBLE PRECISION,
                longitude    DOUBLE PRECISION,
                {SYNC_COLUMNS}
            );
            -- Unique among live rows only: a tombstone must not hold its name.
            CREATE UNIQUE INDEX IF NOT EXISTS site_name_lower_idx
                ON site (LOWER(name)) WHERE deleted_at IS NULL;
            "
        ))
        .await?;

        // An expedition visits many sites, so it does not hang off site.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS campaign (
                id           UUID PRIMARY KEY,
                name         TEXT NOT NULL,
                begin_date   DATE,
                end_date     DATE,
                description  TEXT NOT NULL DEFAULT '',
                {SYNC_COLUMNS}
            );
            CREATE UNIQUE INDEX IF NOT EXISTS campaign_name_lower_idx
                ON campaign (LOWER(name)) WHERE deleted_at IS NULL;
            "
        ))
        .await?;

        // `length_m` is the tape length used for scaling, not the geodesic distance.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS transect (
                id                 UUID PRIMARY KEY,
                site_id            UUID REFERENCES site(id),
                name               TEXT NOT NULL,
                description        TEXT NOT NULL DEFAULT '',
                start_lat          DOUBLE PRECISION NOT NULL,
                start_lon          DOUBLE PRECISION NOT NULL,
                start_accuracy_m   DOUBLE PRECISION,
                end_lat            DOUBLE PRECISION NOT NULL,
                end_lon            DOUBLE PRECISION NOT NULL,
                end_accuracy_m     DOUBLE PRECISION,
                length_m           DOUBLE PRECISION,
                depth_m            DOUBLE PRECISION,
                {SYNC_COLUMNS},
                CONSTRAINT transect_lat_range CHECK (
                    start_lat BETWEEN -90 AND 90 AND end_lat BETWEEN -90 AND 90
                ),
                CONSTRAINT transect_lon_range CHECK (
                    start_lon BETWEEN -180 AND 180 AND end_lon BETWEEN -180 AND 180
                ),
                CONSTRAINT transect_length_positive CHECK (length_m IS NULL OR length_m >= 0),
                CONSTRAINT transect_depth_positive CHECK (depth_m IS NULL OR depth_m >= 0)
            );
            -- Scoped to the site: two teams both naming a line 'T1' is normal.
            CREATE UNIQUE INDEX IF NOT EXISTS transect_site_name_lower_idx
                ON transect (site_id, LOWER(name)) WHERE deleted_at IS NULL;
            CREATE INDEX IF NOT EXISTS transect_site_idx ON transect (site_id);
            "
        ))
        .await?;

        // Identity is `hash`, the sampled imohash a device computes at ingest. It is
        // also what the archive keys a clip's blob on. No `path` column: device-local.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS video_asset (
                id               UUID PRIMARY KEY,
                hash             TEXT,
                file_name        TEXT NOT NULL,
                size_bytes       BIGINT,
                duration_s       DOUBLE PRECISION,
                fps              DOUBLE PRECISION,
                width            INTEGER,
                height           INTEGER,
                codec            TEXT,
                captured_at      TIMESTAMPTZ,
                captured_source  TEXT,
                gravity          TEXT NOT NULL DEFAULT 'unknown',
                gps              TEXT NOT NULL DEFAULT 'unknown',
                {SYNC_COLUMNS},
                -- Tri-state: 'no' is a camera that recorded none, 'unknown' is unread.
                CONSTRAINT video_gravity_tristate CHECK (gravity IN ('yes', 'no', 'unknown')),
                CONSTRAINT video_gps_tristate CHECK (gps IN ('yes', 'no', 'unknown')),
                CONSTRAINT video_captured_source CHECK (captured_source IS NULL OR
                    captured_source IN ('container', 'mtime'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS video_asset_hash_idx
                ON video_asset (hash) WHERE hash IS NOT NULL AND deleted_at IS NULL;
            "
        ))
        .await?;

        // Not a sync table: groups are curated in the console and never replicate, so
        // no server_seq, no trigger and no provenance columns.
        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS pass_group (
                id            UUID PRIMARY KEY,
                name          TEXT NOT NULL,
                period_label  TEXT,
                description   TEXT NOT NULL DEFAULT '',
                created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                deleted_at    TIMESTAMPTZ
            );
            -- Unique among live rows only: a tombstone must not hold its name.
            CREATE UNIQUE INDEX IF NOT EXISTS pass_group_name_lower_idx
                ON pass_group (LOWER(name)) WHERE deleted_at IS NULL;
            ",
        )
        .await?;

        // `transect_id` is nullable: footage is not always laid against a tape, and
        // such a pass runs unscaled.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS transect_pass (
                id                UUID PRIMARY KEY,
                transect_id       UUID REFERENCES transect(id),
                campaign_id       UUID REFERENCES campaign(id),
                begin_s           DOUBLE PRECISION NOT NULL,
                end_s             DOUBLE PRECISION NOT NULL,
                direction         TEXT,
                upside_down       BOOLEAN NOT NULL DEFAULT FALSE,
                label             TEXT NOT NULL DEFAULT '',
                notes             TEXT NOT NULL DEFAULT '',
                quality           TEXT,
                -- Curator-owned, so it stays out of the sync contract: a device
                -- re-pushing its pass must never clobber the grouping.
                survey_group_id   UUID REFERENCES pass_group(id),
                {SYNC_COLUMNS},
                -- Null is 'not recorded', which 38% of the field spreadsheet's rows are.
                CONSTRAINT pass_direction CHECK (direction IS NULL OR direction IN
                    ('forward', 'reverse')),
                CONSTRAINT pass_window CHECK (begin_s >= 0 AND end_s > begin_s),
                -- Normalises the field spreadsheets' free text ('meh', 'good/meh', ...).
                CONSTRAINT pass_quality CHECK (quality IS NULL OR quality IN
                    ('excellent', 'very_good', 'good', 'meh', 'bad', 'very_bad'))
            );
            CREATE INDEX IF NOT EXISTS transect_pass_transect_idx ON transect_pass (transect_id);
            CREATE INDEX IF NOT EXISTS transect_pass_campaign_idx ON transect_pass (campaign_id);
            CREATE INDEX IF NOT EXISTS transect_pass_survey_group_idx
                ON transect_pass (survey_group_id);
            "
        ))
        .await?;

        // A GoPro splits a long swim at ~4 GB, so `begin_s`/`end_s` are offsets into
        // these clips played back to back.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS pass_video (
                id        UUID PRIMARY KEY,
                pass_id   UUID NOT NULL REFERENCES transect_pass(id),
                video_id  UUID NOT NULL REFERENCES video_asset(id),
                ordinal   INTEGER NOT NULL,
                {SYNC_COLUMNS},
                CONSTRAINT pass_video_ordinal_positive CHECK (ordinal >= 0)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS pass_video_ordinal_idx
                ON pass_video (pass_id, ordinal) WHERE deleted_at IS NULL;
            CREATE UNIQUE INDEX IF NOT EXISTS pass_video_unique_idx
                ON pass_video (pass_id, video_id) WHERE deleted_at IS NULL;
            CREATE INDEX IF NOT EXISTS pass_video_video_idx ON pass_video (video_id);
            "
        ))
        .await?;

        // A report of a reconstruction that already ran, not a request to run one.
        // The provenance columns name the software and weights behind its numbers.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS run_record (
                id                  UUID PRIMARY KEY,
                pass_id             UUID NOT NULL REFERENCES transect_pass(id),
                status              TEXT NOT NULL DEFAULT 'pending',
                started_at          TIMESTAMPTZ,
                finished_at         TIMESTAMPTZ,
                error               TEXT NOT NULL DEFAULT '',
                -- Relative to the producing device's output root, not a server path.
                run_dir_name        TEXT NOT NULL,
                gui_version         TEXT,
                library_version     TEXT,
                segmentation_model  TEXT,
                mapping_backend     TEXT,
                taxonomy_version    INTEGER,
                taxonomy_hash       TEXT,
                model_revisions     JSONB,
                preset_name         TEXT,
                preset_deviations   JSONB,
                -- Pin which revision of a named preset the deviations were measured
                -- against.
                preset_version      INTEGER,
                preset_hash         TEXT,
                -- From the run manifest: how long the run took overall and per stage,
                -- and each stage's peak resource use, so a slow or crashed field
                -- laptop is diagnosable from the registry.
                run_duration_s      DOUBLE PRECISION,
                stage_durations     JSONB,
                stage_peaks         JSONB,
                {SYNC_COLUMNS},
                CONSTRAINT run_status CHECK (status IN
                    ('pending', 'running', 'succeeded', 'failed', 'cancelled', 'interrupted'))
            );
            CREATE INDEX IF NOT EXISTS run_record_pass_idx ON run_record (pass_id);
            CREATE INDEX IF NOT EXISTS run_record_status_idx ON run_record (status);
            "
        ))
        .await?;

        // Long-format cover, shaped after `survey/analysis.py::LongCoverRow`.
        // `metric_source` records which cloud the fractions were measured on.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS cover_row (
                id             UUID PRIMARY KEY,
                run_id         UUID NOT NULL REFERENCES run_record(id),
                level          TEXT NOT NULL,
                class_group    TEXT NOT NULL,
                estimator      TEXT NOT NULL DEFAULT 'per_pass',
                fraction       DOUBLE PRECISION NOT NULL,
                point_count    DOUBLE PRECISION,
                denominator    DOUBLE PRECISION,
                metric_source  TEXT,
                {SYNC_COLUMNS},
                CONSTRAINT cover_level CHECK (level IN ('fine', 'intermediate', 'coarse')),
                CONSTRAINT cover_estimator CHECK (estimator IN ('per_pass', 'pooled')),
                CONSTRAINT cover_fraction_range CHECK (fraction BETWEEN 0 AND 1),
                CONSTRAINT cover_metric_source CHECK (metric_source IS NULL OR metric_source IN
                    ('unprojected', 'tsdf'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS cover_row_unique_idx
                ON cover_row (run_id, level, class_group, estimator) WHERE deleted_at IS NULL;
            CREATE INDEX IF NOT EXISTS cover_row_run_idx ON cover_row (run_id);
            CREATE INDEX IF NOT EXISTS cover_row_group_idx ON cover_row (class_group);
            "
        ))
        .await?;

        // A sync table like the others: devices pull presets, never push them. A named
        // settings document the server curates.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS preset (
                id           UUID PRIMARY KEY,
                name         TEXT NOT NULL,
                version      INTEGER NOT NULL DEFAULT 1,
                settings     JSONB NOT NULL,
                description  TEXT NOT NULL DEFAULT '',
                {SYNC_COLUMNS}
            );
            -- Versions of one name coexist, so the run provenance can pin a revision.
            CREATE UNIQUE INDEX IF NOT EXISTS preset_name_lower_version_idx
                ON preset (LOWER(name), version) WHERE deleted_at IS NULL;
            "
        ))
        .await?;

        // Desktop clients authenticate as devices, never as Keycloak users: the
        // application is public and ships no realm details.
        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS device (
                id                   UUID PRIMARY KEY,
                -- Audit only: who onboarded this installation. Never consulted for
                -- authorisation or attribution. Nullable because subject erasure
                -- sets it to NULL, which NOT NULL would refuse.
                enrolled_by          TEXT,
                name                 TEXT NOT NULL,
                -- Non-secret lookup key: argon2 is salted, so lookup cannot go by hash.
                token_prefix         TEXT NOT NULL UNIQUE,
                token_hash           TEXT NOT NULL,
                platform             TEXT,
                gui_version          TEXT,
                -- Heartbeat self-reports: server-side bookkeeping like the rest of the
                -- table, not sync columns, so no trigger and no contract entry.
                library_version      TEXT,
                system_profile       JSONB,
                profile_reported_at  TIMESTAMPTZ,
                created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_seen_at         TIMESTAMPTZ,
                revoked_at           TIMESTAMPTZ
            );
            CREATE INDEX IF NOT EXISTS device_enrolled_by_idx ON device (enrolled_by);
            ",
        )
        .await?;

        // Single use and short lived: a bearer secret that travels through chat.
        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS connect_code (
                id                 UUID PRIMARY KEY,
                code_hash          TEXT NOT NULL UNIQUE,
                -- Nullable because subject erasure sets it to NULL.
                created_by         TEXT,
                note               TEXT NOT NULL DEFAULT '',
                expires_at         TIMESTAMPTZ NOT NULL,
                used_at            TIMESTAMPTZ,
                used_by_device_id  UUID REFERENCES device(id),
                created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            CREATE INDEX IF NOT EXISTS connect_code_expiry_idx ON connect_code (expires_at)
                WHERE used_at IS NULL;
            ",
        )
        .await?;

        // Neither archive table is a sync section, so no `server_seq` and no trigger.
        // Blob state is server-authored operational data: rows move through their
        // statuses on the server's own clock, and the sync machinery's id-upsert and
        // refused-row semantics cannot express content-address dedup, where two clients
        // initiating the same hash must converge on one row rather than the last write
        // winning.
        //
        // One row per distinct blob, keyed by content hash. `status` walks
        // pending -> complete, or lands on failed when the reaper abandons it. The
        // hash is imohash, the identity a device already holds, so a blob and a clip
        // meet without either side reading a 4 GB file. S3 checks every part against
        // the ETag it answers, which is what guards the bytes in flight.
        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS stored_object (
                id                      UUID PRIMARY KEY,
                content_hash            TEXT NOT NULL UNIQUE,
                size_bytes              BIGINT NOT NULL,
                kind                    TEXT NOT NULL,
                status                  TEXT NOT NULL,
                s3_key                  TEXT NOT NULL UNIQUE,
                s3_upload_id            TEXT,
                part_size_bytes         BIGINT,
                uploaded_by_device_id   UUID REFERENCES device(id),
                uploaded_by             TEXT,
                created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_part_at            TIMESTAMPTZ,
                completed_at            TIMESTAMPTZ,
                failure                 TEXT,
                CONSTRAINT stored_object_kind CHECK (kind IN ('video', 'artifact')),
                CONSTRAINT stored_object_status CHECK (status IN
                    ('pending', 'failed', 'complete'))
            );
            CREATE INDEX IF NOT EXISTS stored_object_status_idx ON stored_object (status);
            ",
        )
        .await?;

        // Which blob sits at which path inside a run's output directory. Videos need no
        // join table: they link to their blob by `content_hash = video_asset.hash`.
        db.execute_unprepared(
            r"
            CREATE TABLE IF NOT EXISTS run_artifact (
                id                UUID PRIMARY KEY,
                run_id            UUID NOT NULL REFERENCES run_record(id),
                relpath           TEXT NOT NULL,
                kind              TEXT,
                size_bytes        BIGINT,
                content_hash      TEXT NOT NULL,
                stored_object_id  UUID REFERENCES stored_object(id),
                created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE (run_id, relpath)
            );
            CREATE INDEX IF NOT EXISTS run_artifact_run_idx ON run_artifact (run_id);
            ",
        )
        .await?;

        // In the database, so no write can leave a row with a stale seq, which would
        // be invisible to pulls.
        db.execute_unprepared(
            r"
            CREATE OR REPLACE FUNCTION set_server_seq() RETURNS TRIGGER AS $$
            BEGIN
                NEW.server_seq := nextval('sync_seq');
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;
            ",
        )
        .await?;

        // Covers only the tables this migration made: a syncable table added later
        // must attach set_server_seq and its index by hand.
        for table in SYNCABLE_TABLES {
            db.execute_unprepared(&format!(
                r"
                CREATE TRIGGER {table}_server_seq
                    BEFORE INSERT OR UPDATE ON {table}
                    FOR EACH ROW EXECUTE FUNCTION set_server_seq();
                CREATE INDEX IF NOT EXISTS {table}_server_seq_idx ON {table} (server_seq);
                "
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Reverse creation order, so foreign keys drop cleanly.
        for table in [
            "run_artifact",
            "stored_object",
            "connect_code",
            "device",
            "preset",
            "cover_row",
            "run_record",
            "pass_video",
            "transect_pass",
            "pass_group",
            "video_asset",
            "transect",
            "campaign",
            "site",
        ] {
            db.execute_unprepared(&format!("DROP TABLE IF EXISTS {table} CASCADE"))
                .await?;
        }

        db.execute_unprepared("DROP FUNCTION IF EXISTS set_server_seq() CASCADE")
            .await?;
        db.execute_unprepared("DROP SEQUENCE IF EXISTS sync_seq")
            .await?;

        Ok(())
    }
}
