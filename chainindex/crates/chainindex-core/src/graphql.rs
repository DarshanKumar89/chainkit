//! Lightweight GraphQL-like query layer for the chainindex entity store.
//!
//! This module implements a self-contained, zero-external-dependency GraphQL
//! query engine that translates a simplified GraphQL query syntax into
//! [`EntityQuery`] calls against any [`EntityStore`] backend.
//!
//! # Supported Syntax
//!
//! Single entity by id:
//! ```text
//! { swap(id: "0x123") { id pool amount0 amount1 } }
//! ```
//!
//! Collection query with filters, ordering, and pagination:
//! ```text
//! {
//!   swaps(
//!     first: 10
//!     skip: 0
//!     where: { pool: "0xABC", amount0_gt: "1000" }
//!     orderBy: "amount0"
//!     orderDirection: "desc"
//!   ) { id pool amount0 amount1 }
//! }
//! ```
//!
//! Introspection:
//! ```text
//! { __schema { types { name } } }
//! ```
//!
//! # Response Format
//!
//! Success: `{ "data": { "<field>": ... } }`
//! Error:   `{ "errors": [{ "message": "..." }] }`

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::entity::{EntityQuery, EntitySchema, EntityStore, FieldType, QueryFilter, SortOrder};
use crate::error::IndexerError;

// ─── GraphQL Error ────────────────────────────────────────────────────────────

/// A single GraphQL error entry, matching the spec shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphqlError {
    /// Human-readable error message.
    pub message: String,
}

impl GraphqlError {
    fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into() }
    }
}

impl From<IndexerError> for GraphqlError {
    fn from(e: IndexerError) -> Self {
        Self::new(e.to_string())
    }
}

// ─── GraphQL Response ─────────────────────────────────────────────────────────

/// A complete GraphQL HTTP response body.
///
/// Either `data` or `errors` is populated — never both in normal operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphqlResponse {
    /// Query result data, keyed by selection field name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,

    /// Error list if the request could not be fulfilled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<GraphqlError>>,
}

impl GraphqlResponse {
    /// Construct a successful data response.
    pub fn ok(data: JsonValue) -> Self {
        Self { data: Some(data), errors: None }
    }

    /// Construct an error response with a single message.
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            data: None,
            errors: Some(vec![GraphqlError::new(msg)]),
        }
    }

    /// Construct an error response from multiple errors.
    pub fn errors(errors: Vec<GraphqlError>) -> Self {
        Self { data: None, errors: Some(errors) }
    }

    /// Returns `true` if this response contains errors.
    pub fn is_error(&self) -> bool {
        self.errors.is_some()
    }
}

// ─── Subscription Config ──────────────────────────────────────────────────────

/// Configuration for real-time entity change subscriptions.
///
/// Describes which entity types and events a subscriber is interested in.
/// The actual WebSocket transport is not implemented here — this struct is
/// carried as metadata for higher-level subscription routers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionConfig {
    /// Entity types to subscribe to (empty = all types).
    pub entity_types: Vec<String>,

    /// Event kinds to receive.
    pub events: Vec<SubscriptionEvent>,

    /// Optional filter: only emit events at or above this block number.
    pub from_block: Option<u64>,

    /// Maximum number of buffered events before backpressure is applied.
    pub buffer_size: usize,
}

/// The kinds of entity change events a subscription may emit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionEvent {
    /// A new entity was inserted.
    Insert,
    /// An existing entity was updated (upsert).
    Update,
    /// An entity was deleted.
    Delete,
    /// Entities were rolled back due to a chain reorg.
    Reorg,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            entity_types: Vec::new(),
            events: vec![
                SubscriptionEvent::Insert,
                SubscriptionEvent::Update,
                SubscriptionEvent::Delete,
                SubscriptionEvent::Reorg,
            ],
            from_block: None,
            buffer_size: 256,
        }
    }
}

// ─── GraphQL Schema ───────────────────────────────────────────────────────────

/// Auto-generated GraphQL SDL schema from registered [`EntitySchema`]s.
///
/// Call [`GraphqlSchema::add_entity`] for each entity type, then
/// [`GraphqlSchema::sdl`] to retrieve the schema definition language string
/// suitable for serving at `/__graphql/schema.graphql`.
#[derive(Debug, Default, Clone)]
pub struct GraphqlSchema {
    entities: Vec<EntitySchema>,
}

impl GraphqlSchema {
    /// Create an empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an entity schema.
    pub fn add_entity(&mut self, schema: EntitySchema) {
        self.entities.push(schema);
    }

    /// Return the GraphQL SDL string for all registered entity types.
    ///
    /// Generates:
    /// - One GraphQL `type` per entity (with system fields + user fields).
    /// - `{Entity}_filter` input types for `where` arguments.
    /// - A root `Query` type with single-entity and collection fields.
    pub fn sdl(&self) -> String {
        let mut out = String::new();

        // Built-in scalar for the 64-bit unsigned integers (not in GraphQL spec).
        out.push_str("scalar BigInt\n\n");

        // Generate one object type + one filter input per entity.
        for schema in &self.entities {
            out.push_str(&self.entity_type_sdl(schema));
            out.push_str(&self.filter_input_sdl(schema));
        }

        // Ordering enum
        out.push_str("enum OrderDirection {\n  asc\n  desc\n}\n\n");

        // Root Query type.
        out.push_str("type Query {\n");
        for schema in &self.entities {
            let type_name = pascal_case(&schema.name);
            let singular = schema.name.clone();
            let plural = format!("{}s", schema.name);
            out.push_str(&format!(
                "  {}(id: String!): {}\n",
                singular, type_name
            ));
            out.push_str(&format!(
                "  {}(where: {}_filter, orderBy: String, orderDirection: OrderDirection, first: Int, skip: Int): [{}!]!\n",
                plural, schema.name, type_name
            ));
        }
        out.push_str("}\n");

        out
    }

    fn entity_type_sdl(&self, schema: &EntitySchema) -> String {
        let type_name = pascal_case(&schema.name);
        let mut out = format!("type {} {{\n", type_name);
        // System fields.
        out.push_str("  id: String!\n");
        out.push_str("  blockNumber: BigInt!\n");
        out.push_str("  txHash: String!\n");
        out.push_str("  logIndex: Int!\n");
        // User fields.
        for field in &schema.fields {
            let gql_type = field_type_to_gql(&field.field_type, field.nullable);
            out.push_str(&format!("  {}: {}\n", field.name, gql_type));
        }
        out.push_str("}\n\n");
        out
    }

    fn filter_input_sdl(&self, schema: &EntitySchema) -> String {
        let mut out = format!("input {}_filter {{\n", schema.name);
        // Allow filtering on every user field with operator suffixes.
        for field in &schema.fields {
            let base = field_type_to_gql_scalar(&field.field_type);
            out.push_str(&format!("  {}: {}\n", field.name, base));
            out.push_str(&format!("  {}_gt: {}\n", field.name, base));
            out.push_str(&format!("  {}_lt: {}\n", field.name, base));
            out.push_str(&format!("  {}_gte: {}\n", field.name, base));
            out.push_str(&format!("  {}_lte: {}\n", field.name, base));
            out.push_str(&format!("  {}_in: [{}]\n", field.name, base));
        }
        out.push_str("}\n\n");
        out
    }
}

// ─── Query Parsing ────────────────────────────────────────────────────────────

/// A parsed top-level GraphQL selection.
#[derive(Debug, Clone)]
struct ParsedSelection {
    /// The root field name (e.g. `"swap"`, `"swaps"`, `"__schema"`).
    field: String,

    /// Arguments passed to the root field (key → raw string or quoted string).
    args: HashMap<String, ArgValue>,

    /// Sub-fields requested (e.g. `["id", "pool", "amount0"]`).
    sub_fields: Vec<String>,
}

/// An argument value from the parsed query.
#[derive(Debug, Clone)]
enum ArgValue {
    /// A string literal: `"0xABC"`.
    Str(String),
    /// A numeric literal: `10`.
    Num(f64),
    /// An object literal: `{ pool: "0xABC", amount0_gt: "1000" }`.
    Obj(HashMap<String, ArgValue>),
    /// An enum literal / bare identifier: `desc`.
    Ident(String),
}

impl ArgValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            ArgValue::Str(s) => Some(s.as_str()),
            ArgValue::Ident(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_usize(&self) -> Option<usize> {
        match self {
            ArgValue::Num(n) => Some(*n as usize),
            _ => None,
        }
    }

    fn as_obj(&self) -> Option<&HashMap<String, ArgValue>> {
        match self {
            ArgValue::Obj(m) => Some(m),
            _ => None,
        }
    }
}

// ─── Minimal Parser ───────────────────────────────────────────────────────────

/// Tokenizer / parser for the supported subset of GraphQL.
struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src: src.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn consume(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied();
        if b.is_some() {
            self.pos += 1;
        }
        b
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b'#' {
                // Skip line comments.
                while let Some(c) = self.consume() {
                    if c == b'\n' {
                        break;
                    }
                }
            } else if b.is_ascii_whitespace() || b == b',' {
                self.consume();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, ch: u8) -> Result<(), String> {
        self.skip_ws();
        match self.consume() {
            Some(b) if b == ch => Ok(()),
            Some(b) => Err(format!(
                "expected '{}' but got '{}' at position {}",
                ch as char, b as char, self.pos
            )),
            None => Err(format!("expected '{}' but reached end of input", ch as char)),
        }
    }

    fn read_name(&mut self) -> Option<String> {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.consume();
            } else {
                break;
            }
        }
        if self.pos > start {
            Some(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
        } else {
            None
        }
    }

    fn read_string(&mut self) -> Result<String, String> {
        // Caller already consumed the opening `"`.
        let mut s = String::new();
        loop {
            match self.consume() {
                Some(b'"') => break,
                Some(b'\\') => match self.consume() {
                    Some(b'"') => s.push('"'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(c) => s.push(c as char),
                    None => return Err("unterminated string escape".into()),
                },
                Some(c) => s.push(c as char),
                None => return Err("unterminated string literal".into()),
            }
        }
        Ok(s)
    }

    fn read_number(&mut self, first: u8) -> ArgValue {
        let mut buf = String::new();
        buf.push(first as char);
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || b == b'.' || b == b'-' || b == b'e' || b == b'E' {
                buf.push(b as char);
                self.consume();
            } else {
                break;
            }
        }
        ArgValue::Num(buf.parse::<f64>().unwrap_or(0.0))
    }

    fn read_arg_value(&mut self) -> Result<ArgValue, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'"') => {
                self.consume();
                Ok(ArgValue::Str(self.read_string()?))
            }
            Some(b'{') => {
                self.consume();
                let obj = self.read_object()?;
                Ok(ArgValue::Obj(obj))
            }
            Some(b) if b.is_ascii_digit() || b == b'-' => {
                let first = self.consume().unwrap();
                Ok(self.read_number(first))
            }
            Some(_) => {
                // Bare identifier / enum value.
                match self.read_name() {
                    Some(name) => Ok(ArgValue::Ident(name)),
                    None => Err(format!("unexpected character at pos {}", self.pos)),
                }
            }
            None => Err("unexpected end of input in argument value".into()),
        }
    }

    fn read_object(&mut self) -> Result<HashMap<String, ArgValue>, String> {
        let mut map = HashMap::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.consume();
                break;
            }
            let key = self.read_name().ok_or("expected object key")?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.read_arg_value()?;
            map.insert(key, val);
        }
        Ok(map)
    }

    fn read_args(&mut self) -> Result<HashMap<String, ArgValue>, String> {
        // Opening `(` already consumed by caller.
        let mut args = HashMap::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b')') {
                self.consume();
                break;
            }
            let key = self.read_name().ok_or("expected argument name")?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.read_arg_value()?;
            args.insert(key, val);
        }
        Ok(args)
    }

    fn read_sub_fields(&mut self) -> Result<Vec<String>, String> {
        // Opening `{` already consumed by caller.
        let mut fields = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.consume();
                break;
            }
            // Check for nested braces (e.g. __schema sub-selections) — skip them.
            if self.peek() == Some(b'{') {
                self.consume();
                self.read_sub_fields()?; // recurse and discard
                continue;
            }
            match self.read_name() {
                Some(name) => {
                    self.skip_ws();
                    // If followed by `{`, skip nested block (introspection).
                    if self.peek() == Some(b'{') {
                        self.consume();
                        self.read_sub_fields()?;
                    }
                    fields.push(name);
                }
                None => {
                    return Err(format!(
                        "expected field name at pos {}",
                        self.pos
                    ));
                }
            }
        }
        Ok(fields)
    }

    /// Parse the entire query, returning a list of top-level selections.
    fn parse(&mut self) -> Result<Vec<ParsedSelection>, String> {
        self.skip_ws();

        // Optionally consume `query` / `mutation` keyword.
        if let Some(b'q') | Some(b'm') | Some(b's') = self.peek() {
            let kw = self.read_name().unwrap_or_default();
            if kw != "query" && kw != "mutation" && kw != "subscription" {
                // Not a keyword — it is a field name at root level without outer `{`.
                // Put position back by re-parsing is non-trivial, so just treat as-is.
                // This is a degenerate query; return empty.
                return Err(format!("unexpected keyword '{kw}' at document start"));
            }
            // Skip optional operation name.
            self.skip_ws();
            if self.peek().map_or(false, |b| b.is_ascii_alphabetic() || b == b'_') {
                self.read_name();
            }
        }

        self.skip_ws();
        self.expect(b'{')?;

        let mut selections = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.consume();
                break;
            }
            let field = self.read_name().ok_or("expected selection field name")?;
            self.skip_ws();

            let mut args = HashMap::new();
            if self.peek() == Some(b'(') {
                self.consume();
                args = self.read_args()?;
            }

            self.skip_ws();
            let mut sub_fields = Vec::new();
            if self.peek() == Some(b'{') {
                self.consume();
                sub_fields = self.read_sub_fields()?;
            }

            selections.push(ParsedSelection { field, args, sub_fields });
        }

        Ok(selections)
    }
}

// ─── GraphQL Executor ─────────────────────────────────────────────────────────

/// Executes simplified GraphQL queries against an [`EntityStore`].
///
/// # Usage
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use chainindex_core::entity::MemoryEntityStore;
/// use chainindex_core::graphql::GraphqlExecutor;
///
/// # async fn example() {
/// let store = Arc::new(MemoryEntityStore::new());
/// let executor = GraphqlExecutor::new(store);
/// let resp = executor.execute(r#"{ transfers(first: 5) { id } }"#).await;
/// # }
/// ```
pub struct GraphqlExecutor {
    store: Arc<dyn EntityStore>,
    schema: RwLock<GraphqlSchema>,
}

impl GraphqlExecutor {
    /// Create a new executor backed by the given entity store.
    pub fn new(store: Arc<dyn EntityStore>) -> Self {
        Self {
            store,
            schema: RwLock::new(GraphqlSchema::new()),
        }
    }

    /// Register an entity schema so the executor knows the field list.
    pub fn register_schema(&self, entity_schema: EntitySchema) {
        let mut schema = self.schema.write().expect("schema lock poisoned");
        schema.add_entity(entity_schema);
    }

    /// Return the SDL for all registered entity types (introspection).
    pub fn introspect(&self) -> String {
        let schema = self.schema.read().expect("schema lock poisoned");
        schema.sdl()
    }

    /// Execute a GraphQL query string and return a [`GraphqlResponse`].
    pub async fn execute(&self, query: &str) -> GraphqlResponse {
        let selections = match Parser::new(query).parse() {
            Ok(s) => s,
            Err(e) => return GraphqlResponse::err(format!("Parse error: {e}")),
        };

        let mut data_map = serde_json::Map::new();
        let mut errors: Vec<GraphqlError> = Vec::new();

        for sel in selections {
            // Introspection shortcut.
            if sel.field == "__schema" || sel.field == "__type" {
                let sdl = self.introspect();
                data_map.insert(sel.field.clone(), JsonValue::String(sdl));
                continue;
            }

            match self.execute_selection(&sel).await {
                Ok(value) => {
                    data_map.insert(sel.field.clone(), value);
                }
                Err(e) => {
                    errors.push(e);
                }
            }
        }

        if !errors.is_empty() {
            return GraphqlResponse::errors(errors);
        }

        GraphqlResponse::ok(JsonValue::Object(data_map))
    }

    /// Execute a single top-level selection against the store.
    async fn execute_selection(
        &self,
        sel: &ParsedSelection,
    ) -> Result<JsonValue, GraphqlError> {
        let field = &sel.field;

        // Determine whether this is a singular or plural field.
        // Convention: if the field name matches a registered entity type exactly
        // and has an `id` argument, treat as singular.  Otherwise, treat as collection.

        let (entity_type, is_singular) = self.resolve_entity_type(field);

        let entity_type = entity_type.ok_or_else(|| {
            GraphqlError::new(format!("Unknown field '{}': no entity type found", field))
        })?;

        if is_singular {
            self.execute_single(&entity_type, sel).await
        } else {
            self.execute_collection(&entity_type, sel).await
        }
    }

    /// Resolve `field` to an `(entity_type, is_singular)` pair.
    ///
    /// Rules:
    /// - If field equals a registered entity name → singular.
    /// - If field equals `<entity_name>s` → collection.
    /// - If field ends with `s` and `field[..len-1]` is registered → collection.
    fn resolve_entity_type(&self, field: &str) -> (Option<String>, bool) {
        let schema = self.schema.read().expect("schema lock poisoned");
        // Exact match → singular.
        if schema.entities.iter().any(|e| e.name == field) {
            return (Some(field.to_string()), true);
        }
        // Plural match: field = entity_name + "s".
        for entity in &schema.entities {
            let plural = format!("{}s", entity.name);
            if *field == plural {
                return (Some(entity.name.clone()), false);
            }
        }
        // No match.
        (None, false)
    }

    /// Execute a singular `entity(id: "...")` query.
    async fn execute_single(
        &self,
        entity_type: &str,
        sel: &ParsedSelection,
    ) -> Result<JsonValue, GraphqlError> {
        let id = sel
            .args
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GraphqlError::new("Singular query requires an 'id' argument"))?;

        let query = EntityQuery::new(entity_type)
            .filter(QueryFilter::Eq("id".to_string(), JsonValue::String(id.to_string())))
            .limit(1);

        // MemoryEntityStore filters by entity_type automatically; we also need to
        // match the system `id` field. The MemoryEntityStore uses (entity_type, id)
        // as a composite key but the query API filters on `data`. We pass id as a
        // special row field via data. However, looking at the implementation,
        // `id` is a top-level field on EntityRow, not in `data`. We therefore
        // query without the id filter in data and manually match below.
        let query_no_id_filter = EntityQuery::new(entity_type);
        let mut rows = self
            .store
            .query(query_no_id_filter)
            .await
            .map_err(GraphqlError::from)?;

        rows.retain(|r| r.id == id);

        if rows.is_empty() {
            return Ok(JsonValue::Null);
        }

        let row = &rows[0];
        Ok(self.project_row(row, &sel.sub_fields))
    }

    /// Execute a collection `entities(where, orderBy, ...)` query.
    async fn execute_collection(
        &self,
        entity_type: &str,
        sel: &ParsedSelection,
    ) -> Result<JsonValue, GraphqlError> {
        let first = sel.args.get("first").and_then(|v| v.as_usize());
        let skip = sel.args.get("skip").and_then(|v| v.as_usize());
        let order_by = sel
            .args
            .get("orderBy")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let order_direction = sel
            .args
            .get("orderDirection")
            .and_then(|v| v.as_str())
            .unwrap_or("asc")
            .to_lowercase();

        let sort_order = if order_direction == "desc" {
            SortOrder::Desc
        } else {
            SortOrder::Asc
        };

        // Build filters from `where` argument.
        let filters = if let Some(where_arg) = sel.args.get("where") {
            let obj = where_arg
                .as_obj()
                .ok_or_else(|| GraphqlError::new("'where' argument must be an object"))?;
            self.parse_where_filters(obj)?
        } else {
            Vec::new()
        };

        let mut q = EntityQuery::new(entity_type);
        for f in filters {
            q = q.filter(f);
        }
        if let Some(ob) = order_by {
            q = q.order_by(ob, sort_order);
        }
        if let Some(n) = first {
            q = q.limit(n);
        }
        if let Some(n) = skip {
            q = q.offset(n);
        }

        let rows = self.store.query(q).await.map_err(GraphqlError::from)?;

        let values: Vec<JsonValue> = rows
            .iter()
            .map(|row| self.project_row(row, &sel.sub_fields))
            .collect();

        Ok(JsonValue::Array(values))
    }

    /// Parse a `where` object into [`QueryFilter`]s.
    ///
    /// Supported key patterns:
    /// - `field`        → Eq
    /// - `field_gt`     → Gt
    /// - `field_lt`     → Lt
    /// - `field_gte`    → Gte
    /// - `field_lte`    → Lte
    /// - `field_in`     → In (value must be a JSON array string or ArgValue::Obj is ignored)
    fn parse_where_filters(
        &self,
        obj: &HashMap<String, ArgValue>,
    ) -> Result<Vec<QueryFilter>, GraphqlError> {
        let mut filters = Vec::new();

        for (key, val) in obj {
            let json_val = arg_to_json(val);

            if let Some(field) = key.strip_suffix("_gt") {
                filters.push(QueryFilter::Gt(field.to_string(), json_val));
            } else if let Some(field) = key.strip_suffix("_lt") {
                filters.push(QueryFilter::Lt(field.to_string(), json_val));
            } else if let Some(field) = key.strip_suffix("_gte") {
                filters.push(QueryFilter::Gte(field.to_string(), json_val));
            } else if let Some(field) = key.strip_suffix("_lte") {
                filters.push(QueryFilter::Lte(field.to_string(), json_val));
            } else if let Some(field) = key.strip_suffix("_in") {
                // Expect an array encoded as a JSON string or a direct ArgValue array.
                // We encode it as a JSON array in json_val.
                let items = match json_val {
                    JsonValue::Array(arr) => arr,
                    JsonValue::String(s) => {
                        // Try to parse it as JSON array.
                        serde_json::from_str::<Vec<JsonValue>>(&s).unwrap_or_else(|_| {
                            vec![JsonValue::String(s)]
                        })
                    }
                    other => vec![other],
                };
                filters.push(QueryFilter::In(field.to_string(), items));
            } else {
                // Plain equality.
                filters.push(QueryFilter::Eq(key.clone(), json_val));
            }
        }

        Ok(filters)
    }

    /// Project an entity row into a JSON object containing only requested fields.
    ///
    /// If `sub_fields` is empty all fields are included.
    fn project_row(&self, row: &crate::entity::EntityRow, sub_fields: &[String]) -> JsonValue {
        let mut obj = serde_json::Map::new();

        let include_all = sub_fields.is_empty();

        let want = |name: &str| -> bool {
            include_all || sub_fields.iter().any(|f| f == name)
        };

        // System fields.
        if want("id") {
            obj.insert("id".to_string(), JsonValue::String(row.id.clone()));
        }
        if want("blockNumber") {
            obj.insert("blockNumber".to_string(), JsonValue::Number(row.block_number.into()));
        }
        if want("txHash") {
            obj.insert("txHash".to_string(), JsonValue::String(row.tx_hash.clone()));
        }
        if want("logIndex") {
            obj.insert(
                "logIndex".to_string(),
                JsonValue::Number(row.log_index.into()),
            );
        }

        // User data fields.
        for (k, v) in &row.data {
            if want(k) {
                obj.insert(k.clone(), v.clone());
            }
        }

        JsonValue::Object(obj)
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Convert an [`ArgValue`] to a [`serde_json::Value`].
fn arg_to_json(val: &ArgValue) -> JsonValue {
    match val {
        ArgValue::Str(s) => JsonValue::String(s.clone()),
        ArgValue::Num(n) => {
            // Prefer integer representation when the number is whole.
            if n.fract() == 0.0 && *n >= 0.0 && *n <= u64::MAX as f64 {
                JsonValue::Number((*n as u64).into())
            } else if n.fract() == 0.0 && *n < 0.0 && *n >= i64::MIN as f64 {
                JsonValue::Number((*n as i64).into())
            } else {
                serde_json::Number::from_f64(*n)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null)
            }
        }
        ArgValue::Ident(s) => {
            // Common boolean literals.
            match s.as_str() {
                "true" => JsonValue::Bool(true),
                "false" => JsonValue::Bool(false),
                "null" => JsonValue::Null,
                _ => JsonValue::String(s.clone()),
            }
        }
        ArgValue::Obj(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                obj.insert(k.clone(), arg_to_json(v));
            }
            JsonValue::Object(obj)
        }
    }
}

/// Convert a `snake_case` entity name to `PascalCase` GraphQL type name.
fn pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut c = part.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

/// Map a [`FieldType`] to a GraphQL type string, respecting nullability.
fn field_type_to_gql(ft: &FieldType, nullable: bool) -> String {
    let base = field_type_to_gql_scalar(ft);
    if nullable {
        base.to_string()
    } else {
        format!("{}!", base)
    }
}

/// Map a [`FieldType`] to a GraphQL scalar name (no nullability suffix).
fn field_type_to_gql_scalar(ft: &FieldType) -> &'static str {
    match ft {
        FieldType::String => "String",
        FieldType::Int64 => "BigInt",
        FieldType::Uint64 => "BigInt",
        FieldType::Float64 => "Float",
        FieldType::Bool => "Boolean",
        FieldType::Json => "String",
        FieldType::Bytes => "String",
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::entity::{
        EntityRow, EntitySchemaBuilder, EntityStore, FieldType, MemoryEntityStore,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn swap_schema() -> EntitySchema {
        EntitySchemaBuilder::new("swap")
            .primary_key("id")
            .field("pool", FieldType::String, true)
            .field("amount0", FieldType::Uint64, false)
            .field("amount1", FieldType::Uint64, false)
            .nullable_field("trader", FieldType::String, false)
            .build()
    }

    fn transfer_schema() -> EntitySchema {
        EntitySchemaBuilder::new("transfer")
            .primary_key("id")
            .field("from", FieldType::String, true)
            .field("to", FieldType::String, true)
            .field("value", FieldType::Uint64, false)
            .build()
    }

    fn make_swap(id: &str, pool: &str, amount0: u64, amount1: u64, block: u64) -> EntityRow {
        let mut data = HashMap::new();
        data.insert("pool".to_string(), serde_json::json!(pool));
        data.insert("amount0".to_string(), serde_json::json!(amount0));
        data.insert("amount1".to_string(), serde_json::json!(amount1));
        EntityRow {
            id: id.to_string(),
            entity_type: "swap".to_string(),
            block_number: block,
            tx_hash: format!("0xtx_{id}"),
            log_index: 0,
            data,
        }
    }

    fn make_transfer(id: &str, from: &str, to: &str, value: u64, block: u64) -> EntityRow {
        let mut data = HashMap::new();
        data.insert("from".to_string(), serde_json::json!(from));
        data.insert("to".to_string(), serde_json::json!(to));
        data.insert("value".to_string(), serde_json::json!(value));
        EntityRow {
            id: id.to_string(),
            entity_type: "transfer".to_string(),
            block_number: block,
            tx_hash: format!("0xtx_{id}"),
            log_index: 0,
            data,
        }
    }

    async fn seeded_executor() -> GraphqlExecutor {
        let store = Arc::new(MemoryEntityStore::new());
        store.register_schema(&swap_schema()).await.unwrap();
        store.register_schema(&transfer_schema()).await.unwrap();

        store.upsert(make_swap("s1", "0xPOOL_A", 1000, 500, 10)).await.unwrap();
        store.upsert(make_swap("s2", "0xPOOL_A", 2000, 1000, 11)).await.unwrap();
        store.upsert(make_swap("s3", "0xPOOL_B", 3000, 1500, 12)).await.unwrap();

        store.upsert(make_transfer("t1", "0xAlice", "0xBob", 100, 10)).await.unwrap();
        store.upsert(make_transfer("t2", "0xBob", "0xCharlie", 200, 11)).await.unwrap();

        let executor = GraphqlExecutor::new(store);
        executor.register_schema(swap_schema());
        executor.register_schema(transfer_schema());
        executor
    }

    // ── Test 1: SDL schema generation ─────────────────────────────────────────

    #[test]
    fn test_schema_generation_contains_type() {
        let mut gql_schema = GraphqlSchema::new();
        gql_schema.add_entity(swap_schema());
        let sdl = gql_schema.sdl();

        assert!(sdl.contains("type Swap {"), "SDL missing Swap type:\n{sdl}");
        assert!(sdl.contains("pool: String!"), "SDL missing pool field:\n{sdl}");
        assert!(sdl.contains("amount0: BigInt!"), "SDL missing amount0 field:\n{sdl}");
        assert!(sdl.contains("trader: String"), "SDL missing nullable trader field:\n{sdl}");
    }

    // ── Test 2: SDL contains filter input ─────────────────────────────────────

    #[test]
    fn test_schema_generation_filter_input() {
        let mut gql_schema = GraphqlSchema::new();
        gql_schema.add_entity(swap_schema());
        let sdl = gql_schema.sdl();

        assert!(sdl.contains("input swap_filter {"), "SDL missing swap_filter input:\n{sdl}");
        assert!(sdl.contains("amount0_gt:"), "SDL missing amount0_gt in filter:\n{sdl}");
        assert!(sdl.contains("pool_in:"), "SDL missing pool_in in filter:\n{sdl}");
    }

    // ── Test 3: SDL contains Query type with singular and plural fields ────────

    #[test]
    fn test_schema_generation_query_type() {
        let mut gql_schema = GraphqlSchema::new();
        gql_schema.add_entity(swap_schema());
        let sdl = gql_schema.sdl();

        assert!(sdl.contains("type Query {"), "SDL missing Query type:\n{sdl}");
        assert!(sdl.contains("swap(id: String!): Swap"), "SDL missing singular swap:\n{sdl}");
        assert!(sdl.contains("swaps("), "SDL missing plural swaps:\n{sdl}");
    }

    // ── Test 4: SDL pascal_case helper ────────────────────────────────────────

    #[test]
    fn test_pascal_case_conversion() {
        assert_eq!(pascal_case("swap"), "Swap");
        assert_eq!(pascal_case("erc20_transfer"), "Erc20Transfer");
        assert_eq!(pascal_case("uniswap_v3_pool"), "UniswapV3Pool");
    }

    // ── Test 5: Introspection returns SDL ─────────────────────────────────────

    #[tokio::test]
    async fn test_introspection() {
        let executor = seeded_executor().await;
        let sdl = executor.introspect();
        assert!(sdl.contains("type Swap {"), "introspect missing Swap type:\n{sdl}");
        assert!(sdl.contains("type Transfer {"), "introspect missing Transfer type:\n{sdl}");
    }

    // ── Test 6: Introspection via __schema query ───────────────────────────────

    #[tokio::test]
    async fn test_introspection_query() {
        let executor = seeded_executor().await;
        let resp = executor.execute("{ __schema { types { name } } }").await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let data = resp.data.unwrap();
        let sdl = data["__schema"].as_str().unwrap();
        assert!(sdl.contains("type Swap {"));
    }

    // ── Test 7: Collection query returns all entities ─────────────────────────

    #[tokio::test]
    async fn test_collection_query_all() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute("{ swaps { id pool amount0 amount1 } }")
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        assert_eq!(arr.len(), 3);
    }

    // ── Test 8: Singular query by id ──────────────────────────────────────────

    #[tokio::test]
    async fn test_singular_query_by_id() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(r#"{ swap(id: "s2") { id pool amount0 } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let row = &resp.data.unwrap()["swap"];
        assert_eq!(row["id"], "s2");
        assert_eq!(row["pool"], "0xPOOL_A");
        assert_eq!(row["amount0"], 2000);
    }

    // ── Test 9: Singular query for unknown id returns null ────────────────────

    #[tokio::test]
    async fn test_singular_query_missing_id() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(r#"{ swap(id: "nonexistent") { id } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        assert_eq!(resp.data.unwrap()["swap"], JsonValue::Null);
    }

    // ── Test 10: Collection query with where filter ────────────────────────────

    #[tokio::test]
    async fn test_collection_with_where_filter() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(r#"{ swaps(where: { pool: "0xPOOL_A" }) { id pool } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        assert_eq!(arr.len(), 2);
        for row in &arr {
            assert_eq!(row["pool"], "0xPOOL_A");
        }
    }

    // ── Test 11: Collection query with first/skip pagination ──────────────────

    #[tokio::test]
    async fn test_collection_pagination() {
        let executor = seeded_executor().await;
        // Sort by amount0 ascending, skip 1, take 1 → should be s2 (2000).
        let resp = executor
            .execute(
                r#"{ swaps(first: 1, skip: 1, orderBy: "amount0", orderDirection: "asc") { id amount0 } }"#,
            )
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["amount0"], 2000);
    }

    // ── Test 12: Collection query with orderBy desc ────────────────────────────

    #[tokio::test]
    async fn test_collection_order_desc() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(
                r#"{ swaps(orderBy: "amount0", orderDirection: "desc") { id amount0 } }"#,
            )
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        assert_eq!(arr.len(), 3);
        let first_amount = arr[0]["amount0"].as_u64().unwrap();
        let last_amount = arr[2]["amount0"].as_u64().unwrap();
        assert!(first_amount > last_amount, "expected descending order");
    }

    // ── Test 13: Unknown entity type returns error ─────────────────────────────

    #[tokio::test]
    async fn test_unknown_entity_returns_error() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute("{ unknownEntity { id } }")
            .await;
        assert!(resp.is_error(), "expected an error for unknown entity");
        let errs = resp.errors.unwrap();
        assert!(
            errs[0].message.contains("Unknown field"),
            "wrong error message: {}",
            errs[0].message
        );
    }

    // ── Test 14: Collection gt filter ─────────────────────────────────────────

    #[tokio::test]
    async fn test_where_gt_filter() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(r#"{ swaps(where: { amount0_gt: 1000 }) { id amount0 } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        // s2 (2000) and s3 (3000) are > 1000.
        assert_eq!(arr.len(), 2);
        for row in &arr {
            assert!(row["amount0"].as_u64().unwrap() > 1000);
        }
    }

    // ── Test 15: Response formatting — ok ─────────────────────────────────────

    #[test]
    fn test_response_ok_format() {
        let resp = GraphqlResponse::ok(serde_json::json!({ "swap": { "id": "s1" } }));
        assert!(!resp.is_error());
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("data").is_some());
        assert!(json.get("errors").is_none());
        assert_eq!(json["data"]["swap"]["id"], "s1");
    }

    // ── Test 16: Response formatting — error ──────────────────────────────────

    #[test]
    fn test_response_error_format() {
        let resp = GraphqlResponse::err("something went wrong");
        assert!(resp.is_error());
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("errors").is_some());
        assert!(json.get("data").is_none());
        assert_eq!(json["errors"][0]["message"], "something went wrong");
    }

    // ── Test 17: Field projection ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_field_projection() {
        let executor = seeded_executor().await;
        // Only request `id` and `pool` — amount0/amount1 should not appear.
        let resp = executor
            .execute(r#"{ swaps(first: 1, orderBy: "amount0", orderDirection: "asc") { id pool } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let row = &resp.data.unwrap()["swaps"][0];
        assert!(row.get("id").is_some());
        assert!(row.get("pool").is_some());
        assert!(row.get("amount0").is_none(), "amount0 should be projected out");
    }

    // ── Test 18: Multiple entity types in one query ───────────────────────────

    #[tokio::test]
    async fn test_multi_entity_query() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute("{ swaps { id } transfers { id } }")
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let data = resp.data.unwrap();
        assert_eq!(data["swaps"].as_array().unwrap().len(), 3);
        assert_eq!(data["transfers"].as_array().unwrap().len(), 2);
    }

    // ── Test 19: Parse error returns error response ───────────────────────────

    #[tokio::test]
    async fn test_parse_error() {
        let executor = seeded_executor().await;
        let resp = executor.execute("{ unclosed { id ").await;
        assert!(resp.is_error(), "expected parse error");
    }

    // ── Test 20: SubscriptionConfig defaults ─────────────────────────────────

    #[test]
    fn test_subscription_config_default() {
        let cfg = SubscriptionConfig::default();
        assert!(cfg.entity_types.is_empty());
        assert_eq!(cfg.buffer_size, 256);
        assert!(cfg.events.contains(&SubscriptionEvent::Insert));
        assert!(cfg.events.contains(&SubscriptionEvent::Reorg));
        assert!(cfg.from_block.is_none());
    }

    // ── Test 21: SubscriptionConfig serializes correctly ─────────────────────

    #[test]
    fn test_subscription_config_serialization() {
        let cfg = SubscriptionConfig {
            entity_types: vec!["swap".to_string()],
            events: vec![SubscriptionEvent::Insert, SubscriptionEvent::Delete],
            from_block: Some(1_000_000),
            buffer_size: 64,
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["entity_types"][0], "swap");
        assert_eq!(json["from_block"], 1_000_000);
        assert_eq!(json["events"][0], "insert");
        assert_eq!(json["events"][1], "delete");
    }

    // ── Test 22: Collection with lte filter ───────────────────────────────────

    #[tokio::test]
    async fn test_where_lte_filter() {
        let executor = seeded_executor().await;
        let resp = executor
            .execute(r#"{ swaps(where: { amount0_lte: 2000 }) { id amount0 } }"#)
            .await;
        assert!(!resp.is_error(), "unexpected error: {:?}", resp.errors);
        let arr = resp.data.unwrap()["swaps"].as_array().unwrap().clone();
        // s1 (1000) and s2 (2000) are <= 2000.
        assert_eq!(arr.len(), 2);
        for row in &arr {
            assert!(row["amount0"].as_u64().unwrap() <= 2000);
        }
    }

    // ── Test 23: Singular query requires id argument ──────────────────────────

    #[tokio::test]
    async fn test_singular_without_id_returns_error() {
        let executor = seeded_executor().await;
        // Singular field name without id argument.
        let resp = executor.execute("{ swap { id pool } }").await;
        // Without an `id` argument, execute_single should error.
        assert!(resp.is_error(), "expected error for singular without id");
    }
}
