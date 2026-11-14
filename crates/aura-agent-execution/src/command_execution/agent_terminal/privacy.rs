//! Live-only salted prefix hashes mask echoes across chunks without retaining input.
use aura_domain_common::{SecurityOperationId, jcs::CanonicalSha256HashV1};
use std::collections::BTreeSet;
use zeroize::{Zeroize, Zeroizing};
const MASK: &str = "[安全输入回显已屏蔽]";
pub(super) struct EchoPrivacy {
    salt: String,
    /// Every proper beginning of an input: text that can still grow into one.
    prefixes: BTreeSet<String>,
    complete: BTreeSet<String>,
    pending: Zeroizing<Vec<u8>>,
    /// How much of `pending` is a whole input while a longer input can still follow from it.
    matched: usize,
    firsts: BTreeSet<u64>,
    first_salt: u64,
}
impl Default for EchoPrivacy {
    fn default() -> Self {
        let id = SecurityOperationId::new();
        Self {
            salt: id.to_string(),
            prefixes: Default::default(),
            complete: Default::default(),
            pending: Zeroizing::new(Vec::new()),
            matched: 0,
            firsts: Default::default(),
            first_salt: u64::from_le_bytes(id.to_bytes()[8..].try_into().expect("ID bytes")),
        }
    }
}
impl EchoPrivacy {
    fn first(&self, ch: char) -> u64 {
        u64::from(u32::from(ch)).wrapping_mul(0x9e3779b97f4a7c15).rotate_left(17) ^ self.first_salt
    }
    fn hash(&self, value: &[u8]) -> String {
        let mut bytes = Zeroizing::new(self.salt.as_bytes().to_vec());
        // Console line wrapping can insert LF inside an otherwise literal echo.
        bytes.extend(value.iter().copied().filter(|byte| *byte != b'\n'));
        CanonicalSha256HashV1::sha256(&bytes).hex().to_owned()
    }
    pub fn add(&mut self, value: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.prefixes.len().saturating_add(value.chars().count()) * 64 <= 16 * 1024 * 1024,
            "terminal.sensitive.privacy_capacity: 请关闭该终端并新建后再输入。"
        );
        for (index, ch) in value.char_indices() {
            let end = index + ch.len_utf8();
            if end < value.len() {
                self.prefixes.insert(self.hash(&value.as_bytes()[..end]));
            }
        }
        self.complete.insert(self.hash(value.as_bytes()));
        if let Some(ch) = value.chars().next() {
            self.firsts.insert(self.first(ch));
        }
        Ok(())
    }
    pub fn push(&mut self, text: &str, final_chunk: bool) -> String {
        if self.complete.is_empty() {
            return text.into();
        }
        let mut output = String::new();
        for ch in text.chars() {
            if self.pending.is_empty() && !self.firsts.contains(&self.first(ch)) {
                output.push(ch);
                continue;
            }
            let mut utf8 = [0; 4];
            self.pending.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
            utf8.zeroize();
            if self.pending.len() > 4096 {
                self.mask(&mut output);
                continue;
            }
            while !self.pending.is_empty() {
                let hash = self.hash(&self.pending);
                let longer = self.prefixes.contains(&hash);
                if self.complete.contains(&hash) {
                    if !longer {
                        self.mask(&mut output);
                        break;
                    }
                    // A longer input may still follow: this much is masked whatever comes next.
                    self.matched = self.pending.len();
                }
                if longer {
                    break;
                }
                if self.matched > 0 {
                    // The longer input did not follow: mask the whole input matched before it and
                    // read on from the text after that input.
                    let rest = Zeroizing::new(self.pending[self.matched..].to_vec());
                    self.pending.truncate(self.matched);
                    self.mask(&mut output);
                    self.pending.extend_from_slice(&rest);
                    continue;
                }
                let previous = std::mem::replace(&mut self.pending, Zeroizing::new(Vec::new()));
                let first = std::str::from_utf8(&previous)
                    .expect("complete UTF-8 characters")
                    .chars()
                    .next()
                    .expect("nonempty");
                output.push(first);
                self.pending.extend_from_slice(&previous[first.len_utf8()..]);
            }
        }
        if final_chunk && !self.pending.is_empty() {
            self.mask(&mut output);
        }
        output
    }
    /// `line`, a whole line the terminal shows, with every echo of a secure input in it masked.
    /// The line is complete, so text that only begins an input is left as it is.
    pub fn masked_line(&self, line: &str) -> String {
        if self.complete.is_empty() {
            return line.into();
        }
        let mut output = String::new();
        let mut rest = line;
        while let Some(ch) = rest.chars().next() {
            // The longest input that starts here: one input can begin another.
            let mut matched = None;
            if self.firsts.contains(&self.first(ch)) {
                for (index, next) in rest.char_indices() {
                    let end = index + next.len_utf8();
                    let hash = self.hash(&rest.as_bytes()[..end]);
                    if self.complete.contains(&hash) {
                        matched = Some(end);
                    }
                    if !self.prefixes.contains(&hash) {
                        break;
                    }
                }
            }
            match matched {
                Some(end) => {
                    output.push_str(MASK);
                    rest = &rest[end..];
                }
                None => {
                    output.push(ch);
                    rest = &rest[ch.len_utf8()..];
                }
            }
        }
        output
    }
    fn mask(&mut self, output: &mut String) {
        let lines = self.pending.iter().filter(|byte| **byte == b'\n').count();
        self.pending.zeroize();
        self.matched = 0;
        output.push_str(MASK);
        output.extend(std::iter::repeat_n('\n', lines));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn l02_b4_line_wrapped_echo_never_exposes_the_input_fragments() {
        let mut privacy = EchoPrivacy::default();
        privacy.add("temporary-wrapped-value").expect("L02 test fact");
        let value = privacy.push("temporary-\nwrapped-value", true);
        assert_eq!(value, format!("{MASK}\n"));
    }
    #[test]
    fn l02_b4_split_repeated_unicode_and_delayed_echoes_are_masked_without_holding_input() {
        let mut privacy = EchoPrivacy::default();
        privacy.add("测试Aa123!").expect("L02 test fact");
        privacy.add("Aa123!").expect("L02 test fact");
        let mut result = String::new();
        for ch in "prompt\n测试Aa123!\nlength=8\nAa123!\nlater 测试Aa123!\n".chars() {
            result.push_str(&privacy.push(&ch.to_string(), false));
        }
        result.push_str(&privacy.push("", true));
        assert!(!result.contains("测试Aa123!") && !result.contains("Aa123!"));
        assert!(result.contains("length=8"));
        assert_eq!(result.matches(MASK).count(), 3);
        assert!(privacy.pending.is_empty());
    }
    #[test]
    fn a_whole_line_masks_every_echo_and_leaves_a_mere_beginning() {
        let mut privacy = EchoPrivacy::default();
        privacy.add("hunter2").expect("test value");
        privacy.add("测试Aa1").expect("test value");
        assert_eq!(
            privacy.masked_line("pw: hunter2 again hunter2"),
            format!("pw: {MASK} again {MASK}")
        );
        assert_eq!(privacy.masked_line("hhunter2"), format!("h{MASK}"));
        assert_eq!(privacy.masked_line("ends with hunt"), "ends with hunt");
        assert_eq!(privacy.masked_line("值=测试Aa1。"), format!("值={MASK}。"));
        assert_eq!(EchoPrivacy::default().masked_line("hunter2"), "hunter2");
    }
    #[test]
    fn an_input_that_begins_another_leaves_nothing_of_the_longer_one() {
        let mut privacy = EchoPrivacy::default();
        privacy.add("pass").expect("test value");
        privacy.add("password1").expect("test value");
        assert_eq!(privacy.masked_line("password1 pass passw"), format!("{MASK} {MASK} {MASK}w"));
    }
    #[test]
    fn a_streamed_input_that_begins_another_leaves_nothing_of_the_longer_one() {
        let mut privacy = EchoPrivacy::default();
        privacy.add("pass").expect("test value");
        privacy.add("password1").expect("test value");
        let mut result = String::new();
        for ch in "a password1 b pass c passwx\n".chars() {
            result.push_str(&privacy.push(&ch.to_string(), false));
        }
        result.push_str(&privacy.push("", true));
        assert_eq!(result, format!("a {MASK} b {MASK} c {MASK}wx\n"));
        assert!(privacy.pending.is_empty());
        let whole = privacy.push("x password1 y\n", true);
        assert_eq!(whole, format!("x {MASK} y\n"));
    }
}
