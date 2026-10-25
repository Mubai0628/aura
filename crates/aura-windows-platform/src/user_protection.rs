//! Data only the current Windows user can read back (DPAPI, user scope). `entropy` binds a
//! blob to what it belongs to: the same bytes with another entropy do not decrypt.
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use zeroize::Zeroize;

/// Upper bound for one protected value (an OAuth token set is a few KiB).
const MAX_BYTES: usize = 64 * 1024;

fn blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| io::Error::other("value too large"))?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// Takes DPAPI's output into an owned buffer and frees DPAPI's allocation; the copy DPAPI
/// made is wiped first.
fn take(output: CRYPT_INTEGER_BLOB) -> io::Result<Vec<u8>> {
    if output.pbData.is_null() {
        return Err(io::Error::other("DPAPI returned no data"));
    }
    // SAFETY: successful DPAPI output is valid for cbData bytes and uniquely owned.
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let owned = bytes.to_vec();
    bytes.zeroize();
    // SAFETY: the buffer was allocated by DPAPI with LocalAlloc.
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(owned)
}

/// Encrypts `value` for the current user, bound to `entropy`.
pub fn protect_for_current_user(value: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
    if value.is_empty() || value.len() > MAX_BYTES || entropy.is_empty() {
        return Err(io::Error::other("invalid value to protect"));
    }
    let input = blob(value)?;
    let entropy = blob(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input and entropy point to retained buffers; DPAPI allocates output.
    if unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    take(output)
}

/// Decrypts a value [`protect_for_current_user`] made with the same `entropy`.
pub fn unprotect_for_current_user(
    protected: &[u8],
    entropy: &[u8],
) -> io::Result<crate::SecretBytes> {
    if protected.is_empty() || protected.len() > 2 * MAX_BYTES || entropy.is_empty() {
        return Err(io::Error::other("invalid protected value"));
    }
    let input = blob(protected)?;
    let entropy = blob(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input and entropy point to retained buffers; DPAPI allocates output.
    if unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            &entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    take(output).map(crate::SecretBytes::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_comes_back_only_with_its_own_entropy() {
        let protected = protect_for_current_user(b"token-set", b"server-a").expect("protected");
        assert!(!protected.windows(9).any(|window| window == b"token-set"));
        let back = unprotect_for_current_user(&protected, b"server-a").expect("decrypted");
        assert_eq!(back.expose(), b"token-set");
        assert!(unprotect_for_current_user(&protected, b"server-b").is_err());
        let mut tampered = protected.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(unprotect_for_current_user(&tampered, b"server-a").is_err());
    }
}
