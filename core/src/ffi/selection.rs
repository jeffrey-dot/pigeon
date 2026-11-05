//! Immutable selection handle; pointer conversion only, semantics live in api::select_grid.
use super::{ffi_call, PgSnapshot, PgStr};
use crate::api::{self, PgError, PgResult};
use std::{mem::size_of, ptr};

#[repr(C)]
pub struct PgGridSelection {
    pub size: u32,
    pub version: u32,
    pub anchor_row: u16,
    pub anchor_col: u16,
    pub head_row: u16,
    pub head_col: u16,
    /// 0 cell, 1 word, 2 line. Points are inclusive visible-grid coordinates.
    pub unit: u8,
    pub rectangle: bool,
}
#[repr(C)]
pub struct PgSelectionSpan {
    pub size: u32,
    pub row: u16,
    pub first: u16,
    pub end: u16,
}
pub struct PgSelection {
    selection: api::Selection,
}

#[no_mangle]
pub extern "C" fn pg_select_grid(
    snapshot: *const PgSnapshot,
    request: *const PgGridSelection,
) -> *mut PgSelection {
    ffi_call(ptr::null_mut(), || {
        let request = sized(request)?;
        if request.version != super::PG_CORE_ABI_VERSION {
            return Err(PgError::InvalidArg);
        }
        let unit = match request.unit {
            0 => api::SelectionUnit::Cell,
            1 => api::SelectionUnit::Word,
            2 => api::SelectionUnit::Line,
            _ => return Err(PgError::InvalidArg),
        };
        let picture = picture(snapshot)?;
        let selection = api::select_grid(
            &picture,
            &api::GridSelection {
                anchor: api::GridPoint {
                    row: request.anchor_row,
                    col: request.anchor_col,
                },
                head: api::GridPoint {
                    row: request.head_row,
                    col: request.head_col,
                },
                unit,
                rectangle: request.rectangle,
            },
        )?;
        Ok(Box::into_raw(Box::new(PgSelection { selection })))
    })
}

#[no_mangle]
/// Returned text borrows this selection until pg_selection_free, not the source snapshot.
pub extern "C" fn pg_selection_text(selection: *const PgSelection) -> PgStr {
    ffi_call(
        PgStr {
            size: size_of::<PgStr>() as u32,
            ptr: ptr::null(),
            len: 0,
        },
        || {
            let text = &handle(selection)?.selection.text;
            Ok(PgStr {
                size: size_of::<PgStr>() as u32,
                ptr: text.as_ptr(),
                len: text.len(),
            })
        },
    )
}
#[no_mangle]
pub extern "C" fn pg_selection_spans(
    selection: *const PgSelection,
    out: *mut PgSelectionSpan,
    cap: usize,
) -> usize {
    ffi_call(0, || {
        let spans = &handle(selection)?.selection.spans;
        if out.is_null() {
            return Ok(spans.len());
        }
        if cap < spans.len() {
            return Err(PgError::Overflow);
        }
        for (index, span) in spans.iter().enumerate() {
            // SAFETY: caller provides cap aligned writable entries; cap was checked.
            unsafe {
                out.add(index).write(PgSelectionSpan {
                    size: size_of::<PgSelectionSpan>() as u32,
                    row: span.row,
                    first: span.first,
                    end: span.end,
                });
            }
        }
        Ok(spans.len())
    })
}
#[no_mangle]
pub extern "C" fn pg_selection_free(selection: *mut PgSelection) {
    ffi_call((), || {
        if !selection.is_null() {
            // SAFETY: caller transfers this handle once, allocated by pg_select_grid.
            unsafe {
                drop(Box::from_raw(selection));
            }
        }
        Ok(())
    });
}

fn picture(snapshot: *const PgSnapshot) -> PgResult<api::Snapshot> {
    let snapshot = sized(snapshot)?;
    if snapshot.version != super::PG_CORE_ABI_VERSION
        || snapshot.cols == 0
        || snapshot.rows == 0
        || usize::from(snapshot.cols) * usize::from(snapshot.rows) > 1_000_000
    {
        return Err(PgError::InvalidArg);
    }
    let rows = slice(snapshot.screen, usize::from(snapshot.rows))?;
    let mut screen = Vec::with_capacity(rows.len());
    for row in rows {
        if row.size < size_of::<super::PgRow>() as u32 {
            return Err(PgError::InvalidArg);
        }
        let cells = slice(row.cells, usize::from(snapshot.cols))?;
        let mut converted = Vec::with_capacity(cells.len());
        for cell in cells {
            if cell.size < size_of::<super::PgCell>() as u32 || cell.width > 2 {
                return Err(PgError::InvalidArg);
            }
            converted.push(api::Cell {
                codepoint: cell.codepoint,
                fg: cell.fg,
                bg: cell.bg,
                attrs: cell.attrs,
                link: 0,
                width: cell.width,
                extra_text: None,
                hyperlink: None,
            });
        }
        screen.push(converted);
    }
    let grid_cells = usize::from(snapshot.cols) * usize::from(snapshot.rows);
    let total_cells = (snapshot.history_count as usize)
        .checked_mul(usize::from(snapshot.cols))
        .and_then(|history| grid_cells.checked_add(history))
        .ok_or(PgError::InvalidArg)?;
    if snapshot.text_count as usize > total_cells || snapshot.text_count > 1_000_000 {
        return Err(PgError::InvalidArg);
    }
    let entries = slice(snapshot.text, snapshot.text_count as usize)?;
    let mut text_bytes = 0usize;
    for entry in entries {
        if entry.row < 0 {
            continue;
        }
        if entry.size < size_of::<super::PgCellText>() as u32
            || entry.text.size < size_of::<PgStr>() as u32
            || entry.text.len > 4096
        {
            return Err(PgError::InvalidArg);
        }
        text_bytes = text_bytes
            .checked_add(entry.text.len)
            .ok_or(PgError::Limit)?;
        if text_bytes > 8 * 1024 * 1024 {
            return Err(PgError::Limit);
        }
        let text = std::str::from_utf8(slice(entry.text.ptr, entry.text.len)?)
            .map_err(|_| PgError::InvalidArg)?;
        let cell = screen
            .get_mut(entry.row as usize)
            .and_then(|r| r.get_mut(usize::from(entry.col)))
            .ok_or(PgError::InvalidArg)?;
        cell.extra_text = Some(std::sync::Arc::from(text));
    }
    Ok(api::Snapshot {
        cols: snapshot.cols,
        rows: snapshot.rows,
        alt: snapshot.alt_screen != 0,
        cursor: api::Cursor {
            x: snapshot.cursor.x,
            y: snapshot.cursor.y,
            visible: snapshot.cursor.visible != 0,
            shape: snapshot.cursor.shape,
        },
        scroll: (snapshot.scroll_region.top, snapshot.scroll_region.bottom),
        screen,
        history: vec![],
    })
}
fn sized<'a, T>(value: *const T) -> PgResult<&'a T> {
    if value.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: all sized input structs start with readable u32 size.
    if unsafe { value.cast::<u32>().read() } < size_of::<T>() as u32 {
        return Err(PgError::InvalidArg);
    }
    handle(value)
}
fn handle<'a, T>(value: *const T) -> PgResult<&'a T> {
    if value.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller keeps aligned readable input alive for this call.
    Ok(unsafe { &*value })
}
fn slice<'a, T>(value: *const T, len: usize) -> PgResult<&'a [T]> {
    if len == 0 {
        return Ok(&[]);
    }
    if value.is_null()
        || len
            .checked_mul(size_of::<T>())
            .is_none_or(|bytes| bytes > isize::MAX as usize)
    {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller supplies len readable elements; null/size overflow were checked.
    Ok(unsafe { std::slice::from_raw_parts(value, len) })
}
