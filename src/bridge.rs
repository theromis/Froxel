//! Loopback bridge for Godot and Blender.
//!
//! `froxel bridge` and `froxel --bridge PORT` speak the same protocol: one
//! JSON object per line on `127.0.0.1`. A client asks for script parameters,
//! sends a new set, and receives a glTF of the document. The client never
//! supplies a path. The session file is the only secret, and it is mode
//! 0600 on Unix.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::script::ParamValue;
use crate::state::Session;

pub const DEFAULT_PORT: u16 = 29771;
const MAX_FRAME: usize = 65_536;
const KEPT_RESULTS: usize = 8;
/// A connection has this long to send a request that passes the token check.
/// Until then a silent peer would hold the one client slot.
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum Op {
    Describe,
    Evaluate(Map<String, Value>),
}

pub struct BridgeJob {
    pub id: i64,
    pub op: Op,
    pub reply: Sender<Result<Value, String>>,
}

pub struct BridgeServer {
    pub port: u16,
    pub session_path: PathBuf,
    pub root: PathBuf,
    jobs: Receiver<BridgeJob>,
    wake: Arc<Mutex<Option<egui::Context>>>,
    shutdown: Arc<AtomicBool>,
}

impl BridgeServer {
    pub fn start(port: u16) -> io::Result<Self> {
        Self::launch(port, AUTH_TIMEOUT)
    }

    fn launch(port: u16, auth_timeout: Duration) -> io::Result<Self> {
        let token = random_token()?;
        let root = std::env::temp_dir().join(format!("froxel-bridge-{}", &token[..16]));
        let _guard = DirGuard(root.clone());
        fs::create_dir(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let session_path = root.join("session.json");
        write_private(
            &session_path,
            &serde_json::to_vec(&json!({
                "v": 1,
                "host": "127.0.0.1",
                "port": port,
                "token": token,
                "root": root.display().to_string(),
            }))
            .map_err(|err| io::Error::other(err.to_string()))?,
        )?;
        let (tx, jobs) = mpsc::channel();
        let wake = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));
        let wake_thread = wake.clone();
        let shutdown_thread = shutdown.clone();
        thread::Builder::new()
            .name("froxel-bridge".into())
            .spawn(move || {
                accept_loop(
                    listener,
                    token,
                    auth_timeout,
                    tx,
                    wake_thread,
                    shutdown_thread,
                )
            })?;
        remove_on_signal(root.clone());
        eprintln!("bridge listening on 127.0.0.1:{port}");
        eprintln!("FROXEL_SESSION={}", session_path.display());
        let _ = std::io::stderr().flush();
        std::mem::forget(_guard);
        Ok(Self {
            port,
            session_path,
            root,
            jobs,
            wake,
            shutdown,
        })
    }

    pub fn bind_context(&self, ctx: &egui::Context) {
        *self.wake.lock().expect("bridge wake") = Some(ctx.clone());
    }

    pub fn poll(&self) -> Vec<BridgeJob> {
        let mut jobs = Vec::new();
        while let Ok(job) = self.jobs.try_recv() {
            jobs.push(job);
        }
        jobs
    }

    /// Block until a client request arrives. `Ok(None)` is a timeout.
    /// `Err` means the listener thread has gone.
    pub fn wait(&self, timeout: Duration) -> Result<Option<BridgeJob>, ()> {
        match self.jobs.recv_timeout(timeout) {
            Ok(job) => Ok(Some(job)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(()),
        }
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], self.port)),
            Duration::from_millis(200),
        );
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Ctrl-C and SIGTERM skip `Drop`, which would leave the session directory
/// and its token behind. The handler only sets a flag; a thread removes the
/// directory and exits with the usual status for an interrupted process.
#[cfg(unix)]
fn remove_on_signal(root: PathBuf) {
    static STOP: AtomicBool = AtomicBool::new(false);
    extern "C" fn on_signal(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let _ = thread::Builder::new()
        .name("froxel-bridge-signal".into())
        .spawn(move || loop {
            if STOP.load(Ordering::SeqCst) {
                let _ = fs::remove_dir_all(&root);
                std::process::exit(130);
            }
            thread::sleep(Duration::from_millis(100));
        });
}

/// Windows has no handler here: Ctrl-C ends the process and the directory
/// stays in the temp folder.
#[cfg(not(unix))]
fn remove_on_signal(_root: PathBuf) {}

struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn run(args: &[String]) -> i32 {
    match run_args(args) {
        Ok(None) => 0,
        Ok(Some((port, input))) => match serve(&input, port) {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("{err}");
                1
            }
        },
        Err(err) => {
            eprintln!("{err}");
            2
        }
    }
}

fn run_args(args: &[String]) -> Result<Option<(u16, PathBuf)>, String> {
    let mut port = DEFAULT_PORT;
    let mut input = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--port" => {
                index += 1;
                let text = args
                    .get(index)
                    .filter(|s| !s.starts_with('-'))
                    .ok_or_else(|| "--port needs a port".to_string())?;
                port = text
                    .parse()
                    .map_err(|_| format!("port `{text}` is not a number"))?;
            }
            "--input" => {
                index += 1;
                let text = args
                    .get(index)
                    .filter(|s| !s.starts_with('-'))
                    .ok_or_else(|| "--input needs a .vox path".to_string())?;
                input = Some(PathBuf::from(text));
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
        index += 1;
    }
    let Some(input) = input else {
        print_help();
        return Err("missing --input".into());
    };
    Ok(Some((port, input)))
}

fn print_help() {
    eprintln!(
        "\
usage: froxel bridge --input <file.vox> [--port PORT]

Listens on 127.0.0.1. PORT defaults to {DEFAULT_PORT}; 0 picks a free port.
Prints FROXEL_SESSION=<path> and serves that .vox until the process stops.
Godot and Blender connect with the session file. See docs/bridges.md."
    );
}

fn serve(input: &Path, port: u16) -> Result<(), String> {
    let mut session = Session::new();
    session.open_vox(input)?;
    let server = BridgeServer::start(port).map_err(|err| err.to_string())?;
    loop {
        match server.wait(Duration::from_secs(1)) {
            Ok(Some(job)) => {
                let result = dispatch(&mut session, &server.root, job.id, &job.op);
                let _ = job.reply.send(result);
            }
            Ok(None) => {}
            Err(()) => return Err("bridge stopped".into()),
        }
    }
}

/// Apply one request. `Ok` is the reply without `v` and `id`.
pub fn dispatch(session: &mut Session, root: &Path, id: i64, op: &Op) -> Result<Value, String> {
    match op {
        Op::Describe => Ok(json!({
            "op": "schema",
            "parameters": parameters(session),
        })),
        Op::Evaluate(values) => evaluate(session, root, id, values),
    }
}

fn parameters(session: &Session) -> Vec<Value> {
    let mut out = Vec::new();
    for (index, object) in session.objects.iter().enumerate() {
        for param in &object.script.params {
            let (kind, value, min, max) = match param.value {
                ParamValue::Float { value, min, max } => {
                    ("float", json!(value), Some(json!(min)), Some(json!(max)))
                }
                ParamValue::Int { value, min, max } => {
                    ("int", json!(value), Some(json!(min)), Some(json!(max)))
                }
                ParamValue::Bool(value) => ("bool", json!(value), None, None),
                ParamValue::Color([r, g, b]) => (
                    "color",
                    json!([
                        f64::from(r) / 255.0,
                        f64::from(g) / 255.0,
                        f64::from(b) / 255.0
                    ]),
                    None,
                    None,
                ),
            };
            let mut spec = json!({
                "id": format!("{index}/{}", param.name),
                "name": param.name,
                "object": index,
                "objectName": object.name,
                "type": kind,
                "value": value,
            });
            if let (Some(min), Some(max)) = (min, max) {
                spec["min"] = min;
                spec["max"] = max;
            }
            out.push(spec);
        }
    }
    out
}

fn evaluate(
    session: &mut Session,
    root: &Path,
    id: i64,
    values: &Map<String, Value>,
) -> Result<Value, String> {
    let path = root.join(format!("result-{id}.glb"));
    // One undo step in the window. A rejected value or a failing script
    // leaves the document as it was.
    session.transaction(|session| {
        apply_values(session, values)?;
        let count = session.objects.len();
        for index in 0..count {
            if session.objects[index].script.source.is_empty() {
                continue;
            }
            session.regenerate_script(index)?;
        }
        let options = crate::export::MeshExport {
            vertex_colors: session.export.vertex_colors,
            merge: session.export.merge,
            export_hidden: session.export.export_hidden,
            split_objects: session.export.split_objects,
        };
        crate::export::export_with(session, &path, &options)
    })?;
    prune_results(root);
    let path = path.to_str().ok_or("result path is not utf-8")?;
    Ok(json!({"op": "result", "path": path}))
}

fn apply_values(session: &mut Session, values: &Map<String, Value>) -> Result<(), String> {
    let mut grouped: BTreeMap<usize, Vec<(String, ParamValue)>> = BTreeMap::new();
    for (key, value) in values {
        let (index, name) = resolve(session, key)?;
        let spec = session.objects[index]
            .script
            .params
            .iter()
            .find(|param| param.name == name)
            .expect("resolve checks the parameter")
            .value
            .clone();
        let typed = coerce(key, value, &spec)?;
        grouped.entry(index).or_default().push((name, typed));
    }
    for (index, updates) in grouped {
        session.set_script_params(index, &updates)?;
    }
    Ok(())
}

/// `0/radius` selects one object. A bare name is accepted when only one
/// parameter has it, so a variant file can say `{"radius": 12}`.
fn resolve(session: &Session, key: &str) -> Result<(usize, String), String> {
    if let Some((index, name)) = key.split_once('/') {
        if !name.is_empty() {
            if let Ok(index) = index.parse::<usize>() {
                let found = session.objects.get(index).and_then(|object| {
                    object.script.params.iter().find(|param| param.name == name)
                });
                if found.is_some() {
                    return Ok((index, name.to_string()));
                }
            }
        }
    }
    let mut found = None;
    for (index, object) in session.objects.iter().enumerate() {
        if object.script.params.iter().any(|param| param.name == key) {
            if found.is_some() {
                return Err(format!("parameter `{key}` is ambiguous; use object/name"));
            }
            found = Some(index);
        }
    }
    match found {
        Some(index) => Ok((index, key.to_string())),
        None => Err(format!("unknown parameter `{key}`")),
    }
}

fn coerce(key: &str, value: &Value, spec: &ParamValue) -> Result<ParamValue, String> {
    let fail = |message: &str| format!("parameter `{key}`: {message}");
    match spec {
        ParamValue::Bool(_) => match value {
            Value::Bool(flag) => Ok(ParamValue::Bool(*flag)),
            _ => Err(fail("expected bool")),
        },
        ParamValue::Int { min, max, .. } => {
            if !value.is_i64() {
                return Err(fail("expected int"));
            }
            let number = value.as_i64().expect("json integer");
            let number = i32::try_from(number).map_err(|_| fail("outside parameter range"))?;
            if number < *min || number > *max {
                return Err(fail("outside parameter range"));
            }
            Ok(ParamValue::Int {
                value: number,
                min: *min,
                max: *max,
            })
        }
        ParamValue::Float { min, max, .. } => {
            let Some(number) = value.as_f64().filter(|n| n.is_finite()) else {
                return Err(fail("expected number"));
            };
            let number = number as f32;
            if !number.is_finite() || number < *min || number > *max {
                return Err(fail("outside parameter range"));
            }
            Ok(ParamValue::Float {
                value: number,
                min: *min,
                max: *max,
            })
        }
        ParamValue::Color(_) => {
            let Some(channels) = value.as_array() else {
                return Err(fail("invalid component count"));
            };
            if channels.len() != 3 && channels.len() != 4 {
                return Err(fail("invalid component count"));
            }
            let mut rgb = [0u8; 3];
            for (index, channel) in channels.iter().take(3).enumerate() {
                let Some(component) = channel.as_f64().filter(|n| n.is_finite()) else {
                    return Err(fail("invalid component"));
                };
                if !(0.0..=1.0).contains(&component) {
                    return Err(fail("color outside [0,1]"));
                }
                rgb[index] = (component * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            if channels.len() == 4 {
                let Some(alpha) = channels[3].as_f64().filter(|n| n.is_finite()) else {
                    return Err(fail("invalid component"));
                };
                if !(0.0..=1.0).contains(&alpha) {
                    return Err(fail("color outside [0,1]"));
                }
            }
            Ok(ParamValue::Color(rgb))
        }
    }
}

fn prune_results(root: &Path) {
    let mut files: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("glb"))
        .collect();
    files.sort_by(|a, b| {
        let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
        modified(a).cmp(&modified(b)).then_with(|| a.cmp(b))
    });
    if files.len() > KEPT_RESULTS {
        for old in &files[..files.len() - KEPT_RESULTS] {
            let _ = fs::remove_file(old);
        }
    }
}

fn json_i64(value: &Value) -> Option<i64> {
    if value.is_i64() {
        value.as_i64()
    } else {
        None
    }
}

fn parse_request(msg: &Value, token: &str, last_id: i64) -> Result<(i64, Op), String> {
    let version_ok = msg.get("v").and_then(json_i64) == Some(1);
    if !version_ok || msg.get("token").and_then(Value::as_str) != Some(token) {
        return Err("invalid version or token".into());
    }
    let Some(id) = msg.get("id").and_then(json_i64).filter(|id| *id >= 0) else {
        return Err("invalid id".into());
    };
    if id <= last_id {
        return Err("id must increase within connection".into());
    }
    match msg.get("op").and_then(Value::as_str) {
        Some("describe") => Ok((id, Op::Describe)),
        Some("evaluate") => match msg.get("values") {
            Some(Value::Object(values)) => Ok((id, Op::Evaluate(values.clone()))),
            _ => Err("expected values".into()),
        },
        _ => Err("unknown operation".into()),
    }
}

struct Frames {
    buffer: Vec<u8>,
}

impl Frames {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn feed(&mut self, data: &[u8]) -> Result<Vec<String>, String> {
        self.buffer.extend_from_slice(data);
        let mut messages = Vec::new();
        while let Some(end) = self.buffer.iter().position(|&byte| byte == b'\n') {
            if end > MAX_FRAME {
                return Err("frame too large".into());
            }
            let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            let text = String::from_utf8(line).map_err(|_| "frame is not utf-8".to_string())?;
            messages.push(text);
        }
        if self.buffer.len() > MAX_FRAME {
            return Err("frame too large".into());
        }
        Ok(messages)
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    auth_timeout: Duration,
    tx: Sender<BridgeJob>,
    wake: Arc<Mutex<Option<egui::Context>>>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let Ok((conn, _)) = listener.accept() else {
            break;
        };
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if let Err(err) = serve_conn(conn, &token, auth_timeout, &tx, &wake) {
            eprintln!("bridge connection: {err}");
        }
    }
}

fn serve_conn(
    mut conn: TcpStream,
    token: &str,
    auth_timeout: Duration,
    tx: &Sender<BridgeJob>,
    wake: &Mutex<Option<egui::Context>>,
) -> io::Result<()> {
    conn.set_nodelay(true)?;
    conn.set_read_timeout(Some(auth_timeout))?;
    let mut write = conn.try_clone()?;
    let mut frames = Frames::new();
    let mut chunk = [0u8; 8192];
    let mut last_id = -1i64;
    let mut authenticated = false;
    loop {
        let read = match conn.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err)
                if !authenticated
                    && matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                break;
            }
            Err(err) => return Err(err),
        };
        let lines = match frames.feed(&chunk[..read]) {
            Ok(lines) => lines,
            Err(_) => break,
        };
        for line in lines {
            let msg: Value = match serde_json::from_str(&line) {
                Ok(Value::Object(map)) => Value::Object(map),
                _ => return Ok(()),
            };
            match parse_request(&msg, token, last_id) {
                Ok((id, op)) => {
                    last_id = id;
                    if !authenticated {
                        authenticated = true;
                        conn.set_read_timeout(None)?;
                    }
                    let (reply_tx, reply_rx) = mpsc::channel();
                    if tx
                        .send(BridgeJob {
                            id,
                            op,
                            reply: reply_tx,
                        })
                        .is_err()
                    {
                        return Ok(());
                    }
                    if let Some(ctx) = wake.lock().expect("bridge wake").as_ref() {
                        ctx.request_repaint();
                    }
                    let body = match reply_rx.recv_timeout(Duration::from_secs(60)) {
                        Ok(Ok(mut value)) => {
                            if let Some(object) = value.as_object_mut() {
                                object.insert("v".into(), json!(1));
                                object.insert("id".into(), json!(id));
                            }
                            value
                        }
                        Ok(Err(message)) => error_body(id, &message),
                        Err(_) => error_body(id, "timeout"),
                    };
                    write_json(&mut write, &body)?;
                }
                Err(message) => {
                    let id = msg
                        .get("id")
                        .and_then(Value::as_i64)
                        .filter(|id| *id >= 0)
                        .unwrap_or(-1);
                    write_json(&mut write, &error_body(id, &message))?;
                    if !authenticated {
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

fn error_body(id: i64, message: &str) -> Value {
    json!({"v": 1, "id": id, "op": "error", "message": message})
}

fn write_json(write: &mut TcpStream, body: &Value) -> io::Result<()> {
    let text = serde_json::to_string(body).map_err(|err| io::Error::other(err.to_string()))?;
    write.write_all(text.as_bytes())?;
    write.write_all(b"\n")?;
    write.flush()
}

fn random_token() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|err| io::Error::other(err.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

#[cfg(test)]
#[path = "../tests/bridge.rs"]
mod tests;
