use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement, TransactionTrait};

use crate::common::AppState;
use crate::error::{AppError, AppResult};

/// Hold a transaction-scoped upload lock across API replicas.
pub async fn upload_guard(
    state: &AppState,
    hash: &str,
    shared: bool,
) -> AppResult<DatabaseTransaction> {
    let transaction = state.db.begin().await?;
    let function = if shared {
        "pg_try_advisory_xact_lock_shared"
    } else {
        "pg_try_advisory_xact_lock"
    };
    let query = Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!("SELECT {function}(hashtextextended($1, 0)) AS acquired"),
        [format!("archive:{hash}").into()],
    );
    let result = transaction
        .query_one_raw(query)
        .await?
        .ok_or_else(|| AppError::Internal("Upload lock returned no result".to_string()))?;
    if !result.try_get::<bool>("", "acquired")? {
        return Err(AppError::Conflict("This upload is busy: retry".to_string()));
    }
    Ok(transaction)
}
