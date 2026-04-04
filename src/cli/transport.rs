use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;

use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Send a single JSON-RPC request and read the response.
pub fn send_request(stream: &mut TcpStream, request: &JsonRpcRequest) -> Result<serde_json::Value> {
    let json = serde_json::to_string(request)?;
    writeln!(stream, "{}", json)?;
    stream.flush()?;

    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response: JsonRpcResponse = serde_json::from_str(trimmed)?;

        if let Some(error) = response.error {
            anyhow::bail!("Error ({}): {}", error.code, error.message);
        }

        return Ok(response.result.unwrap_or(serde_json::Value::Null));
    }
}

/// Build a JSON-RPC request from method and params.
pub fn make_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
    }
}
