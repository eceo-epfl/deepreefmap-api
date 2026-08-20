//! Shared input checks for the generated CRUD models.

use crudcrate::validation::{ValidationError, validators};

/// Reject a latitude outside [-90, 90].
///
/// # Errors
///
/// Returns `ValidationError` when the value is out of range.
pub fn latitude(field: &str, value: f64) -> Result<(), ValidationError> {
    validators::validate_range(field, value, Some(-90.0), Some(90.0))
}

/// Reject a longitude outside [-180, 180].
///
/// # Errors
///
/// Returns `ValidationError` when the value is out of range.
pub fn longitude(field: &str, value: f64) -> Result<(), ValidationError> {
    validators::validate_range(field, value, Some(-180.0), Some(180.0))
}
