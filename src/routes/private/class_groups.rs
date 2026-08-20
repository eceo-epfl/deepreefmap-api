use axum::Json;

/// The benthic class groups and the colour each is drawn in.
///
/// Served so the console colours a cover figure the same way the desktop viewer does.
#[utoipa::path(
    get,
    path = "/config/class-groups",
    responses((status = 200, description = "Class groups by level", body = [crate::contract::classes::ClassGroup])),
    tag = "config"
)]
pub async fn get_class_groups() -> Json<&'static [crate::contract::classes::ClassGroup]> {
    Json(crate::contract::classes::CLASS_GROUPS)
}
