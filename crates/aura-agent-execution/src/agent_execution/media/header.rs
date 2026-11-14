//! The codec and frame rate a video file's own header names, read through the verified handle
//! with positional reads; Chromium's decoder reports neither.
use aura_media_container::VideoTrack;
use std::os::windows::fs::FileExt;

pub(super) async fn video_track(file: &std::fs::File, size: u64) -> VideoTrack {
    let unread = |note: String| VideoTrack { note: Some(note), ..Default::default() };
    let file = match file.try_clone() {
        Ok(file) => file,
        Err(error) => return unread(format!("读文件头时出错（{error}）")),
    };
    tokio::task::spawn_blocking(move || {
        aura_media_container::video_track(size, &mut |offset, buffer| {
            file.seek_read(buffer, offset)
        })
    })
    .await
    .unwrap_or_else(|_| unread("读文件头时意外中断".into()))
}
