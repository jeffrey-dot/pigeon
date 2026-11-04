use super::*;

#[test]
fn rapid_scroll_and_bottom_only_display_the_last_answer() {
    let mut scroll = ScrollState::default();
    assert!(scroll.request(-20));
    assert!(scroll.request(-20));
    assert!(scroll.request(0));
    assert!(
        !scroll.request(0),
        "typing a run of bytes needs only one bottom request"
    );
    assert!(!scroll.reply(100, 20));
    assert!(!scroll.reply(100, 40));
    assert!(scroll.active());
    assert!(scroll.reply(100, 0));
    assert!(!scroll.active());
    assert!(
        !scroll.reply(100, 40),
        "unsolicited old answers cannot reopen history"
    );
}

#[test]
fn scroll_after_bottom_is_not_lost() {
    let mut scroll = ScrollState::default();
    scroll.request(-20);
    scroll.request(0);
    scroll.request(-3);
    assert!(!scroll.reply(100, 20));
    assert!(!scroll.reply(100, 0));
    assert!(scroll.reply(100, 3));
    assert_eq!(scroll.offset, 3);
    assert!(scroll.active());
}
