//! The published metadata contract: controlled vocabularies and the artefacts built from them.
//!
//! Vocabularies are declared once here and exported to `contract/`. The migrations spell
//! their `CHECK` terms out literally rather than reading these, and `tests/contract.rs`
//! is what keeps the two in step.

pub mod classes;
pub mod export;
pub mod preset_schema;
pub mod vocab;
