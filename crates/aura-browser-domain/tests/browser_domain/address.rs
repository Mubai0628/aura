//! What a model's `browser.open` url opens: the address bar's reading of the same text.
use aura_browser_domain::{browser_address, local_host, this_computer};

#[test]
fn opens_a_full_address_a_host_and_localhost_over_http() {
    let cases = [
        (" https://Example.com/a?b ", "https://example.com/a?b"),
        ("example.com/docs", "https://example.com/docs"),
        ("localhost:5173", "http://localhost:5173/"),
        ("127.0.0.1:8000/game.html", "http://127.0.0.1:8000/game.html"),
        ("192.168.1.20", "http://192.168.1.20/"),
        ("[::1]:3000", "http://[::1]:3000/"),
        ("about:blank", "about:blank"),
    ];
    for (text, url) in cases {
        assert_eq!(browser_address(text).as_deref(), Some(url), "{text}");
    }
}

#[test]
fn refuses_what_is_not_a_web_address() {
    for text in [
        "",
        "two words",
        "javascript:alert(1)",
        "file:///C:/x",
        "chrome://gpu",
        "foo",
        "data:text/html,x",
    ] {
        assert_eq!(browser_address(text), None, "{text}");
    }
}

#[test]
fn tells_this_computer_and_the_local_network_from_the_internet() {
    for host in [
        "localhost",
        "app.localhost",
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "[::1]",
        "[fd00::1]",
        "[fe80::1]",
    ] {
        assert!(local_host(host), "{host}");
    }
    for host in ["example.com", "8.8.8.8", "172.32.0.1", "[2606:4700::1111]"] {
        assert!(!local_host(host), "{host}");
    }
}

#[test]
fn this_computer_is_loopback_only_not_the_local_network() {
    for host in
        ["localhost", "game.localhost", "127.0.0.1", "127.8.0.1", "[::1]", "[::ffff:127.0.0.1]"]
    {
        assert!(this_computer(host), "{host}");
    }
    for host in
        ["192.168.1.1", "10.0.0.1", "[fd00::1]", "0.0.0.0", "example.com", "localhost.example"]
    {
        assert!(!this_computer(host), "{host}");
    }
}
