use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::archive::store::PART_SIZE_BYTES;
use crate::common::AppState;
use crate::common::auth::deny_device_crud;
use crate::routes::CRUD_BODY_LIMIT;

/// Blob upload negotiation and byte transfer. Both principals upload: laptops push
/// footage under their device token, people upload through the console. No extra
/// guard, so any authenticated identity passes; anonymous callers stop at
/// `auth_middleware`. The overview is the exception: a console view, refused to devices.
///
/// # Panics
///
/// Panics when the part size does not fit `usize`, which no supported target hits.
pub fn router(state: &AppState) -> OpenApiRouter {
    let negotiation = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::initiate))
        .routes(utoipa_axum::routes!(super::views::complete))
        .routes(utoipa_axum::routes!(super::views::download))
        .routes(utoipa_axum::routes!(super::views::by_hash))
        .routes(utoipa_axum::routes!(super::views::probe))
        .routes(utoipa_axum::routes!(super::views::runs_probe))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT));

    // The one route that carries bytes rather than JSON, so it alone takes a
    // part-sized body. `1024` of slack keeps an exact-part-size body under the
    // transport ceiling; the handler enforces the real limit from Content-Length.
    let part_limit = usize::try_from(PART_SIZE_BYTES).expect("the part size fits usize") + 1024;
    let parts = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::upload_part))
        .layer(RequestBodyLimitLayer::new(part_limit));

    let console = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::views::overview))
        .routes(utoipa_axum::routes!(super::bundle::bundle))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT));

    negotiation
        .merge(parts)
        .merge(console)
        .with_state(state.clone())
}
