//! Deletion as a tombstone, for every syncable table.

use axum::{
    extract::Request,
    http::uri::{PathAndQuery, Uri},
    middleware::Next,
    response::Response,
};

/// Force `deleted_at: null` into a list request's filter.
///
/// The generated list handler derives both the page and its `Content-Range` total from
/// the parsed filter, so rewriting the query is the one place that keeps the two
/// agreeing. Overwrites any `deleted_at` the caller sent: `/api/sync/pull` is the only
/// route that hands out tombstones.
pub async fn hide_tombstones(mut request: Request, next: Next) -> Response {
    if request.method().is_safe()
        && let Some(uri) = live_rows_only(request.uri())
    {
        *request.uri_mut() = uri;
    }
    next.run(request).await
}

fn live_rows_only(uri: &Uri) -> Option<Uri> {
    let mut filter: serde_json::Map<String, serde_json::Value> = uri
        .query()
        .and_then(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "filter")
                .and_then(|(_, value)| serde_json::from_str(&value).ok())
        })
        .unwrap_or_default();
    filter.insert("deleted_at".to_string(), serde_json::Value::Null);

    let mut query = form_urlencoded::Serializer::new(String::new());
    for (key, value) in form_urlencoded::parse(uri.query().unwrap_or("").as_bytes()) {
        if key != "filter" {
            query.append_pair(&key, &value);
        }
    }
    query.append_pair("filter", &serde_json::Value::Object(filter).to_string());

    let mut parts = uri.clone().into_parts();
    parts.path_and_query =
        Some(PathAndQuery::try_from(format!("{}?{}", uri.path(), query.finish())).ok()?);
    Uri::from_parts(parts).ok()
}

/// Generate the delete and read hooks a syncable entity needs.
///
/// Invoke in the model module, beside the `DeriveEntityModel` and the api struct it names.
#[macro_export]
macro_rules! soft_delete_hooks {
    ($api_struct:ident) => {
        /// Fetch one live row, treating a tombstone as absent.
        ///
        /// Tombstones exist for `/api/sync/pull` to hand back, not for the console to
        /// show, so this is 404 rather than a deleted row.
        ///
        /// # Errors
        ///
        /// Returns `ApiError::NotFound` when the row is missing or tombstoned.
        pub async fn get_live_one(
            db: &sea_orm::DatabaseConnection,
            id: uuid::Uuid,
        ) -> Result<$api_struct, crudcrate::ApiError> {
            use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

            Entity::find_by_id(id)
                .filter(Column::DeletedAt.is_null())
                .one(db)
                .await
                .map_err(crudcrate::ApiError::database)?
                .map($api_struct::from)
                .ok_or_else(|| {
                    crudcrate::ApiError::not_found(
                        <$api_struct as crudcrate::CRUDResource>::RESOURCE_NAME_SINGULAR,
                        Some(id.to_string()),
                    )
                })
        }

        /// Tombstone one row, returning its id.
        ///
        /// A client that already holds a row learns of its deletion only by pulling the row
        /// back with `deleted_at` set, so the row itself must stay. Already-tombstoned and
        /// repeat calls succeed unchanged.
        ///
        /// # Errors
        ///
        /// Returns `ApiError::NotFound` when no such row exists.
        pub async fn soft_delete_one(
            db: &sea_orm::DatabaseConnection,
            id: uuid::Uuid,
        ) -> Result<uuid::Uuid, crudcrate::ApiError> {
            let tombstoned = soft_delete_many(db, vec![id]).await?;
            if tombstoned.is_empty()
                && !$crate::common::soft_delete::row_exists::<Entity>(db, Column::Id, id).await?
            {
                return Err(crudcrate::ApiError::not_found(
                    <$api_struct as crudcrate::CRUDResource>::RESOURCE_NAME_SINGULAR,
                    Some(id.to_string()),
                ));
            }
            Ok(id)
        }

        /// Tombstone many rows, returning the ids that existed.
        ///
        /// # Errors
        ///
        /// Returns `ApiError::BadRequest` when the batch exceeds the resource's limit.
        pub async fn soft_delete_many(
            db: &sea_orm::DatabaseConnection,
            ids: Vec<uuid::Uuid>,
        ) -> Result<Vec<uuid::Uuid>, crudcrate::ApiError> {
            use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, sea_query::Expr};

            let limit = <$api_struct as crudcrate::CRUDResource>::batch_limit();
            if ids.len() > limit {
                return Err(crudcrate::ApiError::bad_request(format!(
                    "Batch delete limited to {limit} items. Received {} items.",
                    ids.len()
                )));
            }
            if ids.is_empty() {
                return Ok(vec![]);
            }

            let existing: Vec<uuid::Uuid> = Entity::find()
                .select_only()
                .column(Column::Id)
                .filter(Column::Id.is_in(ids.clone()))
                .into_tuple()
                .all(db)
                .await
                .map_err(crudcrate::ApiError::database)?;
            if existing.is_empty() {
                return Ok(vec![]);
            }

            // `updated_at` moves too: it is the clock sync conflicts resolve on, and the
            // UPDATE trigger stamps a fresh `server_seq` so the tombstone reaches pulls.
            let now = chrono::Utc::now();
            Entity::update_many()
                .col_expr(Column::DeletedAt, Expr::value(now))
                .col_expr(Column::UpdatedAt, Expr::value(now))
                .filter(Column::Id.is_in(existing.clone()))
                .filter(Column::DeletedAt.is_null())
                .exec(db)
                .await
                .map_err(crudcrate::ApiError::database)?;

            let existing: std::collections::HashSet<uuid::Uuid> = existing.into_iter().collect();
            let mut seen = std::collections::HashSet::new();
            Ok(ids
                .into_iter()
                .filter(|id| existing.contains(id) && seen.insert(*id))
                .collect())
        }
    };
}

/// Whether a row is present at all, tombstoned or not.
///
/// # Errors
///
/// Returns `ApiError::Database` when the query fails.
pub async fn row_exists<E>(
    db: &sea_orm::DatabaseConnection,
    id_column: E::Column,
    id: uuid::Uuid,
) -> Result<bool, crudcrate::ApiError>
where
    E: sea_orm::EntityTrait,
{
    use sea_orm::{ColumnTrait, QueryFilter, QuerySelect};

    let found = E::find()
        .select_only()
        .column(id_column)
        .filter(id_column.eq(id))
        .into_tuple::<uuid::Uuid>()
        .one(db)
        .await
        .map_err(crudcrate::ApiError::database)?;
    Ok(found.is_some())
}
