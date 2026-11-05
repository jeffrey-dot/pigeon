use pigeon_core::{api, ffi::*};
use std::mem::size_of;

fn text(value: &str) -> PgStr {
    PgStr {
        size: size_of::<PgStr>() as u32,
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

#[test]
fn environment_builder_matches_safe_api_and_keeps_failed_update_atomic() {
    let message = pg_msg_new(PgMsgType::PG_MSG_NEW_SESSION);
    assert!(pg_msg_set_u32(message, c"id".as_ptr(), 3));
    assert!(pg_msg_set_session_env(message, text("X"), text("你好")));
    assert!(!pg_msg_set_session_env(message, text("TERM"), text("dumb")));
    assert!(!pg_msg_set_session_env(
        std::ptr::null_mut(),
        text("X"),
        text("Y")
    ));
    let mut frame = vec![0; pg_msg_encoded_len(message)];
    assert_eq!(
        pg_msg_encode_frame(message, frame.as_mut_ptr(), frame.len()),
        frame.len()
    );
    pg_msg_free(message);
    let decoded = api::decode_frame(&frame).unwrap().unwrap().0;
    let mut expected = api::Message::NewSession {
        id: 3,
        cwd: None,
        shell: None,
        env: None,
    };
    api::set_session_env(&mut expected, "X", "你好").unwrap();
    assert_eq!(decoded, expected);
}
