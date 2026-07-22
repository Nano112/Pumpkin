//! lantern: platform compatibility layer.
//!
//! On native targets these modules are thin re-exports of `tokio::fs` /
//! `tokio::time`. On wasm targets (no filesystem, no tokio time driver) they
//! provide an in-memory filesystem and a pass-through timeout, so crates like
//! `pumpkin-world` can compile unchanged apart from swapping the import path.

#[cfg(not(target_family = "wasm"))]
pub mod fs {
    pub use tokio::fs::*;
}

#[cfg(not(target_family = "wasm"))]
pub mod time {
    pub use tokio::time::*;
}

#[cfg(target_family = "wasm")]
pub mod fs;

#[cfg(target_family = "wasm")]
pub mod time {
    use std::future::Future;
    use std::time::Duration;

    #[derive(Debug)]
    pub struct Elapsed;

    impl std::fmt::Display for Elapsed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "deadline has elapsed")
        }
    }

    impl std::error::Error for Elapsed {}

    /// No real timer driver on wasm yet: awaits the future to completion and
    /// never reports elapse. Good enough while everything is in-memory and
    /// single-threaded; revisit with a JS-timer-backed implementation if a
    /// future here can actually hang.
    pub async fn timeout<F: Future>(_duration: Duration, future: F) -> Result<F::Output, Elapsed> {
        Ok(future.await)
    }
}
