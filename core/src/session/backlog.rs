//! Recent `delta` frames of one pane, so a client that drops off for a moment (sleep, Wi-Fi
//! switch, ssh hiccup) is sent only what it missed instead of a whole screen (issue #4,
//! protocol §3.3 `attach.since`).

use std::collections::VecDeque;

use crate::proto::Message;

/// Frames kept at most. A burst of output coalesces into one delta per 16 ms, so this is
/// about a minute of continuous output; anything older is cheaper as one snapshot anyway.
const MAX_FRAMES: usize = 4096;
/// Cells kept at most across all frames (~13 bytes each on the wire): the replay must
/// stay smaller than the snapshot it replaces, and the memory per pane bounded.
const MAX_CELLS: usize = 512 * 1024;

#[derive(Default)]
pub struct Backlog {
    frames: VecDeque<(u32, usize, Message)>,
    cells: usize,
}

impl Backlog {
    /// Remembers a delta just sent out; `seq` must be one more than the previous one.
    pub fn push(&mut self, seq: u32, message: &Message) {
        let cells = match message {
            Message::Delta { rows, .. } => rows.iter().map(|row| row.cells.len()).sum(),
            _ => return,
        };
        if self
            .frames
            .back()
            .is_some_and(|(last, _, _)| last.wrapping_add(1) != seq)
        {
            self.clear();
        }
        self.frames.push_back((seq, cells, message.clone()));
        self.cells += cells;
        while self.frames.len() > MAX_FRAMES || self.cells > MAX_CELLS {
            let Some((_, dropped, _)) = self.frames.pop_front() else {
                break;
            };
            self.cells -= dropped;
        }
    }

    /// Forgets everything: after a resize old rows no longer fit the grid.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.cells = 0;
    }

    /// The frames after `since` up to `current`, or `None` when some of them are gone
    /// (or `since` is from the future, i.e. another host instance) and only a snapshot
    /// can bring the client up to date.
    pub fn since(&self, since: u32, current: u32) -> Option<Vec<Message>> {
        if since == current {
            return Some(Vec::new());
        }
        if since > current {
            return None;
        }
        let first = self.frames.front()?.0;
        if since.checked_add(1)? < first {
            return None;
        }
        let missed: Vec<Message> = self
            .frames
            .iter()
            .filter(|(seq, _, _)| *seq > since)
            .map(|(_, _, message)| message.clone())
            .collect();
        // Contiguous by construction (`push` clears on a gap); the count proves it.
        (missed.len() == (current - since) as usize).then_some(missed)
    }
}

#[cfg(test)]
mod tests {
    use super::Backlog;
    use crate::proto::{Cell, Message, Row};

    fn delta(seq: u32, cols: usize) -> Message {
        Message::Delta {
            pane_id: 1,
            seq,
            rows: vec![Row {
                y: 0,
                cells: vec![
                    Cell {
                        codepoint: 32,
                        attrs: 0,
                        fg: 0,
                        bg: 0,
                        link: None,
                    };
                    cols
                ],
            }],
            cursor: None,
            scroll: None,
            links: None,
            alt: None,
            modes: None,
        }
    }

    fn seqs(frames: &[Message]) -> Vec<u32> {
        frames
            .iter()
            .filter_map(|frame| match frame {
                Message::Delta { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_short_gap_is_replayed_frame_by_frame() {
        let mut backlog = Backlog::default();
        for seq in 1..=5 {
            backlog.push(seq, &delta(seq, 80));
        }
        assert_eq!(backlog.since(2, 5).map(|f| seqs(&f)), Some(vec![3, 4, 5]));
        assert_eq!(
            backlog.since(5, 5).map(|f| seqs(&f)),
            Some(vec![]),
            "nothing missed"
        );
        assert_eq!(
            backlog.since(0, 5).map(|f| seqs(&f)),
            Some(vec![1, 2, 3, 4, 5])
        );
    }

    #[test]
    fn evicted_frames_or_a_foreign_seq_mean_a_snapshot() {
        let mut backlog = Backlog::default();
        // 512 Ki cells at 200 per frame keeps the last ~2621 frames.
        for seq in 1..=3000 {
            backlog.push(seq, &delta(seq, 200));
        }
        assert!(backlog.since(10, 3000).is_none(), "frame 11 is long gone");
        assert!(backlog.since(2990, 3000).is_some());
        assert!(
            backlog.since(4000, 3000).is_none(),
            "a seq from another host instance"
        );
    }

    #[test]
    fn a_cleared_backlog_only_answers_an_up_to_date_client() {
        let mut backlog = Backlog::default();
        for seq in 1..=3 {
            backlog.push(seq, &delta(seq, 80));
        }
        backlog.clear();
        assert!(
            backlog.since(2, 3).is_none(),
            "the rows before a resize do not fit"
        );
        assert!(backlog.since(3, 3).is_some());
    }

    #[test]
    fn a_gap_in_what_is_pushed_drops_the_older_frames() {
        let mut backlog = Backlog::default();
        backlog.push(1, &delta(1, 80));
        backlog.push(2, &delta(2, 80));
        backlog.push(5, &delta(5, 80));
        assert!(backlog.since(1, 5).is_none(), "3 and 4 were never kept");
        assert_eq!(backlog.since(4, 5).map(|f| seqs(&f)), Some(vec![5]));
    }
}
