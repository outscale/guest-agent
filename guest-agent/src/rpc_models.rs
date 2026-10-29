use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSONRPC request object.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    /// The name of the RPC call.
    pub method: String,
    /// Parameters to the RPC call.
    pub params: Option<Value>,
    /// Identifier for this request; absent means a notification (no response).
    pub id: Option<serde_json::Value>,
    /// jsonrpc field, MUST be "2.0".
    pub jsonrpc: Option<String>,
}

/// A JSONRPC response object.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    /// A result if there is one, or [`None`].
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Box<Value>>,
    /// An error if there is one, or [`None`].
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
    /// Identifier for this response, which should match that of the request.
    id: serde_json::Value,
    /// jsonrpc field, MUST be "2.0".
    jsonrpc: Option<&'static str>,
}

impl Response {
    pub fn error(id: serde_json::Value, err: RpcError) -> Self {
        Self {
            result: None,
            error: Some(err),
            id,
            jsonrpc: Some("2.0"),
        }
    }

    pub fn result(id: serde_json::Value, result: Box<Value>) -> Self {
        Self {
            result: Some(result),
            error: None,
            id,
            jsonrpc: Some("2.0"),
        }
    }
}

/// A JSONRPC error object
#[derive(Clone, Debug, Serialize)]
pub struct RpcError {
    /// The integer identifier of the error
    pub code: i32,
    /// A string describing the error
    pub message: &'static str,
    /// Additional data specific to the error
    pub data: Option<Box<Value>>,
}

impl From<String> for RpcError {
    fn from(msg: String) -> Self {
        Self::server_error(Some(msg))
    }
}

impl RpcError {
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: "Parse error",
            data: None,
        }
    }

    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "Method not found",
            data: None,
        }
    }

    pub fn invalid_params() -> Self {
        Self {
            code: -32602,
            message: "Invalid params",
            data: None,
        }
    }

    pub fn internal_error() -> Self {
        Self {
            code: -32603,
            message: "Internal error",
            data: None,
        }
    }

    pub fn server_error(data: Option<String>) -> Self {
        Self {
            code: -32000,
            message: "Server error",
            data: data.map(|s| Box::new(Value::String(s))),
        }
    }
}

pub type Result<T> = std::result::Result<T, RpcError>;
