//! Range reads use the retained native handle, never a caller-controlled path or a reopen.
use super::MediaBridge;
use aura_agent_domain::*;
use base64::Engine as _;
use std::os::windows::fs::FileExt;

impl MediaBridge {
    pub(crate) fn read(
        &self,
        input: &MediaHostReadRequestV1,
    ) -> anyhow::Result<MediaHostReadResultV1> {
        let file = {
            let pending = self.pending.lock().map_err(|_| anyhow::anyhow!("media.interrupted"))?;
            let entry = pending
                .get(&input.command_id)
                .filter(|entry| {
                    entry.runtime == input.runtime_instance_id && entry.session == input.session_id
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("media.command_unavailable: 视频读取命令不存在或已结束。")
                })?;
            anyhow::ensure!(
                input.length > 0
                    && input.length <= MAX_MEDIA_READ_BYTES
                    && input
                        .offset
                        .checked_add(u64::from(input.length))
                        .is_some_and(|end| end <= entry.size),
                "media.range_invalid: 视频读取范围不可用。"
            );
            std::sync::Arc::clone(&entry.file)
        };
        let mut bytes = vec![0; input.length as usize];
        let mut consumed = 0;
        while consumed < bytes.len() {
            let count =
                file.file().seek_read(&mut bytes[consumed..], input.offset + consumed as u64)?;
            anyhow::ensure!(count > 0, "media.source_changed: 视频文件已截短，无法读取指定范围。");
            consumed += count;
        }
        Ok(MediaHostReadResultV1 {
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }
}
