use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::require_human;
use crate::routes::CRUD_BODY_LIMIT;

pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::accept))
        .routes(utoipa_axum::routes!(super::views::dismiss))
        .routes(utoipa_axum::routes!(super::views::validate))
        .layer(middleware::from_fn(require_human))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
