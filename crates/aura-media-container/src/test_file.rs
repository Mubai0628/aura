//! Files for the tests: parts at offsets with zeros between them, so a file of gigabytes needs only
//! its headers in memory, and every read is counted.
use crate::{VideoTrack, video_track};
use std::io;

pub(crate) struct Sparse {
    parts: Vec<(u64, Vec<u8>)>,
    pub size: u64,
}

impl Sparse {
    pub fn new(size: u64) -> Self {
        Self { parts: Vec::new(), size }
    }

    pub fn put(mut self, offset: u64, bytes: Vec<u8>) -> Self {
        self.parts.push((offset, bytes));
        self
    }

    /// The track, and how many bytes reading it took.
    pub fn track(&self) -> (VideoTrack, u64) {
        let mut read = 0;
        let mut read_at = |offset: u64, buffer: &mut [u8]| -> io::Result<usize> {
            let length = (buffer.len() as u64).min(self.size.saturating_sub(offset)) as usize;
            buffer[..length].fill(0);
            for (start, bytes) in &self.parts {
                for (index, byte) in buffer[..length].iter_mut().enumerate() {
                    let at = offset + index as u64;
                    if at >= *start && at < start + bytes.len() as u64 {
                        *byte = bytes[(at - start) as usize];
                    }
                }
            }
            read += length as u64;
            Ok(length)
        };
        let track = video_track(self.size, &mut read_at);
        (track, read)
    }
}

/// The track of a file holding exactly `bytes`.
pub(crate) fn track_of(bytes: Vec<u8>) -> VideoTrack {
    Sparse::new(bytes.len() as u64).put(0, bytes).track().0
}
