//! `fleetor-ipc` — the `Transport` seam (BUILDING §3), isolated in one crate so
//! nothing else in the system names a `UnixStream`. Unix domain socket now; a
//! named-pipe impl can slot in on Windows behind the same [`Transport`] trait
//! (BUILDING §2 IPC row). Framing is newline-delimited JSON — one
//! [`fleetor_core::wire`] value per line — and rides over any async duplex, so
//! the framing code never changes when the transport does.

use anyhow::{Context, Result};
use fleetor_core::wire::{Hello, Op, Request, Response};
use serde::{de::DeserializeOwned, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

/// A framed, transport-agnostic duplex connection: one JSON value per line.
pub struct Conn {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    line: String,
}

impl Conn {
    fn new<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            line: String::new(),
        }
    }

    /// Write one value as a JSON line and flush so the peer sees it immediately.
    pub async fn write_json<T: Serialize>(&mut self, v: &T) -> Result<()> {
        let mut s = serde_json::to_string(v).context("serializing frame")?;
        s.push('\n');
        self.writer.write_all(s.as_bytes()).await.context("writing frame")?;
        self.writer.flush().await.context("flushing frame")?;
        Ok(())
    }

    /// Read one JSON line into `T`. `Ok(None)` on clean EOF (peer hung up).
    pub async fn read_json<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        self.line.clear();
        let n = self
            .reader
            .read_line(&mut self.line)
            .await
            .context("reading frame")?;
        if n == 0 {
            return Ok(None);
        }
        let v = serde_json::from_str(self.line.trim_end())
            .with_context(|| format!("deserializing frame: {}", self.line.trim_end()))?;
        Ok(Some(v))
    }
}

/// A bound listener yielding framed connections. Concrete over a `UnixListener`
/// today; a named-pipe transport would expose the same `accept() -> Conn`.
pub struct Listener {
    inner: UnixListener,
}

impl Listener {
    /// Accept the next client connection.
    pub async fn accept(&self) -> Result<Conn> {
        let (stream, _addr) = self.inner.accept().await.context("accepting connection")?;
        let (r, w) = stream.into_split();
        Ok(Conn::new(r, w))
    }
}

/// The seam: how a client connects and how the server binds. One impl now
/// ([`UnixTransport`]); a Windows named-pipe impl slots in here later.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self) -> Result<Conn>;
    async fn bind(&self) -> Result<Listener>;
}

/// Unix domain socket transport (macOS-first, Tier-2). The `path` lives under
/// `~/.fleetor/<key>/` in production.
pub struct UnixTransport {
    pub path: PathBuf,
}

impl UnixTransport {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait::async_trait]
impl Transport for UnixTransport {
    async fn connect(&self) -> Result<Conn> {
        let stream = UnixStream::connect(&self.path)
            .await
            .with_context(|| format!("connecting to {:?}", self.path))?;
        let (r, w) = stream.into_split();
        Ok(Conn::new(r, w))
    }

    async fn bind(&self) -> Result<Listener> {
        // Clear a stale socket file so re-bind after a crash succeeds
        // (BUILDING §8, zombie/pidfile row).
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let inner =
            UnixListener::bind(&self.path).with_context(|| format!("binding {:?}", self.path))?;
        Ok(Listener { inner })
    }
}

/// A connected fleet client: sends [`Hello`] once, then issues request/response
/// round-trips. Honors the wire contract's one-in-flight-request-per-connection
/// rule (`fleetor_core::wire`). Used by the shim (per worker) and the lead CLI.
pub struct Client {
    conn: Conn,
}

impl Client {
    /// Connect and announce identity in one step.
    pub async fn connect(transport: &dyn Transport, hello: Hello) -> Result<Client> {
        let mut conn = transport.connect().await?;
        conn.write_json(&hello).await.context("sending hello")?;
        Ok(Client { conn })
    }

    /// Issue one op and await its response. Blocks for as long as the server
    /// holds a blocking op (`AskLead`, `AwaitEvents`).
    pub async fn call(&mut self, op: Op) -> Result<fleetor_core::wire::OpResult> {
        let req = Request::new(op);
        let id = req.id.clone();
        self.conn.write_json(&req).await?;
        let resp: Response = self
            .conn
            .read_json()
            .await?
            .context("server closed the connection before responding")?;
        anyhow::ensure!(
            resp.id == id,
            "response id {} does not match request id {id}",
            resp.id
        );
        Ok(resp.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetor_core::wire::OpResult;
    use fleetor_core::Party;

    // A framed request survives a round-trip over a real unix socket, and the
    // server side reads exactly what the client wrote.
    #[tokio::test]
    async fn request_round_trips_over_a_unix_socket() {
        let dir = std::env::temp_dir().join(format!("fleetor-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sock");
        let transport = UnixTransport::new(&path);
        let listener = transport.bind().await.unwrap();

        // Server: accept, expect Hello then one Request, answer it.
        let server = tokio::spawn(async move {
            let mut conn = listener.accept().await.unwrap();
            let hello: Hello = conn.read_json().await.unwrap().unwrap();
            assert_eq!(hello.party, Party::Worker(2));
            let req: Request = conn.read_json().await.unwrap().unwrap();
            let Op::NotifyLead { text } = &req.op else {
                panic!("expected notify_lead, got {:?}", req.op);
            };
            assert_eq!(text, "hi lead");
            conn.write_json(&Response::new(req.id, OpResult::Ack))
                .await
                .unwrap();
        });

        let mut client = Client::connect(&transport, Hello::new(Party::Worker(2)))
            .await
            .unwrap();
        let result = client
            .call(Op::NotifyLead { text: "hi lead".into() })
            .await
            .unwrap();
        assert_eq!(result, OpResult::Ack);
        server.await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
