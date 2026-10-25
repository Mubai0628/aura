//! Command directories in a temporary root; nothing here reaches a real configuration.
#![allow(dead_code)]
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "name = \"review\"\ntitle = \"审查改动\"\ndescription = \"列出本次改动的风险\"\nargument_hint = \"范围（可选）\"\nversion = 1\n";
pub const TEMPLATE: &str = "请审查 $ARGUMENTS 的改动，列出风险。\n";

pub fn root() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary commands root")
}

/// Writes `<root>/<name>/command.toml` and `template.md`; `None` leaves a file out.
pub fn command(root: &Path, name: &str, manifest: Option<&str>, template: Option<&str>) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(&directory).expect("command directory");
    if let Some(manifest) = manifest {
        std::fs::write(directory.join("command.toml"), manifest).expect("manifest");
    }
    if let Some(template) = template {
        std::fs::write(directory.join("template.md"), template).expect("template");
    }
    directory
}

pub fn manifest_named(name: &str) -> String {
    MANIFEST.replace("\"review\"", &format!("\"{name}\""))
}
