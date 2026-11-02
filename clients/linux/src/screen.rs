//! The one pane this client shows, as the host describes it.
//!
//! The screen is only ever built from `snapshot` messages: the client does not negotiate
//! `delta` (protocol §5), so a `delta` arriving at all means the connection is out of step
//! and the answer is to `attach` again rather than to re-implement width derivation here
//! (`docs/protocol.md` §3.3).

use pigeon_core::api::{snapshot_of, Message, PaneId, Snapshot};

pub struct Screen {
    pub pane_id: PaneId,
    pub snapshot: Snapshot,
    /// `term::MODE_*` bits from the message; only the app-cursor bit is used so far.
    pub modes: u32,
    pub seq: u32,
    /// Set once the program in the pane exited; the screen stays as it was.
    pub exited: Option<i32>,
}

impl Screen {
    /// The pane and screen carried by a `snapshot`; `None` for any other message.
    pub fn from_message(message: &Message) -> Option<Self> {
        let (pane_id, snapshot) = snapshot_of(message).ok()?;
        let (seq, modes) = match message {
            Message::Snapshot { seq, modes, .. } => (*seq, modes.unwrap_or(0)),
            _ => (0, 0),
        };
        Some(Self {
            pane_id,
            snapshot,
            modes,
            seq,
            exited: None,
        })
    }

    /// `false` means "the screen is no longer trustworthy": the caller should drop this
    /// pane's `seq` bookkeeping and `attach` again (protocol §7).
    pub fn apply(&mut self, message: &Message) -> bool {
        match message {
            Message::Snapshot { seq, modes, .. } => match snapshot_of(message) {
                Ok((_, snapshot)) => {
                    self.snapshot = snapshot;
                    self.seq = *seq;
                    self.modes = modes.unwrap_or(0);
                    true
                }
                Err(_) => false,
            },
            Message::PaneExited { code, .. } => {
                self.exited = Some(*code);
                true
            }
            // Not negotiated, so this is a host that does not match our `hello`: re-attach.
            Message::Delta { .. } => false,
            _ => true,
        }
    }

    pub fn cols(&self) -> usize {
        usize::from(self.snapshot.cols)
    }

    pub fn rows(&self) -> usize {
        usize::from(self.snapshot.rows)
    }

    /// DECCKM (`?1h`): unmodified arrows go out as `SS3` (protocol §3.3, `modes` bit 0).
    pub fn app_cursor(&self) -> bool {
        self.modes & 1 != 0
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<&pigeon_core::api::Cell> {
        self.snapshot.screen.get(y).and_then(|row| row.get(x))
    }
}
