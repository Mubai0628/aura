//! ZIP directory metadata only: no extraction, decryption, or entry-content reads.
use std::{fs::File, io::Seek, path::Path};

pub(super) fn listing(path: &str, file: &mut File) -> Option<String> {
    if !Path::new(path).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip")) {
        return None;
    }
    let result = read(file);
    // The same checked handle may subsequently be used by another reader.
    let _ = file.rewind();
    Some(result.unwrap_or_else(|_| "无法读取 ZIP 清单；文件仍按位置附加，可按需读取。".into()))
}

fn read(file: &mut File) -> Result<String, ::zip::result::ZipError> {
    let mut zip = ::zip::ZipArchive::new(file)?;
    let total = zip.len();
    let mut text = format!("共 {total} 项（仅列路径和未压缩大小，不含文件内容）：");
    for index in 0..total.min(200) {
        let entry = zip.by_index_raw(index)?;
        let name = entry
            .name()
            .chars()
            .flat_map(|c| {
                if c.is_control() {
                    format!("\\u{{{:x}}}", c as u32).chars().collect::<Vec<_>>()
                } else {
                    vec![c]
                }
            })
            .collect::<String>();
        text.push_str(&format!(
            "\n- {name} — {} B{}{}",
            entry.size(),
            if entry.is_dir() { " [目录]" } else { "" },
            if entry.encrypted() { " [加密]" } else { "" }
        ));
    }
    if total > 200 {
        text.push_str(&format!("\n另有 {} 项未列出。", total - 200));
    }
    Ok(text)
}
