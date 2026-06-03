use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;

use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A reusable IPC connection that keeps a single BufReader across multiple requests.
pub struct IpcConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl IpcConnection {
    pub fn new(stream: TcpStream) -> Result<Self> {
        let writer = stream.try_clone()?;
        let reader = BufReader::new(stream);
        Ok(Self { writer, reader })
    }

    /// Send a JSON-RPC request and read the response.
    pub fn send(&mut self, request: &JsonRpcRequest) -> Result<serde_json::Value> {
        let json = serde_json::to_string(request)?;
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()?;

        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line)?;
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
}
