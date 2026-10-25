use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::ContentHash;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CanonicalJsonError {
    #[error("canonical JSON serialization failed: {0}")]
    Serialization(String),
    #[error("canonical JSON v1 forbids floating-point numbers")]
    FloatingPoint,
}

/// Serializes a value using Aura Canonical JSON v1.
///
/// The output is UTF-8 without a BOM or insignificant whitespace. Object keys
/// are ordered lexicographically by Unicode scalar value. Callers remain
/// responsible for declaring optional contract fields without
/// `skip_serializing_if` when an explicit `null` is required.
pub fn canonical_json_v1<T>(value: &T) -> Result<Vec<u8>, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    let value = serde_json::to_value(value)
        .map_err(|error| CanonicalJsonError::Serialization(error.to_string()))?;
    let mut output = Vec::new();
    write_value(&value, &mut output)?;
    Ok(output)
}

pub fn canonical_json_v1_sha256<T>(value: &T) -> Result<ContentHash, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    canonical_json_v1(value).map(|bytes| ContentHash::sha256(&bytes))
}

fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => {
            if number.is_f64() {
                return Err(CanonicalJsonError::FloatingPoint);
            }
            output.extend_from_slice(number.to_string().as_bytes());
        }
        Value::String(value) => write_string(value, output)?,
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
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                write_string(key, output)?;
                output.push(b':');
                write_value(value, output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

fn write_string(value: &str, output: &mut Vec<u8>) -> Result<(), CanonicalJsonError> {
    let encoded = serde_json::to_string(value)
        .map_err(|error| CanonicalJsonError::Serialization(error.to_string()))?;
    output.extend_from_slice(encoded.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::{CanonicalJsonError, canonical_json_v1, canonical_json_v1_sha256};

    #[derive(Serialize)]
    struct Fixture<'a> {
        zeta: u64,
        alpha: &'a str,
        optional: Option<&'a str>,
        nested: serde_json::Value,
    }

    #[test]
    fn canonical_json_sorts_keys_and_preserves_explicit_null() {
        let fixture = Fixture {
            zeta: 7,
            alpha: "Aura\n雪",
            optional: None,
            nested: serde_json::json!({"z": 2, "a": [true, null]}),
        };
        assert_eq!(
            canonical_json_v1(&fixture).expect("canonical fixture"),
            r#"{"alpha":"Aura\n雪","nested":{"a":[true,null],"z":2},"optional":null,"zeta":7}"#
                .as_bytes()
        );
    }

    #[test]
    fn canonical_json_rejects_floats_recursively() {
        let error = canonical_json_v1(&serde_json::json!({"nested": [1.5]}))
            .expect_err("float must fail closed");
        assert_eq!(error, CanonicalJsonError::FloatingPoint);
    }

    #[test]
    fn canonical_hash_uses_exact_canonical_bytes() {
        let value = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(
            canonical_json_v1_sha256(&value).expect("hash"),
            aura_domain_common_test_hash(br#"{"a":1,"b":2}"#)
        );
    }

    fn aura_domain_common_test_hash(bytes: &[u8]) -> crate::ContentHash {
        crate::ContentHash::sha256(bytes)
    }
}
