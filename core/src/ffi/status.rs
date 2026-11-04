//! Legacy focused-pane status ABI; new consumers should use status_snapshot.

use super::{ffi_call, unsafe_ref, unsafe_mut, map_state, map_kind, PgCore, PgStr, PgError, PG_CORE_ABI_VERSION};

// ---------------------------------------------------------------- status

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgStatusState {
    PG_STATE_IDLE = 0,
    PG_STATE_WORKING = 1,
    PG_STATE_BLOCKED = 2,
    PG_STATE_DONE = 3,
    PG_STATE_ERROR = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgStatusKind {
    PG_KIND_NONE = 0,
    PG_KIND_PERMISSION = 1,
    PG_KIND_QUESTION = 2,
    PG_KIND_AUTH = 3,
}

#[repr(C)]
pub struct PgStatus {
    pub size: u32,
    pub version: u32,
    pub pane_id: u32,
    /// Monotonic per pane.
    pub seq: u32,
    pub state: PgStatusState,
    pub kind: PgStatusKind,
    /// Plain text only; never interpreted as markup.
    pub msg: PgStr,
}

#[no_mangle]
pub extern "C" fn pg_status_count(core: *mut PgCore) -> usize {
    ffi_call(0, || {
        let (_, records) = unsafe_ref(core)?.focused_status()?;
        Ok(records.count())
    })
}

/// Fills `out` with the focused pane's status record at `index`.
#[no_mangle]
pub extern "C" fn pg_status_at(core: *mut PgCore, index: usize, out: *mut PgStatus) -> bool {
    ffi_call(false, || {
        let core = unsafe_ref(core)?;
        if out.is_null() {
            return Err(PgError::InvalidArg);
        }
        let (pane_id, mut records) = core.focused_status()?;
        let record = records.nth(index).ok_or(PgError::InvalidArg)?;
        // SAFETY: `out` checked above; `msg` borrows the record, which stays
        // alive until the next mutation of this core (documented in the header).
        unsafe {
            *out = PgStatus {
                size: std::mem::size_of::<PgStatus>() as u32,
                version: PG_CORE_ABI_VERSION,
                pane_id,
                seq: record.seq,
                state: map_state(record.state),
                kind: map_kind(record.kind),
                msg: PgStr {
                    size: std::mem::size_of::<PgStr>() as u32,
                    ptr: record.msg.as_ptr(),
                    len: record.msg.len(),
                },
            }
        };
        Ok(true)
    })
}

/// Marks a done/error record as seen; it may then be evicted.
#[no_mangle]
pub extern "C" fn pg_status_ack(core: *mut PgCore, pane_id: u32, seq: u32) -> bool {
    ffi_call(false, || unsafe_mut(core)?.ack_status(pane_id, seq))
}
