//! Server-sent events as Streamable HTTP servers send them: split anywhere, any line ending,
//! comments and other event types ignored, and a size limit.
use aura_mcp::{SseEvent, SseParser, SseTooLarge};

fn all(parser: &mut SseParser, chunks: &[&[u8]]) -> Vec<SseEvent> {
    chunks.iter().flat_map(|chunk| parser.feed(chunk).expect("within the limit")).collect()
}

#[test]
fn events_are_found_however_the_bytes_arrive() {
    let stream = b": keep-alive\r\nid: 7\r\ndata: {\"jsonrpc\":\"2.0\",\r\ndata: \"id\":1}\r\n\r\nevent: ping\ndata: x\n\ndata:{}\r\r";
    let whole = all(&mut SseParser::new(1024), &[stream]);
    let bytewise: Vec<&[u8]> = stream.chunks(1).collect();
    assert_eq!(all(&mut SseParser::new(1024), &bytewise), whole);
    assert_eq!(whole.len(), 3);
    assert_eq!(whole[0].data, "{\"jsonrpc\":\"2.0\",\n\"id\":1}");
    assert_eq!(whole[0].id.as_deref(), Some("7"));
    assert!(whole[0].is_message());
    assert_eq!(whole[1].event.as_deref(), Some("ping"));
    assert!(!whole[1].is_message(), "another event type carries no JSON-RPC");
    assert_eq!(whole[2].data, "{}", "a field without a space after the colon");
}

#[test]
fn an_event_without_data_or_a_blank_line_is_not_dispatched() {
    let mut parser = SseParser::new(1024);
    assert!(parser.feed(b"event: nothing\n\n").expect("ok").is_empty());
    assert!(parser.feed(b"data: unfinished\n").expect("ok").is_empty());
    assert_eq!(parser.feed(b"\n").expect("ok")[0].data, "unfinished");
    let empty_first = parser.feed(b"data:\ndata: second\n\n").expect("ok");
    assert_eq!(empty_first[0].data, "\nsecond");
}

#[test]
fn a_line_or_an_event_over_the_limit_is_refused() {
    assert_eq!(SseParser::new(8).feed(b"data: 0123456789\n"), Err(SseTooLarge(8)));
    let mut parser = SseParser::new(12);
    assert_eq!(parser.feed(b"data: 1234567\ndata: 1234567\n"), Err(SseTooLarge(12)));
}
