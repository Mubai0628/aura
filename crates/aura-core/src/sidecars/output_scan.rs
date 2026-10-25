use super::*;

pub(super) struct CredentialCanaryOutputScannerV1 {
    pub(super) canary: Option<Zeroizing<Vec<u8>>>,
    pub(super) stdout: Zeroizing<Vec<u8>>,
    pub(super) stderr: Zeroizing<Vec<u8>>,
    pub(super) detected: bool,
}

impl CredentialCanaryOutputScannerV1 {
    pub(super) fn new(canary: Option<&[u8]>) -> Self {
        Self {
            canary: canary.map(|value| Zeroizing::new(value.to_vec())),
            stdout: Zeroizing::new(Vec::new()),
            stderr: Zeroizing::new(Vec::new()),
            detected: false,
        }
    }

    #[must_use]
    pub(super) fn canary(&self) -> Option<&[u8]> {
        self.canary.as_ref().map(|value| value.as_slice())
    }

    #[must_use]
    pub(super) const fn detected(&self) -> bool {
        self.detected
    }

    pub(super) fn observe(&mut self, event: &SandboxHostEvent) -> bool {
        if self.detected {
            return true;
        }
        let Some(canary) = self.canary.as_ref() else {
            return false;
        };
        let stream = match event {
            SandboxHostEvent::StdoutChunk { bytes, .. } => {
                self.stdout.extend_from_slice(bytes);
                self.stdout.as_slice()
            }
            SandboxHostEvent::StderrChunk { bytes, .. } => {
                self.stderr.extend_from_slice(bytes);
                self.stderr.as_slice()
            }
            _ => return false,
        };
        self.detected = contains_bytes(stream, canary.as_slice());
        self.detected
    }
}

pub(super) fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
