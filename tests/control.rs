use super::*;

#[test]
fn ping_round_trips_with_the_token() {
    let server = ControlServer::start(0).unwrap();
    let addr = format!("127.0.0.1:{}", server.port);
    let handle = thread::spawn(move || {
        // The client asks the window's version as it connects.
        let job = server.jobs.recv().unwrap();
        assert_eq!(job.method, "server.version");
        job.reply
            .send(Ok(json!(env!("CARGO_PKG_VERSION"))))
            .unwrap();
        let job = server.jobs.recv().unwrap();
        assert_eq!(job.method, "ping");
        job.reply.send(Ok(json!("pong"))).unwrap();
        // Keep the listener alive until the client finishes.
        thread::sleep(Duration::from_millis(200));
    });
    let mut client = ControlClient::connect(&addr).unwrap();
    assert_eq!(client.call("ping", &json!({})).unwrap(), json!("pong"));
    handle.join().unwrap();
}

#[test]
fn tokens_compare_by_every_byte() {
    assert!(same_token("abc123", "abc123"));
    assert!(!same_token("abc124", "abc123"));
    assert!(!same_token("abc12", "abc123"));
    assert!(!same_token("", "abc123"));
}

#[test]
fn the_client_sends_its_token_only_to_a_loopback_address() {
    let err = ControlClient::connect("192.0.2.1:9")
        .err()
        .expect("refused");
    assert!(err.to_string().contains("loopback"), "{err}");
}

#[test]
fn a_line_is_read_only_up_to_its_limit() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let writer = thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(b"short\r\n").unwrap();
        stream.write_all(&[b'x'; 64]).unwrap();
        stream.write_all(b"\n").unwrap();
    });
    let (conn, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(conn);
    let deadline = Some(Instant::now() + Duration::from_secs(5));
    let line = read_line_within(&mut reader, 32, deadline).unwrap();
    assert_eq!(line.as_deref(), Some(&b"short"[..]));
    let err = read_line_within(&mut reader, 32, deadline).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    writer.join().unwrap();
}

#[test]
fn a_trickling_peer_is_cut_off_at_the_deadline() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let writer = thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).unwrap();
        for _ in 0..6 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let (conn, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(conn);
    let started = Instant::now();
    let deadline = Some(started + Duration::from_millis(250));
    let err = read_line_within(&mut reader, 1024, deadline).unwrap_err();
    assert!(matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    assert!(started.elapsed() < Duration::from_millis(450));
    writer.join().unwrap();
}

#[test]
fn a_request_a_drawing_window_never_takes_is_withdrawn_and_never_runs() {
    let (jobs_tx, jobs_rx) = mpsc::channel();
    let server = ControlServer {
        port: 0,
        jobs: jobs_rx,
        window: Arc::new(Window::default()),
    };
    let (reply_tx, reply_rx) = mpsc::channel();
    let state = Arc::new(AtomicU8::new(PENDING));
    jobs_tx
        .send(Job {
            method: "ui.inspect".into(),
            params: Value::Null,
            reply: reply_tx,
            state: state.clone(),
        })
        .unwrap();
    // No frame is drawn: the request is withdrawn after the stall.
    let started = Instant::now();
    let waited = wait_reply(
        &reply_rx,
        &state,
        &server.window.frames,
        Duration::from_millis(50),
        Duration::from_secs(5),
    );
    assert!(matches!(waited, Waited::Stalled));
    assert!(started.elapsed() < Duration::from_secs(1));
    // The window drawing again later does not run it.
    assert!(server.poll().is_empty());
    std::mem::forget(server);
}

#[test]
fn a_taken_request_is_answered_and_not_withdrawn() {
    let frames = AtomicU64::new(0);
    let state = AtomicU8::new(TAKEN);
    let (reply_tx, reply_rx) = mpsc::channel();
    let answer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(120));
        reply_tx.send(Ok(json!({"done": true}))).unwrap();
    });
    // Taken work may take longer than the stall without being withdrawn.
    let waited = wait_reply(
        &reply_rx,
        &state,
        &frames,
        Duration::from_millis(30),
        Duration::from_secs(5),
    );
    assert!(matches!(waited, Waited::Reply(Ok(_))));
    answer.join().unwrap();
}
