//! Camera profiles and the calibrations that measure them.
//!
//! A profile names a rig: a body, a lens mode, a housing, a resolution. It is what
//! `camera_profile_name` in a preset has always meant. A calibration is one
//! measurement of that rig, and versions of it coexist rather than overwrite, the way
//! a preset's versions do: a housing change invalidates the last measurement without
//! invalidating the runs made under it.
//!
//! Both are pull-only sync sections, written through the console. A device publishes a
//! calibration it made through `upload.rs`, which is a plain endpoint like the archive's
//! and not the change ledger.

pub mod calibration;
pub mod model;
pub mod router;
pub mod upload;

pub use calibration::CameraCalibration;
pub use model::CameraProfile;
