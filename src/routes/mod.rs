pub mod admin;
pub mod archive;
pub mod campaigns;
pub mod config;
pub mod cover;
pub mod devices;
pub mod pass_groups;
pub mod passes;
pub mod presets;
pub mod runs;
pub mod sites;
pub mod sync;
pub mod transects;
pub mod videos;

use axum::{
    Router,
    extract::connect_info::IntoMakeServiceWithConnectInfo,
    http::StatusCode,
    middleware,
    routing::{get, post},
};
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, limit::RequestBodyLimitLayer, trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable};

use crate::common::AppState;
use crate::common::auth::{
    AuthContext, Role, auth_middleware, deny_device_crud, require_admin_delete, require_device,
    require_human,
};
use crate::common::contract::{CONTRACT_HEADER, SECTIONS_HEADER, contract_gate, stamp_contract};
use crate::common::soft_delete::hide_tombstones;

/// Body ceiling on ordinary CRUD. Rows here are small.
const CRUD_BODY_LIMIT: usize = 1024 * 1024;
/// Body ceiling on enrolment, which takes a code and two short strings.
const ENROL_BODY_LIMIT: usize = 8 * 1024;

/// Liveness: the process is up.
#[utoipa::path(get, path = "/healthz", responses((status = 200)), tag = "health")]
async fn healthz() -> StatusCode {
    StatusCode::OK
}

#[derive(OpenApi)]
#[openapi(
    paths(
        healthz,
        config::get_keycloak_config,
        config::get_me,
        devices::views::mint_code,
        devices::views::enrol,
        devices::views::revoke,
        devices::views::rename,
        devices::heartbeat::heartbeat,
        sync::push::push,
        sync::pull::pull,
        cover::pooled::pooled_cover,
        cover::series::cover_series,
        videos::runs::runs_for_video,
        config::get_class_groups,
        archive::views::initiate,
        archive::views::complete,
        archive::views::download,
        archive::views::by_hash,
        archive::views::probe,
        archive::views::runs_probe,
        admin::erase_subject,
    ),
    components(schemas(
        config::KeycloakConfigResponse,
        config::MeResponse,
        devices::views::MintConnectCodeRequest,
        devices::views::MintConnectCodeResponse,
        devices::views::EnrolRequest,
        devices::views::EnrolResponse,
        devices::views::RevokeResponse,
        devices::views::RenameDeviceRequest,
        devices::views::RenameDeviceResponse,
        devices::heartbeat::HeartbeatRequest,
        sync::push::PushRequest,
        sync::push::PushResponse,
        sync::push::SectionOutcome,
        sync::pull::PullResponse,
        cover::pooled::PooledCover,
        cover::pooled::GroupCover,
        cover::series::CoverSeries,
        cover::series::CoverSeriesEntry,
        cover::series::SeriesGroupCover,
        crate::contract::classes::ClassGroup,
        archive::views::InitiateRequest,
        archive::views::InitiateResponse,
        archive::views::PartUrl,
        archive::views::CompleteRequest,
        archive::views::CompletedPartBody,
        archive::views::CompleteResponse,
        archive::views::DownloadResponse,
        archive::views::ByHashResponse,
        archive::views::ProbeRequest,
        archive::views::ProbeState,
        archive::views::ProbeResponse,
        archive::views::RunsProbeRequest,
        archive::views::RunArchiveState,
        archive::views::RunsProbeResponse,
        admin::EraseSubjectRequest,
        admin::EraseSubjectResponse,
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

/// Every CRUD and sync route, behind one authentication gate.
///
/// Generated CRUD routers carry their own database handle; hand-written handlers bind
/// `AppState` here. Both end up `OpenApiRouter<()>` so they compose.
fn protected_router(state: &AppState) -> OpenApiRouter {
    use crate::routes::{
        archive::StoredObject, archive::run_artifact::RunArtifact, campaigns::Campaign,
        cover::CoverRow, devices::Device, pass_groups::PassGroup, passes::Pass,
        passes::pass_video::PassVideo, presets::Preset, runs::Run, sites::Site,
        transects::Transect, videos::Video,
    };

    let db = &state.db;

    // Per nest, so no route inside one escapes its guard whatever the ordering.
    let admin_delete = || middleware::from_fn(require_admin_delete);

    // Generated routers ship unauthenticated and unbounded.
    let entities = OpenApiRouter::new()
        .nest("/sites", Site::router(db).layer(admin_delete()))
        .nest("/campaigns", Campaign::router(db).layer(admin_delete()))
        .nest("/transects", Transect::router(db).layer(admin_delete()))
        .nest("/passes", Pass::router(db).layer(admin_delete()))
        .nest("/pass_videos", PassVideo::router(db).layer(admin_delete()))
        // Console-authored curation: survey events over passes, and the presets
        // devices download through sync pull.
        .nest("/pass_groups", PassGroup::router(db).layer(admin_delete()))
        .nest("/presets", Preset::router(db).layer(admin_delete()))
        // Devices report these, so the registry only records them. Footage metadata,
        // provenance and measurements are not things a person types.
        .nest("/videos", Video::read_only_router(db))
        .nest("/runs", Run::read_only_router(db))
        .nest("/cover_rows", CoverRow::read_only_router(db))
        // Every syncable list, since a tombstone is a sync signal and not a row to show.
        // Get-one is filtered per entity by the `read::one::body` hook instead.
        .layer(middleware::from_fn(hide_tombstones))
        // Devices are created by enrolment and retired by revoke, never by CRUD.
        .nest("/devices", Device::read_only_router(db))
        // Archive state, for the console to browse. No delete route exists anywhere:
        // the only delete in the system is the verifier's own hygiene delete.
        // Outside `hide_tombstones`, since neither table has a tombstone column.
        .nest("/stored_objects", StoredObject::read_only_router(db))
        .nest("/run_artifacts", RunArtifact::read_only_router(db))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        // Outermost, so no nest inside can be reached with a device token.
        .layer(middleware::from_fn(deny_device_crud));

    // Ingest is device-only. A push binds provenance from the credential and resolves
    // conflicts on the client's clock, so a member reaching it would hold write and delete
    // over every console-authored row without touching a guarded CRUD route.
    let sync_push = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(sync::push::push))
        .layer(middleware::from_fn(require_device))
        .layer(RequestBodyLimitLayer::new(
            state.config.sync_body_limit_bytes,
        ))
        .with_state(state.clone());

    // A device's own report of what it runs. Identity binds from the credential, so no
    // body field can name a sibling.
    let heartbeat = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(devices::heartbeat::heartbeat))
        .layer(middleware::from_fn(require_device))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    // A cover figure the console and the desktop application both read, so neither
    // computes its own and reports a different number.
    let cover_views = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(cover::pooled::pooled_cover))
        .routes(utoipa_axum::routes!(cover::series::cover_series))
        .routes(utoipa_axum::routes!(config::get_class_groups))
        // The processing history of one clip, joined server side for the same reason.
        .routes(utoipa_axum::routes!(videos::runs::runs_for_video))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    let sync_read = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(sync::pull::pull))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    // Credential operations, so a device cannot invite or retire its siblings.
    let device_admin = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(devices::views::mint_code))
        .routes(utoipa_axum::routes!(devices::views::revoke))
        .routes(utoipa_axum::routes!(devices::views::rename))
        .layer(middleware::from_fn(require_human))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    let me = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(config::get_me))
        .with_state(state.clone());

    // Blob upload negotiation. Both principals upload: laptops push footage under
    // their device token, people upload through the console. No extra guard, so any
    // authenticated identity passes; anonymous callers stop at `auth_middleware`.
    let archive_views = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(archive::views::initiate))
        .routes(utoipa_axum::routes!(archive::views::complete))
        .routes(utoipa_axum::routes!(archive::views::download))
        .routes(utoipa_axum::routes!(archive::views::by_hash))
        .routes(utoipa_axum::routes!(archive::views::probe))
        .routes(utoipa_axum::routes!(archive::views::runs_probe))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    // Admin-only, checked in the handler: the role gate needs the parsed identity,
    // not just its kind.
    let admin_views = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(admin::erase_subject))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone());

    OpenApiRouter::new()
        .merge(entities)
        .merge(sync_push)
        .merge(heartbeat)
        .merge(sync_read)
        .merge(cover_views)
        .merge(device_admin)
        .merge(me)
        .merge(archive_views)
        .merge(admin_views)
}

/// The hand-written paths and the generated CRUD ones, split into a service and its
/// document.
fn split_protected(state: &AppState) -> (Router, utoipa::openapi::OpenApi) {
    let (router, mut openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .merge(protected_router(state))
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

    // Unauthenticated, since the connect code is the credential. Rate limited per
    // IP: each attempt costs an argon2 verification.
    let enrol = {
        let router = Router::new()
            .route("/enrol", post(devices::views::enrol))
            .layer(RequestBodyLimitLayer::new(ENROL_BODY_LIMIT));
        if config.disable_rate_limiting {
            tracing::warn!("Rate limiting DISABLED, including on enrolment");
            router
        } else {
            // Keys on the forwarded client address, since the peer is our own proxy and
            // every enrolling laptop would otherwise share one bucket.
            let limiter = GovernorConfigBuilder::default()
                .period(Duration::from_secs(config.enrol_rate_limit_period_secs))
                .burst_size(config.enrol_rate_limit_burst)
                .key_extractor(SmartIpKeyExtractor)
                .finish()
                .expect("enrolment rate limiter configuration is valid");
            router.layer(GovernorLayer::new(limiter))
        }
    };

    // What a client needs before it can authenticate at all.
    let bootstrap = Router::new()
        .route("/config/keycloak", get(config::get_keycloak_config))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT));

    // Across the whole of `/api`, so enrolment and bootstrap negotiate too. Outside the
    // enrolment rate limiter, so a disjoint client is refused without spending a token from
    // a shared field-wifi bucket. A layer rather than an extractor, so no route can be added
    // without it.
    let api = Router::new()
        .merge(protected)
        .merge(enrol.with_state(state.clone()))
        .merge(bootstrap.with_state(state.clone()))
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
