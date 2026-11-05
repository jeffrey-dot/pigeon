//! Text queries: pointer conversion and ownership only; all semantics live in api.

use super::{ffi_call, msg::PgMsg, unsafe_ref, PgCore, PgStr, PG_CORE_ABI_VERSION};
use crate::api::{self, PgError, PgResult};
use std::{mem::size_of, ptr};

#[repr(C)]
pub struct PgTextRange {
    pub size: u32,
    pub start_row: i32,
    pub start_col: u16,
    pub start_byte: u16,
    pub end_row: i32,
    pub end_col: u16,
    pub end_byte: u16,
}

/// kind: 0 info, 1 literal search, 2 range. Search generation=0 starts fresh.
/// case_sensitive=false folds ASCII only. start_set uses range.start as a search
/// continuation. Range queries use both endpoints. Unused fields are ignored.
#[repr(C)]
pub struct PgTextQuery {
    pub size: u32,
    pub version: u32,
    pub kind: u32,
    pub generation: u64,
    pub pattern: PgStr,
    pub case_sensitive: bool,
    pub max_results: u16,
    pub start_set: bool,
    pub range: PgTextRange,
}

/// Immutable and independently owned, including strings returned by its accessors.
pub struct PgTextResult {
    id: u32,
    pane_id: u32,
    result: api::TextResult,
}

#[repr(C)]
pub struct PgTextResultInfo {
    pub size: u32,
    pub version: u32,
    /// 0 info, 1 search, 2 range.
    pub kind: u32,
    /// Protocol request id, or 0 for an embedded query.
    pub id: u32,
    pub pane_id: u32,
    pub generation: u64,
    pub first_row: i32,
    pub last_row: i32,
    pub cols: u16,
    pub alt: bool,
    pub count: usize,
    /// A page with no matches may still need continuation.
    pub has_next: bool,
    pub next_row: i32,
    pub next_col: u16,
    pub next_byte: u16,
}

/// Runs one bounded query on an embedded pane. NULL means failure; read last_error_code.
#[no_mangle]
pub extern "C" fn pg_text_query(
    core: *const PgCore,
    pane_id: u32,
    query: *const PgTextQuery,
) -> *mut PgTextResult {
    ffi_call(ptr::null_mut(), || {
        let result = unsafe_ref(core)?.text_query(pane_id, &query_of(query)?)?;
        Ok(Box::into_raw(Box::new(PgTextResult {
            id: 0,
            pane_id,
            result,
        })))
    })
}

/// Builds a text_query request for the negotiated text_query capability.
#[no_mangle]
pub extern "C" fn pg_msg_text_query(
    id: u32,
    pane_id: u32,
    query: *const PgTextQuery,
) -> *mut PgMsg {
    ffi_call(ptr::null_mut(), || {
        let message = api::text_query_message(id, pane_id, query_of(query)?)?;
        Ok(Box::into_raw(Box::new(PgMsg::from_message(&message)?)))
    })
}

/// Copies a validated text_query_result; survives freeing the source message.
#[no_mangle]
pub extern "C" fn pg_msg_text_result(msg: *const PgMsg) -> *mut PgTextResult {
    ffi_call(ptr::null_mut(), || {
        let message = handle(msg)?.to_message()?;
        let (id, pane_id, result) = api::text_result_of(&message)?;
        Ok(Box::into_raw(Box::new(PgTextResult {
            id,
            pane_id,
            result,
        })))
    })
}

/// Caller initializes out.size; larger structs have trailing bytes zeroed.
#[no_mangle]
pub extern "C" fn pg_text_result_info(
    result: *const PgTextResult,
    out: *mut PgTextResultInfo,
) -> bool {
    ffi_call(false, || {
        let result = handle(result)?;
        let info = result.result.info();
        let next = result.result.next();
        let point = next.unwrap_or_default();
        let (kind, count) = match &result.result {
            api::TextResult::Info { .. } => (0, 0),
            api::TextResult::Search { hits, .. } => (1, hits.len()),
            api::TextResult::Range { .. } => (2, 0),
        };
        write_sized(
            out,
            PgTextResultInfo {
                size: size_of::<PgTextResultInfo>() as u32,
                version: PG_CORE_ABI_VERSION,
                kind,
                id: result.id,
                pane_id: result.pane_id,
                generation: info.generation,
                first_row: info.first_row,
                last_row: info.last_row,
                cols: info.cols,
                alt: info.alt,
                count,
                has_next: next.is_some(),
                next_row: point.row,
                next_col: point.col,
                next_byte: point.byte,
            },
        )?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_text_result_hit(
    result: *const PgTextResult,
    index: usize,
    out: *mut PgTextRange,
) -> bool {
    ffi_call(false, || {
        let api::TextResult::Search { hits, .. } = &handle(result)?.result else {
            return Err(PgError::InvalidArg);
        };
        let range = hits.get(index).ok_or(PgError::InvalidArg)?;
        write_sized(
            out,
            PgTextRange {
                size: size_of::<PgTextRange>() as u32,
                start_row: range.start.row,
                start_col: range.start.col,
                start_byte: range.start.byte,
                end_row: range.end.row,
                end_col: range.end.col,
                end_byte: range.end.byte,
            },
        )?;
        Ok(true)
    })
}

/// Range text borrows this result until pg_text_result_free, including across other calls.
#[no_mangle]
pub extern "C" fn pg_text_result_text(result: *const PgTextResult, out: *mut PgStr) -> bool {
    ffi_call(false, || {
        let api::TextResult::Range { text, .. } = &handle(result)?.result else {
            return Err(PgError::InvalidArg);
        };
        write_sized(
            out,
            PgStr {
                size: size_of::<PgStr>() as u32,
                ptr: text.as_ptr(),
                len: text.len(),
            },
        )?;
        Ok(true)
    })
}

#[no_mangle]
pub extern "C" fn pg_text_result_free(result: *mut PgTextResult) {
    ffi_call((), || {
        if !result.is_null() {
            // SAFETY: caller transfers a live owned handle exactly once.
            unsafe {
                drop(Box::from_raw(result));
            }
        }
        Ok(())
    });
}

fn query_of(query: *const PgTextQuery) -> PgResult<api::TextQuery> {
    if query.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller supplies readable memory beginning with size: u32.
    if unsafe { query.cast::<u32>().read() } < size_of::<PgTextQuery>() as u32 {
        return Err(PgError::InvalidArg);
    }
    let query = handle(query)?;
    if query.version != PG_CORE_ABI_VERSION {
        return Err(PgError::InvalidArg);
    }
    let range = || -> PgResult<api::TextRange> {
        if query.range.size < size_of::<PgTextRange>() as u32 {
            return Err(PgError::InvalidArg);
        }
        Ok(api::TextRange {
            start: api::TextPoint {
                row: query.range.start_row,
                col: query.range.start_col,
                byte: query.range.start_byte,
            },
            end: api::TextPoint {
                row: query.range.end_row,
                col: query.range.end_col,
                byte: query.range.end_byte,
            },
        })
    };
    match query.kind {
        0 => Ok(api::TextQuery::Info),
        1 => {
            if query.pattern.size < size_of::<PgStr>() as u32
                || query.pattern.len > api::MAX_QUERY_BYTES
            {
                return Err(PgError::InvalidArg);
            }
            if query.pattern.ptr.is_null() {
                return Err(PgError::InvalidArg);
            }
            // SAFETY: caller provides pattern.len readable bytes; bounded above.
            let bytes = unsafe { std::slice::from_raw_parts(query.pattern.ptr, query.pattern.len) };
            let pattern = std::str::from_utf8(bytes)
                .map_err(|_| PgError::InvalidArg)?
                .to_owned();
            Ok(api::TextQuery::Search {
                generation: query.generation,
                pattern,
                case_sensitive: query.case_sensitive,
                max_results: query.max_results,
                start: if query.start_set {
                    Some(range()?.start)
                } else {
                    None
                },
            })
        }
        2 => Ok(api::TextQuery::Range {
            generation: query.generation,
            range: range()?,
        }),
        _ => Err(PgError::InvalidArg),
    }
}

fn handle<'a, T>(value: *const T) -> PgResult<&'a T> {
    if value.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller keeps a valid handle alive for the call; null was rejected.
    Ok(unsafe { &*value })
}

fn write_sized<T>(out: *mut T, value: T) -> PgResult<()> {
    if out.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller supplies aligned writable memory beginning with size: u32.
    let size = unsafe { out.cast::<u32>().read() } as usize;
    if size < size_of::<T>() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: caller promises size writable bytes and proper alignment.
    unsafe {
        out.cast::<u8>().write_bytes(0, size);
        out.write(value);
    }
    Ok(())
}
