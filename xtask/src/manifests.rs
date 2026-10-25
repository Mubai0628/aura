//! The prompt, schema and component manifests every build reads.
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use aura_contract_schema::{COMPONENT_MANIFEST_EXAMPLE_PATH, SCHEMA_MANIFEST_PATH};
use aura_domain_common::ContractVersion;
use aura_runtime_contract::BuildComponentManifest;
use serde_json::Value;

use super::required_string;

pub(crate) fn validate_manifests(root: &Path) -> Result<()> {
    validate_prompt_manifest(root)?;
    validate_schema_manifest(root)?;
    validate_component_manifest(root)?;
    println!("prompt, schema, and component manifests are valid.");
    Ok(())
}

fn validate_prompt_manifest(root: &Path) -> Result<()> {
    let manifest_path = root.join("resources/prompts/manifest.json");
    let manifest = read_json(&manifest_path)?;
    validate_version(&manifest, "manifest_version")?;
    let modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .context("prompt manifest modules must be an array")?;
    let mut ids = BTreeSet::new();

    for module in modules {
        let id = required_string(module, "id")?;
        ensure!(ids.insert(id.to_owned()), "duplicate prompt module ID: {id}");
        let relative = required_string(module, "path")?;
        ensure_safe_relative_path(relative)?;
        ensure!(
            root.join("resources/prompts").join(relative).is_file(),
            "prompt module does not exist: {relative}"
        );
    }
    Ok(())
}

fn validate_schema_manifest(root: &Path) -> Result<()> {
    let manifest_path = root.join(SCHEMA_MANIFEST_PATH);
    let manifest = read_json(&manifest_path)?;
    validate_version(&manifest, "manifest_version")?;
    let schemas = manifest
        .get("schemas")
        .and_then(Value::as_array)
        .context("schema manifest schemas must be an array")?;
    ensure!(!schemas.is_empty(), "schema manifest must contain at least one schema");
    let mut ids = BTreeSet::new();

    for schema in schemas {
        let id = required_string(schema, "id")?;
        ensure!(ids.insert(id.to_owned()), "duplicate schema ID: {id}");
        let relative = required_string(schema, "path")?;
        ensure_safe_relative_path(relative)?;
        let schema_path = root.join("resources/schemas").join(relative);
        let bytes = fs::read(&schema_path)
            .with_context(|| format!("schema does not exist: {}", schema_path.display()))?;
        let hash = schema.get("content_hash").context("schema entry requires content_hash")?;
        ensure!(required_string(hash, "algorithm")? == "blake3", "schema hash must use blake3");
        ensure!(
            required_string(hash, "hex")? == blake3::hash(&bytes).to_hex().as_str(),
            "schema content hash does not match {relative}"
        );
    }
    Ok(())
}

fn validate_component_manifest(root: &Path) -> Result<()> {
    let path = root.join(COMPONENT_MANIFEST_EXAMPLE_PATH);
    let manifest: BuildComponentManifest = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))?;
    ContractVersion::CURRENT.ensure_supported(manifest.manifest_version)?;
    ensure!(!manifest.product_version.is_empty(), "component product version is required");
    ensure!(!manifest.build_commit.is_empty(), "component build commit is required");
    ensure!(!manifest.target_triple.is_empty(), "component target triple is required");
    ensure!(!manifest.components.is_empty(), "component manifest must not be empty");
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read {}", path.display()))?,
    )
    .with_context(|| format!("failed to parse {}", path.display()))
}

fn validate_version(value: &Value, field: &str) -> Result<()> {
    let version = value.get(field).with_context(|| format!("missing {field}"))?;
    let major =
        version.get("major").and_then(Value::as_u64).context("version major must be an integer")?;
    let minor =
        version.get("minor").and_then(Value::as_u64).context("version minor must be an integer")?;
    let major = u16::try_from(major).context("version major exceeds u16")?;
    let minor = u16::try_from(minor).context("version minor exceeds u16")?;
    ContractVersion::CURRENT.ensure_supported(ContractVersion::new(major, minor))?;
    Ok(())
}

fn ensure_safe_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(!path.is_absolute(), "manifest path must be relative: {value}");
    ensure!(
        path.components().all(|component| matches!(component, Component::Normal(_))),
        "manifest path contains an unsafe component: {value}"
    );
    Ok(())
}
