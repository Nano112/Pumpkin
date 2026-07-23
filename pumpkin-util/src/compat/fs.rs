//! lantern: in-memory async "filesystem" for wasm builds.
//!
//! Implements just the surface `pumpkin-world` uses: `read`, `write`,
//! `rename`, `create_dir_all`, `File::create`, and `OpenOptions` with
//! write/create/truncate/append, where `File` is `AsyncRead + AsyncWrite +
//! AsyncSeek`. Contents live in a process-global map keyed by path; a later
//! iteration can swap the backing store for OPFS via a JS bridge.

use std::collections::HashMap;
use std::io::{self, SeekFrom};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

static FILES: LazyLock<Mutex<HashMap<PathBuf, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn store() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Vec<u8>>> {
    FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("memfs: no such file: {}", path.display()),
    )
}

pub async fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref();
    store().get(path).cloned().ok_or_else(|| not_found(path))
}

pub async fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let bytes = read(path).await?;
    String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    store().insert(path.as_ref().to_path_buf(), contents.as_ref().to_vec());
    Ok(())
}

pub async fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let mut files = store();
    let contents = files.remove(from.as_ref()).ok_or_else(|| not_found(from.as_ref()))?;
    files.insert(to.as_ref().to_path_buf(), contents);
    Ok(())
}

pub async fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    store().remove(path.as_ref()).map(|_| ()).ok_or_else(|| not_found(path.as_ref()))
}

/// Directories are implicit in the flat path-keyed store.
pub async fn create_dir_all(_path: impl AsRef<Path>) -> io::Result<()> {
    Ok(())
}

pub async fn try_exists(path: impl AsRef<Path>) -> io::Result<bool> {
    Ok(store().contains_key(path.as_ref()))
}

/// lantern: point-in-time copy of every file, for host-side persistence.
#[must_use]
pub fn snapshot_entries() -> Vec<(PathBuf, Vec<u8>)> {
    store().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// lantern: restore a previously snapshotted set of files (replaces matches).
pub fn restore_entries(entries: Vec<(PathBuf, Vec<u8>)>) {
    let mut files = store();
    for (path, data) in entries {
        files.insert(path, data);
    }
}

/// An in-memory file handle. Writes go to a local buffer and are published to
/// the global store on flush/shutdown (mirrors needing `flush().await` with
/// real files).
pub struct File {
    path: PathBuf,
    buf: Vec<u8>,
    pos: u64,
    writable: bool,
}

impl File {
    pub async fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            buf: Vec::new(),
            pos: 0,
            writable: true,
        })
    }

    pub async fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let buf = store().get(path).cloned().ok_or_else(|| not_found(path))?;
        Ok(Self {
            path: path.to_path_buf(),
            buf,
            pos: 0,
            writable: false,
        })
    }

    fn publish(&self) {
        if self.writable {
            store().insert(self.path.clone(), self.buf.clone());
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        self.publish();
    }
}

impl AsyncRead for File {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let pos = usize::try_from(self.pos).unwrap_or(usize::MAX);
        if pos < self.buf.len() {
            let n = buf.remaining().min(self.buf.len() - pos);
            buf.put_slice(&self.buf[pos..pos + n]);
            self.pos += n as u64;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for File {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        let pos = usize::try_from(self.pos).unwrap_or(usize::MAX);
        let end = pos + data.len();
        if self.buf.len() < end {
            self.buf.resize(end, 0);
        }
        self.buf[pos..end].copy_from_slice(data);
        self.pos = end as u64;
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.publish();
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.publish();
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for File {
    fn start_seek(mut self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let new_pos = match position {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::End(offset) => self.buf.len() as i128 + i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.pos) + i128::from(offset),
        };
        if new_pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "memfs: seek before start of file",
            ));
        }
        self.pos = u64::try_from(new_pos)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "memfs: seek overflow"))?;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.pos))
    }
}

#[derive(Default, Clone)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    create: bool,
    truncate: bool,
    append: bool,
}

impl OpenOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    pub fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }

    pub async fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let path = path.as_ref();
        let existing = store().get(path).cloned();
        let buf = match existing {
            Some(_) if self.truncate => Vec::new(),
            Some(bytes) => bytes,
            None if self.create && (self.write || self.append) => Vec::new(),
            None => return Err(not_found(path)),
        };
        let pos = if self.append { buf.len() as u64 } else { 0 };
        Ok(File {
            path: path.to_path_buf(),
            buf,
            pos,
            writable: self.write || self.append,
        })
    }
}
