use utoipa_axum::router::OpenApiRouter;

use crate::common::AppState;

/// The one camera route a device reaches: publishing a calibration it made.
///
/// The CRUD routers for both entities are console-only and are nested with the rest
/// of the entities, behind `deny_device_crud`. This sits outside that guard because a
/// laptop is exactly who calls it.
pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(super::upload::upload))
        .with_state(state.clone())
}
