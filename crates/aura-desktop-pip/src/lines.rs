//! Lines out of a byte stream: each up to a limit, without its line break. A longer line is read
//! to its end and dropped.
use std::io::{self, BufRead, Read};

pub(crate) enum Line {
    Line(Vec<u8>),
    TooLong,
}

/// The next line of `input`, or `None` at its end. A last line without a break counts.
pub(crate) fn next_line(input: &mut impl BufRead, limit: usize) -> io::Result<Option<Line>> {
    let mut line = Vec::new();
    let read = Read::take(&mut *input, limit as u64 + 1).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        return Ok(Some(if line.len() > limit { Line::TooLong } else { Line::Line(line) }));
    }
    if line.len() <= limit {
        return Ok(Some(Line::Line(line)));
    }
    skip_rest_of_line(input)?;
    Ok(Some(Line::TooLong))
}

fn skip_rest_of_line(input: &mut impl BufRead) -> io::Result<()> {
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok(());
        }
        if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            input.consume(end + 1);
            return Ok(());
        }
        let length = buffer.len();
        input.consume(length);
    }
}
