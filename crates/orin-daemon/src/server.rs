//! IPC server: accepts connections, reads frames, dispatches to handlers, writes responses.

use anyhow::Result;
use interprocess::local_socket::{ListenerNonblockingMode, Stream, prelude::*};
use orin_core::protocol::{
    PROTOCOL_VERSION, Progress, Request, Response, RootWire, StatusData, read_frame, write_frame,
};
use std::io::{BufReader, BufWriter, Write};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::{debug, error, info};

use crate::state::{ScanProgress, SharedState};

/// Main server loop.
pub struct Server {
    state: SharedState,
    idle_exit_secs: u64,
    last_activity: Instant,
}

impl Server {
    /// Create a new server with the given shared state.
    pub fn new(state: crate::state::SharedState, idle_exit_secs: u64) -> Result<Self> {
        Ok(Self {
            state,
            idle_exit_secs,
            last_activity: Instant::now(),
        })
    }

    /// Run the server until shutdown is signaled.
    pub fn run(&mut self, shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
        let listener = {
            let mut state = self.state.lock().unwrap();
            state.listener.take().expect("listener not initialized")
        };

        info!("server listening on {:?}", listener);

        // Non-blocking accept so we can check shutdown; accepted streams stay
        // BLOCKING so request reads wait for the client instead of racing it.
        listener.set_nonblocking(ListenerNonblockingMode::Accept)?;

        loop {
            if shutdown.load(Ordering::SeqCst) {
                info!("shutdown signaled");
                break;
            }

            // Check idle exit
            if self.idle_exit_secs > 0
                && self.last_activity.elapsed() > Duration::from_secs(self.idle_exit_secs)
            {
                info!("idle exit after {}s", self.idle_exit_secs);
                break;
            }

            match listener.accept() {
                Ok(stream) => {
                    self.last_activity = Instant::now();
                    // Handle each connection in a blocking manner (simple, single-threaded)
                    // For higher concurrency, we'd spawn a thread or use async.
                    if let Err(e) = self.handle_connection(stream) {
                        error!("connection error: {}", e);
                    }
                }
                Err(e) => {
                    // Non-blocking accept returns WouldBlock when no connection
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    error!("accept error: {}", e);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }

        Ok(())
    }

    /// Handle a single client connection.
    fn handle_connection(&mut self, stream: Stream) -> Result<()> {
        let (rx, tx) = stream.split();
        let mut reader = BufReader::new(rx);
        let mut writer = BufWriter::new(tx);

        loop {
            // Read request frame
            let request: Option<Request> = match read_frame(&mut reader) {
                Ok(Some(req)) => Some(req),
                Ok(None) => {
                    // Clean EOF
                    debug!("client disconnected");
                    return Ok(());
                }
                Err(e) => {
                    // A request not yet arriving is not fatal (defensive: streams
                    // are blocking, but platforms may still surface WouldBlock).
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    error!("read frame error: {}", e);
                    return Err(e.into());
                }
            };
            let Some(request) = request else { continue };

            self.last_activity = Instant::now();

            // Dispatch request
            let response = self.dispatch(request);

            // Write response frame
            if let Err(e) = write_frame(&mut writer, &response) {
                error!("write frame error: {}", e);
                return Err(e.into());
            }
            writer.flush()?;

            // Stop on Stop command
            if matches!(response, Response::Ack { msg, .. } if msg == "stopping") {
                return Ok(());
            }
        }
    }

    /// Dispatch a request to the appropriate handler.
    fn dispatch(&mut self, request: Request) -> Response {
        use Request::*;

        match request {
            Ping { id } => Response::Pong {
                id,
                version: env!("CARGO_PKG_VERSION").to_string(),
                protocol: PROTOCOL_VERSION,
            },

            Status { id } => {
                let mut state = self.state.lock().unwrap();
                state.refresh_sys();
                let data = StatusData {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    protocol: PROTOCOL_VERSION,
                    state: "ready".to_string(),
                    entries: state.index.len() as u64,
                    mem_bytes: state.index_memory_bytes(),
                    roots: state
                        .roots
                        .iter()
                        .map(|r| RootWire {
                            path: r.path.to_string_lossy().to_string(),
                            entries: r.count as u64,
                            watch: r.watch.clone(),
                        })
                        .collect(),
                    progress: state.scan_progress.clone().map(|p| Progress {
                        entries: p.entries,
                        since_ms: p.since_ms,
                    }),
                    unreadable: state.unreadable,
                };
                Response::Status { id, data }
            }

            Query {
                id,
                q,
                limit,
                offset,
                sort,
                root,
            } => {
                use orin_core::query::{SearchResult, SortKey, parse_query};
                let state = self.state.lock().unwrap();

                let mut query = match parse_query(&q) {
                    Ok(q) => q,
                    Err(e) => {
                        return Response::Error {
                            id,
                            code: "PARSE_ERROR".to_string(),
                            msg: e.to_string(),
                        };
                    }
                };

                // Apply overrides
                query.limit = limit as usize;
                query.offset = offset as usize;
                if let Some(s) = sort {
                    query.sort = match s.as_str() {
                        "name" => SortKey::Name,
                        "size" => SortKey::Size,
                        "mtime" => SortKey::Mtime,
                        "depth" => SortKey::Depth,
                        _ => SortKey::Score,
                    };
                }
                query.root = root;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let result: SearchResult = state.index.search(&query, now);

                let hits: Vec<orin_core::protocol::HitWire> = result
                    .hits
                    .iter()
                    .map(|h| orin_core::protocol::HitWire {
                        p: h.path.to_string_lossy().to_string(),
                        n: h.name.clone(),
                        t: h.kind,
                        s: h.size,
                        m: h.mtime,
                        score: h.score,
                    })
                    .collect();

                Response::Hits {
                    id,
                    hits,
                    total: result.total,
                    next_offset: if result.hits.len() == query.limit {
                        Some((query.offset + query.limit) as u32)
                    } else {
                        None
                    },
                    partial: result.partial,
                    took_us: result.took_us,
                    escalated: None,
                }
            }

            Stat { id, path } => {
                let state = self.state.lock().unwrap();
                let info = state.index.stat_path(std::path::Path::new(&path));
                Response::StatData { id, data: info }
            }

            RootsList { id } => {
                let state = self.state.lock().unwrap();
                let roots: Vec<RootWire> = state
                    .roots
                    .iter()
                    .map(|r| RootWire {
                        path: r.path.to_string_lossy().to_string(),
                        entries: r.count as u64,
                        watch: r.watch.clone(),
                    })
                    .collect();
                Response::Hits {
                    id,
                    hits: vec![],
                    total: roots.len() as u64,
                    next_offset: None,
                    partial: false,
                    took_us: 0,
                    escalated: None,
                }
            }

            RootsAdd { id, path } => {
                let mut state = self.state.lock().unwrap();
                let path_buf = std::path::PathBuf::from(&path);
                if !path_buf.exists() {
                    return Response::Error {
                        id,
                        code: "NOT_FOUND".to_string(),
                        msg: format!("path does not exist: {}", path),
                    };
                }
                let root = crate::state::RootEntry {
                    path: path_buf.clone(),
                    first: u32::MAX,
                    count: 0,
                    watch: "auto".to_string(),
                };
                state.roots.push(root);
                Response::Ack {
                    id,
                    msg: format!("added root: {}", path),
                }
            }

            RootsRemove { id, path } => {
                let mut state = self.state.lock().unwrap();
                let len_before = state.roots.len();
                state
                    .roots
                    .retain(|r| r.path != std::path::Path::new(&path));
                if state.roots.len() == len_before {
                    Response::Error {
                        id,
                        code: "NOT_FOUND".to_string(),
                        msg: format!("root not found: {}", path),
                    }
                } else {
                    Response::Ack {
                        id,
                        msg: format!("removed root: {}", path),
                    }
                }
            }

            Rescan { id, root: _root } => {
                let mut state = self.state.lock().unwrap();
                state.scan_progress = Some(ScanProgress {
                    entries: 0,
                    since_ms: 0,
                });
                // Actual rescan logic will be implemented in scan.rs
                Response::Ack {
                    id,
                    msg: "rescan started".to_string(),
                }
            }

            Stop { id } => Response::Ack {
                id,
                msg: "stopping".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn server_creation() {
        let dir = tempdir().unwrap();
        let state =
            crate::state::test_support::new_shared_hermetic(dir.path(), "server_creation").unwrap();
        let server = Server::new(state, 0).unwrap();
        assert_eq!(server.idle_exit_secs, 0);
    }
}
