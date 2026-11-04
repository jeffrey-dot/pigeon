//! Owned sparse text accompanying snapshots and deltas; PgCell's stride stays stable.

use super::PgStr;
use crate::api::{Cell, PgError, PgResult};
use std::ptr;

#[repr(C)]
pub struct PgCellText {
    pub size: u32,
    pub row: i32,
    pub col: u16,
    /// Borrowed until the owning snapshot/delta is freed.
    pub text: PgStr,
}

/// Keeps every allocation owned until the containing snapshot/delta is ready to return.
pub(super) struct OwnedCellText {
    entries: Box<[PgCellText]>,
    bytes: Vec<Box<[u8]>>,
    count: u32,
}

impl OwnedCellText {
    pub(super) fn new<'a>(rows: impl IntoIterator<Item = (i32, &'a [Cell])>) -> PgResult<Self> {
        let mut entries = Vec::new();
        let mut bytes = Vec::new();
        for (row, cells) in rows {
            for (col, cell) in cells.iter().enumerate() {
                let Some(text) = &cell.extra_text else {
                    continue;
                };
                let col = u16::try_from(col).map_err(|_| PgError::InvalidArg)?;
                let text = text.as_bytes().to_vec().into_boxed_slice();
                entries.push(PgCellText {
                    size: std::mem::size_of::<PgCellText>() as u32,
                    row,
                    col,
                    text: PgStr {
                        size: std::mem::size_of::<PgStr>() as u32,
                        ptr: text.as_ptr(),
                        len: text.len(),
                    },
                });
                bytes.push(text);
            }
        }
        let count = u32::try_from(entries.len()).map_err(|_| PgError::InvalidArg)?;
        Ok(Self {
            entries: entries.into_boxed_slice(),
            bytes,
            count,
        })
    }

    pub(super) fn into_raw(self) -> (u32, *const PgCellText) {
        if self.count == 0 {
            return (0, ptr::null());
        }
        for bytes in self.bytes {
            let _ = Box::into_raw(bytes);
        }
        (self.count, Box::into_raw(self.entries).cast::<PgCellText>())
    }
}

/// # Safety
/// The table must have been allocated by OwnedCellText::into_raw and not yet reclaimed.
pub(super) unsafe fn free(count: u32, entries: *const PgCellText) {
    if count == 0 || entries.is_null() {
        return;
    }
    // SAFETY: the caller owns this exact table and count from OwnedCellText::into_raw.
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
