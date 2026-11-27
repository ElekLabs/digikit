//! The file a project's image came from or was last written to.
use crate::dkp::Head;
use crate::name::Path;

/// The file a bound project came from or was last written to: its header fields and its path.
#[derive(Clone, Copy)]
pub struct SourceFile {
    pub head: Head,
    pub path: Path,
}
