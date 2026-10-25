//! Prototype-only sealed-control checks; not the eventual Runner wire contract.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptControl {
    pub operation: String,
    pub attempt: u32,
    pub nonce: String,
    pub executable_sha256: String,
    pub expires_unix_ms: u64,
    pub cancellation_revision: u64,
}
#[derive(Default)]
pub struct Claims(BTreeSet<String>);
impl Claims {
    pub fn claim(
        &mut self,
        control: &AttemptControl,
        now: u64,
        hash: &str,
        cancellation: u64,
    ) -> Result<()> {
        ensure!(
            control.operation == "r06.fixed-probe" && control.attempt > 0,
            "unrecognized operation"
        );
        ensure!(
            control.nonce.len() == 64
                && control.nonce.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid nonce"
        );
        ensure!(
            now < control.expires_unix_ms && control.expires_unix_ms - now <= 30_000,
            "expired control"
        );
        ensure!(control.executable_sha256 == hash && hash.len() == 64, "image changed");
        ensure!(control.cancellation_revision == cancellation, "cancelled control");
        ensure!(self.0.insert(control.nonce.clone()), "replayed control");
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn control() -> AttemptControl {
        AttemptControl {
            operation: "r06.fixed-probe".into(),
            attempt: 1,
            nonce: "ab".repeat(32),
            executable_sha256: "cd".repeat(32),
            expires_unix_ms: 101,
            cancellation_revision: 1,
        }
    }
    #[test]
    fn stale_changed_cancelled_and_replayed_controls_do_not_claim() {
        let v = control();
        let mut claims = Claims::default();
        assert!(claims.claim(&v, 101, &v.executable_sha256, 1).is_err());
        assert!(claims.claim(&v, 100, &"ef".repeat(32), 1).is_err());
        assert!(claims.claim(&v, 100, &v.executable_sha256, 2).is_err());
        assert!(claims.claim(&v, 100, &v.executable_sha256, 1).is_ok());
        assert!(claims.claim(&v, 100, &v.executable_sha256, 1).is_err());
    }
    #[test]
    fn arbitrary_commands_and_fields_are_not_a_runner_protocol() {
        let mut v = control();
        v.operation = "exec".into();
        assert!(Claims::default().claim(&v, 100, &v.executable_sha256, 1).is_err());
        let mut json = serde_json::to_value(control()).expect("control");
        json["command"] = serde_json::json!("cmd.exe");
        assert!(serde_json::from_value::<AttemptControl>(json).is_err());
    }
    #[test]
    fn nonce_encoding_and_far_future_controls_are_rejected() {
        let mut v = control();
        v.nonce = v.nonce.to_uppercase();
        assert!(Claims::default().claim(&v, 100, &v.executable_sha256, 1).is_err());
        v.nonce = "ab".repeat(32);
        v.expires_unix_ms = 30_101;
        assert!(Claims::default().claim(&v, 100, &v.executable_sha256, 1).is_err());
    }
}
