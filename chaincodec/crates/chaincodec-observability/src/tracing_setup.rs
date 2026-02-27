//! Tracing / logging initialisation helpers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Log level per component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Global default level: "trace" | "debug" | "info" | "warn" | "error"
    #[serde(default = "default_level")]
    pub level: String,
    /// Override per component: component_name → level
    #[serde(default)]
    pub components: HashMap<String, String>,
    /// Emit JSON structured logs (true) or human-readable text (false)
    #[serde(default)]
    pub json: bool,
}

fn default_level() -> String {
    "info".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            components: HashMap::new(),
            json: false,
        }
    }
}

/// OpenTelemetry tracing configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TracingConfig {
    /// OTLP exporter endpoint (e.g. "http://localhost:4317")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otlp_endpoint: Option<String>,
    /// Service name for OTLP traces
    #[serde(default = "default_service_name")]
    pub service_name: String,
}

fn default_service_name() -> String {
    "chaincodec".into()
}

/// Initialise tracing with the given log config.
/// Should be called once at application startup.
pub fn init_tracing(config: &LogConfig) {
    // Build the directive string: "info,chaincodec_stream=debug" etc.
    let mut directives = config.level.clone();
    for (component, level) in &config.components {
        directives.push_str(&format!(",{}={}", component.replace('-', "_"), level));
    }

    let filter = EnvFilter::try_new(&directives)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    if config.json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .init();
    }
}
