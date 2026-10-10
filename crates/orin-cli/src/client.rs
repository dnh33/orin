//! IPC client: socket discovery, daemon auto-spawn, framed request/response.

use anyhow::Context;
use anyhow::bail;
use interprocess::local_socket::Name;
use interprocess::local_socket::Stream;
use interprocess::local_socket::prelude::*;
use orin_core::paths::socket_name;
use orin_core::protocol::HitWire;
use orin_core::protocol::Request;
use orin_core::protocol::Response;
use orin_core::protocol::StatusData;
use orin_core::protocol::read_frame;
use orin_core::protocol::write_frame;
use std::time::Duration;
use std::time::Instant;

/// Id used for every one-shot request; responses must echo it back.
const REQ_ID: u64 = 1;
/// Recv timeout for the handshake ping.
const PING_TIMEOUT: Duration = Duration::from_secs(3);
/// Recv timeout for real responses (long searches on big indexes).
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
/// Total retry budget while waiting for a freshly spawned daemon.
const SPAWN_WAIT: Duration = Duration::from_secs(3);
/// Windows `DETACHED_PROCESS`: spawn the daemon without a console window.
#[cfg(windows)]
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
    let _child = spawn_orind()?;
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
    bail!("spawned `orind` but the daemon did not answer in {secs}s");
}

/// Try each plausible socket name; first daemon to answer a ping wins.
fn try_connect() -> anyhow::Result<Option<Stream>> {
    let name = socket_name().context("invalid ORIN_SOCKET value")?;
    if let Some(stream) = handshake(name) {
        return Ok(Some(stream));
    }
    #[cfg(windows)]
    {
        if let Some(stream) = scan_named_pipes() {
            return Ok(Some(stream));
        }
    }
    Ok(None)
}

/// Connect and verify the far end speaks the orin wire protocol.
fn handshake(name: Name<'_>) -> Option<Stream> {
    let mut stream = Stream::connect(name).ok()?;
    let _ = stream.set_recv_timeout(Some(PING_TIMEOUT));
    write_frame(&mut stream, &Request::Ping { id: REQ_ID }).ok()?;
    let response: Response = read_frame(&mut stream).ok().flatten()?;
    if let Response::Pong { id, .. } = response && id == REQ_ID {
        Some(stream)
    } else {
        None
    }
}

/// Send one request frame, read one response frame.
fn send(stream: &mut Stream, req: &Request) -> anyhow::Result<Response> {
    write_frame(stream, req)?;
    let response: Option<Response> = read_frame(stream).context("no response from daemon")?;
    response.context("daemon closed the connection")
}

/// Locate `orind` next to this executable, else rely on PATH.
fn orind_program() -> std::path::PathBuf {
    let mut exe_name = "orind";
    if cfg!(windows) {
        exe_name = "orind.exe";
    }
    if let Ok(exe) = std::env::current_exe() && let Some(dir) = exe.parent() {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    std::path::PathBuf::from("orind")
}

/// Start `orind` in the background, detached from this console.
fn spawn_orind() -> anyhow::Result<std::process::Child> {
    let program = orind_program();
    let mut command = std::process::Command::new(&program);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(DETACHED_PROCESS);
    }
    match command.spawn() {
        Ok(child) => Ok(child),
        Err(err) => Err(anyhow::anyhow!("failed to start `{}`: {err}", program.display())),
    }
}

/// Windows only: scan the named-pipe namespace for a live orin daemon.
#[cfg(windows)]
fn scan_named_pipes() -> Option<Stream> {
    use interprocess::local_socket::GenericNamespaced;
    use interprocess::local_socket::ToNsName;

    let prefix = format!("orin-{}-", whoami::username());
    let mut candidates: Vec<String> = std::fs::read_dir(r"\\.\pipe\").ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    candidates.sort();
    for pipe in candidates {
        let full = format!(r"\\.\pipe\{pipe}");
        let candidate = full.to_ns_name::<GenericNamespaced>();
        if let Ok(name) = candidate && let Some(stream) = handshake(name) {
            return Some(stream);
        }
    }
    None
}
