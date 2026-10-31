//! The checks Core makes on a Chrome call's own arguments before anything reaches Chrome. The
//! calls themselves are tested with a scripted model in the application tests.
#![allow(clippy::unwrap_used)]
use super::resolve::{element, web_address};

#[test]
fn an_address_is_an_http_page_and_never_chromes_own() {
    assert_eq!(web_address("shop.example/cart").unwrap(), "https://shop.example/cart");
    assert_eq!(web_address(" http://127.0.0.1:5173/ ").unwrap(), "http://127.0.0.1:5173/");
    for refused in [
        "chrome://settings",
        "chrome-extension://abcdefghijklmnopabcdefghijklmnop/page.html",
        "https://chromewebstore.google.com/detail/x",
        "file:///C:/Users",
        "javascript:alert(1)",
        "not an address",
    ] {
        let error = web_address(refused).expect_err(refused).to_string();
        assert!(error.starts_with("chrome."), "{refused}: {error}");
    }
}

#[test]
fn an_element_is_a_ref_from_a_snapshot() {
    assert_eq!(element(Some("e12".into())).unwrap().as_deref(), Some("e12"));
    assert_eq!(element(None).unwrap(), None);
    for wrong in ["12", "e", "x12", "e12a", "e123456789", "#login"] {
        assert!(element(Some(wrong.into())).is_err(), "{wrong}");
    }
}
