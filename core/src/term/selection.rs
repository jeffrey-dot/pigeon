//! Selection over an immutable exported screen. History anchors belong to text_query.
use super::{Cell, Snapshot, ATTR_WRAP};
use crate::error::{PgError, PgResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GridPoint {
    pub row: u16,
    pub col: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionUnit {
    Cell,
    Word,
    Line,
}
#[derive(Clone, Copy, Debug)]
pub struct GridSelection {
    pub anchor: GridPoint,
    pub head: GridPoint,
    pub unit: SelectionUnit,
    pub rectangle: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSpan {
    pub row: u16,
    pub first: u16,
    pub end: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub text: String,
    pub spans: Vec<SelectionSpan>,
}

pub fn select(snapshot: &Snapshot, selection: &GridSelection) -> PgResult<Selection> {
    if snapshot.cols == 0
        || snapshot.rows == 0
        || snapshot.screen.len() != usize::from(snapshot.rows)
        || usize::from(snapshot.cols) * usize::from(snapshot.rows) > 1_000_000
        || snapshot
            .screen
            .iter()
            .any(|row| row.len() != usize::from(snapshot.cols))
        || snapshot
            .screen
            .iter()
            .flatten()
            .any(|cell| cell.width > 2 || cell.extra_text.as_ref().is_some_and(|s| s.len() > 4096))
        || [selection.anchor, selection.head]
            .iter()
            .any(|p| p.row >= snapshot.rows || p.col >= snapshot.cols)
    {
        return Err(PgError::InvalidArg);
    }
    let (mut start, mut end) = if selection.anchor <= selection.head {
        (selection.anchor, selection.head)
    } else {
        (selection.head, selection.anchor)
    };
    match selection.unit {
        SelectionUnit::Line => {
            start.col = 0;
            end.col = snapshot.cols - 1;
        }
        SelectionUnit::Word => {
            start.col = word_bounds(&snapshot.screen[usize::from(start.row)], start.col).0;
            end.col = word_bounds(&snapshot.screen[usize::from(end.row)], end.col).1;
        }
        SelectionUnit::Cell => {}
    }
    let mut result = Selection {
        text: String::new(),
        spans: Vec::new(),
    };
    for y in start.row..=end.row {
        let row = &snapshot.screen[usize::from(y)];
        let (first, last) = if selection.rectangle {
            (
                selection.anchor.col.min(selection.head.col),
                selection.anchor.col.max(selection.head.col),
            )
        } else {
            (
                if y == start.row { start.col } else { 0 },
                if y == end.row {
                    end.col
                } else {
                    snapshot.cols - 1
                },
            )
        };
        let first = owner(row, usize::from(first));
        let last = owner(row, usize::from(last));
        let exclusive = (last + usize::from(row[last].width.max(1))).min(row.len());
        result.spans.push(SelectionSpan {
            row: y,
            first: first as u16,
            end: exclusive as u16,
        });
        let begin = result.text.len();
        for cell in &row[first..exclusive] {
            let text = cell.text();
            if result.text.len().saturating_add(text.len()) > 8 * 1024 * 1024 {
                return Err(PgError::Limit);
            }
            result.text.push_str(&text);
        }
        let joins = !selection.rectangle
            && y < end.row
            && exclusive == row.len()
            && row.last().is_some_and(|cell| cell.attrs & ATTR_WRAP != 0);
        if !joins && !selection.rectangle {
            let trimmed = result.text[begin..].trim_end_matches(' ').len();
            result.text.truncate(begin + trimmed);
        }
        if y < end.row && !joins {
            if result.text.len() == 8 * 1024 * 1024 {
                return Err(PgError::Limit);
            }
            result.text.push('\n');
        }
    }
    Ok(result)
}

fn owner(row: &[Cell], mut col: usize) -> usize {
    while col > 0 && row[col].width == 0 {
        col -= 1;
    }
    col
}
fn word_bounds(row: &[Cell], col: u16) -> (u16, u16) {
    let mut first = owner(row, usize::from(col));
    let mut last = first;
    let word = |cell: &Cell| {
        char::from_u32(cell.codepoint)
            .is_some_and(|ch| ch.is_alphanumeric() || "_-./~:@%+=?&#".contains(ch))
    };
    if word(&row[first]) {
        while first > 0 {
            let previous = owner(row, first - 1);
            if !word(&row[previous]) {
                break;
            }
            first = previous;
        }
        while last + usize::from(row[last].width.max(1)) < row.len() {
            let next = last + usize::from(row[last].width.max(1));
            if !word(&row[next]) {
                break;
            }
            last = next;
        }
    }
    (
        first as u16,
        (last + usize::from(row[last].width.max(1)) - 1).min(row.len() - 1) as u16,
    )
}
