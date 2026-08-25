use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::deny_device_crud;
use crate::routes::CRUD_BODY_LIMIT;

pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(
            super::transects::transects_for_campaign
        ))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
