use super::SecretScanner;
use crate::{GatewayRequest, NetworkControlError};

impl SecretScanner {
    /// Uses the actual wire headers and body, before trusted credential injection.
    /// Planning and Gateway execution call this same scanner; the general package
    /// path retains its existing separately classified body policy.
    pub fn scan_http_request(
        &self,
        request: &GatewayRequest,
        scan_body: bool,
    ) -> Result<(), NetworkControlError> {
        for (name, value) in &request.headers {
            self.scan_header(name, value)?;
        }
        if scan_body && let Ok(text) = std::str::from_utf8(&request.body) {
            self.scan_text("request.body", text)?;
        }
        Ok(())
    }
}
