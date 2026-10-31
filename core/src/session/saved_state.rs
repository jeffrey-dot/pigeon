//! What a host leaves on disk so a restart (a reboot, a crash, an upgrade) still shows the
//! user what was on every screen. Processes cannot be saved; their screens, layouts and
//! names can. A restored pane is a picture of its last screen whose process has ended.
//!
//! The file is CBOR, versioned, and read defensively: it lives in the user's directory and
//! may be truncated by a crash mid-write or written by another build.

use serde::{Deserialize, Serialize};

use crate::error::{PgError, PgResult};
use crate::proto::{Message, Node};

/// Bumped when the layout of this file changes incompatibly; other versions are ignored.
pub const SAVED_STATE_VERSION: u32 = 1;
/// A state file larger than this is refused rather than read into memory.
pub const MAX_SAVED_STATE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedState {
    pub version: u32,
    pub sessions: Vec<SavedSession>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSession {
    pub session_id: u32,
    pub cwd: String,
    pub shell: String,
    /// The `layout.tree` as the protocol writes it.
    pub tree: Node,
    pub focus: Option<u32>,
    pub panes: Vec<SavedPane>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedPane {
    pub pane_id: u32,
    pub cwd: Option<String>,
    pub title: Option<String>,
    /// A `snapshot` message of the screen (no history: it is a reminder, not an archive).
    pub screen: Message,
}

impl SavedState {
    pub fn new(sessions: Vec<SavedSession>) -> Self {
        Self {
            version: SAVED_STATE_VERSION,
            sessions,
        }
    }

    pub fn encode(&self) -> PgResult<Vec<u8>> {
        let mut out = Vec::new();
        ciborium::into_writer(self, &mut out).map_err(|_| PgError::InvalidArg)?;
        Ok(out)
    }

    /// Reads a state file. Anything that is not a complete file of this version, or that
    /// does not hang together (a pane the tree does not name, a screen that is not a
    /// well-formed snapshot), is refused whole: half a restore is worse than none.
    pub fn decode(bytes: &[u8]) -> PgResult<Self> {
        if bytes.len() > MAX_SAVED_STATE {
            return Err(PgError::InvalidArg);
        }
        let state: Self = ciborium::from_reader(bytes).map_err(|_| PgError::InvalidArg)?;
        if state.version != SAVED_STATE_VERSION {
            return Err(PgError::Unsupported);
        }
        for session in &state.sessions {
            let mut leaves = Vec::new();
            collect_leaves(&session.tree, &mut leaves);
            let mut named: Vec<u32> = session.panes.iter().map(|pane| pane.pane_id).collect();
            leaves.sort_unstable();
            named.sort_unstable();
            if leaves.is_empty() || leaves != named {
                return Err(PgError::InvalidArg);
            }
            for pane in &session.panes {
                if crate::session::snapshot_from_message(&pane.screen).is_none() {
                    return Err(PgError::InvalidArg);
                }
            }
        }
        Ok(state)
    }
}

fn collect_leaves(node: &Node, out: &mut Vec<u32>) {
    match node {
        Node::Leaf { leaf } => out.push(*leaf),
        Node::Split { split } => {
            collect_leaves(&split.a, out);
            collect_leaves(&split.b, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Cell, Cursor, Scroll, SplitAxis, SplitNode};

    fn screen(pane_id: u32, text: &str) -> Message {
        let mut row: Vec<Cell> = text
            .chars()
            .map(|ch| Cell {
                codepoint: ch as u32,
                attrs: 0,
                fg: 0,
                bg: 0,
                link: None,
            })
            .collect();
        row.resize(
            8,
            Cell {
                codepoint: 32,
                attrs: 0,
                fg: 0,
                bg: 0,
                link: None,
            },
        );
        Message::Snapshot {
            pane_id,
            seq: 3,
            cols: 8,
            rows: 1,
            alt: false,
            cursor: Cursor {
                x: 0,
                y: 0,
                visible: true,
                shape: 0,
            },
            scroll: Scroll { top: 0, bottom: 0 },
            screen: vec![row],
            history: None,
            links: None,
            history_len: None,
            offset: None,
            modes: None,
        }
    }

    fn sample() -> SavedState {
        SavedState::new(vec![SavedSession {
            session_id: 1,
            cwd: "/w".into(),
            shell: "zsh".into(),
            tree: Node::Split {
                split: SplitNode {
                    id: 1,
                    axis: SplitAxis::Vertical,
                    ratio: 0.5,
                    a: Box::new(Node::Leaf { leaf: 1 }),
                    b: Box::new(Node::Leaf { leaf: 2 }),
                },
            },
            focus: Some(2),
            panes: vec![
                SavedPane {
                    pane_id: 1,
                    cwd: Some("/w".into()),
                    title: Some("vim".into()),
                    screen: screen(1, "left"),
                },
                SavedPane {
                    pane_id: 2,
                    cwd: None,
                    title: None,
                    screen: screen(2, "right"),
                },
            ],
        }])
    }

    #[test]
    fn a_saved_state_round_trips() {
        let state = sample();
        let bytes = state.encode().expect("encode");
        assert_eq!(SavedState::decode(&bytes), Ok(state));
    }

    #[test]
    fn a_truncated_or_foreign_file_is_refused_whole() {
        let bytes = sample().encode().expect("encode");
        assert!(SavedState::decode(&bytes[..bytes.len() / 2]).is_err());
        assert!(SavedState::decode(b"").is_err());
        assert!(SavedState::decode(b"not cbor at all").is_err());

        let mut newer = sample();
        newer.version = SAVED_STATE_VERSION + 1;
        let bytes = newer.encode().expect("encode");
        assert_eq!(SavedState::decode(&bytes), Err(PgError::Unsupported));
    }

    #[test]
    fn a_state_that_does_not_hang_together_is_refused() {
        let mut orphan = sample();
        orphan.sessions[0].panes.pop();
        let bytes = orphan.encode().expect("encode");
        assert!(SavedState::decode(&bytes).is_err(), "a leaf with no screen");

        let mut torn = sample();
        if let Message::Snapshot { rows, .. } = &mut torn.sessions[0].panes[0].screen {
            *rows = 5;
        }
        let bytes = torn.encode().expect("encode");
        assert!(SavedState::decode(&bytes).is_err(), "a screen that is not rows x cols");
    }
}
