//! The persistent stdin/stdout loop — Phase 1's core new capability over the
//! Phase 0 probe. Unlike a one-shot `claude -p "<prompt>"`, a supervised
//! session stays alive across turns: the lead writes user-message NDJSON lines
//! to stdin, and events stream back on stdout until a `result` (turn end),
//! after which the lead may write again (reprompt, next assignment).
//!
//! stdout is read on a background thread, parsed to typed [`Event`]s, and each
//! raw line is tee'd to a per-worker transcript file (handoff §3
//! `logs/worker-N/T-XXX.jsonl`) — the raw log stays on disk and never enters
//! the lead's context.

use crate::agent::AgentProcess;
use crate::event::Event;
use crate::parse::parse_line;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Cap on the retained worker stderr tail — enough to carry a crash reason without
/// letting a chatty worker grow it unbounded.
const STDERR_CAP: usize = 8 * 1024;

/// What the reader thread hands back to the supervisor.
#[derive(Debug)]
pub enum SessionMsg {
    Event(Event),
    /// A line that failed to parse as JSON — logged, not fatal.
    BadLine(String),
}

/// Outcome of waiting for the next event within a deadline.
pub enum Recv {
    Got(SessionMsg),
    /// No event within the timeout — the watchdog interprets this.
    Timeout,
    /// stdout closed: the process is done producing (usually exited).
    Closed,
}

pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<SessionMsg>,
    reader: Option<JoinHandle<()>>,
    /// Reader thread for the child's stderr — captured (not dropped) so a worker
    /// that dies writing an error to stderr can be diagnosed. Ends on stderr EOF.
    stderr_reader: Option<JoinHandle<()>>,
    /// The retained tail of the worker's stderr, shared with `stderr_reader`.
    stderr: Arc<Mutex<String>>,
    label: String,
}

impl Session {
    /// Spawn the agent and start streaming. `raw_log`, if set, receives every
    /// raw stdout line (the on-disk transcript).
    pub fn spawn(agent: &dyn AgentProcess, raw_log: Option<PathBuf>) -> Result<Session> {
        let label = agent.label();
        let mut child = agent
            .command()
            .spawn()
            .with_context(|| format!("failed to spawn agent `{label}`"))?;

        let stdout = child.stdout.take().context("agent has no stdout pipe")?;
        let stdin = child.stdin.take().context("agent has no stdin pipe")?;

        // Capture stderr out-of-band (it was previously piped and never read, so a
        // worker that crashed on startup died silently). A small reader thread keeps
        // the tail; `stderr_tail()` surfaces it on exit.
        let stderr = Arc::new(Mutex::new(String::new()));
        let stderr_reader = child.stderr.take().map(|err| {
            let buf = stderr.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(mut s) = buf.lock() {
                        s.push_str(&line);
                        s.push('\n');
                        if s.len() > STDERR_CAP {
                            *s = s[s.len() - STDERR_CAP..].to_string();
                        }
                    }
                }
            })
        });

        let mut log_file = match raw_log {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                Some(File::create(&path).with_context(|| format!("cannot open log {path:?}"))?)
            }
            None => None,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let buf = BufReader::new(stdout);
            for line in buf.lines() {
                let Ok(line) = line else { break };
                if let Some(f) = log_file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                match parse_line(&line) {
                    Some(Ok(ev)) => {
                        if tx.send(SessionMsg::Event(ev)).is_err() {
                            break; // supervisor dropped the session
                        }
                    }
                    Some(Err(_)) => {
                        let _ = tx.send(SessionMsg::BadLine(line));
                    }
                    None => {} // blank line
                }
            }
            // Channel drops here → supervisor sees `Closed`.
        });

        Ok(Session {
            child,
            stdin: Some(stdin),
            rx,
            reader: Some(reader),
            stderr_reader,
            stderr,
            label,
        })
    }

    /// The tail of the worker's stderr, captured out-of-band. Empty for a healthy
    /// worker; the crash reason for one that died writing to stderr.
    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().map(|s| s.trim().to_string()).unwrap_or_default()
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// The child's OS process id — the handle an external controller (the Phase 4f
    /// runner) uses to interrupt an in-flight turn by signalling the process.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Write one user message to stdin, starting (or continuing) a turn. Flushes
    /// so the worker sees it immediately (NDJSON, one object per line).
    pub fn send_user(&mut self, text: &str) -> Result<()> {
        let line = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
        });
        let stdin = self.stdin.as_mut().context("session stdin already closed")?;
        writeln!(stdin, "{line}").context("failed writing to agent stdin")?;
        stdin.flush().context("failed flushing agent stdin")?;
        Ok(())
    }

    /// Close stdin so a streaming worker knows no more input is coming and may
    /// exit cleanly.
    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    /// Wait up to `timeout` for the next event.
    pub fn recv(&self, timeout: Duration) -> Recv {
        match self.rx.recv_timeout(timeout) {
            Ok(msg) => Recv::Got(msg),
            Err(RecvTimeoutError::Timeout) => Recv::Timeout,
            Err(RecvTimeoutError::Disconnected) => Recv::Closed,
        }
    }

    /// Kill the child (watchdog breach, or cleanup) and reap it.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Reap an already-exited child, returning whether it exited successfully.
    pub fn wait(&mut self) -> Option<bool> {
        self.child.wait().ok().map(|s| s.success())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Never leak a child process (BUILDING §8 zombie row).
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stdin.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}
