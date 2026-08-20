//! Everything behind the authentication gate: CRUD, sync, cover, archive and admin.

pub mod admin;
pub mod archive;
pub mod campaigns;
pub mod class_groups;
pub mod cover;
pub mod devices;
pub mod me;
pub mod pass_groups;
pub mod passes;
pub mod presets;
pub mod runs;
pub mod sites;
pub mod sync;
pub mod transects;
pub mod videos;

use axum::middleware;
use tower_http::limit::RequestBodyLimitLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;
use crate::common::auth::{deny_device_crud, require_admin_delete};
use crate::common::soft_delete::hide_tombstones;
use crate::routes::CRUD_BODY_LIMIT;

/// Every CRUD and sync route, behind one authentication gate.
///
/// Generated CRUD routers carry their own database handle; hand-written handlers bind
/// `AppState` in their component's `router.rs`. Both end up `OpenApiRouter<()>` so they
/// compose.
pub fn protected_router(state: &AppState) -> OpenApiRouter {
    use self::{
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
        // Every syncable read, since a tombstone is a sync signal and not a row to
        // show. The entities above carry `require_scope`, so a nest that loses this
        // layer refuses reads rather than serving tombstones.
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

    let me = OpenApiRouter::new()
        .routes(utoipa_axum::routes!(me::get_me))
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
        .merge(sync::router::router(state))
        .merge(devices::router::router(state))
        .merge(cover::router::router(state))
        .merge(videos::router::router(state))
        .merge(archive::router::router(state))
        .merge(me)
        .merge(admin_views)
}
