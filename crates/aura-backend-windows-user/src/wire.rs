use anyhow::{Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn read<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    let size = reader.read_u32_le().await? as usize;
    ensure!(size > 0 && size <= aura_windows_user_contract::MAX_FRAME_BYTES, "runner.frame_size");
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub async fn write<T: Serialize>(writer: &mut (impl AsyncWrite + Unpin), value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= aura_windows_user_contract::MAX_FRAME_BYTES,
        "runner.frame_size"
    );
    writer.write_u32_le(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn malformed_and_oversized_frames_are_rejected_before_deserialization() {
        for size in [0, aura_windows_user_contract::MAX_FRAME_BYTES as u32 + 1] {
            let bytes = size.to_le_bytes();
            assert!(read::<serde_json::Value>(&mut bytes.as_slice()).await.is_err());
        }
        let mut truncated = Vec::from(100_u32.to_le_bytes());
        truncated.extend_from_slice(b"{}");
        assert!(read::<serde_json::Value>(&mut truncated.as_slice()).await.is_err());
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            &serde_json::json!({"message":"rejected","reason":"test","channel":"main"}),
        )
        .await
        .expect("encode");
        assert!(
            read::<aura_windows_user_contract::RunnerCoreMessage>(&mut bytes.as_slice())
                .await
                .is_err()
        );
        let mut ready = Vec::new();
        write(&mut ready, &aura_windows_user_contract::RunnerCoreMessage::Ready)
            .await
            .expect("encode ready");
        assert!(matches!(
            read::<aura_windows_user_contract::RunnerCoreMessage>(&mut ready.as_slice())
                .await
                .expect("decode ready"),
            aura_windows_user_contract::RunnerCoreMessage::Ready
        ));
    }
    #[tokio::test]
    async fn raw_output_bytes_roundtrip_without_code_page_guessing() {
        let mut bytes = Vec::new();
        let value = vec![0, 255, 13, 10, 0x80];
        write(&mut bytes, &value).await.expect("encode");
        assert_eq!(read::<Vec<u8>>(&mut bytes.as_slice()).await.expect("decode"), value);
    }
}
