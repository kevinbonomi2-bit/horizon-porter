//! Translation in the app: who translates, the API keys, and the `translate` step
//! with live progress.
//!
//! A key is kept in the system's credential store (Windows Credential Manager,
//! the macOS Keychain, the Secret Service on Linux), never in a file, and never
//! goes back to the page: the page only learns whether one is saved. Claude's is
//! sent only to api.anthropic.com and Gemini's only to
//! generativelanguage.googleapis.com, by porter-translate.

use std::path::{Path, PathBuf};

use porter_translate::ai::{self, Status, model};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{Current, Ported, message};

/// Where the credential store keeps the key.
const KEY_SERVICE: &str = "io.github.kevinbonomi2bit.horizonporter";

/// The name a provider's key is kept under there.
fn key_user(provider: &str) -> &'static str {
    match provider {
        "gemini" => "gemini-api-key",
        _ => "anthropic-api-key",
    }
}

/// The free choice the page offers beside Claude's models: the provider, its model
/// and what the page calls it.
pub const FREE: (&str, &str, &str) = ("gemini", "gemini-3.8-flash", "Gemini 3.8 Flash");

/// The choices the page offers.
pub const MODELS: &[(&str, &str, &str)] = &[
    (
        "claude-haiku-5-5",
        "Claude Haiku 5.5",
        "The cheapest Claude: a few dollars for a mod the size of Create",
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
    /// `anthropic`, `gemini` or `ollama`.
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

fn key_entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEY_SERVICE, key_user(provider)).map_err(message)
}

/// The key saved for `provider`, if any.
fn saved_key(provider: &str) -> Option<String> {
    key_entry(provider)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

/// The environment variable that carries `provider`'s key when none is saved.
fn key_variable(provider: &str) -> &'static str {
    model::service_defaults(provider).map_or("ANTHROPIC_API_KEY", |(variable, _)| variable)
}

/// Whether that variable is set.
fn env_key(provider: &str) -> bool {
    std::env::var(key_variable(provider)).is_ok_and(|k| !k.trim().is_empty())
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
    /// The free choice: its provider, model and name.
    free: ModelView,
    free_provider: &'static str,
    /// A key for the chosen provider is saved in the credential store (the key itself
    /// never comes here).
    has_key: bool,
    /// The chosen provider's environment variable is set, which is used when no key
    /// is saved.
    env_key: bool,
    /// That variable's name.
    key_variable: &'static str,
    store: &'static str,
}

#[tauri::command]
pub fn ai_settings(app: AppHandle) -> SettingsView {
    let choice = load_choice(&app);
    SettingsView {
        has_key: saved_key(&choice.provider).is_some(),
        env_key: env_key(&choice.provider),
        key_variable: key_variable(&choice.provider),
        choice,
        free: ModelView {
            id: FREE.1.into(),
            name: FREE.2.into(),
            note: "Free, within Google's daily limits. Needs a free key from Google.".into(),
        },
        free_provider: FREE.0,
        models: MODELS
            .iter()
            .map(|(id, name, note)| ModelView {
                id: (*id).into(),
                name: (*name).into(),
                note: (*note).into(),
            })
            .collect(),
        store: store_name(),
    }
}

#[tauri::command]
pub fn save_choice(app: AppHandle, choice: Choice) -> Result<(), String> {
    if !["anthropic", "gemini", "ollama"].contains(&choice.provider.as_str())
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

/// Saves the chosen provider's key in the credential store. The page clears its
/// field after this.
#[tauri::command]
pub fn save_key(app: AppHandle, key: String) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.chars().any(char::is_whitespace) || key.len() > 512 {
        return Err("That doesn't look like an API key".into());
    }
    key_entry(&load_choice(&app).provider)?
        .set_password(key)
        .map_err(message)
}

/// Removes the chosen provider's key.
#[tauri::command]
pub fn forget_key(app: AppHandle) -> Result<(), String> {
    match key_entry(&load_choice(&app).provider)?.delete_credential() {
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
    /// None for a local model (free), a free service or an unknown price.
    dollars: Option<f64>,
    model: String,
    local: bool,
    /// A service's free models: free within its limits.
    free: bool,
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
    let free = choice.provider == FREE.0;
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
    } else if saved_key(&choice.provider).is_none() && !env_key(&choice.provider) {
        Some(format!(
            "No {} API key is saved",
            if free { "Gemini" } else { "Claude" }
        ))
    } else {
        None
    };
    Ok(EstimateView {
        functions: plan.len(),
        tokens: ai::planned_tokens(&plan),
        dollars: if local || free {
            None
        } else {
            ai::planned_dollars(&plan, &choice.model)
        },
        model: if local {
            format!("{} (local)", choice.model)
        } else if free && choice.model == FREE.1 {
            FREE.2.to_string()
        } else {
            MODELS
                .iter()
                .find(|(id, ..)| *id == choice.model)
                .map_or(choice.model.clone(), |(_, name, _)| (*name).to_string())
        },
        local,
        free,
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
    let free_tier = choice.provider == FREE.0;
    // (A model on this computer answers one at a time anyway.)
    let free_tier = free_tier || choice.provider == "ollama";
    let model: Box<dyn model::Model + Send> = if choice.provider == "ollama" {
        Box::new(model::Ollama::new(Some(&choice.model)).map_err(|e| format!("{e:#}"))?)
    } else if choice.provider == FREE.0 {
        // The saved key, else the one in the environment.
        let key = saved_key(FREE.0)
            .or_else(|| std::env::var(key_variable(FREE.0)).ok())
            .ok_or("No Gemini API key is saved")?;
        Box::new(
            model::Compatible::with_key(FREE.0, &key, Some(&choice.model), None)
                .map_err(|e| format!("{e:#}"))?,
        )
    } else if let Some(key) = saved_key("anthropic") {
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
                // Several functions asked at once; Gemini's free tier allows few a minute.
                jobs: if free_tier { 2 } else { 4 },
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
