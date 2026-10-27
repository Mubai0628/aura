//! Messages between Aura's Core and its Chrome extension. `aura-chrome-host` relays them
//! unchanged between Chrome's native-messaging stdio and Core's named pipe; it reads none of them
//! except the one it writes itself, [`HostHello`].
//!
//! The extension proves it holds the pairing secret by answering a fresh challenge with
//! [`authentication_mac`]; the relay never sees the secret.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The protocol both ends speak; a connection with another version is refused.
pub const CHROME_PROTOCOL_VERSION: u32 = 1;
/// The largest message Chrome accepts from a native host.
pub const MAX_TO_CHROME_BYTES: usize = 1024 * 1024;
/// The largest message a native host accepts from Chrome.
pub const MAX_FROM_CHROME_BYTES: usize = 64 * 1024 * 1024;
/// Digits of a pairing code.
pub const PAIRING_CODE_DIGITS: usize = 8;
/// Bytes of the secret a paired extension keeps.
pub const PAIRING_SECRET_BYTES: usize = 32;

/// The relay's own first message to Core: the caller Chrome named when it started the relay.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostHello {
    /// `chrome-extension://<id>/`, as Chrome passed it.
    pub origin: String,
    pub protocol: u32,
}

/// What the extension sends to Core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtensionMessage {
    /// The first message of a connection.
    Hello {
        extension_id: String,
        extension_version: String,
    },
    /// A pairing code the user typed into the extension.
    Pair {
        code: String,
    },
    /// The answer to Core's challenge: lowercase hex of [`authentication_mac`].
    Authenticate {
        mac: String,
    },
    /// The outcome of one command.
    Result {
        id: u64,
        outcome: CommandOutcome,
    },
    /// The answer to a heartbeat.
    Pong {
        id: u64,
    },
    /// The conversations a tab can be shared with.
    ListSessions,
    /// The user shared this tab with a conversation in the side panel.
    Share {
        tab_id: u32,
        window_id: u32,
        url: String,
        title: String,
        session_id: String,
        #[serde(default)]
        scope: crate::commands::ChromeShareScope,
    },
    /// The user stopped sharing this tab.
    Unshare {
        tab_id: u32,
    },
    /// A shared tab moved or changed its title.
    TabChanged {
        tab_id: u32,
        url: String,
        title: String,
    },
    TabClosed {
        tab_id: u32,
    },
    /// The user pressed Stop in the side panel: every share ends and nothing more runs.
    Stop,
    /// The user's answer, in the side panel, about a new site in a shared tab.
    AnswerOrigin {
        tab_id: u32,
        origin: String,
        allow: bool,
    },
}

/// What Core sends to the extension.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreMessage {
    /// This extension is not paired: the user enters a code from Aura's settings.
    PairingRequired,
    /// Pairing succeeded; the extension keeps the secret (base64) and authenticates with it.
    Paired {
        secret: String,
    },
    PairRefused {
        reason: PairRefusal,
    },
    /// A fresh challenge (base64); the extension answers with [`ExtensionMessage::Authenticate`].
    Challenge {
        nonce: String,
        protocol: u32,
    },
    Authenticated,
    AuthRefused {
        reason: AuthRefusal,
    },
    /// One command; its shape belongs to the tool that sent it.
    Command {
        id: u64,
        command: serde_json::Value,
        /// The site Core authorized the command for. When the tab is no longer on it, the
        /// extension refuses the command and changes nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
    },
    /// The command `id` is no longer wanted.
    Cancel {
        id: u64,
    },
    Ping {
        id: u64,
    },
    /// The user disconnected this extension in Aura; it forgets its secret.
    Revoked,
    Sessions {
        sessions: Vec<crate::commands::ChromeSessionChoice>,
    },
    /// Every shared tab, sent whenever one changes.
    Shares {
        shares: Vec<crate::commands::ChromeShareView>,
    },
    /// A share was refused, in the user's words.
    ShareRefused {
        tab_id: u32,
        reason: String,
    },
    /// The user pressed 完成 in Aura for the tab the model handed them.
    AssistDone {
        tab_id: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PairRefusal {
    /// No code is waiting, or the code is not the one Aura showed.
    CodeInvalid,
    CodeExpired,
    /// Five wrong codes: the code is void and the user asks Aura for a new one.
    TooManyAttempts,
    /// The extension is not the one this installation accepts.
    ExtensionNotAllowed,
    ProtocolUnsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRefusal {
    NotPaired,
    MacInvalid,
    ProtocolUnsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOutcome {
    Done { value: serde_json::Value },
    Failed { code: String, message: String },
}

/// HMAC-SHA256 over the challenge, the extension's id and the protocol version.
#[must_use]
pub fn authentication_mac(secret: &[u8], nonce: &[u8], extension_id: &str) -> [u8; 32] {
    let mut message = Vec::with_capacity(nonce.len() + extension_id.len() + 4);
    message.extend_from_slice(nonce);
    message.extend_from_slice(extension_id.as_bytes());
    message.extend_from_slice(&CHROME_PROTOCOL_VERSION.to_le_bytes());
    hmac_sha256(secret, &message)
}

/// RFC 2104 HMAC with SHA-256 (64-byte blocks).
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0_u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// Compares two byte strings in time that depends only on their lengths.
#[must_use]
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).fold(0_u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

/// The extension id in a `chrome-extension://<id>/` origin.
#[must_use]
pub fn extension_id_of_origin(origin: &str) -> Option<&str> {
    let id = origin.strip_prefix("chrome-extension://")?.strip_suffix('/')?;
    (id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))).then_some(id)
}

/// `path` without the `\\?\` prefix `std::fs::canonicalize` gives it on Windows: the form Windows
/// reports a running program's image in, which both ends compare their peer against, name the pipe
/// by, and write into Chrome's manifest.
#[must_use]
pub fn plain_path(path: &std::path::Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}").into();
    }
    match text.strip_prefix(r"\\?\") {
        Some(local) if local.as_bytes().get(1) == Some(&b':') => local.into(),
        _ => path.to_path_buf(),
    }
}

/// The relay's file name, beside the Core of its version. Chrome starts it on demand, so a relay
/// left running from another version folder of an installation serves nobody and may be stopped.
pub const RELAY_PROGRAM: &str = "aura-chrome-host.exe";

/// Core's pipe for the relay of one installation, named by the folder that holds both programs
/// (canonical and plain, compared without case), so a relay always reaches the Core it was
/// installed with.
#[must_use]
pub fn pipe_name(install_directory: &std::path::Path) -> String {
    let folder = install_directory.to_string_lossy().to_lowercase();
    let digest = Sha256::digest(folder.as_bytes());
    let hex = digest[..8].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!(r"\\.\pipe\aura-chrome-{hex}")
}

/// The installation namespace of Aura's released builds.
pub const PRODUCTION_NAMESPACE: &str = "aura.desktop.production";
/// The extension a released Aura pairs with: the id of the public key in its manifest.
pub const PRODUCTION_EXTENSION_ID: &str = "jhkgloifhjcbdiefjedcjnpjnlicijlg";
/// The extension a test namespace pairs with; a released Aura never accepts it.
pub const TEST_EXTENSION_ID: &str = "bheiijocpacefhgapeegbnekchcpedkj";

/// The native-messaging host name an installation registers: one per namespace, so a test
/// installation never takes over the released one's registration.
#[must_use]
pub fn native_host_name(namespace: &str) -> String {
    if namespace == PRODUCTION_NAMESPACE {
        return "com.aura.chrome_control".to_owned();
    }
    let digest = Sha256::digest(namespace.as_bytes());
    let hex = digest[..6].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("com.aura.chrome_control.t{hex}")
}

/// The only extension an installation in `namespace` pairs with.
#[must_use]
pub fn extension_id_for(namespace: &str) -> &'static str {
    if namespace == PRODUCTION_NAMESPACE { PRODUCTION_EXTENSION_ID } else { TEST_EXTENSION_ID }
}

/// Where an installation keeps a pairing secret: among its own credentials (`Aura/<namespace
/// hash>/...`), which uninstalling it with its data deletes.
pub fn pairing_credential(
    namespace_hash: &str,
    id: aura_domain_common::CredentialId,
) -> Result<aura_domain_common::CredentialRef, aura_domain_common::CredentialRefParseError> {
    format!("wincred://aura/{namespace_hash}/chrome/pairing/{id}").parse()
}
