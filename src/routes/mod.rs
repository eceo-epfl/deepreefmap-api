pub mod private;
pub mod public;

use axum::{
    Router, extract::connect_info::IntoMakeServiceWithConnectInfo, http::StatusCode, middleware,
    routing::get,
};
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::common::AppState;
use crate::common::auth::{AuthContext, Role, auth_middleware};
use crate::common::contract::{CONTRACT_HEADER, SECTIONS_HEADER, contract_gate, stamp_contract};

/// Body ceiling on ordinary CRUD. Rows here are small.
pub(crate) const CRUD_BODY_LIMIT: usize = 1024 * 1024;

/// Liveness: the process is up.
#[utoipa::path(get, path = "/healthz", responses((status = 200)), tag = "health")]
async fn healthz() -> StatusCode {
    StatusCode::OK
}

#[derive(OpenApi)]
#[openapi(
    paths(
        healthz,
        public::config::get_keycloak_config,
        private::me::get_me,
        private::devices::views::mint_code,
        public::enrol::enrol,
        private::devices::views::revoke,
        private::devices::views::rename,
        private::devices::heartbeat::heartbeat,
        private::sync::push::push,
        private::sync::pull::pull,
        private::cover::pooled::pooled_cover,
        private::cover::series::cover_series,
        private::videos::runs::runs_for_video,
        private::class_groups::get_class_groups,
        private::archive::views::initiate,
        private::archive::views::complete,
        private::archive::views::download,
        private::archive::views::by_hash,
        private::archive::views::probe,
        private::archive::views::runs_probe,
        private::admin::erase_subject,
    ),
    components(schemas(
        public::config::KeycloakConfigResponse,
        private::me::MeResponse,
        private::devices::views::MintConnectCodeRequest,
        private::devices::views::MintConnectCodeResponse,
        public::enrol::EnrolRequest,
        public::enrol::EnrolResponse,
        private::devices::views::RevokeResponse,
        private::devices::views::RenameDeviceRequest,
        private::devices::views::RenameDeviceResponse,
        private::devices::heartbeat::HeartbeatRequest,
        private::sync::push::PushRequest,
        private::sync::push::PushResponse,
        private::sync::push::SectionOutcome,
        private::sync::pull::PullResponse,
        private::cover::pooled::PooledCover,
        private::cover::pooled::GroupCover,
        private::cover::series::CoverSeries,
        private::cover::series::CoverSeriesEntry,
        private::cover::series::SeriesGroupCover,
        crate::contract::classes::ClassGroup,
        private::archive::views::InitiateRequest,
        private::archive::views::InitiateResponse,
        private::archive::views::PartUrl,
        private::archive::views::CompleteRequest,
        private::archive::views::CompletedPartBody,
        private::archive::views::CompleteResponse,
        private::archive::views::DownloadResponse,
        private::archive::views::ByHashResponse,
        private::archive::views::ProbeRequest,
        private::archive::views::ProbeState,
        private::archive::views::ProbeResponse,
        private::archive::views::RunsProbeRequest,
        private::archive::views::RunArchiveState,
        private::archive::views::RunsProbeResponse,
        private::admin::EraseSubjectRequest,
        private::admin::EraseSubjectResponse,
    )),
    tags(
        (name = "health", description = "Liveness probe"),
        (name = "config", description = "Runtime bootstrap and caller identity"),
        (name = "sites", description = "Reef locations"),
        (name = "campaigns", description = "Field expeditions"),
        (name = "transects", description = "Survey lines"),
        (name = "videos", description = "Input footage, identified by content hash"),
        (name = "passes", description = "Swims along a transect"),
        (name = "pass_groups", description = "Survey events curated in the console"),
        (name = "presets", description = "Server-defined run settings"),
        (name = "runs", description = "Reconstruction runs and their provenance"),
        (name = "cover", description = "Benthic cover results"),
        (name = "devices", description = "Desktop installation enrolment"),
        (name = "sync", description = "Device ingest and download"),
        (name = "archive", description = "Content-addressed blob storage"),
        (name = "admin", description = "Administrative maintenance"),
    ),
    modifiers(&SecurityAddon),
    // Overwritten at serve time from CARGO_PKG_VERSION; the literal only satisfies
    // the derive macro.
    info(
        title = "DeepReefMap API",
        description = "Reef survey metadata registry",
        version = "0.0.0"
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "keycloak_jwt",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some("Keycloak JWT, used by the web interface"))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "device_token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "Device token issued at enrolment, used by the desktop application",
                    ))
                    .build(),
            ),
        );
    }
}

/// The hand-written paths and the generated CRUD ones, split into a service and its
/// document.
fn split_protected(state: &AppState) -> (Router, utoipa::openapi::OpenApi) {
    let (router, mut openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(private::protected_router(state))
        .split_for_parts();
    openapi.info.version = env!("CARGO_PKG_VERSION").to_string();
    (router, openapi)
}

/// The full `OpenAPI` document. Touches no database, so the contract exporter can call it.
#[must_use]
pub fn openapi_document(state: &AppState) -> utoipa::openapi::OpenApi {
    split_protected(state).1
}

/// The router as it is actually served, carrying peer addresses.
///
/// The enrolment rate limiter keys on the client address and answers 500 without it,
/// so `main.rs` and the tests both go through here rather than calling
/// `into_make_service_with_connect_info` themselves.
#[must_use]
pub fn build_service(state: &AppState) -> IntoMakeServiceWithConnectInfo<Router, SocketAddr> {
    build_router(state).into_make_service_with_connect_info::<SocketAddr>()
}

/// # Panics
///
/// Panics when the rate limiter configuration is invalid, which the constants here
/// cannot produce.
pub fn build_router(state: &AppState) -> Router {
    build(state, None)
}

/// The router with every request resolved to `identity`, skipping token validation.
///
/// The only way to exercise a Keycloak role where no Keycloak instance is configured, so
/// tests reach it and nothing else does.
///
/// # Panics
///
/// Panics when the rate limiter configuration is invalid, which the constants here
/// cannot produce.
#[doc(hidden)]
pub fn build_router_as(state: &AppState, identity: AuthContext) -> Router {
    build(state, Some(identity))
}

fn build(state: &AppState, forced_identity: Option<AuthContext>) -> Router {
    let config = state.config.clone();
    let (protected, openapi) = split_protected(state);

    let protected = if let Some(identity) = forced_identity {
        protected.layer(middleware::from_fn(
            move |mut request: axum::extract::Request, next: middleware::Next| {
                let identity = identity.clone();
                async move {
                    request.extensions_mut().insert(identity);
                    next.run(request).await
                }
            },
        ))
    } else {
        let mut router = protected.layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
        if let Some(instance) = state.keycloak_auth_instance.clone() {
            use axum_keycloak_auth::{PassthroughMode, layer::KeycloakAuthLayer};
            router = router.layer(
                // Pass, not Block: a request without a JWT may carry a device token,
                // and `auth_middleware` behind this is the only thing that decides.
                KeycloakAuthLayer::<Role>::builder()
                    .instance(instance)
                    .passthrough_mode(PassthroughMode::Pass)
                    .persist_raw_claims(false)
                    .expected_audiences(vec![String::from("account")])
                    .build(),
            );
        } else {
            tracing::warn!(
                "Keycloak is not configured: only device tokens can authenticate, \
                 and no new device can be enrolled because minting a connect code \
                 requires an interactive login"
            );
        }
        router
    };

    // Across the whole of `/api`, so enrolment and bootstrap negotiate too. Outside the
    // enrolment rate limiter, so a disjoint client is refused without spending a token from
    // a shared field-wifi bucket. A layer rather than an extractor, so no route can be added
    // without it.
    let api = Router::new()
        .merge(protected)
        .merge(public::router(state))
        .layer(middleware::from_fn(contract_gate));

    let health = Router::new()
        .route("/healthz", get(healthz))
        .with_state(state.clone());

    Router::new()
        .nest("/api", api)
        .merge(health)
        .merge(Scalar::with_url("/docs", openapi))
        .layer(
            ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |_: tower::BoxError| async { StatusCode::REQUEST_TIMEOUT },
                ))
                .timeout(Duration::from_secs(config.request_timeout_seconds)),
        )
        .layer(CompressionLayer::new())
        .layer(cors_layer(&config))
        .layer(TraceLayer::new_for_http())
        // Outermost of the lot, so the server's range reaches the gate's own refusal, the
        // timeout, and a 404 for a path no router claims. A nested router does not own the
        // fallback, so a layer on `/api` alone would miss that last one.
        .layer(middleware::from_fn(stamp_contract))
}

fn cors_layer(config: &crate::config::Config) -> CorsLayer {
    let origins = &config.cors_allowed_origins;
    if origins.iter().any(|o| o == "*") {
        tracing::warn!("CORS: allowing all origins");
        return CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
            // List totals: without it every table paginates blind.
            .expose_headers([axum::http::header::CONTENT_RANGE]);
    }

    let allowed: Vec<axum::http::HeaderValue> =
        origins.iter().filter_map(|o| o.parse().ok()).collect();
    tracing::info!(origins = ?origins, "CORS: restricted origins");
    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        // The negotiation headers included, or a browser client fails preflight on a
        // header it never had to ask permission for.
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
            axum::http::HeaderName::from_static(CONTRACT_HEADER),
            axum::http::HeaderName::from_static(SECTIONS_HEADER),
        ])
        .allow_credentials(true)
        .expose_headers([axum::http::header::CONTENT_RANGE])
}
