//! Translation in the app: who translates, the Claude API key, and the
//! `translate` step with live progress.
//!
//! The key is kept in the system's credential store (Windows Credential Manager,
//! the macOS Keychain, the Secret Service on Linux), never in a file, and never
//! goes back to the page: the page only learns whether one is saved. It is sent
//! only to api.anthropic.com, by porter-translate.

use std::path::{Path, PathBuf};

use porter_translate::ai::{self, Status, model};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{Current, Ported, message};

/// Where the credential store keeps the key.
const KEY_SERVICE: &str = "io.github.kevinbonomi2bit.horizonporter";
const KEY_USER: &str = "anthropic-api-key";

/// The choices the page offers.
pub const MODELS: &[(&str, &str, &str)] = &[
    (
        "claude-haiku-5-5",
        "Claude Haiku 5.5",
        "The cheapest: about $0.20 for a mod the size of Create",
    ),
    (
        "claude-sonnet-5-5",
        "Claude Sonnet 5.5",
        "Stronger, about 20 times Haiku's price",
    ),
    (
        "claude-opus-5-5",
        "Claude Opus 5.5",
        "The strongest, about 40 times Haiku's price",
    ),
];

/// Who translates, as saved in the app's settings (no secrets here).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// `anthropic` or `ollama`.
    pub provider: String,
    pub model: String,
}

impl Default for Choice {
    fn default() -> Self {
        Choice {
            provider: "anthropic".into(),
            model: model::DEFAULT_MODEL.into(),
        }
    }
}

fn choice_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("translation.json"))
}

fn load_choice(app: &AppHandle) -> Choice {
    choice_file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEY_SERVICE, KEY_USER).map_err(message)
}

/// The saved key, if any.
fn saved_key() -> Option<String> {
    key_entry()
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

/// The name of this system's credential store, for the page.
fn store_name() -> &'static str {
    if cfg!(windows) {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else {
        "the system keyring"
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    id: String,
    name: String,
    note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    choice: Choice,
    models: Vec<ModelView>,
    /// A key is saved in the credential store (the key itself never comes here).
    has_key: bool,
    /// ANTHROPIC_API_KEY is set, which is used when no key is saved.
    env_key: bool,
    store: &'static str,
}

#[tauri::command]
pub fn ai_settings(app: AppHandle) -> SettingsView {
    SettingsView {
        choice: load_choice(&app),
        models: MODELS
            .iter()
            .map(|(id, name, note)| ModelView {
                id: (*id).into(),
                name: (*name).into(),
                note: (*note).into(),
            })
            .collect(),
        has_key: saved_key().is_some(),
        env_key: std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty()),
        store: store_name(),
    }
}

#[tauri::command]
pub fn save_choice(app: AppHandle, choice: Choice) -> Result<(), String> {
    if !["anthropic", "ollama"].contains(&choice.provider.as_str())
        || choice.model.trim().is_empty()
    {
        return Err("Choose a model".into());
    }
    let file = choice_file(&app).ok_or("No settings folder")?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(message)?;
    }
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&choice).map_err(message)?,
    )
    .map_err(message)
}

/// Saves the key in the credential store. The page clears its field after this.
#[tauri::command]
pub fn save_key(key: String) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) || key.len() > 512 {
        return Err("That doesn't look like an API key".into());
    }
    key_entry()?.set_password(key).map_err(message)
}

#[tauri::command]
pub fn forget_key() -> Result<(), String> {
    match key_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(message(e)),
    }
}

/// The models installed in the Ollama on this computer.
#[tauri::command]
pub async fn local_models() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(model::local_models)
        .await
        .map_err(message)?
        .map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateView {
    functions: usize,
    tokens: usize,
    /// None for a local model (free) or an unknown price.
    dollars: Option<f64>,
    model: String,
    local: bool,
    /// What is missing before it can start (a key, Ollama), if anything.
    blocker: Option<String>,
}

/// The Java scan of the ported mod: the one read last if it is the same, else read again.
fn ported_scan(dir: &Path, current: &State<'_, Current>) -> Result<porter_model::ModScan, String> {
    let info: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(".porter").join("port.json"))
            .map_err(|_| "Port the mod again with this version of Horizon Porter".to_string())?,
    )
    .map_err(message)?;
    let input = info["input"].as_str().unwrap_or_default().to_string();
    if let Some(scan) = current.0.lock().map_err(message)?.as_ref() {
        let same = std::path::absolute(&scan.input).ok() == std::path::absolute(&input).ok();
        if same {
            return Ok(scan.clone());
        }
    }
    porter_java::scan(Path::new(&input)).map_err(|e| format!("{e:#}"))
}

fn ported_dir(ported: &State<'_, Ported>) -> Result<PathBuf, String> {
    ported
        .0
        .lock()
        .map_err(message)?
        .as_ref()
        .map(|(_, dir)| dir.clone())
        .ok_or_else(|| "Port a mod first".to_string())
}

#[tauri::command]
pub async fn translate_estimate(
    app: AppHandle,
    current: State<'_, Current>,
    ported: State<'_, Ported>,
) -> Result<EstimateView, String> {
    let dir = ported_dir(&ported)?;
    let scan = ported_scan(&dir, &current)?;
    let choice = load_choice(&app);
    let plan = tauri::async_runtime::spawn_blocking(move || {
        ai::plan(
            &dir,
            &scan,
            &ai::Options {
                refresh: true,
                ..ai::Options::default()
            },
        )
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))?;
    let local = choice.provider == "ollama";
    let blocker = if local {
        // Ollama answers only when it runs here and has the model.
        let model = choice.model.clone();
        match tauri::async_runtime::spawn_blocking(model::local_models)
            .await
            .map_err(message)?
        {
            Err(_) => Some("Ollama isn't running on this computer".to_string()),
            Ok(models) if !models.contains(&model) => {
                Some(format!("Ollama doesn't have the model {model}"))
            }
            Ok(_) => None,
        }
    } else if saved_key().is_none()
        && !std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.is_empty())
    {
        Some("No Claude API key is saved".to_string())
    } else {
        None
    };
    Ok(EstimateView {
        functions: plan.len(),
        tokens: ai::planned_tokens(&plan),
        dollars: if local {
            None
        } else {
            ai::planned_dollars(&plan, &choice.model)
        },
        model: if local {
            format!("{} (local)", choice.model)
        } else {
            MODELS
                .iter()
                .find(|(id, ..)| *id == choice.model)
                .map_or(choice.model.clone(), |(_, name, _)| (*name).to_string())
        },
        local,
        blocker,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateView {
    /// Why the model gave no answer, when it stopped for that.
    stopped: Option<String>,
    /// Functions still left to port after this run.
    left: usize,
    ported: usize,
    unchecked: usize,
    failed: Vec<String>,
    input_tokens: u64,
    output_tokens: u64,
}

/// Translates what is left in the mod ported last; progress arrives as
/// "translate-progress" events.
#[tauri::command]
pub async fn translate(
    app: AppHandle,
    current: State<'_, Current>,
    ported: State<'_, Ported>,
) -> Result<TranslateView, String> {
    let dir = ported_dir(&ported)?;
    let scan = ported_scan(&dir, &current)?;
    let choice = load_choice(&app);
    let model: Box<dyn model::Model + Send> = if choice.provider == "ollama" {
        Box::new(model::Ollama::new(Some(&choice.model)).map_err(|e| format!("{e:#}"))?)
    } else if let Some(key) = saved_key() {
        Box::new(model::Anthropic::with_key(&key, Some(&choice.model), None))
    } else {
        Box::new(
            model::Anthropic::from_env(Some(&choice.model), None).map_err(|e| format!("{e:#}"))?,
        )
    };
    let events = app.clone();
    let dir_after = dir.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        ai::translate(
            &dir,
            &scan,
            model.as_ref(),
            &ai::Options {
                refresh: true,
                ..ai::Options::default()
            },
            |line| {
                let _ = events.emit("translate-progress", line.to_string());
            },
        )
    })
    .await
    .map_err(message)?
    .map_err(|e| format!("{e:#}"))?;
    let left = ai::pending(&dir_after).map(|p| p.len()).unwrap_or(0);
    // What the ports say they couldn't do, as a file beside the port.
    let _ = ai::write_notes(&dir_after);
    Ok(TranslateView {
        stopped: out.iter().find_map(|o| match &o.status {
            Status::NoAnswer(why) => Some(why.clone()),
            _ => None,
        }),
        left,
        ported: out.iter().filter(|o| o.status == Status::Ported).count(),
        unchecked: out.iter().filter(|o| o.status == Status::Unchecked).count(),
        failed: out
            .iter()
            .filter_map(|o| match &o.status {
                Status::Failed(why) => Some(format!("{}: {why}", o.unit.function)),
                _ => None,
            })
            .collect(),
        input_tokens: out.iter().map(|o| o.input_tokens).sum(),
        output_tokens: out.iter().map(|o| o.output_tokens).sum(),
    })
}
