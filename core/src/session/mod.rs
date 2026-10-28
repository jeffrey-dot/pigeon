//! Session model: a pane's terminal, its status records, and the mapping from
//! that state to protocol messages.
//!
//! Pure data plus conversions — no IO, no tokio. The host drives this; an
//! embedded client would drive the same struct through the C ABI.

use crate::proto::{Cell as ProtoCell, Cursor as ProtoCursor, Message, Row, Scroll};
use crate::status::StatusStore;
use crate::term::{alacritty::AlacrittyTerm, frozen::FrozenTerm, Snapshot, Term};
use unicode_width::UnicodeWidthChar;

/// One pane's state.
pub struct PaneState {
    pub pane_id: u32,
    /// Monotonic per pane; shared by snapshots and deltas.
    seq: u32,
    cols: u16,
    rows: u16,
    term: Box<dyn Term + Send>,
    status: StatusStore,
}

impl PaneState {
    pub fn new(pane_id: u32, cols: u16, rows: u16, scrollback: usize) -> Self {
        Self {
            pane_id,
            seq: 0,
            cols,
            rows,
            term: Box::new(AlacrittyTerm::new(cols, rows, scrollback)),
            status: StatusStore::new(),
        }
    }

    /// Rebuilds client state from a snapshot — no byte replay, no parsing.
    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        Self {
            pane_id: 1,
            seq: 0,
            cols: snapshot.cols,
            rows: snapshot.rows,
            term: Box::new(FrozenTerm::new(snapshot)),
            status: StatusStore::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.term.feed(bytes);
        if self.status_enabled() {
            self.status.feed(bytes);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.term.resize(cols, rows);
    }

    /// Current cell size of this pane.
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        self.term.take_replies()
    }

    /// Row indices changed since the last call.
    pub fn take_damage(&mut self) -> Vec<u16> {
        self.term.take_damage()
    }

    pub fn status(&self) -> &StatusStore {
        &self.status
    }

    pub fn status_mut(&mut self) -> &mut StatusStore {
        &mut self.status
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.term.snapshot()
    }

    pub fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// Full picture for `attach`. `with_history` follows the negotiated
    /// `scrollback` capability.
    pub fn snapshot_message(&mut self, with_history: bool) -> Message {
        let snapshot = self.term.snapshot();
        let seq = self.next_seq();
        message_from_snapshot(self.pane_id, seq, &snapshot, with_history)
    }

    /// Dirty rows since the last call, or `None` when nothing changed.
    pub fn delta_message(&mut self) -> Option<Message> {
        let dirty = self.term.take_damage();
        if dirty.is_empty() {
            return None;
        }
        let snapshot = self.term.snapshot();
        let rows: Vec<Row> = dirty
            .iter()
            .filter_map(|row| {
                let y = *row;
                snapshot.screen.get(usize::from(y)).map(|cells| Row {
                    y,
                    cells: cells.iter().map(to_proto_cell).collect(),
                })
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let seq = self.next_seq();
        Some(Message::Delta {
            pane_id: self.pane_id,
            seq,
            rows,
            cursor: Some(cursor_of(&snapshot)),
            scroll: Some(scroll_of(&snapshot)),
            links: None,
        })
    }

    fn status_enabled(&self) -> bool {
        true
    }
}

fn message_from_snapshot(
    pane_id: u32,
    seq: u32,
    snapshot: &Snapshot,
    with_history: bool,
) -> Message {
    Message::Snapshot {
        pane_id,
        seq,
        cols: snapshot.cols,
        rows: snapshot.rows,
        alt: snapshot.alt,
        cursor: cursor_of(snapshot),
        scroll: scroll_of(snapshot),
        screen: snapshot
            .screen
            .iter()
            .map(|row| row.iter().map(to_proto_cell).collect())
            .collect(),
        history: with_history.then(|| {
            snapshot
                .history
                .iter()
                .map(|row| row.iter().map(to_proto_cell).collect())
                .collect()
        }),
        links: None,
    }
}

fn cursor_of(snapshot: &Snapshot) -> ProtoCursor {
    ProtoCursor {
        x: snapshot.cursor.x,
        y: snapshot.cursor.y,
        visible: snapshot.cursor.visible,
        shape: snapshot.cursor.shape,
    }
}

fn scroll_of(snapshot: &Snapshot) -> Scroll {
    Scroll {
        top: snapshot.scroll.0,
        bottom: snapshot.scroll.1,
    }
}

fn to_proto_cell(cell: &crate::term::Cell) -> ProtoCell {
    ProtoCell {
        codepoint: cell.codepoint,
        // The wire carries 8 attribute bits; ours is a superset for now.
        attrs: (cell.attrs & 0xff) as u8,
        fg: cell.fg,
        bg: cell.bg,
        link: if cell.link == 0 {
            None
        } else {
            Some(cell.link)
        },
    }
}

/// Rebuilds a terminal-domain [`Snapshot`] from a `snapshot` message.
///
/// The wire does not carry cell widths, so they are re-derived from the
/// codepoints: a wide char owns the next cell, which becomes a continuation.
pub fn snapshot_from_message(message: &Message) -> Option<Snapshot> {
    let Message::Snapshot {
        cols,
        rows,
        alt,
        cursor,
        scroll,
        screen,
        history,
        ..
    } = message
    else {
        return None;
    };
    Some(Snapshot {
        cols: *cols,
        rows: *rows,
        alt: *alt,
        cursor: crate::term::Cursor {
            x: cursor.x,
            y: cursor.y,
            visible: cursor.visible,
            shape: cursor.shape,
        },
        scroll: (scroll.top, scroll.bottom),
        screen: rows_from_proto(screen, *cols)?,
        history: match history {
            Some(rows) => rows_from_proto(rows, *cols)?,
            None => Vec::new(),
        },
    })
}

fn rows_from_proto(rows: &[Vec<ProtoCell>], cols: u16) -> Option<Vec<Vec<crate::term::Cell>>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() != usize::from(cols) {
            return None;
        }
        let mut cells = Vec::with_capacity(row.len());
        let mut continuation = false;
        for cell in row {
            let ch = char::from_u32(cell.codepoint).unwrap_or(' ');
            let width = if continuation {
                continuation = false;
                0
            } else {
                let width = ch.width().unwrap_or(1);
                continuation = width == 2;
                width.min(2) as u8
            };
            cells.push(crate::term::Cell {
                codepoint: cell.codepoint,
                fg: cell.fg,
                bg: cell.bg,
                attrs: u32::from(cell.attrs),
                link: cell.link.unwrap_or(0),
                width,
            });
        }
        out.push(cells);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{to_proto_cell, PaneState};
    use crate::proto::{Cursor as ProtoCursor, Message, Scroll};

    #[test]
    fn attach_snapshot_matches_the_terminal_grid() {
        let mut pane = PaneState::new(7, 10, 2, 0);
        pane.feed(b"hi");
        let Message::Snapshot {
            pane_id,
            cols,
            rows,
            screen,
            ..
        } = pane.snapshot_message(false)
        else {
            panic!("expected a snapshot");
        };
        assert_eq!((pane_id, cols, rows), (7, 10, 2));
        assert_eq!(screen.len(), 2);
        assert_eq!(screen[0].len(), 10);
        assert_eq!(screen[0][0].codepoint, 'h' as u32);
        assert_eq!(screen[0][1].codepoint, 'i' as u32);
    }

    /// Criterion 5: the snapshot, not the byte stream, must be enough to rebuild
    /// the screen. Cells, attributes, colours, widths, cursor, alt flag and
    /// scroll region are all compared.
    #[test]
    fn snapshot_rebuilds_every_cell() {
        let mut pane = PaneState::new(1, 24, 4, 20);
        pane.feed(b"plain \x1b[1;31mbold-red\x1b[0m\r\n");
        pane.feed("\u{4f60}\u{597d} wide\r\n".as_bytes());
        pane.feed(b"\x1b[38;2;1;2;3mfancy\x1b[0m");

        let normal = pane.snapshot();
        assert_eq!(normal.screen[1][0].width, 2, "wide char owner");
        assert_eq!(normal.screen[1][1].width, 0, "continuation cell");
        assert_eq!(normal.screen[2][0].fg, 0x0001_0203, "truecolor survived");
        assert_ne!(normal.screen[0][6].attrs, 0, "bold escaped through");

        let rebuilt = rebuild(&mut pane);
        assert_eq!(rebuilt, normal, "normal screen must rebuild cell for cell");

        // Alternate screen + cursor position are part of the picture too.
        pane.feed(b"\x1b[?1049h\x1b[3;7Halt");
        let alternate = pane.snapshot();
        assert!(alternate.alt, "alternate screen flag");
        // The rebuild comparison below covers the cursor position exactly.
        assert_eq!(alternate.cursor.y, 2, "cursor row after CUP");
        let rebuilt = rebuild(&mut pane);
        assert_eq!(rebuilt, alternate, "alternate screen must rebuild too");
    }

    fn rebuild(pane: &mut PaneState) -> crate::term::Snapshot {
        let original = pane.snapshot();
        let message = Message::Snapshot {
            pane_id: 1,
            seq: 0,
            cols: original.cols,
            rows: original.rows,
            alt: original.alt,
            cursor: ProtoCursor {
                x: original.cursor.x,
                y: original.cursor.y,
                visible: original.cursor.visible,
                shape: original.cursor.shape,
            },
            scroll: Scroll {
                top: original.scroll.0,
                bottom: original.scroll.1,
            },
            screen: original
                .screen
                .iter()
                .map(|row| row.iter().map(to_proto_cell).collect())
                .collect(),
            history: None,
            links: None,
        };
        let payload = crate::proto::encode(&message).expect("encode");
        let decoded = crate::proto::decode(&payload).expect("decode");
        super::snapshot_from_message(&decoded).expect("rebuild")
    }

    /// The rebuild path the client actually uses: a snapshot in, a snapshot out,
    /// with no byte replay. Exercises `FrozenTerm`.
    #[test]
    fn frozen_terminal_replays_the_snapshot_verbatim() {
        let mut pane = PaneState::new(1, 20, 3, 10);
        pane.feed("\x1b[1;32mgreen\x1b[0m \u{4f60}\u{597d}".as_bytes());

        let original = pane.snapshot();
        let mut frozen = PaneState::from_snapshot(original.clone());
        assert_eq!(frozen.snapshot(), original, "stored picture must survive");

        let via_wire = super::snapshot_from_message(&frozen.snapshot_message(false))
            .expect("rebuild from the message");
        assert_eq!(
            via_wire, original,
            "message rebuild equals the live snapshot"
        );
    }

    #[test]
    fn delta_reports_only_dirty_rows() {
        let mut pane = PaneState::new(1, 10, 3, 0);
        pane.feed(b"a\r\nb");
        let Some(Message::Delta { rows, .. }) = pane.delta_message() else {
            panic!("expected a delta");
        };
        assert!(rows.iter().any(|row| row.y == 0), "row 0 changed");
        assert!(rows.iter().any(|row| row.y == 1), "row 1 changed");

        // alacritty always re-damages the cursor row; nothing else may come back.
        if let Some(Message::Delta { rows, .. }) = pane.delta_message() {
            assert!(
                rows.iter().all(|row| row.y == 1),
                "only the cursor row may stay dirty"
            );
        }
    }

    #[test]
    fn sequence_numbers_are_monotonic_across_snapshot_and_delta() {
        let mut pane = PaneState::new(1, 4, 2, 0);
        let Message::Snapshot { seq: first, .. } = pane.snapshot_message(false) else {
            panic!("snapshot");
        };
        pane.feed(b"x");
        let Some(Message::Delta { seq: second, .. }) = pane.delta_message() else {
            panic!("delta");
        };
        assert!(second > first);
    }
}
