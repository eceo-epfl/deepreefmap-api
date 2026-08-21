//! The blob archive: key layout, the S3 client and the reaper.
//!
//! Clients are untrusted. They never delete and never overwrite, and the imohash a
//! client claims is re-computed from the stored bytes before an object counts as
//! `complete`, so nothing wrong can sit at a content-addressed key.

pub mod fetch_token;
pub mod imohash;
pub mod keys;
pub mod reaper;
pub mod store;
