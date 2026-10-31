use std::{collections::BTreeSet, fmt, io, ptr};

use aura_domain_common::CredentialRef;
use thiserror::Error;
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, FILETIME},
    Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIAL_ATTRIBUTEW, CREDENTIALW,
        CredDeleteW, CredFree, CredReadW, CredWriteW,
    },
};
use zeroize::Zeroize;

pub const MAX_CREDENTIAL_BYTES: usize = 5 * 512;
pub const MAX_CREDENTIAL_ATTRIBUTES: usize = 64;
pub const MAX_CREDENTIAL_ATTRIBUTE_KEYWORD_UTF16_UNITS: usize = 256;
pub const MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES: usize = 256;

#[derive(Debug, Error)]
pub enum CredentialStoreError {
    #[error("credential values must not be empty")]
    EmptyValue,
    #[error("credential.value_too_large: Credential Manager accepts at most {maximum} bytes")]
    ValueTooLarge { maximum: usize },
    #[error("credential was not found: {reference}")]
    NotFound { reference: CredentialRef },
    #[error("Credential Manager returned an invalid credential buffer")]
    InvalidBuffer,
    #[error("credential application attribute keyword is empty or invalid")]
    InvalidAttributeKeyword,
    #[error("credential application attribute keyword exceeds {maximum} UTF-16 units: {actual}")]
    AttributeKeywordTooLong { actual: usize, maximum: usize },
    #[error("credential application attribute value exceeds {maximum} bytes: {actual}")]
    AttributeValueTooLarge { actual: usize, maximum: usize },
    #[error("credential application attribute keyword is duplicated: {keyword}")]
    DuplicateAttribute { keyword: String },
    #[error("credential application attribute count exceeds {maximum}: {actual}")]
    TooManyAttributes { actual: usize, maximum: usize },
    #[error("Windows Credential Manager operation failed: {0}")]
    Windows(#[source] io::Error),
}

/// One explicitly non-secret WinCred application attribute.
///
/// Windows stores text values as UTF-16 bytes without a trailing NUL. Callers
/// must never place credentials, hashes of credentials, or recovery copies in
/// this type.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CredentialApplicationAttribute {
    keyword: String,
    value: String,
}

impl CredentialApplicationAttribute {
    pub fn new(
        keyword: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, CredentialStoreError> {
        let attribute = Self { keyword: keyword.into(), value: value.into() };
        attribute.validate()?;
        Ok(attribute)
    }

    #[must_use]
    pub fn keyword(&self) -> &str {
        &self.keyword
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    fn validate(&self) -> Result<(), CredentialStoreError> {
        if self.keyword.is_empty() || self.keyword.contains('\0') || self.value.contains('\0') {
            return Err(CredentialStoreError::InvalidAttributeKeyword);
        }
        let keyword_units = self.keyword.encode_utf16().count();
        if keyword_units > MAX_CREDENTIAL_ATTRIBUTE_KEYWORD_UTF16_UNITS {
            return Err(CredentialStoreError::AttributeKeywordTooLong {
                actual: keyword_units,
                maximum: MAX_CREDENTIAL_ATTRIBUTE_KEYWORD_UTF16_UNITS,
            });
        }
        let value_bytes = self.value.encode_utf16().count().saturating_mul(2);
        if value_bytes > MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES {
            return Err(CredentialStoreError::AttributeValueTooLarge {
                actual: value_bytes,
                maximum: MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES,
            });
        }
        Ok(())
    }
}

pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    #[must_use]
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("REDACTED")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct StoredCredential {
    secret: SecretBytes,
    attributes: Vec<CredentialApplicationAttribute>,
}

impl StoredCredential {
    #[must_use]
    pub fn secret(&self) -> &SecretBytes {
        &self.secret
    }

    #[must_use]
    pub fn attributes(&self) -> &[CredentialApplicationAttribute] {
        &self.attributes
    }

    #[must_use]
    pub fn into_secret(self) -> SecretBytes {
        self.secret
    }
}

impl fmt::Debug for StoredCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoredCredential")
            .field("secret", &"<redacted>")
            .field("attributes", &self.attributes)
            .finish()
    }
}

/// The user's Windows Credential Manager. Every call waits for the user's Credential Manager
/// lock first (`credential_lock`), because overlapping calls lose updates.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    pub fn write(
        self,
        reference: &CredentialRef,
        secret: &SecretBytes,
    ) -> Result<(), CredentialStoreError> {
        self.write_with_attributes(reference, secret, &[])
    }

    pub fn write_with_attributes(
        self,
        reference: &CredentialRef,
        secret: &SecretBytes,
        attributes: &[CredentialApplicationAttribute],
    ) -> Result<(), CredentialStoreError> {
        let bytes = secret.expose();
        if bytes.is_empty() {
            return Err(CredentialStoreError::EmptyValue);
        }
        if bytes.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialStoreError::ValueTooLarge { maximum: MAX_CREDENTIAL_BYTES });
        }
        validate_attribute_set(attributes)?;

        let mut target = wide(&reference.windows_target_name());
        let mut attribute_keywords =
            attributes.iter().map(|attribute| wide(attribute.keyword())).collect::<Vec<_>>();
        let mut attribute_values = attributes
            .iter()
            .map(|attribute| attribute.value().encode_utf16().collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mut raw_attributes = attributes
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let value_bytes = attribute_values[index].len().saturating_mul(2);
                Ok(CREDENTIAL_ATTRIBUTEW {
                    Keyword: attribute_keywords[index].as_mut_ptr(),
                    Flags: 0,
                    ValueSize: u32::try_from(value_bytes).map_err(|_| {
                        CredentialStoreError::AttributeValueTooLarge {
                            actual: value_bytes,
                            maximum: MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES,
                        }
                    })?,
                    Value: attribute_values[index].as_mut_ptr().cast(),
                })
            })
            .collect::<Result<Vec<_>, CredentialStoreError>>()?;
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: u32::try_from(bytes.len()).map_err(|_| {
                CredentialStoreError::ValueTooLarge { maximum: MAX_CREDENTIAL_BYTES }
            })?,
            CredentialBlob: bytes.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: u32::try_from(raw_attributes.len()).map_err(|_| {
                CredentialStoreError::TooManyAttributes {
                    actual: raw_attributes.len(),
                    maximum: MAX_CREDENTIAL_ATTRIBUTES,
                }
            })?,
            Attributes: if raw_attributes.is_empty() {
                ptr::null_mut()
            } else {
                raw_attributes.as_mut_ptr()
            },
            TargetAlias: ptr::null_mut(),
            UserName: ptr::null_mut(),
        };

        let _lock = crate::credential_lock::hold().map_err(CredentialStoreError::Windows)?;
        // SAFETY: every pointer in the structure is either null or points to a
        // live buffer for the duration of the synchronous call.
        if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
            return Err(CredentialStoreError::Windows(io::Error::last_os_error()));
        }
        Ok(())
    }

    pub fn read(self, reference: &CredentialRef) -> Result<SecretBytes, CredentialStoreError> {
        self.read_record(reference).map(StoredCredential::into_secret)
    }

    pub fn read_record(
        self,
        reference: &CredentialRef,
    ) -> Result<StoredCredential, CredentialStoreError> {
        let target = wide(&reference.windows_target_name());
        let mut credential = ptr::null_mut::<CREDENTIALW>();
        let _lock = crate::credential_lock::hold().map_err(CredentialStoreError::Windows)?;
        // SAFETY: target is NUL-terminated and output points to valid storage.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut credential) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                return Err(CredentialStoreError::NotFound { reference: reference.clone() });
            }
            return Err(CredentialStoreError::Windows(error));
        }
        let guard = CredentialBuffer(credential);
        if guard.0.is_null() {
            return Err(CredentialStoreError::InvalidBuffer);
        }
        // SAFETY: CredReadW returned one valid CREDENTIALW allocation owned by guard.
        let raw = unsafe { &mut *guard.0 };
        let length = usize::try_from(raw.CredentialBlobSize)
            .map_err(|_| CredentialStoreError::InvalidBuffer)?;
        if length > MAX_CREDENTIAL_BYTES || (length > 0 && raw.CredentialBlob.is_null()) {
            return Err(CredentialStoreError::InvalidBuffer);
        }
        // SAFETY: Windows guarantees CredentialBlobSize bytes within the returned allocation.
        let value = unsafe { std::slice::from_raw_parts(raw.CredentialBlob, length) }.to_vec();
        securely_zero(raw.CredentialBlob, length);
        let secret = SecretBytes::new(value);
        let attributes = read_attribute_set(raw)?;
        Ok(StoredCredential { secret, attributes })
    }

    pub fn delete(self, reference: &CredentialRef) -> Result<bool, CredentialStoreError> {
        let target = wide(&reference.windows_target_name());
        let _lock = crate::credential_lock::hold().map_err(CredentialStoreError::Windows)?;
        // SAFETY: target is a live, NUL-terminated UTF-16 buffer.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
            return Ok(false);
        }
        Err(CredentialStoreError::Windows(error))
    }
}

fn validate_attribute_set(
    attributes: &[CredentialApplicationAttribute],
) -> Result<(), CredentialStoreError> {
    if attributes.len() > MAX_CREDENTIAL_ATTRIBUTES {
        return Err(CredentialStoreError::TooManyAttributes {
            actual: attributes.len(),
            maximum: MAX_CREDENTIAL_ATTRIBUTES,
        });
    }
    let mut keywords = BTreeSet::new();
    for attribute in attributes {
        attribute.validate()?;
        if !keywords.insert(attribute.keyword()) {
            return Err(CredentialStoreError::DuplicateAttribute {
                keyword: attribute.keyword().to_owned(),
            });
        }
    }
    Ok(())
}

fn read_attribute_set(
    credential: &CREDENTIALW,
) -> Result<Vec<CredentialApplicationAttribute>, CredentialStoreError> {
    let count = usize::try_from(credential.AttributeCount)
        .map_err(|_| CredentialStoreError::InvalidBuffer)?;
    if count > MAX_CREDENTIAL_ATTRIBUTES || (count > 0 && credential.Attributes.is_null()) {
        return Err(CredentialStoreError::InvalidBuffer);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: CredReadW returned AttributeCount contiguous records in its owned allocation.
    let raw_attributes = unsafe { std::slice::from_raw_parts(credential.Attributes, count) };
    let mut attributes = Vec::with_capacity(count);
    for raw in raw_attributes {
        if raw.Flags != 0 || raw.Keyword.is_null() {
            return Err(CredentialStoreError::InvalidBuffer);
        }
        let keyword = read_bounded_wide(raw.Keyword)?;
        let value_byte_length =
            usize::try_from(raw.ValueSize).map_err(|_| CredentialStoreError::InvalidBuffer)?;
        if value_byte_length > MAX_CREDENTIAL_ATTRIBUTE_VALUE_BYTES
            || value_byte_length % 2 != 0
            || (value_byte_length > 0 && raw.Value.is_null())
        {
            return Err(CredentialStoreError::InvalidBuffer);
        }
        let value_units = value_byte_length / 2;
        // SAFETY: Windows returned ValueSize readable bytes for this application attribute.
        let value = String::from_utf16(unsafe {
            std::slice::from_raw_parts(raw.Value.cast::<u16>(), value_units)
        })
        .map_err(|_| CredentialStoreError::InvalidBuffer)?;
        attributes.push(CredentialApplicationAttribute::new(keyword, value)?);
    }
    validate_attribute_set(&attributes)?;
    Ok(attributes)
}

fn read_bounded_wide(pointer: *const u16) -> Result<String, CredentialStoreError> {
    let mut length = 0;
    while length <= MAX_CREDENTIAL_ATTRIBUTE_KEYWORD_UTF16_UNITS {
        // SAFETY: WinCred owns a NUL-terminated Keyword string in the returned allocation.
        if unsafe { *pointer.add(length) } == 0 {
            // SAFETY: the bounded scan established that these UTF-16 units precede the NUL.
            return String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                .map_err(|_| CredentialStoreError::InvalidBuffer);
        }
        length += 1;
    }
    Err(CredentialStoreError::InvalidBuffer)
}

struct CredentialBuffer(*mut CREDENTIALW);

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer was allocated by CredReadW and is freed once.
            unsafe { CredFree(self.0.cast()) };
        }
    }
}

fn securely_zero(pointer: *mut u8, length: usize) {
    if pointer.is_null() {
        return;
    }
    for offset in 0..length {
        // SAFETY: the caller supplied a valid writable buffer of length bytes.
        unsafe { pointer.add(offset).write_volatile(0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use aura_domain_common::CredentialRef;

    use super::{
        CredentialApplicationAttribute, CredentialStoreError, SecretBytes, WindowsCredentialStore,
    };

    #[test]
    fn secret_debug_output_is_constant_and_contains_no_characteristics() {
        let short = SecretBytes::new(b"a".to_vec());
        let long = SecretBytes::new(b"different-secret-value".to_vec());

        assert_eq!(format!("{short:?}"), "REDACTED");
        assert_eq!(format!("{long:?}"), "REDACTED");
    }

    #[test]
    fn test_namespace_is_isolated_and_cleaned_up() {
        let run = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let reference =
            CredentialRef::for_test(&format!("{run:x}"), "roundtrip").expect("test reference");
        let store = WindowsCredentialStore;
        let _ = store.delete(&reference);
        let secret = SecretBytes::new(b"aura-p02-test-secret".to_vec());
        store.write(&reference, &secret).expect("write credential");
        let actual = store.read(&reference).expect("read credential");
        assert_eq!(actual.expose(), secret.expose());
        assert!(store.delete(&reference).expect("delete credential"));
        assert!(matches!(store.read(&reference), Err(CredentialStoreError::NotFound { .. })));
    }

    #[test]
    fn oversized_values_are_rejected_with_a_typed_error() {
        let reference =
            CredentialRef::for_test("oversize", "value").expect("test credential reference");
        let secret = SecretBytes::new(vec![1; 2561]);
        assert!(matches!(
            WindowsCredentialStore.write(&reference, &secret),
            Err(CredentialStoreError::ValueTooLarge { maximum: 2560 })
        ));
    }

    #[test]
    fn non_secret_application_attributes_round_trip_and_replace_atomically_per_target() {
        let run = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let reference =
            CredentialRef::for_test(&format!("{run:x}"), "attributes").expect("test reference");
        let store = WindowsCredentialStore;
        let _ = store.delete(&reference);
        let first_secret = SecretBytes::new(b"first-aura-secret".to_vec());
        let staged = [
            CredentialApplicationAttribute::new("Aura_CredentialId", "end_example")
                .expect("credential id attribute"),
            CredentialApplicationAttribute::new("Aura_CredentialRevision", "1")
                .expect("credential revision attribute"),
            CredentialApplicationAttribute::new("Aura_TransactionId", "jnl_example")
                .expect("transaction attribute"),
            CredentialApplicationAttribute::new("Aura_RecordState", "staged")
                .expect("record state attribute"),
            CredentialApplicationAttribute::new(
                "Aura_BundleNamespaceHash",
                "0123456789abcdef0123456789abcdef",
            )
            .expect("namespace attribute"),
        ];
        store
            .write_with_attributes(&reference, &first_secret, &staged)
            .expect("write staged credential");
        let stored = store.read_record(&reference).expect("read staged credential");
        assert_eq!(stored.secret().expose(), first_secret.expose());
        assert_eq!(stored.attributes(), staged);

        let second_secret = SecretBytes::new(b"replacement-aura-secret".to_vec());
        let mut committed = staged;
        committed[3] = CredentialApplicationAttribute::new("Aura_RecordState", "committed")
            .expect("committed state attribute");
        store
            .write_with_attributes(&reference, &second_secret, &committed)
            .expect("replace credential");
        let stored = store.read_record(&reference).expect("read committed credential");
        assert_eq!(stored.secret().expose(), second_secret.expose());
        assert_eq!(stored.attributes(), committed);
        assert!(store.delete(&reference).expect("delete credential"));
    }

    #[test]
    fn duplicate_application_attribute_keywords_are_rejected() {
        let reference = CredentialRef::for_test("duplicate", "attribute").expect("test reference");
        let secret = SecretBytes::new(b"aura-secret".to_vec());
        let attributes = [
            CredentialApplicationAttribute::new("Aura_RecordState", "staged")
                .expect("first attribute"),
            CredentialApplicationAttribute::new("Aura_RecordState", "committed")
                .expect("second attribute"),
        ];
        assert!(matches!(
            WindowsCredentialStore.write_with_attributes(&reference, &secret, &attributes),
            Err(CredentialStoreError::DuplicateAttribute { .. })
        ));
    }
}
