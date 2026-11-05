//! Text-only access avoids snapshot conversion and history-sized allocations.

use super::AlacrittyTerm;
use crate::term::text_query::TextInfo;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{cell::Flags, TermMode};

impl AlacrittyTerm {
    pub(super) fn query_info(&self) -> TextInfo {
        let grid = self.term.grid();
        TextInfo {
            generation: 0,
            first_row: grid.topmost_line().0,
            last_row: grid.screen_lines() as i32 - 1,
            cols: grid.columns() as u16,
            alt: self.term.mode().contains(TermMode::ALT_SCREEN),
        }
    }

    pub(super) fn query_row(&self, row: i32) -> (u16, bool) {
        let grid = self.term.grid();
        let cols = grid.columns();
        let wrap = grid[Point::new(Line(row), Column(cols - 1))]
            .flags
            .contains(Flags::WRAPLINE);
        if wrap {
            return (cols as u16, true);
        }
        let len = (0..cols)
            .rev()
            .find(|col| {
                let cell = &grid[Point::new(Line(row), Column(*col))];
                cell.c != ' '
                    || cell.zerowidth().is_some_and(|marks| !marks.is_empty())
                    || cell.flags.contains(Flags::WIDE_CHAR_SPACER)
            })
            .map_or(0, |col| col + 1);
        (len as u16, false)
    }

    pub(super) fn query_cell(&self, row: i32, col: u16) -> (String, u8) {
        let cell = &self.term.grid()[Point::new(Line(row), Column(usize::from(col)))];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            return (String::new(), 0);
        }
        let width = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        (
            super::cell::ExportedText::new(cell).text(),
            width,
        )
    }
}
