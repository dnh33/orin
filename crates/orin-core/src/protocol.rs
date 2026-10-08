//! Daemon wire protocol: length-prefixed JSON frames over a local socket.

use crate::query::StatInfo;
use serde::{Deserialize, Serialize};

/// Current protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum frame size (16 MiB).
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Client → daemon requests.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping {
        id: u64,
    },
    Status {
        id: u64,
    },
    Query {
        id: u64,
        q: String,
        limit: u32,
        offset: u32,
        sort: Option<String>,
        root: Option<String>,
    },
    Stat {
        id: u64,
        path: String,
    },
    RootsList {
        id: u64,
    },
    RootsAdd {
        id: u64,
        path: String,
    },
    RootsRemove {
        id: u64,
        path: String,
    },
    Rescan {
        id: u64,
        root: Option<String>,
    },
    Stop {
        id: u64,
    },
}

/// Daemon → client responses.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Pong {
        id: u64,
        version: String,
        protocol: u32,
    },
    Error {
        id: u64,
        code: String,
        msg: String,
    },
    Status {
        id: u64,
        data: StatusData,
    },
    Hits {
        id: u64,
        hits: Vec<HitWire>,
        total: u64,
        next_offset: Option<u32>,
        partial: bool,
        took_us: u64,
        escalated: Option<String>,
    },
    StatData {
        id: u64,
        data: StatInfo,
    },
    Ack {
        id: u64,
        msg: String,
    },
}

/// Wire format for a search hit.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HitWire {
    pub p: String,
    pub n: String,
    pub t: u8,
    pub s: u64,
    pub m: u32,
    pub score: f32,
}

/// Daemon status data.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatusData {
    pub version: String,
    pub protocol: u32,
    pub state: String, // "building" | "ready" | "revalidating"
    pub entries: u64,
    pub mem_bytes: u64,
    pub roots: Vec<RootWire>,
    pub progress: Option<Progress>,
    pub unreadable: u64,
}

/// Root status for wire format.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RootWire {
    pub path: String,
    pub entries: u64,
    pub watch: String,
}

/// Indexing progress info.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Progress {
    pub entries: u64,
    pub since_ms: u64,
}

/// Write a length-prefixed JSON frame.
pub fn write_frame<W: std::io::Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(msg)?;
    if bytes.len() > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let len = bytes.len() as u32;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

/// Read a length-prefixed JSON frame.
pub fn read_frame<R: std::io::Read, T: for<'de> Deserialize<'de>>(
    r: &mut R,
) -> std::io::Result<Option<T>> {
    let mut len_bytes = [0u8; 4];
    match r.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    let msg = serde_json::from_slice(&buf)?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let msg = Request::Ping { id: 42 };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let decoded: Option<Request> = read_frame(&mut cursor).unwrap();
        assert!(matches!(decoded, Some(Request::Ping { id: 42 })));
    }

    #[test]
    fn response_roundtrip() {
        let msg = Response::Pong {
            id: 1,
            version: "0.1.0".into(),
            protocol: 1,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let decoded: Option<Response> = read_frame(&mut cursor).unwrap();
        assert!(matches!(decoded, Some(Response::Pong { id: 1, .. })));
    }
}
