//! Owned URI table. Appended to PgSnapshot/PgDelta without changing PgCell's stride.
use super::PgStr;
use crate::api::{cell_links, Cell};
use std::ptr;

#[repr(C)]
pub struct PgLink {
    pub size: u32,
    /// Resolve only against the table owned by this snapshot/delta.
    pub id: u32,
    /// Borrowed UTF-8 until the owning snapshot/delta is freed; never rich text.
    pub uri: PgStr,
}

pub(super) struct OwnedLinks {
    entries: Box<[PgLink]>,
    bytes: Vec<Box<[u8]>>,
}

impl OwnedLinks {
    pub(super) fn new<'a>(cells: impl Iterator<Item = &'a Cell>) -> Self {
        let links = cell_links(cells);
        let bytes: Vec<Box<[u8]>> = links
            .iter()
            .map(|link| link.uri.as_bytes().into())
            .collect();
        let entries = links
            .iter()
            .zip(&bytes)
            .map(|(link, uri)| PgLink {
                size: std::mem::size_of::<PgLink>() as u32,
                id: link.id,
                uri: PgStr {
                    size: std::mem::size_of::<PgStr>() as u32,
                    ptr: uri.as_ptr(),
                    len: uri.len(),
                },
            })
            .collect();
        Self { entries, bytes }
    }

    pub(super) fn into_raw(self) -> (u32, *const PgLink) {
        if self.entries.is_empty() {
            return (0, ptr::null());
        }
        let count = self.entries.len() as u32;
        for bytes in self.bytes {
            let _ = Box::into_raw(bytes);
        }
        (count, Box::into_raw(self.entries).cast::<PgLink>())
    }
}

/// # Safety
/// `entries` and `count` came from OwnedLinks::into_raw and have not been freed.
pub(super) unsafe fn free(count: u32, entries: *const PgLink) {
    if count == 0 || entries.is_null() {
        return;
    }
    // SAFETY: the caller owns the exact allocation returned by into_raw.
    let entries = unsafe {
        Box::from_raw(ptr::slice_from_raw_parts_mut(
            entries.cast_mut(),
            count as usize,
        ))
    };
    for link in &entries {
        // SAFETY: into_raw transferred each URI's boxed byte slice with this length.
        unsafe {
            drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                link.uri.ptr.cast_mut(),
                link.uri.len,
            )));
        }
    }
}
