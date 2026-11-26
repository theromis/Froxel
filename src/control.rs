//! Loopback control port for the open editor.
//!
//! `froxel --control PORT` listens on `127.0.0.1`. Each line is a JSON
//! request, each reply is one JSON line. The session token is written to an
//! owner-only file so another local user cannot drive this window.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The longest request line. A longer one closes the connection; nothing
/// past this is buffered.
const MAX_LINE: usize = 16 * 1024 * 1024;
/// A connection has this long, in all, to send its first request with the
/// token. A silent or trickling peer is dropped after it.
const AUTH_DEADLINE: Duration = Duration::from_secs(5);
/// An authenticated connection may sit idle this long between requests.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);
/// Connections served at once; one past this is closed at accept.
const MAX_CONNECTIONS: usize = 16;
/// A request the window has not taken after this long, with no frame drawn
/// meanwhile, is withdrawn: the window is hidden, minimised, or otherwise
/// drawing nothing, and would run it whenever it next drew.
const STALL: Duration = Duration::from_secs(5);
/// The longest a taken request may run before the client is told so.
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// A job's [`Job::state`]: waiting, taken by the window, or withdrawn.
const PENDING: u8 = 0;
const TAKEN: u8 = 1;
const WITHDRAWN: u8 = 2;

pub struct Job {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Result<Value, String>>,
    /// [`PENDING`], [`TAKEN`], or [`WITHDRAWN`]; the window takes a job only
    /// while it is pending, so a withdrawn one never runs.
    state: Arc<AtomicU8>,
}

/// What the connection threads know of the window: its context, to wake it,
/// and how many frames it has drawn, to tell when it is drawing none.
#[derive(Default)]
struct Window {
    ctx: Mutex<Option<egui::Context>>,
    frames: AtomicU64,
}

pub struct ControlServer {
    pub port: u16,
    jobs: Receiver<Job>,
    window: Arc<Window>,
}

impl ControlServer {
    pub fn start(port: u16) -> io::Result<Self> {
        crate::sandbox::check_port("control")?;
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let token = issue(port)?;
        let (tx, jobs) = mpsc::channel();
        let window = Arc::new(Window::default());
        let window_thread = window.clone();
        thread::Builder::new()
            .name("froxel-control".into())
            .spawn(move || accept_loop(listener, token, tx, window_thread))?;
        eprintln!("control listening on 127.0.0.1:{port}");
        Ok(Self { port, jobs, window })
    }

    /// Wake the editor when a request arrives, including while it is idle.
    pub fn bind_context(&self, ctx: &egui::Context) {
        *self.window.ctx.lock().expect("control wake") = Some(ctx.clone());
    }

    /// The requests waiting, taken by this frame. Called once a frame, which
    /// is how a connection knows the window is drawing.
    pub fn poll(&self) -> Vec<Job> {
        self.window.frames.fetch_add(1, Ordering::SeqCst);
        let mut jobs = Vec::new();
        while let Ok(job) = self.jobs.try_recv() {
            let taken = job
                .state
                .compare_exchange(PENDING, TAKEN, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok();
            if taken {
                jobs.push(job);
            }
        }
        jobs
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        revoke(self.port);
    }
}

fn accept_loop(listener: TcpListener, token: String, tx: Sender<Job>, window: Arc<Window>) {
    let open = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            drop(conn);
            continue;
        }
        let token = token.clone();
        let tx = tx.clone();
        let window = window.clone();
        let open = open.clone();
        thread::spawn(move || {
            if let Err(err) = serve_conn(conn, &token, &tx, &window) {
                eprintln!("control connection: {err}");
            }
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Whether two tokens are equal, in time that depends only on their length.
pub(crate) fn same_token(given: &str, token: &str) -> bool {
    let (given, token) = (given.as_bytes(), token.as_bytes());
    given.len() == token.len()
        && given
            .iter()
            .zip(token)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// One line from `reader` without its newline, or `None` at the end.
/// Nothing past `max` bytes is buffered, and with a `deadline` every read
/// waits only for what is left of it, so a peer that trickles bytes cannot
/// outlast it.
pub(crate) fn read_line_within(
    reader: &mut BufReader<TcpStream>,
    max: usize,
    deadline: Option<Instant>,
) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        if let Some(deadline) = deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            reader.get_ref().set_read_timeout(Some(left))?;
        }
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if buffer.is_empty() {
            return Ok((!line.is_empty()).then_some(line));
        }
        let (taken, done) = match buffer.iter().position(|&byte| byte == b'\n') {
            Some(end) => (end, true),
            None => (buffer.len(), false),
        };
        if line.len() + taken > max {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
        line.extend_from_slice(&buffer[..taken]);
        reader.consume(taken + usize::from(done));
        if done {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
    }
}

fn serve_conn(conn: TcpStream, token: &str, tx: &Sender<Job>, window: &Window) -> io::Result<()> {
    let mut write = conn.try_clone()?;
    let mut reader = BufReader::new(conn);
    let mut deadline = Some(Instant::now() + AUTH_DEADLINE);
    loop {
        let line = match read_line_within(&mut reader, MAX_LINE, deadline) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) if err.kind() == io::ErrorKind::InvalidData => {
                let _ = writeln!(write, r#"{{"id":null,"ok":false,"error":"line too long"}}"#);
                break;
            }
            Err(err)
                if deadline.is_some()
                    && matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                break;
            }
            Err(err) => return Err(err),
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let msg: Value = match serde_json::from_slice::<Value>(&line) {
            Ok(msg) if msg.is_object() && msg.get("method").and_then(Value::as_str).is_some() => {
                msg
            }
            _ => {
                let _ = writeln!(
                    write,
                    r#"{{"id":null,"ok":false,"error":"request must be a JSON object with method"}}"#
                );
                break;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let given = msg.get("token").and_then(Value::as_str).unwrap_or("");
        if !same_token(given, token) {
            let reply = json!({"id": id, "ok": false, "error": "unauthorized: missing or invalid control token"});
            writeln!(write, "{reply}")?;
            break;
        }
        if deadline.take().is_some() {
            reader.get_ref().set_read_timeout(Some(IDLE_TIMEOUT))?;
        }
        let method = msg["method"].as_str().unwrap().to_string();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let (reply_tx, reply_rx) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(PENDING));
        if tx
            .send(Job {
                method,
                params,
                reply: reply_tx,
                state: state.clone(),
            })
            .is_err()
        {
            break;
        }
        if let Some(ctx) = window.ctx.lock().expect("control wake").as_ref() {
            ctx.request_repaint();
        }
        let body = match wait_reply(&reply_rx, &state, &window.frames, STALL, REPLY_TIMEOUT) {
            Waited::Reply(Ok(value)) => json!({"id": id, "ok": true, "result": value}),
            Waited::Reply(Err(error)) => json!({"id": id, "ok": false, "error": error}),
            Waited::Stalled => json!({
                "id": id,
                "ok": false,
                "error": "the editor window is drawing no frames (hidden, minimised, or covered), so it did not take the request; show the window and send it again",
                "state": "not-run"
            }),
            Waited::Timeout => {
                json!({"id": id, "ok": false, "error": "timeout", "state": "may-have-run"})
            }
        };
        writeln!(write, "{body}")?;
        write.flush()?;
    }
    Ok(())
}

/// How a request ended for the connection.
enum Waited {
    Reply(Result<Value, String>),
    /// Withdrawn before the window took it: it will not run.
    Stalled,
    /// Taken, but no reply within [`REPLY_TIMEOUT`].
    Timeout,
}

/// Wait for the window's reply. A request still pending after `stall`
/// ([`STALL`]) with no frame drawn since it was sent is withdrawn, so a
/// window that draws nothing answers at once instead of running it much
/// later; a taken one gets `timeout` ([`REPLY_TIMEOUT`]).
fn wait_reply(
    reply: &Receiver<Result<Value, String>>,
    state: &AtomicU8,
    frames: &AtomicU64,
    stall: Duration,
    timeout: Duration,
) -> Waited {
    let sent = Instant::now();
    let frames_at_send = frames.load(Ordering::SeqCst);
    loop {
        match reply.recv_timeout(stall.min(Duration::from_millis(250))) {
            Ok(result) => return Waited::Reply(result),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Waited::Reply(Err("the editor dropped the request".into()))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let elapsed = sent.elapsed();
        let drew = frames.load(Ordering::SeqCst) != frames_at_send;
        if elapsed >= stall
            && !drew
            && state
                .compare_exchange(PENDING, WITHDRAWN, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            return Waited::Stalled;
        }
        if elapsed >= timeout {
            return Waited::Timeout;
        }
    }
}

pub struct ControlClient {
    stream: TcpStream,
    token: String,
    next_id: i64,
}

impl ControlClient {
    pub fn connect(addr: &str) -> io::Result<Self> {
        crate::sandbox::check_port("control")?;
        let port = addr
            .rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or_else(|| io::Error::other(format!("no port in {addr}")))?;
        let socket: std::net::SocketAddr = addr
            .parse()
            .map_err(|e| io::Error::other(format!("{addr}: {e}")))?;
        // The token goes to whatever answers: only this machine may.
        if !socket.ip().is_loopback() {
            return Err(io::Error::other(format!(
                "{addr} is not a loopback address; the control port listens on 127.0.0.1"
            )));
        }
        let token = read_token(port)?;
        let stream = TcpStream::connect_timeout(&socket, Duration::from_secs(5)).map_err(|e| {
            io::Error::other(format!(
                "Froxel is not answering at {addr} ({e}). Open it with `froxel --control {port}`"
            ))
        })?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        let mut client = Self {
            stream,
            token,
            next_id: 1,
        };
        // The schema this client checks arguments against is its own; a
        // window of another version may run them differently, so say so.
        let ours = env!("CARGO_PKG_VERSION");
        match client.call("server.version", &json!({})) {
            Ok(theirs) if theirs == ours => {}
            Ok(theirs) => eprintln!(
                "froxel mcp {ours} is connected to a Froxel {} window; tools may differ until both are the same version",
                theirs.as_str().unwrap_or("of another")
            ),
            Err(_) => eprintln!(
                "froxel mcp {ours} is connected to an older Froxel window; tools may differ until both are the same version"
            ),
        }
        Ok(client)
    }

    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"id": id, "method": method, "params": params, "token": self.token});
        writeln!(self.stream, "{msg}").map_err(|e| e.to_string())?;
        self.stream.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(self.stream.try_clone().map_err(|e| e.to_string())?)
            .read_line(&mut line)
            .map_err(|e| format!("control reply: {e}"))?;
        if line.is_empty() {
            return Err("control connection closed".into());
        }
        let reply: Value =
            serde_json::from_str(&line).map_err(|e| format!("control reply: {e}"))?;
        if reply.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(reply.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("control call failed")
                .to_string())
        }
    }
}

fn token_path(port: u16) -> io::Result<PathBuf> {
    let dir = if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        PathBuf::from(dir)
    } else if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(home).join(".cache")
    } else {
        std::env::temp_dir()
    };
    Ok(dir.join(format!("froxel-control-{port}.token")))
}

fn issue(port: u16) -> io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| io::Error::other(err.to_string()))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let path = token_path(port)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A stale file or a link someone left in its place is removed, and
        // the token goes only into a file this process creates, owner-only.
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(token.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&path, token.as_bytes())?;
    }
    eprintln!("control token {}", path.display());
    Ok(token)
}

fn read_token(port: u16) -> io::Result<String> {
    let token = fs::read_to_string(token_path(port)?)?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(io::Error::other("empty control token"));
    }
    Ok(token)
}

fn revoke(port: u16) {
    if let Ok(path) = token_path(port) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "../tests/control.rs"]
mod tests;
