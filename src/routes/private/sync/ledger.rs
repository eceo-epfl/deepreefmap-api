//! The change ledger: every write to a replicated row, and how it was decided.
//!
//! The tables are the projection of the applied entries. A device sends each row with
//! the `base_seq` it last saw; the difference against that base is the entry's patch.
//!
//! 1. Nothing moved since the base: applied.
//! 2. The row moved on other fields: applied, merged over the current row.
//! 3. A field the console changed, a validated, deleted or console-authored row:
//!    proposed. A curator accepts or dismisses it.
//! 4. Another device's row: superseded.
//! 5. A field the same device moved past: superseded, the base is stale.
//! 6. A value the database refuses: rejected.
//!
//! Console entries are always applied: the console writes through CRUD and records
//! the result here.

use sea_orm::{ConnectionTrait, DatabaseTransaction, Statement, TransactionTrait, Value};
use serde_json::Value as Json;
use uuid::Uuid;

use super::rows::{Image, current_image};
use super::schema::{ColumnSpec, TableSpec, to_value};
use crate::error::{AppError, AppResult};

/// How an entry was decided.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Applied,
    Superseded,
    Proposed,
    Rejected,
    Dismissed,
}

impl Status {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Superseded => "superseded",
            Self::Proposed => "proposed",
            Self::Rejected => "rejected",
            Self::Dismissed => "dismissed",
        }
    }
}

/// Who wrote an entry.
#[derive(Debug, Clone)]
pub enum Author {
    Device(Uuid),
    Console(Option<String>),
}

impl Author {
    fn device_id(&self) -> Option<Uuid> {
        match self {
            Self::Device(id) => Some(*id),
            Self::Console(_) => None,
        }
    }

    fn subject(&self) -> Option<&str> {
        match self {
            Self::Device(_) => None,
            Self::Console(sub) => sub.as_deref(),
        }
    }
}

/// The outcome of one row of a push.
#[derive(Debug, Clone)]
pub struct Decision {
    pub status: Status,
    pub reason: Option<&'static str>,
    /// The ledger position of the entry.
    pub seq: i64,
    /// The `server_seq` the row holds after an applied entry, which the device stores
    /// as the row's `base_seq` for its next edit.
    pub projected_seq: Option<i64>,
}

/// One row of a device's document, typed and reduced to what the client may write.
pub struct Incoming {
    pub id: Uuid,
    /// The row's `server_seq` the device last saw. `None` from a client too old to
    /// send one, which reads as "the row as it stands now".
    pub base_seq: Option<i64>,
    pub image: Image,
}

/// Columns no patch names: identity, provenance and the two stamps.
const UNDIFFED: [&str; 4] = ["id", "created_at", "device_id", "updated_at"];

/// Apply one row of a device's push, recording the entry whatever the outcome.
///
/// The projection alone takes a savepoint, so a refused value is a rejected entry and
/// not a failed document.
pub async fn apply_device_row(
    txn: &DatabaseTransaction,
    spec: &TableSpec,
    agreed: u32,
    device_id: Uuid,
    incoming: Incoming,
) -> AppResult<Decision> {
    let writable = spec.writable_at(agreed);
    let current = current_image(txn, spec, incoming.id).await?;

    let Some(current) = current else {
        let mut image = incoming.image.clone();
        image.insert("device_id".to_string(), Json::String(device_id.to_string()));
        let patch = diff(&Image::new(), &image, &writable);
        return project_and_record(
            txn,
            spec,
            &writable,
            &Author::Device(device_id),
            incoming.base_seq.unwrap_or(0),
            image,
            patch,
        )
        .await;
    };

    let current_seq = seq_of(&current);
    let owner = current
        .get("device_id")
        .and_then(Json::as_str)
        .and_then(|s| s.parse::<Uuid>().ok());
    let validated = current.get("validated_at").is_some_and(|v| !v.is_null());
    let deleted = current.get("deleted_at").is_some_and(|v| !v.is_null());

    // The base the device edited against, and where intervening entries count from.
    let (base, base_at) = match incoming.base_seq {
        Some(seq) => base_image(txn, spec, incoming.id, seq)
            .await?
            .unwrap_or_else(|| (current.clone(), current_seq)),
        None => (current.clone(), current_seq),
    };
    let patch = diff(&base, &incoming.image, &writable);

    let author = Author::Device(device_id);
    let base_seq = incoming.base_seq.unwrap_or(current_seq);
    if patch.is_empty() {
        // An unchanged echo, ancestors included. Acknowledged at the current position,
        // so the device's base moves up.
        return unprojected(
            txn,
            spec,
            &author,
            &incoming,
            base_seq,
            &patch,
            Status::Applied,
            None,
            Some(current_seq),
        )
        .await;
    }

    let refusal = if owner != Some(device_id) {
        Some(if owner.is_none() {
            (Status::Proposed, "console_authored")
        } else {
            (Status::Superseded, "another_device")
        })
    } else if validated {
        Some((Status::Proposed, "validated"))
    } else if deleted && !incoming_deleted(&incoming.image) {
        Some((Status::Proposed, "deleted"))
    } else if incoming.base_seq.is_none() && !stamp_newer(&incoming.image, &current) {
        // A client too old to send a base: its stamp is all that tells a stale copy
        // from a fresh edit.
        Some((Status::Superseded, "stale"))
    } else {
        overlap(txn, spec, incoming.id, base_at, &patch).await?
    };

    if let Some((status, reason)) = refusal {
        return unprojected(
            txn,
            spec,
            &author,
            &incoming,
            base_seq,
            &patch,
            status,
            Some(reason),
            None,
        )
        .await;
    }

    // Merged over the row as it stands: fields outside the patch keep what intervening
    // entries gave them.
    let mut image = current;
    for (name, value) in &patch {
        image.insert(name.clone(), value.clone());
    }
    if let Some(stamp) = incoming.image.get("updated_at") {
        image.insert("updated_at".to_string(), stamp.clone());
    }
    project_and_record(txn, spec, &writable, &author, base_seq, image, patch).await
}

/// Record what the console just wrote through CRUD, as an applied entry.
///
/// Reads the row back, so the image carries the trigger stamps.
pub async fn record_console<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: Uuid,
    author: Option<String>,
    validate: bool,
) -> AppResult<Option<i64>> {
    let Some(image) = current_image(db, spec, id).await? else {
        return Ok(None);
    };
    let projected_seq = seq_of(&image);
    let base = last_applied(db, spec, id)
        .await?
        .map(|(image, _)| image)
        .unwrap_or_default();
    let base_seq = seq_of(&base);
    let patch = diff(&base, &image, &spec.columns());
    let seq = record(
        db,
        spec,
        &Author::Console(author),
        id,
        base_seq,
        &image,
        &patch,
        Status::Applied,
        None,
        validate,
        Some(projected_seq),
    )
    .await?;
    Ok(Some(seq))
}

/// Record an entry that writes nothing: an unchanged echo, or a refusal.
#[allow(clippy::too_many_arguments)]
async fn unprojected(
    txn: &DatabaseTransaction,
    spec: &TableSpec,
    author: &Author,
    incoming: &Incoming,
    base_seq: i64,
    patch: &Image,
    status: Status,
    reason: Option<&'static str>,
    projected_seq: Option<i64>,
) -> AppResult<Decision> {
    let seq = record(
        txn,
        spec,
        author,
        incoming.id,
        base_seq,
        &incoming.image,
        patch,
        status,
        reason,
        false,
        projected_seq,
    )
    .await?;
    Ok(Decision {
        status,
        reason,
        seq,
        projected_seq,
    })
}

/// Project an image and record the entry, rejected instead when the database refuses it.
async fn project_and_record(
    txn: &DatabaseTransaction,
    spec: &TableSpec,
    writable: &[ColumnSpec],
    author: &Author,
    base_seq: i64,
    image: Image,
    patch: Image,
) -> AppResult<Decision> {
    let id = row_id(&image)?;
    let savepoint = txn.begin().await?;
    let projected = match project(&savepoint, spec, writable, &image).await {
        Ok(seq) => {
            savepoint.commit().await?;
            Ok(seq)
        }
        Err(e) => {
            savepoint.rollback().await?;
            match constraint_reason(&e) {
                Some(reason) => Err(reason),
                None => return Err(e),
            }
        }
    };

    match projected {
        Ok(projected_seq) => {
            let seq = record(
                txn,
                spec,
                author,
                id,
                base_seq,
                &image,
                &patch,
                Status::Applied,
                None,
                false,
                Some(projected_seq),
            )
            .await?;
            Ok(Decision {
                status: Status::Applied,
                reason: None,
                seq,
                projected_seq: Some(projected_seq),
            })
        }
        Err(reason) => {
            tracing::info!(section = spec.section, %id, reason, "Rejected a row the database would not take");
            let seq = record(
                txn,
                spec,
                author,
                id,
                base_seq,
                &image,
                &patch,
                Status::Rejected,
                Some(reason),
                false,
                None,
            )
            .await?;
            Ok(Decision {
                status: Status::Rejected,
                reason: Some(reason),
                seq,
                projected_seq: None,
            })
        }
    }
}

/// Write an image into its table over `columns`, returning the `server_seq` it took.
///
/// `id` and `created_at` are never updated; columns outside `columns` stay as they are.
pub async fn project<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    columns: &[ColumnSpec],
    image: &Image,
) -> AppResult<i64> {
    let names: Vec<&str> = columns.iter().map(|c| c.name).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${i}")).collect();
    let updates: Vec<String> = names
        .iter()
        .filter(|name| **name != "id" && **name != "created_at")
        .map(|name| format!("{name} = EXCLUDED.{name}"))
        .collect();
    // Only descriptor-sourced names are interpolated.
    let sql = format!(
        "INSERT INTO {table} ({cols}) VALUES ({vals}) \
         ON CONFLICT (id) DO UPDATE SET {updates} RETURNING server_seq",
        table = spec.table,
        cols = names.join(", "),
        vals = placeholders.join(", "),
        updates = updates.join(", "),
    );

    let mut values = Vec::with_capacity(columns.len());
    for column in columns {
        values.push(to_value(column, image.get(column.name))?);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await?
        .ok_or_else(|| {
            AppError::Database(sea_orm::DbErr::Custom(format!(
                "{}: the projection reported nothing",
                spec.section
            )))
        })?;
    Ok(row.try_get("", "server_seq")?)
}

/// Append one entry.
#[allow(clippy::too_many_arguments)]
async fn record<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    author: &Author,
    id: Uuid,
    base_seq: i64,
    image: &Image,
    patch: &Image,
    status: Status,
    reason: Option<&str>,
    validate: bool,
    projected_seq: Option<i64>,
) -> AppResult<i64> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO change_log \
             (table_key, row_id, device_id, author, base_seq, after_image, patch, \
              status, reason, validate, projected_seq) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING seq",
            [
                Value::from(spec.section),
                Value::from(id),
                Value::Uuid(author.device_id()),
                Value::String(author.subject().map(str::to_string)),
                Value::from(base_seq),
                Value::from(Json::Object(image.clone())),
                Value::from(Json::Object(patch.clone())),
                Value::from(status.as_str()),
                Value::String(reason.map(str::to_string)),
                Value::from(validate),
                Value::BigInt(projected_seq),
            ],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Database(sea_orm::DbErr::Custom(
                "the ledger insert reported nothing".to_string(),
            ))
        })?;
    Ok(row.try_get("", "seq")?)
}

/// The image a device edited against: the newest applied entry at or below `base_seq`.
async fn base_image<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: Uuid,
    base_seq: i64,
) -> AppResult<Option<(Image, i64)>> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT after_image, projected_seq FROM change_log \
             WHERE table_key = $1 AND row_id = $2 AND status = 'applied' \
               AND projected_seq IS NOT NULL AND projected_seq <= $3 \
             ORDER BY projected_seq DESC LIMIT 1",
            [
                Value::from(spec.section),
                Value::from(id),
                Value::from(base_seq),
            ],
        ))
        .await?;
    row.as_ref().map(entry_image).transpose()
}

/// The newest applied entry for a row, whatever its position.
async fn last_applied<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: Uuid,
) -> AppResult<Option<(Image, i64)>> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT after_image, projected_seq FROM change_log \
             WHERE table_key = $1 AND row_id = $2 AND status = 'applied' \
               AND projected_seq IS NOT NULL \
             ORDER BY projected_seq DESC LIMIT 1",
            [Value::from(spec.section), Value::from(id)],
        ))
        .await?;
    row.as_ref().map(entry_image).transpose()
}

fn entry_image(row: &sea_orm::QueryResult) -> AppResult<(Image, i64)> {
    let image: Json = row.try_get("", "after_image")?;
    let seq: i64 = row.try_get("", "projected_seq")?;
    let Json::Object(image) = image else {
        return Err(AppError::Internal(
            "a ledger entry's after_image is not an object".to_string(),
        ));
    };
    Ok((image, seq))
}

/// Whether an applied entry since `base_at` touched a field the patch names, and by whom.
async fn overlap<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: Uuid,
    base_at: i64,
    patch: &Image,
) -> AppResult<Option<(Status, &'static str)>> {
    if patch.is_empty() {
        return Ok(None);
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT device_id, patch FROM change_log \
             WHERE table_key = $1 AND row_id = $2 AND status = 'applied' \
               AND projected_seq > $3 \
             ORDER BY projected_seq ASC",
            [
                Value::from(spec.section),
                Value::from(id),
                Value::from(base_at),
            ],
        ))
        .await?;

    let mut verdict: Option<(Status, &'static str)> = None;
    for row in rows {
        let device: Option<Uuid> = row.try_get("", "device_id")?;
        let touched: Json = row.try_get("", "patch")?;
        let Json::Object(touched) = touched else {
            continue;
        };
        if !touched.keys().any(|key| patch.contains_key(key)) {
            continue;
        }
        // The console's word is final; the device's own later write makes this one stale.
        if device.is_none() {
            return Ok(Some((Status::Proposed, "console_edited")));
        }
        verdict = Some((Status::Superseded, "stale"));
    }
    Ok(verdict)
}

/// The fields of `incoming` that differ from `base`, over `columns`. An absent field
/// reads as null.
#[must_use]
pub fn diff(base: &Image, incoming: &Image, columns: &[ColumnSpec]) -> Image {
    let mut patch = Image::new();
    for column in columns {
        if UNDIFFED.contains(&column.name) {
            continue;
        }
        let before = base.get(column.name).unwrap_or(&Json::Null);
        let after = incoming.get(column.name).unwrap_or(&Json::Null);
        if before != after {
            patch.insert(column.name.to_string(), after.clone());
        }
    }
    patch
}

/// Whether the incoming `updated_at` is later than the row's.
fn stamp_newer(incoming: &Image, current: &Image) -> bool {
    let parse = |image: &Image| {
        image
            .get("updated_at")
            .and_then(Json::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    };
    match (parse(incoming), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => true,
    }
}

fn incoming_deleted(image: &Image) -> bool {
    image.get("deleted_at").is_some_and(|v| !v.is_null())
}

fn seq_of(image: &Image) -> i64 {
    image.get("server_seq").and_then(Json::as_i64).unwrap_or(0)
}

fn row_id(image: &Image) -> AppResult<Uuid> {
    image
        .get("id")
        .and_then(Json::as_str)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::Internal("an image without an id".to_string()))
}

/// Why a write failed on a unique, foreign key or check constraint, as a reason a
/// device can act on; `None` for any other error.
fn constraint_reason(error: &AppError) -> Option<&'static str> {
    let AppError::Database(e) = error else {
        return None;
    };
    let text = e.to_string();
    if text.contains("violates foreign key constraint") {
        Some("missing_parent")
    } else if text.contains("violates unique constraint") {
        Some("unique_collision")
    } else if text.contains("violates check constraint") {
        Some("outside_vocabulary")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::private::sync::schema::table_for_section;

    fn image(pairs: &[(&str, Json)]) -> Image {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn test_diff_names_only_the_fields_that_moved() {
        let spec = table_for_section("transects").expect("transects");
        let base = image(&[("name", "T1".into()), ("length_m", 100.0.into())]);
        let incoming = image(&[("name", "T1".into()), ("length_m", 50.0.into())]);
        let patch = diff(&base, &incoming, &spec.writable_at(2));
        assert_eq!(patch.len(), 1);
        assert_eq!(patch["length_m"], 50.0);
    }

    #[test]
    fn test_diff_reads_absent_as_null() {
        let spec = table_for_section("transects").expect("transects");
        let base = image(&[("depth_m", 8.0.into())]);
        let incoming = image(&[]);
        let patch = diff(&base, &incoming, &spec.writable_at(2));
        assert_eq!(patch.get("depth_m"), Some(&Json::Null));
    }

    #[test]
    fn test_diff_never_names_identity_or_provenance() {
        let spec = table_for_section("transects").expect("transects");
        let base = image(&[]);
        let incoming = image(&[
            ("id", "x".into()),
            ("device_id", "y".into()),
            ("created_at", "z".into()),
        ]);
        assert!(diff(&base, &incoming, &spec.writable_at(2)).is_empty());
    }
}
