use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use zeroize::Zeroize;

#[derive(Clone, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SecretNonce(#[schemars(with = "String")] String);

impl SecretNonce {
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        let mut value = String::with_capacity(64);
        for byte in bytes {
            use fmt::Write as _;
            write!(&mut value, "{byte:02x}").expect("writing to String is infallible");
        }
        Self(value)
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretNonce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretNonce([REDACTED])")
    }
}

impl Drop for SecretNonce {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
