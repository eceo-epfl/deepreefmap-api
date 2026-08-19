//! Bidirectional metadata sync.
//!
//! `push` applies a client's document, `pull` hands back what the client has not seen.
//! `schema` describes the replicated tables, published as a contract artefact.

pub mod pull;
pub mod push;
pub mod schema;

use sea_orm::{ConnectionTrait, Statement};

use crate::error::AppResult;

/// The sequence's current position.
///
/// Reads the sequence relation, not `currval`, which is session-scoped. `is_called`
/// separates a sequence that has issued `last_value` from one that has issued nothing.
pub async fn current_cursor<C: ConnectionTrait>(db: &C) -> AppResult<i64> {
    let row = db
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT CASE WHEN is_called THEN last_value ELSE 0 END AS cursor FROM sync_seq"
                .to_string(),
        ))
        .await?;

    Ok(match row {
        Some(row) => row.try_get("", "cursor")?,
        None => 0,
    })
}
