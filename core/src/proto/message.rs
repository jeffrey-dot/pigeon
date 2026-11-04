//! Message schema and wire names (docs/protocol.md §3).

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use super::{Cell, CellText, Cursor, Link, Node, Row, Scroll, SessionInfo, SplitAxis};

/// Every message the M1 host and client speak. `t` carries the snake-case name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    Hello {
        proto: u16,
        client: String,
        features: Vec<String>,
    },
    HelloAck {
        proto: u16,
        server: String,
        features: Vec<String>,
        host_id: String,
        client_id: u32,
        /// Changes every time the host process starts. `seq` and pane ids start over with
        /// it, so a client may only send `attach.since` to the instance it got it from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<String>,
    },
    HelloError {
        reason: String,
        detail: String,
    },
    Sessions {
        /// Empty in a client request (`pg_msg_new(PG_MSG_SESSIONS)` + encode); the host always
        /// answers with the full list.
        #[serde(default)]
        items: Vec<SessionInfo>,
    },
    NewSession {
        id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        env: Option<BTreeMap<String, String>>,
    },
    Created {
        id: u32,
        session_id: u32,
        pane_id: u32,
    },
    ClosePane {
        id: u32,
        pane_id: u32,
    },
    SplitPane {
        id: u32,
        pane_id: u32,
        axis: SplitAxis,
        ratio: f32,
    },
    ResizeSplit {
        id: u32,
        split_id: u32,
        ratio: f32,
    },
    Focus {
        pane_id: u32,
    },
    Layout {
        session_id: u32,
        tree: Node,
        #[serde(skip_serializing_if = "Option::is_none")]
        zoom: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        focus: Option<u32>,
    },
    Attach {
        id: u32,
        pane_id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_policy: Option<String>,
        /// `false`: the snapshot leaves `history` out. A client that scrolls through the
        /// host (`scroll`) never reads it, and over ssh it is up to 2 MB per attach.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<bool>,
        /// The `seq` the client's screen of this pane already shows (it reconnects after a
        /// drop). If the host still has every delta after it, those are the answer instead
        /// of a snapshot; otherwise it falls back to a snapshot as if this were absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<u32>,
    },
    Snapshot {
        pane_id: u32,
        seq: u32,
        cols: u16,
        rows: u16,
        alt: bool,
        cursor: Cursor,
        scroll: Scroll,
        screen: Vec<Vec<Cell>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        text: Vec<CellText>,
        #[serde(skip_serializing_if = "Option::is_none")]
        history: Option<Vec<Vec<Cell>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        links: Option<Vec<Link>>,
        /// Lines of scrollback the pane holds; only sent with a `scroll` answer, so the
        /// client can draw where it is in the history (protocol §3.3).
        #[serde(skip_serializing_if = "Option::is_none")]
        history_len: Option<u32>,
        /// How far back that answer is scrolled (0 = live screen).
        #[serde(skip_serializing_if = "Option::is_none")]
        offset: Option<u32>,
        /// `term::MODE_*` bits: what the program asked of the keyboard, mouse and paste.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modes: Option<u32>,
    },
    Delta {
        pane_id: u32,
        seq: u32,
        rows: Vec<Row>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        text: Vec<CellText>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
        #[serde(skip_serializing_if = "Option::is_none")]
        scroll: Option<Scroll>,
        #[serde(skip_serializing_if = "Option::is_none")]
        links: Option<Vec<Link>>,
        /// Whether the alternate screen is up; the wheel means something else there, and
        /// without it a delta-only client would never notice `vim` starting.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<bool>,
        /// `term::MODE_*` bits, sent with every delta: `vim` turning on mouse reporting
        /// changes what the very next click must send.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modes: Option<u32>,
    },
    /// The client's current theme, so the host can answer `OSC 4/10/11` colour queries for
    /// the programs inside the panes. All fields are optional: a client that never sends this
    /// (or sends it without colours) leaves the host's behaviour exactly as it was.
    Appearance {
        #[serde(skip_serializing_if = "Option::is_none")]
        fg: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        bg: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ansi: Vec<u32>,
    },
    Keys {
        pane_id: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    /// Text from the clipboard. The host wraps it in bracketed-paste markers when the
    /// program asked for them (`?2004`), so the client never has to track that mode.
    Paste {
        pane_id: u32,
        text: String,
    },
    ResizePane {
        pane_id: u32,
        cols: u16,
        rows: u16,
    },
    /// Look back into the scrollback (protocol §3.3): a negative `offset` moves that many
    /// lines into history, `0` returns to the live screen.
    Scroll {
        pane_id: u32,
        offset: i32,
    },
    /// Zoom one pane to the whole window (protocol §3.2).
    ZoomPane {
        id: u32,
        pane_id: u32,
    },
    /// Give every pane its place in the tree back.
    Unzoom {
        id: u32,
        pane_id: u32,
    },
    /// Broadcast after every size change: who won the size policy and how many
    /// clients are watching (protocol §6).
    SizeState {
        pane_id: u32,
        cols: u16,
        rows: u16,
        source: String,
        clients: u8,
    },
    PaneExited {
        pane_id: u32,
        code: i32,
    },
    Status {
        pane_id: u32,
        state: String,
        seq: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        msg: Option<String>,
        /// Which agent reported (`app=` in OSC 7501), e.g. `claude`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        /// The record's id (`id=`); nested agents use dotted ids (`main.sub1`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Milliseconds since the record entered this state, on the host's clock. A
        /// duration rather than a timestamp, so client and server clocks need not agree.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        age_ms: Option<u64>,
    },
    AckStatus {
        pane_id: u32,
        seq: u32,
    },
    Ping {
        ts: u64,
    },
    /// Ends the host and every session in it (the window's "Stop Session Host…").
    Shutdown,
    Pong {
        ts: u64,
    },
    Error {
        code: String,
        detail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
    },
    Unsupported {
        t_of: String,
    },
}

impl Message {
    /// The value carried in `t`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::HelloAck { .. } => "hello_ack",
            Self::HelloError { .. } => "hello_error",
            Self::Sessions { .. } => "sessions",
            Self::NewSession { .. } => "new_session",
            Self::Created { .. } => "created",
            Self::ClosePane { .. } => "close_pane",
            Self::SplitPane { .. } => "split_pane",
            Self::ResizeSplit { .. } => "resize_split",
            Self::Focus { .. } => "focus",
            Self::Layout { .. } => "layout",
            Self::Attach { .. } => "attach",
            Self::Snapshot { .. } => "snapshot",
            Self::Delta { .. } => "delta",
            Self::Appearance { .. } => "appearance",
            Self::Keys { .. } => "keys",
            Self::Paste { .. } => "paste",
            Self::ResizePane { .. } => "resize_pane",
            Self::Scroll { .. } => "scroll",
            Self::ZoomPane { .. } => "zoom_pane",
            Self::Unzoom { .. } => "unzoom",
            Self::SizeState { .. } => "size_state",
            Self::PaneExited { .. } => "pane_exited",
            Self::Status { .. } => "status",
            Self::AckStatus { .. } => "ack_status",
            Self::Ping { .. } => "ping",
            Self::Shutdown => "shutdown",
            Self::Pong { .. } => "pong",
            Self::Error { .. } => "error",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}
