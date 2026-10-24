use super::*;

#[test]
fn ping_round_trips_with_the_token() {
    let server = ControlServer::start(0).unwrap();
    let addr = format!("127.0.0.1:{}", server.port);
    let handle = thread::spawn(move || {
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
