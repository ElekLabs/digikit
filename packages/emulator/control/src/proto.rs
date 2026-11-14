//! The control protocol: JSON-RPC 2.0 messages, error codes and parameter
//! access. No I/O and no threads, so it builds for any host (a browser worker
//! feeds it the same objects over `postMessage`). See
//! `docs/design/emulator-control.md`.

use serde_json::{Value, json};

/// Bumped on an incompatible change; additive changes keep it.
pub const SCHEMA_VERSION: u32 = 1;

/// Error codes (JSON-RPC `code`).
pub mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const BAD_PARAMS: i64 = -32602;
    pub const NO_FIRMWARE: i64 = 1;
    pub const STALE_SESSION: i64 = 2;
    pub const BUSY: i64 = 3;
    pub const FIRMWARE_REJECTED: i64 = 4;
    pub const AUDIO_UNAVAILABLE: i64 = 5;
    pub const GUEST_FAULT: i64 = 6;
    pub const DSP_HALTED: i64 = 7;
    pub const IO: i64 = 8;
    pub const TIMEOUT: i64 = 9;
    /// A long job was cancelled (`job.cancel`).
    pub const CANCELLED: i64 = 10;
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn bad_params(message: impl Into<String>) -> Self {
        Self::new(code::BAD_PARAMS, message)
    }
    pub fn no_firmware() -> Self {
        Self::new(code::NO_FIRMWARE, "No firmware selected")
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(code::IO, message)
    }
    /// The error for a string from the emulator or the desktop commands.
    pub fn from_message(message: String) -> Self {
        match message.as_str() {
            "No firmware selected" | "No startup firmware selected" => Self::no_firmware(),
            "stale emulator session" => Self::new(code::STALE_SESSION, message),
            _ => Self::new(code::GUEST_FAULT, message),
        }
    }
    pub fn to_json(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(data) = &self.data {
            error["data"] = data.clone();
        }
        error
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code {})", self.message, self.code)
    }
}

impl From<RpcError> for String {
    fn from(error: RpcError) -> String {
        error.message
    }
}

pub type RpcResult = Result<Value, RpcError>;

/// A parsed request. A request without an `id` is a notification: it gets
/// no response.
#[derive(Clone, Debug)]
pub struct Request {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

/// Parse one JSON-RPC request object. On failure the error response to send.
pub fn parse_request(text: &str) -> Result<Request, Value> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| error_response(Value::Null, &RpcError::new(code::PARSE, e.to_string())))?;
    request_from_value(&value)
}

pub fn request_from_value(value: &Value) -> Result<Request, Value> {
    let id = value.get("id").cloned();
    let invalid = |message: &str| {
        error_response(
            id.clone().unwrap_or(Value::Null),
            &RpcError::new(code::INVALID_REQUEST, message),
        )
    };
    if !value.is_object() {
        return Err(invalid("a request is a JSON object"));
    }
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing method"))?
        .to_string();
    let params = match value.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(p) if p.is_object() => p.clone(),
        Some(_) => return Err(invalid("params must be an object")),
    };
    Ok(Request { id, method, params })
}

pub fn ok_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error_response(id: Value, error: &RpcError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": error.to_json()})
}

/// An event notification.
pub fn event_notification(params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "event", "params": params})
}

/// Typed access to a request's `params` object.
pub struct Params<'a>(pub &'a Value);

impl<'a> Params<'a> {
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key).filter(|v| !v.is_null())
    }
    pub fn opt_str(&self, key: &str) -> Result<Option<&'a str>, RpcError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .map(Some)
                .ok_or_else(|| RpcError::bad_params(format!("{key} must be a string"))),
        }
    }
    pub fn str(&self, key: &str) -> Result<&'a str, RpcError> {
        self.opt_str(key)?
            .ok_or_else(|| RpcError::bad_params(format!("missing {key}")))
    }
    pub fn opt_u64(&self, key: &str) -> Result<Option<u64>, RpcError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| {
                RpcError::bad_params(format!("{key} must be a non-negative integer"))
            }),
        }
    }
    pub fn u64(&self, key: &str) -> Result<u64, RpcError> {
        self.opt_u64(key)?
            .ok_or_else(|| RpcError::bad_params(format!("missing {key}")))
    }
    pub fn opt_i64(&self, key: &str) -> Result<Option<i64>, RpcError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_i64()
                .map(Some)
                .ok_or_else(|| RpcError::bad_params(format!("{key} must be an integer"))),
        }
    }
    pub fn opt_f64(&self, key: &str) -> Result<Option<f64>, RpcError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_f64()
                .map(Some)
                .ok_or_else(|| RpcError::bad_params(format!("{key} must be a number"))),
        }
    }
    pub fn opt_bool(&self, key: &str) -> Result<Option<bool>, RpcError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_bool()
                .map(Some)
                .ok_or_else(|| RpcError::bad_params(format!("{key} must be a boolean"))),
        }
    }
    pub fn u8(&self, key: &str) -> Result<u8, RpcError> {
        u8::try_from(self.u64(key)?).map_err(|_| RpcError::bad_params(format!("{key} over 255")))
    }
}

/// Standard base64 (RFC 4648, padded) for binary results.
pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }

    #[test]
    fn requests_parse_and_bad_ones_get_error_responses() {
        let r = parse_request(r#"{"jsonrpc":"2.0","id":7,"method":"status"}"#).unwrap();
        assert_eq!(r.id, Some(json!(7)));
        assert_eq!(r.method, "status");
        assert_eq!(r.params, json!({}));
        let e = parse_request("{nope").unwrap_err();
        assert_eq!(e["error"]["code"], code::PARSE);
        let e = parse_request(r#"{"id":1,"params":{}}"#).unwrap_err();
        assert_eq!(e["error"]["code"], code::INVALID_REQUEST);
        assert_eq!(e["id"], 1);
        let e = parse_request(r#"{"id":1,"method":"x","params":[1]}"#).unwrap_err();
        assert_eq!(e["error"]["code"], code::INVALID_REQUEST);
    }

    #[test]
    fn params_report_the_missing_or_mistyped_key() {
        let v = json!({"a": 5, "s": "x", "n": null});
        let p = Params(&v);
        assert_eq!(p.u64("a").unwrap(), 5);
        assert_eq!(p.str("s").unwrap(), "x");
        assert!(p.opt_u64("n").unwrap().is_none());
        assert_eq!(p.u64("missing").unwrap_err().code, code::BAD_PARAMS);
        assert_eq!(p.u64("s").unwrap_err().code, code::BAD_PARAMS);
        assert_eq!(p.u8("a").unwrap(), 5);
    }
}
