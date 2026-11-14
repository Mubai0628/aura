//! ISO BMFF (MP4, MOV): the first video track's sample description names the codec; its time-to-
//! sample table, or its frame count over its duration, gives the frame rate. Boxes are walked by
//! their headers, so an index written after the media data is found without reading the media.
use crate::reader::{Reader, TRUNCATED};
use crate::{VideoTrack, codec, frames_per_second, joined};

/// Box types a file of this family may start with; QuickTime files need not start with `ftyp`.
const FIRST: [&[u8; 4]; 7] = [b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot"];

pub(crate) fn starts(head: &[u8]) -> bool {
    FIRST.iter().any(|kind| head[4..8] == kind[..])
}

#[derive(Clone, Copy)]
struct Found {
    kind: [u8; 4],
    /// Where its content starts, past the header.
    start: u64,
    end: u64,
}

impl Found {
    fn holds(&self, bytes: u64) -> bool {
        self.end - self.start >= bytes
    }
}

pub(crate) fn read(reader: &mut Reader) -> Result<VideoTrack, String> {
    let size = reader.size;
    let moov = each(reader, 0, size, |_, found| Ok((&found.kind == b"moov").then_some(found)))?
        .ok_or("没有找到 MP4 的索引（moov），文件可能不完整")?;
    each(reader, moov.start, moov.end, |reader, trak| {
        if &trak.kind == b"trak" { video(reader, trak) } else { Ok(None) }
    })?
    .ok_or_else(|| "MP4 的索引里没有视频轨".into())
}

/// Visits the boxes in `start..end` in order until `visit` finds what it looks for.
fn each<T>(
    reader: &mut Reader,
    start: u64,
    end: u64,
    mut visit: impl FnMut(&mut Reader, Found) -> Result<Option<T>, String>,
) -> Result<Option<T>, String> {
    let mut at = start;
    while end - at >= 8 {
        let head = reader.bytes(at, 8)?;
        let kind = [head[4], head[5], head[6], head[7]];
        let (mut header, size) = match u32::from_be_bytes([head[0], head[1], head[2], head[3]]) {
            // To the end of the box around it.
            0 => (8, end - at),
            1 => (16, reader.u64_at(at + 8)?),
            size => (8, u64::from(size)),
        };
        if &kind == b"uuid" {
            header += 16;
        }
        if size < header || size > end - at {
            return Err(TRUNCATED.into());
        }
        if let Some(found) = visit(reader, Found { kind, start: at + header, end: at + size })? {
            return Ok(Some(found));
        }
        at += size;
    }
    Ok(None)
}

fn child(reader: &mut Reader, parent: Found, kind: &[u8; 4]) -> Result<Option<Found>, String> {
    each(reader, parent.start, parent.end, |_, found| Ok((&found.kind == kind).then_some(found)))
}

/// The track's codec and frame rate, when it is a video track.
fn video(reader: &mut Reader, trak: Found) -> Result<Option<VideoTrack>, String> {
    let Some(mdia) = child(reader, trak, b"mdia")? else {
        return Ok(None);
    };
    let Some(hdlr) = child(reader, mdia, b"hdlr")?.filter(|hdlr| hdlr.holds(12)) else {
        return Ok(None);
    };
    // Version and flags, QuickTime's component type or ISO's zero, then the handler type.
    if reader.bytes(hdlr.start + 8, 4)? != b"vide" {
        return Ok(None);
    }
    let mut track = VideoTrack::default();
    let mut notes = Vec::new();
    let stbl = match child(reader, mdia, b"minf")? {
        Some(minf) => child(reader, minf, b"stbl")?,
        None => None,
    };
    let Some(stbl) = stbl else {
        track.note = Some("视频轨没有样本表（stbl），编码和帧率没有读".into());
        return Ok(Some(track));
    };
    // Version and flags, the entry count, then the first entry's size and format.
    match child(reader, stbl, b"stsd")?.filter(|stsd| stsd.holds(16)) {
        Some(stsd) => {
            let format = reader.bytes(stsd.start + 12, 4)?;
            track.codec = Some(codec::from_fourcc([format[0], format[1], format[2], format[3]]));
        }
        None => notes.push("视频轨没有样本描述（stsd），编码没有读".into()),
    }
    match rate(reader, mdia, stbl) {
        Ok(rate) => track.frame_rate = Some(rate),
        Err(note) => notes.push(note),
    }
    track.note = joined(notes);
    Ok(Some(track))
}

fn rate(reader: &mut Reader, mdia: Found, stbl: Found) -> Result<f64, String> {
    let mdhd = child(reader, mdia, b"mdhd")?
        .filter(|mdhd| mdhd.holds(20))
        .ok_or("视频轨没有媒体头（mdhd），帧率没有读")?;
    let (timescale, duration) = if reader.bytes(mdhd.start, 1)?[0] == 1 && mdhd.holds(32) {
        (reader.u32_at(mdhd.start + 20)?, reader.u64_at(mdhd.start + 24)?)
    } else {
        (reader.u32_at(mdhd.start + 12)?, u64::from(reader.u32_at(mdhd.start + 16)?))
    };
    if timescale == 0 {
        return Err("视频轨的时间单位是零，帧率没有读".into());
    }
    // Every frame lasting as long: the exact rate.
    if let Some(stts) = child(reader, stbl, b"stts")?.filter(|stts| stts.holds(16))
        && reader.u32_at(stts.start + 4)? == 1
    {
        let delta = reader.u32_at(stts.start + 12)?;
        if let Some(rate) = frames_per_second(f64::from(timescale), f64::from(delta)) {
            return Ok(rate);
        }
    }
    // Otherwise the average: the frame count over the track's duration.
    let sizes = match child(reader, stbl, b"stsz")? {
        Some(stsz) => Some(stsz),
        None => child(reader, stbl, b"stz2")?,
    };
    let frames = match sizes.filter(|sizes| sizes.holds(12)) {
        Some(sizes) => reader.u32_at(sizes.start + 8)?,
        None => 0,
    };
    if frames == 0 || duration == 0 {
        return Err("这个 MP4 的帧写在各个分段里（fMP4），索引里没有帧数，帧率没有读".into());
    }
    frames_per_second(f64::from(frames), duration as f64 / f64::from(timescale))
        .ok_or_else(|| "视频轨写的帧数和时长算不出合理的帧率，帧率没有读".into())
}

#[cfg(test)]
#[path = "mp4_tests.rs"]
mod tests;
