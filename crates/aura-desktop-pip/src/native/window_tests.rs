use super::*;

#[test]
fn a_posted_window_message_cannot_forge_a_core_command() {
    PENDING.with(|pending| pending.borrow_mut().clear());
    // SAFETY: this branch only queues an input; there is no window or handler in this test.
    unsafe {
        procedure(HWND::default(), COMMAND, WPARAM(3), LPARAM(0));
    }
    assert!(PENDING.with(|pending| pending.borrow().is_empty()));
}
