//! The decoder reports neither codec nor frame rate; media.probe reads them from the file's own
//! header, and says why when it cannot.
use super::*;

fn boxed(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    [&((content.len() + 8) as u32).to_be_bytes()[..], kind, content].concat()
}

fn full(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    boxed(kind, &[&[0; 4][..], content].concat())
}

/// The smallest MP4 whose index names an H.264 track of 30000/1001 frames per second.
fn mp4() -> Vec<u8> {
    let stbl = [
        full(b"stsd", &[&1u32.to_be_bytes()[..], &boxed(b"avc1", &[0; 78])].concat()),
        full(b"stts", &[1u32, 300, 1001].map(u32::to_be_bytes).concat()),
        full(b"stsz", &[0u32, 300].map(u32::to_be_bytes).concat()),
    ]
    .concat();
    let mdia = [
        full(b"mdhd", &[0u32, 0, 30000, 300_300, 0].map(u32::to_be_bytes).concat()),
        full(b"hdlr", &[&[0; 4][..], b"vide", &[0; 13]].concat()),
        boxed(b"minf", &boxed(b"stbl", &stbl)),
    ]
    .concat();
    let moov = boxed(b"moov", &boxed(b"trak", &boxed(b"mdia", &mdia)));
    [boxed(b"ftyp", b"isom\0\0\x02\0isom"), moov, boxed(b"mdat", &[0; 32])].concat()
}

async fn probe(name: &str, bytes: &'static [u8]) -> String {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario("media.probe", json!({ "path": name }))).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    images(&runtime, model, true).await;
    std::fs::write(host.root.path().join("waiting-workspace").join(name), bytes).unwrap();
    let (seen, task) = play(&runtime, bytes);
    let started = start(&runtime, session, model, AgentTaskMode::Normal).await;
    finish(&runtime, &started).await;
    assert_eq!(seen.lock().unwrap().len(), 1, "{}", tool_text(&provider));
    task.abort();
    tool_text(&provider)
}

#[tokio::test]
async fn a_probe_reports_the_codec_and_frame_rate_the_header_names() {
    let text = probe("clip.mp4", Box::leak(mp4().into_boxed_slice())).await;
    assert!(text.contains(r#""codec":"H.264 (avc1)""#), "{text}");
    assert!(text.contains(r#""frame_rate":29.97"#), "{text}");
    assert!(!text.contains(r#""note""#), "{text}");
}

#[tokio::test]
async fn a_probe_of_a_file_without_such_a_header_says_why_its_codec_is_null() {
    let text = probe("fixture.webm", SOURCE).await;
    assert!(text.contains(r#""codec":null"#), "{text}");
    assert!(text.contains(r#""note":"只读 MP4、MOV、WebM 和 MKV 的文件头"#), "{text}");
}
