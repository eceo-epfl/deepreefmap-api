use sea_orm_migration::prelude::*;

/// Clean-slate schema for the metadata registry.
///
/// Mirrors the desktop application's survey catalogue
/// (`deepreefmap_gui/survey/models/`), plus a `site`/`campaign` hierarchy above
/// transects, sync columns, the change ledger, device enrolment, server-defined
/// presets and the blob archive.
///
/// A site name is unique within its country, not the world. A transect may lack
/// coordinates and records the depth at each end. A survey event is the passes of one
/// transect in one campaign; a pass records the day it was swum. A clip records the
/// camera and rig position it came from, whether it was mounted upside down, and a
/// review verdict. A run records the scale it was computed at and the session it was
/// processed in.
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

/// The projection of a console entry that validated the row. Every curated table
/// carries it; `preset` is server-authored and needs no stamp.
const VALIDATION_COLUMNS: &str = r"
    validated_at TIMESTAMPTZ,
    validated_by TEXT
";

/// Tables carrying the sync columns, in foreign-key order.
const SYNCABLE_TABLES: [&str; 11] = [
    "site",
    "campaign",
    "transect",
    "video_asset",
    "transect_pass",
    "pass_video",
    "preset",
    "camera_profile",
    "camera_calibration",
    "run_record",
    "cover_row",
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
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS}
            );
            -- Unique among live rows only: a tombstone must not hold its name.
            CREATE UNIQUE INDEX IF NOT EXISTS site_country_name_lower_idx
                ON site (LOWER(COALESCE(country, '')), LOWER(name)) WHERE deleted_at IS NULL;
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
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS}
            );
            CREATE UNIQUE INDEX IF NOT EXISTS campaign_name_lower_idx
                ON campaign (LOWER(name)) WHERE deleted_at IS NULL;
            "
        ))
        .await?;

        // `length_m` is the tape length used for scaling, not the geodesic distance.
        // End points are nullable: the historical lines mostly have none. So are the
        // depths: a line is often drawn before the dive that measures it. `depth_m`
        // is derived from the two ends by trigger below where both are recorded.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS transect (
                id                 UUID PRIMARY KEY,
                site_id            UUID REFERENCES site(id),
                name               TEXT NOT NULL,
                description        TEXT NOT NULL DEFAULT '',
                start_lat          DOUBLE PRECISION,
                start_lon          DOUBLE PRECISION,
                start_accuracy_m   DOUBLE PRECISION,
                end_lat            DOUBLE PRECISION,
                end_lon            DOUBLE PRECISION,
                end_accuracy_m     DOUBLE PRECISION,
                length_m           DOUBLE PRECISION,
                depth_m            DOUBLE PRECISION,
                start_depth_m      DOUBLE PRECISION,
                end_depth_m        DOUBLE PRECISION,
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS},
                CONSTRAINT transect_lat_range CHECK (
                    start_lat BETWEEN -90 AND 90 AND end_lat BETWEEN -90 AND 90
                ),
                CONSTRAINT transect_lon_range CHECK (
                    start_lon BETWEEN -180 AND 180 AND end_lon BETWEEN -180 AND 180
                ),
                CONSTRAINT transect_length_positive CHECK (length_m IS NULL OR length_m >= 0),
                CONSTRAINT transect_depth_positive CHECK (depth_m IS NULL OR depth_m >= 0),
                CONSTRAINT transect_start_depth_positive CHECK (
                    start_depth_m IS NULL OR start_depth_m >= 0
                ),
                CONSTRAINT transect_end_depth_positive CHECK (
                    end_depth_m IS NULL OR end_depth_m >= 0
                )
            );
            -- Scoped to the site: two teams both naming a line 'T1' is normal.
            CREATE UNIQUE INDEX IF NOT EXISTS transect_site_name_lower_idx
                ON transect (site_id, LOWER(name)) WHERE deleted_at IS NULL;
            CREATE INDEX IF NOT EXISTS transect_site_idx ON transect (site_id);
            "
        ))
        .await?;

        // A line measured at each end has one depth and it is their mean, so the
        // database derives it rather than trusting three numbers to agree. In a
        // trigger because every writer reaches the columns directly: console CRUD,
        // `/api/sync/push` (which merges field by field, so one device may set the
        // start and another the end), and the spreadsheet importer. A row carrying
        // only a single historical reading keeps it: the ends are null, and the
        // trigger leaves `depth_m` alone.
        db.execute_unprepared(
            r"
            CREATE OR REPLACE FUNCTION transect_mean_depth() RETURNS TRIGGER AS $$
            BEGIN
                IF NEW.start_depth_m IS NOT NULL AND NEW.end_depth_m IS NOT NULL THEN
                    NEW.depth_m := (NEW.start_depth_m + NEW.end_depth_m) / 2;
                END IF;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;

            CREATE TRIGGER transect_depth_from_ends
                BEFORE INSERT OR UPDATE ON transect
                FOR EACH ROW EXECUTE FUNCTION transect_mean_depth();
            ",
        )
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
                camera_label     TEXT,
                rig_position     TEXT,
                upside_down      BOOLEAN NOT NULL DEFAULT FALSE,
                review           TEXT NOT NULL DEFAULT 'unreviewed',
                notes            TEXT NOT NULL DEFAULT '',
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS},
                -- Tri-state: 'no' is a camera that recorded none, 'unknown' is unread.
                CONSTRAINT video_gravity_tristate CHECK (gravity IN ('yes', 'no', 'unknown')),
                CONSTRAINT video_gps_tristate CHECK (gps IN ('yes', 'no', 'unknown')),
                CONSTRAINT video_captured_source CHECK (captured_source IS NULL OR
                    captured_source IN ('container', 'mtime')),
                CONSTRAINT video_rig_position CHECK (rig_position IS NULL OR
                    rig_position IN ('left', 'centre', 'right')),
                CONSTRAINT video_review CHECK (review IN ('unreviewed', 'usable', 'excluded'))
            );
            CREATE UNIQUE INDEX IF NOT EXISTS video_asset_hash_idx
                ON video_asset (hash) WHERE hash IS NOT NULL AND deleted_at IS NULL;
            "
        ))
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
                label             TEXT NOT NULL DEFAULT '',
                notes             TEXT NOT NULL DEFAULT '',
                quality           TEXT,
                surveyed_on       DATE,
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS},
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
                {VALIDATION_COLUMNS},
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

        // A sync table like the others: devices pull presets, never push them. A named
        // settings document the server curates. Ahead of runs, which name the preset
        // they ran under.
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

        // A camera profile names a rig: a body, a lens mode, a housing, a resolution.
        // It is what `camera_profile_name` in a preset has always meant, and it holds
        // no measurements of its own: those are its calibrations.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS camera_profile (
                id           UUID PRIMARY KEY,
                name         TEXT NOT NULL,
                description  TEXT NOT NULL DEFAULT '',
                -- Which calibration laptops run under. NULL follows the newest, which
                -- is what a profile does until a curator deploys a particular one.
                current_calibration_id UUID,
                {SYNC_COLUMNS}
            );
            -- The name a preset and a run record carry, so it resolves to one profile.
            CREATE UNIQUE INDEX IF NOT EXISTS camera_profile_name_lower_idx
                ON camera_profile (LOWER(name)) WHERE deleted_at IS NULL;
            "
        ))
        .await?;

        // One measurement of one profile. Versions coexist rather than overwrite, as
        // a preset's do: a housing change or a firmware update invalidates the last
        // calibration without invalidating the runs made under it. `document` is the
        // profile JSON the pipeline reads, stored as the desktop writes it.
        db.execute_unprepared(&format!(
            r"
            CREATE TABLE IF NOT EXISTS camera_calibration (
                id                    UUID PRIMARY KEY,
                camera_profile_id     UUID NOT NULL REFERENCES camera_profile(id),
                version               INTEGER NOT NULL DEFAULT 1,
                document              JSONB NOT NULL,
                image_width           INTEGER,
                image_height          INTEGER,
                reprojection_error_px DOUBLE PRECISION,
                registered_frames     INTEGER,
                source_clip           TEXT NOT NULL DEFAULT '',
                calibrated_at         TIMESTAMPTZ,
                description           TEXT NOT NULL DEFAULT '',
                {SYNC_COLUMNS},
                CONSTRAINT camera_calibration_size_positive CHECK (
                    (image_width IS NULL OR image_width > 0)
                    AND (image_height IS NULL OR image_height > 0)
                ),
                CONSTRAINT camera_calibration_error_positive CHECK (
                    reprojection_error_px IS NULL OR reprojection_error_px >= 0
                )
            );
            CREATE UNIQUE INDEX IF NOT EXISTS camera_calibration_profile_version_idx
                ON camera_calibration (camera_profile_id, version) WHERE deleted_at IS NULL;
            CREATE INDEX IF NOT EXISTS camera_calibration_profile_idx
                ON camera_calibration (camera_profile_id);
            "
        ))
        .await?;

        // What a profile deploys has to hold against every writer: the console, a sync
        // apply, the import. A validator cannot read another table, and a key cannot
        // express liveness, which here is a tombstone rather than a missing row.
        db.execute_unprepared(
            r"
            CREATE OR REPLACE FUNCTION camera_profile_deploys_its_own() RETURNS TRIGGER AS $$
            BEGIN
                IF NEW.current_calibration_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM camera_calibration
                    WHERE id = NEW.current_calibration_id
                      AND camera_profile_id = NEW.id
                      AND deleted_at IS NULL
                ) THEN
                    RAISE EXCEPTION
                        'calibration % is not a live calibration of camera profile %',
                        NEW.current_calibration_id, NEW.id;
                END IF;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;

            CREATE TRIGGER camera_profile_deploys_a_live_calibration
                BEFORE INSERT OR UPDATE ON camera_profile
                FOR EACH ROW EXECUTE FUNCTION camera_profile_deploys_its_own();

            CREATE OR REPLACE FUNCTION camera_calibration_release_pin() RETURNS TRIGGER AS $$
            BEGIN
                IF NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL THEN
                    UPDATE camera_profile SET current_calibration_id = NULL
                    WHERE current_calibration_id = NEW.id;
                END IF;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql;

            -- Tombstoning what a profile deploys must not strand the laptops that
            -- resolve it: the profile falls back to following the newest.
            CREATE TRIGGER camera_calibration_releases_its_pin
                AFTER UPDATE ON camera_calibration
                FOR EACH ROW EXECUTE FUNCTION camera_calibration_release_pin();
            ",
        )
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
                -- The processing configuration the run used, matching the desktop
                -- application's performance-history grain: resolution and fps set
                -- the memory regime, batch size gates VRAM.
                processing_width    INTEGER,
                processing_height   INTEGER,
                fps                 INTEGER,
                preprocess_batch_size INTEGER,
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
                camera_profile      TEXT,
                pixel_size_m        DOUBLE PRECISION,
                scale_type          TEXT,
                transect_length_m   DOUBLE PRECISION,
                crop_width_m        DOUBLE PRECISION,
                preset_id           UUID REFERENCES preset(id),
                camera_calibration_id UUID REFERENCES camera_calibration(id),
                -- The session the run was processed in, as a correlation key. A
                -- session is one workstation's queue and has no table here, so there
                -- is deliberately no foreign key: the row it names lives on the device.
                batch_id            UUID,
                {SYNC_COLUMNS},
                {VALIDATION_COLUMNS},
                CONSTRAINT run_status CHECK (status IN
                    ('pending', 'running', 'succeeded', 'failed', 'cancelled', 'interrupted'))
            );
            CREATE INDEX IF NOT EXISTS run_record_pass_idx ON run_record (pass_id);
            CREATE INDEX IF NOT EXISTS run_record_status_idx ON run_record (status);
            CREATE INDEX IF NOT EXISTS run_record_batch_id_idx
                ON run_record (batch_id) WHERE batch_id IS NOT NULL;
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
                {VALIDATION_COLUMNS},
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
                -- When a heartbeat last reported a different version. Enrolment does
                -- not stamp it, so NULL reads as unchanged since enrolment.
                versions_changed_at  TIMESTAMPTZ,
                system_profile       JSONB,
                profile_reported_at  TIMESTAMPTZ,
                -- Which preset-schema.json revision the installation understands,
                -- from its heartbeat.
                preset_schema_version INTEGER,
                -- The server-chosen default preset, and the device's own report of
                -- which preset it runs under. Assignment rides the heartbeat
                -- response, the report rides the next request, so the console can
                -- tell assigned from acknowledged.
                assigned_preset_id   UUID REFERENCES preset(id),
                assigned_at          TIMESTAMPTZ,
                active_preset_name   TEXT,
                active_preset_version INTEGER,
                active_preset_reported_at TIMESTAMPTZ,
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
                -- Minting names the device: enrolment adopts this as device.name,
                -- so the portal is the one place a name originates.
                device_name        TEXT NOT NULL DEFAULT '',
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

        // The change ledger. Every write to a replicated row is recorded here; the
        // tables are the projection of the applied entries. `seq` draws from
        // `sync_seq`, so one cursor orders rows and entries alike. `projected_seq` is
        // the `server_seq` the row took when the entry was applied, which a device
        // sends back as `base_seq`.
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

        // The desktop application's bundled defaults, seeded so a fresh registry has a
        // preset to assign on day one. After the trigger loop, so the row takes a real
        // `server_seq` and reaches devices on their first pull. Values mirror the
        // desktop's `survey_preset.yaml`, publishable keys only.
        db.execute_unprepared(
            r#"
            INSERT INTO preset (id, name, version, settings, description)
            SELECT gen_random_uuid(), 'Standard reef survey', 1,
                '{
                    "fps": 5,
                    "segmentation_name": "coralscapes-vit-b-dpt",
                    "mapping_name": "loger_star",
                    "camera_profile_name": "gopro_hero_10",
                    "transect_crop_width": 1.0,
                    "enable_tsdf": false,
                    "skip_segmentation": false,
                    "resolution_preset": "Native",
                    "processing_width": null,
                    "processing_height": null,
                    "preprocess_batch_size": 4,
                    "grid_bins": 2000,
                    "require_gravity_telemetry": false,
                    "replacement_radius_factor": 0.0,
                    "replacement_radius_estimation_frames": 30,
                    "replacement_radius_override": 0.0,
                    "loger_window_size": 32,
                    "loger_overlap_size": 3,
                    "refine_intrinsics_from_mapper": false,
                    "scs_target_width": 512,
                    "scs_target_height": 256
                }'::jsonb,
                'The desktop application''s bundled defaults.'
            WHERE NOT EXISTS (
                SELECT 1 FROM preset WHERE LOWER(name) = 'standard reef survey' AND version = 1
            );
            "#,
        )
        .await?;

        // The profile the pipeline packages, and the calibration inside it, so a fresh
        // registry can name in a preset what every install already has. Beside the
        // preset seed and for the same reason. `document` is the file
        // `deepreefmap/resources/camera_profiles/gopro_hero_10.json` byte for byte,
        // which is what a device writes into `camera_profiles_dir()` on pull.
        db.execute_unprepared(
            r#"
            INSERT INTO camera_profile (id, name, description)
            SELECT '00000000-0000-4000-8000-000000000010'::uuid, 'gopro_hero_10',
                'Bundled with the pipeline. Every install resolves this name without syncing.'
            WHERE NOT EXISTS (
                SELECT 1 FROM camera_profile WHERE LOWER(name) = 'gopro_hero_10'
            );

            INSERT INTO camera_calibration (
                id, camera_profile_id, version, document, image_width, image_height,
                reprojection_error_px, registered_frames, source_clip, description
            )
            SELECT '00000000-0000-4000-8000-000000000011'::uuid,
                '00000000-0000-4000-8000-000000000010'::uuid, 1,
                '{
                    "name": "gopro_hero_10",
                    "source": "colmap_radial_v1",
                    "distorted": {
                        "model": "RADIAL",
                        "params": {
                            "fx": 1243.6276334472113,
                            "fy": 1243.6276334472113,
                            "cx": 960.0,
                            "cy": 540.0,
                            "k1": 0.36223110184368823,
                            "k2": 0.2476961799393366
                        }
                    },
                    "rectified_pinhole": {
                        "image_size": [1920, 1080],
                        "K": [
                            [1562.98876953125, 0.0, 959.5],
                            [0.0, 1562.98876953125, 539.5],
                            [0.0, 0.0, 1.0]
                        ]
                    },
                    "diagnostics": {
                        "n_input_frames": 100,
                        "n_registered_images": 100,
                        "mean_reprojection_error_px": 0.783395585447909,
                        "camera_model": "RADIAL",
                        "source_video": "redacted-example-source-video",
                        "sampling_fps": 10,
                        "begin_s": 12.0,
                        "end_s": null,
                        "valid_roi_xywh": [0, 0, 1919, 1079]
                    }
                }'::jsonb,
                1920, 1080, 0.783395585447909, 100, '',
                'The calibration the pipeline packages.'
            WHERE NOT EXISTS (
                SELECT 1 FROM camera_calibration
                WHERE camera_profile_id = '00000000-0000-4000-8000-000000000010'::uuid
                  AND version = 1
            );
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Reverse creation order, so foreign keys drop cleanly.
        for table in [
            "run_artifact",
            "stored_object",
            "camera_calibration",
            "camera_profile",
            "change_log",
            "connect_code",
            "device",
            "cover_row",
            "run_record",
            "preset",
            "pass_video",
            "transect_pass",
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
