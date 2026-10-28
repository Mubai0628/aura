//! Counting a request before any provider has counted one of its model.
//!
//! The usual rules of thumb: English and code about three characters a token, a Chinese, Japanese
//! or Korean character about one, an emoji about two. They are near what the connected providers
//! count and can be low for some tokenizers. A provider that then finds the request too long
//! refuses it, and the request is rebuilt smaller; a provider's count of a request replaces the
//! rule from the next request on ([`crate::MeasuredRequest`]).

/// ASCII characters a token.
const ASCII_CHARACTERS_PER_TOKEN: u64 = 3;
/// The tokens of a character outside the Basic Multilingual Plane (an emoji, a rare ideograph).
const SUPPLEMENTARY_CHARACTER_TOKENS: u64 = 2;

/// The tokens of a text by the rule of thumb, never more than its bytes.
#[must_use]
pub fn character_class_tokens(text: &str) -> u64 {
    let (mut ascii, mut other) = (0_u64, 0_u64);
    for character in text.chars() {
        match character.len_utf8() {
            1 => ascii += 1,
            4 => other += SUPPLEMENTARY_CHARACTER_TOKENS,
            _ => other += 1,
        }
    }
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    ascii.div_ceil(ASCII_CHARACTERS_PER_TOKEN).saturating_add(other).min(bytes)
}

#[cfg(test)]
mod tests {
    use super::character_class_tokens;

    #[test]
    fn english_and_code_count_three_characters_a_token() {
        assert_eq!(character_class_tokens(""), 0);
        assert_eq!(character_class_tokens("a"), 1);
        assert_eq!(character_class_tokens("abc"), 1);
        assert_eq!(character_class_tokens("fn main() {}"), 4);
    }

    #[test]
    fn a_chinese_character_counts_one_token_and_an_emoji_two() {
        assert_eq!(character_class_tokens("上下文"), 3, "nine bytes, three tokens");
        assert_eq!(character_class_tokens("é"), 1);
        assert_eq!(character_class_tokens("😀"), 2);
        // "Hello, " and "!" are eight ASCII characters, three tokens; 世界 two more.
        assert_eq!(character_class_tokens("Hello, 世界!"), 5);
    }
}
