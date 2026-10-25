//! Fixed D1 client using the same write-restricted token construction as D0.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Duration};

pub fn run(root: &Path, image: &Path, digest: &str, run: &str, canary: &str) -> Result<Value> {
    ensure!(image.file_name().is_some_and(|n| n == "fixture.exe"), "fixed fixture filename");
    ensure!(digest.len() == 64 && digest.bytes().all(|c| c.is_ascii_hexdigit()), "fixture digest");
    ensure!(
        run.len() == 32 && run.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "fixture run"
    );
    ensure!(matches!(canary, "111" | "121" | "131"), "fixed Offline phase Canary");
    let bytes = fs::read(image)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(digest),
        "reviewed fixture image changed"
    );
    let (owner, observation) =
        crate::egress_probe::with_attempt(root, |attempt, sid, id, runtime| {
            let copy = attempt.join("allowed/fixture.exe");
            fs::write(&copy, &bytes)?;
            let mut child = crate::monitor::launch(
                sid,
                &copy,
                &["call-json".into(), run.into(), canary.into()],
                attempt,
                Some(id),
                Some(runtime),
            )?;
            crate::monitor::collect_until(&mut child, false, 65_536, Duration::from_secs(8))
        })?;
    Ok(
        json!({"schema":"r06-d1-write-restricted-client-v1","owner":owner,"acl_restored":true,"image_sha256":digest,"observation":observation,"d1_complete":false}),
    )
}
