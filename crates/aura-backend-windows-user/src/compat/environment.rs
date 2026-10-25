//! Trusted desktop environment only; a shell request has no environment override field.
use anyhow::{Result, ensure};
use aura_windows_platform::{NativeEnvironmentEntry, windows_ordinal_compare_ignore_case};
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    net::SocketAddr,
    os::windows::ffi::OsStrExt,
};

fn excluded(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("AURA_")
        || name.starts_with("CODEX_")
        || ["OPENAI_", "ANTHROPIC_", "DEEPSEEK_"].iter().any(|prefix| name.starts_with(prefix))
        || name
            .split('_')
            .any(|part| matches!(part, "TOKEN" | "KEY" | "SECRET" | "PASSWORD" | "CREDENTIAL"))
        || name == "__COMPAT_LAYER"
}

pub fn excluded_names(trusted: &[(OsString, OsString)]) -> Vec<String> {
    trusted
        .iter()
        .filter_map(|(name, _)| name.to_str().filter(|name| excluded(name)).map(str::to_owned))
        .collect()
}

pub fn build(
    trusted: impl IntoIterator<Item = (OsString, OsString)>,
    offline_proxy: Option<&str>,
) -> Result<Vec<NativeEnvironmentEntry>> {
    let mut entries: Vec<NativeEnvironmentEntry> = Vec::new();
    for (name, value) in trusted {
        let name = name
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("compat environment name is not representable"))?;
        ensure!(!name.is_empty() && !name.contains(['=', '\0']), "invalid compat environment name");
        if excluded(name) {
            continue;
        }
        for previous in &entries {
            ensure!(
                windows_ordinal_compare_ignore_case(OsStr::new(&previous.name), OsStr::new(name))?
                    != Ordering::Equal,
                "duplicate compat environment name"
            );
        }
        let value = if name.eq_ignore_ascii_case("PATH") {
            let text = value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("compat PATH is not representable"))?;
            text.split(';')
                .filter(|segment| !segment.trim().is_empty())
                .collect::<Vec<_>>()
                .join(";")
                .encode_utf16()
                .collect()
        } else {
            value.encode_wide().collect::<Vec<_>>()
        };
        ensure!(!value.contains(&0), "NUL in compat environment value");
        entries.push(NativeEnvironmentEntry { name: name.to_owned(), value_utf16: value });
    }
    if let Some(proxy) = offline_proxy {
        let endpoint: SocketAddr = proxy
            .strip_prefix("http://")
            .ok_or_else(|| anyhow::anyhow!("owned HTTP reject proxy required"))?
            .trim_end_matches('/')
            .parse()?;
        ensure!(endpoint.ip().is_loopback(), "soft offline proxy must be local");
        for name in
            ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NPM_CONFIG_PROXY", "NPM_CONFIG_HTTPS_PROXY"]
        {
            replace(&mut entries, name, proxy);
        }
        for name in ["NO_PROXY", "NPM_CONFIG_NOPROXY"] {
            replace(&mut entries, name, "");
        }
        if !entries.iter().any(|entry| entry.name.eq_ignore_ascii_case("NODE_USE_ENV_PROXY")) {
            replace(&mut entries, "NODE_USE_ENV_PROXY", "1");
        }
    }
    Ok(entries)
}

fn replace(entries: &mut Vec<NativeEnvironmentEntry>, name: &str, value: &str) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.name.eq_ignore_ascii_case(name)) {
        entry.value_utf16 = value.encode_utf16().collect();
        return;
    }
    entries.push(NativeEnvironmentEntry {
        name: name.into(),
        value_utf16: value.encode_utf16().collect(),
    });
}
