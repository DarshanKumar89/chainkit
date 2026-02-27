//! SQLite-backed `SchemaRegistry` implementation.
//!
//! Persists schemas as JSON blobs in a local SQLite database.  Suitable for
//! long-running processes (indexers, servers) that need durable schema storage
//! without a full database server.
//!
//! ## Feature flag
//! This module is only compiled when the `sqlite` feature is enabled:
//! ```toml
//! chaincodec-registry = { version = "...", features = ["sqlite"] }
//! ```
//!
//! ## Schema
//! ```sql
//! CREATE TABLE cc_schemas (
//!     name        TEXT    NOT NULL,
//!     version     INTEGER NOT NULL,
//!     fingerprint TEXT    NOT NULL,
//!     chain       TEXT    NOT NULL,   -- comma-separated chain slugs
//!     deprecated  INTEGER NOT NULL DEFAULT 0,
//!     schema_json TEXT    NOT NULL,
//!     PRIMARY KEY (name, version)
//! );
//! CREATE UNIQUE INDEX cc_schemas_fp ON cc_schemas (fingerprint);
//! ```

use chaincodec_core::{
    error::RegistryError,
    event::EventFingerprint,
    schema::{Schema, SchemaRegistry},
};
use rusqlite::{params, Connection};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

/// SQLite-backed schema registry.
///
/// Thread-safe via an internal `Arc<Mutex<Connection>>`.
/// All methods open a single shared connection; WAL mode is enabled for
/// concurrent read performance.
#[derive(Clone)]
pub struct SqliteRegistry {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteRegistry {
    /// Open (or create) a registry database at the given path.
    ///
    /// Runs `CREATE TABLE IF NOT EXISTS` and creates indexes on first open.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let conn = Connection::open(path.as_ref()).map_err(|e| {
            RegistryError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("sqlite open error: {e}"),
            ))
        })?;

        // Enable WAL mode for better read concurrency
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(sqlite_err)?;

        // Create schema table and indexes
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cc_schemas (
                name        TEXT    NOT NULL,
                version     INTEGER NOT NULL,
                fingerprint TEXT    NOT NULL,
                chains      TEXT    NOT NULL,
                deprecated  INTEGER NOT NULL DEFAULT 0,
                schema_json TEXT    NOT NULL,
                PRIMARY KEY (name, version)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS cc_schemas_fp
                ON cc_schemas (fingerprint);
            CREATE INDEX IF NOT EXISTS cc_schemas_chain
                ON cc_schemas (chains);",
        )
        .map_err(sqlite_err)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory registry (useful for tests).
    pub fn in_memory() -> Result<Self, RegistryError> {
        Self::open(":memory:")
    }

    /// Insert a schema into the database.
    ///
    /// Returns `RegistryError::AlreadyExists` if the (name, version) pair is
    /// already present.
    pub fn add(&self, schema: &Schema) -> Result<(), RegistryError> {
        let json = serde_json::to_string(schema).map_err(|e| {
            RegistryError::ParseError(format!("failed to serialize schema: {e}"))
        })?;

        let chains = schema.chains.join(",");
        let conn = self.conn.lock().unwrap();

        // Check for existing (name, version)
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM cc_schemas WHERE name = ?1 AND version = ?2",
                params![&schema.name, schema.version],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if exists {
            return Err(RegistryError::AlreadyExists {
                name: schema.name.clone(),
                version: schema.version,
            });
        }

        conn.execute(
            "INSERT INTO cc_schemas (name, version, fingerprint, chains, deprecated, schema_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &schema.name,
                schema.version,
                schema.fingerprint.as_hex(),
                &chains,
                schema.deprecated as i32,
                &json,
            ],
        )
        .map_err(sqlite_err)?;

        Ok(())
    }

    /// Upsert a schema — insert or replace if (name, version) already exists.
    pub fn upsert(&self, schema: &Schema) -> Result<(), RegistryError> {
        let json = serde_json::to_string(schema).map_err(|e| {
            RegistryError::ParseError(format!("failed to serialize schema: {e}"))
        })?;

        let chains = schema.chains.join(",");
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT OR REPLACE INTO cc_schemas
                (name, version, fingerprint, chains, deprecated, schema_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                &schema.name,
                schema.version,
                schema.fingerprint.as_hex(),
                &chains,
                schema.deprecated as i32,
                &json,
            ],
        )
        .map_err(sqlite_err)?;

        Ok(())
    }

    /// Load all `.csdl` files from a directory recursively and store them.
    ///
    /// Uses `upsert` so existing schemas are updated rather than erroring.
    /// Returns the total number of schema versions loaded.
    pub fn load_directory(&self, dir: impl AsRef<Path>) -> Result<usize, RegistryError> {
        use crate::csdl::CsdlParser;

        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(RegistryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} is not a directory", dir.display()),
            )));
        }

        let mut count = 0;
        for entry in walkdir_csdl(dir)? {
            let content = std::fs::read_to_string(&entry).map_err(RegistryError::Io)?;
            for schema in CsdlParser::parse_all(&content)? {
                self.upsert(&schema)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Load a single CSDL file and store all schemas it contains.
    pub fn load_file(&self, path: impl AsRef<Path>) -> Result<usize, RegistryError> {
        use crate::csdl::CsdlParser;

        let content = std::fs::read_to_string(path.as_ref()).map_err(RegistryError::Io)?;
        let schemas = CsdlParser::parse_all(&content)?;
        if schemas.is_empty() {
            return Err(RegistryError::ParseError("empty CSDL file".into()));
        }
        let count = schemas.len();
        for schema in schemas {
            self.upsert(&schema)?;
        }
        Ok(count)
    }

    /// Returns the total number of schema versions stored.
    pub fn len(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM cc_schemas", [], |row| {
            row.get::<_, usize>(0)
        })
        .unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the names of all schemas (deduplicated).
    pub fn all_names(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT name FROM cc_schemas ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    /// Find the highest non-deprecated version number for a given schema name.
    fn latest_version_of(&self, conn: &Connection, name: &str) -> Option<u32> {
        conn.query_row(
            "SELECT MAX(version) FROM cc_schemas
             WHERE name = ?1 AND deprecated = 0",
            params![name],
            |row| row.get::<_, Option<u32>>(0),
        )
        .ok()
        .flatten()
    }
}

impl SchemaRegistry for SqliteRegistry {
    fn get_by_fingerprint(&self, fp: &EventFingerprint) -> Option<Schema> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT schema_json FROM cc_schemas WHERE fingerprint = ?1",
            params![fp.as_hex()],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn get_by_name(&self, name: &str, version: Option<u32>) -> Option<Schema> {
        let conn = self.conn.lock().unwrap();
        let v = match version {
            Some(v) => v,
            None => self.latest_version_of(&conn, name)?,
        };
        conn.query_row(
            "SELECT schema_json FROM cc_schemas WHERE name = ?1 AND version = ?2",
            params![name, v],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
    }

    fn list_for_chain(&self, chain_slug: &str) -> Vec<Schema> {
        let conn = self.conn.lock().unwrap();
        // chains column is a comma-separated list; use LIKE for simple matching
        let pattern = format!("%{chain_slug}%");
        let mut stmt = conn
            .prepare(
                "SELECT schema_json FROM cc_schemas WHERE chains LIKE ?1",
            )
            .unwrap();
        stmt.query_map(params![pattern], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<Schema>(&json).ok())
            // Verify the chain_slug is an exact match in the list (not just a substring)
            .filter(|s| s.chains.iter().any(|c| c == chain_slug))
            .collect()
    }

    fn history(&self, name: &str) -> Vec<Schema> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT schema_json FROM cc_schemas WHERE name = ?1 ORDER BY version ASC",
            )
            .unwrap();
        stmt.query_map(params![name], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<Schema>(&json).ok())
            .collect()
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn sqlite_err(e: rusqlite::Error) -> RegistryError {
    RegistryError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("sqlite error: {e}"),
    ))
}

fn walkdir_csdl(dir: &Path) -> Result<Vec<std::path::PathBuf>, RegistryError> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(RegistryError::Io)? {
        let entry = entry.map_err(RegistryError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walkdir_csdl(&path)?);
        } else if path.extension().map(|e| e == "csdl").unwrap_or(false) {
            files.push(path);
        }
    }
    Ok(files)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chaincodec_core::{
        event::EventFingerprint,
        schema::{Schema, SchemaMeta},
    };

    fn make_schema(name: &str, version: u32, fp: &str, chains: &[&str]) -> Schema {
        Schema {
            name: name.to_string(),
            version,
            chains: chains.iter().map(|s| s.to_string()).collect(),
            address: None,
            event: "TestEvent".into(),
            fingerprint: EventFingerprint::new(fp),
            supersedes: None,
            superseded_by: None,
            deprecated: false,
            fields: vec![],
            meta: SchemaMeta::default(),
        }
    }

    #[test]
    fn open_in_memory_and_add() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s = make_schema("ERC20Transfer", 1, "0xddf252ad", &["ethereum"]);
        reg.add(&s).unwrap();
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn duplicate_add_rejected() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s = make_schema("Foo", 1, "0x01", &["ethereum"]);
        reg.add(&s).unwrap();
        let err = reg.add(&s);
        assert!(err.is_err());
    }

    #[test]
    fn upsert_overwrites() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s = make_schema("Foo", 1, "0x01", &["ethereum"]);
        reg.upsert(&s).unwrap();
        reg.upsert(&s).unwrap(); // no error
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn get_by_fingerprint() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s = make_schema("ERC20Transfer", 1, "0xfeedbeef", &["ethereum"]);
        reg.add(&s).unwrap();
        let found = reg.get_by_fingerprint(&EventFingerprint::new("0xfeedbeef"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "ERC20Transfer");
    }

    #[test]
    fn get_by_name_latest() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let mut s1 = make_schema("Foo", 1, "0x01", &["ethereum"]);
        s1.deprecated = true;
        let s2 = make_schema("Foo", 2, "0x02", &["ethereum"]);
        reg.add(&s1).unwrap();
        reg.add(&s2).unwrap();
        // Latest non-deprecated should be v2
        let found = reg.get_by_name("Foo", None).unwrap();
        assert_eq!(found.version, 2);
    }

    #[test]
    fn get_by_name_specific_version() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s1 = make_schema("Bar", 1, "0xaa", &["ethereum"]);
        let s2 = make_schema("Bar", 2, "0xbb", &["ethereum"]);
        reg.add(&s1).unwrap();
        reg.add(&s2).unwrap();
        let found = reg.get_by_name("Bar", Some(1)).unwrap();
        assert_eq!(found.version, 1);
    }

    #[test]
    fn list_for_chain() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let eth = make_schema("ETHEvent", 1, "0xee1", &["ethereum"]);
        let arb = make_schema("ARBEvent", 1, "0xee2", &["arbitrum"]);
        let both = make_schema("BothEvent", 1, "0xee3", &["ethereum", "arbitrum"]);
        reg.add(&eth).unwrap();
        reg.add(&arb).unwrap();
        reg.add(&both).unwrap();

        let eth_schemas = reg.list_for_chain("ethereum");
        let names: Vec<_> = eth_schemas.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"ETHEvent"));
        assert!(names.contains(&"BothEvent"));
        assert!(!names.contains(&"ARBEvent"));
    }

    #[test]
    fn history_ordered() {
        let reg = SqliteRegistry::in_memory().unwrap();
        let s1 = make_schema("Evolving", 1, "0xhist1", &["ethereum"]);
        let s2 = make_schema("Evolving", 2, "0xhist2", &["ethereum"]);
        let s3 = make_schema("Evolving", 3, "0xhist3", &["ethereum"]);
        reg.add(&s1).unwrap();
        reg.add(&s3).unwrap();
        reg.add(&s2).unwrap();

        let hist = reg.history("Evolving");
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[0].version, 1);
        assert_eq!(hist[1].version, 2);
        assert_eq!(hist[2].version, 3);
    }

    #[test]
    fn all_names() {
        let reg = SqliteRegistry::in_memory().unwrap();
        reg.add(&make_schema("Alpha", 1, "0xa1", &["ethereum"])).unwrap();
        reg.add(&make_schema("Beta", 1, "0xb1", &["ethereum"])).unwrap();
        reg.add(&make_schema("Alpha", 2, "0xa2", &["ethereum"])).unwrap();

        let names = reg.all_names();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }
}
