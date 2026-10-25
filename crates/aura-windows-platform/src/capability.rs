//! Read-only capability identity for the install-scoped execution devices.
//!
//! This module is compiled unconditionally: the SID derivation and the exact
//! access mask are pure functions of an installation UUID, and both Core and the
//! standalone sandbox build need them to answer "is this ACE Aura's?". The
//! device *modification* paths (`ExecutionDevice::plan/apply/capture_*`) stay
//! behind `restricted-user-runtime`, because they really do need the execution
//! runtime; nothing here does.
use std::{io, sync::OnceLock};

use sha2::{Digest, Sha256};

/// The access mask Aura grants on the NUL device.
pub const NULL_ACCESS_MASK: u32 = 0x0012_019f;

/// Which device a capability belongs to, by its derivation prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityDevice {
    Null,
    Cng,
    Ksec,
}

impl CapabilityDevice {
    /// The prefix the install plan hashes into the SID.
    #[must_use]
    const fn prefix(self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::Cng => "Cng",
            Self::Ksec => "Ksec",
        }
    }

    /// The exact access mask this device's capability grants.
    #[must_use]
    pub const fn access_mask(self) -> u32 {
        match self {
            Self::Null | Self::Cng => NULL_ACCESS_MASK,
            Self::Ksec => 0x0010_0003,
        }
    }
}

/// Derive the capability SID for `installation_id` on `device`.
///
/// The derivation is the install plan's own (`Aura.R06.{Prefix}Capability.v1/{id}`
/// hashed with SHA-256, read as four little-endian `u32`s), kept in one place so
/// the verification path and the install path can never drift apart.
pub fn capability_sid(device: CapabilityDevice, installation_id: &str) -> io::Result<String> {
    if installation_id.len() != 36
        || !installation_id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
    {
        return Err(io::Error::other("canonical installation UUID required"));
    }
    let hash =
        Sha256::digest(format!("Aura.R06.{}Capability.v1/{installation_id}", device.prefix()));
    let parts = hash[..16]
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four bytes")).to_string())
        .collect::<Vec<_>>();
    Ok(format!("S-1-5-21-{}", parts.join("-")))
}

/// Whether `sid` is shaped like a derived capability SID.
#[must_use]
pub fn is_capability_sid_shape(sid: &str) -> bool {
    let Some(body) = sid.strip_prefix("S-1-5-21-") else {
        return false;
    };
    body.split('-').count() == 4 && body.split('-').all(|part| part.parse::<u32>().is_ok())
}

/// Cached derivation for the NUL device, so a hot verification path does not
/// re-hash for every call. The cache is keyed by installation UUID.
pub fn null_capability_sid(installation_id: &str) -> io::Result<String> {
    static CACHE: OnceLock<std::sync::Mutex<std::collections::BTreeMap<String, String>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(sid) = cache.get(installation_id)
    {
        return Ok(sid.clone());
    }
    let sid = capability_sid(CapabilityDevice::Null, installation_id)?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(installation_id.to_owned(), sid.clone());
    }
    Ok(sid)
}
