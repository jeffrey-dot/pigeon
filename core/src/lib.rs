//! pigeon terminal core.
//!
//! Everything a terminal must get right — VT state, layout, OSC 7501 status,
//! and the client/host protocol — lives here and nowhere else. The library is
//! pure logic: no IO, no platform API, no `unsafe` outside [`ffi`].
//!
//! Two consumers, one implementation:
//!   * `pigeon-host` (this crate's binary) owns the PTYs and speaks the wire
//!     protocol from [`proto`];
//!   * a native client links the C ABI from [`ffi`], or (in Rust) uses
//!     [`api`], and skips the wire.
//!
//! The internal modules are public only with the `internals` feature, which
//! `pigeon-host` and the integration tests turn on. A Rust client depends on
//! this crate with `default-features = false` and sees [`api`] and [`ffi`] only.

pub mod api;
pub mod error;
pub mod ffi;

/// Declares a module that is public with `internals` and crate-private without.
macro_rules! internal_mod {
    ($name:ident) => {
        #[cfg(feature = "internals")]
        pub mod $name;
        #[cfg(not(feature = "internals"))]
        #[allow(dead_code)]
        mod $name;
    };
}

internal_mod!(layout);
internal_mod!(proto);
internal_mod!(session);
internal_mod!(status);
internal_mod!(term);

pub use error::{PgError, PgResult};
