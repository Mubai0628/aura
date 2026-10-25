//! File hashes kept between Skill listings.
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// A file as last seen: its path, length and modification time.
pub type FileIdentity = (PathBuf, u64, Option<SystemTime>);

/// Hashes kept between listings (about 300 bytes each). Past it, the half used longest ago is
/// forgotten. It used to forget everything past 4,096 files, so ten full Skills were all
/// read again at every listing.
const KEPT_HASHES: usize = 32_768;

/// File hashes by path, length and modification time, so listing Skills does not read every
/// file again. Text handed to the model is always hashed from the bytes actually read.
#[derive(Default)]
pub struct FileHashCache(Mutex<Kept>);

/// Each hash with the moment it was last used.
#[derive(Default)]
struct Kept {
    hashes: HashMap<FileIdentity, (String, u64)>,
    tick: u64,
}

impl Kept {
    fn forget_the_older_half(&mut self) {
        let mut used: Vec<u64> = self.hashes.values().map(|(_, at)| *at).collect();
        let middle = used.len() / 2;
        let (_, cut, _) = used.select_nth_unstable(middle);
        let cut = *cut;
        self.hashes.retain(|_, (_, at)| *at > cut);
    }
}

impl FileHashCache {
    /// The BLAKE3 of the file at `path`, read again only when its length or modification time
    /// changed since the last read.
    pub fn hash(&self, path: &Path, metadata: &std::fs::Metadata) -> std::io::Result<String> {
        let key = (path.to_path_buf(), metadata.len(), metadata.modified().ok());
        if let Some(hash) = self.cached(&key) {
            return Ok(hash);
        }
        let mut file = std::fs::File::open(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let hash = hasher.finalize().to_hex().to_string();
        self.remember(key, &hash);
        Ok(hash)
    }

    /// Records the hash of bytes read elsewhere, for the file as it was then.
    pub fn remember(&self, key: FileIdentity, hash: &str) {
        if let Ok(mut kept) = self.0.lock() {
            if kept.hashes.len() >= KEPT_HASHES && !kept.hashes.contains_key(&key) {
                kept.forget_the_older_half();
            }
            kept.tick += 1;
            let at = kept.tick;
            kept.hashes.insert(key, (hash.to_owned(), at));
        }
    }

    /// A known hash, now counted as just used.
    fn cached(&self, key: &FileIdentity) -> Option<String> {
        let mut kept = self.0.lock().ok()?;
        kept.tick += 1;
        let at = kept.tick;
        let (hash, used) = kept.hashes.get_mut(key)?;
        *used = at;
        Some(hash.clone())
    }

    #[cfg(test)]
    fn knows(&self, key: &FileIdentity) -> bool {
        self.0.lock().is_ok_and(|kept| kept.hashes.contains_key(key))
    }

    #[cfg(test)]
    fn kept(&self) -> usize {
        self.0.lock().map_or(0, |kept| kept.hashes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: usize) -> FileIdentity {
        (PathBuf::from(format!(r"C:\skills\many\{n}.md")), n as u64, None)
    }

    /// 5,000 files (ten
    /// Skills at their 500-file limit) are all still known at the next listing, which then reads
    /// none of them again.
    #[test]
    fn a_listing_of_many_skills_keeps_every_hash() {
        let cache = FileHashCache::default();
        for n in 0..5_000 {
            cache.remember(key(n), "hash");
        }
        assert_eq!(cache.kept(), 5_000);
        assert!((0..5_000).all(|n| cache.knows(&key(n))));
    }

    #[test]
    fn past_the_limit_the_hashes_used_longest_ago_are_forgotten() {
        let cache = FileHashCache::default();
        for n in 0..KEPT_HASHES {
            cache.remember(key(n), "hash");
        }
        // The first file was looked at again just now: it is among the recently used.
        assert!(cache.cached(&key(0)).is_some());
        cache.remember(key(KEPT_HASHES), "hash");
        assert!(cache.knows(&key(0)) && cache.knows(&key(KEPT_HASHES)));
        assert!(!cache.knows(&key(1)), "the one used longest ago");
        let kept = cache.kept();
        assert!((KEPT_HASHES / 2 - 1..=KEPT_HASHES / 2 + 1).contains(&kept), "{kept}");
    }
}
