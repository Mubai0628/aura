//! Native-messaging frames: a 4-byte little-endian length, then that many bytes of UTF-8 JSON.
//! Core's pipe carries the same frames, so the relay checks one shape on both sides.
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

fn refused(code: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, code)
}

/// The next frame, or `None` when the stream ended cleanly between frames.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0_u8; 4];
    let mut filled = 0;
    while filled < length.len() {
        let read = reader.read(&mut length[filled..]).await?;
        if read == 0 {
            return if filled == 0 {
                Ok(None)
            } else {
                Err(refused("chrome_host.frame_truncated"))
            };
        }
        filled += read;
    }
    let length = usize::try_from(u32::from_le_bytes(length))
        .map_err(|_| refused("chrome_host.frame_too_large"))?;
    if length > max {
        return Err(refused("chrome_host.frame_too_large"));
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).await.map_err(|_| refused("chrome_host.frame_truncated"))?;
    if std::str::from_utf8(&body).is_err() {
        return Err(refused("chrome_host.frame_not_utf8"));
    }
    Ok(Some(body))
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    body: &[u8],
    max: usize,
) -> io::Result<()> {
    if body.len() > max {
        return Err(refused("chrome_host.frame_too_large"));
    }
    let length = u32::try_from(body.len()).map_err(|_| refused("chrome_host.frame_too_large"))?;
    writer.write_all(&length.to_le_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}
