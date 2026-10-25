use anyhow::{Result, ensure};
use aura_windows_platform::NativeEnvironmentEntry;
use std::{ffi::OsString, path::Path};

/// Retains Shell's reject proxy for exactly the lifetime of this invocation.
pub struct ProgramEnvironment {
    entries: Vec<NativeEnvironmentEntry>,
    _proxy: Option<crate::compat::soft_proxy::SoftProxy>,
}
impl ProgramEnvironment {
    pub fn current(
        declared: &[(String, String)],
        ordinary_network: bool,
        scratch: &Path,
    ) -> Result<Self> {
        Self::from_block(
            aura_windows_platform::current_user_environment()?,
            declared,
            ordinary_network,
            scratch,
        )
    }

    pub fn from_block(
        block: impl IntoIterator<Item = (OsString, OsString)>,
        declared: &[(String, String)],
        ordinary_network: bool,
        scratch: &Path,
    ) -> Result<Self> {
        ensure!(scratch.is_absolute(), "package.absolute_scratch_required");
        let proxy = if ordinary_network {
            None
        } else {
            Some(crate::compat::soft_proxy::SoftProxy::open()?)
        };
        let mut entries = crate::compat::environment::build(
            block,
            proxy.as_ref().map(|value| value.endpoint.as_str()),
        )?;
        let mut names = std::collections::BTreeSet::new();
        for (name, value) in declared {
            ensure!(
                !name.is_empty()
                    && !name.contains(['=', '\0'])
                    && !value.contains('\0')
                    && names.insert(name.to_ascii_uppercase()),
                "package.invalid_environment_mapping"
            );
            replace(&mut entries, name, value);
        }
        // A declaration cannot turn off the no-network hint. It is an environment
        // hint only, exactly as for Shell, never an enforcement guarantee.
        if let Some(proxy) = &proxy {
            for name in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NPM_CONFIG_PROXY",
                "NPM_CONFIG_HTTPS_PROXY",
            ] {
                replace(&mut entries, name, &proxy.endpoint);
            }
            for name in ["NO_PROXY", "NPM_CONFIG_NOPROXY"] {
                replace(&mut entries, name, "");
            }
            replace(&mut entries, "NODE_USE_ENV_PROXY", "1");
        }
        let scratch =
            scratch.to_str().ok_or_else(|| anyhow::anyhow!("package.scratch_not_representable"))?;
        for name in ["TEMP", "TMP"] {
            replace(&mut entries, name, scratch);
        }
        Ok(Self { entries, _proxy: proxy })
    }

    pub fn entries(&self) -> &[NativeEnvironmentEntry] {
        &self.entries
    }
}

fn replace(entries: &mut Vec<NativeEnvironmentEntry>, name: &str, value: &str) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.name.eq_ignore_ascii_case(name)) {
        entry.value_utf16 = value.encode_utf16().collect();
    } else {
        entries.push(NativeEnvironmentEntry {
            name: name.into(),
            value_utf16: value.encode_utf16().collect(),
        });
    }
}
