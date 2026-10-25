use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentHashAlgorithm {
    Blake3,
    Sha256,
}

impl fmt::Display for ContentHashAlgorithm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blake3 => formatter.write_str("blake3"),
            Self::Sha256 => formatter.write_str("sha256"),
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("content hash must contain exactly 64 lowercase hexadecimal characters")]
pub struct ContentHashError;

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentHash {
    pub algorithm: ContentHashAlgorithm,
    pub hex: String,
}

impl ContentHash {
    pub fn new(
        algorithm: ContentHashAlgorithm,
        hex: impl Into<String>,
    ) -> Result<Self, ContentHashError> {
        let hex = hex.into();
        if hex.len() != 64
            || !hex.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(ContentHashError);
        }

        Ok(Self { algorithm, hex })
    }

    #[must_use]
    pub fn blake3(bytes: &[u8]) -> Self {
        Self {
            algorithm: ContentHashAlgorithm::Blake3,
            hex: blake3::hash(bytes).to_hex().to_string(),
        }
    }

    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self {
            algorithm: ContentHashAlgorithm::Sha256,
            hex: format!("{:x}", Sha256::digest(bytes)),
        }
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.algorithm, self.hex)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireContentHash {
            algorithm: ContentHashAlgorithm,
            hex: String,
        }

        let wire = WireContentHash::deserialize(deserializer)?;
        Self::new(wire.algorithm, wire.hex).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::ContentHash;

    #[test]
    fn blake3_known_vectors() {
        assert_eq!(
            ContentHash::blake3(b"").hex,
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert_eq!(
            ContentHash::blake3(b"abc").hex,
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        assert_eq!(
            ContentHash::blake3(&[7_u8; 1025]).hex,
            "a437a561f23dd6338b07f72b060b6c46f1995cfffd53ad6d8df4f46249091937"
        );
    }

    #[test]
    fn blake3_block_and_chunk_boundaries() {
        let cases = [
            (64_usize, 0_u8, "4d006976636a8696d909a630a4081aad4d7c50f81afdee04020bf05086ab6a55"),
            (1024, 0, "d6fd9de5bccf223f523b316c9cd1cf9a9d87ea42473d68e011dad13f09bf8917"),
            (328, 0, "06d2131c9ba44b36c4b6c339e056de0d5e7eeeda82dd846b65e3d52825ba3405"),
            (1046, 0, "92e5b306af1b7774346359fb07d16576c1f26ac78840e11ecb393dbd3f9c230e"),
            (2000, 5, "68b394780ea56ccaef52a6a3c441fe937ba2b3603dd5f9e4ad378a0d94d4577c"),
            (3072, 2, "e9b48260ab50cbedf94c5e7de2d5da42fbe1631f87937cccf46545ed89f397f0"),
            (5120, 1, "89cf722e5a3721b7a15b19be17da6011a8a2f175fb89ad3a57ef3391febd251b"),
            (1500, 9, "ebff2731ccd2c0e9a1412c7d5c056bdd07d3e6511048d9313dcd5ab74bce5312"),
        ];
        for (len, fill, expected) in cases {
            assert_eq!(
                ContentHash::blake3(&vec![fill; len]).hex,
                expected,
                "len={len} fill={fill}"
            );
        }
    }
}
