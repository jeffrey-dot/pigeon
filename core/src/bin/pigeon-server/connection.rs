//! One client connection: the frames it sends, the handshake it starts with, and the
//! queue its writer drains.
//!
//! `state` is what the host remembers about the connection; `out_queue` bounds what it may
//! fall behind by; `resync` catches it up when it does.

pub mod client;
pub mod handshake;
pub mod out_queue;
pub mod outbound;
pub mod resync;
pub mod state;
