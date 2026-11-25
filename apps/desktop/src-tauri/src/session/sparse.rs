use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

/// Independent copy of a card. APFS can clone in constant time. The fallback
/// never writes empty extents, including on volumes without clone support.
pub(super) fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let from = CString::new(source.as_os_str().as_bytes())?;
        let to = CString::new(destination.as_os_str().as_bytes())?;
        // SAFETY: the two C strings are live and NUL-terminated.
        if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
            return File::open(destination)?.sync_all();
        }
    }
    let input = File::open(source)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    let size = input.metadata()?.len();
    let mut reader = BufReader::new(input);
    let mut offset = 0;
    let mut bytes = vec![0; 128 * 1024];
    while offset < size {
        let n = (size - offset).min(bytes.len() as u64) as usize;
        let part = &mut bytes[..n];
        reader.read_exact(part)?;
        if part.iter().any(|&b| b != 0) {
            output.seek(SeekFrom::Start(offset))?;
            output.write_all(part)?;
        }
        offset += n as u64;
    }
    output.set_len(size)?;
    output.sync_all()
}
