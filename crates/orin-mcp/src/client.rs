//! Thin blocking IPC client for the orin daemon: socket discovery,
//! daemon auto-spawn, and framed request/response calls.
//!
//! Mirrors the connection pattern of the `orin` CLI client, which is a
//! binary-only module and therefore cannot be reused from this crate.

use anyhow::Context;
use anyhow::bail;
use interprocess::local_socket::Stream;
use interprocess::local_socket::prelude::*;
use orin_core::paths::socket_name;
use orin_core::protocol::HitWire;
use orin_core::protocol::Request;
use orin_core::protocol::Response;
use orin_core::protocol::StatusData;
use orin_core::protocol::read_frame;
use orin_core::protocol::write_frame;
use orin_core::query::StatInfo;
use std::os::windows::process::CommandExt;
use std::time::Duration;
use std::time::Instant;

/// Id used for every one-shot request; responses must echo it back.
const REQ_ID: u64 = 1;
/// Recv timeout for real responses (long searches on big indexes).
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Total retry budget while waiting for a freshly spawned daemon.
const SPAWN_WAIT: Duration = Duration::from_secs(30);
/// `DETACHED_PROCESS`: spawn the daemon without a console window.
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Run a search query against the daemon, auto-spawning it when absent.
pub(crate) fn query(q: &str, limit: u32) -> anyhow::Result<Vec<HitWire>> {
    let mut stream = connected()?;
    let req = Request::Query {
        id: REQ_ID,
        q: q.to_string(),
        limit,
        offset: 0,
        sort: None,
        root: None,
    };
    let response = send(&mut stream, &req)?;
    match response {
        Response::Hits { id, hits, .. } if id == REQ_ID => Ok(hits),
        Response::Hits { id, .. } => bail!("unexpected response id {id} from daemon"),
        Response::Error { code, msg, .. } => bail!("daemon error {code}: {msg}"),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

/// Fetch metadata for one path, auto-spawning the daemon when absent.
pub(crate) fn stat(path: &str) -> anyhow::Result<StatInfo> {
    let mut stream = connected()?;
    let req = Request::Stat {
        id: REQ_ID,
        path: path.to_string(),
    };
    let response = send(&mut stream, &req)?;
    match response {
        Response::StatData { id, data } if id == REQ_ID => Ok(data),
        Response::StatData { id, .. } => bail!("unexpected response id {id} from daemon"),
        Response::Error { code, msg, .. } => bail!("daemon error {code}: {msg}"),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

/// Fetch daemon status, auto-spawning it when absent.
pub(crate) fn status() -> anyhow::Result<StatusData> {
    let mut stream = connected()?;
    let req = Request::Status { id: REQ_ID };
    let response = send(&mut stream, &req)?;
    match response {
        Response::Status { id, data } if id == REQ_ID => Ok(data),
        Response::Status { id, .. } => bail!("unexpected response id {id} from daemon"),
        Response::Error { code, msg, .. } => bail!("daemon error {code}: {msg}"),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

/// Establish a connection with the response read timeout applied.
fn connected() -> anyhow::Result<Stream> {
    let stream = ensure_connected()?;
    let _ = stream.set_recv_timeout(Some(RESPONSE_TIMEOUT));
    Ok(stream)
}

/// Connect to a running daemon, spawning one in the background if needed.
fn ensure_connected() -> anyhow::Result<Stream> {
    if let Some(stream) = try_connect()? {
        return Ok(stream);
    }
    let no_spawn = std::env::var("ORIN_NO_SPAWN");
    if no_spawn.is_ok_and(|value| value == "1") {
        bail!("daemon not running; ORIN_NO_SPAWN=1 forbids auto-spawn");
    }
    let _child = spawn_daemon()?;
    let deadline = Instant::now() + SPAWN_WAIT;
    let mut delay = Duration::from_millis(50);
    loop {
        if let Some(stream) = try_connect()? {
            return Ok(stream);
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        std::thread::sleep(delay.min(deadline - now));
        delay = (delay * 2).min(Duration::from_millis(400));
    }
    let secs = SPAWN_WAIT.as_secs();
    bail!("spawned `orin daemon` but the daemon did not answer in {secs}s");
}

/// Try each plausible socket name; the first listener to accept wins.
///
/// Accepting the connection is the reachability probe: Query and Status
/// validate the far end by their own read/write, so no Ping round trip is
/// spent before the real request.
fn try_connect() -> anyhow::Result<Option<Stream>> {
    let name = socket_name().context("invalid ORIN_SOCKET value")?;
    if let Ok(stream) = Stream::connect(name) {
        return Ok(Some(stream));
    }
    if let Some(stream) = scan_named_pipes() {
        return Ok(Some(stream));
    }
    Ok(None)
}

/// Send one request frame, read one response frame.
fn send(stream: &mut Stream, req: &Request) -> anyhow::Result<Response> {
    write_frame(stream, req)?;
    let response: Option<Response> = read_frame(stream).context("no response from daemon")?;
    response.context("daemon closed the connection")
}

/// Locate `orin` next to this executable, else rely on PATH.
fn orin_program() -> std::path::PathBuf {
    let exe_name = "orin.exe";
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    std::path::PathBuf::from("orin")
}

/// Spawn without handle inheritance.
///
/// A daemon spawned from a shell pipeline can inherit the shell's pipe
/// handles and hold them open forever (that is how a background daemon kept
/// `orin status --json | head` from ever seeing EOF). Clear the inherit flag
/// on this process's standard handles around the spawn: the daemon receives
/// its own null handles and nothing else. Previous flags are restored.
fn spawn_without_inherit(
    command: &mut std::process::Command,
) -> std::io::Result<std::process::Child> {
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    const GETS: [u32; 3] = [((-10i32) as u32), ((-11i32) as u32), ((-12i32) as u32)];
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n: u32) -> *mut core::ffi::c_void;
        fn GetHandleInformation(h: *mut core::ffi::c_void, flags: *mut u32) -> i32;
        fn SetHandleInformation(h: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    unsafe {
        let mut saved = [0u32; 3];
        for (i, n) in GETS.iter().enumerate() {
            let h = GetStdHandle(*n);
            GetHandleInformation(h, &mut saved[i]);
            SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
        }
        let result = command.spawn();
        for (i, n) in GETS.iter().enumerate() {
            SetHandleInformation(GetStdHandle(*n), HANDLE_FLAG_INHERIT, saved[i] & HANDLE_FLAG_INHERIT);
        }
        result
    }
}

/// Start `orin daemon` in the background, detached from this console.
fn spawn_daemon() -> anyhow::Result<std::process::Child> {
    let program = orin_program();
    let mut command = std::process::Command::new(&program);
    command
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.creation_flags(DETACHED_PROCESS);
    match spawn_without_inherit(&mut command) {
        Ok(child) => Ok(child),
        Err(err) => Err(anyhow::anyhow!(
            "failed to start `{} daemon`: {err}",
            program.display()
        )),
    }
}

/// Scan the named-pipe namespace for a live orin daemon.
fn scan_named_pipes() -> Option<Stream> {
    use interprocess::local_socket::GenericNamespaced;
    use interprocess::local_socket::ToNsName;

    let prefix = format!("orin-{}-", whoami::username());
    let mut candidates: Vec<String> = std::fs::read_dir(r"\\.\pipe\")
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    candidates.sort();
    for pipe in candidates {
        let full = format!(r"\\.\pipe\{pipe}");
        if let Ok(name) = full.to_ns_name::<GenericNamespaced>()
            && let Ok(stream) = Stream::connect(name)
        {
            return Some(stream);
        }
    }
    None
}
