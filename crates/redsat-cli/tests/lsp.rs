//! End to end: drive `redsat lsp` over stdio with real JSON-RPC messages.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Client {
    fn start() -> Self {
        // A hung server must fail the test, not stall the whole test run.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(60));
            eprintln!("the language server did not answer within 60 s");
            std::process::exit(1);
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_redsat"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("redsat lsp starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn receive(&mut self) -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).expect("header");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().expect("length");
            }
        }
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body).expect("body");
        serde_json::from_slice(&body).expect("JSON")
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.receive();
            if message["id"] == json!(id) {
                return message["result"].clone();
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn diagnostics(&mut self) -> Value {
        loop {
            let message = self.receive();
            if message["method"] == "textDocument/publishDiagnostics" {
                return message["params"]["diagnostics"].clone();
            }
        }
    }
}

const URI: &str = "file:///tmp/redsat-lsp-test/factor.rsat";

fn position(line: u32, character: u32) -> Value {
    json!({"textDocument": {"uri": URI}, "position": {"line": line, "character": character}})
}

#[test]
fn serves_diagnostics_completion_hover_and_definition() {
    let mut client = Client::start();
    let init = client.request(
        "initialize",
        json!({"processId": null, "rootUri": null, "capabilities": {}}),
    );
    assert_eq!(init["capabilities"]["hoverProvider"], json!(true));
    client.notify("initialized", json!({}));

    // A broken document first: one error.
    let broken = "var x: int[0..=9]\nrequire x + z > 3\n";
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": URI, "languageId": "rsat", "version": 1, "text": broken}}),
    );
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics.as_array().map(Vec::len), Some(1));
    assert_eq!(
        diagnostics[0]["range"]["start"],
        json!({"line": 1, "character": 12})
    );

    // A fixed document: no errors, and hovers show inferred domains.
    let fixed = "var n = 1000\nvar x, y\nrequire x > 1 and y > 1\nrequire x * y == n\nsolve x, y\n";
    client.notify(
        "textDocument/didChange",
        json!({"textDocument": {"uri": URI, "version": 2}, "contentChanges": [{"text": fixed}]}),
    );
    assert_eq!(client.diagnostics(), json!([]));

    let hover = client.request("textDocument/hover", position(1, 4));
    let text = hover["contents"]["value"].as_str().expect("markdown");
    assert!(
        text.contains("var x: int") && text.contains("[2, 500]"),
        "{text}"
    );

    let completion = client.request("textDocument/completion", position(4, 6));
    let labels: Vec<&str> = completion
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|i| i["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"x") && labels.contains(&"n") && labels.contains(&"sum"),
        "{labels:?}"
    );

    let definition = client.request("textDocument/definition", position(3, 8));
    assert_eq!(
        definition["range"]["start"],
        json!({"line": 1, "character": 4})
    );

    let hints = client.request(
        "textDocument/inlayHint",
        json!({"textDocument": {"uri": URI}, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}}),
    );
    assert_eq!(hints.as_array().map(Vec::len), Some(2));

    assert_eq!(client.request("shutdown", Value::Null), Value::Null);
    client.notify("exit", Value::Null);
    assert!(client.child.wait().expect("exit").success());
}
