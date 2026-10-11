//! IPC server: accepts connections, reads frames, dispatches to handlers, writes responses.

use anyhow::Result;
use interprocess::local_socket::{ListenerNonblockingMode, Name, Stream, prelude::*};
use orin_core::protocol::{
    PROTOCOL_VERSION, Progress, Request, Response, RootWire, StatusData, read_frame, write_frame,
};
use std::io::{BufReader, BufWriter, Write};
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use crate::priority;
use crate::revalidate::Revalidate;
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
    pub fn run(&mut self, shutdown: Arc<AtomicBool>, waker: Arc<AcceptWaker>) -> Result<()> {
        let listener = {
            let mut state = self.state.lock().unwrap();
            state.listener.take().expect("listener not initialized")
        };

        info!("server listening on {:?}", listener);

        // Blocking accept: the kernel wakes us the moment a client connects.
        // The waker thread parks on a condvar (it polls nothing) and opens a
        // dummy connection only when shutdown is signaled or the armed idle
        // deadline comes due, which is what keeps this wait interruptible.
        listener.set_nonblocking(ListenerNonblockingMode::Neither)?;
        let _stop_waker = StopWaker(waker.clone());

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
            let deadline = if self.idle_exit_secs > 0 {
                self.last_activity
                    .checked_add(Duration::from_secs(self.idle_exit_secs))
            } else {
                None
            };
            waker.arm(deadline);

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
                    state: if state.scan_progress.is_some() {
                        "building".to_string()
                    } else {
                        "ready".to_string()
                    },
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
                // Revalidation runs on demand only: here (client request) and
                // once at startup. The filesystem watcher stays the live
                // source of truth in between, so no periodic pass exists.
                let shared = self.state.clone();
                {
                    let mut guard = shared.lock().unwrap();
                    if guard.scan_progress.is_some() {
                        return Response::Ack {
                            id,
                            msg: "rescan already running".to_string(),
                        };
                    }
                    guard.scan_progress = Some(ScanProgress {
                        entries: 0,
                        since_ms: 0,
                    });
                }
                // On its own below-normal thread: a full walk must never
                // delay the request path or compete with a query in flight.
                let _rescan = std::thread::spawn(move || {
                    priority::set_below_normal();
                    let result = Revalidate::run(&shared);
                    shared.lock().unwrap().scan_progress = None;
                    if let Err(e) = result {
                        warn!("rescan revalidation failed: {}", e);
                    }
                });
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

/// Stops the waker thread when the server loop exits, on every path.
struct StopWaker(Arc<AcceptWaker>);

impl Drop for StopWaker {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// Event-driven interrupt for the server's blocking `accept()`.
///
/// There is no polling loop: the waker thread parks on a [`Condvar`] and
/// wakes only for a real event — the armed idle deadline coming due, a
/// shutdown request, or the server re-arming the deadline. Between events it
/// blocks with no timeout at all, so an idle daemon does no periodic work.
pub struct AcceptWaker {
    /// Every condition the waker thread waits on.
    core: Mutex<WakerState>,
    /// Signalled whenever `core` changes.
    changed: Condvar,
}

/// Wake-up conditions shared by the server loop and the waker thread.
struct WakerState {
    /// Deadline at which the server must re-check its idle-exit condition.
    wake_at: Option<Instant>,
    /// One-shot request from the shutdown path to interrupt `accept()`.
    shutdown: bool,
    /// Set once the server loop is done and the waker may exit.
    stop: bool,
}

impl AcceptWaker {
    /// Create a waker for the listener `socket` and start its thread.
    pub fn start(socket: Name<'static>) -> Arc<Self> {
        let core = WakerState {
            wake_at: None,
            shutdown: false,
            stop: false,
        };
        let waker = Arc::new(Self {
            core: Mutex::new(core),
            changed: Condvar::new(),
        });
        let for_thread = Arc::clone(&waker);
        let _thread = std::thread::spawn(move || for_thread.run(socket));
        waker
    }

    /// Arm the deadline at which `accept()` must be interrupted; `None`
    /// means "never" (idle exit disabled).
    pub fn arm(&self, wake_at: Option<Instant>) {
        let mut core = self.core.lock().unwrap();
        core.wake_at = wake_at;
        self.changed.notify_all();
    }

    /// Report a shutdown request so the waker interrupts `accept()` once.
    ///
    /// The flag is set while holding the lock, so the notification cannot be
    /// lost between the waker's condition check and its wait.
    pub fn notify_shutdown(&self) {
        let mut core = self.core.lock().unwrap();
        core.shutdown = true;
        core.wake_at = None;
        self.changed.notify_all();
    }

    /// Tell the waker thread to exit; the server sets this on every exit path.
    pub fn stop(&self) {
        let mut core = self.core.lock().unwrap();
        core.stop = true;
        self.changed.notify_all();
    }

    /// Park until an armed event fires, then interrupt `accept()` once.
    fn run(&self, socket: Name<'static>) {
        loop {
            let mut core = self.core.lock().unwrap();
            loop {
                if core.stop {
                    return;
                }
                if core.shutdown {
                    // Consume the one-shot request, then sleep until stop.
                    core.shutdown = false;
                    core.wake_at = None;
                    break;
                }
                let deadline = core.wake_at;
                let Some(at) = deadline else {
                    core = self.changed.wait(core).unwrap_or_else(|p| p.into_inner());
                    continue;
                };
                let now = Instant::now();
                if now >= at {
                    core.wake_at = None;
                    break;
                }
                let (next, _) = self
                    .changed
                    .wait_timeout(core, at - now)
                    .unwrap_or_else(|p| p.into_inner());
                core = next;
            }
            drop(core);
            wake_accept(&socket);
        }
    }
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
