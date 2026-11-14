//! The codec and frame rate a video file's own header names, read with a bounded number of small
//! positional reads and nothing decoded: ISO BMFF (MP4, MOV) and Matroska (WebM, MKV).
mod codec;
mod matroska;
mod mp4;
mod reader;
#[cfg(test)]
mod test_file;

pub use reader::ReadAt;

/// What a video file's header says about its first video track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoTrack {
    /// The codec, as `H.264 (avc1)`: its usual name, then what the file wrote.
    pub codec: Option<String>,
    /// Frames per second, rounded to thousandths; the average when frame durations vary.
    pub frame_rate: Option<f64>,
    /// Why a value is missing, as one sentence.
    pub note: Option<String>,
}

const NOT_KNOWN: &str = "只读 MP4、MOV、WebM 和 MKV 的文件头；这个文件不是这几种，编码和帧率没有读";

/// The first video track of a file of `size` bytes, read through `read_at`.
pub fn video_track(size: u64, read_at: ReadAt<'_>) -> VideoTrack {
    let mut reader = reader::Reader::new(size, read_at);
    let found = reader.bytes(0, size.min(8) as usize).and_then(|head| {
        if head.len() == 8 && mp4::starts(&head) {
            mp4::read(&mut reader)
        } else if head.starts_with(&matroska::EBML) {
            matroska::read(&mut reader)
        } else {
            Err(NOT_KNOWN.into())
        }
    });
    let mut track =
        found.unwrap_or_else(|reason| VideoTrack { note: Some(reason), ..Default::default() });
    track.note = track.note.map(|note| clean(&note));
    track
}

/// Frames over seconds, as a frame rate a person would write.
pub(crate) fn frames_per_second(frames: f64, seconds: f64) -> Option<f64> {
    let rate = frames / seconds;
    (rate.is_finite() && rate > 0.0 && rate <= 1000.0).then(|| (rate * 1000.0).round() / 1000.0)
}

/// Notes joined into one sentence, or none.
pub(crate) fn joined(notes: Vec<String>) -> Option<String> {
    (!notes.is_empty()).then(|| notes.join("；"))
}

/// One bounded line: a system message may end in a line break.
fn clean(text: &str) -> String {
    text.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).take(300).collect()
}
