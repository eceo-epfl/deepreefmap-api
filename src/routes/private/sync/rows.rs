//! Reading replicated rows back out as the JSON a document carries.

use sea_orm::{ConnectionTrait, Statement, TryGetable};

use super::schema::{ColumnKind, ColumnSpec, TableSpec};
use crate::error::AppResult;

/// One row's contract columns as a JSON object, `server_seq` alongside.
pub type Image = serde_json::Map<String, serde_json::Value>;

/// Read one column back out as JSON, in the shape a push accepts.
pub fn column_to_json(
    row: &sea_orm::QueryResult,
    spec: &ColumnSpec,
) -> AppResult<serde_json::Value> {
    fn get<T: TryGetable>(row: &sea_orm::QueryResult, name: &str) -> AppResult<Option<T>> {
        Ok(row.try_get::<Option<T>>("", name)?)
    }

    let name = spec.name;
    Ok(match spec.kind {
        ColumnKind::Uuid => get::<uuid::Uuid>(row, name)?.map_or(serde_json::Value::Null, |v| {
            serde_json::Value::String(v.to_string())
        }),
        ColumnKind::Text => get::<String>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::Float => get::<f64>(row, name)?
            .and_then(serde_json::Number::from_f64)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        ColumnKind::Int => get::<i32>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::BigInt => get::<i64>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::Bool => get::<bool>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        // Canonical `Z`, so the wire form is the contract's and not chrono's default.
        // `AutoSi` keeps the sub-second precision a stamp comparison needs.
        ColumnKind::Timestamp => get::<chrono::DateTime<chrono::Utc>>(row, name)?
            .map_or(serde_json::Value::Null, |v| {
                serde_json::Value::String(v.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
            }),
        ColumnKind::Date => get::<chrono::NaiveDate>(row, name)?
            .map_or(serde_json::Value::Null, |v| {
                serde_json::Value::String(v.to_string())
            }),
        ColumnKind::Json => get::<serde_json::Value>(row, name)?.unwrap_or(serde_json::Value::Null),
    })
}

/// A whole row as JSON over `columns`, `server_seq` included.
pub fn row_to_image(row: &sea_orm::QueryResult, columns: &[ColumnSpec]) -> AppResult<Image> {
    let mut object = Image::with_capacity(columns.len() + 1);
    for column in columns {
        object.insert(column.name.to_string(), column_to_json(row, column)?);
    }
    let seq: i64 = row.try_get("", "server_seq")?;
    object.insert("server_seq".to_string(), seq.into());
    Ok(object)
}

/// The row as the table holds it now, at the newest contract, or `None` when absent.
pub async fn current_image<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: uuid::Uuid,
) -> AppResult<Option<Image>> {
    let columns = spec.columns();
    let names: Vec<&str> = columns
        .iter()
        .map(|c| c.name)
        .chain(std::iter::once("server_seq"))
        .collect();
    // Only descriptor-sourced names are interpolated.
    let sql = format!(
        "SELECT {cols} FROM {table} WHERE id = $1",
        cols = names.join(", "),
        table = spec.table
    );
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            [id.into()],
        ))
        .await?;
    row.map(|row| row_to_image(&row, &columns)).transpose()
}
