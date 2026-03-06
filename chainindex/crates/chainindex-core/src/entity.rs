//! Entity system — structured storage for indexed blockchain data.
//!
//! Entities are typed records (like database rows) that handlers insert
//! during indexing. The entity system provides:
//! - Schema definition (field names, types, indexes)
//! - Insert, upsert, delete operations
//! - Query with filters
//! - Automatic rollback on reorg (delete entities above fork block)
//!
//! # Example
//!
//! ```rust
//! use chainindex_core::entity::{EntitySchemaBuilder, FieldType};
//!
//! let schema = EntitySchemaBuilder::new("erc20_transfer")
//!     .primary_key("id")
//!     .field("from", FieldType::String, true)
//!     .field("to", FieldType::String, true)
//!     .field("amount", FieldType::Uint64, false)
//!     .nullable_field("memo", FieldType::String, false)
//!     .build();
//!
//! assert_eq!(schema.name, "erc20_transfer");
//! assert_eq!(schema.primary_key, "id");
//! assert_eq!(schema.fields.len(), 4);
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::IndexerError;

// ─── Field Types ─────────────────────────────────────────────────────────────

/// Supported field types for entity schemas.
///
/// These map to database column types in concrete storage backends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    /// UTF-8 string (TEXT in SQL).
    String,
    /// Signed 64-bit integer (BIGINT in SQL).
    Int64,
    /// Unsigned 64-bit integer (stored as BIGINT in most backends).
    Uint64,
    /// 64-bit floating point (DOUBLE/REAL in SQL).
    Float64,
    /// Boolean (BOOLEAN in SQL).
    Bool,
    /// Arbitrary JSON value (JSONB in Postgres, TEXT in SQLite).
    Json,
    /// Raw byte data (BYTEA in Postgres, BLOB in SQLite).
    Bytes,
}

// ─── EntityField ─────────────────────────────────────────────────────────────

/// A single field in an entity schema.
///
/// Describes the name, type, and indexing behavior of one column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityField {
    /// The field name (column name).
    pub name: std::string::String,
    /// The field type.
    pub field_type: FieldType,
    /// Whether a database index should be created on this field.
    pub indexed: bool,
    /// Whether the field can be NULL.
    pub nullable: bool,
}

// ─── EntitySchema ────────────────────────────────────────────────────────────

/// Schema definition for an entity.
///
/// Every entity automatically has a `block_number` field (u64) used for
/// reorg rollback via [`EntityStore::delete_after_block`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySchema {
    /// The entity/table name (e.g. `"erc20_transfer"`).
    pub name: std::string::String,
    /// The field name used as the primary key.
    pub primary_key: std::string::String,
    /// The fields in this entity.
    pub fields: Vec<EntityField>,
    // block_number is always implicit for reorg rollback.
}

// ─── EntityRow ───────────────────────────────────────────────────────────────

/// A single row in the entity store (dynamic key-value).
///
/// The `data` map holds all non-system fields as JSON values.
/// System fields (`id`, `entity_type`, `block_number`, `tx_hash`, `log_index`)
/// are stored as top-level fields for efficient filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRow {
    /// Primary key value.
    pub id: std::string::String,
    /// The entity schema name (table name).
    pub entity_type: std::string::String,
    /// Block number where this entity was created/updated.
    pub block_number: u64,
    /// Transaction hash that produced this entity.
    pub tx_hash: std::string::String,
    /// Log index within the block.
    pub log_index: u32,
    /// User-defined field data.
    pub data: HashMap<std::string::String, serde_json::Value>,
}

// ─── Query Types ─────────────────────────────────────────────────────────────

/// Query filter for entities.
///
/// Build queries using the builder methods:
///
/// ```rust
/// use chainindex_core::entity::{EntityQuery, QueryFilter, SortOrder};
///
/// let query = EntityQuery::new("erc20_transfer")
///     .filter(QueryFilter::Eq("from".into(), serde_json::json!("0xAlice")))
///     .order_by("block_number", SortOrder::Desc)
///     .limit(10);
/// ```
#[derive(Debug, Clone, Default)]
pub struct EntityQuery {
    /// The entity type to query.
    pub entity_type: std::string::String,
    /// Filters to apply.
    pub filters: Vec<QueryFilter>,
    /// Sort order.
    pub order_by: Option<(std::string::String, SortOrder)>,
    /// Maximum number of results.
    pub limit: Option<usize>,
    /// Number of results to skip.
    pub offset: Option<usize>,
}

impl EntityQuery {
    /// Create a new query for the given entity type.
    pub fn new(entity_type: impl Into<std::string::String>) -> Self {
        Self {
            entity_type: entity_type.into(),
            filters: Vec::new(),
            order_by: None,
            limit: None,
            offset: None,
        }
    }

    /// Add a filter to the query.
    pub fn filter(mut self, f: QueryFilter) -> Self {
        self.filters.push(f);
        self
    }

    /// Set the sort order.
    pub fn order_by(mut self, field: impl Into<std::string::String>, order: SortOrder) -> Self {
        self.order_by = Some((field.into(), order));
        self
    }

    /// Set the maximum number of results.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set the offset (number of results to skip).
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = Some(n);
        self
    }
}

/// A single filter predicate for an entity query.
#[derive(Debug, Clone)]
pub enum QueryFilter {
    /// Field equals value.
    Eq(std::string::String, serde_json::Value),
    /// Field is greater than value.
    Gt(std::string::String, serde_json::Value),
    /// Field is less than value.
    Lt(std::string::String, serde_json::Value),
    /// Field is greater than or equal to value.
    Gte(std::string::String, serde_json::Value),
    /// Field is less than or equal to value.
    Lte(std::string::String, serde_json::Value),
    /// Field is one of the given values.
    In(std::string::String, Vec<serde_json::Value>),
    /// Field is between two values (inclusive).
    Between(std::string::String, serde_json::Value, serde_json::Value),
}

/// Sort order for query results.
#[derive(Debug, Clone, Copy)]
pub enum SortOrder {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

// ─── EntityStore trait ───────────────────────────────────────────────────────

/// Trait for entity storage backends.
///
/// Backends (memory, SQLite, Postgres) implement this trait to provide
/// structured entity storage.
#[async_trait::async_trait]
pub trait EntityStore: Send + Sync {
    /// Register (create) a schema. Backends may create tables/collections.
    async fn register_schema(&self, schema: &EntitySchema) -> Result<(), IndexerError>;

    /// Insert a new entity row. Errors if a row with the same primary key exists.
    async fn insert(&self, row: EntityRow) -> Result<(), IndexerError>;

    /// Insert or update an entity row. If a row with the same primary key exists,
    /// it is replaced.
    async fn upsert(&self, row: EntityRow) -> Result<(), IndexerError>;

    /// Delete a single entity by type and primary key.
    async fn delete(&self, entity_type: &str, id: &str) -> Result<(), IndexerError>;

    /// Delete all entities of the given type with `block_number > block_number`.
    /// Used during reorg rollback. Returns the number of deleted rows.
    async fn delete_after_block(
        &self,
        entity_type: &str,
        block_number: u64,
    ) -> Result<u64, IndexerError>;

    /// Query entities with filters, sorting, and pagination.
    async fn query(&self, query: EntityQuery) -> Result<Vec<EntityRow>, IndexerError>;

    /// Count entities of the given type.
    async fn count(&self, entity_type: &str) -> Result<u64, IndexerError>;
}

// ─── EntitySchemaBuilder ─────────────────────────────────────────────────────

/// Fluent builder for [`EntitySchema`].
///
/// # Example
///
/// ```rust
/// use chainindex_core::entity::{EntitySchemaBuilder, FieldType};
///
/// let schema = EntitySchemaBuilder::new("swap")
///     .primary_key("id")
///     .field("pair", FieldType::String, true)
///     .field("amount_in", FieldType::Uint64, false)
///     .field("amount_out", FieldType::Uint64, false)
///     .nullable_field("memo", FieldType::String, false)
///     .build();
/// ```
pub struct EntitySchemaBuilder {
    name: std::string::String,
    primary_key: std::string::String,
    fields: Vec<EntityField>,
}

impl EntitySchemaBuilder {
    /// Create a new builder for an entity with the given name.
    pub fn new(name: impl Into<std::string::String>) -> Self {
        Self {
            name: name.into(),
            primary_key: "id".to_string(),
            fields: Vec::new(),
        }
    }

    /// Set the primary key field name (default: `"id"`).
    pub fn primary_key(mut self, pk: impl Into<std::string::String>) -> Self {
        self.primary_key = pk.into();
        self
    }

    /// Add a required (non-nullable) field.
    pub fn field(
        mut self,
        name: impl Into<std::string::String>,
        field_type: FieldType,
        indexed: bool,
    ) -> Self {
        self.fields.push(EntityField {
            name: name.into(),
            field_type,
            indexed,
            nullable: false,
        });
        self
    }

    /// Add a nullable field.
    pub fn nullable_field(
        mut self,
        name: impl Into<std::string::String>,
        field_type: FieldType,
        indexed: bool,
    ) -> Self {
        self.fields.push(EntityField {
            name: name.into(),
            field_type,
            indexed,
            nullable: true,
        });
        self
    }

    /// Build the [`EntitySchema`].
    pub fn build(self) -> EntitySchema {
        EntitySchema {
            name: self.name,
            primary_key: self.primary_key,
            fields: self.fields,
        }
    }
}

// ─── MemoryEntityStore ───────────────────────────────────────────────────────

/// In-memory entity store for testing and development.
///
/// Stores entities in a `HashMap<(entity_type, id), EntityRow>` behind
/// a `Mutex`. Not suitable for production (no persistence).
pub struct MemoryEntityStore {
    /// Registered schemas: entity_type -> EntitySchema.
    schemas: Mutex<HashMap<std::string::String, EntitySchema>>,
    /// All stored rows: (entity_type, id) -> EntityRow.
    rows: Mutex<HashMap<(std::string::String, std::string::String), EntityRow>>,
}

impl MemoryEntityStore {
    /// Create a new empty in-memory entity store.
    pub fn new() -> Self {
        Self {
            schemas: Mutex::new(HashMap::new()),
            rows: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryEntityStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Check whether a single row matches a query filter.
fn matches_filter(row: &EntityRow, filter: &QueryFilter) -> bool {
    match filter {
        QueryFilter::Eq(field, value) => row.data.get(field) == Some(value),
        QueryFilter::Gt(field, value) => row
            .data
            .get(field)
            .is_some_and(|v| json_cmp(v, value) == Some(std::cmp::Ordering::Greater)),
        QueryFilter::Lt(field, value) => row
            .data
            .get(field)
            .is_some_and(|v| json_cmp(v, value) == Some(std::cmp::Ordering::Less)),
        QueryFilter::Gte(field, value) => row.data.get(field).is_some_and(|v| {
            matches!(
                json_cmp(v, value),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            )
        }),
        QueryFilter::Lte(field, value) => row.data.get(field).is_some_and(|v| {
            matches!(
                json_cmp(v, value),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        }),
        QueryFilter::In(field, values) => row.data.get(field).is_some_and(|v| values.contains(v)),
        QueryFilter::Between(field, low, high) => row.data.get(field).is_some_and(|v| {
            matches!(
                json_cmp(v, low),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ) && matches!(
                json_cmp(v, high),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
        }),
    }
}

/// Compare two JSON values numerically or lexicographically.
fn json_cmp(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
    // Try numeric comparison first.
    if let (Some(an), Some(bn)) = (a.as_f64(), b.as_f64()) {
        return an.partial_cmp(&bn);
    }
    // Try string comparison.
    if let (Some(a_str), Some(b_str)) = (a.as_str(), b.as_str()) {
        return Some(a_str.cmp(b_str));
    }
    None
}

#[async_trait::async_trait]
impl EntityStore for MemoryEntityStore {
    async fn register_schema(&self, schema: &EntitySchema) -> Result<(), IndexerError> {
        let mut schemas = self
            .schemas
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        schemas.insert(schema.name.clone(), schema.clone());
        Ok(())
    }

    async fn insert(&self, row: EntityRow) -> Result<(), IndexerError> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        let key = (row.entity_type.clone(), row.id.clone());
        if rows.contains_key(&key) {
            return Err(IndexerError::Storage(format!(
                "entity '{}' with id '{}' already exists",
                row.entity_type, row.id
            )));
        }
        rows.insert(key, row);
        Ok(())
    }

    async fn upsert(&self, row: EntityRow) -> Result<(), IndexerError> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        let key = (row.entity_type.clone(), row.id.clone());
        rows.insert(key, row);
        Ok(())
    }

    async fn delete(&self, entity_type: &str, id: &str) -> Result<(), IndexerError> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        rows.remove(&(entity_type.to_string(), id.to_string()));
        Ok(())
    }

    async fn delete_after_block(
        &self,
        entity_type: &str,
        block_number: u64,
    ) -> Result<u64, IndexerError> {
        let mut rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        let to_remove: Vec<_> = rows
            .iter()
            .filter(|((et, _), row)| et == entity_type && row.block_number > block_number)
            .map(|(key, _)| key.clone())
            .collect();
        let count = to_remove.len() as u64;
        for key in to_remove {
            rows.remove(&key);
        }
        Ok(count)
    }

    async fn query(&self, query: EntityQuery) -> Result<Vec<EntityRow>, IndexerError> {
        let rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;

        // Filter by entity_type and all query filters.
        let mut results: Vec<EntityRow> = rows
            .values()
            .filter(|row| {
                row.entity_type == query.entity_type
                    && query.filters.iter().all(|f| matches_filter(row, f))
            })
            .cloned()
            .collect();

        // Sort.
        if let Some((ref field, ref order)) = query.order_by {
            results.sort_by(|a, b| {
                let va = a.data.get(field);
                let vb = b.data.get(field);
                let cmp = match (va, vb) {
                    (Some(va), Some(vb)) => json_cmp(va, vb).unwrap_or(std::cmp::Ordering::Equal),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                match order {
                    SortOrder::Asc => cmp,
                    SortOrder::Desc => cmp.reverse(),
                }
            });
        }

        // Offset.
        if let Some(offset) = query.offset {
            if offset < results.len() {
                results = results.split_off(offset);
            } else {
                results.clear();
            }
        }

        // Limit.
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn count(&self, entity_type: &str) -> Result<u64, IndexerError> {
        let rows = self
            .rows
            .lock()
            .map_err(|e| IndexerError::Storage(format!("lock poisoned: {e}")))?;
        let count = rows
            .values()
            .filter(|row| row.entity_type == entity_type)
            .count() as u64;
        Ok(count)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_schema() -> EntitySchema {
        EntitySchemaBuilder::new("transfer")
            .primary_key("id")
            .field("from", FieldType::String, true)
            .field("to", FieldType::String, true)
            .field("amount", FieldType::Uint64, false)
            .nullable_field("memo", FieldType::String, false)
            .build()
    }

    fn make_row(id: &str, from: &str, to: &str, amount: u64, block: u64) -> EntityRow {
        let mut data = HashMap::new();
        data.insert("from".to_string(), serde_json::json!(from));
        data.insert("to".to_string(), serde_json::json!(to));
        data.insert("amount".to_string(), serde_json::json!(amount));
        EntityRow {
            id: id.to_string(),
            entity_type: "transfer".to_string(),
            block_number: block,
            tx_hash: format!("0xtx_{id}"),
            log_index: 0,
            data,
        }
    }

    #[tokio::test]
    async fn register_schema() {
        let store = MemoryEntityStore::new();
        let schema = test_schema();
        store.register_schema(&schema).await.unwrap();
        // Re-registering should overwrite without error.
        store.register_schema(&schema).await.unwrap();
    }

    #[tokio::test]
    async fn insert_and_query() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        let row = make_row("t1", "0xAlice", "0xBob", 100, 10);
        store.insert(row).await.unwrap();

        let results = store.query(EntityQuery::new("transfer")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");
    }

    #[tokio::test]
    async fn insert_duplicate_fails() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        let row = make_row("t1", "0xAlice", "0xBob", 100, 10);
        store.insert(row.clone()).await.unwrap();

        let err = store.insert(row).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("already exists"), "got: {msg}");
    }

    #[tokio::test]
    async fn upsert_overwrites() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        let row1 = make_row("t1", "0xAlice", "0xBob", 100, 10);
        store.insert(row1).await.unwrap();

        // Upsert with different amount.
        let row2 = make_row("t1", "0xAlice", "0xBob", 200, 11);
        store.upsert(row2).await.unwrap();

        let results = store.query(EntityQuery::new("transfer")).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data["amount"], serde_json::json!(200));
        assert_eq!(results[0].block_number, 11);
    }

    #[tokio::test]
    async fn delete_entity() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();

        store.delete("transfer", "t1").await.unwrap();

        let count = store.count("transfer").await.unwrap();
        assert_eq!(count, 1);

        let results = store.query(EntityQuery::new("transfer")).await.unwrap();
        assert_eq!(results[0].id, "t2");
    }

    #[tokio::test]
    async fn delete_after_block_for_reorg() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 300, 12))
            .await
            .unwrap();
        store
            .insert(make_row("t4", "0xA", "0xE", 400, 13))
            .await
            .unwrap();

        // Reorg: delete everything after block 11.
        let deleted = store.delete_after_block("transfer", 11).await.unwrap();
        assert_eq!(deleted, 2); // t3 (12) and t4 (13)

        let count = store.count("transfer").await.unwrap();
        assert_eq!(count, 2); // t1 (10) and t2 (11) remain
    }

    #[tokio::test]
    async fn query_with_eq_filter() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xAlice", "0xBob", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xAlice", "0xCharlie", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xBob", "0xCharlie", 300, 12))
            .await
            .unwrap();

        let results = store
            .query(
                EntityQuery::new("transfer")
                    .filter(QueryFilter::Eq("from".into(), serde_json::json!("0xAlice"))),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|r| r.data["from"] == serde_json::json!("0xAlice")));
    }

    #[tokio::test]
    async fn query_with_gt_lt_filters() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 300, 12))
            .await
            .unwrap();

        // amount > 100 AND amount < 300 => only t2 (200)
        let results = store
            .query(
                EntityQuery::new("transfer")
                    .filter(QueryFilter::Gt("amount".into(), serde_json::json!(100)))
                    .filter(QueryFilter::Lt("amount".into(), serde_json::json!(300))),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t2");
    }

    #[tokio::test]
    async fn query_with_in_filter() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xAlice", "0xBob", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xBob", "0xCharlie", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xDave", "0xEve", 300, 12))
            .await
            .unwrap();

        let results = store
            .query(EntityQuery::new("transfer").filter(QueryFilter::In(
                "from".into(),
                vec![serde_json::json!("0xAlice"), serde_json::json!("0xDave")],
            )))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn query_with_sort_and_limit() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 300, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 100, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 200, 12))
            .await
            .unwrap();

        // Sort by amount ascending, limit 2.
        let results = store
            .query(
                EntityQuery::new("transfer")
                    .order_by("amount", SortOrder::Asc)
                    .limit(2),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].data["amount"], serde_json::json!(100));
        assert_eq!(results[1].data["amount"], serde_json::json!(200));
    }

    #[tokio::test]
    async fn query_with_sort_desc() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 300, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 200, 12))
            .await
            .unwrap();

        let results = store
            .query(EntityQuery::new("transfer").order_by("amount", SortOrder::Desc))
            .await
            .unwrap();
        assert_eq!(results[0].data["amount"], serde_json::json!(300));
        assert_eq!(results[1].data["amount"], serde_json::json!(200));
        assert_eq!(results[2].data["amount"], serde_json::json!(100));
    }

    #[tokio::test]
    async fn count_entities() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        assert_eq!(store.count("transfer").await.unwrap(), 0);

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();

        assert_eq!(store.count("transfer").await.unwrap(), 2);
        // Different entity type returns 0.
        assert_eq!(store.count("approval").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn schema_builder_defaults() {
        let schema = EntitySchemaBuilder::new("test_entity")
            .field("name", FieldType::String, true)
            .field("value", FieldType::Uint64, false)
            .build();

        assert_eq!(schema.name, "test_entity");
        assert_eq!(schema.primary_key, "id"); // default primary key
        assert_eq!(schema.fields.len(), 2);
        assert!(schema.fields[0].indexed);
        assert!(!schema.fields[0].nullable);
        assert!(!schema.fields[1].indexed);
    }

    #[tokio::test]
    async fn query_with_between_filter() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 300, 12))
            .await
            .unwrap();
        store
            .insert(make_row("t4", "0xA", "0xE", 400, 13))
            .await
            .unwrap();

        let results = store
            .query(EntityQuery::new("transfer").filter(QueryFilter::Between(
                "amount".into(),
                serde_json::json!(200),
                serde_json::json!(300),
            )))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| {
            let amt = r.data["amount"].as_u64().unwrap();
            (200..=300).contains(&amt)
        }));
    }

    #[tokio::test]
    async fn query_with_offset() {
        let store = MemoryEntityStore::new();
        store.register_schema(&test_schema()).await.unwrap();

        store
            .insert(make_row("t1", "0xA", "0xB", 100, 10))
            .await
            .unwrap();
        store
            .insert(make_row("t2", "0xA", "0xC", 200, 11))
            .await
            .unwrap();
        store
            .insert(make_row("t3", "0xA", "0xD", 300, 12))
            .await
            .unwrap();

        // Sort ascending by amount, skip first, take 1.
        let results = store
            .query(
                EntityQuery::new("transfer")
                    .order_by("amount", SortOrder::Asc)
                    .offset(1)
                    .limit(1),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].data["amount"], serde_json::json!(200));
    }
}
