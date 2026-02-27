//! JSON-RPC 2.0 wire types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC request ID — string, number, or null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Number(u64),
    String(String),
    Null,
}

impl RpcId {
    pub fn number(n: u64) -> Self {
        Self::Number(n)
    }
}

impl std::fmt::Display for RpcId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s}"),
            Self::Null => write!(f, "null"),
        }
    }
}

/// A single JSON-RPC parameter value.
pub type RpcParam = Value;

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Vec<RpcParam>,
    pub id: RpcId,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request.
    pub fn new(id: u64, method: impl Into<String>, params: Vec<RpcParam>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: RpcId::Number(id),
        }
    }
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RpcId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Returns `true` if this is a successful response (has result, no error).
    pub fn is_ok(&self) -> bool {
        self.error.is_none() && self.result.is_some()
    }

    /// Unwrap the result value or return an error.
    pub fn into_result(self) -> Result<Value, JsonRpcError> {
        if let Some(err) = self.error {
            Err(err)
        } else {
            Ok(self.result.unwrap_or(Value::Null))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization() {
        let req = JsonRpcRequest::new(
            1,
            "eth_blockNumber",
            vec![],
        );
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"eth_blockNumber\""));
    }

    #[test]
    fn response_into_result_ok() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: RpcId::Number(1),
            result: Some(Value::String("0x12345".into())),
            error: None,
        };
        assert!(resp.is_ok());
        let val = resp.into_result().unwrap();
        assert_eq!(val, Value::String("0x12345".into()));
    }

    #[test]
    fn response_into_result_error() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: RpcId::Number(1),
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: "execution reverted".into(),
                data: None,
            }),
        };
        assert!(!resp.is_ok());
        let err = resp.into_result().unwrap_err();
        assert_eq!(err.code, -32000);
    }
}
