//! Bounded text traversal and literal matching, independent of the VT engine.

use super::{text_query::*, Term};
use crate::error::{PgError, PgResult};
use std::collections::VecDeque;

pub(crate) fn query(term: &dyn Term, info: TextInfo, query: &TextQuery) -> PgResult<TextResult> {
    query.validate()?;
    let generation = match query {
        TextQuery::Info => return Ok(TextResult::Info { info }),
        TextQuery::Search { generation, .. }
        | TextQuery::Range { generation, .. }
        | TextQuery::Viewport { generation, .. } => *generation,
    };
    if generation != 0 && generation != info.generation {
        return Err(PgError::StaleText);
    }
    match query {
        TextQuery::Search {
            pattern,
            case_sensitive,
            max_results,
            start,
            ..
        } => search(term, info, pattern, *case_sensitive, *max_results, *start),
        TextQuery::Range { range, .. } => extract(term, info, *range),
        TextQuery::Viewport { .. } => Err(PgError::Unsupported),
        TextQuery::Info => Ok(TextResult::Info { info }),
    }
}

pub(crate) fn valid(term: &dyn Term, info: TextInfo, point: TextPoint) -> PgResult<()> {
    if point.row < info.first_row || point.row > info.last_row || point.col > info.cols {
        return Err(PgError::InvalidArg);
    }
    if point.col == info.cols {
        return if point.byte == 0 {
            Ok(())
        } else {
            Err(PgError::InvalidArg)
        };
    }
    let (text, _) = term.text_cell(point.row, point.col);
    if !text.is_char_boundary(usize::from(point.byte)) {
        return Err(PgError::InvalidArg);
    }
    Ok(())
}

struct Stream<'a> {
    term: &'a dyn Term,
    info: TextInfo,
    point: TextPoint,
    row: Option<(i32, u16, bool)>,
    cell: Option<(i32, u16, String, u8)>,
    cells_left: usize,
    rows_left: usize,
    bytes_left: usize,
    finished: bool,
}

impl<'a> Stream<'a> {
    fn new(term: &'a dyn Term, info: TextInfo, point: TextPoint) -> Self {
        Self {
            term,
            info,
            point,
            row: None,
            cell: None,
            cells_left: MAX_SCAN_CELLS,
            rows_left: MAX_SCAN_ROWS,
            bytes_left: MAX_TEXT_BYTES,
            finished: false,
        }
    }

    /// None with !finished is a budget boundary, never an implicit EOF.
    fn next(&mut self, end: Option<TextPoint>) -> Option<(char, TextRange)> {
        loop {
            if self.point.row > self.info.last_row || end.is_some_and(|end| self.point >= end) {
                self.finished = true;
                return None;
            }
            if self.row.is_none_or(|(row, _, _)| row != self.point.row) {
                if self.rows_left == 0 || self.cells_left < usize::from(self.info.cols) {
                    return None;
                }
                self.rows_left -= 1;
                self.cells_left -= usize::from(self.info.cols);
                let (len, wrap) = self.term.text_row(self.point.row);
                self.row = Some((self.point.row, len, wrap));
            }
            let (_, len, wrap) = self.row?;
            if self.point.col >= len {
                if self.point.row == self.info.last_row {
                    self.finished = true;
                    return None;
                }
                let start = TextPoint {
                    row: self.point.row,
                    col: self.info.cols,
                    byte: 0,
                };
                if end.is_some_and(|end| start >= end) {
                    self.finished = true;
                    return None;
                }
                if !wrap && self.bytes_left == 0 {
                    return None;
                }
                self.point = TextPoint {
                    row: self.point.row + 1,
                    col: 0,
                    byte: 0,
                };
                if !wrap {
                    self.bytes_left -= 1;
                    return Some((
                        '\n',
                        TextRange {
                            start,
                            end: self.point,
                        },
                    ));
                }
                continue;
            }
            if self
                .cell
                .as_ref()
                .is_none_or(|(row, col, _, _)| (*row, *col) != (self.point.row, self.point.col))
            {
                if self.cells_left == 0 {
                    return None;
                }
                self.cells_left -= 1;
                let (text, width) = self.term.text_cell(self.point.row, self.point.col);
                self.cell = Some((self.point.row, self.point.col, text, width));
            }
            let (_, _, text, width) = self.cell.as_ref()?;
            let Some(ch) = text.get(usize::from(self.point.byte)..)?.chars().next() else {
                self.point.col = self
                    .point
                    .col
                    .saturating_add(u16::from((*width).max(1)))
                    .min(self.info.cols);
                self.point.byte = 0;
                continue;
            };
            if ch.len_utf8() > self.bytes_left {
                return None;
            }
            self.bytes_left -= ch.len_utf8();
            let start = self.point;
            self.point.byte += ch.len_utf8() as u16;
            let end = self.point;
            return Some((ch, TextRange { start, end }));
        }
    }
}

fn extract(term: &dyn Term, info: TextInfo, range: TextRange) -> PgResult<TextResult> {
    valid(term, info, range.start)?;
    valid(term, info, range.end)?;
    let mut stream = Stream::new(term, info, range.start);
    let mut text = String::new();
    while let Some((ch, _)) = stream.next(Some(range.end)) {
        text.push(ch);
    }
    let next = (!stream.finished).then_some(stream.point);
    if next == Some(range.start) {
        return Err(PgError::Limit);
    }
    Ok(TextResult::Range { info, text, next })
}

fn search(
    term: &dyn Term,
    info: TextInfo,
    pattern: &str,
    case_sensitive: bool,
    max_results: u16,
    start: Option<TextPoint>,
) -> PgResult<TextResult> {
    let fold = |ch: char| {
        if case_sensitive {
            ch
        } else {
            ch.to_ascii_lowercase()
        }
    };
    let needle: Vec<char> = pattern.chars().map(fold).collect();
    // KMP makes repeated prefixes linear, including adversarial patterns and cells.
    let mut prefix = vec![0; needle.len()];
    for index in 1..needle.len() {
        let mut count = prefix[index - 1];
        while count > 0 && needle[index] != needle[count] {
            count = prefix[count - 1];
        }
        if needle[index] == needle[count] {
            count += 1;
        }
        prefix[index] = count;
    }
    let start = start.unwrap_or(TextPoint {
        row: info.first_row,
        col: 0,
        byte: 0,
    });
    valid(term, info, start)?;
    let mut stream = Stream::new(term, info, start);
    let mut recent = VecDeque::with_capacity(needle.len());
    let mut matched = 0;
    let mut hits = Vec::new();
    while let Some((ch, range)) = stream.next(None) {
        recent.push_back(range);
        if recent.len() > needle.len() {
            recent.pop_front();
        }
        let ch = fold(ch);
        while matched > 0 && ch != needle[matched] {
            matched = prefix[matched - 1];
        }
        if ch == needle[matched] {
            matched += 1;
        }
        if matched == needle.len() {
            if let Some(first) = recent.front() {
                hits.push(TextRange {
                    start: first.start,
                    end: range.end,
                });
            }
            matched = prefix[matched - 1];
            if hits.len() == usize::from(max_results) {
                break;
            }
        }
    }
    // Re-read only the unresolved prefix on the next page; no server cursor to cancel.
    let next = if stream.finished {
        None
    } else if matched > 0 {
        recent.get(recent.len() - matched).map(|range| range.start)
    } else {
        Some(stream.point)
    };
    if next == Some(start) {
        return Err(PgError::Limit);
    }
    Ok(TextResult::Search { info, hits, next })
}
