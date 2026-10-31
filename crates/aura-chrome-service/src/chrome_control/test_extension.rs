//! An extension double that speaks to Core through the real relay code over the real named pipe;
//! the test process is both the relay and the Core it checks.
use super::*;
use aura_chrome_domain::commands::{ChromeCommand, ChromeShareView};
use aura_chrome_domain::protocol::{
    ExtensionMessage, MAX_FROM_CHROME_BYTES, MAX_TO_CHROME_BYTES, TEST_EXTENSION_ID,
    authentication_mac,
};
use aura_chrome_host::frame::{read_frame, write_frame};
use aura_chrome_host::relay::{RelayTarget, relay};
use base64::Engine as _;

const BASE64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// The extension end of the relay: what Chrome would write to and read from its stdio.
pub struct Extension {
    id: String,
    to_relay: tokio::io::DuplexStream,
    from_relay: tokio::io::DuplexStream,
    _relay: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Extension {
    /// The test extension, through the relay to `pipe`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn connect(pipe: &str) -> Self {
        Self::connect_as(pipe, TEST_EXTENSION_ID).await
    }

    pub(crate) async fn connect_as(pipe: &str, id: &str) -> Self {
        let target = RelayTarget {
            pipe_name: pipe.to_owned(),
            expected_server: std::env::current_exe().expect("this test is Core"),
        };
        let (to_relay, relay_reads) = tokio::io::duplex(1 << 20);
        let (relay_writes, from_relay) = tokio::io::duplex(1 << 20);
        let origin = format!("chrome-extension://{id}/");
        let relay =
            tokio::spawn(async move { relay(&origin, &target, relay_reads, relay_writes).await });
        Self { id: id.to_owned(), to_relay, from_relay, _relay: relay }
    }

    pub(crate) async fn send(&mut self, message: ExtensionMessage) {
        let bytes = serde_json::to_vec(&message).expect("json");
        write_frame(&mut self.to_relay, &bytes, MAX_FROM_CHROME_BYTES).await.expect("send");
    }

    /// The next message from Core, or `None` once Core closed the connection.
    async fn next(&mut self) -> Option<CoreMessage> {
        read_frame(&mut self.from_relay, MAX_TO_CHROME_BYTES)
            .await
            .ok()
            .flatten()
            .map(|frame| serde_json::from_slice(&frame).expect("a Core message"))
    }

    pub(crate) async fn receive(&mut self) -> Option<CoreMessage> {
        tokio::time::timeout(Duration::from_secs(10), self.next())
            .await
            .expect("Core answers in time")
    }

    pub(crate) async fn hello(&mut self) -> Option<CoreMessage> {
        self.send(ExtensionMessage::Hello {
            extension_id: self.id.clone(),
            extension_version: "1.0.0".to_owned(),
        })
        .await;
        self.receive().await
    }

    /// Answers `challenge` with `secret`; Core's answer and the proof sent.
    pub(crate) async fn authenticate(
        &mut self,
        challenge: CoreMessage,
        secret: &[u8],
    ) -> (Option<CoreMessage>, String) {
        let CoreMessage::Challenge { nonce, .. } = challenge else { panic!("{challenge:?}") };
        let nonce = BASE64.decode(nonce).expect("base64 nonce");
        let mac = authentication_mac(secret, &nonce, &self.id)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        self.send(ExtensionMessage::Authenticate { mac: mac.clone() }).await;
        (self.receive().await, mac)
    }
}

impl ChromeControl {
    pub(crate) fn pipe(&self) -> &str {
        &self.inner.options.pipe_name
    }

    pub async fn until(&self, state: ChromeConnectionState) {
        for _ in 0..500 {
            if self.status().connection == state {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("connection never became {state:?}: {:?}", self.status());
    }

    /// The extension this installation pairs with, paired through the whole exchange, with the
    /// secret it was given. Core's first word after the proof, the shares it holds, is read.
    pub async fn pair_extension(&self) -> (Extension, Vec<u8>) {
        let code = self.begin_pairing().expect("pairing code").code;
        let mut extension =
            Extension::connect_as(self.pipe(), &self.inner.options.extension_id).await;
        assert_eq!(extension.hello().await, Some(CoreMessage::PairingRequired));
        extension.send(ExtensionMessage::Pair { code }).await;
        let Some(CoreMessage::Paired { secret }) = extension.receive().await else {
            panic!("paired");
        };
        let secret = BASE64.decode(secret).expect("base64 secret");
        let challenge = extension.receive().await.expect("challenge");
        let (answer, _) = extension.authenticate(challenge, &secret).await;
        assert_eq!(answer, Some(CoreMessage::Authenticated));
        assert!(matches!(extension.receive().await, Some(CoreMessage::Shares { .. })));
        self.until(ChromeConnectionState::Connected).await;
        (extension, secret)
    }
}

/// How the played extension answers a command; `None` leaves it unanswered.
pub type Answers = Box<dyn Fn(&ChromeCommand) -> Option<CommandOutcome> + Send + Sync>;

enum Control {
    Send(ExtensionMessage),
    /// Drops the connection, as Chrome closing would.
    Disconnect,
}

/// A paired extension that answers on its own, keeps the heartbeat, records every command and
/// the shares Core last sent, and sends what the test hands it.
pub struct PlayedExtension {
    pub commands: Arc<Mutex<Vec<ChromeCommand>>>,
    /// The site each command was authorized for, in the same order.
    pub origins: Arc<Mutex<Vec<Option<String>>>>,
    pub shares: Arc<Mutex<Vec<ChromeShareView>>>,
    /// The commands Core withdrew, by the command each was.
    pub cancelled: Arc<Mutex<Vec<ChromeCommand>>>,
    /// The tabs the user finished with in Aura.
    pub assist_done: Arc<Mutex<Vec<u32>>>,
    control: mpsc::UnboundedSender<Control>,
}

impl PlayedExtension {
    pub fn play(mut extension: Extension, answers: Answers) -> Self {
        let commands = Arc::<Mutex<Vec<ChromeCommand>>>::default();
        let origins = Arc::<Mutex<Vec<Option<String>>>>::default();
        let shares = Arc::<Mutex<Vec<ChromeShareView>>>::default();
        let cancelled = Arc::<Mutex<Vec<ChromeCommand>>>::default();
        let assist_done = Arc::<Mutex<Vec<u32>>>::default();
        let finished = Arc::clone(&assist_done);
        let (control, mut controls) = mpsc::unbounded_channel();
        let (seen, shared, withdrawn) =
            (Arc::clone(&commands), Arc::clone(&shares), Arc::clone(&cancelled));
        let sites = Arc::clone(&origins);
        tokio::spawn(async move {
            let mut out = BTreeMap::new();
            loop {
                tokio::select! {
                    message = extension.next() => match message {
                        Some(CoreMessage::Command { id, command, origin }) => {
                            let command: ChromeCommand =
                                serde_json::from_value(command).expect("a Chrome command");
                            seen.lock().expect("commands").push(command.clone());
                            sites.lock().expect("origins").push(origin);
                            match answers(&command) {
                                Some(outcome) => {
                                    extension.send(ExtensionMessage::Result { id, outcome }).await;
                                }
                                None => {
                                    out.insert(id, command);
                                }
                            }
                        }
                        Some(CoreMessage::Cancel { id }) => {
                            if let Some(command) = out.remove(&id) {
                                withdrawn.lock().expect("cancelled").push(command);
                            }
                        }
                        Some(CoreMessage::Ping { id }) => {
                            extension.send(ExtensionMessage::Pong { id }).await;
                        }
                        Some(CoreMessage::AssistDone { tab_id }) => {
                            finished.lock().expect("assist_done").push(tab_id);
                        }
                        Some(CoreMessage::Shares { shares }) => {
                            *shared.lock().expect("shares") = shares;
                        }
                        Some(_) => {}
                        None => return,
                    },
                    control = controls.recv() => match control {
                        Some(Control::Send(message)) => extension.send(message).await,
                        Some(Control::Disconnect) | None => return,
                    },
                }
            }
        });
        Self { commands, origins, shares, cancelled, assist_done, control }
    }

    pub fn send(&self, message: ExtensionMessage) {
        let _ = self.control.send(Control::Send(message));
    }

    pub fn disconnect(&self) {
        let _ = self.control.send(Control::Disconnect);
    }

    pub fn commands(&self) -> Vec<ChromeCommand> {
        self.commands.lock().expect("commands").clone()
    }
}
