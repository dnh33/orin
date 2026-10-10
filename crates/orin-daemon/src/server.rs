//! IPC server: accepts connections, reads frames, dispatches to handlers, writes responses.

use anyhow::Result;
use interprocess::local_socket::{ListenerNonblockingMode, Name, Stream, prelude::*};
use orin_core::paths::socket_name;
use orin_core::protocol::{
    PROTOCOL_VERSION, Progress, Request, Response, RootWire, StatusData, read_frame, write_frame,
};
use std::io::{BufReader, BufWriter, Write};
use std::sync::Mutex;
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

        // Blocking accept: the kernel wakes us the moment a client connects,
        // instead of the accept path sleeping through a polling interval. The
        // waker thread keeps that wait interruptible by opening a dummy
        // connection when shutdown is signaled or idle exit comes due.
        listener.set_nonblocking(ListenerNonblockingMode::Neither)?;
        let wake_at = std::sync::Arc::new(Mutex::new(None));
        spawn_accept_waker(shutdown.clone(), wake_at.clone(), socket_name()?);

        loop {
            if shutdown.load(Ordering::SeqCst) {
                info!("shutdown signaled");
                break;
            }

            // Check idle exit
            if self.idle_expired() {
                info!("idle exit after {}s", self.idle_exit_secs);
                break;
            }

            // Arm the waker for the wait ahead: it fires the moment the idle
            // check above would start failing (never when idle exit is off).
            *wake_at.lock().unwrap() = if self.idle_exit_secs > 0 {
                self.last_activity.checked_add(Duration::from_secs(self.idle_exit_secs))
            } else {
                None
            };

            match listener.accept() {
                Ok(stream) => {
                    // Shutdown and idle wake-ups arrive as ordinary
                    // connections; test both before this one counts as
                    // activity and gets served.
                    if shutdown.load(Ordering::SeqCst) {
                        info!("shutdown signaled");
                        break;
                    }
                    if self.idle_expired() {
                        info!("idle exit after {}s", self.idle_exit_secs);
                        break;
                    }
                    self.last_activity = Instant::now();
                    // Handle each connection in a blocking manner (simple, single-threaded)
                    // For higher concurrency, we'd spawn a thread or use async.
                    if let Err(e) = self.handle_connection(stream) {
                        error!("connection error: {}", e);
                    }
                }
                Err(e) => {
                    // In blocking mode accept() only returns here when the
                    // listener is broken; there is nothing left to poll, so
                    // surface the error instead of sleeping on it.
                    error!("accept error: {}", e);
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    /// True once the daemon has been idle longer than `idle_exit_secs` allows.
    fn idle_expired(&self) -> bool {
        self.idle_exit_secs > 0
            && self.last_activity.elapsed() > Duration::from_secs(self.idle_exit_secs)
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

/// Keep a blocked `accept()` interruptible.
///
/// Polling the shutdown flag on this thread (100ms ticks) never touches the
/// request path, so client latency is unaffected. When shutdown is signaled
/// or the armed idle-exit deadline passes, a dummy connection to the
/// listener's own name is what makes `accept()` return, letting the server
/// loop re-check its exit conditions.
fn spawn_accept_waker(
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake_at: std::sync::Arc<Mutex<Option<Instant>>>,
    socket: Name<'static>,
) {
    let _waker = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        if shutdown.load(Ordering::SeqCst) {
            wake_accept(&socket);
            break;
        }
        let due = {
            let mut slot = wake_at.lock().unwrap();
            let fired = matches!(*slot, Some(at) if Instant::now() > at);
            if fired {
                *slot = None;
            }
            fired
        };
        if due {
            wake_accept(&socket);
        }
    });
}

/// Open a connection to `socket` so a blocked `accept()` returns.
///
/// The connection is held briefly before dropping: closing it first would
/// turn the wake-up into a dead-on-arrival connection that `accept()`
/// discards internally, losing the wake-up entirely.
fn wake_accept(socket: &Name<'static>) {
    for _ in 0..20 {
        if let Ok(stream) = Stream::connect(socket.clone()) {
            std::thread::sleep(Duration::from_millis(500));
            drop(stream);
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    debug!("accept waker could not reach the listener");
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
