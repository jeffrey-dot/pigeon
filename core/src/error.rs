//! Error type shared by the whole core. No panics cross the FFI boundary: every
//! failure becomes one of these and is reported as a `PgErrorCode`.

/// What went wrong. Kept small and copyable; the C ABI mirrors it in `ffi`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgError {
    /// Caller passed a bad size/version/enum value.
    InvalidArg,
    /// `pane_id` is not known to this core.
    NoSuchPane,
    /// `split_id` is not known to this core.
    NoSuchSplit,
    /// Allocation failed.
    NoMemory,
    /// Output buffer too small; retry with the reported size.
    Overflow,
    /// A panic was caught at the FFI boundary.
    Panic,
    /// Valid request, not implemented in this build.
    Unsupported,
}

pub type PgResult<T> = Result<T, PgError>;

impl std::fmt::Display for PgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::InvalidArg => "invalid argument",
            Self::NoSuchPane => "no such pane",
            Self::NoSuchSplit => "no such split",
            Self::NoMemory => "out of memory",
            Self::Overflow => "output buffer too small",
            Self::Panic => "panic caught at the FFI boundary",
            Self::Unsupported => "unsupported in this build",
        };
        f.write_str(text)
    }
}

impl std::error::Error for PgError {}
