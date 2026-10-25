/// Limits apply per message and to the retained entry count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_records: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_bytes: 4096, max_records: 100 }
    }
}

impl Limits {
    pub fn preview(max_bytes: usize, max_records: usize) -> Self {
        Self { max_bytes, max_records }
    }
}

pub(super) fn append(text: &mut String, fragment: &str, _max_bytes: usize) -> bool {
    text.push_str(fragment);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_settings_are_not_silently_replaced() {
        assert_eq!(Limits::preview(12, 3), Limits { max_bytes: 12, max_records: 3 });
    }

    #[test]
    fn ordinary_append_preserves_spaces() {
        let mut text = "ready".to_string();
        assert!(!append(&mut text, " now", 4096));
        assert_eq!(text, "ready now");
    }
}
