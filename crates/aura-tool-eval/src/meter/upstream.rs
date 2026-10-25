use super::Upstream;
use crate::owner::ApiFormat;
use anyhow::{Result, ensure};
use aura_windows_platform::WindowsCredentialStore;
use serde_json::{Value, json};

pub const UPSTREAM_TIMEOUT_SECONDS: u64 = 360;
pub const CONNECTION_TIMEOUT_SECONDS: u64 = 480;
/// Core's Connect phase lasts until the response head arrives, and the meter writes its head
/// only after the complete upstream body, so Core must wait at least as long as the meter.
pub const CORE_CONNECT_MS: u64 = 420_000;
pub const CORE_FIRST_EVENT_MS: u64 = 420_000;
pub const CORE_TOTAL_MS: u64 = 450_000;

/// Endpoint timeouts of the isolated Core profile that talks to the meter.
pub fn core_timeouts() -> Value {
    json!({"connect_ms":CORE_CONNECT_MS,"first_event_ms":CORE_FIRST_EVENT_MS,"stream_idle_ms":CORE_FIRST_EVENT_MS,"total_ms":CORE_TOTAL_MS})
}

pub(super) async fn send(
    request: &mut Value,
    upstream: &Upstream,
    format: ApiFormat,
) -> Result<(u16, Vec<u8>, String)> {
    send_with_timeout(
        request,
        upstream,
        format,
        std::time::Duration::from_secs(UPSTREAM_TIMEOUT_SECONDS),
    )
    .await
}

async fn send_with_timeout(
    request: &mut Value,
    upstream: &Upstream,
    format: ApiFormat,
    timeout: std::time::Duration,
) -> Result<(u16, Vec<u8>, String)> {
    if let Upstream::Replay(replay) = upstream {
        return replay.response(request, format);
    }
    let (url, secret) = match upstream {
        Upstream::DeepSeek { target } => {
            ensure!(target.settings.api_format == format, "eval.owner_format_mismatch");
            (
                target.request_url.clone(),
                Some(WindowsCredentialStore.read_record(&target.credential_ref)?.into_secret()),
            )
        }
        Upstream::Local(url) => (format!("{}{}", url.trim_end_matches('/'), format.suffix()), None),
        Upstream::Replay(_) => unreachable!("replay handled above"),
    };
    // Responses terminal events already contain usage; stream_options is Chat-only.
    if format == ApiFormat::OpenAiChatCompletions {
        request["stream_options"] = json!({"include_usage":true});
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()?;
    let mut send = client.post(url).json(request);
    if let Upstream::DeepSeek { target } = upstream {
        if let Some(org) = &target.settings.openai_organization_id {
            send = send.header("OpenAI-Organization", org);
        }
        if let Some(project) = &target.settings.openai_project_id {
            send = send.header("OpenAI-Project", project);
        }
    }
    if let Some(secret) = secret.as_ref() {
        send = send.bearer_auth(std::str::from_utf8(secret.expose())?);
    }
    let response = send.send().await?;
    let status = response.status().as_u16();
    let content_type = if response
        .headers()
        .get("content-type")
        .is_some_and(|v| v.as_bytes().starts_with(b"text/event-stream"))
    {
        "text/event-stream"
    } else {
        "application/json"
    };
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(body.len() + chunk.len() <= 16 * 1024 * 1024, "eval.response_too_large");
        body.extend_from_slice(&chunk);
    }
    Ok((status, body, content_type.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn r2_upstream_timeout_includes_waiting_for_the_complete_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback");
        let upstream =
            Upstream::Local(format!("http://{}", listener.local_addr().expect("address")));
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request");
            let mut request = [0; 4096];
            assert!(
                socket.read(&mut request).await.expect("bytes") > 0,
                "a real request precedes the stalled body"
            );
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\npartial",
                )
                .await
                .expect("partial body");
            std::future::pending::<()>().await;
        });
        let error = send_with_timeout(
            &mut json!({"stream":true,"max_output_tokens":8192}),
            &upstream,
            ApiFormat::OpenAiResponses,
            std::time::Duration::from_millis(100),
        )
        .await
        .expect_err("never return a truncated unmetered body");
        assert!(error.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_timeout));
        worker.abort();
    }
}
