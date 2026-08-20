use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::{require_device, require_human};
use crate::routes::CRUD_BODY_LIMIT;

pub fn router(state: &AppState) -> OpenApiRouter {
    // Credential operations, so a device cannot invite or retire its siblings.
    let device_admin = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::mint_code))
        .routes(utoipa_axum::routes!(super::views::revoke))
        .routes(utoipa_axum::routes!(super::views::rename))
        .layer(middleware::from_fn(require_human))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    // A device's own report of what it runs. Identity binds from the credential, so no
    // body field can name a sibling.
    let heartbeat = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::heartbeat::heartbeat))
        .layer(middleware::from_fn(require_device))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    device_admin.merge(heartbeat)
}
