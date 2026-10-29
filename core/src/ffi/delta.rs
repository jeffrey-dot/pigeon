//! `delta` frames across the C ABI: the dirty rows of a pane, decoded by the core so the
//! client patches its grid without knowing CBOR (protocol §3.3, `docs/core.md`).
//!
//! A full `snapshot` per keystroke is ~100 KB on a 200×50 pane; a delta is the one or two
//! rows that changed (~2 KB). Over ssh that difference is the typing latency.

use std::ptr;

use super::msg::{msg_ref, PgMsgType};
use super::{ffi_call, to_pg_cell, PgCell, PgCursor};
use crate::error::PgError;
use crate::proto::Message;

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
}

/// Builds an owned delta from a `delta` message; NULL for any other message type.
#[no_mangle]
pub extern "C" fn pg_msg_delta(msg: *const super::msg::PgMsg) -> *mut PgDelta {
    ffi_call(ptr::null_mut(), || {
        let msg = msg_ref(msg)?;
        if msg.kind() != PgMsgType::PG_MSG_DELTA {
            return Err(PgError::InvalidArg);
        }
        let message = msg.to_message()?;
        let Message::Delta { pane_id, .. } = message else {
            return Err(PgError::InvalidArg);
        };
        let delta = crate::session::delta_from_message(&message).ok_or(PgError::InvalidArg)?;
        let rows = delta
            .rows
            .into_iter()
            .map(|(y, cells)| {
                let cells: Box<[PgCell]> = cells.iter().map(to_pg_cell).collect();
                let cols = u16::try_from(cells.len()).map_err(|_| PgError::InvalidArg)?;
                Ok(PgDeltaRow {
                    size: std::mem::size_of::<PgDeltaRow>() as u32,
                    y,
                    cols,
                    // Reclaimed in pg_delta_free with the same length (`cols`).
                    cells: Box::into_raw(cells).cast::<PgCell>().cast_const(),
                })
            })
            .collect::<Result<Box<[PgDeltaRow]>, PgError>>()?;
        let row_count = u32::try_from(rows.len()).map_err(|_| PgError::InvalidArg)?;
        let cursor = delta.cursor.unwrap_or(crate::term::Cursor {
            x: 0,
            y: 0,
            visible: false,
            shape: 0,
        });
        Ok(Box::into_raw(Box::new(PgDelta {
            size: std::mem::size_of::<PgDelta>() as u32,
            pane_id,
            seq: delta.seq,
            row_count,
            rows: Box::into_raw(rows).cast::<PgDeltaRow>().cast_const(),
            has_cursor: u8::from(delta.cursor.is_some()),
            cursor: PgCursor {
                size: std::mem::size_of::<PgCursor>() as u32,
                x: cursor.x,
                y: cursor.y,
                visible: u8::from(cursor.visible),
                shape: cursor.shape,
            },
            alt_screen: delta.alt.map_or(2, u8::from),
        })))
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
