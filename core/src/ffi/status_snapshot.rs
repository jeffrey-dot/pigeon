//! Owned status samples. All lifecycle and wire validation are delegated to api.

use super::{
    ffi_call, map_kind, map_state, msg::PgMsg, unsafe_ref, PgCore, PgStatusKind, PgStatusState,
    PgStr, PG_CORE_ABI_VERSION,
};
use crate::api::{self, PgError, PgResult};
use std::{mem::size_of, ptr};

/// Immutable handle; independently owned, valid until pg_status_snapshot_free.
pub struct PgStatusSnapshot {
    pane_id: u32,
    snapshot: api::StatusSnapshot,
}

#[repr(C)]
pub struct PgStatusSnapshotInfo {
    pub size: u32,
    pub version: u32,
    pub pane_id: u32,
    /// Collection revision, independent of record seq and screen seq.
    pub revision: u64,
    pub count: usize,
}

#[repr(C)]
pub struct PgStatusSnapshotRecord {
    pub size: u32,
    pub version: u32,
    /// Record identity for ack_status, not a collection revision.
    pub seq: u32,
    pub state: PgStatusState,
    pub kind: PgStatusKind,
    /// All strings borrow the snapshot handle, not the core or message.
    pub id: PgStr,
    pub msg: PgStr,
    pub app: PgStr,
    /// Frozen at sampling time; reads do not advance this value.
    pub age_ms: u64,
}

/// Copies all pending records for pane_id, independently of focus. Never consumes or
/// acknowledges. Empty is a valid handle; missing pane is NULL / PG_ERR_NO_SUCH_PANE.
#[no_mangle]
pub extern "C" fn pg_status_snapshot_new(
    core: *const PgCore,
    pane_id: u32,
) -> *mut PgStatusSnapshot {
    ffi_call(ptr::null_mut(), || {
        let snapshot = unsafe_ref(core)?.status_snapshot(pane_id)?;
        Ok(Box::into_raw(Box::new(PgStatusSnapshot {
            pane_id,
            snapshot,
        })))
    })
}

/// Copies a validated status_snapshot message. Wrong type or invalid fields return
/// NULL / PG_ERR_INVALID_ARG. The handle survives freeing the source message.
#[no_mangle]
pub extern "C" fn pg_msg_status_snapshot(msg: *const PgMsg) -> *mut PgStatusSnapshot {
    ffi_call(ptr::null_mut(), || {
        if msg.is_null() {
            return Err(PgError::InvalidArg);
        }
        // SAFETY: the caller supplies a live PgMsg; null was rejected above.
        let message = unsafe { &*msg }.to_message()?;
        let (pane_id, snapshot) = api::status_snapshot_of(&message)?;
        Ok(Box::into_raw(Box::new(PgStatusSnapshot {
            pane_id,
            snapshot,
        })))
    })
}

/// Reads immutable metadata. Caller initializes out.size; smaller structs are rejected,
/// larger structs have their trailing bytes zeroed. NULL is PG_ERR_INVALID_ARG.
#[no_mangle]
pub extern "C" fn pg_status_snapshot_info(
    snapshot: *const PgStatusSnapshot,
    out: *mut PgStatusSnapshotInfo,
) -> bool {
    ffi_call(false, || {
        let snapshot = snapshot_ref(snapshot)?;
        write_sized(
            out,
            PgStatusSnapshotInfo {
                size: size_of::<PgStatusSnapshotInfo>() as u32,
                version: PG_CORE_ABI_VERSION,
                pane_id: snapshot.pane_id,
                revision: snapshot.snapshot.revision,
                count: snapshot.snapshot.records.len(),
            },
        )?;
        Ok(true)
    })
}

/// Reads one record in id order. out.size follows pg_status_snapshot_info. Index >= count
/// returns false / PG_ERR_INVALID_ARG. Strings remain valid until this handle is freed.
#[no_mangle]
pub extern "C" fn pg_status_snapshot_at(
    snapshot: *const PgStatusSnapshot,
    index: usize,
    out: *mut PgStatusSnapshotRecord,
) -> bool {
    ffi_call(false, || {
        let snapshot = snapshot_ref(snapshot)?;
        let record = snapshot
            .snapshot
            .records
            .get(index)
            .ok_or(PgError::InvalidArg)?;
        write_sized(
            out,
            PgStatusSnapshotRecord {
                size: size_of::<PgStatusSnapshotRecord>() as u32,
                version: PG_CORE_ABI_VERSION,
                seq: record.seq,
                state: map_state(record.state),
                kind: map_kind(record.kind),
                id: text(&record.id),
                msg: text(&record.msg),
                app: text(&record.app),
                age_ms: record.age_ms,
            },
        )?;
        Ok(true)
    })
}

/// Frees a snapshot and invalidates every string borrowed from it. NULL is a no-op.
#[no_mangle]
pub extern "C" fn pg_status_snapshot_free(snapshot: *mut PgStatusSnapshot) {
    ffi_call((), || {
        if !snapshot.is_null() {
            // SAFETY: the caller transfers an owned, not previously freed handle.
            unsafe {
                drop(Box::from_raw(snapshot));
            }
        }
        Ok(())
    });
}

fn snapshot_ref<'a>(snapshot: *const PgStatusSnapshot) -> PgResult<&'a PgStatusSnapshot> {
    if snapshot.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: the caller keeps the immutable handle alive for this call.
    Ok(unsafe { &*snapshot })
}

fn text(value: &str) -> PgStr {
    PgStr {
        size: size_of::<PgStr>() as u32,
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

// Only called for the two repr(C) output structs above, both starting with size: u32.
fn write_sized<T>(out: *mut T, value: T) -> PgResult<()> {
    if out.is_null() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: the caller provides aligned writable memory starting with its u32 size.
    let size = unsafe { out.cast::<u32>().read() } as usize;
    if size < size_of::<T>() {
        return Err(PgError::InvalidArg);
    }
    // SAFETY: the caller promises size bytes; padding and future fields are zeroed.
    unsafe {
        out.cast::<u8>().write_bytes(0, size);
        out.write(value);
    }
    Ok(())
}
