//! MCP `reload` and `froxel mcp --watch`: run a rebuilt `froxel` binary in
//! place of this server without the client restarting it.
//!
//! The server re-executes itself (`exec` on Unix): the process, its PID,
//! and its stdin and stdout stay, so the client's connection is kept. Before
//! that, the new binary is asked `froxel mcp --reload-check`, so a broken
//! or an older build is refused and this server keeps running. The open
//! document is written to a temporary `.vox` with a small JSON state (its
//! path, the active object, unsaved changes, the protocol version, and any
//! input already read past the request), which the new process passes as
//! `--resume <state.json>`: it reopens the document, answers as an
//! initialized server, and sends `notifications/tools/list_changed`, so a
//! client that honours `listChanged` asks for `tools/list` again. Connected
//! to an editor (`--connect`), the document lives in the window and only
//! the server restarts. Undo history and a preview pose do not survive.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::state::Session;

/// Asked of the new binary before it replaces this one.
pub const CHECK_FLAG: &str = "--reload-check";
/// Its answer: the resume format it reads.
pub const CHECK_REPLY: &str = "froxel-mcp-resume 1";
/// Passed to the new process with the state file.
pub const RESUME_FLAG: &str = "--resume";

/// What identifies a build of the binary on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

/// This executable's path. Linux names a replaced binary `... (deleted)`;
/// the path a rebuild wrote is the one without it.
pub fn exe() -> io::Result<PathBuf> {
    let path = std::env::current_exe()?;
    let text = path.to_string_lossy();
    Ok(match text.strip_suffix(" (deleted)") {
        Some(base) => PathBuf::from(base),
        None => path,
    })
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Ask `exe` whether it can resume this server's state.
pub fn check(exe: &Path) -> Result<(), String> {
    let output = std::process::Command::new(exe)
        .args(["mcp", CHECK_FLAG])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("could not run {}: {err}", exe.display()))?;
    let text = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && text.trim() == CHECK_REPLY {
        Ok(())
    } else {
        Err(format!(
            "{} does not answer {CHECK_FLAG} with `{CHECK_REPLY}` (a build without reload, or a broken one); this server keeps running",
            exe.display()
        ))
    }
}

/// A reload ready to happen once the reply to `reload` is written.
#[derive(Debug)]
pub struct Plan {
    pub exe: PathBuf,
    pub state: PathBuf,
    pub document: Option<PathBuf>,
}

impl Plan {
    /// Remove the state of a reload that did not happen.
    pub fn clean_up(&self) {
        let _ = std::fs::remove_file(&self.state);
        if let Some(document) = &self.document {
            let _ = std::fs::remove_file(document);
        }
    }
}

/// Write the state the new process resumes from: the document (headless
/// only) and `protocolVersion`.
pub fn save(session: &mut Session, headless: bool, version: &str) -> Result<Plan, String> {
    let exe = exe().map_err(|err| format!("this server's binary: {err}"))?;
    let dir = std::env::temp_dir();
    let id = std::process::id();
    let document = if headless {
        let path = dir.join(format!("froxel-mcp-reload-{id}.vox"));
        let bytes = session.export_bytes()?;
        std::fs::write(&path, bytes)
            .map_err(|err| format!("could not write {}: {err}", path.display()))?;
        Some(path)
    } else {
        None
    };
    let state = json!({
        "version": 1,
        "protocolVersion": version,
        "document": document.as_ref().map(|p| p.display().to_string()),
        "path": session.path.as_ref().map(|p| p.display().to_string()),
        "nameHint": session.name_hint,
        "selected": session.selected,
        // A reopened .vox may rebase local cells to the SIZE corner; these
        // put each object back in its own frame, so local cells stay put.
        "frames": session.objects.iter().map(|o| json!({
            "translation": [o.translation.0, o.translation.1, o.translation.2],
            "boxMin": [o.box_min.0, o.box_min.1, o.box_min.2],
            "dims": [o.dims.0, o.dims.1, o.dims.2],
        })).collect::<Vec<_>>(),
        "unsaved": headless && session.has_unsaved_changes(),
        "pending": [],
    });
    let path = dir.join(format!("froxel-mcp-reload-{id}.json"));
    std::fs::write(&path, state.to_string())
        .map_err(|err| format!("could not write {}: {err}", path.display()))?;
    Ok(Plan {
        exe,
        state: path,
        document,
    })
}

/// Hand over to the new binary with the input read past the request.
/// Returns only when `exec` failed; the caller keeps serving then.
pub fn exec(plan: &Plan, args: &[String], pending: &[u8]) -> String {
    let result = std::fs::read_to_string(&plan.state)
        .map_err(|err| err.to_string())
        .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|err| err.to_string()))
        .and_then(|mut state| {
            state["pending"] = json!(pending);
            std::fs::write(&plan.state, state.to_string()).map_err(|err| err.to_string())
        });
    if let Err(err) = result {
        return format!("could not hand over the state: {err}");
    }
    let mut next: Vec<String> = vec!["mcp".into()];
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == RESUME_FLAG {
            skip = true;
            continue;
        }
        next.push(arg.clone());
    }
    next.push(RESUME_FLAG.into());
    next.push(plan.state.display().to_string());
    exec_binary(&plan.exe, &next)
}

#[cfg(unix)]
fn exec_binary(exe: &Path, args: &[String]) -> String {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(exe).args(args).exec();
    format!("could not run {}: {err}", exe.display())
}

#[cfg(not(unix))]
fn exec_binary(_exe: &Path, _args: &[String]) -> String {
    "reload re-executes the server in place, which needs Unix; restart the server instead".into()
}

/// What a resumed server continues with.
pub struct Resumed {
    pub version: String,
    pub pending: Vec<u8>,
    pub objects: usize,
}

/// Restore a session from a state file and delete the state.
pub fn resume(path: &Path, session: &mut Session) -> Result<Resumed, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| format!("reload state {}: {err}", path.display()))?;
    let _ = std::fs::remove_file(path);
    let state: Value = serde_json::from_str(&text).map_err(|err| format!("reload state: {err}"))?;
    if state["version"] != 1 {
        return Err(format!(
            "reload state version {} is unknown",
            state["version"]
        ));
    }
    if let Some(document) = state["document"].as_str() {
        let bytes = std::fs::read(document).map_err(|err| format!("{document}: {err}"));
        let _ = std::fs::remove_file(document);
        session.import_bytes(&bytes?)?;
        session.path = state["path"].as_str().map(PathBuf::from);
        session.name_hint = state["nameHint"].as_str().map(str::to_string);
        let frames = state["frames"].as_array().cloned().unwrap_or_default();
        if frames.len() == session.objects.len() {
            for (object, frame) in session.objects.iter_mut().zip(&frames) {
                let triple = |key: &str| -> Option<(i32, i32, i32)> {
                    let v = frame[key].as_array()?;
                    let n = |i: usize| v.get(i)?.as_i64().map(|n| n as i32);
                    Some((n(0)?, n(1)?, n(2)?))
                };
                let (Some(t), Some(box_min), Some(dims)) =
                    (triple("translation"), triple("boxMin"), triple("dims"))
                else {
                    continue;
                };
                let now = object.translation;
                let d = (now.0 - t.0, now.1 - t.1, now.2 - t.2);
                if d != (0, 0, 0) {
                    object.cells = object
                        .cells
                        .drain()
                        .map(|(c, slot)| ((c.0 + d.0, c.1 + d.1, c.2 + d.2), slot))
                        .collect();
                    object.translation = t;
                }
                object.box_min = box_min;
                object.dims = dims;
            }
        }
        if let Some(index) = state["selected"].as_u64() {
            session.select_object(index as usize);
        }
        if state["unsaved"] == true {
            session.mark_unsaved();
        }
    }
    let pending = state["pending"]
        .as_array()
        .map(|bytes| {
            bytes
                .iter()
                .filter_map(|b| b.as_u64().map(|b| b as u8))
                .collect()
        })
        .unwrap_or_default();
    Ok(Resumed {
        version: state["protocolVersion"]
            .as_str()
            .unwrap_or(super::PROTOCOL)
            .to_string(),
        pending,
        objects: session.objects.len(),
    })
}

/// Lines of stdin, read into a buffer this server owns, so input already
/// read past a `reload` request can be handed to the new process.
pub struct Lines {
    buf: Vec<u8>,
    source: Box<dyn Read>,
    eof: bool,
}

impl Lines {
    pub fn stdin(pending: Vec<u8>) -> Self {
        Self {
            buf: pending,
            source: raw_stdin(),
            eof: false,
        }
    }

    #[cfg(test)]
    pub fn from(source: Box<dyn Read>, pending: Vec<u8>) -> Self {
        Self {
            buf: pending,
            source,
            eof: false,
        }
    }

    /// The next line without its newline; `None` at the end of input.
    pub fn next_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(at) = self.buf.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=at).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            if self.eof {
                return Ok((!self.buf.is_empty()).then(|| std::mem::take(&mut self.buf)));
            }
            let mut chunk = [0u8; 65536];
            match self.source.read(&mut chunk) {
                Ok(0) => self.eof = true,
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
    }

    /// Input read but not yet returned as a line.
    pub fn rest(&self) -> &[u8] {
        &self.buf
    }
}

/// Stdin with no buffer of its own, so nothing is read that `rest` does
/// not hold.
#[cfg(unix)]
fn raw_stdin() -> Box<dyn Read> {
    use std::os::unix::io::FromRawFd;
    struct Raw(std::mem::ManuallyDrop<std::fs::File>);
    impl Read for Raw {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }
    // SAFETY: fd 0 is this process's stdin for its whole life; ManuallyDrop
    // never closes it.
    Box::new(Raw(std::mem::ManuallyDrop::new(unsafe {
        std::fs::File::from_raw_fd(0)
    })))
}

#[cfg(not(unix))]
fn raw_stdin() -> Box<dyn Read> {
    Box::new(io::stdin())
}
