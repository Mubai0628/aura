pub fn page(items: &[i32], cursor: usize, limit: usize) -> Vec<i32> {
    let _ = cursor;
    items.iter().take(limit).copied().collect()
}
