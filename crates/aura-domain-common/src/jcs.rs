use std::{borrow::Cow, collections::BTreeMap, fmt};

use crate::ContentHashAlgorithm;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalizationVersion {
    Rfc8785JcsV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSha256HashV1 {
    algorithm: ContentHashAlgorithm,
    canonicalization_version: CanonicalizationVersion,
    hex: String,
}

impl CanonicalSha256HashV1 {
    pub fn new(hex: impl Into<String>) -> Result<Self, CanonicalJsonError> {
        let hex = hex.into();
        validate_digest(&hex)?;
        Ok(Self {
            algorithm: ContentHashAlgorithm::Sha256,
            canonicalization_version: CanonicalizationVersion::Rfc8785JcsV1,
            hex,
        })
    }

    #[must_use]
    pub fn sha256(canonical_bytes: &[u8]) -> Self {
        Self {
            algorithm: ContentHashAlgorithm::Sha256,
            canonicalization_version: CanonicalizationVersion::Rfc8785JcsV1,
            hex: format!("{:x}", Sha256::digest(canonical_bytes)),
        }
    }

    #[must_use]
    pub const fn algorithm(&self) -> ContentHashAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub const fn canonicalization_version(&self) -> CanonicalizationVersion {
        self.canonicalization_version
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl<'de> Deserialize<'de> for CanonicalSha256HashV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireHash {
            algorithm: ContentHashAlgorithm,
            canonicalization_version: CanonicalizationVersion,
            hex: String,
        }

        let wire = WireHash::deserialize(deserializer)?;
        if wire.algorithm != ContentHashAlgorithm::Sha256
            || wire.canonicalization_version != CanonicalizationVersion::Rfc8785JcsV1
        {
            return Err(D::Error::custom("canonical SHA-256 hash metadata is invalid"));
        }
        Self::new(wire.hex).map_err(D::Error::custom)
    }
}

impl JsonSchema for CanonicalSha256HashV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CanonicalSha256HashV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "algorithm": { "type": "string", "const": "sha256" },
                "canonicalization_version": {
                    "type": "string",
                    "const": "rfc8785_jcs_v1"
                },
                "hex": {
                    "type": "string",
                    "minLength": 64,
                    "maxLength": 64,
                    "pattern": "^[0-9a-f]{64}$"
                }
            },
            "required": ["algorithm", "canonicalization_version", "hex"]
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalBlake3HashV1 {
    algorithm: ContentHashAlgorithm,
    canonicalization_version: CanonicalizationVersion,
    hex: String,
}

impl CanonicalBlake3HashV1 {
    pub fn new(hex: impl Into<String>) -> Result<Self, CanonicalJsonError> {
        let hex = hex.into();
        validate_digest(&hex)?;
        Ok(Self {
            algorithm: ContentHashAlgorithm::Blake3,
            canonicalization_version: CanonicalizationVersion::Rfc8785JcsV1,
            hex,
        })
    }

    #[must_use]
    pub fn blake3(canonical_bytes: &[u8]) -> Self {
        Self {
            algorithm: ContentHashAlgorithm::Blake3,
            canonicalization_version: CanonicalizationVersion::Rfc8785JcsV1,
            hex: blake3::hash(canonical_bytes).to_hex().to_string(),
        }
    }

    #[must_use]
    pub const fn algorithm(&self) -> ContentHashAlgorithm {
        self.algorithm
    }

    #[must_use]
    pub const fn canonicalization_version(&self) -> CanonicalizationVersion {
        self.canonicalization_version
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        &self.hex
    }
}

impl<'de> Deserialize<'de> for CanonicalBlake3HashV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireHash {
            algorithm: ContentHashAlgorithm,
            canonicalization_version: CanonicalizationVersion,
            hex: String,
        }

        let wire = WireHash::deserialize(deserializer)?;
        if wire.algorithm != ContentHashAlgorithm::Blake3
            || wire.canonicalization_version != CanonicalizationVersion::Rfc8785JcsV1
        {
            return Err(D::Error::custom("canonical BLAKE3 hash metadata is invalid"));
        }
        Self::new(wire.hex).map_err(D::Error::custom)
    }
}

impl JsonSchema for CanonicalBlake3HashV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("CanonicalBlake3HashV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "algorithm": { "type": "string", "const": "blake3" },
                "canonicalization_version": {
                    "type": "string",
                    "const": "rfc8785_jcs_v1"
                },
                "hex": {
                    "type": "string",
                    "minLength": 64,
                    "maxLength": 64,
                    "pattern": "^[0-9a-f]{64}$"
                }
            },
            "required": ["algorithm", "canonicalization_version", "hex"]
        })
    }
}

/// Existing Rust consumers use the historical source name, but its wire is
/// now strictly the SHA-256/JCS V1 contract. Schemars exposes only the
/// authoritative `CanonicalSha256HashV1` definition.
pub type CanonicalHashV1 = CanonicalSha256HashV1;

fn validate_digest(hex: &str) -> Result<(), CanonicalJsonError> {
    if hex.len() != 64
        || !hex.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(CanonicalJsonError::InvalidDigest);
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CanonicalJsonError {
    #[error("canonical JSON input is not valid UTF-8 JSON")]
    InvalidJson,
    #[error("canonical JSON input contains duplicate object key '{key}'")]
    DuplicateObjectKey { key: String },
    #[error("canonical JSON only accepts finite binary64 numbers")]
    NonFiniteNumber,
    #[error("canonical JSON integer is outside the safe range")]
    UnsafeInteger,
    #[error("canonical JSON serialization failed")]
    Serialization,
    #[error("canonical hash digest must contain 64 lowercase hexadecimal characters")]
    InvalidDigest,
}

/// Parses JSON without losing duplicate-key evidence.
pub fn parse_json_strict(bytes: &[u8]) -> Result<Value, CanonicalJsonError> {
    parse_json_strict_located(bytes).map_err(|failure| failure.error)
}

/// The same strict gate with authored source coordinates for import diagnostics.
/// Existing callers retain their exact error enum via [`parse_json_strict`].
#[derive(Debug, Error)]
#[error("{error}")]
pub struct LocatedCanonicalJsonError {
    #[source]
    pub error: CanonicalJsonError,
    pub line: usize,
    pub column: usize,
}

pub fn parse_json_strict_located(bytes: &[u8]) -> Result<Value, LocatedCanonicalJsonError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValueSeed.deserialize(&mut deserializer).map_err(|error| {
        let (line, column) = (error.line(), error.column());
        LocatedCanonicalJsonError { error: map_deserialize_error(error), line, column }
    })?;
    deserializer.end().map_err(|error| LocatedCanonicalJsonError {
        error: CanonicalJsonError::InvalidJson,
        line: error.line(),
        column: error.column(),
    })?;
    Ok(value)
}

pub fn canonical_json_jcs<T>(value: &T) -> Result<Vec<u8>, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(value).map_err(|_| CanonicalJsonError::Serialization)?;
    canonicalize_json_value(&value)
}

pub fn canonicalize_json_value(value: &Value) -> Result<Vec<u8>, CanonicalJsonError> {
    let mut output = Vec::new();
    write_value(value, &mut output)?;
    Ok(output)
}

pub fn canonical_sha256<T>(value: &T) -> Result<CanonicalSha256HashV1, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    canonical_json_jcs(value).map(|bytes| CanonicalSha256HashV1::sha256(&bytes))
}

pub fn canonical_blake3<T>(value: &T) -> Result<CanonicalBlake3HashV1, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    canonical_json_jcs(value).map(|bytes| CanonicalBlake3HashV1::blake3(&bytes))
}

fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => write_number(number, output)?,
        Value::String(value) => write_string(value, output),
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_value(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| {
                left.encode_utf16().cmp(right.encode_utf16())
            });
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_string(key, output);
                output.push(b':');
                write_value(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn write_number(number: &Number, output: &mut Vec<u8>) -> Result<(), CanonicalJsonError> {
    if number.is_i64() {
        let value = number.as_i64().ok_or(CanonicalJsonError::Serialization)?;
        if value.unsigned_abs() > MAX_SAFE_JSON_INTEGER {
            return Err(CanonicalJsonError::UnsafeInteger);
        }
        output.extend_from_slice(value.to_string().as_bytes());
        return Ok(());
    }
    if number.is_u64() {
        let value = number.as_u64().ok_or(CanonicalJsonError::Serialization)?;
        if value > MAX_SAFE_JSON_INTEGER {
            return Err(CanonicalJsonError::UnsafeInteger);
        }
        output.extend_from_slice(value.to_string().as_bytes());
        return Ok(());
    }

    let value = number.as_f64().ok_or(CanonicalJsonError::Serialization)?;
    if !value.is_finite() {
        return Err(CanonicalJsonError::NonFiniteNumber);
    }
    if value == 0.0 {
        output.push(b'0');
        return Ok(());
    }
    let mut buffer = ryu_js::Buffer::new();
    output.extend_from_slice(buffer.format(value).as_bytes());
    Ok(())
}

fn write_string(value: &str, output: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(br#"\""#),
            '\\' => output.extend_from_slice(br#"\\"#),
            '\u{0008}' => output.extend_from_slice(br"\b"),
            '\t' => output.extend_from_slice(br"\t"),
            '\n' => output.extend_from_slice(br"\n"),
            '\u{000c}' => output.extend_from_slice(br"\f"),
            '\r' => output.extend_from_slice(br"\r"),
            character if character <= '\u{001f}' => {
                let value = character as u8;
                output.extend_from_slice(br"\u00");
                output.push(HEX[usize::from(value >> 4)]);
                output.push(HEX[usize::from(value & 0x0f)]);
            }
            character => {
                let mut buffer = [0_u8; 4];
                output.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    output.push(b'"');
}

struct StrictValueSeed;

impl<'de> DeserializeSeed<'de> for StrictValueSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value.unsigned_abs() > MAX_SAFE_JSON_INTEGER {
            return Err(E::custom("unsafe_integer"));
        }
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if value > MAX_SAFE_JSON_INTEGER {
            return Err(E::custom("unsafe_integer"));
        }
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value).map(Value::Number).ok_or_else(|| E::custom("non_finite_number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValueSeed)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(StrictValueSeed)?;
            if values.insert(key.clone(), value).is_some() {
                return Err(A::Error::custom(format!("duplicate_object_key:{key}")));
            }
        }
        Ok(Value::Object(values.into_iter().collect()))
    }
}

fn map_deserialize_error(error: serde_json::Error) -> CanonicalJsonError {
    let message = error.to_string();
    if let Some(remainder) = message.strip_prefix("duplicate_object_key:") {
        let key = remainder.split(" at line ").next().unwrap_or(remainder).to_owned();
        return CanonicalJsonError::DuplicateObjectKey { key };
    }
    if message.starts_with("unsafe_integer") {
        return CanonicalJsonError::UnsafeInteger;
    }
    if message.starts_with("non_finite_number") {
        return CanonicalJsonError::NonFiniteNumber;
    }
    CanonicalJsonError::InvalidJson
}

#[cfg(test)]
mod tests {
    use crate::ContentHashAlgorithm;
    use serde_json::json;

    use super::{
        CanonicalBlake3HashV1, CanonicalJsonError, CanonicalSha256HashV1, CanonicalizationVersion,
        canonical_blake3, canonical_json_jcs, canonical_sha256, parse_json_strict,
    };

    #[test]
    fn jcs_matches_rfc_number_and_utf16_order_vectors() {
        let value = json!({
            "numbers": [333_333_333.333_333_3, 1E30, 4.50, 2e-3, 0.000000000000000000000000001],
            "string": "€$\u{000f}\nA'B\"\\\"/"
        });
        assert_eq!(
            String::from_utf8(canonical_json_jcs(&value).expect("JCS")).expect("UTF-8"),
            r#"{"numbers":[333333333.3333333,1e+30,4.5,0.002,1e-27],"string":"€$\u000f\nA'B\"\\\"/"}"#
        );

        let ordered = json!({"\u{20ac}": 1, "\r": 2, "\u{fb33}": 3, "1": 4, "\u{1f600}": 5, "\u{0080}": 6, "ö": 7});
        assert_eq!(
            String::from_utf8(canonical_json_jcs(&ordered).expect("JCS")).expect("UTF-8"),
            "{\"\\r\":2,\"1\":4,\"\u{80}\":6,\"ö\":7,\"€\":1,\"😀\":5,\"דּ\":3}"
        );
    }

    #[test]
    fn strict_parser_rejects_duplicates_and_unsafe_integer_tokens() {
        assert!(matches!(
            parse_json_strict(br#"{"a":1,"a":2}"#),
            Err(CanonicalJsonError::DuplicateObjectKey { .. })
        ));
        assert_eq!(parse_json_strict(b"9007199254740992"), Err(CanonicalJsonError::UnsafeInteger));
    }

    #[test]
    fn p07_hash_identifies_algorithm_and_canonicalization() {
        let hash = canonical_sha256(&json!({"b": 2, "a": 1})).expect("hash");
        assert_eq!(hash.algorithm(), ContentHashAlgorithm::Sha256);
        assert_eq!(hash.canonicalization_version(), CanonicalizationVersion::Rfc8785JcsV1);
        assert_eq!(hash.hex().len(), 64);

        let security = canonical_blake3(&json!({"b": 2, "a": 1})).expect("security hash");
        assert_eq!(security.algorithm(), ContentHashAlgorithm::Blake3);
        assert_eq!(security.canonicalization_version(), CanonicalizationVersion::Rfc8785JcsV1);

        assert!(
            serde_json::from_value::<CanonicalSha256HashV1>(json!({
                "algorithm": "blake3",
                "canonicalization_version": "rfc8785_jcs_v1",
                "hex": "00".repeat(32)
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<CanonicalBlake3HashV1>(json!({
                "algorithm": "blake3",
                "canonicalization_version": "rfc8785_jcs_v1",
                "hex": "AA".repeat(32)
            }))
            .is_err()
        );
    }
}
