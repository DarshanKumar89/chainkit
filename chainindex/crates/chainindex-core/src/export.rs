//! Data export — write indexed events to JSONL or CSV files for analytics.
//!
//! Supports exporting decoded events with filtering by block range, schema,
//! and address. Designed for feeding data to DuckDB, BigQuery, or Spark.
//!
//! # Supported Formats
//!
//! - **JSONL** (newline-delimited JSON) — one JSON object per line
//! - **CSV** — comma-separated with header row
//!
//! # Example
//!
//! ```rust
//! use chainindex_core::export::{ExportConfig, ExportFormat, export_events};
//! use chainindex_core::handler::DecodedEvent;
//!
//! let events: Vec<DecodedEvent> = vec![];
//! let config = ExportConfig {
//!     format: ExportFormat::Jsonl,
//!     ..Default::default()
//! };
//!
//! let mut buf = Vec::new();
//! let stats = export_events(&events, &config, &mut buf).unwrap();
//! ```

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::error::IndexerError;
use crate::handler::DecodedEvent;

// ─── ExportFormat ───────────────────────────────────────────────────────────

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// Newline-delimited JSON (one JSON object per line).
    Jsonl,
    /// Comma-separated values with header row.
    Csv,
}

// ─── ExportConfig ───────────────────────────────────────────────────────────

/// Configuration for a data export operation.
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// Output format.
    pub format: ExportFormat,
    /// Only export events from blocks >= this number.
    pub from_block: Option<u64>,
    /// Only export events from blocks <= this number.
    pub to_block: Option<u64>,
    /// Only export events matching these schema names (empty = all).
    pub schema_filter: Vec<String>,
    /// Only export events from these addresses (empty = all).
    pub address_filter: Vec<String>,
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            format: ExportFormat::Jsonl,
            from_block: None,
            to_block: None,
            schema_filter: Vec::new(),
            address_filter: Vec::new(),
        }
    }
}

// ─── ExportStats ────────────────────────────────────────────────────────────

/// Statistics from a completed export operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportStats {
    /// Number of events written.
    pub events_exported: u64,
    /// Total bytes written.
    pub bytes_written: u64,
    /// Number of events skipped by filters.
    pub events_skipped: u64,
}

// ─── Exporter Trait ─────────────────────────────────────────────────────────

/// Trait for format-specific exporters.
pub trait Exporter {
    /// Write a single event.
    fn write_event(&mut self, event: &DecodedEvent) -> Result<(), IndexerError>;
    /// Finalize the export and return bytes written.
    fn finish(&mut self) -> Result<u64, IndexerError>;
}

// ─── JsonlExporter ──────────────────────────────────────────────────────────

/// Writes events as newline-delimited JSON (JSONL).
pub struct JsonlExporter<W: Write> {
    writer: W,
    bytes_written: u64,
}

impl<W: Write> JsonlExporter<W> {
    /// Create a new JSONL exporter writing to the given writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            bytes_written: 0,
        }
    }
}

impl<W: Write> Exporter for JsonlExporter<W> {
    fn write_event(&mut self, event: &DecodedEvent) -> Result<(), IndexerError> {
        let json = serde_json::to_string(event)
            .map_err(|e| IndexerError::Other(format!("JSON serialization error: {e}")))?;
        let line = format!("{json}\n");
        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| IndexerError::Other(format!("Write error: {e}")))?;
        self.bytes_written += line.len() as u64;
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, IndexerError> {
        self.writer
            .flush()
            .map_err(|e| IndexerError::Other(format!("Flush error: {e}")))?;
        Ok(self.bytes_written)
    }
}

// ─── CsvExporter ────────────────────────────────────────────────────────────

/// Writes events as CSV with a header row.
///
/// Columns: chain, schema, address, tx_hash, block_number, log_index, fields_json
pub struct CsvExporter<W: Write> {
    writer: W,
    bytes_written: u64,
    header_written: bool,
}

impl<W: Write> CsvExporter<W> {
    /// Create a new CSV exporter writing to the given writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            bytes_written: 0,
            header_written: false,
        }
    }

    fn write_header(&mut self) -> Result<(), IndexerError> {
        if !self.header_written {
            let header = "chain,schema,address,tx_hash,block_number,log_index,fields_json\n";
            self.writer
                .write_all(header.as_bytes())
                .map_err(|e| IndexerError::Other(format!("Write error: {e}")))?;
            self.bytes_written += header.len() as u64;
            self.header_written = true;
        }
        Ok(())
    }
}

impl<W: Write> Exporter for CsvExporter<W> {
    fn write_event(&mut self, event: &DecodedEvent) -> Result<(), IndexerError> {
        self.write_header()?;

        let fields_json = serde_json::to_string(&event.fields_json)
            .map_err(|e| IndexerError::Other(format!("JSON error: {e}")))?;

        // Escape CSV fields that contain commas or quotes
        let line = format!(
            "{},{},{},{},{},{},\"{}\"\n",
            csv_escape(&event.chain),
            csv_escape(&event.schema),
            csv_escape(&event.address),
            csv_escape(&event.tx_hash),
            event.block_number,
            event.log_index,
            fields_json.replace('"', "\"\""),
        );
        self.writer
            .write_all(line.as_bytes())
            .map_err(|e| IndexerError::Other(format!("Write error: {e}")))?;
        self.bytes_written += line.len() as u64;
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, IndexerError> {
        self.writer
            .flush()
            .map_err(|e| IndexerError::Other(format!("Flush error: {e}")))?;
        Ok(self.bytes_written)
    }
}

/// Escape a CSV field value (wrap in quotes if it contains comma, quote, or newline).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ─── Filter + Export ────────────────────────────────────────────────────────

/// Check if an event passes the export filters.
fn passes_filter(event: &DecodedEvent, config: &ExportConfig) -> bool {
    // Block range filter
    if let Some(from) = config.from_block {
        if event.block_number < from {
            return false;
        }
    }
    if let Some(to) = config.to_block {
        if event.block_number > to {
            return false;
        }
    }
    // Schema filter
    if !config.schema_filter.is_empty()
        && !config
            .schema_filter
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&event.schema))
    {
        return false;
    }
    // Address filter
    if !config.address_filter.is_empty()
        && !config
            .address_filter
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&event.address))
    {
        return false;
    }
    true
}

/// Export a slice of events to a writer using the given config.
///
/// Returns statistics about the export operation.
pub fn export_events<W: Write>(
    events: &[DecodedEvent],
    config: &ExportConfig,
    writer: W,
) -> Result<ExportStats, IndexerError> {
    let mut stats = ExportStats::default();

    match config.format {
        ExportFormat::Jsonl => {
            let mut exporter = JsonlExporter::new(writer);
            for event in events {
                if passes_filter(event, config) {
                    exporter.write_event(event)?;
                    stats.events_exported += 1;
                } else {
                    stats.events_skipped += 1;
                }
            }
            stats.bytes_written = exporter.finish()?;
        }
        ExportFormat::Csv => {
            let mut exporter = CsvExporter::new(writer);
            for event in events {
                if passes_filter(event, config) {
                    exporter.write_event(event)?;
                    stats.events_exported += 1;
                } else {
                    stats.events_skipped += 1;
                }
            }
            stats.bytes_written = exporter.finish()?;
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(schema: &str, address: &str, block: u64) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: schema.into(),
            address: address.into(),
            tx_hash: format!("0xtx_{block}"),
            block_number: block,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 100}),
        }
    }

    fn test_events() -> Vec<DecodedEvent> {
        vec![
            make_event("Transfer", "0xToken1", 100),
            make_event("Approval", "0xToken1", 101),
            make_event("Transfer", "0xToken2", 102),
            make_event("Swap", "0xPool1", 103),
            make_event("Transfer", "0xToken1", 200),
        ]
    }

    #[test]
    fn jsonl_export_single_event() {
        let events = vec![make_event("Transfer", "0xToken", 100)];
        let mut buf = Vec::new();
        let config = ExportConfig::default();

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 1);
        assert!(stats.bytes_written > 0);

        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 1);

        // Verify it's valid JSON
        let _: DecodedEvent = serde_json::from_str(lines[0]).unwrap();
    }

    #[test]
    fn jsonl_export_multiple_events() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig::default();

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 5);

        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn csv_export_with_header() {
        let events = vec![make_event("Transfer", "0xToken", 100)];
        let mut buf = Vec::new();
        let config = ExportConfig {
            format: ExportFormat::Csv,
            ..Default::default()
        };

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 1);

        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 2); // header + 1 data row
        assert!(lines[0].starts_with("chain,schema,address"));
        assert!(lines[1].starts_with("ethereum,Transfer"));
    }

    #[test]
    fn block_range_filter() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig {
            from_block: Some(101),
            to_block: Some(103),
            ..Default::default()
        };

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 3); // blocks 101, 102, 103
        assert_eq!(stats.events_skipped, 2); // blocks 100, 200
    }

    #[test]
    fn schema_filter() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig {
            schema_filter: vec!["Transfer".into()],
            ..Default::default()
        };

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 3); // 3 Transfer events
        assert_eq!(stats.events_skipped, 2); // Approval + Swap
    }

    #[test]
    fn address_filter() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig {
            address_filter: vec!["0xToken1".into()],
            ..Default::default()
        };

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 3); // 3 events from 0xToken1
        assert_eq!(stats.events_skipped, 2);
    }

    #[test]
    fn combined_filters() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig {
            schema_filter: vec!["Transfer".into()],
            address_filter: vec!["0xToken1".into()],
            from_block: Some(100),
            to_block: Some(150),
            ..Default::default()
        };

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 1); // only Transfer at 0xToken1 block 100
    }

    #[test]
    fn empty_export() {
        let events: Vec<DecodedEvent> = vec![];
        let mut buf = Vec::new();
        let config = ExportConfig::default();

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 0);
        assert_eq!(stats.bytes_written, 0);
    }

    #[test]
    fn export_stats_accurate() {
        let events = test_events();
        let mut buf = Vec::new();
        let config = ExportConfig::default();

        let stats = export_events(&events, &config, &mut buf).unwrap();
        assert_eq!(stats.events_exported, 5);
        assert_eq!(stats.events_skipped, 0);
        assert_eq!(stats.bytes_written, buf.len() as u64);
    }
}
