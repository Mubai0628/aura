use super::support::*;
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};

struct LocalHttp {
    url: String,
    hits: Arc<AtomicUsize>,
    ended: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl LocalHttp {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("local test service");
        listener.set_nonblocking(true).expect("bounded local worker");
        let url = format!("http://{}/proof", listener.local_addr().expect("local address"));
        let hits = Arc::new(AtomicUsize::new(0));
        let ended = Arc::new(AtomicBool::new(false));
        let observed = hits.clone();
        let stop = ended.clone();
        let worker = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).expect("bounded blocking local request");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .expect("bound local request");
                        let mut request = [0; 4096];
                        if stream.read(&mut request).is_ok() {
                            observed.fetch_add(1, Ordering::AcqRel);
                            let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Self { url, hits, ended, worker: Some(worker) }
    }
    fn count(&self) -> usize {
        self.hits.load(Ordering::Acquire)
    }
}
impl Drop for LocalHttp {
    fn drop(&mut self) {
        self.ended.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("local service ends");
        }
    }
}

async fn call(f: &mut Fixture, tool: &str, network: bool, approve_network: bool) -> String {
    let batch = f.admit(&[(tool, arguments("node request.cjs", network))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    if approve_network {
        approve(f, &batch.calls[&id]).await;
    }
    let group = f.execute(batch).await;
    assert_eq!(f.output(id)["exit_code"], 0, "the actual local Node HTTP client completes");
    let view = aura_persistence_v2::execution::shell::query(
        f.service().broker.lock().expect("journal").database.connection(),
        f.turn.session_id,
        &aura_windows_user_contract::ShellQueryInput {
            tool_call_id: id,
            stdout_offset: 0,
            stderr_offset: 0,
            max_bytes: 4096,
        },
    )
    .expect("actual output");
    let stdout = String::from_utf8(view.stdout.clone()).expect("Node's ASCII response status");
    assert!(!stdout.is_empty(), "the real Node response is observable: {view:?}");
    f.continue_after_tools(batch_id, &group);
    stdout
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_node_http_obeys_the_soft_default_proxy_and_the_separate_network_answer() {
    let service = LocalHttp::start();
    for tool in ["shell.powershell", "shell.cmd"] {
        let mut f=Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite,&[tool],|root|{
            std::fs::write(root.join("request.cjs"),format!("require('http').get({:?},r=>{{console.log(r.statusCode);r.resume();}}).on('error',e=>{{console.error(e.message);process.exitCode=1;}});",service.url)).expect("local client fixture");
        }).await;
        let before = service.count();
        let offline = call(&mut f, tool, false, false).await;
        assert!(offline.contains("403"), "{tool} uses the local reject proxy: {offline:?}");
        assert_eq!(
            service.count(),
            before,
            "soft offline reaches no local target in this Node version"
        );
        let enabled = call(&mut f, tool, true, true).await;
        assert!(enabled.contains("200"));
        assert_eq!(service.count(), before + 1);
        let reused = call(&mut f, tool, true, false).await;
        assert!(reused.contains("200"));
        assert_eq!(service.count(), before + 2);
        let rows = crate::r03_tests::success(
            &f.runtime,
            "security.network_session_consent.list",
            json!({"session_id":f.turn.session_id,"include_inactive":false}),
        )
        .await;
        let row = &rows.as_array().expect("network answers")[0];
        crate::r03_tests::success(&f.runtime,"security.network_session_consent.revoke",json!({"session_id":f.turn.session_id,"grant_id":row["grant_id"],"expected_revision":row["grant_revision"]})).await;
        let offline = call(&mut f, tool, false, false).await;
        assert!(offline.contains("403"));
        assert_eq!(service.count(), before + 2);
        mode(&f, PermissionMode::FullAccess).await;
        let full = call(&mut f, tool, false, false).await;
        assert!(full.contains("200"), "FullAccess enables networking even without a declaration");
        assert_eq!(service.count(), before + 3);
        println!(
            "S08_NODE_HTTP {tool}: soft default=403/0 target hits, session enabled=200, reuse=200, revoked new child=403, FullAccess default=200"
        );
    }
}
