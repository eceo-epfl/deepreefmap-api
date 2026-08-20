use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::routes::CRUD_BODY_LIMIT;

/// Blob upload negotiation. Both principals upload: laptops push footage under
/// their device token, people upload through the console. No extra guard, so any
/// authenticated identity passes; anonymous callers stop at `auth_middleware`.
pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::initiate))
        .routes(utoipa_axum::routes!(super::views::complete))
        .routes(utoipa_axum::routes!(super::views::download))
        .routes(utoipa_axum::routes!(super::views::by_hash))
        .routes(utoipa_axum::routes!(super::views::probe))
        .routes(utoipa_axum::routes!(super::views::runs_probe))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
