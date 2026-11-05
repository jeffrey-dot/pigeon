#[cfg(feature = "internals")]
use pigeon_core::api::encode_frame;
use pigeon_core::{
    api::{Config, Core},
    ffi::*,
};

#[test]
fn link_table_outlives_core_and_cell_stride_stays_compatible() {
    let core = pg_core_new(std::ptr::null());
    let pane = pg_pane_new(core, std::ptr::null());
    let input = b"\x1b]8;;https://example.com\x07hi\x1b]8;;\x07";
    assert!(pg_pane_feed(core, pane, input.as_ptr(), input.len()));
    let snapshot = pg_pane_snapshot(core, pane);
    assert!(!snapshot.is_null());
    pg_core_free(core);
    unsafe {
        let snapshot_ref = &*snapshot;
        assert_eq!(std::mem::size_of::<PgCell>(), 28);
        assert_eq!(snapshot_ref.link_count, 1);
        let entry = &*snapshot_ref.links;
        let row = &*snapshot_ref.screen;
        assert_eq!((*row.cells).hyperlink_id, entry.id);
        assert_eq!((*row.cells.add(1)).hyperlink_id, entry.id);
        assert_eq!((*row.cells.add(2)).hyperlink_id, 0);
        assert_eq!(
            std::slice::from_raw_parts(entry.uri.ptr, entry.uri.len),
            b"https://example.com"
        );
    }
    pg_snapshot_free(snapshot);
}

#[cfg(feature = "internals")]
#[test]
fn decoded_snapshot_and_delta_own_links_after_message_free() {
    use pigeon_core::session::PaneState;
    let mut pane = PaneState::new(1, 8, 2, 10);
    pane.feed(b"\x1b]8;;https://example.com\x07ok\x1b]8;;\x07");
    for message in [pane.snapshot_message(true), pane.delta_message().unwrap()] {
        let bytes = encode_frame(&message).unwrap();
        let mut consumed = 0;
        let msg = pg_msg_decode_frame(bytes.as_ptr(), bytes.len(), &mut consumed);
        assert!(!msg.is_null());
        assert_eq!(consumed, bytes.len());
        let snapshot = pg_msg_snapshot(msg);
        let delta = if snapshot.is_null() {
            pg_msg_delta(msg)
        } else {
            std::ptr::null_mut()
        };
        pg_msg_free(msg);
        unsafe {
            let (count, links) = if !snapshot.is_null() {
                ((*snapshot).link_count, (*snapshot).links)
            } else {
                assert!(!delta.is_null());
                ((*delta).link_count, (*delta).links)
            };
            assert_eq!(count, 1);
            assert_eq!(
                std::slice::from_raw_parts((*links).uri.ptr, (*links).uri.len),
                b"https://example.com"
            );
        }
        pg_snapshot_free(snapshot);
        pg_delta_free(delta);
    }
}

#[test]
fn rust_api_exports_the_same_owned_link_table() {
    let mut core = Core::new(Config::default());
    let pane = core.new_pane(8, 2).unwrap();
    core.feed(pane, b"\x1b]8;;https://example.com\x07hi")
        .unwrap();
    let snap = core.snapshot(pane).unwrap();
    let links = pigeon_core::api::cell_links(snap.screen.iter().flatten());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].id, snap.screen[0][0].link);
    assert_eq!(links[0].uri, "https://example.com");
}
