use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

const CAPACITY: usize = 128;

fn successful_documents() -> &'static Mutex<VecDeque<String>> {
    static CACHE: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(super) fn contains(actual_hash: &str) -> bool {
    // Poisoning only disables this optimization; it never skips policy validation.
    successful_documents().lock().is_ok_and(|cache| cache.iter().any(|known| known == actual_hash))
}

pub(super) fn remember(actual_hash: &str) {
    if let Ok(mut cache) = successful_documents().lock() {
        if cache.iter().any(|known| known == actual_hash) {
            return;
        }
        if cache.len() == CAPACITY {
            cache.pop_front();
        }
        cache.push_back(actual_hash.to_owned());
    }
}
