//! Trusted desktop environment only. A command gets a credential of the user's environment only
//! when it declares the variable, and never one of Aura's own.
use anyhow::{Result, ensure};
use aura_windows_platform::{NativeEnvironmentEntry, windows_ordinal_compare_ignore_case};
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    net::SocketAddr,
    os::windows::ffi::OsStrExt,
};

/// What a variable of the user's environment is to a command Aura runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VariableClass {
    /// Aura's own or the compatibility layer's: never passed.
    Internal,
    /// A credential: passed only to a command that declares it, and hidden in output.
    Secret,
    /// Passed as it is.
    Plain,
}

/// Classifies one variable by its name, and by a password in a connection address it holds.
pub fn classify(name: &str, value: &OsStr) -> VariableClass {
    let name = name.to_ascii_uppercase();
    if name.starts_with("AURA_") || name.starts_with("CODEX_") || name == "__COMPAT_LAYER" {
        return VariableClass::Internal;
    }
    let secret_part = |part: &str| {
        matches!(
            part,
            "TOKEN"
                | "TOKENS"
                | "KEY"
                | "SECRET"
                | "SECRETS"
                | "PASSWORD"
                | "PASSWORDS"
                | "PASS"
                | "PASSWD"
                | "PASSPHRASE"
                | "CREDENTIAL"
                | "PWD"
        )
    };
    if ["OPENAI_", "ANTHROPIC_", "DEEPSEEK_"].iter().any(|prefix| name.starts_with(prefix))
        || name == "PGPASSWORD"
        || (name != "PWD" && name.split('_').any(secret_part))
        || (!is_proxy(&name) && value.to_str().and_then(url_password).is_some())
    {
        return VariableClass::Secret;
    }
    VariableClass::Plain
}

fn is_proxy(upper_name: &str) -> bool {
    upper_name.ends_with("PROXY")
}

/// The password of the first `scheme://user:password@` address in a value.
fn url_password(value: &str) -> Option<&str> {
    let (_, rest) = value.split_once("://")?;
    let (userinfo, _) = rest.split_once('@')?;
    let (user, password) = userinfo.split_once(':')?;
    let plain = |text: &str| !text.contains(['/', '\\', ' ', '@']);
    (!user.is_empty() && !password.is_empty() && plain(user) && plain(password)).then_some(password)
}

fn excluded(name: &str, value: &OsStr) -> bool {
    classify(name, value) != VariableClass::Plain
}

pub fn excluded_names(trusted: &[(OsString, OsString)]) -> Vec<String> {
    trusted
        .iter()
        .filter_map(|(name, value)| {
            name.to_str().filter(|name| excluded(name, value)).map(str::to_owned)
        })
        .collect()
}

/// Why a command may not have the variables it declared, if it may not.
///
/// A declared name must be set in the user's environment and must not be Aura's own. Declaring an
/// ordinary variable changes nothing: it is passed anyway.
pub fn check_declared(
    trusted: &[(OsString, OsString)],
    declared: &[String],
) -> std::result::Result<(), String> {
    for wanted in declared {
        let found = trusted
            .iter()
            .find(|(name, _)| name.to_str().is_some_and(|name| name.eq_ignore_ascii_case(wanted)));
        match found {
            None => return Err(format!("{wanted} is not set in the user's environment")),
            Some((name, value)) => {
                if classify(name.to_str().unwrap_or_default(), value) == VariableClass::Internal {
                    return Err(format!("{wanted} is Aura's own variable and is never passed"));
                }
            }
        }
    }
    Ok(())
}

/// The values a command's output must not show, by the name of their variable: every credential
/// of the user's environment, declared or not, and the password of a proxy address.
pub fn secret_values(trusted: &[(OsString, OsString)]) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for (name, value) in trusted {
        let (Some(name), Some(text)) = (name.to_str(), value.to_str()) else { continue };
        let password = url_password(text);
        if classify(name, value) == VariableClass::Secret {
            values.push((name.to_owned(), text.to_owned()));
        }
        if let Some(password) = password {
            values.push((name.to_owned(), password.to_owned()));
        }
    }
    values
}

pub fn build(
    trusted: impl IntoIterator<Item = (OsString, OsString)>,
    offline_proxy: Option<&str>,
) -> Result<Vec<NativeEnvironmentEntry>> {
    build_declared(trusted, offline_proxy, &[])
}

/// The environment of a command that declared the credentials it needs.
pub fn build_declared(
    trusted: impl IntoIterator<Item = (OsString, OsString)>,
    offline_proxy: Option<&str>,
    declared: &[String],
) -> Result<Vec<NativeEnvironmentEntry>> {
    let mut entries: Vec<NativeEnvironmentEntry> = Vec::new();
    for (name, value) in trusted {
        let name = name
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("compat environment name is not representable"))?;
        ensure!(!name.is_empty() && !name.contains(['=', '\0']), "invalid compat environment name");
        match classify(name, &value) {
            VariableClass::Internal => continue,
            VariableClass::Secret
                if !declared.iter().any(|wanted| wanted.eq_ignore_ascii_case(name)) =>
            {
                continue;
            }
            VariableClass::Secret | VariableClass::Plain => {}
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
