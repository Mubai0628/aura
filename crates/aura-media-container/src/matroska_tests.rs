use super::{
    CODEC_ID, DEFAULT_DURATION, FRAME_RATE, SEGMENT, TRACK_ENTRY, TRACK_TYPE, TRACKS, VIDEO,
};
use crate::test_file::track_of;

const CLUSTER: u64 = 0x1f43_b675;
const INFO: u64 = 0x1549_a966;

fn id(id: u64) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap_or(7);
    bytes[first..].to_vec()
}

/// An element with an eight-byte size, as some muxers write every size.
fn element(element: u64, content: &[u8]) -> Vec<u8> {
    let mut size = (content.len() as u64).to_be_bytes();
    size[0] = 0x01;
    [id(element), size.to_vec(), content.to_vec()].concat()
}

fn unknown_size(element: u64) -> Vec<u8> {
    [id(element), vec![0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]].concat()
}

fn uint(element_id: u64, value: u64) -> Vec<u8> {
    element(element_id, &value.to_be_bytes())
}

fn entry(kind: u64, codec: &str, frame_ns: Option<u64>) -> Vec<u8> {
    let duration = frame_ns.map(|ns| uint(DEFAULT_DURATION, ns)).unwrap_or_default();
    let content =
        [uint(0xd7, 1), uint(TRACK_TYPE, kind), element(CODEC_ID, codec.as_bytes()), duration];
    element(TRACK_ENTRY, &content.concat())
}

fn header() -> Vec<u8> {
    element(0x1a45_dfa3, &element(0x4282, b"webm"))
}

/// A file with a segment of known size: information, the tracks, one cluster.
fn file(entries: &[Vec<u8>]) -> Vec<u8> {
    let segment = [
        element(INFO, &uint(0x2a_d7b1, 1_000_000)),
        element(TRACKS, &entries.concat()),
        element(CLUSTER, &[0; 32]),
    ];
    [header(), element(SEGMENT, &segment.concat())].concat()
}

#[test]
fn the_video_entry_names_its_codec_and_how_long_each_frame_lasts() {
    let track = track_of(file(&[entry(2, "A_OPUS", None), entry(1, "V_VP9", Some(33_366_667))]));
    assert_eq!((track.codec.as_deref(), track.frame_rate), (Some("VP9 (V_VP9)"), Some(29.97)));
    assert_eq!(track.note, None);
    let hevc = track_of(file(&[entry(1, "V_MPEGH/ISO/HEVC", Some(40_000_000))]));
    assert_eq!(
        (hevc.codec.as_deref(), hevc.frame_rate),
        (Some("H.265 (V_MPEGH/ISO/HEVC)"), Some(25.0))
    );
}

#[test]
fn a_recording_without_frame_durations_names_its_codec_and_says_why_it_has_no_rate() {
    // As a browser's recorder writes it: a segment and clusters of unknown size.
    let recording = [
        header(),
        unknown_size(SEGMENT),
        element(INFO, &uint(0x2a_d7b1, 1_000_000)),
        element(TRACKS, &entry(1, "V_VP8", None)),
        unknown_size(CLUSTER),
        vec![0xa3, 0x84, 0x81, 0, 0, 0x80],
    ]
    .concat();
    let track = track_of(recording);
    assert_eq!((track.codec.as_deref(), track.frame_rate), (Some("VP8 (V_VP8)"), None));
    assert!(
        track.note.as_deref().is_some_and(|note| note.contains("DefaultDuration")),
        "{track:?}"
    );
}

#[test]
fn tracks_after_data_of_unknown_size_are_not_reached() {
    let file = [
        header(),
        unknown_size(SEGMENT),
        unknown_size(CLUSTER),
        vec![0; 16],
        element(TRACKS, &entry(1, "V_VP9", Some(40_000_000))),
    ]
    .concat();
    let track = track_of(file);
    assert_eq!(track.codec, None);
    assert!(track.note.as_deref().is_some_and(|note| note.contains("大小未知")), "{track:?}");
}

#[test]
fn a_frame_rate_written_in_the_video_element_is_used() {
    let video = element(VIDEO, &element(FRAME_RATE, &23.976f64.to_be_bytes()));
    let entry = element(
        TRACK_ENTRY,
        &[uint(TRACK_TYPE, 1), element(CODEC_ID, b"V_MPEG4/ISO/AVC"), video].concat(),
    );
    let track = track_of(file(&[entry]));
    assert_eq!(
        (track.codec.as_deref(), track.frame_rate),
        (Some("H.264 (V_MPEG4/ISO/AVC)"), Some(23.976))
    );
}

#[test]
fn a_file_with_only_audio_says_it_has_no_video() {
    let track = track_of(file(&[entry(2, "A_OPUS", None)]));
    assert_eq!(track.codec, None);
    assert!(track.note.as_deref().is_some_and(|note| note.contains("没有视频轨")), "{track:?}");
}
