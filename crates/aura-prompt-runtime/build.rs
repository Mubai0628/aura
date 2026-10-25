use std::{
    collections::BTreeSet,
    env, fs,
    path::{Component, Path, PathBuf},
};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    bundle_version: String,
    serializer_version: u32,
    modules: Vec<Module>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    id: String,
    version: String,
    path: String,
    required: bool,
    section: String,
    activation: String,
    order: u32,
}

fn main() {
    if let Err(error) = build() {
        panic!("prompt bundle build validation failed: {error}");
    }
}

fn build() -> Result<(), String> {
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let prompt_root = crate_root.join("../../resources/prompts");
    let manifest_path = prompt_root.join("prompt-bundle.toml");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    let manifest: Manifest = toml::from_str(&manifest_text).map_err(|error| error.to_string())?;
    if manifest.schema_version != 1 || manifest.serializer_version != 1 {
        return Err("only prompt manifest schema 1 and serializer 1 are supported".to_owned());
    }
    validate_semver(&manifest.bundle_version)?;
    if manifest.modules.is_empty() {
        return Err("prompt manifest must contain modules".to_owned());
    }
    let mut ids = BTreeSet::new();
    let mut order_keys = BTreeSet::new();
    let mut embedded = String::new();
    embedded.push_str("pub(crate) static EMBEDDED_MODULES: &[EmbeddedPromptModule] = &[\n");
    for module in &manifest.modules {
        validate_module(module)?;
        if !ids.insert(module.id.clone()) {
            return Err(format!("duplicate module id {}", module.id));
        }
        if !order_keys.insert((module.section.clone(), module.order)) {
            return Err(format!("ambiguous order {} in section {}", module.order, module.section));
        }
        let relative = Path::new(&module.path);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!("module {} path escapes prompt root", module.id));
        }
        let path = prompt_root.join(relative);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if !module.required && error.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| format!("module {} is not UTF-8", module.id))?;
        let normalized = normalize_module(text, &module.id)?;
        let hash = sha256_hex(normalized.as_bytes());
        embedded.push_str("    EmbeddedPromptModule {\n");
        for (field, value) in [
            ("id", module.id.as_str()),
            ("version", module.version.as_str()),
            ("path", module.path.as_str()),
            ("section", module.section.as_str()),
            ("activation", module.activation.as_str()),
            ("content_hash_hex", hash.as_str()),
            ("content", normalized.as_str()),
        ] {
            embedded.push_str(&format!("        {field}: {value:?},\n"));
        }
        embedded.push_str(&format!("        required: {},\n", module.required));
        embedded.push_str(&format!("        order: {},\n", module.order));
        embedded.push_str("    },\n");
    }
    embedded.push_str("];\n");
    embedded.push_str(&format!(
        "pub(crate) const EMBEDDED_SCHEMA_VERSION: u32 = {};\n",
        manifest.schema_version
    ));
    embedded.push_str(&format!(
        "pub(crate) const EMBEDDED_BUNDLE_VERSION: &str = {:?};\n",
        manifest.bundle_version
    ));
    embedded.push_str(&format!(
        "pub(crate) const EMBEDDED_SERIALIZER_VERSION: u32 = {};\n",
        manifest.serializer_version
    ));
    embedded.push_str(&format!(
        "pub(crate) const EMBEDDED_MANIFEST_HASH_HEX: &str = {:?};\n",
        sha256_hex(normalize_line_endings(&manifest_text).as_bytes())
    ));
    let output = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?)
        .join("embedded_prompt_bundle.rs");
    fs::write(output, embedded).map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_module(module: &Module) -> Result<(), String> {
    if !is_module_id(&module.id) {
        return Err(format!("module id {} is not canonical", module.id));
    }
    validate_semver(&module.version)?;
    const SECTIONS: &[&str] = &[
        "core",
        "permission",
        "tool_summary",
        "workspace_agents",
        "historical_snapshot",
        "agent_mode",
        "hook",
        "task_state",
        "compaction",
        "maintenance",
    ];
    if !SECTIONS.contains(&module.section.as_str()) {
        return Err(format!("module {} has unknown section", module.id));
    }
    if !is_activation(&module.activation) {
        return Err(format!("module {} has unknown activation", module.id));
    }
    Ok(())
}

fn validate_semver(value: &str) -> Result<(), String> {
    let parsed = Version::parse(value).map_err(|_| format!("invalid SemVer {value}"))?;
    if parsed.to_string() != value {
        return Err(format!("non-canonical SemVer {value}"));
    }
    Ok(())
}

fn is_module_id(value: &str) -> bool {
    !value.is_empty()
        && value.split(['.', '-']).all(|part| {
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn is_activation(value: &str) -> bool {
    value == "always"
        || value == "child_agent"
        || value == "compaction"
        || value.starts_with("agent_mode:")
        || value.starts_with("capability:")
        || value.starts_with("recovery:")
        || value.starts_with("maintenance:")
}

fn normalize_module(value: &str, id: &str) -> Result<String, String> {
    if value.starts_with('\u{feff}') {
        return Err(format!("module {id} contains a BOM"));
    }
    let normalized = normalize_line_endings(value);
    if normalized.contains("</Aura-") {
        return Err(format!("module {id} contains a reserved closing tag"));
    }
    if normalized.chars().any(|character| {
        (character <= '\u{001f}' && !matches!(character, '\n' | '\t')) || character == '\u{007f}'
    }) {
        return Err(format!("module {id} contains a forbidden control character"));
    }
    Ok(format!("{}\n", normalized.trim_end_matches('\n')))
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
