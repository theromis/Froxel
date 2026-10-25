//! Loopback control port for the open editor.
//!
//! `froxel --control PORT` listens on `127.0.0.1`. Each line is a JSON
//! request, each reply is one JSON line. The session token is written to an
//! owner-only file so another local user cannot drive this window.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

pub struct Job {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Result<Value, String>>,
}

pub struct ControlServer {
    pub port: u16,
    jobs: Receiver<Job>,
    wake: Arc<Mutex<Option<egui::Context>>>,
}

impl ControlServer {
    pub fn start(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let token = issue(port)?;
        let (tx, jobs) = mpsc::channel();
        let wake = Arc::new(Mutex::new(None));
        let wake_thread = wake.clone();
        thread::Builder::new()
            .name("froxel-control".into())
            .spawn(move || accept_loop(listener, token, tx, wake_thread))?;
        eprintln!("control listening on 127.0.0.1:{port}");
        Ok(Self { port, jobs, wake })
    }

    /// Wake the editor when a request arrives, including while it is idle.
    pub fn bind_context(&self, ctx: &egui::Context) {
        *self.wake.lock().expect("control wake") = Some(ctx.clone());
    }

    pub fn poll(&self) -> Vec<Job> {
        let mut jobs = Vec::new();
        while let Ok(job) = self.jobs.try_recv() {
            jobs.push(job);
        }
        jobs
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        revoke(self.port);
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    tx: Sender<Job>,
    wake: Arc<Mutex<Option<egui::Context>>>,
) {
    for conn in listener.incoming() {
        let Ok(conn) = conn else { continue };
        let token = token.clone();
        let tx = tx.clone();
        let wake = wake.clone();
        thread::spawn(move || {
            if let Err(err) = serve_conn(conn, &token, &tx, &wake) {
                eprintln!("control connection: {err}");
            }
        });
    }
}

fn serve_conn(
    conn: TcpStream,
    token: &str,
    tx: &Sender<Job>,
    wake: &Mutex<Option<egui::Context>>,
) -> io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(600)))?;
    let mut write = conn.try_clone()?;
    let reader = BufReader::new(conn);
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => return Err(err),
        };
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > 16 * 1024 * 1024 {
            let _ = writeln!(write, r#"{{"id":null,"ok":false,"error":"line too long"}}"#);
            break;
        }
        let msg: Value = match serde_json::from_str::<Value>(&line) {
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
        if given != token {
            let reply = json!({"id": id, "ok": false, "error": "unauthorized: missing or invalid control token"});
            writeln!(write, "{reply}")?;
            continue;
        }
        let method = msg["method"].as_str().unwrap().to_string();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx
            .send(Job {
                method,
                params,
                reply: reply_tx,
            })
            .is_err()
        {
            break;
        }
        if let Some(ctx) = wake.lock().expect("control wake").as_ref() {
            ctx.request_repaint();
        }
        let result = reply_rx.recv_timeout(Duration::from_secs(60));
        let body = match result {
            Ok(Ok(value)) => json!({"id": id, "ok": true, "result": value}),
            Ok(Err(error)) => json!({"id": id, "ok": false, "error": error}),
            Err(_) => json!({"id": id, "ok": false, "error": "timeout", "state": "may-have-run"}),
        };
        writeln!(write, "{body}")?;
        write.flush()?;
    }
    Ok(())
}

pub struct ControlClient {
    stream: TcpStream,
    token: String,
    next_id: i64,
}

impl ControlClient {
    pub fn connect(addr: &str) -> io::Result<Self> {
        let port = addr
            .rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or_else(|| io::Error::other(format!("no port in {addr}")))?;
        let token = read_token(port)?;
        let stream = TcpStream::connect_timeout(
            &addr
                .parse()
                .map_err(|e| io::Error::other(format!("{addr}: {e}")))?,
            Duration::from_secs(5),
        )
        .map_err(|e| {
            io::Error::other(format!(
                "Froxel is not answering at {addr} ({e}). Open it with `froxel --control {port}`"
            ))
        })?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        Ok(Self {
            stream,
            token,
            next_id: 1,
        })
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
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
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
