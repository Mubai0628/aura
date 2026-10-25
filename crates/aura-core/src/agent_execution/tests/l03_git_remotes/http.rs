//! A loopback-only CGI smart HTTP fixture. Its Git config and data are all temporary.
use super::super::l03_git_writes::result;
use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    url: String,
    pushes: Arc<std::sync::atomic::AtomicUsize>,
    first_request: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(root: &Path, deny: bool, drop_push_response: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback");
        let url = format!("http://{}/remote.git", listener.local_addr().expect("port"));
        let root = root.to_owned();
        let profile_path = std::env::var_os("AURA_GIT_TEST_PROFILE").expect("test feature profile");
        // MinGit is a client distribution and has no http-backend. The local test
        // server always uses installed Git, while every Aura client keeps the
        // selected (including 2.40) image from its own test profile.
        let server_runner = aura_backend_windows_user::git::GitRunner::isolated(
            Path::new("C:/Program Files/Git/cmd/git.exe"),
            Path::new(&profile_path),
        )
        .expect("isolated server Git");
        let image = server_runner.image_binding().path.clone();
        let profile: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&profile_path).expect("own profile"))
                .expect("profile");
        let pushes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = pushes.clone();
        let first_request = Arc::new(std::sync::Mutex::new(None));
        let accepted = first_request.clone();
        let task = tokio::spawn(async move {
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break };
                accepted
                    .lock()
                    .expect("request timestamp")
                    .get_or_insert_with(std::time::Instant::now);
                let root = root.clone();
                let image = image.clone();
                let profile = profile.clone();
                let counted = counted.clone();
                handlers.spawn(async move {
                    let mut input = vec![]; let mut block = [0u8; 8192];
                    let header_end = loop {
                        let n = stream.read(&mut block).await.expect("request"); if n == 0 { return; }
                        input.extend_from_slice(&block[..n]);
                        if let Some(end) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n") { break end + 4; }
                        assert!(input.len() < 65536);
                    };
                    if deny { let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await; return; }
                    let header = String::from_utf8(input[..header_end].to_vec()).expect("headers");
                    let length = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().expect("length"))).unwrap_or(0);
                    assert!(length < 16 * 1024 * 1024);
                    if header.to_ascii_lowercase().contains("expect: 100-continue") { stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.expect("continue"); }
                    while input.len() < header_end + length { let n = stream.read(&mut block).await.expect("body"); if n == 0 { return; } input.extend_from_slice(&block[..n]); }
                    let mut request = header.split_whitespace(); let method = request.next().expect("method").to_owned();
                    let target = request.next().expect("path"); let (path, query) = target.split_once('?').unwrap_or((target, ""));
                    let path = path.to_owned(); let query = query.to_owned();
                    let receiving = method == "POST" && path.ends_with("/git-receive-pack");
                    if receiving { counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
                    let content_type = header.lines().find_map(|line| line.strip_prefix("Content-Type: ")).unwrap_or("").to_owned();
                    let body = input[header_end..header_end + length].to_vec();
                    let response = tokio::task::spawn_blocking(move || {
                        use std::{io::Write, process::{Command, Stdio}};
                        let mut command = Command::new(image);
                        command.arg("http-backend").current_dir(&root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
                        for (name, _) in std::env::vars_os() { if name.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") { command.env_remove(name); } }
                        command.env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", profile["global_config"].as_str().expect("global")).env("GIT_CONFIG_SYSTEM", profile["system_config"].as_str().expect("system")).env("GIT_TERMINAL_PROMPT", "0").env("HOME", &root).env("XDG_CONFIG_HOME", &root)
                            .env("GIT_PROJECT_ROOT", &root).env("GIT_HTTP_EXPORT_ALL", "1").env("REQUEST_METHOD", method).env("PATH_INFO", path).env("QUERY_STRING", query).env("CONTENT_TYPE", content_type).env("CONTENT_LENGTH", body.len().to_string()).env("REMOTE_ADDR", "127.0.0.1");
                        let mut child = command.spawn().expect("temporary CGI");
                        child.stdin.take().expect("CGI input").write_all(&body).expect("CGI body");
                        let out = child.wait_with_output().expect("CGI output");
                        assert!(out.status.success(), "temporary smart HTTP backend: {}", String::from_utf8_lossy(&out.stderr)); out.stdout
                    }).await.expect("fixture CGI worker");
                    if receiving && drop_push_response { return; }
                    let end = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").expect("CGI response");
                    let headers = String::from_utf8_lossy(&response[..end]);
                    let status = headers.lines().find_map(|line| line.strip_prefix("Status: ")).unwrap_or("200 OK");
                    let mut output = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n", response.len() - end - 4);
                    for line in headers.lines().filter(|line| !line.starts_with("Status:")) { output.push_str(line); output.push_str("\r\n"); }
                    output.push_str("\r\n");
                    let _ = stream.write_all(output.as_bytes()).await;
                    let _ = stream.write_all(&response[end + 4..]).await;
                });
            }
        });
        Self { url, task, pushes, first_request }
    }
}

#[tokio::test]
async fn smart_http_push_and_fetch_use_native_git_without_aura_proxy() {
    let (temporary, _) = remote();
    git(&temporary.path().join("remote.git"), &["config", "http.receivepack", "true"]);
    let server = Server::start(temporary.path(), false, false).await;
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::FullAccess,
        &["git.push", "git.sync"],
        |root| {
            initialize(root);
            git(root, &["remote", "add", "origin", &server.url]);
        },
    )
    .await;
    let push = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = push.batch.calls[0].tool_call_id;
    let batch_id = push.batch.tool_batch_id;
    let group = f.execute(push).await;
    f.output(id);
    assert_eq!(oid(&temporary.path().join("remote.git"), "refs/heads/main"), oid(&f.root, "HEAD"));
    f.continue_after_tools(batch_id, &group);
    let fetch = f.admit(&[("git.sync", json!({"kind":"fetch","remote":"origin"}))]);
    let id = fetch.batch.calls[0].tool_call_id;
    f.execute(fetch).await;
    f.output(id);
}

#[tokio::test]
async fn authentication_failure_returns_quickly_without_prompting_or_exposing_url_secrets() {
    let (temporary, _) = remote();
    let server = Server::start(temporary.path(), true, false).await;
    let url = server.url.replacen("http://", "http://temporary-user:temporary-test-token@", 1);
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &url]);
    })
    .await;
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let frozen = serde_json::to_string(&batch.calls[&id].operation).expect("durable scope");
    assert!(!frozen.contains("temporary-test-token") && !frozen.contains("temporary-user"));
    tokio::time::timeout(Duration::from_secs(60), f.execute(batch))
        .await
        .expect("no credential prompt hang");
    assert!(
        server
            .first_request
            .lock()
            .expect("timestamp")
            .expect("loopback request was received")
            .elapsed()
            < Duration::from_secs(5),
        "authentication fails promptly after Git first connects; preparation time is outside this boundary"
    );
    let receipt = serde_json::to_string(&result(&f, id)).expect("receipt");
    assert!(receipt.contains("git.authentication_failed"), "{receipt}");
    assert!(!receipt.contains("temporary-test-token") && !receipt.contains("temporary-user"));
}

#[tokio::test]
async fn a_lost_push_response_can_follow_a_real_remote_effect_and_recovery_never_replays() {
    let (temporary, _) = remote();
    git(&temporary.path().join("remote.git"), &["config", "http.receivepack", "true"]);
    let server = Server::start(temporary.path(), false, true).await;
    let f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &server.url]);
    })
    .await;
    let expected = oid(&f.root, "HEAD");
    let batch = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let operation = batch.calls[&id].operation.operation_id;
    f.execute_rejecting(batch).await;
    assert_eq!(
        oid(&temporary.path().join("remote.git"), "refs/heads/main"),
        expected,
        "the server received the push before the response was lost"
    );
    let receipt = result(&f, id);
    assert!(matches!(receipt.outcome, ToolResultOutcomeV1::Indeterminate { .. }), "{receipt:?}");
    assert_eq!(receipt.side_effect_state, SideEffectStateV1::Unknown);
    assert!(
        serde_json::to_string(&receipt).expect("model-visible uncertainty").contains("远端可能"),
        "the model must be told that the remote may already have received the push: {receipt:?}"
    );
    let recovered =
        f.service().agent_execution_required().resume_batch(batch_id).expect("durable recovery");
    f.execute_rejecting(recovered).await;
    assert_eq!(
        server.pushes.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "recovery must not resend an unknown push"
    );
    let store = f.service().broker.lock().expect("attempt ledger");
    let count: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE operation_id=?1",
            [operation.to_string()],
            |row| row.get(0),
        )
        .expect("attempt count");
    assert_eq!(count, 1);
}
