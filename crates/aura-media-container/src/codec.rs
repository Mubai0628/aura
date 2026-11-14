//! Codec names as people know them, followed by what the container wrote.

/// An ISO BMFF sample entry's four-character code.
pub(crate) fn from_fourcc(code: [u8; 4]) -> String {
    let written = if code.iter().all(|byte| byte.is_ascii_graphic() || *byte == b' ') {
        String::from_utf8_lossy(&code).trim_end().to_owned()
    } else {
        format!("0x{}", code.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
    };
    let known = match &code {
        b"avc1" | b"avc2" | b"avc3" | b"avc4" => Some("H.264"),
        b"hvc1" | b"hev1" => Some("H.265"),
        b"dvh1" | b"dvhe" => Some("Dolby Vision (H.265)"),
        b"dva1" | b"dvav" => Some("Dolby Vision (H.264)"),
        b"av01" => Some("AV1"),
        b"vp08" => Some("VP8"),
        b"vp09" => Some("VP9"),
        b"mp4v" => Some("MPEG-4 Part 2"),
        b"s263" | b"h263" => Some("H.263"),
        b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => Some("Apple ProRes"),
        b"jpeg" | b"mjpa" | b"mjpb" | b"mjp2" => Some("Motion JPEG"),
        b"encv" => Some("加密的视频"),
        _ => None,
    };
    named(known, &written)
}

/// A Matroska CodecID such as `V_VP9`.
pub(crate) fn from_matroska(id: &str) -> String {
    let known = match id {
        "V_MPEG4/ISO/AVC" => Some("H.264"),
        "V_MPEGH/ISO/HEVC" => Some("H.265"),
        "V_AV1" => Some("AV1"),
        "V_VP8" => Some("VP8"),
        "V_VP9" => Some("VP9"),
        "V_THEORA" => Some("Theora"),
        "V_PRORES" => Some("Apple ProRes"),
        "V_MPEG1" => Some("MPEG-1"),
        "V_MPEG2" => Some("MPEG-2"),
        "V_MJPEG" => Some("Motion JPEG"),
        "V_FFV1" => Some("FFV1"),
        id if id.starts_with("V_MPEG4/ISO/") => Some("MPEG-4 Part 2"),
        _ => None,
    };
    named(known, id)
}

fn named(known: Option<&str>, written: &str) -> String {
    match known {
        Some(name) => format!("{name} ({written})"),
        None => written.to_owned(),
    }
}
