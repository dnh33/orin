//! MCP tool handlers: `find`, `stat`, and `status` over the daemon protocol.

use crate::client;
use orin_core::protocol::HitWire;
use orin_core::protocol::Progress;
use orin_core::protocol::RootWire;
use orin_core::protocol::StatusData;
use orin_core::query::StatInfo;
use rmcp::Json;
use rmcp::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::Tool;
use rmcp::schemars::JsonSchema;
use rmcp::tool;
use rmcp::tool_handler;
use rmcp::tool_router;
use serde::Deserialize;
use serde::Serialize;

/// Default number of paths returned by `find` when the caller omits `limit`.
const DEFAULT_LIMIT: u32 = 20;

/// MCP server exposing the orin index as tools.
#[derive(Debug, Clone)]
pub struct OrinServer {
    tool_router: ToolRouter<Self>,
}

impl Default for OrinServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Arguments for the `find` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindParams {
    /// Search query (query language: ext:rs, size:>10M, path:src, ...).
    pub query: String,
    /// Maximum number of paths to return; defaults to 20.
    pub limit: Option<u32>,
}

/// Arguments for the `stat` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatParams {
    /// Absolute path to inspect.
    pub path: String,
}

/// One matching path from the index.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HitResult {
    /// Absolute path of the match.
    pub path: String,
    /// File name component of the path.
    pub name: String,
    /// Entry kind: file, dir, link, or other.
    #[serde(rename = "type")]
    pub kind: String,
    /// Size in bytes.
    pub size: u64,
    /// Modification time, unix seconds.
    pub mtime: u32,
    /// Relevance score from the index.
    pub score: f32,
}

/// Structured payload returned by the `find` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FindResult {
    /// The query that was run.
    pub query: String,
    /// Matching paths, best match first.
    pub hits: Vec<HitResult>,
}

/// Structured payload returned by the `stat` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatResult {
    /// Path that was inspected.
    pub path: String,
    /// Whether the path exists.
    pub exists: bool,
    /// Entry kind, when present: file, dir, or link.
    pub kind: Option<u8>,
    /// Size in bytes, when present.
    pub size: Option<u64>,
    /// Modification time, unix seconds, when present.
    pub mtime: Option<u32>,
    /// Depth relative to the index root, when present.
    pub depth: Option<u8>,
}

/// One indexed root directory.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RootResult {
    /// Root path.
    pub path: String,
    /// Entries under this root.
    pub entries: u64,
    /// Watch mode: polling, notify, or off.
    pub watch: String,
}

/// Indexing progress while a scan is running.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProgressResult {
    /// Entries scanned so far.
    pub entries: u64,
    /// Milliseconds since the scan started.
    pub since_ms: u64,
}

/// Structured payload returned by the `status` tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusResult {
    /// Daemon version.
    pub version: String,
    /// Protocol version.
    pub protocol: u32,
    /// Daemon state: building, ready, or revalidating.
    pub state: String,
    /// Total indexed entries.
    pub entries: u64,
    /// Resident memory in bytes.
    pub mem_bytes: u64,
    /// Entries the daemon could not read.
    pub unreadable: u64,
    /// Watched root directories.
    pub roots: Vec<RootResult>,
    /// Scan progress, while indexing is running.
    pub progress: Option<ProgressResult>,
}

impl From<HitWire> for HitResult {
    fn from(hit: HitWire) -> Self {
        let kind = match hit.t {
            0 => "file",
            1 => "dir",
            2 => "link",
            _ => "other",
        };
        Self {
            path: hit.p,
            name: hit.n,
            kind: kind.to_string(),
            size: hit.s,
            mtime: hit.m,
            score: hit.score,
        }
    }
}

impl From<StatInfo> for StatResult {
    fn from(info: StatInfo) -> Self {
        Self {
            path: info.path,
            exists: info.exists,
            kind: info.kind,
            size: info.size,
            mtime: info.mtime,
            depth: info.depth,
        }
    }
}

impl From<RootWire> for RootResult {
    fn from(root: RootWire) -> Self {
        Self {
            path: root.path,
            entries: root.entries,
            watch: root.watch,
        }
    }
}

impl From<Progress> for ProgressResult {
    fn from(progress: Progress) -> Self {
        Self {
            entries: progress.entries,
            since_ms: progress.since_ms,
        }
    }
}

impl From<StatusData> for StatusResult {
    fn from(data: StatusData) -> Self {
        Self {
            version: data.version,
            protocol: data.protocol,
            state: data.state,
            entries: data.entries,
            mem_bytes: data.mem_bytes,
            unreadable: data.unreadable,
            roots: data.roots.into_iter().map(RootResult::from).collect(),
            progress: data.progress.map(ProgressResult::from),
        }
    }
}

#[tool_router]
impl OrinServer {
    /// Create the server with the `find`, `stat`, and `status` tools installed.
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Search the index and return matching paths.
    #[tool(
        name = "find",
        description = "Search the index and return matching paths."
    )]
    async fn find(&self, params: Parameters<FindParams>) -> Result<Json<FindResult>, String> {
        let limit = params.0.limit.unwrap_or(DEFAULT_LIMIT);
        let query = params.0.query;
        let search = query.clone();
        let wire = daemon_call(move || client::query(&search, limit)).await?;
        let hits = wire.into_iter().map(HitResult::from).collect();
        Ok(Json(FindResult { query, hits }))
    }

    /// Return exists/kind/size/mtime for a path.
    #[tool(
        name = "stat",
        description = "Return exists/kind/size/mtime for a path."
    )]
    async fn stat(&self, params: Parameters<StatParams>) -> Result<Json<StatResult>, String> {
        let path = params.0.path;
        let info = daemon_call(move || client::stat(&path)).await?;
        Ok(Json(StatResult::from(info)))
    }

    /// Return daemon status: version, entries, roots, progress.
    #[tool(
        name = "status",
        description = "Return daemon status: entries, roots, version."
    )]
    async fn status(&self) -> Result<Json<StatusResult>, String> {
        let data = daemon_call(client::status).await?;
        Ok(Json(StatusResult::from(data)))
    }
}

/// Run one blocking daemon call on the runtime's blocking pool.
async fn daemon_call<T, F>(call: F) -> Result<T, String>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(result) => result.map_err(|err| format!("{err:#}")),
        Err(err) => Err(format!("daemon call failed: {err}")),
    }
}

/// Enumerate the MCP tool definitions served by [`OrinServer`].
pub fn tool_definitions() -> Vec<Tool> {
    OrinServer::tool_router().list_all()
}

#[tool_handler(router = self.tool_router, name = "orin")]
impl ServerHandler for OrinServer {}
