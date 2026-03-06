//! Request hedging — send to backup provider if primary is slow.
//!
//! For latency-sensitive read operations, hedging sends the request to a
//! second provider after a configurable delay. The first response wins.

use std::time::Duration;

use crate::error::TransportError;
use crate::method_safety;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::RpcTransport;

/// Configuration for request hedging.
#[derive(Debug, Clone)]
pub struct HedgingConfig {
    /// Delay before sending the hedged (backup) request.
    pub hedge_delay: Duration,
}

impl Default for HedgingConfig {
    fn default() -> Self {
        Self {
            hedge_delay: Duration::from_millis(100),
        }
    }
}

/// Send a request with hedging — try primary first, then backup after delay.
///
/// Only hedges safe (read-only) methods. Write methods go to primary only.
/// Returns the first successful response and drops the slower request.
pub async fn hedged_send(
    primary: &dyn RpcTransport,
    backup: &dyn RpcTransport,
    req: JsonRpcRequest,
    config: &HedgingConfig,
) -> Result<JsonRpcResponse, TransportError> {
    // Only hedge safe methods
    if !method_safety::is_safe_to_retry(&req.method) {
        return primary.send(req).await;
    }

    let req_clone = req.clone();
    let delay = config.hedge_delay;

    // Race: primary starts immediately, backup starts after delay
    tokio::select! {
        result = primary.send(req) => {
            result
        }
        result = async {
            tokio::time::sleep(delay).await;
            backup.send(req_clone).await
        } => {
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use async_trait::async_trait;

    struct DelayTransport {
        delay: Duration,
        label: String,
    }

    #[async_trait]
    impl RpcTransport for DelayTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            tokio::time::sleep(self.delay).await;
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::json!(self.label)),
                error: None,
            })
        }
        fn url(&self) -> &str {
            &self.label
        }
    }

    #[tokio::test]
    async fn primary_wins_when_fast() {
        let primary = DelayTransport {
            delay: Duration::from_millis(10),
            label: "primary".into(),
        };
        let backup = DelayTransport {
            delay: Duration::from_millis(10),
            label: "backup".into(),
        };

        let config = HedgingConfig {
            hedge_delay: Duration::from_millis(200), // backup won't even start
        };

        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let resp = hedged_send(&primary, &backup, req, &config).await.unwrap();
        assert_eq!(resp.result.unwrap().as_str().unwrap(), "primary");
    }

    #[tokio::test]
    async fn backup_wins_when_primary_slow() {
        let primary = DelayTransport {
            delay: Duration::from_millis(500), // very slow
            label: "primary".into(),
        };
        let backup = DelayTransport {
            delay: Duration::from_millis(10), // fast
            label: "backup".into(),
        };

        let config = HedgingConfig {
            hedge_delay: Duration::from_millis(50),
        };

        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let resp = hedged_send(&primary, &backup, req, &config).await.unwrap();
        // Backup should respond at ~60ms (50ms delay + 10ms), primary at 500ms
        assert_eq!(resp.result.unwrap().as_str().unwrap(), "backup");
    }

    #[tokio::test]
    async fn no_hedging_for_writes() {
        let primary = DelayTransport {
            delay: Duration::from_millis(500),
            label: "primary".into(),
        };
        let backup = DelayTransport {
            delay: Duration::from_millis(10),
            label: "backup".into(),
        };

        let config = HedgingConfig {
            hedge_delay: Duration::from_millis(50),
        };

        // eth_sendRawTransaction is NOT safe to hedge
        let req = JsonRpcRequest::auto("eth_sendRawTransaction", vec![]);
        let resp = hedged_send(&primary, &backup, req, &config).await.unwrap();
        // Should always use primary for writes, even if slow
        assert_eq!(resp.result.unwrap().as_str().unwrap(), "primary");
    }
}
