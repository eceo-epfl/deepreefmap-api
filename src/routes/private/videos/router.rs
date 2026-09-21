use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::deny_device_crud;
use crate::routes::CRUD_BODY_LIMIT;

/// The processing history of one clip, joined server side so the console and the
/// desktop application report the same runs.
pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::runs::runs_for_video))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
