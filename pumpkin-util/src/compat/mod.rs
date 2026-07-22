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

/// lantern: blocking HTTP for wasm via the host page.
///
/// The page mounts `http.sock` next to `net.sock`. A request is one frame
/// `[u32 BE total_len][u32 BE req_id]["METHOD url"]`; the response comes back
/// as `[u32 BE total_len][u32 BE req_id][u16 BE status][body]`. Calls are
/// serialized under a mutex, so responses always match the waiting request.
#[cfg(target_family = "wasm")]
pub mod http {
    use std::io;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    struct Bridge {
        fd: u32,
        next_id: u32,
        acc: Vec<u8>,
    }

    static BRIDGE: OnceLock<Option<Mutex<Bridge>>> = OnceLock::new();

    fn bridge() -> Option<&'static Mutex<Bridge>> {
        BRIDGE
            .get_or_init(|| {
                use std::os::fd::IntoRawFd;
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open("http.sock")
                    .ok()
                    .map(|f| {
                        Mutex::new(Bridge {
                            fd: f.into_raw_fd() as u32,
                            next_id: 1,
                            acc: Vec::new(),
                        })
                    })
            })
            .as_ref()
    }

    pub struct Response {
        pub status: u16,
        pub body: Vec<u8>,
    }

    pub fn get(url: &str) -> io::Result<Response> {
        request("GET", url)
    }

    pub fn request(method: &str, url: &str) -> io::Result<Response> {
        let Some(lock) = bridge() else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no http.sock mounted by host page",
            ));
        };
        let mut b = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let id = b.next_id;
        b.next_id = b.next_id.wrapping_add(1).max(1);

        let payload = format!("{method} {url}");
        let mut frame = Vec::with_capacity(8 + payload.len());
        frame.extend_from_slice(&(4 + payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&id.to_be_bytes());
        frame.extend_from_slice(payload.as_bytes());
        fd_write_all(b.fd, &frame)?;

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut tmp = vec![0u8; 64 * 1024];
        loop {
            let n = fd_read_now(b.fd, &mut tmp);
            if n == 0 {
                if Instant::now() > deadline {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "http bridge timeout"));
                }
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            let chunk = tmp[..n].to_vec();
            b.acc.extend_from_slice(&chunk);

            while b.acc.len() >= 4 {
                let flen = u32::from_be_bytes([b.acc[0], b.acc[1], b.acc[2], b.acc[3]]) as usize;
                if flen < 6 {
                    b.acc.clear();
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "corrupt http frame"));
                }
                if b.acc.len() < 4 + flen {
                    break;
                }
                let frame_bytes: Vec<u8> = b.acc.drain(..4 + flen).skip(4).collect();
                let rid = u32::from_be_bytes([frame_bytes[0], frame_bytes[1], frame_bytes[2], frame_bytes[3]]);
                let status = u16::from_be_bytes([frame_bytes[4], frame_bytes[5]]);
                let body = frame_bytes[6..].to_vec();
                if rid == id {
                    return Ok(Response { status, body });
                }
                // A response for a request that timed out earlier; drop it.
            }
        }
    }

    fn fd_read_now(fd: u32, buf: &mut [u8]) -> usize {
        let iov = wasi::Iovec {
            buf: buf.as_mut_ptr(),
            buf_len: buf.len(),
        };
        unsafe { wasi::fd_read(fd, &[iov]).unwrap_or(0) }
    }

    fn fd_write_all(fd: u32, mut data: &[u8]) -> std::io::Result<()> {
        while !data.is_empty() {
            let iov = wasi::Ciovec {
                buf: data.as_ptr(),
                buf_len: data.len(),
            };
            match unsafe { wasi::fd_write(fd, &[iov]) } {
                Ok(0) | Err(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "http bridge write failed",
                    ));
                }
                Ok(n) => data = &data[n..],
            }
        }
        Ok(())
    }
}
