use super::*;

const MASK: &str = "[安全输入回显已屏蔽]";

fn screen(columns: u16, rows: u16) -> ScreenLines {
    ScreenLines::new(TerminalSize { columns, rows }, 0)
}

fn feed(lines: &mut ScreenLines, privacy: &EchoPrivacy, text: &str) {
    let start = lines.fed;
    lines.push(start, text.as_bytes(), privacy);
}

fn read(lines: &ScreenLines, privacy: &EchoPrivacy, from: Option<u64>, max: usize) -> Page {
    let shown = lines.shown(privacy);
    lines.page(&shown, from, max)
}

fn all(lines: &ScreenLines, privacy: &EchoPrivacy) -> String {
    read(lines, privacy, Some(1), 2000).lines
}

#[test]
fn a_line_redrawn_in_place_reads_as_the_screen_shows_it() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(80, 24);
    // A typo fixed by moving back and erasing, as PSReadLine redraws the input line.
    feed(&mut lines, &privacy, "PS C:\\w> gti\x1b[3D\x1b[Kgit status\r\n");
    // A progress line rewritten after carriage returns.
    feed(&mut lines, &privacy, "Downloading 10%\rDownloading 55%\rDownloading 100%\r\n");
    // Wide characters written again over themselves.
    feed(&mut lines, &privacy, "口令\x1b[4D口令\r\nPS C:\\w> ");
    assert_eq!(all(&lines, &privacy), "PS C:\\w> git status\nDownloading 100%\n口令\nPS C:\\w>");
    // The same output with only its controls removed, as terminal.read gave it before.
    let mut plain = super::super::text::PlainText::default();
    let removed = plain.push(
        "PS C:\\w> gti\x1b[3D\x1b[Kgit status\r\nDownloading 10%\rDownloading 55%\r\n".as_bytes(),
        true,
    );
    assert_eq!(removed, "PS C:\\w> gtigit status\nDownloading 10%Downloading 55%\n");
}

#[test]
fn a_line_wrapped_at_the_width_is_one_line_on_the_screen_and_after_it_leaves() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(10, 3);
    feed(&mut lines, &privacy, "0123456789abcde\r\n");
    assert_eq!(all(&lines, &privacy), "0123456789abcde");
    feed(&mut lines, &privacy, "x\r\ny\r\nz\r\nw");
    assert_eq!(lines.kept.front().map(String::as_str), Some("0123456789abcde"));
    assert_eq!(all(&lines, &privacy), "0123456789abcde\nx\ny\nz\nw");
}

#[test]
fn lines_that_leave_the_screen_are_kept_in_order_and_numbered() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(20, 3);
    feed(&mut lines, &privacy, "1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(
        read(&lines, &privacy, Some(1), 2000),
        Page { lines: "1\n2\n3\n4\n5".into(), first: 1, last: 5, next: None, earliest: 1 }
    );
}

#[test]
fn a_full_screen_programs_screen_leaves_nothing_behind() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(20, 3);
    feed(&mut lines, &privacy, "before\r\n\x1b[?1049h");
    feed(&mut lines, &privacy, "vim-1\r\nvim-2\r\nvim-3\r\nvim-4\r\nvim-5");
    assert_eq!(all(&lines, &privacy), "vim-3\nvim-4\nvim-5", "what it shows while it runs");
    feed(&mut lines, &privacy, "\x1b[?1049lafter\r\n1\r\n2\r\n3");
    let text = all(&lines, &privacy);
    assert!(text.starts_with("before\nafter") && !text.contains("vim"), "{text}");
}

#[test]
fn echoes_of_secure_input_are_masked_on_the_screen_and_once_kept() {
    let mut privacy = EchoPrivacy::default();
    privacy.add("hunter2").expect("test value");
    let mut lines = screen(30, 2);
    feed(&mut lines, &privacy, "pw: hunter2\r\n");
    assert_eq!(all(&lines, &privacy), format!("pw: {MASK}"));
    feed(&mut lines, &privacy, "x\r\ny\r\nz");
    assert_eq!(lines.kept.front().map(String::as_str), Some(format!("pw: {MASK}").as_str()));
    assert!(!all(&lines, &privacy).contains("hunter2"));
}

#[test]
fn the_newest_lines_come_first_and_pages_read_on() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(20, 5);
    let text = (1..=10).map(|n| format!("line-{n}")).collect::<Vec<_>>().join("\r\n");
    feed(&mut lines, &privacy, &text);
    let newest = read(&lines, &privacy, None, 3);
    assert_eq!(
        (newest.lines.as_str(), newest.first, newest.last),
        ("line-8\nline-9\nline-10", 8, 10)
    );
    assert_eq!(newest.next, None);
    let page = read(&lines, &privacy, Some(2), 3);
    assert_eq!(
        (page.lines.as_str(), page.first, page.next),
        ("line-2\nline-3\nline-4", 2, Some(5))
    );
    let past = read(&lines, &privacy, Some(11), 3);
    assert_eq!((past.lines.as_str(), past.first, past.next), ("", 11, None));
    assert_eq!(read(&lines, &privacy, Some(0), 1).first, 1);
}

#[test]
fn lines_beyond_what_is_kept_are_dropped_from_the_oldest() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(20, 2);
    let text = (1..=5005).map(|n| format!("n-{n}\r\n")).collect::<String>();
    feed(&mut lines, &privacy, &text);
    let oldest = read(&lines, &privacy, Some(1), 2);
    assert_eq!((oldest.lines.as_str(), oldest.first, oldest.earliest), ("n-4\nn-5", 4, 4));
    let newest = read(&lines, &privacy, None, 1);
    assert_eq!((newest.lines.as_str(), newest.last), ("n-5005", 5005));
}

#[test]
fn a_resize_takes_effect_at_the_output_printed_after_it() {
    let privacy = EchoPrivacy::default();
    let wide = TerminalSize { columns: 20, rows: 3 };
    let at_column_15 = format!("a{}x", " ".repeat(13));
    // Made once the output before it was read.
    let mut lines = screen(10, 3);
    feed(&mut lines, &privacy, "a");
    lines.resize_at(1, wide, &privacy);
    feed(&mut lines, &privacy, "\x1b[1;15Hx");
    assert_eq!(all(&lines, &privacy), at_column_15);
    // Made before the output around it was read: it takes effect between the two parts.
    let mut lines = screen(10, 3);
    lines.resize_at(1, wide, &privacy);
    feed(&mut lines, &privacy, "a\x1b[1;15Hx");
    assert_eq!(all(&lines, &privacy), at_column_15);
    // Without it, the screen is still 10 columns wide.
    let mut lines = screen(10, 3);
    feed(&mut lines, &privacy, "a\x1b[1;15Hx");
    assert_eq!(all(&lines, &privacy), format!("a{}x", " ".repeat(8)));
}

#[test]
fn a_page_holds_at_most_its_bytes_and_cuts_a_longer_line() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(200, 3);
    feed(&mut lines, &privacy, &format!("{}\r\nz\r\n", "y".repeat(40_000)));
    let page = read(&lines, &privacy, Some(1), 10);
    assert!(
        page.lines.starts_with("yyy") && page.lines.contains("字节没有显示"),
        "{}",
        page.lines.len()
    );
    assert!(page.lines.len() <= PAGE_BYTES);
    assert_eq!((page.first, page.next), (1, Some(2)));

    let mut lines = screen(1000, 5);
    let text = (0..100).map(|_| "w".repeat(1000)).collect::<Vec<_>>().join("\r\n");
    feed(&mut lines, &privacy, &text);
    let newest = read(&lines, &privacy, None, 200);
    assert_eq!((newest.first, newest.last), (69, 100));
}

#[test]
fn blank_rows_before_the_first_text_are_not_lines() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(20, 5);
    feed(&mut lines, &privacy, "\x1b[4;1Hhello");
    let page = read(&lines, &privacy, Some(1), 10);
    assert_eq!((page.lines.as_str(), page.first, page.last), ("hello", 1, 1));
    feed(&mut lines, &privacy, "\r\n1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(all(&lines, &privacy), "hello\n1\n2\n3\n4\n5", "hello stays line 1");
}

#[test]
fn what_the_screen_showed_last_is_kept_when_the_terminal_ends() {
    let mut privacy = EchoPrivacy::default();
    privacy.add("hunter2").expect("test value");
    let mut lines = screen(20, 3);
    feed(&mut lines, &privacy, "x\r\nsecret: hunter2\r\ny");
    lines.end(&privacy);
    feed(&mut lines, &EchoPrivacy::default(), "\r\nlater");
    assert!(lines.shown(&privacy).is_empty());
    let page = lines.page(&[], Some(1), 10);
    assert_eq!(page.lines, format!("x\nsecret: {MASK}\ny"));
}

#[test]
fn saved_text_reads_as_lines() {
    let lines = ScreenLines::saved("\n\na  \n\nb\n");
    let page = lines.page(&[], None, 10);
    assert_eq!((page.lines.as_str(), page.first, page.last), ("a\n\nb", 1, 3));
}

#[test]
fn the_lines_since_a_write_read_as_the_screen_shows_them_within_a_smaller_page() {
    let privacy = EchoPrivacy::default();
    let mut lines = screen(80, 5);
    feed(&mut lines, &privacy, "earlier\r\nPS C:\\w> ");
    let from = lines.last_line(&lines.shown(&privacy));
    assert_eq!(from, 2);
    feed(&mut lines, &privacy, "dl\r\n进度 1%\r进度 50%\r进度 100%\r\nPS C:\\w> ");
    let page = lines.page_within(&lines.shown(&privacy), Some(from), 200, 8192);
    assert_eq!(page.lines, "PS C:\\w> dl\n进度 100%\nPS C:\\w>");
    assert_eq!((page.first, page.next), (2, None));
    // More than the page holds: it ends where its bytes do and names the line after it.
    for number in 0..400 {
        feed(&mut lines, &privacy, &format!("line {number:03} {}\r\n", "x".repeat(40)));
    }
    let page = lines.page_within(&lines.shown(&privacy), Some(from), 200, 8192);
    assert!(page.lines.len() <= 8192, "{}", page.lines.len());
    assert_eq!(page.first, 2);
    assert!(page.next.is_some_and(|next| next > 2 && next <= page.last), "{page:?}");
    // A line longer than the page is cut to it.
    feed(&mut lines, &privacy, &format!("{}\r\n", "y".repeat(20_000)));
    let last = lines.last_line(&lines.shown(&privacy));
    let page = lines.page_within(&lines.shown(&privacy), Some(last), 200, 8192);
    assert!(
        page.lines.len() <= 8192 && page.lines.contains("字节没有显示"),
        "{}",
        page.lines.len()
    );
}
