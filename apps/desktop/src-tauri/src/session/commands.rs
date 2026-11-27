//! Desktop dialog and controller adapters for session bundles.
use super::{
    bundle::{Create, OpenSession, WorkingCard, attach, create, mount, read, save},
    document::{MANIFEST, asset, atomic, problem},
};
use digi_control::{cards::PRIVATE_CARD_MARKER, firmware::sha256_hex};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tempfile::TempDir;

#[derive(Default)]
pub(crate) struct Mounts(Mutex<Vec<TempDir>>);

async fn run<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(problem)?
}

#[tauri::command]
pub(crate) async fn session_pick(
    app: AppHandle,
    kind: String,
    name: Option<String>,
) -> Result<Option<PathBuf>, String> {
    let dialog = app.dialog().file();
    let pick = match kind.as_str() {
        "open" => {
            run(move || {
                Ok(dialog
                    .set_title("Open Workbench session.json")
                    .add_filter("Session", &["json"])
                    .blocking_pick_file())
            })
            .await?
        }
        "image" => {
            run(move || {
                Ok(dialog
                    .set_title("Choose card image")
                    .add_filter("Card image", &["img"])
                    .blocking_pick_file())
            })
            .await?
        }
        "folder" => {
            run(move || {
                Ok(dialog
                    .set_title("Choose +Drive folder")
                    .blocking_pick_folder())
            })
            .await?
        }
        "attachment" => {
            run(move || {
                Ok(dialog
                    .set_title("Attach a capture, WAV or comparison")
                    .add_filter("Capture, WAV or comparison", &["json", "wav"])
                    .blocking_pick_file())
            })
            .await?
        }
        "save" => {
            let name = name
                .unwrap_or_else(|| "Untitled Session".into())
                .replace(['/', '\\', ':'], "-");
            let mut dialog = dialog
                .set_title("Create session folder")
                .set_file_name(name);
            if let Ok(music) = app.path().audio_dir() {
                let dir = music.join("Digikit Sessions");
                if fs::create_dir_all(&dir).is_ok() {
                    dialog = dialog.set_directory(dir);
                }
            }
            run(move || Ok(dialog.blocking_save_file())).await?
        }
        _ => return Err("unknown session picker".into()),
    };
    pick.map(|p| p.into_path().map_err(problem)).transpose()
}

#[tauri::command]
pub(crate) async fn session_open(file: PathBuf) -> Result<OpenSession, String> {
    run(move || {
        let root = if file.is_dir() {
            &file
        } else if file.file_name().is_some_and(|n| n == MANIFEST) {
            file.parent()
                .ok_or("the session file needs a parent folder")?
        } else {
            return Err(
                "select session.json to open a session, or use Open capture for a recording".into(),
            );
        };
        read(root)
    })
    .await
}

#[tauri::command]
pub(crate) async fn session_create(
    host: State<'_, crate::Host>,
    input: Create,
) -> Result<OpenSession, String> {
    let handle = host.handle.clone();
    run(move || create(&handle, input)).await
}

#[tauri::command]
pub(crate) async fn session_mount(
    app: AppHandle,
    root: PathBuf,
    firmware: Option<PathBuf>,
) -> Result<(OpenSession, PathBuf), String> {
    run(move || {
        let mut session = read(&root)?;
        let doc = &mut session.document;
        let path = firmware.unwrap_or_else(|| doc.firmware.path.clone());
        let source = fs::read(&path).map_err(|_| "firmware not found; choose Locate firmware")?;
        if sha256_hex(&source) != doc.firmware.sha256 {
            return Err(
                "this is not the firmware saved in this session; choose Locate firmware".into(),
            );
        }
        if path != doc.firmware.path {
            doc.firmware.path = path.canonicalize().map_err(problem)?;
            doc.publish(&session.root)?;
        }
        let (dir, image) = mount(&session)?;
        app.state::<Mounts>().0.lock().map_err(problem)?.push(dir);
        Ok((session, image))
    })
    .await
}

#[tauri::command]
pub(crate) async fn session_release(
    app: AppHandle,
    host: State<'_, crate::Host>,
    card: PathBuf,
) -> Result<(), String> {
    let handle = host.handle.clone();
    run(move || {
        if handle.path().ok().as_ref() == Some(&card) {
            return Err("cannot release a card that is still mounted".into());
        }
        app.state::<Mounts>()
            .0
            .lock()
            .map_err(problem)?
            .retain(|dir| card.parent() != Some(dir.path()));
        Ok(())
    })
    .await
}

#[tauri::command]
pub(crate) async fn session_save(
    host: State<'_, crate::Host>,
    root: PathBuf,
    card: Option<PathBuf>,
    name: String,
) -> Result<OpenSession, String> {
    let handle = host.handle.clone();
    run(move || save(&handle, &root, card.as_deref(), &name)).await
}

#[tauri::command]
pub(crate) async fn session_preserve(
    host: State<'_, crate::Host>,
    card: PathBuf,
) -> Result<(), String> {
    let handle = host.handle.clone();
    run(move || {
        let dir = card.parent().ok_or("no working card folder")?;
        if !dir.join(PRIVATE_CARD_MARKER).is_file() {
            return Err("not a session mount".into());
        }
        let bytes = handle.read(&card)?.into_bytes();
        atomic(dir, "overlay.bin", &bytes)?;
        atomic(dir, PRIVATE_CARD_MARKER, b"used")
    })
    .await
}

#[tauri::command]
pub(crate) async fn session_attach(root: PathBuf, source: PathBuf) -> Result<OpenSession, String> {
    run(move || attach(root, source)).await
}

#[tauri::command]
pub(crate) async fn session_read_capture(root: PathBuf, path: String) -> Result<String, String> {
    run(move || {
        let session = read(&root)?;
        if !session
            .document
            .attachments
            .iter()
            .any(|a| a.path == path && a.kind == "capture")
        {
            return Err("not a capture attached to this session".into());
        }
        fs::read_to_string(asset(&session.root, &path)?).map_err(problem)
    })
    .await
}

#[tauri::command]
pub(crate) async fn session_open_attachment(
    app: AppHandle,
    root: PathBuf,
    path: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let file = run(move || {
        let session = read(&root)?;
        if !session
            .document
            .attachments
            .iter()
            .any(|a| a.path == path && matches!(a.kind.as_str(), "audio" | "comparison"))
        {
            return Err("not an audio artifact attached to this session".into());
        }
        asset(&session.root, &path)
    })
    .await?;
    app.opener()
        .open_path(file.display().to_string(), None::<&str>)
        .map_err(problem)
}
