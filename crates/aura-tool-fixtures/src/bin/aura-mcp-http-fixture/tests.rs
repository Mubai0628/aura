//! What the server notes for a test to read.
use super::*;

/// Each connection runs on its own thread. Lines noted at once stay whole, because a test counts
/// each one by the start of its line: one written into the middle of another would be lost.
#[test]
fn lines_noted_at_once_by_many_connections_stay_whole() {
    let folder = tempfile::tempdir().expect("marker folder");
    let marker = folder.path().join("marker.txt");
    let server = Arc::new(Server {
        state: Mutex::new(State::default()),
        marker: Some(marker.to_string_lossy().into_owned()),
        noting: Mutex::new(()),
        auth: None,
        oauth: None,
    });
    let threads: Vec<_> = (0..8)
        .map(|thread| {
            let server = Arc::clone(&server);
            std::thread::spawn(move || {
                for line in 0..200 {
                    server.note(&format!("call tool{thread} {{line {line}}}"));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("a noting thread");
    }
    let text = std::fs::read_to_string(&marker).expect("the marker");
    let lines: Vec<&str> = text.lines().collect();
    for line in &lines {
        assert!(line.starts_with("call tool") && line.matches("call ").count() == 1, "{line:?}");
    }
    assert_eq!(lines.len(), 8 * 200);
}
