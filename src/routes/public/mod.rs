//! Routes that answer before any credential exists: enrolment, bootstrap, and the
//! signed archive fetch links (whose HMAC is the credential).

pub mod archive_bundle;
pub mod archive_fetch;
pub mod config;
pub mod enrol;

use axum::{Router, routing::get};
use tower_http::limit::RequestBodyLimitLayer;

use crate::common::AppState;
use crate::routes::CRUD_BODY_LIMIT;

pub fn router(state: &AppState) -> Router {
    // What a client needs before it can authenticate at all.
    let bootstrap = Router::new()
        .route("/config/keycloak", get(config::get_keycloak_config))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    // Bearer-free by design: the signature in the query is the credential, so a
    // plain browser navigation can save the file.
    let fetch = Router::new()
        .route("/archive/{object_id}/fetch", get(archive_fetch::fetch))
        .route(
            "/archive/runs/{run_id}/outputs.zip",
            get(archive_bundle::bundle),
        )
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    Router::new()
        .merge(enrol::router(state))
        .merge(bootstrap)
        .merge(fetch)
}
