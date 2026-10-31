//! Incremental VT removal, including sequences and UTF-8 split across pipe reads.
#[derive(Default)]
pub(super) struct PlainText {
    state: u8,
    utf8: Vec<u8>,
}
impl PlainText {
    pub fn push(&mut self, bytes: &[u8], final_chunk: bool) -> String {
        let mut plain = Vec::new();
        for &byte in bytes {
            match self.state {
                0 => match byte {
                    0x1b => self.state = 1,
                    b'\r' | 0..=8 | 11..=12 | 14..=31 | 127 => {}
                    _ => plain.push(byte),
                },
                1 => {
                    self.state = match byte {
                        b'[' => 2,
                        b']' | b'P' | b'X' | b'^' | b'_' => 3,
                        b'(' | b')' | b'*' | b'+' => 5,
                        _ => 0,
                    }
                }
                2 => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = 0;
                    }
                }
                3 => match byte {
                    7 => self.state = 0,
                    0x1b => self.state = 4,
                    _ => {}
                },
                4 => self.state = if byte == b'\\' { 0 } else { 3 },
                _ => self.state = 0,
            }
        }
        self.utf8.extend(plain);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.utf8) {
                Ok(text) => {
                    output.push_str(text);
                    self.utf8.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    output.push_str(
                        std::str::from_utf8(&self.utf8[..valid]).expect("valid UTF-8 prefix"),
                    );
                    self.utf8.drain(..valid);
                    if let Some(length) = error.error_len() {
                        output.push('\u{fffd}');
                        self.utf8.drain(..length);
                    } else {
                        if final_chunk {
                            output.push('\u{fffd}');
                            self.utf8.clear();
                        }
                        break;
                    }
                }
            }
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn l02_split_utf8_csi_osc_and_string_controls_never_reach_model_text() {
        let bytes = "\x1b[31m中\x1b[0m\x1b]0;private title\x07文\x1bPignored\x1b\\\r\n".as_bytes();
        let mut parser = PlainText::default();
        let mut result = String::new();
        for byte in bytes {
            result.push_str(&parser.push(&[*byte], false));
        }
        result.push_str(&parser.push(&[], true));
        assert_eq!(result, "中文\n");
    }
}
