//! `fpl mcp --http` end to end: the real binary on 127.0.0.1, driven with raw
//! HTTP the way the deploy-your-own connector's Worker reaches it through a
//! tunnel. Keychain-free: the bearer token comes from `FPL_MCP_TOKEN`, and the
//! one call made fails on its arguments before any read. The rules the server
//! enforces are cli-common's, tested there; this holds fpl to them.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "5f0c1e2d3b4a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
const TOOLS: [&str; 4] = ["fpl_balance", "fpl_bills", "fpl_payments", "fpl_usage"];

fn fpl() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fpl"));
    c.env_remove("FPL_MCP_TOKEN").env(
        "XDG_CONFIG_HOME",
        std::env::temp_dir().join(format!("fpl-mcp-http-test-{}", std::process::id())),
    );
    c
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Each line the server writes to stderr, read on a thread until it closes.
fn stderr_lines(stderr: ChildStderr) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    rx
}

struct Server {
    child: Child,
    port: u16,
    start_line: String,
}

impl Server {
    /// The server on a free port. It is ready when its own start line, which
    /// names the port, is on stderr; something else answering on the port
    /// does not count. A port taken between `free_port` and the bind is
    /// retried on another, and any other exit before the start line fails at
    /// once with the server's stderr. A thread reads stderr to its end, so the
    /// server never blocks on a full pipe.
    fn start() -> Self {
        for _ in 0..3 {
            let port = free_port();
            let mut child = fpl()
                .args(["mcp", "--http", "--port", &port.to_string()])
                .env("FPL_MCP_TOKEN", TOKEN)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("fpl starts");
            let lines = stderr_lines(child.stderr.take().unwrap());
            let ready = format!("at http://127.0.0.1:{port}/mcp");
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut seen = String::new();
            loop {
                match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(line) if line.contains(&ready) => {
                        return Self {
                            child,
                            port,
                            start_line: line,
                        }
                    }
                    Ok(line) => {
                        seen.push_str(&line);
                        seen.push('\n');
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let status = child.wait().unwrap();
                        if seen.contains("listening on") {
                            break; // the port was taken: try another
                        }
                        panic!("fpl exited ({status}) before serving: {seen}");
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let _ = child.kill();
                        panic!("fpl was not serving on {port} within 20 s: {seen}");
                    }
                }
            }
        }
        panic!("no free port could be bound in three tries");
    }

    /// One POST /mcp: the status and the JSON body.
    fn post(&self, auth: Option<&str>, body: Value) -> (u16, Value) {
        let mut stream = TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], self.port))).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let body = body.to_string();
        let mut req = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
             Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
             MCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(token) = auth {
            req.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&body);
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").expect("a response head");
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        (
            status,
            serde_json::from_str(body.trim()).unwrap_or(Value::Null),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_start_line_names_the_version() {
    let s = Server::start();
    let expected = format!("fpl {} mcp: serving", env!("CARGO_PKG_VERSION"));
    assert!(s.start_line.contains(&expected), "{}", s.start_line);
}

#[test]
fn the_worker_gets_the_four_read_only_tools_with_the_token_only() {
    let s = Server::start();
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});

    let (status, _) = s.post(None, list.clone());
    assert_eq!(status, 401, "no token");
    let (status, _) = s.post(Some(&TOKEN.replace('5', "6")), list.clone());
    assert_eq!(status, 401, "a wrong token");

    let (status, got) = s.post(Some(TOKEN), list);
    assert_eq!(status, 200, "{got}");
    let names: Vec<&str> = got["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, TOOLS);

    // A call goes through to the tool, which refuses a bad date before it
    // reads a credential.
    let (_, call) = s.post(
        Some(TOKEN),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "fpl_bills", "arguments": {"since": "yesterday"}}}),
    );
    assert_eq!(call["result"]["isError"], true, "{call}");
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("usage:"), "{text}");
}

#[test]
fn it_will_not_start_without_a_port_or_a_usable_token() {
    let out = fpl().args(["mcp", "--http"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "no --port");

    let out = fpl()
        .args(["mcp", "--http", "--port", &free_port().to_string()])
        .env("FPL_MCP_TOKEN", "too-short")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("FPL_MCP_TOKEN"));
}
