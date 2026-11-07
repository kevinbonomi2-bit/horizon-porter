//! Adding a ported mod to one of the player's own Horizon instances, beside the
//! mods already there: its packaged files go in the instance's `horizon-mods/` and
//! `horizon-native-mods/`, it is turned on, and the instance's native mods are built
//! together (after the player says they trust the code).

use porter_verify::{Instance, Trust};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::{Ported, launcher, message};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetView {
    id: String,
    name: String,
    /// The one the launcher's Play starts.
    selected: bool,
}

/// The player's Horizon instances (Java Edition ones left out).
#[tauri::command]
pub async fn install_targets(app: AppHandle) -> Result<Vec<TargetView>, String> {
    let Some(launcher) = launcher(&app)? else {
        return Err("Horizon Porter needs the Horizon launcher".into());
    };
    let list = tauri::async_runtime::spawn_blocking(move || launcher.instances())
        .await
        .map_err(message)?
        .map_err(|e| format!("{e:#}"))?;
    Ok(list
        .into_iter()
        .filter(|i| i.plays_horizon() && i.home.is_some())
        // The porter's own test instances ("<id> port") go last.
        .map(|i| TargetView {
            id: i.id,
            name: i.name,
            selected: i.selected,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    instance: String,
    instance_id: String,
    /// It has a native half: the instance needs a build before it plays.
    needs_build: bool,
}

async fn find_instance(app: &AppHandle, id: &str) -> Result<Instance, String> {
    let launcher = launcher(app)?.ok_or("No Horizon launcher")?;
    let id = id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        launcher
            .instances()?
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| anyhow::anyhow!("that instance is gone"))
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))
}

/// Packages the mod ported last and installs it into the instance `id`.
#[tauri::command]
pub async fn install_port(
    app: AppHandle,
    ported: State<'_, Ported>,
    id: String,
) -> Result<InstalledView, String> {
    let (mod_id, dir) = ported
        .0
        .lock()
        .map_err(message)?
        .clone()
        .ok_or_else(|| "Port a mod first".to_string())?;
    let instance = find_instance(&app, &id).await?;
    let home = instance
        .home
        .clone()
        .ok_or("That instance has no game folder")?;
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let dist = dir.join(".porter").join("dist");
        let files = porter_translate::bundle::package(&dir, &dist)?;
        let pick = |ext: &str| {
            files
                .iter()
                .find(|f| f.extension().is_some_and(|e| e == ext))
                .cloned()
        };
        porter_verify::install(
            &home,
            &mod_id,
            pick("hmod").as_deref(),
            pick("hnmod").as_deref(),
        )
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))?;
    Ok(InstalledView {
        instance: instance.name,
        instance_id: instance.id,
        needs_build: installed.native.is_some(),
    })
}

/// Builds every native mod in the instance `id` together, after the player says
/// they trust the code: `trusted` when they just did, on the page's "I trust it"
/// button; else the app asks. Progress arrives as "build-progress" events.
#[tauri::command]
pub async fn build_installed(
    app: AppHandle,
    ported: State<'_, Ported>,
    id: String,
    trusted: Option<bool>,
) -> Result<Option<String>, String> {
    // The mod ported last: its translated code the build refuses is put back, then it builds again.
    let port = ported.0.lock().map_err(message)?.clone();
    let instance = find_instance(&app, &id).await?;
    let trusted = trusted == Some(true) || app
        .dialog()
        .message(format!(
            "Building compiles the Rust code of every native mod in \"{}\" into Horizon, this port's \
             included. Native mods are not sandboxed: the code runs with your rights on this computer. \
             Build only code you have read and trust. The instance then plays that build.",
            instance.name
        ))
        .title("Trust these mods' code")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "I trust them: build".into(),
            "Cancel".into(),
        ))
        .blocking_show();
    if !trusted {
        return Ok(None);
    }
    let launcher = launcher(&app)?.ok_or("No Horizon launcher")?;
    let events = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _ = events.emit(
            "build-progress",
            format!("Building \"{}\" with {}", instance.name, launcher.version),
        );
        let home = instance.home.clone();
        let repair = |log: &str, tree: &std::path::Path| -> anyhow::Result<Vec<String>> {
            let (Some((mod_id, dir)), Some(home)) = (&port, &home) else {
                return Ok(Vec::new());
            };
            let done = porter_translate::refused::repair(dir, log, tree)?;
            if !done.is_empty() {
                let files =
                    porter_translate::bundle::package(dir, &dir.join(".porter").join("dist"))?;
                let hnmod = files
                    .iter()
                    .find(|f| f.extension().is_some_and(|e| e == "hnmod"));
                porter_verify::install(home, mod_id, None, hnmod.map(|p| p.as_path()))?;
            }
            Ok(done)
        };
        let (last, put_back) = launcher.build_instance_repairing(
            &instance,
            Trust::given_by_player(),
            repair,
            |line| {
                let _ = events.emit("build-progress", line.to_string());
            },
        )?;
        Ok::<_, anyhow::Error>(if put_back.is_empty() {
            last
        } else {
            format!(
                "{last}
{} translated function(s) the build refused are back to their stubs: the mod plays without them.",
                put_back.len()
            )
        })
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))?;
    Ok(Some(result))
}

/// Plays the port in the instance `id` (installed and built there) and says what
/// doesn't work: a test world opens, each of its blocks is placed and those that act
/// are right-clicked. Progress arrives as "check-progress" events.
#[tauri::command]
pub async fn check_installed(
    app: AppHandle,
    ported: State<'_, Ported>,
    id: String,
) -> Result<porter_verify::play::PlayReport, String> {
    let (_, dir) = ported
        .0
        .lock()
        .map_err(message)?
        .clone()
        .ok_or_else(|| "Port a mod first".to_string())?;
    // Found again: a build just changed the version it plays.
    let instance = find_instance(&app, &id).await?;
    let events = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        porter_verify::play::play_check(
            &instance,
            &dir,
            &porter_verify::play::PlayOptions::default(),
            |line| {
                let _ = events.emit("check-progress", line.to_string());
            },
        )
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))
}

/// Starts the instance `id`, its mods installed (nothing loaded from a folder).
#[tauri::command]
pub async fn play_installed(app: AppHandle, id: String) -> Result<(), String> {
    let instance = find_instance(&app, &id).await?;
    let launcher = launcher(&app)?.ok_or("No Horizon launcher")?;
    launcher.play(&instance, &[]).map_err(|e| format!("{e:#}"))
}
