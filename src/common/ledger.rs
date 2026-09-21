//! Recording what the console writes through CRUD into the change ledger.
//!
//! The generated handlers carry no request context, so the caller's subject travels in
//! a task-local the entities router sets.

use axum::{extract::Request, middleware::Next, response::Response};
use crudcrate::ApiError;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, Value};

use crate::common::auth::AuthContext;
use crate::error::AppError;
use crate::routes::private::sync::ledger::record_console;
use crate::routes::private::sync::schema::{TableSpec, table_for_section};

tokio::task_local! {
    /// The Keycloak subject behind the request, for the CRUD hooks.
    pub static CONSOLE_SUBJECT: Option<String>;
}

/// Run the request inside a subject scope, so the CRUD hooks can attribute their entries.
pub async fn scope_subject(request: Request, next: Next) -> Response {
    let subject = request
        .extensions()
        .get::<AuthContext>()
        .and_then(|auth| auth.human_subject().map(str::to_string));
    CONSOLE_SUBJECT.scope(subject, next.run(request)).await
}

/// The subject of the request in flight, if a person.
#[must_use]
pub fn subject() -> Option<String> {
    CONSOLE_SUBJECT.try_with(Clone::clone).ok().flatten()
}

fn spec(section: &str) -> Result<&'static TableSpec, ApiError> {
    table_for_section(section)
        .ok_or_else(|| ApiError::internal(format!("{section} is not a replicated section"), None))
}

fn api_error(error: AppError) -> ApiError {
    match error {
        AppError::Database(e) => ApiError::database(e),
        other => ApiError::internal(other.to_string(), None),
    }
}

/// Stamp a row validated, if not already, returning whether this call did it.
pub async fn validate_row<C: ConnectionTrait>(
    db: &C,
    spec: &TableSpec,
    id: uuid::Uuid,
    subject: Option<&str>,
) -> Result<bool, ApiError> {
    if !spec.curated {
        return Ok(false);
    }
    // Only the descriptor-sourced table name is interpolated.
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "UPDATE {} SET validated_at = NOW(), validated_by = $1 \
                 WHERE id = $2 AND validated_at IS NULL",
                spec.table
            ),
            [Value::String(subject.map(str::to_string)), Value::from(id)],
        ))
        .await
        .map_err(ApiError::database)?;
    Ok(result.rows_affected() > 0)
}

/// Record one row the console just wrote.
pub async fn record_written(
    db: &DatabaseConnection,
    section: &str,
    id: uuid::Uuid,
) -> Result<(), ApiError> {
    let spec = spec(section)?;
    record_console(db, spec, id, subject(), false)
        .await
        .map_err(api_error)?;
    Ok(())
}

/// Record rows the console just tombstoned.
pub async fn record_deleted(
    db: &DatabaseConnection,
    section: &str,
    ids: &[uuid::Uuid],
) -> Result<(), ApiError> {
    let spec = spec(section)?;
    let subject = subject();
    for id in ids {
        record_console(db, spec, *id, subject.clone(), false)
            .await
            .map_err(api_error)?;
    }
    Ok(())
}

/// Generate the CRUD post hooks that record a console write for one entity.
///
/// Invoke in the model module beside `soft_delete_hooks!`, and name the four functions
/// in the `crudcrate` attribute.
#[macro_export]
macro_rules! ledger_hooks {
    ($api_struct:ident, $section:literal) => {
        /// # Errors
        ///
        /// Returns the ledger's database error.
        pub async fn ledger_created(
            db: &sea_orm::DatabaseConnection,
            created: &$api_struct,
        ) -> Result<(), crudcrate::ApiError> {
            $crate::common::ledger::record_written(db, $section, created.id).await
        }

        /// # Errors
        ///
        /// Returns the ledger's database error.
        pub async fn ledger_updated(
            db: &sea_orm::DatabaseConnection,
            updated: &$api_struct,
        ) -> Result<(), crudcrate::ApiError> {
            $crate::common::ledger::record_written(db, $section, updated.id).await
        }

        /// # Errors
        ///
        /// Returns the ledger's database error.
        pub async fn ledger_created_many(
            db: &sea_orm::DatabaseConnection,
            created: &[$api_struct],
        ) -> Result<(), crudcrate::ApiError> {
            for row in created {
                $crate::common::ledger::record_written(db, $section, row.id).await?;
            }
            Ok(())
        }

        /// # Errors
        ///
        /// Returns the ledger's database error.
        pub async fn ledger_updated_many(
            db: &sea_orm::DatabaseConnection,
            updated: &[$api_struct],
        ) -> Result<(), crudcrate::ApiError> {
            for row in updated {
                $crate::common::ledger::record_written(db, $section, row.id).await?;
            }
            Ok(())
        }
    };
}
