//! Horizon Porter: the desktop app. Drop a mod's source folder or jar on the window,
//! read the report, and port it to a Horizon native mod.
//!
//! The page in `ui/` calls the commands below; file dialogs and opening folders run
//! here in Rust, so the page needs no plugin permissions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;

use std::fmt::Display;
use std::path::PathBuf;
use std::sync::Mutex;

use porter_model::{InputKind, ModScan};
use porter_verify::{Launcher, Trust};
use serde::Serialize;
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

/// The mod scanned last.
#[derive(Default)]
struct Current(Mutex<Option<ModScan>>);

/// The mod ported last: its id and the folder it was written to.
#[derive(Default)]
struct Ported(Mutex<Option<(String, PathBuf)>>);

/// The Horizon launcher, once found or chosen, and the instance built last.
#[derive(Default)]
struct Horizon {
    launcher: Mutex<Option<Launcher>>,
    /// The instance built last, and the content mod folder it plays with.
    built: Mutex<Option<(porter_verify::Instance, Option<PathBuf>)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildView {
    instance: String,
    result: String,
}

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
    /// Whether it has a native half to build; a content mod alone just plays.
    native: bool,
    wired: Vec<String>,
    /// Hooks with a helper ready to port into.
    stubbed: Vec<String>,
    unwired: Vec<String>,
    blocks: usize,
    items: usize,
    block_entities: usize,
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
    let bundle = porter_translate::bundle::generate_with(
        &scan,
        &lang,
        porter_map::ledger::horizon_source().as_deref(),
    );
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
    *app.state::<Ported>().0.lock().map_err(message)? = Some((bundle.id.clone(), out.clone()));
    Ok(Some(PortView {
        id: bundle.id.clone(),
        out: out.display().to_string(),
        porting: out.join("PORTING.md").display().to_string(),
        native: bundle.native,
        wired: bundle.wired.keys().map(|h| format!("native_{h}")).collect(),
        stubbed: bundle
            .stubbed
            .keys()
            .map(|h| format!("native_{h}"))
            .collect(),
        unwired: bundle
            .unwired
            .keys()
            .map(|h| format!("native_{h}"))
            .collect(),
        blocks: bundle.content.blocks.len(),
        items: bundle.content.items.len(),
        block_entities: bundle.block_entities.len(),
    }))
}

/// Opens a folder in the file manager, or a file in its default app.
#[tauri::command]
fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().open_path(path, None::<&str>).map_err(message)
}

/// Where the launcher's path is remembered once the player has shown it.
fn launcher_memory(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("launcher-path.txt"))
}

/// The Horizon launcher: remembered, found in its usual places, or shown by the
/// player. None when they cancel.
fn launcher(app: &AppHandle) -> Result<Option<Launcher>, String> {
    let horizon = app.state::<Horizon>();
    if let Some(found) = horizon.launcher.lock().map_err(message)?.clone() {
        return Ok(Some(found));
    }
    let remembered = launcher_memory(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|p| PathBuf::from(p.trim()));
    let found = remembered
        .and_then(|p| Launcher::at(&p).ok())
        .or_else(|| Launcher::find(None).ok());
    let launcher = match found {
        Some(l) => l,
        None => {
            let show = app
                .dialog()
                .message("Horizon Porter could not find the Horizon launcher. Show it where the launcher is?")
                .title("Find the Horizon launcher")
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Choose the launcher".into(),
                    "Cancel".into(),
                ))
                .blocking_show();
            if !show {
                return Ok(None);
            }
            let Some(file) = app
                .dialog()
                .file()
                .set_title("Choose horizon-launcher")
                .blocking_pick_file()
            else {
                return Ok(None);
            };
            let path = file.into_path().map_err(message)?;
            let launcher = Launcher::at(&path).map_err(|e| format!("{e:#}"))?;
            if let Some(memory) = launcher_memory(app) {
                if let Some(dir) = memory.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(memory, path.display().to_string());
            }
            launcher
        }
    };
    *horizon.launcher.lock().map_err(message)? = Some(launcher.clone());
    Ok(Some(launcher))
}

/// Builds the mod ported last into a "<id> port" instance with the player's
/// launcher, after the player says they trust its code. Progress arrives as
/// "build-progress" events.
#[tauri::command]
async fn build(app: AppHandle, ported: State<'_, Ported>) -> Result<Option<BuildView>, String> {
    let (id, out) = ported
        .0
        .lock()
        .map_err(message)?
        .clone()
        .ok_or_else(|| "Port a mod first".to_string())?;
    let port = porter_verify::Port::read(&out).map_err(|e| format!("{e:#}"))?;
    let Some(folder) = port.native.clone() else {
        // A content mod alone: nothing to build or trust, it plays as it is.
        let Some(launcher) = launcher(&app)? else {
            return Ok(None);
        };
        let instance = tauri::async_runtime::spawn_blocking(move || {
            launcher.ensure_instance(&format!("{id} port"))
        })
        .await
        .map_err(message)?
        .map_err(|e| format!("{e:#}"))?;
        let name = instance.name.clone();
        *app.state::<Horizon>().built.lock().map_err(message)? = Some((instance, port.content));
        return Ok(Some(BuildView {
            instance: name,
            result: "A content mod: nothing to build. Play loads it from content/.".into(),
        }));
    };
    // Native code is the player's decision: ask every time, in plain words.
    let trusted = app
        .dialog()
        .message(format!(
            "Building compiles the Rust code in {} into Horizon. Native mods are not sandboxed: \
             the code runs with your rights on this computer. Build only code you have read and trust.",
            out.join("mixins").join(&id).display()
        ))
        .title("Trust this mod's code")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "I trust it: build".into(),
            "Cancel".into(),
        ))
        .blocking_show();
    if !trusted {
        return Ok(None);
    }
    let Some(launcher) = launcher(&app)? else {
        return Ok(None);
    };
    let events = app.clone();
    let (instance, result) = tauri::async_runtime::spawn_blocking(move || {
        let instance = launcher.ensure_instance(&format!("{id} port"))?;
        let _ = events.emit(
            "build-progress",
            format!(
                "Building into \"{}\" with {}",
                instance.name, launcher.version
            ),
        );
        let result =
            launcher.native_build(&folder, &instance, Trust::given_by_player(), |line| {
                let _ = events.emit("build-progress", line.to_string());
            })?;
        anyhow::Ok((instance, result))
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))?;
    let name = instance.name.clone();
    *app.state::<Horizon>().built.lock().map_err(message)? = Some((instance, port.content));
    Ok(Some(BuildView {
        instance: name,
        result,
    }))
}

/// Starts the instance built last, with the port's content mod loaded.
#[tauri::command]
fn play(app: AppHandle) -> Result<(), String> {
    let (instance, content) = app
        .state::<Horizon>()
        .built
        .lock()
        .map_err(message)?
        .clone()
        .ok_or_else(|| "Build the mod first".to_string())?;
    let launcher = launcher(&app)?.ok_or_else(|| "No Horizon launcher".to_string())?;
    let dev_mods: Vec<&std::path::Path> = content.as_deref().into_iter().collect();
    launcher
        .play(&instance, &dev_mods)
        .map_err(|e| format!("{e:#}"))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Current::default())
        .manage(Ported::default())
        .manage(Horizon::default())
        .invoke_handler(tauri::generate_handler![
            scan,
            pick,
            save_report,
            port,
            open_path,
            build,
            play,
            ai::ai_settings,
            ai::save_choice,
            ai::save_key,
            ai::forget_key,
            ai::local_models,
            ai::translate_estimate,
            ai::translate
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
