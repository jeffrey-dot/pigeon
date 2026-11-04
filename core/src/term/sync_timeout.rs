//! Deadline for a DEC 2026 synchronized update (`CSI ? 2026 h` … `CSI ? 2026 l`).
//!
//! vte buffers everything between BSU and ESU but has no clock of its own: whoever drives
//! the parser must flush once the deadline passes, or a program that dies (or forgets ESU)
//! mid-update freezes the pane. vte asks for 150 ms; we use 1 s like Ghostty, so a slow
//! redraw over ssh is not cut in half.

use std::time::{Duration, Instant};

use alacritty_terminal::vte::ansi::Timeout;

/// How long a synchronized update may stay open before its bytes are shown anyway.
pub const SYNC_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct SyncTimeout {
    deadline: Option<Instant>,
}

impl SyncTimeout {
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

impl Timeout for SyncTimeout {
    fn set_timeout(&mut self, _requested: Duration) {
        self.deadline = Some(Instant::now() + SYNC_TIMEOUT);
    }

    fn clear_timeout(&mut self) {
        self.deadline = None;
    }

    fn pending_timeout(&self) -> bool {
        self.deadline.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::term::{alacritty::AlacrittyTerm, Term};

    fn first_row(term: &AlacrittyTerm) -> String {
        term.screen_snapshot().screen[0]
            .iter()
            .filter_map(|cell| char::from_u32(cell.codepoint))
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn an_open_update_has_a_deadline_and_flushing_shows_it() {
        let mut term = AlacrittyTerm::new(20, 3, 0);
        term.feed(b"\x1b[?2026hhidden");
        assert!(term.sync_deadline().is_some());
        assert_eq!(first_row(&term), "");
        term.flush_sync();
        assert!(term.sync_deadline().is_none());
        assert_eq!(first_row(&term), "hidden");
    }

    #[test]
    fn a_closed_update_leaves_no_deadline() {
        let mut term = AlacrittyTerm::new(20, 3, 0);
        term.feed(b"\x1b[?2026hshown\x1b[?2026l");
        assert!(term.sync_deadline().is_none());
        assert_eq!(first_row(&term), "shown");
    }
}
