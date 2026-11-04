//! Horizon Porter: the desktop app. Drop a mod's source folder or jar on the window,
//! read the report, and port it to a Horizon native mod.
//!
//! The page in `ui/` calls the commands below; file dialogs and opening folders run
//! here in Rust, so the page needs no plugin permissions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Mutex;

use porter_model::{InputKind, ModScan};
use serde::Serialize;
use tauri::{AppHandle, DragDropEvent, Emitter, State, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

/// The mod scanned last.
#[derive(Default)]
struct Current(Mutex<Option<ModScan>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanView {
    title: String,
    facts: Vec<String>,
    input: String,
    is_jar: bool,
    /// The report page, without its own header (the app shows that).
    report: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortView {
    id: String,
    out: String,
    porting: String,
    wired: Vec<String>,
    unwired: Vec<String>,
}

fn message(e: impl Display) -> String {
    format!("{e:#}")
}

#[tauri::command]
async fn scan(path: String, current: State<'_, Current>) -> Result<ScanView, String> {
    let input = PathBuf::from(&path);
    let scan = tauri::async_runtime::spawn_blocking(move || porter_java::scan(&input))
        .await
        .map_err(message)?
        .map_err(|e| format!("{e:#}"))?;
    let (title, facts) = porter_report::heading(&scan);
    let view = ScanView {
        title,
        facts,
        input: scan.input.clone(),
        is_jar: scan.input_kind == InputKind::Jar,
        report: porter_report::html_embedded(&scan),
    };
    *current.0.lock().map_err(message)? = Some(scan);
    Ok(view)
}

/// A native picker for a mod's source folder, or for a jar.
#[tauri::command]
async fn pick(app: AppHandle, jar: bool) -> Option<String> {
    let dialog = app.dialog().file();
    let picked = if jar {
        dialog
            .set_title("Choose a mod jar")
            .add_filter("Mod jar", &["jar"])
            .blocking_pick_file()
    } else {
        dialog
            .set_title("Choose a mod's source folder")
            .blocking_pick_folder()
    };
    picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

fn mod_id(scan: &ModScan) -> String {
    scan.metadata
        .iter()
        .find_map(|m| m.id.clone())
        .unwrap_or_else(|| "mod".into())
}

#[tauri::command]
async fn save_report(
    app: AppHandle,
    current: State<'_, Current>,
) -> Result<Option<String>, String> {
    let (html, name) = {
        let guard = current.0.lock().map_err(message)?;
        let scan = guard
            .as_ref()
            .ok_or_else(|| "Scan a mod first".to_string())?;
        (
            porter_report::html(scan),
            format!("{}-report.html", mod_id(scan)),
        )
    };
    let Some(file) = app
        .dialog()
        .file()
        .set_title("Save the report")
        .set_file_name(&name)
        .add_filter("HTML page", &["html"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = file.into_path().map_err(message)?;
    std::fs::write(&path, html).map_err(message)?;
    Ok(Some(path.display().to_string()))
}

/// Writes the Horizon mod into `<chosen folder>/<id>-horizon`, asking before it
/// replaces generated files in a folder that already has files.
#[tauri::command]
async fn port(app: AppHandle, current: State<'_, Current>) -> Result<Option<PortView>, String> {
    let scan = current
        .0
        .lock()
        .map_err(message)?
        .clone()
        .ok_or_else(|| "Scan a mod first".to_string())?;
    if scan.input_kind == InputKind::Jar {
        return Err(
            "Porting needs the mod's source folder. A jar can be scanned, not ported.".into(),
        );
    }
    let Some(dir) = app
        .dialog()
        .file()
        .set_title("Choose where to put the Horizon mod")
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let parent = dir.into_path().map_err(message)?;
    let lang = porter_translate::bundle::load_lang(&scan);
    let bundle = porter_translate::bundle::generate(&scan, &lang);
    let out = parent.join(format!("{}-horizon", bundle.id));
    let has_files = std::fs::read_dir(&out)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);
    if has_files {
        let replace = app
            .dialog()
            .message(format!(
                "{} already has files. Replace the files Horizon Porter generates there? Your other files stay.",
                out.display()
            ))
            .title("Replace the generated files")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Replace".into(),
                "Cancel".into(),
            ))
            .blocking_show();
        if !replace {
            return Ok(None);
        }
    }
    porter_translate::bundle::write(&bundle, &out, has_files).map_err(|e| format!("{e:#}"))?;
    Ok(Some(PortView {
        id: bundle.id.clone(),
        out: out.display().to_string(),
        porting: out.join("PORTING.md").display().to_string(),
        wired: bundle.wired.keys().map(|h| format!("native_{h}")).collect(),
        unwired: bundle
            .unwired
            .keys()
            .map(|h| format!("native_{h}"))
            .collect(),
    }))
}

/// Opens a folder in the file manager, or a file in its default app.
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(message)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Current::default())
        .invoke_handler(tauri::generate_handler![
            scan,
            pick,
            save_report,
            port,
            open_path
        ])
        .on_window_event(|window, event| {
            // Drops are handled natively and forwarded to the page as paths.
            if let WindowEvent::DragDrop(drop) = event {
                let _ = match drop {
                    DragDropEvent::Enter { .. } => window.emit("drop-hover", true),
                    DragDropEvent::Leave => window.emit("drop-hover", false),
                    DragDropEvent::Drop { paths, .. } => window.emit(
                        "dropped",
                        paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>(),
                    ),
                    _ => Ok(()),
                };
            }
        })
        .run(tauri::generate_context!())
        .expect("Horizon Porter could not start");
}
