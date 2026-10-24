use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    process::Output,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const HUDSON: &str = env!("CARGO_BIN_EXE_hudson");
const AGENT: &str = env!("CARGO_BIN_EXE_hudson-cli");
const SANDBOX: &str = env!("CARGO_BIN_EXE_hudsonbox");
const RUN: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";

fn binary_path(binary: &str) -> std::path::PathBuf {
    match std::env::var_os("HUDSON_TEST_BIN_DIR") {
        Some(directory) => std::path::PathBuf::from(directory)
            .join(std::path::Path::new(binary).file_name().unwrap()),
        None => binary.into(),
    }
}

async fn cli(binary: &str, args: &[&str], code: i32) -> Output {
    cli_input(binary, args, None, code).await
}

async fn cli_input(binary: &str, args: &[&str], input: Option<&[u8]>, code: i32) -> Output {
    let mut child = tokio::process::Command::new(binary_path(binary));
    child
        .args(args)
        .kill_on_drop(true)
        .env_remove("HUDSONBOX_CONFIG")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy");
    use std::process::Stdio;
    child
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(15), async {
        let mut child = child.spawn().unwrap();
        if let Some(input) = input {
            let mut stdin = child.stdin.take().unwrap();
            if let Err(error) = stdin.write_all(input).await {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
                assert_ne!(code, 0);
            }
        }
        child.wait_with_output().await
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout)
            .chars()
            .take(2000)
            .collect::<String>(),
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(2000)
            .collect::<String>()
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-private-token"));
    output
}

struct Reply {
    status: &'static str,
    body: Value,
    headers: &'static str,
}
impl Reply {
    fn ok(body: Value) -> Self {
        Self {
            status: "200 OK",
            body,
            headers: "",
        }
    }
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Value,
}
type Requests = Arc<Mutex<Vec<Request>>>;

async fn capture_request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, requests: Requests) {
    let mut bytes = Vec::new();
    let end = loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() <= 65536);
        if bytes.ends_with(b"\r\n\r\n") {
            break bytes.len();
        }
    };
    let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_owned();
    let path = first.next().unwrap().to_owned();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_lowercase(), value.trim().to_owned()))
        .collect();
    let size = headers
        .get("content-length")
        .map_or(0, |size| size.parse::<usize>().unwrap());
    assert!(size <= 65536);
    let mut body = vec![0; size];
    stream.read_exact(&mut body).await.unwrap();
    requests.lock().unwrap().push(Request {
        method,
        path,
        headers,
        body: if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        },
    });
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    reply: Reply,
    requests: Requests,
) {
    capture_request(&mut stream, requests).await;
    let body = serde_json::to_vec(&reply.body).unwrap();
    let header = format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n", reply.status, body.len(), reply.headers);
    stream.write_all(header.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    stream.shutdown().await.unwrap();
}

struct Server {
    url: String,
    requests: Requests,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn raw_http(response: Vec<u8>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Requests::default();
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
                assert!(head.len() <= 65536);
            }
            let head = String::from_utf8(head).unwrap();
            let mut first = head.lines().next().unwrap().split_whitespace();
            captured.lock().unwrap().push(Request {
                method: first.next().unwrap().to_owned(),
                path: first.next().unwrap().to_owned(),
                headers: BTreeMap::new(),
                body: Value::Null,
            });
            // Clients can reject a response before consuming its body.
            let _ = stream.write_all(&response).await;
            let _ = stream.shutdown().await;
        });
        Self {
            url,
            requests,
            task,
        }
    }

    async fn http(replies: Vec<Reply>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Requests::default();
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::time::timeout(
                    Duration::from_secs(10),
                    exchange(stream, reply, captured.clone()),
                )
                .await
                .unwrap();
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }
}

#[tokio::test]
async fn help_and_versions_are_offline_for_all_entrypoints() {
    for (binary, args) in [
        (HUDSON, vec!["--help"]),
        (HUDSON, vec!["agent", "--help"]),
        (HUDSON, vec!["sandbox", "--help"]),
        (HUDSON, vec!["start", "--help"]),
        (HUDSON, vec!["health", "--help"]),
        (AGENT, vec!["--help"]),
        (SANDBOX, vec!["--help"]),
    ] {
        let output = cli(binary, &args, 0).await;
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    let help = cli(HUDSON, &["--help"], 0).await;
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("agent") && help.contains("sandbox"));
    for binary in [HUDSON, AGENT, SANDBOX] {
        let output = cli(binary, &["--version"], 0).await;
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION")));
    }
}

#[tokio::test]
async fn shell_completion_discovers_products_and_options_offline() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let output = cli(HUDSON, &["completion", shell], 0).await;
        let script = String::from_utf8(output.stdout).unwrap();
        for item in ["hudson", "agent", "sandbox", "key-file", "agent-version"] {
            assert!(script.contains(item), "missing {item} for {shell}");
        }
        assert!(output.stderr.is_empty());
    }
    cli(HUDSON, &["completion", "unsupported-shell"], 2).await;
}

#[tokio::test]
async fn agent_pipeline_preserves_explicit_json_values() {
    let server = Server::http(vec![Reply::ok(json!({"run_id":RUN}))]).await;
    let input = br#"{"task":"@literal-value","items":[1,true]}"#;
    cli_input(
        HUDSON,
        &[
            "agent",
            "--url",
            &server.url,
            "start",
            "--input-file",
            "-",
            "--request-key",
            "stdin-agent-key-0001",
        ],
        Some(input),
        0,
    )
    .await;
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].body["input"],
        serde_json::from_slice::<Value>(input).unwrap()
    );
}

#[tokio::test]
async fn agent_namespace_and_legacy_commands_preserve_requests() {
    let server = Server::http(vec![
        Reply::ok(json!({"status":"ok"})),
        Reply::ok(json!({"run_id":RUN})),
        Reply::ok(json!({"id":"analyst","version":7})),
        Reply::ok(json!({"status":"ok"})),
    ])
    .await;
    cli(HUDSON, &["agent", "--url", &server.url, "health"], 0).await;
    let output = cli(
        HUDSON,
        &[
            "agent",
            "--url",
            &server.url,
            "start",
            "--task",
            "hello",
            "--agent",
            "analyst",
            "--agent-version",
            "7",
            "--request-key",
            "stable-start-key",
        ],
        0,
    )
    .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["run_id"],
        RUN
    );
    cli(HUDSON, &["--url", &server.url, "agent", "analyst", "7"], 0).await;
    cli(AGENT, &["--url", &server.url, "health"], 0).await;
    let requests = server.requests.lock().unwrap();
    assert_eq!(
        (requests[0].method.as_str(), requests[0].path.as_str()),
        ("GET", "/health")
    );
    assert_eq!(
        (requests[1].method.as_str(), requests[1].path.as_str()),
        ("POST", "/runs")
    );
    assert_eq!(
        requests[1].body,
        json!({"input":"hello","request_key":"stable-start-key","agent_ref":{"id":"analyst","version":7}})
    );
    assert_eq!(requests[2].path, "/agents/analyst/versions/7");
    assert!(!requests[0].headers.contains_key("authorization"));
    assert_eq!(requests.len(), 4);
}

#[tokio::test]
async fn agent_api_failures_do_not_pollute_stdout() {
    for binary in [HUDSON, AGENT] {
        let server = Server::http(vec![Reply {
            status: "401 Unauthorized",
            body: json!({"error":"fixture-private-token"}),
            headers: "",
        }])
        .await;
        let mut args = vec!["--url", server.url.as_str(), "health"];
        if binary == HUDSON {
            args.insert(0, "agent");
        }
        let output = cli(binary, &args, 1).await;
        assert!(
            output.stdout.is_empty(),
            "failed API body was printed to stdout"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("401"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn agent_execution_errors_are_json_only_when_requested() {
    let server = Server::http(vec![
        Reply {
            status: "409 Conflict",
            body: json!({"error":"fixture-private-token"}),
            headers: "",
        },
        Reply::ok(json!({"status":"ok"})),
    ])
    .await;
    let output = cli(
        HUDSON,
        &["agent", "--url", &server.url, "health", "--json"],
        1,
    )
    .await;
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"], "http");
    assert_eq!(error["http_status"], 409);
    assert!(error["message"].as_str().unwrap().contains("409"));
    let output = cli(AGENT, &["--url", &server.url, "--json", "health"], 0).await;
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"status":"ok"})
    );
}

#[tokio::test]
async fn agent_error_status_survives_non_json_bodies_and_transport_errors_hide_urls() {
    let response = b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Type: text/html\r\n\r\n<html>fixture-private-token</html>".to_vec();
    let server = Server::raw_http(response).await;
    let output = cli(
        HUDSON,
        &["agent", "--url", &server.url, "--json", "health"],
        1,
    )
    .await;
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["http_status"], 503);
    assert_eq!(error["error"], "http");
    let server = Server::raw_http(Vec::new()).await;
    let url = format!("{}/fixture-private-token", server.url);
    let output = cli(HUDSON, &["agent", "--url", &url, "--json", "health"], 1).await;
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"], "transport");
    assert!(error["http_status"].is_null());
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&server.url));
}

#[tokio::test]
async fn agent_large_responses_are_rejected_before_output() {
    let body = serde_json::to_vec(&json!({"data":"x".repeat(8 * 1024 * 1024)})).unwrap();
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n".to_vec();
    response.extend(body);
    let server = Server::raw_http(response).await;
    let output = cli(
        HUDSON,
        &["agent", "--url", &server.url, "--json", "health"],
        1,
    )
    .await;
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"], "protocol");
    assert!(error["message"].as_str().unwrap().contains("8 MiB"));
}

#[tokio::test]
async fn agent_response_bounds_cover_advertised_lengths_and_invalid_json() {
    for (response, expected) in [
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n{}".to_vec(),
            "8 MiB",
        ),
        (
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{invalid fixture-private-token".to_vec(),
            "invalid JSON",
        ),
    ] {
        let server = Server::raw_http(response).await;
        let output = cli(
            HUDSON,
            &["agent", "--url", &server.url, "--json", "health"],
            1,
        )
        .await;
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"], "protocol");
        assert!(error["message"].as_str().unwrap().contains(expected));
    }
}

#[tokio::test]
async fn agent_closed_output_pipe_returns_an_error_without_panicking() {
    use std::process::Stdio;
    for binary in [HUDSON, AGENT] {
        let server = Server::http(vec![Reply::ok(json!({"status":"ok"}))]).await;
        let mut command = tokio::process::Command::new(binary_path(binary));
        if binary == HUDSON {
            command.arg("agent");
        }
        let mut child = command
            .args(["--url", &server.url, "--json", "health"])
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"], "client");
        assert!(error["message"].as_str().unwrap().contains("output"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[tokio::test]
async fn agent_command_routes_and_payloads_are_preserved() {
    let operation = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let directory = tempfile::tempdir().unwrap();
    let tool_file = directory.path().join("tool.json");
    let agent_file = directory.path().join("agent.json");
    let tool_body =
        json!({"request_key":"tool-publication-001","tool":{"id":"search","version":1}});
    let agent_body =
        json!({"request_key":"agent-publication-001","agent":{"id":"analyst","version":1}});
    std::fs::write(&tool_file, serde_json::to_vec(&tool_body).unwrap()).unwrap();
    std::fs::write(&agent_file, serde_json::to_vec(&agent_body).unwrap()).unwrap();
    let cases = vec![
        (
            vec!["capabilities"],
            "GET",
            "/capabilities".to_owned(),
            Value::Null,
        ),
        (
            vec!["publish-tool", tool_file.to_str().unwrap()],
            "POST",
            "/tools".to_owned(),
            tool_body,
        ),
        (
            vec!["publish-agent", agent_file.to_str().unwrap()],
            "POST",
            "/agents".to_owned(),
            agent_body,
        ),
        (
            vec!["tool", "search", "1"],
            "GET",
            "/tools/search/versions/1".to_owned(),
            Value::Null,
        ),
        (
            vec!["agent", "analyst", "1"],
            "GET",
            "/agents/analyst/versions/1".to_owned(),
            Value::Null,
        ),
        (
            vec![
                "steer",
                RUN,
                "--text",
                "direction",
                "--request-key",
                "direction-001",
            ],
            "POST",
            format!("/runs/{RUN}/messages"),
            json!({"kind":"steer","input":"direction","request_key":"direction-001"}),
        ),
        (
            vec![
                "follow-up",
                RUN,
                "--text",
                "next task",
                "--request-key",
                "followup-001",
            ],
            "POST",
            format!("/runs/{RUN}/messages"),
            json!({"kind":"follow_up","input":"next task","request_key":"followup-001"}),
        ),
        (
            vec!["inbox", RUN],
            "GET",
            format!("/runs/{RUN}/messages"),
            Value::Null,
        ),
        (
            vec!["clear-inbox", RUN],
            "DELETE",
            format!("/runs/{RUN}/messages"),
            Value::Null,
        ),
        (
            vec![
                "reply",
                RUN,
                "--question-id",
                "7",
                "--text",
                "answer",
                "--request-key",
                "reply-001",
            ],
            "POST",
            format!("/runs/{RUN}/input"),
            json!({"question_id":7,"input":"answer","request_key":"reply-001"}),
        ),
        (
            vec!["resume", RUN],
            "POST",
            format!("/runs/{RUN}/resume"),
            Value::Null,
        ),
        (vec!["get", RUN], "GET", format!("/runs/{RUN}"), Value::Null),
        (
            vec!["children", RUN],
            "GET",
            format!("/runs/{RUN}/children"),
            Value::Null,
        ),
        (
            vec!["operation", operation],
            "GET",
            format!("/operations/{operation}"),
            Value::Null,
        ),
        (
            vec!["events", RUN, "--after", "42"],
            "GET",
            format!("/runs/{RUN}/events?after=42"),
            Value::Null,
        ),
        (
            vec!["approve", operation],
            "POST",
            format!("/operations/{operation}/approval"),
            json!({"approved":true}),
        ),
        (
            vec!["approve", operation, "--deny"],
            "POST",
            format!("/operations/{operation}/approval"),
            json!({"approved":false}),
        ),
        (
            vec!["cancel", RUN],
            "POST",
            format!("/runs/{RUN}/cancel"),
            Value::Null,
        ),
    ];
    let server = Server::http(
        cases
            .iter()
            .map(|_| Reply::ok(json!({"ok":true})))
            .collect(),
    )
    .await;
    for (arguments, _, _, _) in &cases {
        let mut args = vec!["agent", "--url", server.url.as_str()];
        args.extend(arguments);
        cli(HUDSON, &args, 0).await;
    }
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), cases.len());
    for (request, (_, method, path, body)) in requests.iter().zip(cases) {
        assert_eq!(request.method, method);
        assert_eq!(request.path, path);
        assert_eq!(request.body, body);
    }
}

#[tokio::test]
async fn invalid_commands_fail_before_io() {
    // Parse errors must not reinterpret invalid product arguments as another request.
    cli(
        HUDSON,
        &["agent", "start", "--agent", "analyst", "--task", "hello"],
        2,
    )
    .await;
    cli(HUDSON, &["sandbox", "destroy", RUN], 2).await;
    cli(HUDSON, &["unknown-product"], 2).await;
}

#[tokio::test]
async fn sandbox_key_is_shared_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("mutation.key");
    let key = key.to_str().unwrap();
    let output = cli(HUDSON, &["sandbox", "--json", "key", "--to", key], 0).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({"saved":true})
    );
    let saved = std::fs::read(key).unwrap();
    assert_eq!(saved.last(), Some(&b'\n'));
    cli(SANDBOX, &["--json", "key", "--to", key], 1).await;
    assert_eq!(std::fs::read(key).unwrap(), saved);
}

#[cfg(unix)]
mod sandbox_https {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, path::Path};
    use tokio_rustls::{
        rustls::{self, pki_types::PrivatePkcs8KeyDer},
        TlsAcceptor,
    };

    fn private(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    struct Profile {
        server: Server,
        config: String,
        _dir: tempfile::TempDir,
    }
    enum Response {
        Json(Reply),
        Stream(String),
        Disconnect,
    }
    impl Profile {
        async fn new(replies: Vec<Reply>) -> Self {
            Self::responses(replies.into_iter().map(Response::Json).collect()).await
        }

        async fn responses(replies: Vec<Response>) -> Self {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
            let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
            let tls = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert.cert.der().clone()], key.into())
                .unwrap();
            let acceptor = TlsAcceptor::from(Arc::new(tls));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!(
                "https://localhost:{}",
                listener.local_addr().unwrap().port()
            );
            let requests = Requests::default();
            let captured = requests.clone();
            let task = tokio::spawn(async move {
                for reply in replies {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut stream = acceptor.accept(stream).await.unwrap();
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        async {
                            match reply {
                                Response::Json(reply) => exchange(stream, reply, captured.clone()).await,
                                Response::Stream(body) => {
                                    capture_request(&mut stream, captured.clone()).await;
                                    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                                    stream.write_all(header.as_bytes()).await.unwrap();
                                    stream.write_all(body.as_bytes()).await.unwrap();
                                    stream.shutdown().await.unwrap();
                                }
                                Response::Disconnect => {
                                    capture_request(&mut stream, captured.clone()).await;
                                    stream.shutdown().await.unwrap();
                                }
                            }
                        },
                    )
                    .await
                    .unwrap();
                }
            });
            let dir = tempfile::tempdir().unwrap();
            private(&dir.path().join("ca.pem"), cert.cert.pem().as_bytes());
            private(&dir.path().join("credential.json"), &serde_json::to_vec(&json!({"version":1,"project_id":"prj_fixture","name":"fixture","token":"fixture-private-token","created_at":1,"expires_at":4102444800_i64})).unwrap());
            let config = dir.path().join("config.json");
            private(&config, &serde_json::to_vec(&json!({"version":1,"endpoint":url,"credential_file":"credential.json","ca_file":"ca.pem","request_timeout_seconds":2})).unwrap());
            Self {
                server: Server {
                    url,
                    requests,
                    task,
                },
                config: config.to_str().unwrap().to_owned(),
                _dir: dir,
            }
        }
    }

    fn end_frame() -> String {
        format!(
            "event: end\nid: final-cursor\ndata: {}\n\n",
            json!({
                "reason":"complete", "stdout":{"seen":1,"stored":1,"truncated":false},
                "stderr":{"seen":0,"stored":0,"truncated":false},
                "simulated":true,"guest_reported":true
            })
        )
    }

    #[tokio::test]
    async fn stream_retries_connection_failures_without_losing_the_committed_cursor() {
        for (binary, prefix) in [(HUDSON, vec!["sandbox"]), (SANDBOX, vec![])] {
            for resumed in [false, true] {
                let mut replies = Vec::new();
                if resumed {
                    replies.push(Response::Stream(format!(
                        "event: output\nid: committed+/?=&\ndata: {}\n\nevent: output\nid: partial\ndata:",
                        json!({"stream":"stdout","offset":0,"next_offset":1,"data_base64":"YQ==",
                            "at_end":true,"complete":true,"seen":1,"stored":1,"truncated":false,
                            "simulated":true,"guest_reported":true})
                    )));
                }
                replies.extend([Response::Disconnect, Response::Stream(end_frame())]);
                let p = Profile::responses(replies).await;
                let mut args = prefix.clone();
                args.extend([
                    "--config",
                    &p.config,
                    "--json",
                    "stream",
                    "op_fixture",
                    "--cursor",
                    "saved+/?=&",
                    "--reconnects",
                    "2",
                    "--seconds",
                    "5",
                ]);
                let output = cli(binary, &args, 0).await;
                assert!(output.stderr.is_empty());
                let records: Vec<Value> = String::from_utf8(output.stdout)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
                assert_eq!(records.len(), if resumed { 2 } else { 1 });
                assert_eq!(records.last().unwrap()["event"], "end");
                if resumed {
                    assert_eq!(records[0]["event"], "output");
                    assert_eq!(records[0]["cursor"], "committed+/?=&");
                }
                let requests = p.server.requests.lock().unwrap();
                assert_eq!(requests.len(), if resumed { 3 } else { 2 });
                for (index, request) in requests.iter().enumerate() {
                    assert_eq!(request.method, "GET");
                    let url =
                        reqwest::Url::parse(&format!("https://fixture{}", request.path)).unwrap();
                    assert_eq!(url.path(), "/v1/operations/op_fixture/stream");
                    let query: BTreeMap<_, _> = url.query_pairs().collect();
                    assert_eq!(
                        query["cursor"],
                        if resumed && index > 0 {
                            "committed+/?=&"
                        } else {
                            "saved+/?=&"
                        }
                    );
                    assert_eq!(
                        request.headers["authorization"],
                        "Bearer fixture-private-token"
                    );
                    assert_eq!(request.body, Value::Null);
                }
            }
        }
    }

    #[tokio::test]
    async fn stream_connection_retries_are_bounded_and_permanent_errors_are_not_retried() {
        for (responses, expected_code, expected_requests, kind) in [
            (
                vec![Response::Disconnect, Response::Disconnect],
                7,
                2,
                "transport",
            ),
            (
                vec![Response::Json(Reply {
                    status: "401 Unauthorized",
                    body: json!({}),
                    headers: "",
                })],
                8,
                1,
                "http",
            ),
            (
                vec![Response::Stream("event: output\ndata: invalid\n\n".into())],
                1,
                1,
                "protocol",
            ),
        ] {
            let p = Profile::responses(responses).await;
            let output = cli(
                HUDSON,
                &[
                    "sandbox",
                    "--config",
                    &p.config,
                    "--json",
                    "stream",
                    "op_fixture",
                    "--reconnects",
                    "1",
                    "--seconds",
                    "5",
                ],
                expected_code,
            )
            .await;
            assert!(output.stdout.is_empty());
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"],
                kind
            );
            assert_eq!(p.server.requests.lock().unwrap().len(), expected_requests);
        }
    }

    #[tokio::test]
    async fn sandbox_namespace_and_alias_preserve_tls_keys_handles_and_exit_status() {
        let admitted = json!({"sandbox_id":"sbx_fixture","operation_id":"op_fixture","status":"admitted","status_url":"/v1/operations/op_fixture"});
        let failed = json!({"operation_id":"op_fixture","sandbox_id":"sbx_fixture","kind":"execute","status":"failed","result":{"exit_code":23},"error":null,"created_at":"2026-10-23T00:00:00Z"});
        let p = Profile::new(vec![
            Reply::ok(json!({"items":[],"next_cursor":null})),
            Reply {
                status: "202 Accepted",
                body: admitted.clone(),
                headers: "",
            },
            Reply::ok(failed),
        ])
        .await;
        cli(
            HUDSON,
            &[
                "sandbox",
                "--config",
                &p.config,
                "--json",
                "list",
                "--limit",
                "2",
                "--cursor",
                "next_cursor",
            ],
            0,
        )
        .await;
        let request = p._dir.path().join("execute.json");
        private(&request, &serde_json::to_vec(&json!({"argv":["true"],"deadline_unix_ms":4102444800000_i64,"expected_generation":7})).unwrap());
        let output = cli(
            HUDSON,
            &[
                "sandbox",
                "--config",
                &p.config,
                "--json",
                "execute",
                "sbx_fixture",
                "--request",
                request.to_str().unwrap(),
                "--key",
                "stable-mutation-key-0001",
            ],
            0,
        )
        .await;
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            admitted
        );
        cli(
            SANDBOX,
            &[
                "--config",
                &p.config,
                "--json",
                "wait",
                "op_fixture",
                "--seconds",
                "1",
            ],
            10,
        )
        .await;
        let requests = p.server.requests.lock().unwrap();
        assert!(requests[0].path.starts_with("/v1/sandboxes?"));
        assert!(
            requests[0].path.contains("limit=2") && requests[0].path.contains("cursor=next_cursor")
        );
        assert_eq!(requests[1].path, "/v1/sandboxes/sbx_fixture/execute");
        assert_eq!(
            requests[1].headers["idempotency-key"],
            "stable-mutation-key-0001"
        );
        assert_eq!(requests[1].body["expected_generation"], 7);
        assert_eq!(requests[1].body["argv"], json!(["true"]));
        assert_eq!(requests[2].path, "/v1/operations/op_fixture");
        assert!(requests
            .iter()
            .all(|r| r.headers["authorization"] == "Bearer fixture-private-token"));
        assert_eq!(requests.len(), 3);
    }

    #[tokio::test]
    async fn sandbox_pipeline_preserves_request_values_and_mutation_keys() {
        let admitted = json!({"sandbox_id":"sbx_fixture","operation_id":"op_fixture","status":"admitted","status_url":"/v1/operations/op_fixture"});
        let p = Profile::new(vec![
            Reply {
                status: "202 Accepted",
                body: admitted.clone(),
                headers: "",
            },
            Reply {
                status: "202 Accepted",
                body: admitted,
                headers: "",
            },
        ])
        .await;
        let create = json!({"image_digest":format!("sha256:{}", "a".repeat(64)),"name":"@literal-file-name","resources":{"vcpu":1,"memory_mib":128,"disk_mib":64}});
        cli_input(
            HUDSON,
            &[
                "sandbox",
                "--config",
                &p.config,
                "--json",
                "create",
                "--request",
                "-",
                "--key",
                "stdin-create-key-0001",
            ],
            Some(&serde_json::to_vec(&create).unwrap()),
            0,
        )
        .await;
        let execute = json!({"argv":["printf","@literal-value"],"deadline_unix_ms":4102444800000_i64,"expected_generation":4});
        cli_input(
            SANDBOX,
            &[
                "--config",
                &p.config,
                "--json",
                "execute",
                "sbx_fixture",
                "--request",
                "-",
                "--key",
                "stdin-execute-key-0001",
            ],
            Some(&serde_json::to_vec(&execute).unwrap()),
            0,
        )
        .await;
        let requests = p.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/v1/sandboxes");
        assert_eq!(requests[0].body["name"], "@literal-file-name");
        assert_eq!(
            requests[0].headers["idempotency-key"],
            "stdin-create-key-0001"
        );
        assert_eq!(requests[1].body["argv"], execute["argv"]);
        assert_eq!(requests[1].body["expected_generation"], 4);
        assert_eq!(
            requests[1].headers["idempotency-key"],
            "stdin-execute-key-0001"
        );
    }

    #[tokio::test]
    async fn invalid_or_oversized_piped_requests_fail_before_sending() {
        let p = Profile::new(vec![Reply::ok(json!({}))]).await;
        let mut body = json!({"image_digest":format!("sha256:{}", "a".repeat(64)),"name":"","resources":{"vcpu":1,"memory_mib":128,"disk_mib":64}});
        let overhead = serde_json::to_vec(&body).unwrap().len();
        body["name"] = json!("x".repeat(65537 - overhead));
        let large = serde_json::to_vec(&body).unwrap();
        assert_eq!(large.len(), 65537);
        for input in [b"{".as_slice(), large.as_slice()] {
            let output = cli_input(
                HUDSON,
                &[
                    "sandbox",
                    "--config",
                    &p.config,
                    "--json",
                    "create",
                    "--request",
                    "-",
                    "--key",
                    "stdin-invalid-key-0001",
                ],
                Some(input),
                1,
            )
            .await;
            assert!(output.stdout.is_empty());
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"],
                "request"
            );
        }
        assert!(p.server.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn api_problem_is_machine_readable_and_not_retried() {
        let p = Profile::new(vec![Reply {status:"409 Conflict", body:json!({"title":"Conflict","status":409,"code":"sandbox_generation_mismatch","operation_id":"op_fixture"}), headers:""}]).await;
        let output = cli(
            HUDSON,
            &[
                "sandbox",
                "--config",
                &p.config,
                "--json",
                "destroy",
                "sbx_fixture",
                "--key",
                "stable-mutation-key-0002",
            ],
            8,
        )
        .await;
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["http_status"], 409);
        assert_eq!(error["code"], "sandbox_generation_mismatch");
        assert_eq!(p.server.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sandbox_redirect_is_rejected_without_forwarding_credentials() {
        let p = Profile::new(vec![Reply {
            status: "307 Temporary Redirect",
            body: json!({}),
            headers: "Location: https://example.invalid/steal\r\n",
        }])
        .await;
        cli(
            HUDSON,
            &["sandbox", "--config", &p.config, "--json", "list"],
            8,
        )
        .await;
        assert_eq!(p.server.requests.lock().unwrap().len(), 1);
    }
}

#[cfg(windows)]
#[tokio::test]
async fn sandbox_profile_loading_fails_closed_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.json");
    std::fs::write(&config, "{}").unwrap();
    let output = cli(
        HUDSON,
        &[
            "sandbox",
            "--config",
            config.to_str().unwrap(),
            "--json",
            "list",
        ],
        1,
    )
    .await;
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"], "configuration");
}
