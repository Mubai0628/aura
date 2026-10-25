use std::fmt::Write as _;

use aura_domain_common::WorkspacePath;
use aura_workspace::decode_text;
use aura_workspace_domain::{BinaryDisplayFormat, FileStorageSource, FsInspectResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub(crate) fn inspect_bytes(
    path: WorkspacePath,
    bytes: &[u8],
    source: FileStorageSource,
    is_dirty: bool,
) -> FsInspectResult {
    let detected = detect_format(path.as_str(), bytes);
    let decoded = decode_text(bytes).ok();
    let text_encoding = decoded.as_ref().map(|value| value.encoding);
    let binary_format = detected.executable
        || (!detected.text_by_format && detected.media_type != "application/octet-stream");
    let is_text = !binary_format
        && decoded
            .as_ref()
            .is_some_and(|value| decoded_content_looks_textual(bytes, value.encoding));
    FsInspectResult {
        path,
        byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        content_hash: aura_domain_common::ContentHash::blake3(bytes),
        extension: detected.extension,
        media_type: detected.media_type.to_owned(),
        magic_description: detected.description.to_owned(),
        is_text,
        text_encoding,
        is_executable: detected.executable,
        suitable_for_native_attachment: is_text || detected.native_attachment,
        source,
        is_dirty,
    }
}

fn decoded_content_looks_textual(
    bytes: &[u8],
    encoding: aura_workspace_domain::TextEncoding,
) -> bool {
    if matches!(
        encoding,
        aura_workspace_domain::TextEncoding::Utf16Le | aura_workspace_domain::TextEncoding::Utf16Be
    ) {
        return true;
    }
    !bytes.iter().any(|byte| {
        *byte == 0
            || (*byte < b' ' && !matches!(*byte, b'\t' | b'\n' | b'\r' | 0x0c))
            || *byte == 0x7f
    })
}

pub(crate) fn render_binary(bytes: &[u8], format: BinaryDisplayFormat) -> String {
    match format {
        BinaryDisplayFormat::Base64 => STANDARD.encode(bytes),
        BinaryDisplayFormat::Hex => render_hex(bytes),
        BinaryDisplayFormat::HexWithAscii => render_hex_ascii(bytes),
    }
}

struct DetectedFormat {
    extension: Option<String>,
    media_type: &'static str,
    description: &'static str,
    executable: bool,
    native_attachment: bool,
    text_by_format: bool,
}

fn detect_format(path: &str, bytes: &[u8]) -> DetectedFormat {
    let extension = path
        .rsplit_once('.')
        .map(|(_, value)| value.to_ascii_lowercase())
        .filter(|value| !value.contains('/'));
    let (media_type, description, executable, native_attachment, text_by_format) =
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ("image/png", "PNG image", false, true, false)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            ("image/jpeg", "JPEG image", false, true, false)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            ("image/gif", "GIF image", false, true, false)
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            ("image/webp", "WebP image", false, true, false)
        } else if bytes.starts_with(b"%PDF-") {
            ("application/pdf", "PDF document", false, true, false)
        } else if bytes.starts_with(b"PK\x03\x04") {
            ("application/zip", "ZIP-compatible archive", false, false, false)
        } else if bytes.starts_with(b"MZ") {
            (
                "application/vnd.microsoft.portable-executable",
                "Windows PE executable",
                true,
                false,
                false,
            )
        } else if bytes.starts_with(b"\x7fELF") {
            ("application/x-elf", "ELF executable", true, false, false)
        } else if bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
            ("application/java-vm", "Java class file", true, false, false)
        } else if bytes.starts_with(b"{") || bytes.starts_with(b"[") {
            ("application/json", "JSON-compatible text", false, true, true)
        } else {
            extension_format(extension.as_deref())
        };
    DetectedFormat {
        extension,
        media_type,
        description,
        executable,
        native_attachment,
        text_by_format,
    }
}

fn extension_format(extension: Option<&str>) -> (&'static str, &'static str, bool, bool, bool) {
    match extension {
        Some("svg") => ("image/svg+xml", "SVG document", false, true, true),
        Some("html" | "htm") => ("text/html", "HTML document", false, true, true),
        Some("css") => ("text/css", "CSS stylesheet", false, true, true),
        Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx") => {
            ("text/plain", "source text", false, true, true)
        }
        Some("md" | "txt" | "rs" | "toml" | "yaml" | "yml" | "xml" | "csv") => {
            ("text/plain", "text document", false, true, true)
        }
        Some("mp3") => ("audio/mpeg", "MP3 audio", false, true, false),
        Some("wav") => ("audio/wav", "WAVE audio", false, true, false),
        Some("mp4") => ("video/mp4", "MP4 video", false, true, false),
        Some("webm") => ("video/webm", "WebM video", false, true, false),
        Some("exe" | "dll" | "com" | "scr" | "msi") => {
            ("application/octet-stream", "executable artifact", true, false, false)
        }
        _ => ("application/octet-stream", "unknown binary or text data", false, false, false),
    }
}

fn render_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn render_hex_ascii(bytes: &[u8]) -> String {
    let mut output = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(output, "{:08x}  ", row.saturating_mul(16));
        for index in 0..16 {
            if let Some(byte) = chunk.get(index) {
                let _ = write!(output, "{byte:02x} ");
            } else {
                output.push_str("   ");
            }
            if index == 7 {
                output.push(' ');
            }
        }
        output.push_str(" |");
        for byte in chunk {
            output.push(if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            });
        }
        output.push_str("|\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use aura_domain_common::WorkspacePath;
    use aura_workspace_domain::{BinaryDisplayFormat, FileStorageSource};

    use super::{inspect_bytes, render_binary};

    #[test]
    fn detects_pe_and_renders_hex_ascii() {
        let inspected = inspect_bytes(
            WorkspacePath::parse("bin/a.exe").expect("path"),
            b"MZ\0\0",
            FileStorageSource::Disk,
            false,
        );
        assert!(inspected.is_executable);
        assert!(!inspected.is_text);
        assert!(render_binary(b"A\0", BinaryDisplayFormat::HexWithAscii).contains("|A.|"));
    }
}
