//! The one file-sized buffer of the service.
use crate::dkp::Image;

/// Owns the buffer that holds a file's bytes: the scratch of a listing probe, of a load and of an archive or copy read, and the bytes that a write job writes. It lives in the
/// [`StoreJob`](super::StoreJob) cell, outside the service's owner, because the I/O task reads it while a job runs. Rule (kept by the cell's phase): the UI task uses it only while
/// no job is posted, and the I/O task only while one is.
pub struct FileBuffer {
    image: Image,
}

impl FileBuffer {
    pub const fn new() -> FileBuffer {
        FileBuffer {
            image: Image::new(),
        }
    }

    /// Lend the buffer for one call of the UI task. Its content is whatever the last user left.
    pub fn lend(&mut self) -> &mut Image {
        &mut self.image
    }

    /// The first `len` bytes, for the write job (I/O task).
    pub fn job_bytes(&self, len: usize) -> &[u8] {
        &self.image.0[..len]
    }
}

impl Default for FileBuffer {
    fn default() -> Self {
        Self::new()
    }
}
