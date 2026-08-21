# deepreefmap-api

**A metadata registry for reef surveys processed on the desktop.**

Laptops run the DeepReefMap pipeline and hold the frames, clouds and orthomosaics. This
server holds the catalogue: sites, transects, passes, runs and benthic cover. Metadata
syncs both ways, so a transect made in the browser reaches every laptop, and a run
finished in the field reaches everyone else.

Footage never passes through the API. Clients send it straight to S3-compatible storage.

## Quick Start

Bring the stack up from the UI repository, where the browser and the API meet:

```bash
cd ../deepreefmap-ui
docker compose up -d
```

That runs Postgres, Keycloak with a seeded realm, MinIO, this API with hot reload, and
the web interface behind one traefik.

| What | Where |
|---|---|
| Web interface | <http://localhost:88> |
| API reference | <http://localhost:88/docs> |
| Keycloak | <http://localhost:8280> (`admin` / `admin`) |

To run the API alone, copy `.env.example` to `.env`, point `DATABASE_URL` at your own
Postgres, and `cargo run`. Migrations apply at boot under an advisory lock, so several
replicas can start together.

## Authentication

The browser presents a Keycloak JWT. The desktop application presents a device token.
Both resolve to the same identity, so `created_by` means the same thing either way.

The desktop application ships with no server address in it. Enrol one in a single paste:

1. Mint a code: `POST /api/devices/connect-codes`.
2. Copy the `drm1.<base64url>` string the interface shows. It wraps the server URL and
   a single-use secret.
3. Paste it into the application. It redeems the code at `POST /api/enrol` and keeps
   the device token.

Codes are single use and expire in `CONNECT_CODE_TTL_SECONDS`. Device tokens are
argon2id-hashed at rest, and revoked from the interface.

## Sync

```bash
GET  /api/sync/pull?since=<n>   # rows changed since a cursor, tombstones included
POST /api/sync/push             # apply a document of rows
```

A client tracks one integer, `cursor`, and both endpoints return the next one. The
replicated tables and their columns are `contract/sync-contract.json`, checked in
rather than served.

Each side declares a range, and the exchange runs at `min(client_max, server_max)`:

```http
Deepreefmap-Contract: 1-1
Deepreefmap-Sections: sites,campaigns,transects,videos,passes,pass_videos,runs,cover_rows
```

Both headers are optional, and a malformed value counts as absent. Send neither and you
get contract 1 with no narrowing. Every response carries the server's own range back.

Conflicts resolve last-write-wins on `updated_at`, whole row. A row the server holds at
an equal or newer stamp is skipped and listed in `skipped`. Deletes are tombstones,
because an absent row is indistinguishable from one a client has not seen yet. A push
amends only rows its own origin authored, and refuses anything else row by row.

## Archive

`/api/archive/*` catalogues footage and run outputs held in S3-compatible storage.
Without the `S3_*` variables it answers 503 and nothing else changes. Initiate, PUT
each part to its presigned URL, then complete:

```bash
POST /api/archive/initiate         # presigned parts, or an object that already exists
POST /api/archive/{id}/complete
GET  /api/archive/by-hash/{content_hash}
```

Keys are content-addressed, `{S3_PREFIX}/videos/imohash/{hex}` and
`{S3_PREFIX}/runs/{run_id}/{relpath}`, so re-uploading a clip is a no-op. Upload state
lives in Postgres, so a 4 GB transfer resumes after a restart. The presigning
credentials carry `PutObject` and `GetObject` only. Nothing here deletes or overwrites.

## Entities

Everything above `pass_group` in this table replicates. The desktop application's batch,
batch-item and notification tables do not: they are one workstation's queue.

| Entity | Notes |
|---|---|
| `site` | Reef location. Transect names are unique within one. |
| `campaign` | Field expedition, ie. `2025_10_eritrea`. Visits many sites. |
| `transect` | Survey line, two end points plus the tape length used for scaling. |
| `video_asset` | Input clip, identified by imohash. Paths stay device-local. |
| `transect_pass` | One swim: a time window over one or more clips. |
| `pass_video` | Which clips a pass spans, in playing order. |
| `run_record` | A reconstruction that already ran, with its provenance. |
| `cover_row` | Benthic cover in long format, one row per class group. |
| `preset` | Named run settings the server defines. Pull only. |
| `pass_group` | Curated grouping for statistics. Server-side only. |
| `device` | Enrolled desktop installation. Server-side only. |
| `stored_object` | An archived blob and its upload state. Server-side only. |

The CRUD routers speak the `Content-Range` list dialect, so `ra-data-simple-rest` works
against them unchanged.

Preset settings are validated against `contract/preset-schema.json`, generated from
`src/contract/preset_schema.rs`. That file mirrors the desktop application's
`survey/preset_schema.py` and `models/cache.py`, and the web console builds its
preset form from the published copy. A lagging mirror degrades to a missing dropdown
entry on the laptop, never a broken run: the desktop drops names it cannot offer.

## Development

```bash
cargo clippy --all-targets -- -D warnings
cargo test --lib                    # no database needed
DATABASE_URL=postgresql://postgres:psql@localhost:5444/deepreefmap_test cargo test
cargo run --bin export-contract -- --check
```

Each test builds its own database, so a local run and the watcher container do not
collide. Start `deepreefmap-test-watcher` in the compose stack to rerun the suite on
every save.

## Configuration

See [`.env.example`](.env.example) for the full set. The ones worth knowing:

| Variable | Why it matters |
|---|---|
| `DATABASE_URL` | Or `DB_USER`/`DB_PASSWORD`/`DB_HOST`/`DB_NAME`. |
| `DEPLOYMENT` | `stage` and `prod` refuse to start without Keycloak. |
| `PUBLIC_BASE_URL` | Embedded in connect codes. Without it, no client can enrol. |
| `KEYCLOAK_BROWSER_URL` | When the browser cannot resolve `KEYCLOAK_URL`, ie. in containers. |
| `S3_*` | The archive. All five or none. |
| `SYNC_BODY_LIMIT_BYTES` | Ceiling on a push body. |

Roles are realm roles. `deepreefmap-member` reads and writes survey metadata and enrols
its own devices, `deepreefmap-admin` also revokes anyone's devices. A login carrying
neither is refused with `no_deepreefmap_role`.

## Caveats

- The enrol rate limiter trusts `X-Forwarded-For`. Your reverse proxy has to overwrite
  that header, or the per-IP bucket is spoofable.
- An unredeemed connect code cannot be revoked. Its TTL is the whole mitigation.
- Conflict resolution is whole-row, not per field. Two people editing different fields
  of one transect within a sync interval will see one edit win entirely.
- Tombstones are never collected, so the row count only grows.
- A content hash is an identity a client asserts, not a proof. imohash samples a file
  rather than reading it, and the integrity of the bytes in flight is S3's own.
- `cover_row.metric_source` records which cloud a fraction was measured on. The pipeline
  can produce either, and the numbers are not comparable across the two.

## License

MIT
