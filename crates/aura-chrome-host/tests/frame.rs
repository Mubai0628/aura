use aura_chrome_host::frame::{read_frame, write_frame};

fn framed(body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len()).expect("length").to_le_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

#[tokio::test]
async fn a_frame_round_trips_and_a_clean_end_is_none() {
    let mut written = Vec::new();
    write_frame(&mut written, br#"{"type":"hello"}"#, 64).await.expect("write");
    assert_eq!(written, framed(br#"{"type":"hello"}"#), "4-byte little-endian length first");
    let mut reader = written.as_slice();
    assert_eq!(
        read_frame(&mut reader, 64).await.expect("frame"),
        Some(br#"{"type":"hello"}"#.to_vec())
    );
    assert_eq!(read_frame(&mut reader, 64).await.expect("end"), None);
}

#[tokio::test]
async fn a_half_frame_an_oversized_frame_and_non_utf8_are_refused_by_code_only() {
    let secret_body = br#"{"type":"paired","secret":"c2VjcmV0LXZhbHVl"}"#;
    let cases: [(Vec<u8>, usize, &str); 4] = [
        (vec![5, 0], 64, "chrome_host.frame_truncated"),
        (framed(secret_body)[..10].to_vec(), 64, "chrome_host.frame_truncated"),
        (framed(secret_body), 8, "chrome_host.frame_too_large"),
        (framed(&[0xff, 0xfe, b'{']), 64, "chrome_host.frame_not_utf8"),
    ];
    for (bytes, max, code) in cases {
        let error = read_frame(&mut bytes.as_slice(), max).await.expect_err(code);
        assert_eq!(error.to_string(), code, "the error names no content");
    }
    let mut sink = Vec::new();
    let error = write_frame(&mut sink, secret_body, 8).await.expect_err("too large to send");
    assert_eq!(error.to_string(), "chrome_host.frame_too_large");
    assert!(sink.is_empty(), "nothing of an oversized frame is written");
}
