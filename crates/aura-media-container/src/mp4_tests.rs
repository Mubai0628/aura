use crate::test_file::{Sparse, track_of};
use crate::{NOT_KNOWN, VideoTrack};

fn boxed(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    [&((content.len() + 8) as u32).to_be_bytes()[..], kind, content].concat()
}

fn full(kind: &[u8; 4], version: u8, content: &[u8]) -> Vec<u8> {
    boxed(kind, &[&[version, 0, 0, 0][..], content].concat())
}

fn mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let times = [0u32, 0, timescale, duration, 0].map(u32::to_be_bytes).concat();
    full(b"mdhd", 0, &times)
}

/// An ISO handler box; QuickTime writes its component type where ISO writes zero.
fn hdlr(component: &[u8; 4], handler: &[u8; 4]) -> Vec<u8> {
    full(b"hdlr", 0, &[&component[..], handler, &[0; 12], b"\0"].concat())
}

fn stsd(format: &[u8; 4]) -> Vec<u8> {
    full(b"stsd", 0, &[&1u32.to_be_bytes()[..], &boxed(format, &[0; 78])].concat())
}

fn stts(entries: &[(u32, u32)]) -> Vec<u8> {
    let mut content = (entries.len() as u32).to_be_bytes().to_vec();
    for (count, delta) in entries {
        content.extend(count.to_be_bytes());
        content.extend(delta.to_be_bytes());
    }
    full(b"stts", 0, &content)
}

fn stsz(count: u32) -> Vec<u8> {
    full(b"stsz", 0, &[0u32.to_be_bytes(), count.to_be_bytes()].concat())
}

struct Trak<'a> {
    handler: &'a [u8; 4],
    format: &'a [u8; 4],
    timescale: u32,
    duration: u32,
    durations: &'a [(u32, u32)],
    frames: u32,
}

const VIDEO: Trak = Trak {
    handler: b"vide",
    format: b"avc1",
    timescale: 30000,
    duration: 300_300,
    durations: &[(300, 1001)],
    frames: 300,
};

fn trak(trak: &Trak, component: &[u8; 4]) -> Vec<u8> {
    let stbl = [stsd(trak.format), stts(trak.durations), stsz(trak.frames)].concat();
    let mdia = [
        mdhd(trak.timescale, trak.duration),
        hdlr(component, trak.handler),
        boxed(b"minf", &boxed(b"stbl", &stbl)),
    ]
    .concat();
    boxed(b"trak", &boxed(b"mdia", &mdia))
}

fn moov(traks: &[Trak]) -> Vec<u8> {
    let traks = traks.iter().map(|one| trak(one, &[0; 4])).collect::<Vec<_>>().concat();
    boxed(b"moov", &traks)
}

fn ftyp() -> Vec<u8> {
    boxed(b"ftyp", b"isom\0\0\x02\0isomiso2avc1mp41")
}

fn mp4(parts: &[Vec<u8>]) -> Vec<u8> {
    [&[ftyp()][..], parts].concat().concat()
}

#[test]
fn a_constant_rate_track_gives_its_codec_and_exact_rate() {
    let track = track_of(mp4(&[moov(&[VIDEO]), boxed(b"mdat", &[0; 64])]));
    assert_eq!(
        track,
        VideoTrack { codec: Some("H.264 (avc1)".into()), frame_rate: Some(29.97), note: None }
    );
}

#[test]
fn the_video_track_is_found_after_an_audio_track() {
    let audio = Trak { handler: b"soun", format: b"mp4a", ..VIDEO };
    let video = Trak {
        format: b"hvc1",
        timescale: 12800,
        duration: 128_000,
        durations: &[(250, 512)],
        frames: 250,
        ..VIDEO
    };
    let track = track_of(mp4(&[moov(&[audio, video])]));
    assert_eq!((track.codec.as_deref(), track.frame_rate), (Some("H.265 (hvc1)"), Some(25.0)));
}

#[test]
fn a_varying_rate_is_the_average_over_the_tracks_duration() {
    let video = Trak {
        timescale: 1000,
        duration: 10_000,
        durations: &[(100, 33), (150, 45)],
        frames: 250,
        ..VIDEO
    };
    assert_eq!(track_of(mp4(&[moov(&[video])])).frame_rate, Some(25.0));
}

#[test]
fn an_index_after_gigabytes_of_media_is_found_without_reading_the_media() {
    let media = 5u64 << 30;
    let mut mdat = 1u32.to_be_bytes().to_vec();
    mdat.extend(b"mdat");
    mdat.extend((media + 16).to_be_bytes());
    let head = [ftyp(), mdat].concat();
    let index = moov(&[VIDEO]);
    let at = head.len() as u64 - 16 + media + 16;
    let file = Sparse::new(at + index.len() as u64).put(0, head).put(at, index);
    let (track, read) = file.track();
    assert_eq!(track.codec.as_deref(), Some("H.264 (avc1)"));
    assert_eq!(track.frame_rate, Some(29.97));
    assert!(read < 4096, "{read} bytes read");
}

#[test]
fn a_fragmented_file_names_its_codec_and_says_why_it_has_no_rate() {
    let fragmented = Trak { duration: 0, durations: &[], frames: 0, ..VIDEO };
    let track = track_of(mp4(&[moov(&[fragmented])]));
    assert_eq!(track.codec.as_deref(), Some("H.264 (avc1)"));
    assert_eq!(track.frame_rate, None);
    assert!(track.note.as_deref().is_some_and(|note| note.contains("fMP4")), "{track:?}");
}

#[test]
fn a_file_without_its_index_says_so() {
    let track = track_of(mp4(&[boxed(b"mdat", &[0; 64])]));
    assert_eq!((track.codec.as_deref(), track.frame_rate), (None, None));
    assert!(track.note.as_deref().is_some_and(|note| note.contains("moov")), "{track:?}");
    let cut = mp4(&[boxed(b"mdat", &[0; 64])])[..60].to_vec();
    assert!(track_of(cut).note.as_deref().is_some_and(|note| note.contains("不完整")));
}

#[test]
fn a_header_that_needs_more_reads_than_allowed_is_left_unread() {
    let free = (0..5000).map(|_| boxed(b"free", &[])).collect::<Vec<_>>().concat();
    let track = track_of(mp4(&[free, moov(&[VIDEO])]));
    assert_eq!(track.codec, None);
    assert!(track.note.as_deref().is_some_and(|note| note.contains("读取上限")), "{track:?}");
}

#[test]
fn an_unknown_code_is_kept_as_written_and_a_binary_one_in_hex() {
    let named = |format| track_of(mp4(&[moov(&[Trak { format, ..VIDEO }])])).codec;
    assert_eq!(named(b"xyz1").as_deref(), Some("xyz1"));
    assert_eq!(named(&[0, 1, 2, 3]).as_deref(), Some("0x00010203"));
    assert_eq!(named(b"vp09").as_deref(), Some("VP9 (vp09)"));
}

#[test]
fn a_quicktime_file_without_ftyp_is_read() {
    let prores = Trak {
        format: b"apcn",
        timescale: 24000,
        duration: 240_240,
        durations: &[(240, 1001)],
        frames: 240,
        ..VIDEO
    };
    let file = [boxed(b"wide", &[]), boxed(b"moov", &trak(&prores, b"mhlr"))].concat();
    let track = track_of(file);
    assert_eq!(
        (track.codec.as_deref(), track.frame_rate),
        (Some("Apple ProRes (apcn)"), Some(23.976))
    );
}

#[test]
fn a_file_of_another_kind_or_an_unreadable_one_says_why() {
    let avi = track_of(b"RIFF\0\0\0\0AVI LIST".to_vec());
    assert_eq!(avi.note.as_deref(), Some(NOT_KNOWN));
    assert_eq!(track_of(Vec::new()).note.as_deref(), Some(NOT_KNOWN));
    let mut failing = |_: u64, _: &mut [u8]| -> std::io::Result<usize> {
        Err(std::io::Error::other("disk gone\r\n"))
    };
    let track = crate::video_track(1000, &mut failing);
    assert_eq!(track.note.as_deref(), Some("读文件头时出错（disk gone  ）"));
}
