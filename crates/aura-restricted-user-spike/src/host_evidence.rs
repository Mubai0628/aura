//! Read-only host-preparation evidence.
//!
//! This module answers one question with the machine's real state: which of the
//! facts the AppContainer host preparation needs are already true, which are
//! missing, and exactly what the existing broker plan would add or remove. It
//! performs no write of any kind — no ACL change, no integrity label, no device
//! descriptor — and it is not a route, a Core capability or a product entry
//! point.
//!
//! Two things are reported separately, because they answer different questions:
//!
//! * what the *current implementation* requires (probe results plus the frozen
//!   constants it compares against), and
//! * what the machine actually looks like right now (the raw DACL entries and
//!   the NUL device's security descriptor), so a reviewer can see the delta
//!   instead of trusting a summary.
use serde_json::{Value, json};

use aura_windows_platform::installation_identity::{
    self, InstallationIdentity, NullDeviceVerdict, published_installations_base,
    verify_installation_identity,
};

/// Everything the read-only investigation needs, in one JSON object.
pub fn report() -> Value {
    let probe = aura_windows_platform::probe_host_compatibility();
    let expected_null = aura_windows_platform::NULL_DEVICE_COMPATIBILITY_SDDL;
    let current_null = aura_windows_platform::capture_null_device_security_descriptor();
    let system_drive = aura_windows_platform::system_drive_root();
    let readiness = installation_identity::probe_null_device_readiness();
    let identity = published_installations_base()
        .map(|base| verify_installation_identity(&base))
        .map_err(|error| error.to_string());
    let system_drive_probe =
        system_drive.as_ref().map_err(|error| error.to_string()).and_then(|root| {
            aura_windows_platform::probe_system_drive_compatibility(root)
                .map_err(|error| error.to_string())
        });
    json!({
        "read_only": true,
        "process": {
            "elevated": aura_windows_platform::current_process_is_elevated().unwrap_or(false),
            "user": std::env::var("USERNAME").unwrap_or_else(|_| "unknown".to_owned()),
        },
        "probe": {
            "system_drive_root": system_drive
                .as_ref()
                .map(|root| root.display().to_string())
                .map_err(|error| error.to_string()),
            "outcome": probe
                .as_ref()
                .map(|probe| json!({
                    "all_application_packages_ready": probe.all_application_packages_ready,
                    "all_restricted_application_packages_ready":
                        probe.all_restricted_application_packages_ready,
                    "system_drive_ready": probe.system_drive_ready,
                    "null_device_ready": probe.null_device_ready,
                }))
                .map_err(|error| error.to_string()),
            "system_drive_compatibility": system_drive_probe
                .as_ref()
                .map_err(|error| error.to_string()),
            "null_device_compatibility": aura_windows_platform::probe_null_device_compatibility()
                .map_err(|error| error.to_string()),
        },
        // The rule the product's gates and plans now use, reported next to the
        // strict one so the difference between "the frozen constant alone" and
        // "the frozen constant plus a verified installation's capability" is
        // visible rather than implied.
        "readiness_rule": {
            "published_installation_base": published_installations_base()
                .map(|base| base.display().to_string())
                .map_err(|error| error.to_string()),
            "installation_identity": identity
                .as_ref()
                .map(identity_json)
                .map_err(|error| error.to_string()),
            "null_device_verdict": verdict_json(&readiness),
            "capability_aces_present":
                capability_aces(current_null.as_deref().unwrap_or_default(), identity.as_ref()),
            "consumer":
                "aura_windows_platform::installation_identity::probe_null_device_readiness",
        },
        "required_facts": {
            "all_application_packages_sid": aura_windows_platform::ALL_APPLICATION_PACKAGES_SID,
            "all_restricted_application_packages_sid":
                aura_windows_platform::ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
            "system_drive_mask_hex": format!(
                "0x{:08X}",
                aura_windows_platform::SYSTEM_DRIVE_COMPATIBILITY_MASK
            ),
            "system_drive_inheritance": 0,
            "null_device_expected_sddl": expected_null,
            "gated_isolation_modes": ["compatibility_appcontainer", "strict_appcontainer"],
            "consumer":
                "crates/aura-sandbox/src/lib.rs::probe_host_compatibility before planning",
        },
        "current": {
            "null_device_sddl": current_null.as_ref().map_err(|error| error.to_string()),
            "null_device_entry_diff": null_device_entry_diff(
                current_null.as_deref().unwrap_or_default(),
                expected_null,
            ),
            "system_drive_acl": system_drive_acl(),
        },
    })
}

/// The `C:\` DACL as raw entries, read through the same API the product uses, so
/// the delta is visible per entry instead of summarised as a boolean.
fn system_drive_acl() -> Value {
    let Ok(root) = aura_windows_platform::system_drive_root() else {
        return json!({ "error": "system drive root is unavailable" });
    };
    match aura_windows_platform::capture_file_dacl(&root) {
        Ok(sddl) => {
            let entries = sddl_entries(&sddl);
            let required = entries
                .iter()
                .filter(|entry| {
                    let trustee = canonical_trustee(&entry.trustee);
                    trustee == aura_windows_platform::ALL_APPLICATION_PACKAGES_SID
                        || trustee == aura_windows_platform::ALL_RESTRICTED_APPLICATION_PACKAGES_SID
                })
                .map(entry_json)
                .collect::<Vec<_>>();
            json!({
                "sddl": sddl,
                "entries": entries.iter().map(entry_json).collect::<Vec<_>>(),
                "required_sid_entries": required,
            })
        }
        Err(error) => json!({ "error": error.to_string() }),
    }
}

fn entry_json(entry: &SddlEntry) -> Value {
    json!({
        "trustee": entry.trustee,
        "rights": entry.rights,
        "flags": entry.flags,
        "rights_mask": entry.rights_mask().map(|mask| format!("0x{mask:08X}")),
    })
}

/// The SID behind a trustee as the descriptor prints it.
///
/// Windows abbreviates well-known SIDs (`AC` for ALL APPLICATION PACKAGES, `RC`
/// for ALL RESTRICTED APPLICATION PACKAGES, `BA`/`SY`/`BU`/`AU`/`WD` for the rest),
/// so comparing the printed text against a literal SID silently misses exactly the
/// entries a host-preparation review is looking for — it reported "0 required
/// entries" on a volume that already had both.
fn canonical_trustee(trustee: &str) -> &str {
    match trustee {
        "AC" => aura_windows_platform::ALL_APPLICATION_PACKAGES_SID,
        "RC" => aura_windows_platform::ALL_RESTRICTED_APPLICATION_PACKAGES_SID,
        other => other,
    }
}

/// Entry-level difference between the NUL device's descriptor today and the one
/// the frozen constant asks for.
///
/// Reported per ACE because that is what a reviewer has to approve: an addition,
/// a change of granted rights, or the removal of a grant that exists today.
fn null_device_entry_diff(current: &str, expected: &str) -> Value {
    let current_entries = sddl_entries(current);
    let expected_entries = sddl_entries(expected);
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for entry in &expected_entries {
        match current_entries.iter().find(|candidate| candidate.trustee == entry.trustee) {
            Some(existing) if rights_agree(existing, entry) => {}
            Some(existing) => changed.push(json!({
                "trustee": entry.trustee,
                "current": entry_json(existing),
                "expected": entry_json(entry),
            })),
            None => added.push(entry_json(entry)),
        }
    }
    for entry in &current_entries {
        if !expected_entries.iter().any(|candidate| candidate.trustee == entry.trustee) {
            removed.push(entry_json(entry));
        }
    }
    json!({
        "current_entries": current_entries.iter().map(entry_json).collect::<Vec<_>>(),
        "expected_entries": expected_entries.iter().map(entry_json).collect::<Vec<_>>(),
        "added": added,
        "changed": changed,
        "removed": removed,
        "labels": {
            "current": sddl_labels(current),
            "expected": sddl_labels(expected),
        },
        "owner_group": {
            "current": sddl_owner_group(current),
            "expected": sddl_owner_group(expected),
            "note": "the capture requests DACL and label information only, so the current owner and group are not observed here and must be checked before applying a whole-descriptor replacement",
        },
        "note": "the frozen constant is applied as a whole descriptor, so every `removed` entry would be deleted by applying it as-is; rights are compared after mapping generic spellings to their numeric form, and ACE flags are compared as well",
    })
}

/// Whether two entries grant the same rights to the same trustee under the same
/// flags. Undecodable spellings compare textually rather than being called equal.
fn rights_agree(left: &SddlEntry, right: &SddlEntry) -> bool {
    if left.flags != right.flags {
        return false;
    }
    match (left.rights_mask(), right.rights_mask()) {
        (Some(left), Some(right)) => left == right,
        _ => left.rights == right.rights,
    }
}

struct SddlEntry {
    rights: String,
    /// ACE header flags (inheritance, inherit-only, audit flags).
    flags: String,
    trustee: String,
}

impl SddlEntry {
    /// Numeric form of the granted rights, so a generic spelling (`GRGWGX`) and
    /// the numeric form the same descriptor prints elsewhere (`0x1201bf`) compare
    /// equal instead of looking like a change. `None` means the token sequence is
    /// not one this tool decodes, and the comparison then falls back to text
    /// rather than pretending it differs.
    ///
    /// These are the file-object mappings of the generic rights:
    /// `GR = 0x120089`, `GW = 0x120116`, `GX = 0x1200A0`.
    fn rights_mask(&self) -> Option<u32> {
        const TOKENS: [(&str, u32); 8] = [
            ("FA", 0x001F_01FF),
            ("GR", 0x0012_0089),
            ("GW", 0x0012_0116),
            ("GX", 0x0012_00A0),
            ("GA", 0x1000_0000),
            ("RC", 0x0002_0000),
            ("SD", 0x0001_0000),
            ("WO", 0x0008_0000),
        ];
        if let Some(hex) = self.rights.strip_prefix("0x") {
            return u32::from_str_radix(hex, 16).ok();
        }
        let mut mask = 0_u32;
        let mut rest = self.rights.as_str();
        while !rest.is_empty() {
            let (token, value) = TOKENS.iter().find(|(token, _)| rest.starts_with(token))?;
            mask |= value;
            rest = &rest[token.len()..];
        }
        (mask != 0).then_some(mask)
    }
}

/// Parse `(A;;rights;;;trustee)` access entries out of an SDDL string.
fn sddl_entries(sddl: &str) -> Vec<SddlEntry> {
    let mut entries = Vec::new();
    let mut rest = sddl;
    while let Some(start) = rest.find('(') {
        let Some(end) = rest[start..].find(')') else {
            break;
        };
        let body = &rest[start + 1..start + end];
        let fields: Vec<&str> = body.split(';').collect();
        if fields.len() >= 6 && (fields[0] == "A" || fields[0] == "D") {
            entries.push(SddlEntry {
                rights: fields[2].to_owned(),
                flags: fields[1].to_owned(),
                trustee: fields[5].to_owned(),
            });
        }
        rest = &rest[start + end + 1..];
    }
    entries
}

/// Integrity-label statements (`S:(ML;;NW;;;LW)`) as text.
fn sddl_labels(sddl: &str) -> Vec<String> {
    sddl.match_indices("S:(")
        .filter_map(|(start, _)| {
            sddl[start..].find(')').map(|end| sddl[start..start + end + 1].to_owned())
        })
        .collect()
}

/// Owner and group as written in the descriptor text, when present.
fn sddl_owner_group(sddl: &str) -> Value {
    let owner = sddl.find("O:").map(|start| {
        let rest = &sddl[start + 2..];
        let end = rest.find(['G', 'D', 'S']).unwrap_or(rest.len());
        rest[..end].to_owned()
    });
    let group = sddl.find("G:").map(|start| {
        let rest = &sddl[start + 2..];
        let end = rest.find(['D', 'S']).unwrap_or(rest.len());
        rest[..end].to_owned()
    });
    json!({ "owner": owner, "group": group })
}

/// The four identity states as text, with the capability each one implies.
fn identity_json(identity: &InstallationIdentity) -> Value {
    match identity {
        InstallationIdentity::Absent => json!({ "state": "absent" }),
        InstallationIdentity::Unverified(reason) => {
            json!({ "state": "unverified", "reason": reason })
        }
        InstallationIdentity::Verified(installation) => json!({
            "state": "verified",
            "root": installation.root.display().to_string(),
            "installation_id": installation.installation_id,
            "recorded_capability_sid": installation.null_capability_sid,
            "capability": match identity.null_capability() {
                Ok(Some(ace)) => json!({ "sid": ace.sid, "mask": format!("0x{:08X}", ace.mask) }),
                Ok(None) => Value::Null,
                Err(reason) => json!({ "error": reason }),
            },
        }),
    }
}

/// The readiness verdict as text plus the reason, so "unanswerable" is visible.
fn verdict_json(verdict: &NullDeviceVerdict) -> Value {
    match verdict {
        NullDeviceVerdict::Ready => json!({ "verdict": "ready" }),
        NullDeviceVerdict::NotReady => json!({ "verdict": "not_ready" }),
        NullDeviceVerdict::Unanswerable(reason) => {
            json!({ "verdict": "unanswerable", "reason": reason })
        }
    }
}

/// Which of the NUL device's entries are Aura capability ACEs, and whether the
/// verified installation's own ACE is among them.
fn capability_aces(sddl: &str, identity: Result<&InstallationIdentity, &String>) -> Value {
    let Ok(identity) = identity else {
        return json!({ "error": "the installation identity is unavailable" });
    };
    let capability_shaped = sddl_entries(sddl)
        .into_iter()
        .filter(|entry| aura_windows_platform::is_capability_sid_shape(&entry.trustee))
        .collect::<Vec<_>>();
    json!({
        "verified_capability_granted": match identity.descriptor_carries_capability(sddl) {
            Ok(value) => json!(value),
            Err(reason) => json!({ "error": reason }),
        },
        "capability_shaped_entries": capability_shaped.iter().map(entry_json).collect::<Vec<_>>(),
        "note": "a capability-shaped SID is not by itself proof of ownership; only the read-only installation verification decides whose ACE it is",
    })
}
