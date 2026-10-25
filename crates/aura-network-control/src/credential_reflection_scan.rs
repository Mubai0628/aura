use std::collections::BTreeSet;

use aura_domain_common::{CredentialId, CredentialRevision};
use aura_permission_domain::{
    CredentialReflectionEncodingSetV1, CredentialReflectionEncodingV1,
    CredentialReflectionScanEvidenceV1, CredentialReflectionScanStatusV1,
    CredentialReflectionSurfaceSetV1, CredentialReflectionSurfaceV1, NormalizedOrigin,
};
use base64::{Engine as _, engine::general_purpose};
use reqwest::header::HeaderMap;
use zeroize::Zeroize;

use crate::NetworkControlError;

/// Gateway-private scanner state. It is deliberately neither serializable nor
/// clonable because every contained byte pattern is derived from a live
/// credential value and must remain inside the P04 Gateway.
pub(crate) struct CredentialReflectionScannerV1 {
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
    origin: NormalizedOrigin,
    injected_value: Vec<u8>,
    variants: Vec<(CredentialReflectionEncodingV1, Vec<u8>)>,
    surfaces_scanned: BTreeSet<CredentialReflectionSurfaceV1>,
    scanned_header_count: u32,
    scanned_body_bytes: u64,
}

impl CredentialReflectionScannerV1 {
    pub(crate) fn new(
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        origin: NormalizedOrigin,
        injected_value: &[u8],
    ) -> Result<Self, NetworkControlError> {
        let mut variants = encodings(injected_value)?;
        // A bearer header's token alone is the secret too: a server that echoes only the token
        // reflects the credential as much as one that echoes the whole header value.
        if let Some(token) = bearer_token(injected_value) {
            variants.extend(encodings(token)?);
        }
        variants.sort_by_key(|(encoding, _)| *encoding);

        Ok(Self {
            credential_id,
            credential_revision,
            origin,
            injected_value: injected_value.to_vec(),
            variants,
            surfaces_scanned: BTreeSet::new(),
            scanned_header_count: 0,
            scanned_body_bytes: 0,
        })
    }

    pub(crate) fn injected_value(&self) -> &[u8] {
        &self.injected_value
    }

    pub(crate) fn matches_binding(
        &self,
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        origin: &NormalizedOrigin,
    ) -> bool {
        self.credential_id == credential_id
            && self.credential_revision == credential_revision
            && self.origin == *origin
    }

    pub(crate) fn scan_headers(
        &mut self,
        headers: &HeaderMap,
        surface: CredentialReflectionSurfaceV1,
    ) -> Result<(), NetworkControlError> {
        self.surfaces_scanned.insert(surface);
        for value in headers.values() {
            self.scanned_header_count =
                self.scanned_header_count.checked_add(1).ok_or_else(|| self.incomplete_error())?;
            if let Some(encoding) = self.detect(value.as_bytes()) {
                return Err(self.detected_error(surface, encoding));
            }
        }
        Ok(())
    }

    pub(crate) fn scan_body(
        &mut self,
        body: &[u8],
        surface: CredentialReflectionSurfaceV1,
    ) -> Result<(), NetworkControlError> {
        self.surfaces_scanned.insert(surface);
        self.scanned_body_bytes = self
            .scanned_body_bytes
            .checked_add(u64::try_from(body.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| self.incomplete_error())?;
        if let Some(encoding) = self.detect(body) {
            return Err(self.detected_error(surface, encoding));
        }
        Ok(())
    }

    /// Scans one chunk of a body read as it arrives. `tail` holds the end of the earlier
    /// chunks, so a reflected value split across two chunks is still found; it is replaced by
    /// the end of this window, never longer than the longest pattern.
    pub(crate) fn scan_body_chunk(
        &mut self,
        tail: &mut Vec<u8>,
        chunk: &[u8],
        surface: CredentialReflectionSurfaceV1,
    ) -> Result<(), NetworkControlError> {
        self.surfaces_scanned.insert(surface);
        self.scanned_body_bytes = self
            .scanned_body_bytes
            .checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| self.incomplete_error())?;
        tail.extend_from_slice(chunk);
        let detected = self.detect(tail);
        let keep = self.variants.iter().map(|(_, pattern)| pattern.len()).max().unwrap_or(1) - 1;
        let drop = tail.len().saturating_sub(keep);
        tail[..drop].zeroize();
        tail.drain(..drop);
        if let Some(encoding) = detected {
            return Err(self.detected_error(surface, encoding));
        }
        Ok(())
    }

    pub(crate) fn passed_evidence(
        &self,
    ) -> Result<CredentialReflectionScanEvidenceV1, NetworkControlError> {
        self.evidence(CredentialReflectionScanStatusV1::Passed, None, None)
    }

    pub(crate) fn incomplete_error(&self) -> NetworkControlError {
        let evidence =
            self.evidence(CredentialReflectionScanStatusV1::Incomplete, None, None).unwrap_or_else(
                |_| fallback_incomplete_evidence(self.credential_id, self.credential_revision),
            );
        NetworkControlError::CredentialReflectionScanIncomplete {
            evidence: Box::new(evidence),
            response_terminal_known: false,
        }
    }

    fn detected_error(
        &self,
        surface: CredentialReflectionSurfaceV1,
        encoding: CredentialReflectionEncodingV1,
    ) -> NetworkControlError {
        let evidence = self
            .evidence(CredentialReflectionScanStatusV1::Detected, Some(surface), Some(encoding))
            .unwrap_or_else(|_| {
                fallback_incomplete_evidence(self.credential_id, self.credential_revision)
            });
        if evidence.status == CredentialReflectionScanStatusV1::Detected {
            NetworkControlError::CredentialReflectionDetected { evidence: Box::new(evidence) }
        } else {
            NetworkControlError::CredentialReflectionScanIncomplete {
                evidence: Box::new(evidence),
                response_terminal_known: false,
            }
        }
    }

    fn detect(&self, value: &[u8]) -> Option<CredentialReflectionEncodingV1> {
        self.variants.iter().find_map(|(encoding, pattern)| {
            (!pattern.is_empty() && contains_bytes(value, pattern)).then_some(*encoding)
        })
    }

    fn evidence(
        &self,
        status: CredentialReflectionScanStatusV1,
        detected_surface: Option<CredentialReflectionSurfaceV1>,
        detected_encoding: Option<CredentialReflectionEncodingV1>,
    ) -> Result<CredentialReflectionScanEvidenceV1, NetworkControlError> {
        CredentialReflectionScanEvidenceV1::new(
            status,
            self.credential_id,
            self.credential_revision,
            CredentialReflectionSurfaceSetV1 {
                values: self.surfaces_scanned.iter().copied().collect(),
            },
            CredentialReflectionEncodingSetV1 {
                values: self
                    .variants
                    .iter()
                    .map(|(encoding, _)| *encoding)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            },
            self.scanned_header_count,
            self.scanned_body_bytes,
            detected_surface,
            detected_encoding,
        )
        .map_err(|_| NetworkControlError::CredentialBindingMismatch)
    }
}

impl Drop for CredentialReflectionScannerV1 {
    fn drop(&mut self) {
        self.injected_value.zeroize();
        for (_, value) in &mut self.variants {
            value.zeroize();
        }
    }
}

fn fallback_incomplete_evidence(
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
) -> CredentialReflectionScanEvidenceV1 {
    CredentialReflectionScanEvidenceV1::new(
        CredentialReflectionScanStatusV1::Incomplete,
        credential_id,
        credential_revision,
        CredentialReflectionSurfaceSetV1 { values: Vec::new() },
        CredentialReflectionEncodingSetV1 { values: Vec::new() },
        0,
        0,
        None,
        None,
    )
    .expect("empty incomplete evidence is structurally valid")
}

/// The token after `Bearer ` (any case), when there is one.
fn bearer_token(value: &[u8]) -> Option<&[u8]> {
    let prefix = value.get(..7)?;
    let token = value.get(7..)?;
    (prefix.eq_ignore_ascii_case(b"bearer ") && !token.is_empty()).then_some(token)
}

/// The eight encodings of one value the scanner looks for.
fn encodings(
    value: &[u8],
) -> Result<Vec<(CredentialReflectionEncodingV1, Vec<u8>)>, NetworkControlError> {
    let text =
        std::str::from_utf8(value).map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
    if text.is_empty() {
        return Err(NetworkControlError::CredentialBindingMismatch);
    }
    let json =
        serde_json::to_string(text).map_err(|_| NetworkControlError::CredentialBindingMismatch)?;
    let json_escaped = json
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(NetworkControlError::CredentialBindingMismatch)?
        .as_bytes()
        .to_vec();
    Ok(vec![
        (CredentialReflectionEncodingV1::RawUtf8, value.to_vec()),
        (CredentialReflectionEncodingV1::JsonStringEscaped, json_escaped),
        (CredentialReflectionEncodingV1::PercentEncodedUppercase, percent_encode(value, false)),
        (CredentialReflectionEncodingV1::PercentEncodedLowercase, percent_encode(value, true)),
        (
            CredentialReflectionEncodingV1::Base64StandardPadded,
            general_purpose::STANDARD.encode(value).into_bytes(),
        ),
        (
            CredentialReflectionEncodingV1::Base64StandardUnpadded,
            general_purpose::STANDARD_NO_PAD.encode(value).into_bytes(),
        ),
        (
            CredentialReflectionEncodingV1::Base64UrlSafePadded,
            general_purpose::URL_SAFE.encode(value).into_bytes(),
        ),
        (
            CredentialReflectionEncodingV1::Base64UrlSafeUnpadded,
            general_purpose::URL_SAFE_NO_PAD.encode(value).into_bytes(),
        ),
    ])
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn percent_encode(value: &[u8], lowercase: bool) -> Vec<u8> {
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    let alphabet = if lowercase { LOWER } else { UPPER };
    let mut encoded = Vec::with_capacity(value.len().saturating_mul(3));
    for byte in value {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte);
        } else {
            encoded.push(b'%');
            encoded.push(alphabet[usize::from(byte >> 4)]);
            encoded.push(alphabet[usize::from(byte & 0x0f)]);
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CredentialId, CredentialRevision};
    use aura_permission_domain::{
        CredentialReflectionEncodingV1, CredentialReflectionScanStatusV1,
        CredentialReflectionSurfaceV1,
    };
    use reqwest::header::{HeaderMap, HeaderValue};

    use super::CredentialReflectionScannerV1;
    use crate::NetworkControlError;

    #[test]
    fn scanner_detects_frozen_encodings_without_exposing_secret() {
        let cases = [
            ("a b", b"a b".as_slice(), CredentialReflectionEncodingV1::RawUtf8),
            ("a\"b", br#"a\"b"#.as_slice(), CredentialReflectionEncodingV1::JsonStringEscaped),
            ("a b", b"a%20b".as_slice(), CredentialReflectionEncodingV1::PercentEncodedUppercase),
            ("a/b", b"a%2fb".as_slice(), CredentialReflectionEncodingV1::PercentEncodedLowercase),
            (
                "token!",
                b"dG9rZW4h".as_slice(),
                CredentialReflectionEncodingV1::Base64StandardPadded,
            ),
        ];
        for (secret, reflected, expected) in cases {
            let mut scanner = CredentialReflectionScannerV1::new(
                CredentialId::new(),
                CredentialRevision::INITIAL,
                aura_permission_domain::NormalizedOrigin {
                    scheme: "https".to_owned(),
                    host: "example.test".to_owned(),
                    port: 443,
                },
                secret.as_bytes(),
            )
            .expect("scanner");
            let error = scanner
                .scan_body(reflected, CredentialReflectionSurfaceV1::FinalResponseBody)
                .expect_err("reflection detected");
            let NetworkControlError::CredentialReflectionDetected { evidence } = error else {
                panic!("unexpected scan result");
            };
            assert_eq!(evidence.status, CredentialReflectionScanStatusV1::Detected);
            assert_eq!(evidence.detected_encoding, Some(expected));
            assert!(!serde_json::to_string(&evidence).expect("evidence").contains(secret));
        }
    }

    #[test]
    fn scanner_counts_headers_and_detects_collected_cross_chunk_body() {
        let mut scanner = CredentialReflectionScannerV1::new(
            CredentialId::new(),
            CredentialRevision::INITIAL,
            aura_permission_domain::NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "example.test".to_owned(),
                port: 443,
            },
            b"cross-boundary",
        )
        .expect("scanner");
        let mut headers = HeaderMap::new();
        headers.insert("x-safe", HeaderValue::from_static("safe"));
        scanner
            .scan_headers(&headers, CredentialReflectionSurfaceV1::FinalHeaderValue)
            .expect("safe headers");
        let body = [b"cross-".as_slice(), b"boundary".as_slice()].concat();
        assert!(matches!(
            scanner.scan_body(&body, CredentialReflectionSurfaceV1::FinalResponseBody),
            Err(NetworkControlError::CredentialReflectionDetected { .. })
        ));
    }
}
