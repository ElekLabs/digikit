//! The one file-sized buffer of the service and the rule of who may use it when.
use crate::dkp::Image;

/// Who holds the buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)] // zero first: the service is a bss static
enum Holder {
    /// The UI task may lend it.
    Nobody,
    /// A write job reads it in the I/O task, from the post to the end of the job.
    WriteJob,
}

/// Owns the buffer that holds a file's bytes: the scratch of a listing probe, of a load and of an archive or copy read, and the bytes that a write job writes. Held by
/// `ProjectService`, which lends it to the owner that needs it for one call. Rule: the UI task lends it only while no write job holds it (`lend` asserts that in a debug
/// build); the I/O task reads it through `job_bytes` while the job holds it.
pub struct FileBuffer {
    image: Image,
    holder: Holder,
}

impl FileBuffer {
    pub const fn new() -> FileBuffer {
        FileBuffer {
            image: Image::new(),
            holder: Holder::Nobody,
        }
    }

    /// Lend the buffer for one call of the UI task. Its content is whatever the last user left.
    pub fn lend(&mut self) -> &mut Image {
        debug_assert!(
            self.holder == Holder::Nobody,
            "the write job holds the buffer"
        );
        &mut self.image
    }

    /// A write job was posted: it reads the buffer until it ends.
    pub fn give_to_job(&mut self) {
        self.holder = Holder::WriteJob;
    }

    /// The write job ended: the UI task may lend the buffer again.
    pub fn take_back(&mut self) {
        self.holder = Holder::Nobody;
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
