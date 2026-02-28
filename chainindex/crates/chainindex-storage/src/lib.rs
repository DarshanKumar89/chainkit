//! chainindex-storage — pluggable storage backends for ChainIndex.
//!
//! Backends:
//! - [`memory`] — in-memory (dev/testing, no persistence)
//! - [`sqlite`] — SQLite via `sqlx` (embedded, single-file persistence)
//! - [`postgres`] — PostgreSQL via `sqlx` (production, high-throughput)

pub mod memory;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

pub use memory::InMemoryStorage;

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStorage;

#[cfg(feature = "postgres")]
pub use postgres::PostgresStorage;
