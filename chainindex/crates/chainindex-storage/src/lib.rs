//! chainindex-storage — pluggable storage backends for ChainIndex.
//!
//! Backends:
//! - [`memory`] — in-memory (dev/testing, no persistence)
//! - [`sqlite`] — SQLite via `sqlx` (embedded, single-file persistence)
//! - `postgres` — PostgreSQL via `sqlx` (Phase 3)

pub mod memory;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use memory::InMemoryStorage;
