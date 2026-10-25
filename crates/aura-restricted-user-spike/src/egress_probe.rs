//! Fixed D0 diagnostic payload under the existing write-restricted launch path.
//! No caller script, host policy change, production route or isolation verdict.
use anyhow::{Result, bail, ensure};
use aura_windows_platform::{
    capture_file_dacl, current_user_token_identity, grant_empty_execution_write_root,
    inspect_existing_path, restore_file_dacl,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    sync::mpsc,
    time::Duration,
};

const SCENARIOS: [&str; 9] = [
    "dns_custom",
    "dns_system",
    "getaddrinfo",
    "dotnet_dns",
    "resolve_dnsname",
    "nslookup",
    "winhttp",
    "udp",
    "tcp",
];

fn validate(scenario: &str, peer: &str, canary: &str) -> Result<u8> {
    ensure!(SCENARIOS.contains(&scenario), "fixed D0 scenario required");
    let family = match peer {
        "192.0.2.2" | "127.0.0.1" => 4,
        "fd00:a06:d0::2" | "::1" => 6,
        _ => bail!("reviewed isolated peer or loopback required"),
    };
    ensure!(
        canary.len() == 32
            && canary.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "fixed random Canary format required"
    );
    Ok(family)
}

pub fn run(root: &Path, scenario: &str, peer: &str, canary: &str) -> Result<Value> {
    let family = validate(scenario, peer, canary)?;
    let (owner, observation) = with_attempt(root, |attempt, sid, id, runtime| {
        for (name, source) in [
            ("invoke-probe.ps1", include_str!("../../../scripts/r06/egress/invoke-probe.ps1")),
            ("dns-api.cs", include_str!("../../../scripts/r06/egress/dns-api.cs")),
            ("token-api.cs", include_str!("../../../scripts/r06/egress/token-api.cs")),
            ("winhttp-api.cs", include_str!("../../../scripts/r06/egress/winhttp-api.cs")),
        ] {
            fs::write(attempt.join("allowed").join(name), source)?;
        }
        let command = format!(
            "& ./invoke-probe.ps1 -Scenario '{scenario}' -Peer '{peer}' -Canary '{canary}' -Family {family} -GateFromStdin"
        );
        let mut child = crate::monitor::launch(
            sid,
            &runtime.join("pwsh.exe"),
            &aura_backend_windows_user::environment::powershell_command(&command),
            attempt,
            Some(id),
            Some(runtime),
        )?;
        println!(
            "{}",
            json!({"event":"d0_child_ready","pid":child.process_id(),"canary":canary,"scenario":scenario,"capture_gate":"stdin-start","write_restricted":true})
        );
        io::stdout().flush()?;
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = io::stdin().read_line(&mut line).map(|_| line);
            let _ = send.send(result);
        });
        let gate = receive.recv_timeout(Duration::from_secs(120))??;
        ensure!(gate.trim_end_matches(['\r', '\n']) == "start", "fixed D0 gate required");
        child.write_stdin(b"start\n")?;
        crate::monitor::collect_until(&mut child, false, 65_536, Duration::from_secs(20))
    })?;
    Ok(
        json!({"schema":"r06-d0-restricted-probe-v1","scenario":scenario,"canary":canary,"peer":peer,"family":family,"dedicated_identity":owner,"write_restricted_child":true,"acl_restored":true,"observation":observation,"d0_complete":false,"strong_offline":"not_proven"}),
    )
}

pub(crate) fn with_attempt(
    root: &Path,
    action: impl FnOnce(&Path, &str, &str, &Path) -> Result<Value>,
) -> Result<(String, Value)> {
    let record: Value = serde_json::from_slice(&fs::read(root.join("record.json"))?)?;
    let (owner, restricted) = current_user_token_identity()?;
    ensure!(
        !restricted && record["offline_sid"] == owner && record["state"] == "installed",
        "dedicated installed identity required"
    );
    aura_backend_windows_user::runtime_package::verify(root, &record)?;
    let id = record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("install ID"))?;
    let runtime = root.join("runtime");
    let attempt = tempfile::Builder::new().prefix("r06-d0-").tempdir_in(root.join("scratch"))?;
    let allowed = attempt.path().join("allowed");
    fs::create_dir(&allowed)?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(anyhow::Error::msg)?;
    let sid = format!(
        "S-1-5-21-{}",
        random
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")).max(1).to_string())
            .collect::<Vec<_>>()
            .join("-")
    );
    let before = capture_file_dacl(&allowed)?;
    let identity = inspect_existing_path(&allowed, false)?.identity;
    fs::write(
        attempt.path().join("acl-journal.json"),
        serde_json::to_vec(&json!({"path":allowed,"before":before,"sid":sid,"pending":"grant"}))?,
    )?;
    if let Err(error) = grant_empty_execution_write_root(&allowed, &sid) {
        bail!("D0 grant failed: {error}; preserve {}", attempt.keep().display());
    }
    let outcome = (|| -> Result<Value> {
        let applied = capture_file_dacl(&allowed)?;
        fs::write(
            attempt.path().join("acl-journal.json"),
            serde_json::to_vec(
                &json!({"path":allowed,"before":before,"applied":applied,"sid":sid}),
            )?,
        )?;
        let observation = action(attempt.path(), &sid, id, &runtime);
        ensure!(
            inspect_existing_path(&allowed, false)?.identity == identity
                && capture_file_dacl(&allowed)? == applied,
            "D0 ACL or identity drift"
        );
        restore_file_dacl(&allowed, &before)?;
        ensure!(
            crate::probe::restored_dacl_matches(&before, &capture_file_dacl(&allowed)?),
            "D0 ACL restore drift"
        );
        observation
    })();
    match outcome {
        Ok(value) => Ok((owner, value)),
        Err(error) => bail!("{error}; preserve D0 fixture {}", attempt.keep().display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_probe_scope_rejects_scripts_destinations_and_malformed_canaries() {
        let canary = "0123456789abcdef0123456789abcdef";
        for scenario in SCENARIOS {
            assert_eq!(
                validate(scenario, "192.0.2.2", canary).expect("fixed IPv4 fixture scope"),
                4
            );
        }
        assert_eq!(
            validate("dns_custom", "fd00:a06:d0::2", canary).expect("fixed IPv6 fixture scope"),
            6
        );
        for (scenario, peer, id) in [
            ("udp;whoami", "192.0.2.2", canary),
            ("udp", "8.8.8.8", canary),
            ("udp", "peer.test", canary),
            ("udp", "::1", "bad'command"),
        ] {
            assert!(validate(scenario, peer, id).is_err());
        }
    }
}
