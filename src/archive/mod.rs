//! The blob archive: key layout, the S3 client and the reaper.
//!
//! Clients are untrusted. They never delete and never overwrite. The hash a client
//! claims is an identity it asserts, not a proof: imohash samples the file rather
//! than reading it, so the archive dedups on it and trusts S3 for the integrity of
//! the bytes in flight.

pub mod keys;
pub mod reaper;
pub mod store;
