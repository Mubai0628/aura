use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

const PDF: &str = "application/pdf";
const ZIP: &str = "application/zip";
const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// Identify common documents on the handle that is about to be committed. A missing signature
/// leaves the original bytes available to the caller without certifying their declared type.
pub(super) fn detect(file: &mut File, size: u64) -> Option<&'static str> {
    examine(file, size).ok().flatten()
}

fn examine(file: &mut (impl Read + Seek), size: u64) -> io::Result<Option<&'static str>> {
    file.rewind()?;
    let mut header = [0u8; 512];
    let head = &mut header[..size.min(512) as usize];
    file.read_exact(head)?;
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head).trim_ascii_start();
    if head.starts_with(b"%PDF-") {
        let mut tail = [0u8; 1024];
        let tail = &mut tail[..size.min(1024) as usize];
        file.seek(SeekFrom::End(-(tail.len() as i64)))?;
        file.read_exact(tail)?;
        return Ok(tail.trim_ascii_end().ends_with(b"%%EOF").then_some(PDF));
    }
    if [b"<!doctype html".as_slice(), b"<html", b"<head", b"<body"].iter().any(|prefix| {
        head.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    }) {
        return Ok(Some("text/html"));
    }
    if !head.starts_with(b"PK\x03\x04") && !head.starts_with(b"PK\x05\x06") {
        return Ok(None);
    }
    // A ZIP comment can occupy 65535 bytes after its 22-byte end record. Inspect the normal
    // bounded central directory only; no entry is decompressed to classify an HTTP receipt.
    let mut tail = vec![0; size.min(65535 + 22) as usize];
    file.seek(SeekFrom::End(-(tail.len() as i64)))?;
    file.read_exact(&mut tail)?;
    let Some(end) = tail.windows(22).enumerate().rev().find_map(|(at, row)| {
        let comment = u16::from_le_bytes([row[20], row[21]]);
        (row.starts_with(b"PK\x05\x06") && at + 22 + usize::from(comment) == tail.len())
            .then_some(at)
    }) else {
        return Ok(None);
    };
    let row = &tail[end..end + 22];
    let directory_at = u32::from_le_bytes([row[16], row[17], row[18], row[19]]);
    let directory_bytes = u32::from_le_bytes([row[12], row[13], row[14], row[15]]);
    if row[4..8] != [0; 4]
        || row[8..10] != row[10..12]
        || u64::from(directory_at) + u64::from(directory_bytes)
            != size - tail.len() as u64 + end as u64
        || directory_bytes > 8 * 1024 * 1024
        || u16::from_le_bytes([row[10], row[11]]) > 8192
    {
        return Ok(None);
    }
    let Ok(archive) = zip::ZipArchive::new(file) else { return Ok(None) };
    let word = archive.index_for_name("word/document.xml").is_some()
        && archive.index_for_name("[Content_Types].xml").is_some();
    Ok(Some(if word { DOCX } else { ZIP }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn downloaded_media_comes_from_the_file_and_incomplete_documents_stay_unconfirmed() {
        let probe = |data: &[u8]| {
            examine(&mut Cursor::new(data), data.len() as u64).expect("inspect sample bytes")
        };
        assert_eq!(probe(b"%PDF-1.7\nbody\n%%EOF\n"), Some(PDF));
        assert_eq!(probe(b"%PDF-1.7\npartial body"), None);
        assert_eq!(probe(b"\xef\xbb\xbf <!DOCTYPE HTML><form>"), Some("text/html"));
        assert_eq!(probe(b"PK\x03\x04partial"), None);
        for is_word in [false, true] {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for path in ["word/document.xml", if is_word { "[Content_Types].xml" } else { "other" }]
            {
                writer
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .expect("start ZIP entry");
                writer.write_all(b"document").expect("write ZIP entry");
            }
            let bytes = writer.finish().expect("finish sample ZIP").into_inner();
            assert_eq!(probe(&bytes), Some(if is_word { DOCX } else { ZIP }));
        }
    }
}
