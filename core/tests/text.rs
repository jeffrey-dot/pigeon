//! Text fidelity at the safe client interface, including logical line boundaries.

use pigeon_core::api::{Config, Core, ATTR_WRAP};

#[test]
fn automatic_wrap_is_distinct_from_an_explicit_newline() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(4, 3).expect("pane");
    core.feed(pane, b"abcdefgh\r\nxy").expect("feed");
    let screen = core.snapshot(pane).expect("snapshot");
    assert_ne!(
        screen.screen[0][3].attrs & ATTR_WRAP,
        0,
        "abcd continues into efgh"
    );
    assert_eq!(
        screen.screen[1][3].attrs & ATTR_WRAP,
        0,
        "the CRLF ends the logical line"
    );
}

#[cfg(feature = "internals")]
#[test]
fn wrap_metadata_survives_snapshot_delta_history_and_c_abi() {
    use pigeon_core::api::{decode_frame, delta_of, encode_frame, snapshot_of};
    use pigeon_core::ffi::{pg_msg_decode_frame, pg_msg_free, pg_msg_snapshot, pg_snapshot_free};
    use pigeon_core::session::PaneState;

    let mut pane = PaneState::new(7, 4, 3, 20);
    pane.feed(b"abcdefgh");
    let delta = pane.delta_message().expect("delta");
    let frame = encode_frame(&delta).expect("encode delta");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    let (_, decoded) = delta_of(&decoded).expect("delta picture");
    assert_ne!(decoded.rows[0].1[3].attrs & ATTR_WRAP, 0);

    let snapshot = pane.snapshot_message(false);
    let frame = encode_frame(&snapshot).expect("encode snapshot");
    let (decoded, _) = decode_frame(&frame).expect("decode").expect("complete");
    assert_ne!(
        snapshot_of(&decoded).expect("picture").1.screen[0][3].attrs & ATTR_WRAP,
        0
    );
    let mut used = 0;
    let message = pg_msg_decode_frame(frame.as_ptr(), frame.len(), &mut used);
    let picture = pg_msg_snapshot(message);
    assert!(!picture.is_null());
    // SAFETY: the live snapshot owns a 4-column screen and is freed after this read.
    assert_ne!(
        unsafe { (*(*picture).screen).cells.add(3).read().attrs } & ATTR_WRAP,
        0
    );
    pg_snapshot_free(picture);
    pg_msg_free(message);

    pane.feed(b"\r\nx\r\ny\r\nz");
    let (scrolled, _) = pane.scroll_message(3);
    let (_, history) = snapshot_of(&scrolled).expect("scrolled picture");
    assert_ne!(history.screen[0][3].attrs & ATTR_WRAP, 0);
}
