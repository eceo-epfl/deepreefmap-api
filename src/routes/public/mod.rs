//! Routes that answer before any credential exists: enrolment and bootstrap.

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

    Router::new().merge(enrol::router(state)).merge(bootstrap)
}
