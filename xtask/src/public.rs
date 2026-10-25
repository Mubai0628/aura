//! Checks for a source tree before it is published: credentials found by Aura's own secret
//! scanner, reported by file, line and kind, with the line's SHA-256 instead of its text.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};

const SKIPPED: [&str; 3] = [".git", "node_modules", "target"];
const LARGEST: u64 = 16 * 1024 * 1024;

fn walk(folder: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(folder).with_context(|| format!("cannot list {}", folder.display()))?
    {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if !SKIPPED.iter().any(|name| entry.file_name() == *name) {
                walk(&entry.path(), out)?;
            }
        } else if kind.is_file() {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Prints one JSON line per detection and a summary; text files only, binary files are skipped.
pub(crate) fn scan_secrets(folder: Option<String>) -> Result<()> {
    let root = PathBuf::from(folder.context("scan-secrets <folder>")?);
    let mut files = Vec::new();
    walk(&root, &mut files)?;
    files.sort();
    let (mut scanned, mut found) = (0_usize, 0_usize);
    for path in &files {
        if fs::metadata(path)?.len() > LARGEST {
            continue;
        }
        let Ok(text) = String::from_utf8(fs::read(path)?) else { continue };
        scanned += 1;
        let relative = path.strip_prefix(&root)?.to_string_lossy().replace('\\', "/");
        for (index, line) in text.lines().enumerate() {
            if let Some(kind) = aura_secret_scanner::detect_text(line) {
                found += 1;
                println!(
                    "{}",
                    json!({
                        "path": relative,
                        "line": index + 1,
                        "kind": kind,
                        "line_sha256": format!("{:x}", Sha256::digest(line.trim().as_bytes())),
                    })
                );
            }
        }
    }
    println!("{}", json!({ "scanned_files": scanned, "detections": found }));
    Ok(())
}
