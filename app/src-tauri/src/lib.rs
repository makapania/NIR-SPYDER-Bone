//! SPYDER Bone desktop shell.
//!
//! The shell is deliberately thin: it owns the window, the (macOS) menu, the capabilities and the IPC
//! commands. All numerical work belongs to `spyder-core`, loaded at startup from the bundled `plugins/`
//! (a Tauri resource) plus the user's plug-in folder (`engine`). Opened files and watched-folder arrivals are
//! analysed by the core into one session (`session`, `scoring`) and mapped to the UI's `ScanResult`
//! (`mapping`). Display arrays are computed in Rust (`display`) and cross the IPC boundary as raw
//! little-endian float32 bytes (`tauri::ipc::Response`), never as JSON number arrays.
//!
//! Live folder watching (`live.rs`, on top of the `spyder-watch` crate) delivers settled, complete files as
//! immutable byte snapshots; each is analysed before its arrival event is sent.

pub mod binary;
pub mod commands;
pub mod display;
pub mod engine;
mod live;
pub mod mapping;
pub mod scoring;
pub mod session;

use std::sync::Arc;

use serde::Serialize;

/// Static facts about the running app, shown in About and the status bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    /// True when the plug-ins loaded and a verdict model is available.
    pub core_connected: bool,
    /// The startup-error state in plain words ("No verdict model available: <reason>" in the UI).
    pub core_error: Option<String>,
    /// Registry notes (a user plug-in disabled, a pin fallen back, ...).
    pub plugin_notes: Vec<String>,
    pub bundled_plugins: String,
    pub user_plugins: Option<String>,
}

pub fn app_info_value(core: &engine::Core) -> AppInfo {
    AppInfo {
        name: "SPYDER Bone",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        core_connected: core.error.is_none(),
        core_error: core.error.clone(),
        plugin_notes: core.notes.clone(),
        bundled_plugins: core.bundled_dir.to_string_lossy().into_owned(),
        user_plugins: core
            .user_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
fn app_info(shared: tauri::State<'_, Arc<commands::Shared>>) -> AppInfo {
    app_info_value(&shared.core)
}

/// Load the analysis core: bundled plug-ins (resource folder, or the repository's in development) and the
/// user's plug-in folder (`$SPYDER_USER_PLUGINS_DIR`, else `plugins/` in the app data folder, created empty so
/// users can find it).
fn load_core(app: &tauri::AppHandle) -> engine::Core {
    use tauri::Manager;
    let resource = app.path().resource_dir().ok().map(|d| d.join("plugins"));
    let bundled = engine::bundled_dir(resource);
    let user = match std::env::var_os("SPYDER_USER_PLUGINS_DIR") {
        Some(u) => Some(std::path::PathBuf::from(u)),
        None => live::home_dirs(app)
            .ok()
            .map(|(_, data)| data.join("plugins")),
    };
    if let Some(u) = &user {
        let _ = std::fs::create_dir_all(u);
    }
    let core = engine::Core::load(&bundled, user.as_deref());
    match &core.error {
        Some(e) => eprintln!("SPYDER Bone: no verdict model available: {e}"),
        None => eprintln!("SPYDER Bone: plug-ins loaded from {}", bundled.display()),
    }
    core
}

/// Binary IPC self-test: returns `n` float32 values of a known test signal as raw little-endian
/// bytes. The UI decodes them into a `Float32Array` without copying and checks them against the same
/// formula (see `ui/src/lib/f32.ts`). This is the path every display array will take.
#[tauri::command]
fn ipc_selftest(n: Option<u32>) -> tauri::ipc::Response {
    let n = n.unwrap_or(binary::GRID_N as u32).min(1 << 20) as usize;
    let values = binary::test_signal(n);
    tauri::ipc::Response::new(binary::f32_le_bytes(&values))
}

#[cfg(target_os = "macos")]
fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    use tauri::Emitter;

    let open_file = MenuItemBuilder::with_id("open_file", "Open Files…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    let open_folder = MenuItemBuilder::with_id("open_folder", "Open Folder…")
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;
    let watch_folder = MenuItemBuilder::with_id("watch_folder", "Watch Folder…").build(app)?;
    let help_item = MenuItemBuilder::with_id("help", "SPYDER Bone Help").build(app)?;
    let app_menu = SubmenuBuilder::new(app, "SPYDER Bone")
        .about(None)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&open_file)
        .item(&open_folder)
        .item(&watch_folder)
        .separator()
        .close_window()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .fullscreen()
        .build()?;
    let help = SubmenuBuilder::new(app, "Help").item(&help_item).build()?;
    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &window, &help])
        .build()?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if id == "open_file" || id == "open_folder" || id == "watch_folder" || id == "help" {
            // The UI listens for "menu" and opens the matching native dialog (or the Help dialog).
            let _ = app.emit("menu", id);
        }
    });
    Ok(())
}

/// Laptops first (Matt, 2026-10-03): the design size is 1440 × 900 logical px, but a 1080p laptop at 150 % scaling
/// has about 1280 × 670 to spare and a 1366 × 768 screen less than 900 high. Size the window to the screen it opens
/// on (at most the design size, at least the minimum), centre it, and maximise it when the screen is smaller than
/// the design size, so un-maximising still gives a window that fits.
fn fit_window_to_screen(app: &tauri::App) {
    use tauri::{LogicalSize, Manager};
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let Ok(Some(m)) = w.current_monitor() else {
        return;
    };
    let area = m.work_area().size.to_logical::<f64>(m.scale_factor());
    let width = (area.width * 0.96).clamp(1100.0, 1440.0);
    let height = (area.height * 0.94).clamp(600.0, 900.0);
    let _ = w.set_size(LogicalSize::new(width, height));
    let _ = w.center();
    if area.width < 1440.0 || area.height < 900.0 {
        let _ = w.maximize();
    }
}

/// Builds and runs the app. Windows and Linux get no menu bar: the toolbar carries every action and
/// Ctrl+O / Ctrl+Shift+O are handled in the UI. macOS gets the minimal menu users expect.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            install_menu(app)?;
            fit_window_to_screen(app);
            let shared = Arc::new(commands::Shared::new(load_core(app.handle())));
            let state = live::Live::new(app.handle(), shared.clone())?;
            tauri::Manager::manage(app, shared);
            tauri::Manager::manage(app, state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            ipc_selftest,
            live::watch_probe,
            live::watch_start,
            live::watch_status,
            live::watch_pause,
            live::watch_resume,
            live::watch_stop,
            live::watch_resume_last,
            live::folder_settings_set,
            live::settings_get,
            live::snapshot_bytes,
            commands::session_get,
            commands::session_set_class,
            commands::session_scans,
            commands::session_open,
            commands::scan_views,
            commands::display_info,
            commands::reference_meta,
            commands::reference_views,
            commands::export_csv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running SPYDER Bone");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_is_honest_about_the_core() {
        let core = engine::Core::load(&engine::bundled_dir(None), None);
        let info = app_info_value(&core);
        assert_eq!(info.name, "SPYDER Bone");
        assert!(info.core_connected, "{:?}", info.core_error);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"coreConnected\":true"));
        let missing = engine::Core::load(std::path::Path::new("no/such/plugins"), None);
        let info = app_info_value(&missing);
        assert!(!info.core_connected);
        assert!(info.core_error.unwrap().contains("missing"));
    }
}
