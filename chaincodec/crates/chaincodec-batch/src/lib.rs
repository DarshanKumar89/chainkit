//! # chaincodec-batch
//!
//! High-throughput batch decode engine for historical data processing.
//!
//! ## Features
//! - Memory-bounded chunking (default 10,000 events per chunk)
//! - CPU-parallel decoding via Rayon
//! - Progress callbacks (for progress bars / ETAs)
//! - Three error modes: Skip, Collect, Throw
//!
//! ## Usage
//! ```no_run
//! use chaincodec_batch::{BatchEngine, BatchRequest};
//!
//! // let engine = BatchEngine::new(registry, decoder);
//! // let result = engine.decode(request).await?;
//! ```

pub mod engine;
pub mod request;

pub use engine::BatchEngine;
pub use request::BatchRequest;
