//! The module pages' save of files: the folder comes from the folder dialog (`local_pick_folder`), the raw body is the packed list (per file a
//! 4-byte big-endian name length, the name, a 4-byte big-endian size, the bytes) and `x-folder` (percent-encoded) the folder. A file of the same
//! name in the folder is overwritten: the user has chosen the folder just before. Every name is checked before the first file is written.
use crate::firmware::{body, unescape};
use std::path::Path;

/// A plain file name: not empty, no path separator, no `..`, no NUL.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && !name.contains("..") && !name.contains(['/', '\\', '\0'])
}

/// The files of a packed list, or why the list is malformed or holds a name that could leave the folder.
fn unpack(packed: &[u8]) -> Result<Vec<(String, &[u8])>, String> {
    fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], String> {
        if rest.len() < n {
            return Err("the file list is cut short".into());
        }
        let (head, tail) = rest.split_at(n);
        *rest = tail;
        Ok(head)
    }
    fn length(rest: &mut &[u8]) -> Result<usize, String> {
        let b = take(rest, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    let mut rest = packed;
    let mut files = Vec::new();
    while !rest.is_empty() {
        let n = length(&mut rest)?;
        let name = std::str::from_utf8(take(&mut rest, n)?)
            .map_err(|_| "a file name is not UTF-8".to_string())?
            .to_string();
        if !plain_name(&name) {
            return Err(format!("not a plain file name: {name}"));
        }
        let size = length(&mut rest)?;
        files.push((name, take(&mut rest, size)?));
    }
    Ok(files)
}

/// Writes the files into `folder` (which must exist); the number written.
fn write_all(folder: &Path, packed: &[u8]) -> Result<u32, String> {
    if !folder.is_dir() {
        return Err(format!("{}: not a folder", folder.display()));
    }
    let files = unpack(packed)?;
    for (name, bytes) in &files {
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(files.len() as u32)
}

/// Writes the packed list in the raw body into the folder of header `x-folder`; the number of files written.
#[tauri::command]
pub async fn files_save_to_folder(request: tauri::ipc::Request<'_>) -> Result<u32, String> {
    let packed = body(&request)?;
    let folder = request
        .headers()
        .get("x-folder")
        .and_then(|v| v.to_str().ok())
        .map(unescape)
        .ok_or("no folder given")?;
    tauri::async_runtime::spawn_blocking(move || write_all(Path::new(&folder), &packed))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
