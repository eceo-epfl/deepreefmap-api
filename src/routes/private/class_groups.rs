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
    Json(crate::contract::classes::CLASS_GROUPS.as_slice())
}

/// The segmentation classes, their colours, and the group each rolls into.
///
/// A label id in a run's `ortho.npz` is one of these ids, so a viewer can paint the
/// class ortho in the same colours the cover figure uses.
#[utoipa::path(
    get,
    path = "/config/classes",
    responses((status = 200, description = "Classes by label id", body = [crate::contract::classes::BenthicClass])),
    tag = "config"
)]
pub async fn get_classes() -> Json<&'static [crate::contract::classes::BenthicClass]> {
    Json(crate::contract::classes::CLASSES)
}
