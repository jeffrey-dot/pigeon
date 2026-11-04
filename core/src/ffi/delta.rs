//! `delta` frames across the C ABI: the dirty rows of a pane, decoded by the core so the
//! client patches its grid without knowing CBOR (protocol §3.3, `docs/core.md`).
//!
//! A full `snapshot` per keystroke is ~100 KB on a 200×50 pane; a delta is the one or two
//! rows that changed (~2 KB). Over ssh that difference is the typing latency.

use std::ptr;

use super::msg::msg_ref;
use super::{ffi_call, to_pg_cell, PgCell, PgCursor};
use crate::api::{delta_of, Cursor, PgError};

/// One changed row of a `delta`.
#[repr(C)]
pub struct PgDeltaRow {
    pub size: u32,
    /// Screen row this replaces.
    pub y: u16,
    /// Entries in `cells`: the pane's width when the host sent the frame.
    pub cols: u16,
    pub cells: *const PgCell,
}

/// The decoded rows of a `delta` message. Owned by the caller; free with
/// [`pg_delta_free`]. Immutable, thread-safe.
#[repr(C)]
pub struct PgDelta {
    pub size: u32,
    pub pane_id: u32,
    /// The frame's sequence number; a gap means a missed frame (re-`attach`).
    pub seq: u32,
    pub row_count: u32,
    pub rows: *const PgDeltaRow,
    /// 1 when `cursor` is meaningful; deltas may leave the cursor out.
    pub has_cursor: u8,
    pub cursor: PgCursor,
    /// 0 main screen, 1 alternate screen, 2 not reported (keep what you had).
    pub alt_screen: u8,
    pub text_count: u32,
    pub text: *const super::PgCellText,
}

/// Builds an owned delta from a `delta` message; NULL for any other message type.
#[no_mangle]
pub extern "C" fn pg_msg_delta(msg: *const super::msg::PgMsg) -> *mut PgDelta {
    ffi_call(ptr::null_mut(), || {
        let msg = msg_ref(msg)?;
        let message = msg.to_message()?;
        let delta = delta_of(&message)?;
        let pane_id = delta.pane_id;
        let text = super::cell_text::OwnedCellText::new(
            delta
                .rows
                .iter()
                .map(|(y, cells)| (i32::from(*y), cells.as_slice())),
        )?;
        let row_count = u32::try_from(delta.rows.len()).map_err(|_| PgError::InvalidArg)?;
        let cells: Vec<Box<[PgCell]>> = delta
            .rows
            .iter()
            .map(|(_, cells)| cells.iter().map(to_pg_cell).collect())
            .collect();
        let rows: Box<[PgDeltaRow]> = delta
            .rows
            .iter()
            .zip(&cells)
            .map(|((y, _), cells)| {
                Ok(PgDeltaRow {
                    size: std::mem::size_of::<PgDeltaRow>() as u32,
                    y: *y,
                    cols: u16::try_from(cells.len()).map_err(|_| PgError::InvalidArg)?,
                    cells: cells.as_ptr(),
                })
            })
            .collect::<Result<_, PgError>>()?;
        let cursor = delta.cursor.unwrap_or(Cursor {
            x: 0,
            y: 0,
            visible: false,
            shape: 0,
        });
        let mut result = Box::new(PgDelta {
            size: std::mem::size_of::<PgDelta>() as u32,
            pane_id,
            seq: delta.seq,
            row_count,
            rows: rows.as_ptr(),
            has_cursor: u8::from(delta.cursor.is_some()),
            cursor: PgCursor {
                size: std::mem::size_of::<PgCursor>() as u32,
                x: cursor.x,
                y: cursor.y,
                visible: u8::from(cursor.visible),
                shape: cursor.shape,
            },
            alt_screen: delta.alt.map_or(2, u8::from),
            text_count: 0,
            text: ptr::null(),
        });
        // No fallible work remains once ownership crosses the C boundary.
        (result.text_count, result.text) = text.into_raw();
        for row in cells {
            let _ = Box::into_raw(row);
        }
        let _ = Box::into_raw(rows);
        Ok(Box::into_raw(result))
    })
}

#[no_mangle]
pub extern "C" fn pg_delta_free(delta: *mut PgDelta) {
    ffi_call((), || {
        if delta.is_null() {
            return Ok(());
        }
        // SAFETY: pointer came from pg_msg_delta and is freed exactly once.
        let delta = unsafe { Box::from_raw(delta) };
        // SAFETY: sparse text is exclusively owned by this delta.
        unsafe {
            super::cell_text::free(delta.text_count, delta.text);
        }
        let count = delta.row_count as usize;
        if count == 0 || delta.rows.is_null() {
            return Ok(());
        }
        // SAFETY: `rows` was a leaked `Box<[PgDeltaRow]>` of `row_count` entries, and each
        // row's `cells` a leaked `Box<[PgCell]>` of `cols` entries.
        unsafe {
            let rows = Box::from_raw(ptr::slice_from_raw_parts_mut(delta.rows.cast_mut(), count));
            for row in rows.iter() {
                if !row.cells.is_null() && row.cols > 0 {
                    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                        row.cells.cast_mut(),
                        usize::from(row.cols),
                    )));
                }
            }
        }
        Ok(())
    });
}
