//! Text extraction for a document already opened by the file authorization boundary.
use crate::FsToolError;
use aura_domain_common::ContentHash;
use aura_workspace_domain::{FsReadDocumentFormat, FsReadDocumentHeading, TextReadRange};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, Mutex},
};

const ARCHIVE_LIMIT: u64 = 8 * 1024 * 1024;
const XML_LIMIT: u64 = 4 * 1024 * 1024;
const INVALID: &str =
    "fs.document_invalid: 文档格式无效或不完整，未能提取文本。请核对原文件或尝试重新导出。";
const TOO_LARGE: &str = "fs.document_too_large: 文档超过安全提取上限，没有返回文本。可将需要的内容导出为较小的文档或文本再读取。";
const PDF: &str = "fs.pdf_text_unavailable: 已识别为 PDF。fs.read 暂不提取 PDF 文本。可使用已配置的 PDF 读取工具，或在可用的浏览器中查看；若有 pdftotext，可在已授权范围内提取文字。";

impl crate::FsToolService {
    /// Extract text from the verified disk handle used for final delivery checks.
    pub fn read_opened_delivery_text(
        &self,
        file: &mut File,
        path: &Path,
    ) -> Result<String, FsToolError> {
        if let Some(kind) = kind(path) {
            return Ok(read(file, path, kind, &TextReadRange::Full, &self.policy)?.text);
        }
        const TEXT_LIMIT: u64 = 2 * 1024 * 1024;
        let mut bytes = Vec::new();
        file.take(TEXT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| crate::error::io_error("read delivery file", path, error))?;
        if bytes.len() as u64 > TEXT_LIMIT {
            return Err(FsToolError::Document(TOO_LARGE));
        }
        Ok(aura_workspace::decode_text(&bytes)?.text)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DocumentKind {
    Docx,
    Pdf,
}

pub(crate) fn kind(path: &Path) -> Option<DocumentKind> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("docx") {
        Some(DocumentKind::Docx)
    } else if extension.eq_ignore_ascii_case("pdf") {
        Some(DocumentKind::Pdf)
    } else {
        None
    }
}

pub(crate) struct DocumentSnapshot {
    pub text: String,
    pub logical_hash: ContentHash,
    pub disk_hash: ContentHash,
    pub total_characters: u64,
    pub total_lines: u64,
    pub peak_retained_bytes: usize,
    pub format: FsReadDocumentFormat,
    pub headings: Vec<FsReadDocumentHeading>,
}

struct ExtractedDocument {
    text: String,
    headings: Vec<FsReadDocumentHeading>,
}

// Look up only after a verified handle supplied these bytes. A replacement with another
// version cannot reuse cached text even when its timestamp, path and size are unchanged.
static DOCX_CACHE: Mutex<VecDeque<(ContentHash, Arc<ExtractedDocument>)>> =
    Mutex::new(VecDeque::new());

fn extract(bytes: &[u8], hash: &ContentHash) -> Result<Arc<ExtractedDocument>, FsToolError> {
    if let Some(cached) = DOCX_CACHE.lock().ok().and_then(|cache| {
        cache.iter().find(|(key, _)| key == hash).map(|(_, document)| Arc::clone(document))
    }) {
        return Ok(cached);
    }
    let value = Arc::new(docx_text(bytes)?);
    if let Ok(mut cache) = DOCX_CACHE.lock() {
        if cache.len() >= 4 {
            cache.pop_front();
        }
        cache.push_back((hash.clone(), Arc::clone(&value)));
    }
    Ok(value)
}

pub(crate) fn read(
    file: &mut File,
    path: &Path,
    kind: DocumentKind,
    range: &TextReadRange,
    policy: &crate::FsToolPolicy,
) -> Result<DocumentSnapshot, FsToolError> {
    let io_error = |error| crate::error::io_error("read document", path, error);
    if matches!(kind, DocumentKind::Pdf) {
        let mut signature = Vec::with_capacity(1024);
        file.take(1024).read_to_end(&mut signature).map_err(io_error)?;
        return Err(FsToolError::Document(if signature.windows(5).any(|part| part == b"%PDF-") {
            PDF
        } else {
            INVALID
        }));
    }
    let length = file.metadata().map_err(io_error)?.len();
    if length > ARCHIVE_LIMIT {
        return Err(FsToolError::Document(TOO_LARGE));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(ARCHIVE_LIMIT + 1).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 > ARCHIVE_LIMIT {
        return Err(FsToolError::Document(TOO_LARGE));
    }
    let disk_hash = ContentHash::sha256(&bytes);
    let document = extract(&bytes, &disk_hash)?;
    let text = &document.text;
    crate::read::enforce_text_policy(policy, text.len(), range)?;
    Ok(DocumentSnapshot {
        total_characters: text.chars().count() as u64,
        total_lines: text.lines().count() as u64,
        logical_hash: ContentHash::sha256(text.as_bytes()),
        disk_hash,
        text: crate::read::select_text_range(text, range)?,
        peak_retained_bytes: bytes.len() + text.len(),
        format: FsReadDocumentFormat::Docx,
        headings: document.headings.clone(),
    })
}

fn docx_text(bytes: &[u8]) -> Result<ExtractedDocument, FsToolError> {
    let invalid = |_| FsToolError::Document(INVALID);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    let part = archive.by_name("word/document.xml").map_err(invalid)?;
    if part.size() > XML_LIMIT {
        return Err(FsToolError::Document(TOO_LARGE));
    }
    let mut xml = String::with_capacity(part.size() as usize);
    part.take(XML_LIMIT + 1)
        .read_to_string(&mut xml)
        .map_err(|_| FsToolError::Document(INVALID))?;
    if xml.len() as u64 > XML_LIMIT {
        return Err(FsToolError::Document(TOO_LARGE));
    }
    let doc = roxmltree::Document::parse_with_options(
        &xml,
        roxmltree::ParsingOptions { nodes_limit: 200_000, ..Default::default() },
    )
    .map_err(|_| FsToolError::Document(INVALID))?;
    let is_word = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.has_tag_name(("http://schemas.openxmlformats.org/wordprocessingml/2006/main", name))
            || node.has_tag_name(("http://purl.oclc.org/ooxml/wordprocessingml/main", name))
    };
    if !is_word(doc.root_element(), "document") {
        return Err(FsToolError::Document(INVALID));
    }
    let body = doc
        .root_element()
        .children()
        .find(|node| is_word(*node, "body"))
        .ok_or(FsToolError::Document(INVALID))?;
    let mut text = String::new();
    let mut headings = Vec::new();
    let mut line = 0;
    let mut first_paragraph = true;
    for node in body.descendants() {
        if is_word(node, "p") {
            if !first_paragraph {
                text.push('\n');
                line += 1;
            }
            first_paragraph = false;
            if headings.len() < 32 && heading(node) {
                let name = node
                    .descendants()
                    .filter(|child| is_word(*child, "t"))
                    .filter_map(|child| child.text())
                    .flat_map(str::chars)
                    .take(120)
                    .collect::<String>();
                if !name.trim().is_empty() {
                    headings.push(FsReadDocumentHeading {
                        text: name.trim().to_owned(),
                        start_line_index: line,
                    });
                }
            }
        } else if is_word(node, "t") {
            text.push_str(node.text().unwrap_or_default());
        } else if is_word(node, "tab") {
            text.push('\t');
        } else if is_word(node, "br") || is_word(node, "cr") {
            text.push('\n');
            line += 1;
        }
    }
    Ok(ExtractedDocument { text, headings })
}

fn heading(paragraph: roxmltree::Node<'_, '_>) -> bool {
    let namespace = paragraph.tag_name().namespace();
    let has_name = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.is_element()
            && node.tag_name().namespace() == namespace
            && node.tag_name().name() == name
    };
    paragraph.children().find(|child| has_name(*child, "pPr")).is_some_and(|properties| {
        properties.children().any(|part| {
            let Some(value) = part
                .attributes()
                .find(|a| a.name() == "val" && a.namespace() == namespace)
                .map(|a| a.value())
            else {
                return false;
            };
            if has_name(part, "outlineLvl") {
                return value.parse::<u8>().is_ok_and(|level| level <= 8);
            }
            has_name(part, "pStyle")
                && ["heading", "标题"].iter().any(|key| {
                    value
                        .to_lowercase()
                        .strip_prefix(key)
                        .is_some_and(|rest| rest.trim().parse::<u8>().is_ok())
                })
        })
    })
}

#[cfg(test)]
#[test]
fn docx_text_uses_only_bounded_document_xml_and_preserves_paragraphs() {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default())
        .expect("start DOCX XML entry");
    zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A &amp; B</w:t><w:tab/><w:t>C</w:t></w:r></w:p><w:p><w:r><w:t>D</w:t></w:r></w:p></w:body></w:document>"#).expect("write DOCX XML fixture");
    let blob = zip.finish().expect("finish DOCX fixture").into_inner();
    assert_eq!(docx_text(&blob).expect("DOCX text").text, "A & B\tC\nD");
    assert!(docx_text(b"PK\x03\x04bad").is_err());
}

#[cfg(test)]
#[test]
fn docx_headings_point_at_the_actual_extracted_lines() {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("word/document.xml", zip::write::SimpleFileOptions::default())
        .expect("start DOCX XML entry");
    zip.write_all(br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>One</w:t><w:br/><w:t>Two</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Evidence</w:t></w:r></w:p></w:body></w:document>"#).expect("write DOCX XML fixture");
    let blob = zip.finish().expect("finish DOCX fixture").into_inner();
    let hash = ContentHash::sha256(&blob);
    let document = extract(&blob, &hash).expect("extract DOCX headings");
    let cached = extract(&blob, &hash).expect("retrieve cached DOCX text");
    assert!(Arc::ptr_eq(&document, &cached));
    assert_eq!(document.headings[0].start_line_index, 2);
    assert_eq!(document.text.lines().nth(2), Some(document.headings[0].text.as_str()));
}
