use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::deny_device_crud;
use crate::routes::CRUD_BODY_LIMIT;

/// Cover figures the console and the desktop application both read, so neither
/// computes its own and reports a different number.
pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::pooled::pooled_cover))
        .routes(utoipa_axum::routes!(super::series::cover_series))
        .routes(utoipa_axum::routes!(
            crate::routes::private::class_groups::get_class_groups
        ))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
