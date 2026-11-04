/// Message kinds from `docs/protocol.md` §3.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgMsgType {
    PG_MSG_UNKNOWN = 0,
    PG_MSG_HELLO = 1,
    PG_MSG_HELLO_ACK = 2,
    PG_MSG_HELLO_ERROR = 3,
    PG_MSG_SESSIONS = 4,
    PG_MSG_NEW_SESSION = 5,
    PG_MSG_CREATED = 6,
    PG_MSG_CLOSE_PANE = 7,
    PG_MSG_ATTACH = 8,
    PG_MSG_SNAPSHOT = 9,
    PG_MSG_DELTA = 10,
    PG_MSG_KEYS = 11,
    PG_MSG_RESIZE_PANE = 12,
    PG_MSG_PANE_EXITED = 13,
    PG_MSG_PING = 14,
    PG_MSG_PONG = 15,
    PG_MSG_ERROR = 16,
    PG_MSG_UNSUPPORTED = 17,
    PG_MSG_SPLIT_PANE = 18,
    PG_MSG_RESIZE_SPLIT = 19,
    PG_MSG_FOCUS = 20,
    PG_MSG_LAYOUT = 21,
    PG_MSG_STATUS = 22,
    PG_MSG_ACK_STATUS = 23,
    PG_MSG_APPEARANCE = 24,
    PG_MSG_SIZE_STATE = 25,
    PG_MSG_ZOOM_PANE = 26,
    PG_MSG_UNZOOM = 27,
    PG_MSG_SCROLL = 28,
    PG_MSG_PASTE = 29,
    PG_MSG_SHUTDOWN = 30,
    PG_MSG_STATUS_SNAPSHOT = 31,
}

/// Wire name ↔ enum. Kept in one place so the two can never drift.
const NAMES: &[(PgMsgType, &str)] = &[
    (PgMsgType::PG_MSG_HELLO, "hello"),
    (PgMsgType::PG_MSG_HELLO_ACK, "hello_ack"),
    (PgMsgType::PG_MSG_HELLO_ERROR, "hello_error"),
    (PgMsgType::PG_MSG_SESSIONS, "sessions"),
    (PgMsgType::PG_MSG_NEW_SESSION, "new_session"),
    (PgMsgType::PG_MSG_CREATED, "created"),
    (PgMsgType::PG_MSG_CLOSE_PANE, "close_pane"),
    (PgMsgType::PG_MSG_ATTACH, "attach"),
    (PgMsgType::PG_MSG_SNAPSHOT, "snapshot"),
    (PgMsgType::PG_MSG_DELTA, "delta"),
    (PgMsgType::PG_MSG_KEYS, "keys"),
    (PgMsgType::PG_MSG_RESIZE_PANE, "resize_pane"),
    (PgMsgType::PG_MSG_PANE_EXITED, "pane_exited"),
    (PgMsgType::PG_MSG_PING, "ping"),
    (PgMsgType::PG_MSG_PONG, "pong"),
    (PgMsgType::PG_MSG_ERROR, "error"),
    (PgMsgType::PG_MSG_UNSUPPORTED, "unsupported"),
    (PgMsgType::PG_MSG_SPLIT_PANE, "split_pane"),
    (PgMsgType::PG_MSG_RESIZE_SPLIT, "resize_split"),
    (PgMsgType::PG_MSG_FOCUS, "focus"),
    (PgMsgType::PG_MSG_LAYOUT, "layout"),
    (PgMsgType::PG_MSG_STATUS, "status"),
    (PgMsgType::PG_MSG_ACK_STATUS, "ack_status"),
    (PgMsgType::PG_MSG_APPEARANCE, "appearance"),
    (PgMsgType::PG_MSG_SIZE_STATE, "size_state"),
    (PgMsgType::PG_MSG_ZOOM_PANE, "zoom_pane"),
    (PgMsgType::PG_MSG_UNZOOM, "unzoom"),
    (PgMsgType::PG_MSG_SCROLL, "scroll"),
    (PgMsgType::PG_MSG_PASTE, "paste"),
    (PgMsgType::PG_MSG_SHUTDOWN, "shutdown"),
    (PgMsgType::PG_MSG_STATUS_SNAPSHOT, "status_snapshot"),
];

pub(super) fn name_of(msg_type: PgMsgType) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(candidate, _)| *candidate == msg_type)
        .map(|(_, name)| *name)
}

pub(super) fn type_of(name: &str) -> PgMsgType {
    NAMES
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map_or(PgMsgType::PG_MSG_UNKNOWN, |(kind, _)| *kind)
}
