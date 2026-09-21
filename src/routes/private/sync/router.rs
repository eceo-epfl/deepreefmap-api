use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::require_device;
use crate::routes::CRUD_BODY_LIMIT;

pub fn router(state: &AppState) -> OpenApiRouter {
    // Ingest is device-only. A push binds provenance from the credential and resolves
    // conflicts on the client's clock, so a member reaching it would hold write and delete
    // over every console-authored row without touching a guarded CRUD route.
    let push = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::push::push))
        .layer(middleware::from_fn(require_device))
        .layer(RequestBodyLimitLayer::new(
            state.config.sync_body_limit_bytes,
        ))
        .with_state(state.clone());

    let pull = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::pull::pull))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    push.merge(pull)
}
