//! The module pages' save of files. The raw body is the packed list (per file a 4-byte big-endian name length, the name, a 4-byte big-endian size,
//! the bytes). The command opens the folder dialog itself and writes into the folder the user picks; the webview never names a folder. A file of the
//! same name in the folder is replaced (the user has chosen the folder just before), except a symbolic link, which is refused. The whole list is
//! checked before the first file is written (names, count, total size, case-insensitive duplicates, targets); each file is written to a temporary
//! file in the folder and renamed, so a failure leaves no half file. The name rule is the one of `@digi/module-ui` (`checkSaveName`); both read
//! `packages/module-ui/tests/fixtures/save-names.json` in their tests.
use crate::firmware::body;
use std::path::{Component, Path};

const MAX_NAME_BYTES: usize = 255;
const MAX_FILES: usize = 256;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// True when the name is exactly one `Normal` path component.
fn single_component(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    )
}

/// The name rule: not empty, at most 255 bytes, none of `/ \ : * ? " < > |`, no control character, not only dots, no trailing dot or space,
/// no Windows reserved device name (with or without an extension), and one `Normal` path component.
fn check_name(name: &str) -> Result<(), String> {
    let bad = || Err(format!("not a plain file name: {name}"));
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return bad();
    }
    if name
        .chars()
        .any(|c| (c as u32) < 0x20 || c as u32 == 0x7f || "/\\:*?\"<>|".contains(c))
    {
        return bad();
    }
    if name.chars().all(|c| c == '.') || name.ends_with(['.', ' ']) {
        return bad();
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        return Err(format!("a reserved device name: {name}"));
    }
    if !single_component(name) {
        return bad();
    }
    Ok(())
}

/// The files of a packed list, or why the list is malformed, holds a bad or repeated name, or is over the limits.
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
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    while !rest.is_empty() {
        let n = length(&mut rest)?;
        let name = std::str::from_utf8(take(&mut rest, n)?)
            .map_err(|_| "a file name is not UTF-8".to_string())?
            .to_string();
        check_name(&name)?;
        if files.len() == MAX_FILES {
            return Err(format!("save at most {MAX_FILES} files at once"));
        }
        let size = length(&mut rest)?;
        total = total.saturating_add(size);
        if total > MAX_TOTAL_BYTES {
            return Err(format!("save at most {MAX_TOTAL_BYTES} bytes at once"));
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(format!("the file name is used twice: {name}"));
        }
        files.push((name, take(&mut rest, size)?));
    }
    Ok(files)
}

/// Writes the files into `folder` (which must exist); the number written. Nothing is written when a name, a limit or a target is refused.
fn write_all(folder: &Path, packed: &[u8]) -> Result<u32, String> {
    if !folder.is_dir() {
        return Err(format!("{}: not a folder", folder.display()));
    }
    let files = unpack(packed)?;
    for (name, _) in &files {
        let path = folder.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("{}: is a symbolic link", path.display()))
            }
            Ok(m) if !m.is_file() => return Err(format!("{}: is not a file", path.display())),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    for (i, (name, bytes)) in files.iter().enumerate() {
        let path = folder.join(name);
        let temp = folder.join(format!(".digikit-save-{}-{i}.tmp", std::process::id()));
        let written = std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, &path));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("{}: {e}", path.display()));
        }
    }
    Ok(files.len() as u32)
}

/// Checks the packed list in the raw body, opens the folder dialog and writes the files into the folder the user picks. Returns the save result
/// (`saved`, `cancelled`, `where`); cancelling the dialog writes nothing.
#[tauri::command]
pub async fn files_save_with_dialog(
    app: tauri::AppHandle,
    request: tauri::ipc::Request<'_>,
) -> Result<serde_json::Value, String> {
    let packed = body(&request)?;
    unpack(&packed)?;
    let picked =
        crate::local::local_pick_folder(app, None, Some("Save the files in this folder".into()))
            .await?;
    let Some(folder) = picked else {
        return Ok(serde_json::json!({ "saved": 0, "cancelled": true }));
    };
    let target = folder.clone();
    let saved =
        tauri::async_runtime::spawn_blocking(move || write_all(Path::new(&target), &packed))
            .await
            .map_err(|e| e.to_string())??;
    Ok(serde_json::json!({ "saved": saved, "cancelled": false, "where": folder }))
}

#[cfg(test)]
mod tests;
