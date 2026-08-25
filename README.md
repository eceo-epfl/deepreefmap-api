# deepreefmap-api

**A metadata registry for reef surveys processed on the desktop.**

Laptops run the DeepReefMap pipeline and hold the frames, clouds and orthomosaics. This
server holds the catalogue: sites, campaigns, transects, passes, runs and benthic cover.
Metadata syncs both ways through a change ledger: a transect made in the browser
reaches every laptop, a run finished in the field reaches everyone else, and the
console has the last word on any row it has touched.

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
A person writes through the CRUD routes and is named as `author` in the ledger; a
device writes through `/api/sync/push` and is named as `device_id`.

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

Every pushed row becomes an entry in `change_log`, decided against the `base_seq` the
device last saw for it:

| Outcome | When |
|---|---|
| `applied` | nothing moved since the base, or only other fields did (merged) |
| `proposed` | the console edited the same field, validated, deleted or authored the row |
| `superseded` | another device's row, or a field this device has since moved past |
| `rejected` | a unique collision, a missing parent, or a value outside its vocabulary |

Nothing a laptop sends is lost: a proposal keeps its values, and `GET /api/changes`
lists them for a curator to accept (`POST /api/changes/{seq}/accept`) or dismiss.
`POST /api/changes/validate` marks rows checked, after which every laptop's change to
them is a proposal. Deletes are tombstones, because an absent row is indistinguishable
from one a client has not seen yet.

Under contract 2 a device also pulls its own rows of the upload sections back, so it
learns what the console curated, validated or deleted, and `outbox` in the pull carries
the decisions on its proposals. Contract 1 clients keep the older push shape and pull
only the catalogue.

## Archive

`/api/archive/*` catalogues footage and run outputs held in S3-compatible storage.
Without the `S3_*` variables it answers 503 and nothing else changes. Clients never
talk to the store: every byte flows through the API under the caller's credential,
and the store stays on its internal network. Initiate, PUT each missing part, then
complete:

```bash
POST /api/archive/initiate                       # part size and parts already stored
PUT  /api/archive/{id}/parts/{n}                 # one part's raw bytes
POST /api/archive/{id}/complete
GET  /api/archive/{id}/download                  # a signed fetch link on this API
GET  /api/archive/by-hash/{content_hash}
```

Keys are content-addressed, `{S3_PREFIX}/videos/imohash/{hex}` and
`{S3_PREFIX}/runs/{run_id}/{relpath}`, so re-uploading a clip is a no-op. Upload state
lives in Postgres, so a 4 GB transfer resumes after a restart. Downloads redeem a
short-lived HMAC-signed link at `/api/archive/{id}/fetch`, minted only to
authenticated callers and bound to one object, so a plain browser navigation works
without exposing anything. Nothing here deletes or overwrites.

## Entities

Everything above `device` in this table replicates. The desktop application's batch,
batch-item and notification tables do not: they are one workstation's queue.
`change_log` is the ledger behind sync, server-side only. A survey event is derived, the
passes of one transect in one campaign, so there is nothing to curate by hand.

| Entity | Notes |
|---|---|
| `site` | Reef location, unique within its country. Transect names are unique within one. |
| `campaign` | One trip, ie. `2025_10_eritrea`. Visits many sites; a repeat visit is a new campaign. |
| `transect` | Survey line: end points where known, plus the tape length and depth. |
| `video_asset` | Input clip, identified by imohash, with its camera, rig position and review verdict. Paths stay device-local. |
| `transect_pass` | One swim: a time window over one or more clips, on a day, in a campaign. |
| `pass_video` | Which clips a pass spans, in playing order. |
| `run_record` | A reconstruction that already ran, with its provenance and scale. |
| `cover_row` | Benthic cover in long format, one row per class group. |
| `preset` | Named run settings the server defines. Pull only. |
| `device` | Enrolled desktop installation. Server-side only. |
| `stored_object` | An archived blob and its upload state. Server-side only. |

The CRUD routers speak the `Content-Range` list dialect, so `ra-data-simple-rest` works
against them unchanged.

Preset settings are validated against `contract/preset-schema.json`, generated from
`src/contract/preset_schema.rs`. That file mirrors the desktop application's
`survey/preset_schema.py` and `models/cache.py`, and the web console builds its
preset form from the published copy. A lagging mirror degrades to a missing dropdown
entry on the laptop, never a broken run: the desktop drops names it cannot offer.

## Importing the field spreadsheets

The historical catalogue lives in two spreadsheets: one row per clip with comma-joined
lists of passes, and a results sheet with per-transect coordinates. `import-field-csv`
folds them onto the schema, with `import/site_aliases.json` mapping the free-text
places onto sites. Every imported row is console-owned and validated. Dry run by
default; ids derive from the natural keys, so running it twice changes nothing.

```bash
cargo run --bin import-field-csv -- --videos csv_videos_timestamp_export.csv --results results.csv
DATABASE_URL=... cargo run --bin import-field-csv -- --videos ... --results ... --apply
```

A pass whose window runs to the end of a clip has no end time the sheet can give, so it
is noted on the clip rather than invented.

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
