//! Core's side of Chrome control against an extension double that speaks through the real relay
//! code over the real named pipe; this test process is both the relay and the Core it checks.
use super::test_extension::Extension;
use super::*;
use aura_chrome_domain::protocol::{
    AuthRefusal, CoreMessage, ExtensionMessage, PairRefusal, TEST_EXTENSION_ID,
};
use aura_config::{AuraPaths, SettingsFacade};
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};

mod lock;

struct Fixture {
    control: ChromeControl,
    secrets: Arc<MemoryChromeSecrets>,
    pipe: String,
    _folder: tempfile::TempDir,
}

fn unique() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!("{}x{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn fixture_with(adjust: impl FnOnce(&mut ChromeControlOptions)) -> Fixture {
    let folder = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(folder.path().join("config"), folder.path().join("data"));
    let settings = SettingsFacade::open(paths).expect("settings");
    let id = unique();
    let mut options = ChromeControlOptions {
        host_name: format!("com.aura.chrome_control.tcore{}", id.replace('x', "_")),
        manifest_path: folder.path().join("chrome").join("manifest.json"),
        relay_executable: std::env::current_exe().expect("this test is the relay"),
        pipe_name: format!(r"\\.\pipe\aura-chrome-core-test-{id}"),
        extension_id: TEST_EXTENSION_ID.to_owned(),
        pairing_valid_for: Duration::from_secs(300),
        heartbeat: Duration::from_secs(30),
        register: false,
    };
    adjust(&mut options);
    let pipe = options.pipe_name.clone();
    let secrets = Arc::new(MemoryChromeSecrets::default());
    let control =
        ChromeControl::new(options, settings, Arc::clone(&secrets) as Arc<dyn ChromeSecrets>);
    Fixture { control, secrets, pipe, _folder: folder }
}

impl Fixture {
    async fn enable(&self) {
        let revision = self.control.status().revision.expect("settings revision");
        self.control.set_enabled(revision, true).await.expect("Chrome control on");
    }

    async fn until(&self, state: ChromeConnectionState) {
        self.control.until(state).await;
    }

    async fn paired(&self) -> (Extension, Vec<u8>) {
        self.control.pair_extension().await
    }
}

#[tokio::test]
async fn pairing_gives_a_secret_once_and_every_later_connection_proves_it() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (extension, secret) = fixture.paired().await;
    let status = fixture.control.status();
    assert_eq!(status.paired.expect("paired").extension_id, TEST_EXTENSION_ID);
    assert_eq!(fixture.secrets.len(), 1, "the secret is kept by Core");
    drop(extension);
    fixture.until(ChromeConnectionState::Waiting).await;
    let mut again = Extension::connect(&fixture.pipe).await;
    let challenge = again.hello().await.expect("challenge");
    assert!(matches!(challenge, CoreMessage::Challenge { .. }), "no second pairing: {challenge:?}");
    assert_eq!(again.authenticate(challenge, &secret).await.0, Some(CoreMessage::Authenticated));
}

#[tokio::test]
async fn a_wrong_an_exhausted_and_an_expired_code_pair_nothing() {
    let fixture = fixture_with(|options| options.pairing_valid_for = Duration::from_millis(400));
    fixture.enable().await;
    let code = fixture.control.begin_pairing().expect("code").code;
    let wrong = if code == "00000000" { "11111111" } else { "00000000" };
    let mut extension = Extension::connect(&fixture.pipe).await;
    assert_eq!(extension.hello().await, Some(CoreMessage::PairingRequired));
    let mut refusals = Vec::new();
    for typed in [wrong, wrong, wrong, wrong, wrong, &code] {
        extension.send(ExtensionMessage::Pair { code: typed.to_owned() }).await;
        refusals.push(extension.receive().await);
    }
    let refused = |reason| Some(CoreMessage::PairRefused { reason });
    assert_eq!(refusals[..4], vec![refused(PairRefusal::CodeInvalid); 4][..]);
    assert_eq!(refusals[4], refused(PairRefusal::TooManyAttempts));
    assert_eq!(refusals[5], refused(PairRefusal::TooManyAttempts), "even the right code now");
    let late = fixture.control.begin_pairing().expect("a new code").code;
    tokio::time::sleep(Duration::from_millis(500)).await;
    extension.send(ExtensionMessage::Pair { code: late }).await;
    assert_eq!(extension.receive().await, refused(PairRefusal::CodeExpired));
    assert!(fixture.control.status().paired.is_none());
    assert_eq!(fixture.secrets.len(), 0);
}

#[tokio::test]
async fn a_proof_for_an_earlier_challenge_is_refused_and_ends_the_connection() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (first, secret) = fixture.paired().await;
    drop(first);
    fixture.until(ChromeConnectionState::Waiting).await;
    let mut second = Extension::connect(&fixture.pipe).await;
    let challenge = second.hello().await.expect("challenge");
    let (answer, proof) = second.authenticate(challenge, &secret).await;
    assert_eq!(answer, Some(CoreMessage::Authenticated));
    drop(second);
    fixture.until(ChromeConnectionState::Waiting).await;
    let mut replay = Extension::connect(&fixture.pipe).await;
    assert!(matches!(replay.hello().await, Some(CoreMessage::Challenge { .. })));
    replay.send(ExtensionMessage::Authenticate { mac: proof }).await;
    assert_eq!(
        replay.receive().await,
        Some(CoreMessage::AuthRefused { reason: AuthRefusal::MacInvalid })
    );
    assert_eq!(replay.receive().await, None, "the connection ends");
}

#[tokio::test]
async fn revoking_tells_the_extension_forgets_the_secret_and_asks_for_pairing_again() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (mut extension, _) = fixture.paired().await;
    let status = fixture.control.revoke().await.expect("revoke");
    assert!(status.paired.is_none());
    assert_eq!(extension.receive().await, Some(CoreMessage::Revoked));
    assert_eq!(extension.receive().await, None, "and disconnects it");
    assert_eq!(fixture.secrets.len(), 0, "the secret is deleted");
    fixture.until(ChromeConnectionState::Waiting).await;
    let mut again = Extension::connect(&fixture.pipe).await;
    assert_eq!(again.hello().await, Some(CoreMessage::PairingRequired));
}

#[tokio::test]
async fn a_command_is_answered_or_cancelled_and_a_lost_connection_fails_it() {
    let fixture = fixture_with(|_| {});
    fixture.enable().await;
    let (mut extension, _) = fixture.paired().await;
    let control = fixture.control.clone();
    let answered = tokio::spawn(async move {
        let site = Some("https://shop.example".to_owned());
        control.request(json!({"op":"one"}), site, Duration::from_secs(5)).await
    });
    let Some(CoreMessage::Command { id, command, origin }) = extension.receive().await else {
        panic!("command");
    };
    assert_eq!(command, json!({"op":"one"}));
    assert_eq!(origin.as_deref(), Some("https://shop.example"), "the site it was authorized for");
    let done = CommandOutcome::Done { value: json!(42) };
    extension.send(ExtensionMessage::Result { id, outcome: done }).await;
    assert_eq!(answered.await.expect("join"), Ok(json!(42)));

    let control = fixture.control.clone();
    let late = tokio::spawn(async move {
        control.request(json!({"op":"slow"}), None, Duration::from_millis(200)).await
    });
    let Some(CoreMessage::Command { id, .. }) = extension.receive().await else {
        panic!("command")
    };
    assert_eq!(late.await.expect("join"), Err(ChromeRequestError::TimedOut));
    assert_eq!(extension.receive().await, Some(CoreMessage::Cancel { id }));

    let control = fixture.control.clone();
    let dropped = tokio::spawn(async move {
        control.request(json!({"op":"dropped"}), None, Duration::from_secs(30)).await
    });
    let Some(CoreMessage::Command { id, .. }) = extension.receive().await else {
        panic!("command")
    };
    dropped.abort();
    assert_eq!(extension.receive().await, Some(CoreMessage::Cancel { id }), "a caller that stops");

    let control = fixture.control.clone();
    let lost = tokio::spawn(async move {
        control.request(json!({"op":"lost"}), None, Duration::from_secs(30)).await
    });
    assert!(matches!(extension.receive().await, Some(CoreMessage::Command { .. })));
    drop(extension);
    assert_eq!(lost.await.expect("join"), Err(ChromeRequestError::Disconnected));
    assert_eq!(
        fixture.control.request(json!({}), None, Duration::from_secs(1)).await,
        Err(ChromeRequestError::NotConnected)
    );
}

#[tokio::test]
async fn an_unanswered_heartbeat_ends_the_connection() {
    let fixture = fixture_with(|options| options.heartbeat = Duration::from_millis(150));
    fixture.enable().await;
    let (mut extension, _) = fixture.paired().await;
    let Some(CoreMessage::Ping { id }) = extension.receive().await else { panic!("ping") };
    extension.send(ExtensionMessage::Pong { id }).await;
    assert!(matches!(extension.receive().await, Some(CoreMessage::Ping { .. })), "still alive");
    assert_eq!(extension.receive().await, None, "a missed beat ends it");
    fixture.until(ChromeConnectionState::Waiting).await;
}

#[tokio::test]
async fn a_pipe_client_that_is_not_the_relay_is_not_served() {
    let fixture = fixture_with(|options| {
        options.relay_executable = r"C:\Program Files\Elsewhere\aura-chrome-host.exe".into();
    });
    fixture.enable().await;
    let mut extension = Extension::connect(&fixture.pipe).await;
    assert_eq!(extension.hello().await, None, "Core closes without a word");
}

/// Removes a test registration from the user's registry whatever the test did.
struct Registration(String);

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = aura_windows_platform::installer::remove_chrome_native_host(&self.0);
    }
}

#[tokio::test]
async fn turning_chrome_control_on_registers_the_relay_and_off_forgets_everything() {
    let fixture = fixture_with(|options| options.register = true);
    let host = fixture.control.inner.options.host_name.clone();
    let manifest = fixture.control.inner.options.manifest_path.clone();
    let _cleanup = Registration(host.clone());
    fixture.enable().await;
    assert_eq!(
        aura_windows_platform::installer::read_chrome_native_host(&host).expect("registry"),
        Some(manifest.clone())
    );
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).expect("manifest")).expect("json");
    assert_eq!(written["path"], json!(std::env::current_exe().expect("relay")));
    assert_eq!(written["type"], "stdio");
    assert_eq!(
        written["allowed_origins"],
        json!([format!("chrome-extension://{TEST_EXTENSION_ID}/")])
    );
    assert!(fixture.control.status().registered);
    let (mut extension, _) = fixture.paired().await;
    let revision = fixture.control.status().revision.expect("revision");
    let status = fixture.control.set_enabled(revision, false).await.expect("off");
    assert!(!status.enabled && !status.registered && status.paired.is_none());
    assert_eq!(status.connection, ChromeConnectionState::Off);
    assert_eq!(extension.receive().await, Some(CoreMessage::Revoked));
    assert_eq!(fixture.secrets.len(), 0);
    assert_eq!(
        aura_windows_platform::installer::read_chrome_native_host(&host).expect("registry"),
        None
    );
    assert!(!manifest.exists());
}
