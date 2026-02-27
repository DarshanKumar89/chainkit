//! In-memory `SchemaRegistry` implementation.
//!
//! Suitable for testing, CLI use, and embedded deployments.
//! Thread-safe via `Arc<RwLock<Inner>>`.

use chaincodec_core::{
    error::RegistryError,
    event::EventFingerprint,
    schema::{Schema, SchemaRegistry},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
};

use crate::csdl::CsdlParser;

/// Key for schema lookup by name + version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct NameVersion(String, u32);

struct Inner {
    /// Fingerprint → Schema (latest version)
    by_fingerprint: HashMap<String, Schema>,
    /// (name, version) → Schema
    by_name_version: HashMap<NameVersion, Schema>,
    /// name → sorted list of versions (ascending)
    versions: HashMap<String, Vec<u32>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            by_fingerprint: HashMap::new(),
            by_name_version: HashMap::new(),
            versions: HashMap::new(),
        }
    }

    fn insert(&mut self, schema: Schema) {
        // Index by fingerprint
        self.by_fingerprint
            .insert(schema.fingerprint.as_hex().to_string(), schema.clone());

        // Track version list
        let versions = self.versions.entry(schema.name.clone()).or_default();
        if !versions.contains(&schema.version) {
            versions.push(schema.version);
            versions.sort_unstable();
        }

        // Index by name + version
        self.by_name_version
            .insert(NameVersion(schema.name.clone(), schema.version), schema);
    }

    fn latest_version(&self, name: &str) -> Option<u32> {
        self.versions
            .get(name)?
            .iter()
            .rev()
            .find(|&&v| {
                self.by_name_version
                    .get(&NameVersion(name.to_string(), v))
                    .map(|s| !s.deprecated)
                    .unwrap_or(false)
            })
            .copied()
    }
}

/// Thread-safe in-memory schema registry.
#[derive(Clone)]
pub struct MemoryRegistry {
    inner: Arc<RwLock<Inner>>,
}

impl MemoryRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::new())),
        }
    }

    /// Add a schema to the registry.
    pub fn add(&self, schema: Schema) -> Result<(), RegistryError> {
        let mut inner = self.inner.write().unwrap();
        let key = NameVersion(schema.name.clone(), schema.version);
        if inner.by_name_version.contains_key(&key) {
            return Err(RegistryError::AlreadyExists {
                name: schema.name.clone(),
                version: schema.version,
            });
        }
        inner.insert(schema);
        Ok(())
    }

    /// Load all `.csdl` files from a directory recursively.
    ///
    /// Each file may contain multiple schema documents (separated by `---`).
    /// Returns the total number of schema versions loaded.
    pub fn load_directory(&self, dir: &Path) -> Result<usize, RegistryError> {
        let mut count = 0;
        for entry in walkdir_csdl(dir)? {
            let content = std::fs::read_to_string(&entry).map_err(RegistryError::Io)?;
            for schema in CsdlParser::parse_all(&content)? {
                self.add(schema)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Load a single CSDL file.
    ///
    /// If the file contains multiple schemas (separated by `---`), all are
    /// added to the registry. Returns the count of schemas loaded.
    pub fn load_file(&self, path: &Path) -> Result<usize, RegistryError> {
        let content = std::fs::read_to_string(path).map_err(RegistryError::Io)?;
        let schemas = CsdlParser::parse_all(&content)?;
        if schemas.is_empty() {
            return Err(RegistryError::ParseError("empty CSDL file".into()));
        }
        let count = schemas.len();
        for schema in schemas {
            self.add(schema)?;
        }
        Ok(count)
    }

    /// Alias for `add()` — adds a schema to the registry.
    pub fn insert(&self, schema: Schema) -> Result<(), RegistryError> {
        self.add(schema)
    }

    /// Returns the total number of schema versions stored.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().by_name_version.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns all schema names (deduplicated, one entry per name regardless of versions).
    pub fn all_names(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        let mut names: Vec<String> = inner.versions.keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns all schemas (latest non-deprecated version of each).
    pub fn all_schemas(&self) -> Vec<Schema> {
        let inner = self.inner.read().unwrap();
        let mut schemas = Vec::new();
        for name in inner.versions.keys() {
            if let Some(v) = inner.latest_version(name) {
                if let Some(s) = inner.by_name_version.get(&NameVersion(name.clone(), v)) {
                    schemas.push(s.clone());
                }
            }
        }
        schemas
    }
}

impl Default for MemoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaRegistry for MemoryRegistry {
    fn get_by_fingerprint(&self, fp: &EventFingerprint) -> Option<Schema> {
        self.inner
            .read()
            .unwrap()
            .by_fingerprint
            .get(fp.as_hex())
            .cloned()
    }

    fn get_by_name(&self, name: &str, version: Option<u32>) -> Option<Schema> {
        let inner = self.inner.read().unwrap();
        let v = match version {
            Some(v) => v,
            None => inner.latest_version(name)?,
        };
        inner
            .by_name_version
            .get(&NameVersion(name.to_string(), v))
            .cloned()
    }

    fn list_for_chain(&self, chain_slug: &str) -> Vec<Schema> {
        self.inner
            .read()
            .unwrap()
            .by_name_version
            .values()
            .filter(|s| s.chains.iter().any(|c| c == chain_slug))
            .cloned()
            .collect()
    }

    fn history(&self, name: &str) -> Vec<Schema> {
        let inner = self.inner.read().unwrap();
        let versions = match inner.versions.get(name) {
            Some(v) => v.clone(),
            None => return Vec::new(),
        };
        versions
            .iter()
            .filter_map(|&v| {
                inner
                    .by_name_version
                    .get(&NameVersion(name.to_string(), v))
                    .cloned()
            })
            .collect()
    }
}

/// Collect all `.csdl` files under `dir` recursively.
fn walkdir_csdl(dir: &Path) -> Result<Vec<std::path::PathBuf>, RegistryError> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Err(RegistryError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a directory", dir.display()),
        )));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_schema(name: &str, version: u32, fingerprint: &str) -> Schema {
        use chaincodec_core::schema::SchemaMeta;
        Schema {
            name: name.to_string(),
            version,
            chains: vec!["ethereum".into()],
            address: None,
            event: "Transfer".into(),
            fingerprint: EventFingerprint::new(fingerprint),
            supersedes: None,
            superseded_by: None,
            deprecated: false,
            fields: vec![],
            meta: SchemaMeta::default(),
        }
    }

    #[test]
    fn add_and_lookup() {
        let reg = MemoryRegistry::new();
        let schema = make_schema(
            "ERC20Transfer",
            1,
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        );
        reg.add(schema.clone()).unwrap();

        let found = reg.get_by_fingerprint(&EventFingerprint::new(
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        ));
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "ERC20Transfer");
    }

    #[test]
    fn duplicate_rejected() {
        let reg = MemoryRegistry::new();
        let s = make_schema("ERC20Transfer", 1, "0xabc");
        reg.add(s.clone()).unwrap();
        let err = reg.add(s);
        assert!(err.is_err());
    }

    #[test]
    fn latest_version_skips_deprecated() {
        let reg = MemoryRegistry::new();
        let mut s1 = make_schema("Foo", 1, "0x01");
        let s2 = make_schema("Foo", 2, "0x02");
        s1.deprecated = true;
        reg.add(s1).unwrap();
        reg.add(s2).unwrap();

        let found = reg.get_by_name("Foo", None).unwrap();
        assert_eq!(found.version, 2);
    }
}
