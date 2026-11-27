//! A content module that is loaded.
use crate::engine::restore::*;

/// A content module that is loaded: its id and the store path it was loaded from.
#[derive(Clone, Copy)]
pub struct LoadedModule {
    pub id: ModuleId,
    pub path: ModulePath,
}
