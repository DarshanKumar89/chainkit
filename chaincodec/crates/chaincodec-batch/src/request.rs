//! Batch decode request configuration.

use chaincodec_core::{decoder::ErrorMode, event::RawEvent};

/// Configuration for a batch decode job.
pub struct BatchRequest {
    /// The raw events to decode
    pub logs: Vec<RawEvent>,
    /// Chain slug — determines which decoder to use
    pub chain: String,
    /// Number of parallel Rayon workers (0 = use all available CPUs)
    pub concurrency: usize,
    /// Max events per chunk (memory safety)
    pub chunk_size: usize,
    /// How to handle decode errors
    pub error_mode: ErrorMode,
    /// Optional progress callback
    pub on_progress: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
}

impl BatchRequest {
    pub fn new(chain: impl Into<String>, logs: Vec<RawEvent>) -> Self {
        Self {
            logs,
            chain: chain.into(),
            concurrency: 0,
            chunk_size: 10_000,
            error_mode: ErrorMode::Skip,
            on_progress: None,
        }
    }

    pub fn chunk_size(mut self, n: usize) -> Self {
        self.chunk_size = n;
        self
    }

    pub fn error_mode(mut self, mode: ErrorMode) -> Self {
        self.error_mode = mode;
        self
    }

    pub fn on_progress<F: Fn(usize, usize) + Send + Sync + 'static>(
        mut self,
        f: F,
    ) -> Self {
        self.on_progress = Some(Box::new(f));
        self
    }
}
