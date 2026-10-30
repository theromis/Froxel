use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::*;
use crate::script::ParamValue;
use crate::state::Session;

const SCRIPT: &str = r#"
let side = param_int("side", 1, 1, 4);
let flag = param_bool("flag", true);
let tint = param_color("tint", [255, 0, 0]);
for x in 0..side {
    set(x, 0, 0, 1);
}
"#;

#[test]
fn describe_lists_each_objects_parameters() {
    let mut session = scripted();
    let schema = dispatch(&mut session, Path::new("."), 1, &Op::Describe).unwrap();
    let params = schema["parameters"].as_array().unwrap();
    assert_eq!(params.len(), 3);
    assert_eq!(params[0]["id"], "0/side");
    assert_eq!(params[0]["name"], "side");
    assert_eq!(params[0]["object"], 0);
    assert_eq!(params[0]["objectName"], "Tree");
    assert_eq!(params[0]["type"], "int");
    assert_eq!(params[0]["value"], 1);
    assert_eq!(params[0]["min"], 1);
    assert_eq!(params[0]["max"], 4);
    assert_eq!(params[1]["type"], "bool");
    assert_eq!(params[1]["value"], true);
    assert_eq!(params[2]["type"], "color");
    assert_eq!(params[2]["value"], json!([1.0, 0.0, 0.0]));
}

#[test]
fn evaluate_reruns_the_script_and_writes_a_glb() {
    let mut session = scripted();
    let root = temp_root("eval");
    let reply = dispatch(
        &mut session,
        &root,
        7,
        &Op::Evaluate(map(&[("side", json!(3)), ("0/flag", json!(false))])),
    )
    .unwrap();
    assert_eq!(reply["op"], "result");
    let path = PathBuf::from(reply["path"].as_str().unwrap());
    assert_eq!(path.parent().unwrap(), root.as_path());
    assert_eq!(&fs::read(&path).unwrap()[..4], b"glTF");
    assert_eq!(session.objects[0].cells.len(), 3);
    assert!(matches!(
        session.objects[0].script.params[1].value,
        ParamValue::Bool(false)
    ));
    assert!(matches!(
        &session.objects[0].script.params[2].value,
        ParamValue::Color([255, 0, 0])
    ));

    let kept = dispatch(
        &mut session,
        &root,
        8,
        &Op::Evaluate(map(&[("tint", json!([0.0, 0.0, 1.0, 1.0]))])),
    )
    .unwrap();
    assert_eq!(kept["op"], "result");
    assert_eq!(session.objects[0].cells.len(), 3);
    assert!(matches!(
        session.objects[0].script.params[2].value,
        ParamValue::Color([0, 0, 255])
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn evaluate_rejects_a_bad_value_without_exporting() {
    let mut session = scripted();
    let root = temp_root("reject");
    let err = dispatch(
        &mut session,
        &root,
        1,
        &Op::Evaluate(map(&[("side", json!(9))])),
    )
    .unwrap_err();
    assert!(err.contains("outside parameter range"), "{err}");
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    assert_eq!(session.objects[0].cells.len(), demo_cells());

    let err = dispatch(
        &mut session,
        &root,
        2,
        &Op::Evaluate(map(&[("flag", json!(1))])),
    )
    .unwrap_err();
    assert!(err.contains("expected bool"), "{err}");

    let err = dispatch(
        &mut session,
        &root,
        3,
        &Op::Evaluate(map(&[("missing", json!(1))])),
    )
    .unwrap_err();
    assert!(err.contains("unknown parameter"), "{err}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_bare_name_is_ambiguous_across_objects() {
    let mut session = scripted();
    session
        .add_object(Some("Pad"), [4, 0, 0], None, false)
        .unwrap();
    session.set_script(1, SCRIPT).unwrap();
    let root = temp_root("ambiguous");
    let err = dispatch(
        &mut session,
        &root,
        1,
        &Op::Evaluate(map(&[("side", json!(2))])),
    )
    .unwrap_err();
    assert!(err.contains("ambiguous"), "{err}");
    dispatch(
        &mut session,
        &root,
        2,
        &Op::Evaluate(map(&[("1/side", json!(2))])),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.len(), 1);
    assert_eq!(session.objects[1].cells.len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_model_without_scripts_still_exports() {
    let mut session = Session::new();
    let voxels = session.objects[0].cells.len();
    assert!(voxels > 0);
    let root = temp_root("static");
    let reply = dispatch(&mut session, &root, 1, &Op::Evaluate(Map::new())).unwrap();
    assert_eq!(
        &fs::read(reply["path"].as_str().unwrap()).unwrap()[..4],
        b"glTF"
    );
    assert_eq!(session.objects[0].cells.len(), voxels);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn old_results_are_capped() {
    let mut session = scripted();
    let root = temp_root("cap");
    for id in 1..=9 {
        dispatch(&mut session, &root, id, &Op::Evaluate(Map::new())).unwrap();
    }
    let glbs = fs::read_dir(&root)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .ok()
                .and_then(|entry| entry.path().extension().map(|ext| ext == "glb"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(glbs, 8);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn request_envelope_and_frames() {
    let good = json!({"v": 1, "id": 1, "token": "secret", "op": "describe"});
    assert!(parse_request(&good, "secret", -1).is_ok());
    assert!(parse_request(
        &json!({"v": 2, "id": 1, "token": "secret", "op": "describe"}),
        "secret",
        -1
    )
    .is_err());
    assert!(parse_request(&good, "other", -1).is_err());
    assert!(parse_request(
        &json!({"v": 1, "id": 1.5, "token": "secret", "op": "describe"}),
        "secret",
        -1
    )
    .is_err());
    assert!(parse_request(
        &json!({"v": true, "id": 1, "token": "secret", "op": "describe"}),
        "secret",
        -1
    )
    .is_err());
    assert!(parse_request(&good, "secret", 1)
        .unwrap_err()
        .contains("increase"));
    assert!(parse_request(
        &json!({"v": 1, "id": 2, "token": "secret", "op": "nope"}),
        "secret",
        -1
    )
    .is_err());
    assert!(parse_request(
        &json!({"v": 1, "id": 2, "token": "secret", "op": "evaluate"}),
        "secret",
        -1
    )
    .is_err());

    let wire = format!("{}\n{}\n", json!({"name": "Этаж"}), json!({"id": 2}));
    let bytes = wire.as_bytes();
    let mut frames = Frames::new();
    let mut got = Vec::new();
    for byte in bytes {
        got.extend(frames.feed(&[*byte]).unwrap());
    }
    assert_eq!(got.len(), 2);
    assert!(frames.feed(&vec![b'x'; MAX_FRAME + 1]).is_err());
    assert!(Frames::new().feed(b"\xff\n").is_err());
}

#[test]
fn bridge_round_trips() {
    let server = BridgeServer::start(0).unwrap();
    let mut session = scripted();
    let session_path = server.session_path.clone();
    let client = thread::spawn(move || {
        let mut client = Client::connect(&session_path);
        let schema = client.call("describe", None);
        assert_eq!(schema["op"], "schema");
        assert_eq!(schema["v"], 1);
        let result = client.call("evaluate", Some(json!({"values": {"side": 4}})));
        assert_eq!(result["op"], "result", "{result}");
        assert_eq!(
            &fs::read(result["path"].as_str().unwrap()).unwrap()[..4],
            b"glTF"
        );
        let refused = client.raw(json!({"v": 1, "id": 3, "token": "nope", "op": "describe"}));
        assert_eq!(refused["op"], "error");
    });
    serve_until(&server, &mut session, client);
    assert_eq!(session.objects[0].cells.len(), 4);
}

#[test]
fn a_failed_evaluate_leaves_the_document_and_undo_as_they_were() {
    let mut session = Session::new();
    session
        .set_script(
            0,
            r#"
let side = param_int("side", 1, 1, 4);
if side == 4 { throw "too wide"; }
for x in 0..side {
    set(x, 0, 0, 1);
}
"#,
        )
        .unwrap();
    let root = temp_root("rollback");
    dispatch(
        &mut session,
        &root,
        1,
        &Op::Evaluate(map(&[("side", json!(2))])),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.len(), 2);

    let err = dispatch(
        &mut session,
        &root,
        2,
        &Op::Evaluate(map(&[("side", json!(4))])),
    )
    .unwrap_err();
    assert!(err.contains("too wide"), "{err}");
    assert_eq!(session.objects[0].cells.len(), 2);
    assert!(matches!(
        session.objects[0].script.params[0].value,
        ParamValue::Int { value: 2, .. }
    ));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

    // The good evaluate was one undo step, and the failed one added none.
    session.undo();
    assert_eq!(session.objects[0].cells.len(), demo_cells());
    assert!(matches!(
        session.objects[0].script.params[0].value,
        ParamValue::Int { value: 1, .. }
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_silent_or_unauthenticated_connection_is_dropped() {
    let server = BridgeServer::launch(0, Duration::from_millis(200), REPLY_TIMEOUT).unwrap();
    let mut session = scripted();
    let session_path = server.session_path.clone();
    let port = server.port;
    let client = thread::spawn(move || {
        // Holds the slot without a word until the timeout closes it.
        let idle = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut client = Client::connect(&session_path);
        let schema = client.call("describe", None);
        assert_eq!(schema["op"], "schema");
        drop(idle);
        drop(client);

        let mut stranger = Client::connect(&session_path);
        let refused = stranger.raw(json!({"v": 1, "id": 1, "token": "nope", "op": "describe"}));
        assert_eq!(refused["op"], "error");
        let mut rest = String::new();
        assert_eq!(stranger.read.read_line(&mut rest).unwrap(), 0, "{rest}");
    });
    serve_until(&server, &mut session, client);
}

#[test]
fn the_reply_waits_out_the_script_limit_and_names_the_timeout() {
    assert!(REPLY_TIMEOUT > SCRIPT_TIMEOUT);
    let server = BridgeServer::launch(0, AUTH_TIMEOUT, Duration::from_millis(200)).unwrap();
    let session_path = server.session_path.clone();
    let client = thread::spawn(move || {
        let mut client = Client::connect(&session_path);
        let reply = client.call("evaluate", Some(json!({"values": {}})));
        assert_eq!(reply["op"], "error");
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["message"], "bake timed out after 0.2 s");
    });
    // Nobody answers the job, as when the editor is busy past the wait.
    let started = std::time::Instant::now();
    let mut held = Vec::new();
    while !client.is_finished() {
        if let Ok(Some(job)) = server.wait(Duration::from_millis(50)) {
            held.push(job);
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no timeout reply"
        );
    }
    client.join().unwrap();
    assert_eq!(held.len(), 1);
}

#[test]
fn an_evaluate_past_its_budget_names_the_bake_and_keeps_the_document() {
    let mut session = scripted();
    let root = temp_root("budget");
    let before = session.objects[0].cells.clone();
    let err = evaluate_within(
        &mut session,
        &root,
        1,
        &map(&[("side", json!(3))]),
        Duration::ZERO,
    )
    .unwrap_err();
    assert_eq!(err, "bake timed out after 0 s");
    assert_eq!(session.objects[0].cells, before);
    assert!(!root.join("result-1.glb").exists());
    let _ = fs::remove_dir_all(root);
}

fn serve_until(server: &BridgeServer, session: &mut Session, client: thread::JoinHandle<()>) {
    let started = std::time::Instant::now();
    while !client.is_finished() {
        match server.wait(Duration::from_millis(50)) {
            Ok(Some(job)) => {
                let result = dispatch(session, &server.root, job.id, &job.op);
                let _ = job.reply.send(result);
            }
            Ok(None) if started.elapsed() > Duration::from_secs(10) => {
                panic!("bridge round trip timed out");
            }
            Ok(None) => {}
            Err(()) => panic!("bridge closed"),
        }
    }
    client.join().unwrap();
}

fn scripted() -> Session {
    let mut session = Session::new();
    session.set_script(0, SCRIPT).unwrap();
    session
}

fn demo_cells() -> usize {
    Session::new().objects[0].cells.len()
}

fn map(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn temp_root(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "froxel-bridge-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

struct Client {
    write: TcpStream,
    read: BufReader<TcpStream>,
    token: String,
    next: i64,
}

impl Client {
    fn connect(session_path: &Path) -> Self {
        let session: Value =
            serde_json::from_str(&fs::read_to_string(session_path).unwrap()).unwrap();
        assert_eq!(session["host"], "127.0.0.1");
        assert_eq!(session["v"], 1);
        let port = u16::try_from(session["port"].as_u64().unwrap()).unwrap();
        let token = session["token"].as_str().unwrap().to_string();
        let write = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write.set_nodelay(true).unwrap();
        write
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let read_stream = write.try_clone().unwrap();
        read_stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Self {
            write,
            read: BufReader::new(read_stream),
            token,
            next: 1,
        }
    }

    fn call(&mut self, op: &str, extra: Option<Value>) -> Value {
        let id = self.next;
        self.next += 1;
        let mut msg = json!({"v": 1, "id": id, "token": self.token, "op": op});
        if let Some(extra) = extra.and_then(|value| value.as_object().cloned()) {
            for (key, value) in extra {
                msg[key] = value;
            }
        }
        self.raw(msg)
    }

    fn raw(&mut self, msg: Value) -> Value {
        writeln!(self.write, "{msg}").unwrap();
        self.write.flush().unwrap();
        let mut line = String::new();
        self.read.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
}

#[test]
fn list_parameters_are_in_the_schema_and_checked_on_evaluate() {
    let mut session = Session::new();
    session
        .set_script(
            0,
            r#"
let path = param_vec2s("path", [[0.0, 0.0], [2.0, 1.0]], 0.0, 8.0, 4);
for p in path { set(to_int(p[0]), to_int(p[1]), 0, 1); }
"#,
        )
        .unwrap();
    let schema = dispatch(&mut session, Path::new("."), 1, &Op::Describe).unwrap();
    let spec = &schema["parameters"][0];
    assert_eq!(spec["type"], "vec2s");
    assert_eq!(spec["value"], json!([[0, 0], [2, 1]]));
    assert_eq!(spec["min"], json!(0.0));
    assert_eq!(spec["max"], json!(8.0));
    assert_eq!(spec["maxLen"], 4);

    let root = temp_root("lists");
    let err = dispatch(
        &mut session,
        &root,
        2,
        &Op::Evaluate(map(&[(
            "path",
            json!([[0, 0], [1, 1], [2, 2], [3, 3], [4, 4]]),
        )])),
    )
    .unwrap_err();
    assert!(err.contains("parameter `path`: has 5 entries"), "{err}");
    let err = dispatch(
        &mut session,
        &root,
        3,
        &Op::Evaluate(map(&[("path", json!([[0, 9]]))])),
    )
    .unwrap_err();
    assert!(err.contains("outside 0..=8"), "{err}");
    dispatch(
        &mut session,
        &root,
        4,
        &Op::Evaluate(map(&[("path", json!([[5, 5], [6, 7]]))])),
    )
    .unwrap();
    assert_eq!(session.objects[0].cells.get(&(6, 7, 0)), Some(&1));
    let _ = fs::remove_dir_all(root);
}
