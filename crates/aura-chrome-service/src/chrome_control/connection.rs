//! One extension connection, in order: the relay names the caller, the extension says hello, it
//! pairs or proves its pairing against a fresh challenge, and only then carries commands. A missed
//! heartbeat, a wrong proof or anything out of order ends the connection.
use super::{Inner, Live};
use aura_chrome_domain::protocol::{
    AuthRefusal, CHROME_PROTOCOL_VERSION, CoreMessage, ExtensionMessage, HostHello,
    MAX_FROM_CHROME_BYTES, MAX_TO_CHROME_BYTES, authentication_mac, constant_time_eq,
    extension_id_of_origin,
};
use aura_chrome_host::frame::{read_frame, write_frame};
use base64::Engine as _;
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use tokio::io::AsyncRead;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{mpsc, oneshot};

const BASE64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub(super) async fn serve(inner: Arc<Inner>, pipe: NamedPipeServer) {
    let (mut reader, mut writer) = tokio::io::split(pipe);
    let Ok(Some(first)) = read_frame(&mut reader, MAX_FROM_CHROME_BYTES).await else { return };
    let Ok(hello) = serde_json::from_slice::<HostHello>(&first) else { return };
    if hello.protocol != CHROME_PROTOCOL_VERSION
        || extension_id_of_origin(&hello.origin) != Some(inner.options.extension_id.as_str())
    {
        return;
    }
    let (outgoing, mut queue) = mpsc::channel::<CoreMessage>(64);
    let (close, closed) = oneshot::channel();
    inner.lock().live = Some(Live {
        outgoing: outgoing.clone(),
        pending: BTreeMap::new(),
        authenticated: false,
        close: Some(close),
    });
    let mut writing = tokio::spawn(async move {
        while let Some(message) = queue.recv().await {
            let Ok(bytes) = serde_json::to_vec(&message) else { break };
            if write_frame(&mut writer, &bytes, MAX_TO_CHROME_BYTES).await.is_err() {
                break;
            }
            if message == CoreMessage::Revoked {
                break;
            }
        }
    });
    let mut written = false;
    tokio::select! {
        _ = &mut writing => written = true,
        _ = converse(&inner, &mut reader, &outgoing) => {}
        _ = closed => {}
    }
    // A command still out has no outcome any more: its waiter sees the connection end.
    {
        let mut state = inner.lock();
        if state.live.as_ref().is_some_and(|live| live.outgoing.same_channel(&outgoing)) {
            state.live = None;
        }
    }
    // Nothing new can be queued now; what was - a refusal, `Revoked` - still reaches the
    // extension before the pipe closes.
    drop(outgoing);
    if !written {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writing).await;
    }
}

async fn next<R: AsyncRead + Unpin>(reader: &mut R) -> anyhow::Result<ExtensionMessage> {
    let frame = read_frame(reader, MAX_FROM_CHROME_BYTES)
        .await?
        .ok_or_else(|| anyhow::anyhow!("chrome.extension_closed"))?;
    Ok(serde_json::from_slice(&frame)?)
}

async fn send(outgoing: &mpsc::Sender<CoreMessage>, message: CoreMessage) -> anyhow::Result<()> {
    outgoing.send(message).await.map_err(|_| anyhow::anyhow!("chrome.extension_closed"))
}

/// A fresh challenge, sent; the nonce is kept to check the answer.
async fn challenge(outgoing: &mpsc::Sender<CoreMessage>) -> anyhow::Result<Vec<u8>> {
    let mut nonce = vec![0_u8; 32];
    getrandom::fill(&mut nonce).map_err(|error| anyhow::anyhow!("chrome.randomness: {error}"))?;
    send(
        outgoing,
        CoreMessage::Challenge { nonce: BASE64.encode(&nonce), protocol: CHROME_PROTOCOL_VERSION },
    )
    .await?;
    Ok(nonce)
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    text.len()
        .is_multiple_of(2)
        .then(|| {
            (0..text.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
                .collect()
        })
        .flatten()
}

async fn converse<R: AsyncRead + Unpin>(
    inner: &Arc<Inner>,
    reader: &mut R,
    outgoing: &mpsc::Sender<CoreMessage>,
) -> anyhow::Result<()> {
    let ExtensionMessage::Hello { extension_id, .. } = next(reader).await? else {
        anyhow::bail!("chrome.hello_expected");
    };
    anyhow::ensure!(extension_id == inner.options.extension_id, "chrome.extension_not_allowed");
    let mut nonce = if inner.secret_of(&extension_id).is_some() {
        Some(challenge(outgoing).await?)
    } else {
        send(outgoing, CoreMessage::PairingRequired).await?;
        None
    };
    let mut heartbeat = tokio::time::interval(inner.options.heartbeat);
    heartbeat.tick().await;
    let mut unanswered: Option<u64> = None;
    loop {
        tokio::select! {
            message = next(reader) => match message? {
                ExtensionMessage::Pair { code } => {
                    let checked = inner
                        .lock()
                        .pairing
                        .as_mut()
                        .map_or(Err(aura_chrome_domain::protocol::PairRefusal::CodeInvalid), |(pending, _)| {
                            pending.check(&code, Instant::now())
                        });
                    match checked {
                        Ok(()) => {
                            let secret = inner.pair(&extension_id)?;
                            send(outgoing, CoreMessage::Paired { secret: BASE64.encode(secret) }).await?;
                            nonce = Some(challenge(outgoing).await?);
                        }
                        Err(reason) => send(outgoing, CoreMessage::PairRefused { reason }).await?,
                    }
                }
                ExtensionMessage::Authenticate { mac } => {
                    let expected = nonce
                        .take()
                        .zip(inner.secret_of(&extension_id))
                        .map(|(nonce, secret)| authentication_mac(&secret, &nonce, &extension_id));
                    let Some(expected) = expected else {
                        send(outgoing, CoreMessage::AuthRefused { reason: AuthRefusal::NotPaired }).await?;
                        anyhow::bail!("chrome.not_paired");
                    };
                    if !decode_hex(&mac).is_some_and(|mac| constant_time_eq(&mac, &expected)) {
                        send(outgoing, CoreMessage::AuthRefused { reason: AuthRefusal::MacInvalid }).await?;
                        anyhow::bail!("chrome.mac_invalid");
                    }
                    if let Some(live) = inner.lock().live.as_mut() {
                        live.authenticated = true;
                    }
                    send(outgoing, CoreMessage::Authenticated).await?;
                    // The shares Core still holds, so the extension can drop tabs that are gone.
                    inner.broadcast_shares();
                }
                ExtensionMessage::Result { id, outcome } => {
                    let mut state = inner.lock();
                    let live = state.live.as_mut().filter(|live| live.authenticated);
                    if let Some(waiter) = live.and_then(|live| live.pending.remove(&id)) {
                        let _ = waiter.send(outcome);
                    }
                }
                ExtensionMessage::Pong { id } => {
                    if unanswered == Some(id) {
                        unanswered = None;
                    }
                }
                ExtensionMessage::Hello { .. } => anyhow::bail!("chrome.hello_repeated"),
                other => {
                    anyhow::ensure!(
                        inner.lock().live.as_ref().is_some_and(|live| live.authenticated),
                        "chrome.not_authenticated"
                    );
                    shared(inner, outgoing, other).await?;
                }
            },
            _ = heartbeat.tick() => {
                anyhow::ensure!(unanswered.is_none(), "chrome.heartbeat_missed");
                let id = {
                    let mut state = inner.lock();
                    state.next_id += 1;
                    state.next_id
                };
                unanswered = Some(id);
                send(outgoing, CoreMessage::Ping { id }).await?;
            }
        }
    }
}

/// What the side panel tells Core about shares, tabs and the user's answers.
async fn shared(
    inner: &Arc<Inner>,
    outgoing: &mpsc::Sender<CoreMessage>,
    message: ExtensionMessage,
) -> anyhow::Result<()> {
    match message {
        ExtensionMessage::ListSessions => {
            send(outgoing, CoreMessage::Sessions { sessions: inner.session_choices() }).await?;
        }
        ExtensionMessage::Share { tab_id, url, title, session_id, scope, .. } => {
            let known =
                inner.session_choices().iter().any(|choice| choice.session_id == session_id);
            let session =
                session_id.parse::<aura_domain_common::SessionId>().ok().filter(|_| known);
            let refusal = if aura_security_contract::chrome_origin_blocked(&url) {
                Some("这个页面不能共享给 Aura。")
            } else if session.is_none() {
                Some("这个会话已不存在，请重新选择。")
            } else {
                None
            };
            match (refusal, session) {
                (None, Some(session)) => {
                    inner.lock().shares.add(session, tab_id, &url, &title, false, scope);
                    inner.broadcast_shares();
                }
                (reason, _) => {
                    let reason = reason.unwrap_or("这个会话已不存在，请重新选择。").to_owned();
                    send(outgoing, CoreMessage::ShareRefused { tab_id, reason }).await?;
                }
            }
        }
        ExtensionMessage::Unshare { tab_id } | ExtensionMessage::TabClosed { tab_id } => {
            inner.lock().shares.remove(tab_id);
            inner.broadcast_shares();
        }
        ExtensionMessage::TabChanged { tab_id, url, title } => {
            inner.lock().shares.changed(tab_id, &url, &title);
            inner.broadcast_shares();
        }
        ExtensionMessage::AnswerOrigin { tab_id, origin, allow } => {
            inner.lock().shares.answer(tab_id, &origin, allow);
            inner.broadcast_shares();
        }
        ExtensionMessage::Stop => {
            inner.stop_all();
        }
        _ => anyhow::bail!("chrome.unexpected_message"),
    }
    Ok(())
}
