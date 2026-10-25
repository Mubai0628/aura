//! Read the installation account password through the current owner's DPAPI.
//! The caller must first verify the protected file's binding and ownership.
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData},
};
use zeroize::Zeroize;

pub fn decrypt_execution_account_secret(ciphertext: &[u8]) -> io::Result<crate::SecretBytes> {
    if ciphertext.is_empty() || ciphertext.len() > 16_384 {
        return Err(io::Error::other("invalid protected account credential"));
    }
    let input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: ciphertext is a retained byte buffer; DPAPI allocates output.
    if unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if output.pbData.is_null() {
        return Err(io::Error::other("DPAPI returned an empty credential"));
    }
    // SAFETY: successful DPAPI output is valid for cbData bytes, uniquely owned.
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let result = if bytes.len() == 72
        && bytes.starts_with(b"R06!aA9-")
        && bytes[8..].iter().all(|v| v.is_ascii_alphanumeric() || matches!(v, b'+' | b'/'))
    {
        Ok(crate::SecretBytes::new(bytes.to_vec()))
    } else {
        Err(io::Error::other("protected account credential format mismatch"))
    };
    bytes.zeroize();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    result
}
