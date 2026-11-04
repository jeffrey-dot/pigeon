//! Sparse text conversion; both client interfaces validate the same coordinates.

use std::collections::HashSet;
use std::sync::Arc;

use crate::proto::{CellText, MAX_CELL_TEXT_BYTES};
use crate::term::{Cell, Snapshot};

pub fn collect<'a>(rows: impl IntoIterator<Item = (i32, &'a [Cell])>) -> Vec<CellText> {
    rows.into_iter().flat_map(|(row, cells)| {
        cells.iter().enumerate().filter_map(move |(col, cell)| {
            Some(CellText { row, col: u16::try_from(col).ok()?, text: cell.extra_text.as_ref()?.to_string() })
        })
    }).collect()
}

fn set(cell: &mut Cell, text: &str) -> Option<()> {
    if cell.width == 0 || text.len() > MAX_CELL_TEXT_BYTES
        || text.chars().any(char::is_control)
        || text.chars().next().map(u32::from) != Some(cell.codepoint)
    { return None; }
    cell.extra_text = Some(Arc::from(text));
    Some(())
}

pub fn snapshot(snapshot: &mut Snapshot, entries: &[CellText]) -> Option<()> {
    let mut seen = HashSet::new();
    for entry in entries {
        if !seen.insert((entry.row, entry.col)) { return None; }
        let row = if entry.row >= 0 {
            snapshot.screen.get_mut(entry.row as usize)?
        } else {
            let distance = usize::try_from(entry.row.unsigned_abs()).ok()?;
            let index = snapshot.history.len().checked_sub(distance)?;
            snapshot.history.get_mut(index)?
        };
        set(row.get_mut(usize::from(entry.col))?, &entry.text)?;
    }
    Some(())
}

pub fn delta(rows: &mut [(u16, Vec<Cell>)], entries: &[CellText]) -> Option<()> {
    let mut seen = HashSet::new();
    for entry in entries {
        if !seen.insert((entry.row, entry.col)) { return None; }
        let y = u16::try_from(entry.row).ok()?;
        let index = rows.binary_search_by_key(&y, |(y, _)| *y).ok()?;
        set(rows[index].1.get_mut(usize::from(entry.col))?, &entry.text)?;
    }
    Some(())
}
