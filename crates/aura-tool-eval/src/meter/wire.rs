use anyhow::{Result, ensure};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub(super) async fn request(
    socket: &mut TcpStream,
    format: crate::owner::ApiFormat,
) -> Result<(String, Vec<u8>)> {
    let mut head = Vec::new();
    loop {
        ensure!(head.len() < 65_536, "eval.header_too_large");
        head.push(socket.read_u8().await?);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&head)?;
    let mut lines = text.lines();
    let first = lines.next().ok_or_else(|| anyhow::anyhow!("eval.invalid_http"))?;
    let mut parts = first.split_whitespace();
    ensure!(parts.next() == Some("POST"), "eval.only_model_posts");
    let path = parts.next().ok_or_else(|| anyhow::anyhow!("eval.invalid_path"))?.to_owned();
    ensure!(path == format!("/v1{}", format.suffix()), "eval.unmetered_route_refused");
    let mut length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            ensure!(!name.eq_ignore_ascii_case("transfer-encoding"), "eval.chunked_input_refused");
            // No input header is forwarded or persisted, including any model authentication.
            if name.eq_ignore_ascii_case("content-length") {
                ensure!(length.is_none(), "eval.duplicate_length");
                length = Some(value.trim().parse::<usize>()?);
            }
        }
    }
    let length = length.ok_or_else(|| anyhow::anyhow!("eval.length_required"))?;
    ensure!(length <= 8 * 1024 * 1024, "eval.request_too_large");
    let mut body = vec![0; length];
    socket.read_exact(&mut body).await?;
    Ok((path, body))
}
pub(super) async fn respond(
    socket: &mut TcpStream,
    status: u16,
    body: &[u8],
    content_type: &str,
) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status} Eval\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body).await?;
    socket.shutdown().await?;
    Ok(())
}
