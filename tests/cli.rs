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

async fn cli(binary: &str, args: &[&str], code: i32) -> Output {
    let mut child = tokio::process::Command::new(binary);
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
    let output = tokio::time::timeout(Duration::from_secs(15), child.output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    reply: Reply,
    requests: Requests,
) {
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
    impl Profile {
        async fn new(replies: Vec<Reply>) -> Self {
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
                    let stream = acceptor.accept(stream).await.unwrap();
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        exchange(stream, reply, captured.clone()),
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
