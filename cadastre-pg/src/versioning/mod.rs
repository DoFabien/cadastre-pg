//! Gestion du versioning temporel

pub mod diff;
pub mod temporal;
pub mod upsert;

pub use diff::geometry_hash;
pub use temporal::{mark_all_as_ended, MarkingReport};
pub use upsert::{upsert_entity, EntityUpsert, UpsertReport, UpsertResult};
