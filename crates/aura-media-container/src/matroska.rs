//! Matroska (WebM, MKV): the first video track entry names its CodecID and, when the muxer wrote
//! one, how long each frame lasts. Elements are walked by their sizes; an element of unknown size,
//! as a file written while recording has, cannot be stepped over.
use crate::reader::{Reader, TRUNCATED};
use crate::{VideoTrack, codec, frames_per_second, joined};

pub(crate) const EBML: [u8; 4] = [0x1a, 0x45, 0xdf, 0xa3];
const SEGMENT: u64 = 0x1853_8067;
const TRACKS: u64 = 0x1654_ae6b;
const TRACK_ENTRY: u64 = 0xae;
const TRACK_TYPE: u64 = 0x83;
const CODEC_ID: u64 = 0x86;
const DEFAULT_DURATION: u64 = 0x23_e383;
const VIDEO: u64 = 0xe0;
const FRAME_RATE: u64 = 0x23_83e3;
/// TrackType of a video track.
const VIDEO_TRACK: u64 = 1;

const BROKEN: &str = "WebM/MKV 的文件头结构不完整或已损坏，编码和帧率没有读";
const UNKNOWN_SIZE: &str =
    "WebM/MKV 在轨道信息之前有一段大小未知的数据（边录边写的文件），轨道信息没有读";

struct Element {
    id: u64,
    /// Where its content starts, past the ID and size.
    start: u64,
    /// None when its size is unknown.
    end: Option<u64>,
}

pub(crate) fn read(reader: &mut Reader) -> Result<VideoTrack, String> {
    let header = element(reader, 0)?;
    let segment = element(reader, header.end.ok_or(BROKEN)?)?;
    if segment.id != SEGMENT {
        return Err(BROKEN.into());
    }
    let end = segment.end.unwrap_or(reader.size);
    let mut at = segment.start;
    while at < end {
        let child = element(reader, at)?;
        if child.id == TRACKS {
            return tracks(reader, child.start, child.end.ok_or(BROKEN)?);
        }
        at = child.end.ok_or(UNKNOWN_SIZE)?;
    }
    Err("WebM/MKV 文件里没有轨道信息（Tracks），编码和帧率没有读".into())
}

fn tracks(reader: &mut Reader, start: u64, end: u64) -> Result<VideoTrack, String> {
    let mut at = start;
    while at < end {
        let entry = element(reader, at)?;
        let entry_end = entry.end.ok_or(BROKEN)?;
        if entry.id == TRACK_ENTRY
            && let Some(track) = video_entry(reader, entry.start, entry_end)?
        {
            return Ok(track);
        }
        at = entry_end;
    }
    Err("WebM/MKV 的轨道信息里没有视频轨".into())
}

fn video_entry(reader: &mut Reader, start: u64, end: u64) -> Result<Option<VideoTrack>, String> {
    let (mut kind, mut codec_id, mut frame_ns, mut written_rate) = (None, None, None, None);
    let mut at = start;
    while at < end {
        let child = element(reader, at)?;
        let child_end = child.end.ok_or(BROKEN)?;
        let length = child_end - child.start;
        match child.id {
            TRACK_TYPE if length <= 8 => {
                kind = Some(uint(&reader.bytes(child.start, length as usize)?))
            }
            CODEC_ID if length <= 64 => {
                codec_id = Some(text(&reader.bytes(child.start, length as usize)?))
            }
            DEFAULT_DURATION if length <= 8 => {
                frame_ns = Some(uint(&reader.bytes(child.start, length as usize)?));
            }
            VIDEO => written_rate = frame_rate(reader, child.start, child_end)?,
            _ => {}
        }
        at = child_end;
    }
    if kind != Some(VIDEO_TRACK) {
        return Ok(None);
    }
    let mut notes = Vec::new();
    let codec = codec_id.filter(|id| !id.is_empty()).map(|id| codec::from_matroska(&id));
    if codec.is_none() {
        notes.push("视频轨没有写编码（CodecID）".into());
    }
    let frame_rate = frame_ns
        .filter(|ns| *ns > 0)
        .and_then(|ns| frames_per_second(1e9, ns as f64))
        .or(written_rate);
    if frame_rate.is_none() {
        notes.push("文件没有写每帧时长（DefaultDuration），帧率要逐帧统计，没有读".into());
    }
    Ok(Some(VideoTrack { codec, frame_rate, note: joined(notes) }))
}

/// The frame rate a Video element states, which older muxers write.
fn frame_rate(reader: &mut Reader, start: u64, end: u64) -> Result<Option<f64>, String> {
    let mut at = start;
    while at < end {
        let child = element(reader, at)?;
        let child_end = child.end.ok_or(BROKEN)?;
        if child.id == FRAME_RATE {
            let bytes = reader.bytes(child.start, (child_end - child.start).min(8) as usize)?;
            let rate = match bytes.len() {
                4 => f64::from(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
                8 => f64::from_be_bytes(bytes.try_into().expect("eight bytes were read")),
                _ => return Ok(None),
            };
            return Ok(frames_per_second(rate, 1.0));
        }
        at = child_end;
    }
    Ok(None)
}

/// The element at `at`. Its ID keeps its length marker, as Matroska writes IDs.
fn element(reader: &mut Reader, at: u64) -> Result<Element, String> {
    if at >= reader.size {
        return Err(TRUNCATED.into());
    }
    let head = reader.bytes(at, (reader.size - at).min(12) as usize)?;
    let (id, id_length) = vint(&head, true).ok_or(BROKEN)?;
    let (size, size_length) = vint(&head[id_length..], false).ok_or(BROKEN)?;
    let start = at + (id_length + size_length) as u64;
    // A size with every bit set is unknown.
    let end = if size == (1 << (7 * size_length)) - 1 {
        None
    } else {
        Some(start.checked_add(size).filter(|end| *end <= reader.size).ok_or(TRUNCATED)?)
    };
    Ok(Element { id, start, end })
}

/// An EBML variable-length integer and its length; an ID keeps its length marker, a size does not.
fn vint(bytes: &[u8], id: bool) -> Option<(u64, usize)> {
    let first = *bytes.first()?;
    let length = first.leading_zeros() as usize + 1;
    if length > if id { 4 } else { 8 } || bytes.len() < length {
        return None;
    }
    let mut value =
        if id { u64::from(first) } else { u64::from(first) & ((1 << (8 - length)) - 1) };
    for byte in &bytes[1..length] {
        value = value << 8 | u64::from(*byte);
    }
    Some((value, length))
}

fn uint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |value, byte| value << 8 | u64::from(*byte))
}

/// A CodecID as text: printable ASCII, without the padding some muxers leave.
fn text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| if byte.is_ascii_graphic() { char::from(*byte) } else { '?' })
        .collect()
}

#[cfg(test)]
#[path = "matroska_tests.rs"]
mod tests;
