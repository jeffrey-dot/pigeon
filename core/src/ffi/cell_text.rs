//! Owned sparse text accompanying snapshots and deltas; PgCell's stride stays stable.

use super::PgStr;
use crate::api::Cell;
use std::ptr;

#[repr(C)]
pub struct PgCellText {
    pub size: u32,
    pub row: i32,
    pub col: u16,
    /// Borrowed until the owning snapshot/delta is freed.
    pub text: PgStr,
}

pub(super) fn export<'a>(
    rows: impl IntoIterator<Item = (i32, &'a [Cell])>,
) -> (u32, *const PgCellText) {
    let entries: Vec<_> = rows
        .into_iter()
        .flat_map(|(row, cells)| {
            cells.iter().enumerate().filter_map(move |(col, cell)| {
                let bytes = cell
                    .extra_text
                    .as_ref()?
                    .as_bytes()
                    .to_vec()
                    .into_boxed_slice();
                let len = bytes.len();
                let col = u16::try_from(col).ok()?;
                Some(PgCellText {
                    size: std::mem::size_of::<PgCellText>() as u32,
                    row,
                    col,
                    text: PgStr {
                        size: std::mem::size_of::<PgStr>() as u32,
                        ptr: Box::into_raw(bytes).cast::<u8>(),
                        len,
                    },
                })
            })
        })
        .collect();
    if entries.is_empty() {
        return (0, ptr::null());
    }
    let count = entries.len() as u32;
    (
        count,
        Box::into_raw(entries.into_boxed_slice()).cast::<PgCellText>(),
    )
}

/// # Safety
/// The table must have been allocated by export and not yet reclaimed.
pub(super) unsafe fn free(count: u32, entries: *const PgCellText) {
    if count == 0 || entries.is_null() {
        return;
    }
    // SAFETY: the caller owns this exact table and count from export.
    let entries = unsafe {
        Box::from_raw(ptr::slice_from_raw_parts_mut(
            entries.cast_mut(),
            count as usize,
        ))
    };
    for entry in entries.iter() {
        // SAFETY: every string was exported as a boxed byte slice with this length.
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                entry.text.ptr.cast_mut(),
                entry.text.len,
            )));
        }
    }
}
