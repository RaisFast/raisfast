//! Minimal MCP **client** (stdio transport) — vendored semantics from zeroclaw
//! `zeroclaw-tools/src/{mcp_protocol,mcp_client,mcp_transport}.rs` (MIT/Apache-2.0).
//!
//! Scope: external tool servers reachable by spawning a local process
//! (`command [args...]`), JSON-RPC over newline-delimited stdio. Supports
//! `initialize`/`notifications/initialized`, `tools/list`, `tools/call` with
//! timeouts and strict request/reply correlation. No third-party deps beyond
//! tokio/serde. HTTP/SSE transports and prompts/resources are out of scope.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

pub const JSONRPC_VERSION: &str = "2.0";
/// Protocol version we advertise. Per official rust-sdk this is the current
/// stable spec (2026-07-28); older legacy servers (2025-11-25 and earlier)
/// still answer the classic `initialize` handshake we use. Full discovery
/// lifecycle negotiation is out of scope for this minimal tools client.
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 30;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 16_078_168;

/// Wire transport for a remote MCP server (zeroclaw `McpTransport` shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    #[default]
    Stdio,
    Http,
    Sse,
}

/// Admin-declared connection to an external MCP server. Shape aligned with
/// zeroclaw `McpServerConfig` (transport enum + url/command/args/headers);
/// `description` is our additive field used to explain the server to the agent.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransport,
    pub url: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub headers: std::collections::HashMap<String, String>,
    pub tool_timeout_secs: Option<u64>,
    pub max_response_bytes: Option<usize>,
}

impl<'de> serde::Deserialize<'de> for McpServerConfig {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Raw {
            name: String,
            #[serde(default)]
            transport: Option<McpTransport>,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            args: Option<Vec<String>>,
            #[serde(default)]
            headers: std::collections::HashMap<String, String>,
            #[serde(default)]
            tool_timeout_secs: Option<u64>,
            #[serde(default)]
            max_response_bytes: Option<usize>,
        }
        let r: Raw = <Raw as serde::Deserialize>::deserialize(d)?;
        // Keep ergonomic shorthand configs valid: no explicit transport + a
        // `url` means HTTP; otherwise stdio (like zeroclaw's examples imply).
        let transport = match r.transport {
            Some(t) => t,
            None if r.url.is_some() => McpTransport::Http,
            _ => McpTransport::Stdio,
        };
        Ok(McpServerConfig {
            name: r.name,
            transport,
            url: r.url,
            command: r.command.unwrap_or_default(),
            args: r.args.unwrap_or_default(),
            headers: r.headers,
            tool_timeout_secs: r.tool_timeout_secs,
            max_response_bytes: r.max_response_bytes,
        })
    }
}

// ── JSON-RPC 2.0 protocol types (zeroclaw `mcp_protocol.rs`) ────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: Some(serde_json::Value::Number(id.into())),
            method: method.into(),
            params: Some(params),
        }
    }

    pub fn notification(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: None,
            method: method.into(),
            params: Some(params),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct McpToolsListResult {
    pub tools: Vec<McpToolDef>,
}

// ── stdio session ───────────────────────────────────────────────────────────

/// A connected MCP server session over stdio (owned child).
pub struct McpSession {
    server: String,
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
    timeout: Duration,
    max_response_bytes: usize,
}

impl McpSession {
    /// Spawn the server process, run the MCP handshake and return a session.
    pub async fn connect(cfg: &McpServerConfig) -> Result<Self> {
        let mut child = Command::new(&cfg.command)
            .args(&cfg.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn mcp server '{}'", cfg.name))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("no stdin on mcp server '{}'", cfg.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("no stdout on mcp server '{}'", cfg.name))?;
        let lines = BufReader::new(stdout).lines();

        let mut session = Self {
            server: cfg.name.clone(),
            child,
            stdin,
            lines,
            next_id: 1,
            timeout: Duration::from_secs(
                cfg.tool_timeout_secs.unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS),
            ),
            max_response_bytes: cfg.max_response_bytes.unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
        };
        session
            .handshake()
            .await
            .with_context(|| format!("mcp handshake failed for '{}'", cfg.name))?;
        Ok(session)
    }

    async fn handshake(&mut self) -> Result<()> {
        let id = self.next_request_id();
        let init = JsonRpcRequest::new(
            id,
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "raisfast", "version": env!("CARGO_PKG_VERSION") }
            }),
        );
        let _resp = self.send_and_recv(&init).await?;
        self.write_request(&JsonRpcRequest::notification(
            "notifications/initialized",
            serde_json::json!({}),
        ))
        .await?;
        Ok(())
    }

    /// List all tools advertised by the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        let req = JsonRpcRequest::new(self.next_request_id(), "tools/list", serde_json::json!({}));
        let resp = self.send_and_recv(&req).await?;
        let result = resp
            .result
            .ok_or_else(|| anyhow!("tools/list returned no result"))?;
        let list: McpToolsListResult =
            serde_json::from_value(result).context("parse tools/list result")?;
        Ok(list.tools)
    }

    /// Call a remote tool. `arguments` is the caller-supplied JSON object.
    /// Returns the concatenated text content of the result.
    pub async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> Result<String> {
        let req = JsonRpcRequest::new(
            self.next_request_id(),
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        );
        let resp = self.send_and_recv(&req).await?;
        let result = resp
            .result
            .ok_or_else(|| anyhow!("tools/call returned no result"))?;
        let is_error = result
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let text = content_text(result.get("content")).unwrap_or_default();
        if is_error {
            bail!("mcp tool '{name}' error: {text}");
        }
        if text.is_empty() {
            bail!("mcp tool '{name}' returned empty result");
        }
        Ok(text)
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Write a request/notification frame without awaiting a reply.
    async fn write_request(&mut self, req: &JsonRpcRequest) -> Result<()> {
        let wire = serde_json::to_string(req)?;
        self.stdin.write_all(wire.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Write one request, read lines until a response with a matching id
    /// arrives. Must only be used for id-bearing method calls.
    async fn send_and_recv(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        self.write_request(req).await?;
        let wanted = req.id.clone();
        let fut = async {
            loop {
                let line = self
                    .lines
                    .next_line()
                    .await?
                    .ok_or_else(|| anyhow!("mcp server '{}' closed stdout", self.server))?;
                if line.trim().is_empty() {
                    continue;
                }
                if line.len() > self.max_response_bytes {
                    bail!(
                        "mcp server '{}' response exceeded {} bytes",
                        self.server,
                        self.max_response_bytes
                    );
                }
                let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) else {
                    continue;
                };
                if resp.id != wanted {
                    continue;
                }
                if let Some(err) = resp.error {
                    bail!("mcp '{}' error {}: {}", self.server, err.code, err.message);
                }
                return Ok::<_, anyhow::Error>(resp);
            }
        };
        match timeout(self.timeout, fut).await {
            Ok(r) => r,
            Err(_) => bail!("mcp server '{}' timed out", self.server),
        }
    }
}

/// Concatenate `text` blocks of an MCP `content` array into one string.
fn content_text(content: Option<&serde_json::Value>) -> Option<String> {
    let arr = content?.as_array()?;
    let mut out = String::new();
    for block in arr {
        if block.get("type").and_then(serde_json::Value::as_str) == Some("text")
            && let Some(t) = block.get("text").and_then(serde_json::Value::as_str)
        {
            out.push_str(t);
            out.push('\n');
        }
    }
    let out = out.trim_end().to_string();
    (!out.is_empty()).then_some(out)
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

// ── streamable HTTP client (stateless JSON mode) ────────────────────────────
//
// Sends each request as a POST whose JSON body carries the JSON-RPC message.
// v1 targets servers that accept a plain `application/json` response (no SSE
// streaming); if the server answers with `Mcp-Session-Id` we keep and echo it.

use std::collections::HashMap;

pub struct McpHttpSession {
    client: reqwest::Client,
    url: String,
    name: String,
    headers: HashMap<String, String>,
    session_id: Option<String>,
    next_id: u64,
    timeout_secs: u64,
    max_response_bytes: usize,
}

impl McpHttpSession {
    pub fn new(cfg: &McpServerConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: cfg.url.clone().unwrap_or_default(),
            name: cfg.name.clone(),
            headers: cfg.headers.clone(),
            session_id: None,
            next_id: 1,
            timeout_secs: cfg.tool_timeout_secs.unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS),
            max_response_bytes: cfg.max_response_bytes.unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
        }
    }

    async fn post(&mut self, req: &JsonRpcRequest) -> Result<JsonRpcResponse> {
        let mut builder = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(req);
        if let Some(sid) = &self.session_id {
            builder = builder.header("mcp-session-id", sid);
        }
        for (k, v) in &self.headers {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_bytes()) {
                builder = builder.header(name, v);
            }
        }
        let resp = timeout(
            Duration::from_secs(self.timeout_secs.max(1)),
            builder.send(),
        )
        .await
        .map_err(|_| anyhow!("mcp http '{name}' request timed out", name = self.name))?
        .context("mcp http send")?;
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            self.session_id = Some(sid.to_string());
        }
        let status = resp.status();
        let mut bytes: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("mcp http read chunk")?;
            if bytes.len().saturating_add(chunk.len()) > self.max_response_bytes {
                bail!(
                    "mcp http '{}' response exceeded {} bytes",
                    self.name,
                    self.max_response_bytes
                );
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = String::from_utf8_lossy(&bytes);
        let parsed: Result<JsonRpcResponse, _> = serde_json::from_str(&body);
        match parsed {
            Ok(resp) => {
                if let Some(err) = resp.error {
                    bail!(
                        "mcp http '{name}' error {code}: {msg}",
                        name = self.name,
                        code = err.code,
                        msg = err.message
                    );
                }
                Ok(resp)
            }
            Err(_) => bail!(
                "mcp http '{name}' status {status}: {body}",
                name = self.name
            ),
        }
    }

    /// `initialize` and echo the negotiated server protocol version.
    pub async fn initialize(&mut self) -> Result<()> {
        let req = JsonRpcRequest::new(
            self.next_id(),
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "raisfast", "version": env!("CARGO_PKG_VERSION") }
            }),
        );
        let resp = self.post(&req).await?;
        if resp.result.is_none() {
            bail!("mcp http '{}' initialize missing result", self.name);
        }
        Ok(())
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        let req = JsonRpcRequest::new(self.next_id(), "tools/list", serde_json::json!({}));
        let resp = self.post(&req).await?;
        let result = resp
            .result
            .ok_or_else(|| anyhow!("tools/list returned no result"))?;
        let list: McpToolsListResult =
            serde_json::from_value(result).context("parse tools/list result")?;
        Ok(list.tools)
    }

    pub async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> Result<String> {
        let req = JsonRpcRequest::new(
            self.next_id(),
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        );
        let resp = self.post(&req).await?;
        let result = resp
            .result
            .ok_or_else(|| anyhow!("tools/call returned no result"))?;
        let is_error = result
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let text = content_text(result.get("content")).unwrap_or_default();
        if is_error {
            bail!("mcp tool '{name}' error: {text}");
        }
        if text.is_empty() {
            bail!("mcp tool '{name}' returned empty result");
        }
        Ok(text)
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn request_serializes_with_id_and_notification_omits_id() {
        let req = JsonRpcRequest::new(1, "tools/list", serde_json::json!({}));
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"id\":1"));
        let notif =
            JsonRpcRequest::notification("notifications/initialized", serde_json::json!({}));
        assert!(!serde_json::to_string(&notif).unwrap().contains("\"id\""));
    }

    #[test]
    fn tool_def_and_list_result_deserialize() {
        let json =
            r#"{"tools":[{"name":"add","description":"add","inputSchema":{"type":"object"}}]}"#;
        let list: McpToolsListResult = serde_json::from_str(json).unwrap();
        assert_eq!(list.tools.len(), 1);
        assert_eq!(list.tools[0].name, "add");
    }

    #[test]
    fn content_text_joins_text_blocks() {
        let v = serde_json::json!([{"type":"text","text":"a"},{"type":"image","data":"x"},{"type":"text","text":"b"}]);
        assert_eq!(content_text(Some(&v)).unwrap(), "a\nb");
        assert!(content_text(Some(&serde_json::json!([]))).is_none());
    }

    #[tokio::test]
    async fn http_session_against_bun_hono_fixture() {
        use std::process::Stdio;
        // Skip when bun isn't installed (CI may not have it).
        let check = tokio::process::Command::new("bun")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut check) = check else {
            println!("bun not installed; skipping");
            return;
        };
        let _ = check.wait().await;

        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent());
        let script = root
            .map(|r| r.join("scripts/agents/mcp_server/mcp_http_server.ts"))
            .expect("workspace root");
        assert!(
            script.exists(),
            "http fixture missing: {}",
            script.display()
        );
        let mut server = tokio::process::Command::new("bun")
            .arg("run")
            .arg(&script)
            .env("PORT", "9897")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn bun fixture");
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let cfg: McpServerConfig = serde_json::from_value(serde_json::json!({
            "name": "bun-echo",
            "url": "http://127.0.0.1:9897",
        }))
        .expect("parse cfg");
        let mut session = McpHttpSession::new(&cfg);
        session.initialize().await.expect("http initialize");
        let tools = session.list_tools().await.expect("http list_tools");
        assert_eq!(tools.len(), 1);
        let out = session
            .call_tool("echo", serde_json::json!({ "msg": "hi-from-http" }))
            .await
            .expect("http call_tool");
        assert_eq!(out, "echo:hi-from-http");
        let _ = server.kill().await;
    }

    #[tokio::test]
    async fn stdio_session_lists_and_calls_local_echo_server() {
        // Deterministic offline check: spawn the vendored echo server fixture
        // (python3) and verify discover + call through the real stdio path.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent());
        let script = root
            .map(|r| r.join("scripts/agents/mcp_echo_server.py"))
            .expect("workspace root");
        assert!(
            script.exists(),
            "echo fixture missing: {}",
            script.display()
        );
        let mut session = McpSession::connect(&McpServerConfig {
            name: "echo".into(),
            transport: McpTransport::Stdio,
            url: None,
            command: "python3".into(),
            args: vec![script.to_string_lossy().into_owned()],
            headers: Default::default(),
            tool_timeout_secs: None,
            max_response_bytes: None,
        })
        .await
        .expect("connect to echo server");

        let tools = session.list_tools().await.expect("list_tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        let out = session
            .call_tool("echo", serde_json::json!({ "msg": "hello-mcp" }))
            .await
            .expect("call_tool");
        assert_eq!(out, "echo:hello-mcp", "output: {out}");
    }
}
