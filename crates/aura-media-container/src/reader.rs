//! Bounded positional reads of a file: every read counts, and a header that needs more than the
//! limits allow is left unread.
use std::io;

/// How many reads, and how many bytes in all, one file's header may take.
pub(crate) const MAX_READS: u32 = 4096;
pub(crate) const MAX_BYTES: u64 = 1024 * 1024;

pub(crate) const TRUNCATED: &str = "文件头在文件结束前没有读完，文件可能不完整";

/// Reads into the buffer from the file offset, returning how many bytes were read (0 at the end).
pub type ReadAt<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> io::Result<usize>;

pub(crate) struct Reader<'a> {
    pub size: u64,
    read_at: ReadAt<'a>,
    reads: u32,
    bytes: u64,
}

impl<'a> Reader<'a> {
    pub fn new(size: u64, read_at: ReadAt<'a>) -> Self {
        Self { size, read_at, reads: 0, bytes: 0 }
    }

    /// `length` bytes from `offset`, or why they were not read.
    pub fn bytes(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        if offset.checked_add(length as u64).is_none_or(|end| end > self.size) {
            return Err(TRUNCATED.into());
        }
        self.reads += 1;
        self.bytes += length as u64;
        if self.reads > MAX_READS || self.bytes > MAX_BYTES {
            return Err(format!(
                "文件头的结构超过了读取上限（{MAX_READS} 次读取、{} KiB），没有读完",
                MAX_BYTES / 1024
            ));
        }
        let mut buffer = vec![0; length];
        let mut filled = 0;
        while filled < length {
            let count = (self.read_at)(offset + filled as u64, &mut buffer[filled..])
                .map_err(|error| format!("读文件头时出错（{error}）"))?;
            if count == 0 {
                return Err(TRUNCATED.into());
            }
            filled += count;
        }
        Ok(buffer)
    }

    pub fn u32_at(&mut self, offset: u64) -> Result<u32, String> {
        let bytes = self.bytes(offset, 4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn u64_at(&mut self, offset: u64) -> Result<u64, String> {
        let bytes = self.bytes(offset, 8)?;
        Ok(u64::from_be_bytes(bytes.try_into().expect("eight bytes were read")))
    }
}
