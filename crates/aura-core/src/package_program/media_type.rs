//! Shared media grammar; legacy and V2 output imports enforce the same rules.
pub(crate) fn valid_media_type_v1(value: &str) -> bool {
    if !value.is_ascii() || value.is_empty() {
        return false;
    }
    let mut parts = value.split(';');
    let Some(essence) = parts.next() else { return false };
    let Some((kind, subtype)) = essence.split_once('/') else { return false };
    if kind.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !kind.bytes().all(is_media_token_byte)
        || !subtype.bytes().all(is_media_token_byte)
    {
        return false;
    }
    parts.all(|parameter| {
        let parameter = parameter.trim_start_matches([' ', '\t']);
        let Some((name, value)) = parameter.split_once('=') else { return false };
        !name.is_empty()
            && !value.is_empty()
            && name.bytes().all(is_media_token_byte)
            && (value.bytes().all(is_media_token_byte) || valid_quoted_media_value(value))
    })
}

fn is_media_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn valid_quoted_media_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'"') || bytes.last() != Some(&b'"') {
        return false;
    }
    let mut escaped = false;
    for byte in &bytes[1..bytes.len() - 1] {
        if escaped {
            escaped = false;
            if !matches!(*byte, b'\t' | 0x20..=0x7e) {
                return false;
            }
        } else if *byte == b'\\' {
            escaped = true;
        } else if !matches!(*byte, b'\t' | 0x20..=0x7e) || *byte == b'"' {
            return false;
        }
    }
    !escaped
}
