use tauri::{
    AppHandle, Emitter,
    menu::{Menu, MenuItemBuilder, PredefinedMenuItem, Submenu},
};

const ACTIONS: &[(&str, &str, &str)] = &[
    ("new", "New Session…", "CmdOrCtrl+N"),
    ("open", "Open Session…", "CmdOrCtrl+O"),
    ("save", "Save Session", "CmdOrCtrl+S"),
    ("save-as", "Save Session As…", "CmdOrCtrl+Shift+S"),
];

fn file_menu(menu: &Menu<tauri::Wry>) -> tauri::Result<Option<Submenu<tauri::Wry>>> {
    Ok(menu.items()?.into_iter().find_map(|item| {
        item.as_submenu()
            .filter(|m| m.text().is_ok_and(|t| t == "File"))
            .cloned()
    }))
}

pub(crate) fn build(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let items = ACTIONS
        .iter()
        .map(|(id, title, keys)| {
            MenuItemBuilder::with_id(format!("session-{id}"), *title)
                .accelerator(*keys)
                .enabled(false)
                .build(app)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    if let Some(file) = file_menu(&menu)? {
        for (i, item) in items.iter().enumerate() {
            file.insert(item, i)?;
        }
        file.insert(&PredefinedMenuItem::separator(app)?, items.len())?;
    } else {
        let file = Submenu::new(app, "File", true)?;
        for item in &items {
            file.append(item)?;
        }
        menu.insert(&file, usize::from(cfg!(target_os = "macos")))?;
    }
    Ok(menu)
}

#[tauri::command]
pub(crate) async fn session_set_actions_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    let menu = app.menu().ok_or("Workbench menu not installed")?;
    let file = file_menu(&menu)
        .map_err(|e| e.to_string())?
        .ok_or("Workbench File menu not installed")?;
    for (id, _, _) in ACTIONS {
        let item = file
            .get(&format!("session-{id}"))
            .ok_or("session action not installed")?;
        item.as_menuitem()
            .ok_or("unexpected session menu item")?
            .set_enabled(enabled)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn dispatch(app: &AppHandle, event: tauri::menu::MenuEvent) {
    if let Some(action) = event.id().as_ref().strip_prefix("session-") {
        let _ = app.emit_to("main", "workbench-session-action", action);
    }
}
