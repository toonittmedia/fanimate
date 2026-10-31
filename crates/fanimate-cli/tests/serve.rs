//! `fanimate-cli serve` as a child process: the scripting host over a local
//! socket, which a running tool or a plugin connects to.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

struct Host {
    child: Child,
    port: u64,
    token: String,
}

fn start() -> Host {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(["serve"])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .expect("fanimate-cli runs");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let ready: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"));
    assert_eq!(ready["host"], "127.0.0.1");
    assert_eq!(ready["api_version"], fanimate_api::API_VERSION);
    Host {
        child,
        port: ready["port"].as_u64().expect("a port"),
        token: ready["token"].as_str().expect("a token").to_owned(),
    }
}

struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(port: u64) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connects");
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        Self {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    fn ask(&mut self, request: Value) -> Value {
        writeln!(self.writer, "{request}").unwrap();
        self.writer.flush().unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    }

    fn auth(&mut self, token: &str) -> Value {
        self.ask(json!({"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": token}}))
    }
}

#[test]
fn a_client_needs_the_token_then_calls_work_and_the_work_outlives_the_connection() {
    let mut host = start();

    // no token, a wrong one, and a call before auth are all refused and the connection is closed
    let mut stranger = Client::connect(host.port);
    let refused = stranger.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "layers"}));
    assert_eq!(refused["error"]["data"]["code"], "unauthorized", "{refused}");
    let mut wrong = Client::connect(host.port);
    assert_eq!(
        wrong.auth("not-the-token")["error"]["data"]["code"],
        "unauthorized"
    );
    let mut closed = String::new();
    assert_eq!(
        wrong.reader.read_line(&mut closed).unwrap(),
        0,
        "the host hung up"
    );

    // the right token works
    let mut first = Client::connect(host.port);
    assert_eq!(first.auth(&host.token)["result"], true);
    let made = first.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "add_text_layer",
        "params": {"content": "Over a socket", "x": 10, "y": 10}}));
    let id = made["result"].as_u64().expect("a layer id");
    drop(first);

    // a second connection sees what the first did: one project, many clients
    let mut second = Client::connect(host.port);
    assert_eq!(second.auth(&host.token)["result"], true);
    let layers = second.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "layers"}));
    assert_eq!(layers["result"][0]["id"], id, "{layers}");
    // and a bad request is an answer, not the end of the host
    let bad = second.ask(json!({"jsonrpc": "2.0", "id": 2, "method": "nothing_like_it"}));
    assert_eq!(bad["error"]["code"], -32601);
    // quit ends only this connection
    assert_eq!(
        second.ask(json!({"jsonrpc": "2.0", "id": 3, "method": "quit"}))["id"],
        3
    );
    drop(second);
    let mut third = Client::connect(host.port);
    assert_eq!(third.auth(&host.token)["result"], true);

    // shutdown stops the host, which exits cleanly
    assert_eq!(
        third.ask(json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}))["id"],
        4
    );
    assert!(host.child.wait().unwrap().success());
}

#[test]
fn two_hosts_get_different_tokens_and_ports() {
    let mut a = start();
    let mut b = start();
    assert_ne!(a.token, b.token, "a token is random");
    assert_ne!(a.port, b.port);
    assert_eq!(a.token.len(), 32);
    for host in [&mut a, &mut b] {
        let mut c = Client::connect(host.port);
        assert_eq!(c.auth(&host.token)["result"], true);
        c.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown"}));
        assert!(host.child.wait().unwrap().success());
    }
    // a's token is no good to b
    let mut b2 = start();
    let mut c = Client::connect(b2.port);
    assert_eq!(c.auth(&a.token)["error"]["data"]["code"], "unauthorized");
    let mut d = Client::connect(b2.port);
    assert_eq!(d.auth(&b2.token)["result"], true);
    d.ask(json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown"}));
    assert!(b2.child.wait().unwrap().success());
}
