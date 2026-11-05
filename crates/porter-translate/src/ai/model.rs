// The AI models the translator can ask: Claude through the Messages API (raw
// HTTP: Rust has no official Anthropic SDK), Claude Code's `claude` command, or any
// command that reads a prompt on stdin and writes the answer on stdout.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// The default model: Claude Haiku 5.5, the cheapest Claude ($0.10 per million
/// input tokens, $0.50 per million output). The compile-and-fix loop catches what
/// it gets wrong in runtime.rs; `--model claude-sonnet-5-5` or `claude-opus-5-5`
/// for harder mods.
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";

/// Dollars per million input and output tokens, for the estimate.
pub fn price(model: &str) -> Option<(f64, f64)> {
    Some(match model {
        "claude-haiku-5-5" => (0.10, 0.50),
        "claude-sonnet-5-5" | "claude-sonnet-5" => (2.0, 10.0),
        "claude-opus-5-5" => (4.0, 20.0),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" => (5.0, 25.0),
        "claude-fable-5-1" | "claude-fable-5" => (10.0, 50.0),
        _ => return None,
    })
}

/// An answer, with what it cost when the provider says.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Something that answers a prompt.
pub trait Model {
    /// How it is named in PORTING.md and the state file: `claude-opus-5-5`.
    fn name(&self) -> String;
    /// Answers `prompt`. `shared` comes first: the part every prompt of its kind
    /// has, which the Claude API caches between calls (it may be empty).
    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply>;
}

/// Claude through the Messages API.
pub struct Anthropic {
    pub model: String,
    /// How hard it thinks; the model's own default when none.
    pub effort: Option<String>,
    auth: Auth,
}

enum Auth {
    ApiKey(String),
    Bearer(String),
}

impl Anthropic {
    /// Credentials from `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`, else the
    /// `ant` CLI's signed-in profile (`ant auth login`).
    pub fn from_env(model: Option<&str>, effort: Option<&str>) -> Result<Anthropic> {
        let auth = if let Some(key) = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
        {
            Auth::ApiKey(key)
        } else if let Some(token) = std::env::var("ANTHROPIC_AUTH_TOKEN")
            .ok()
            .filter(|k| !k.is_empty())
        {
            Auth::Bearer(token)
        } else if let Some(token) = ant_token() {
            Auth::Bearer(token)
        } else {
            bail!(
                "no Claude API credentials: set ANTHROPIC_API_KEY (an API key from platform.claude.com), \
                 or sign in with `ant auth login`, or use --ai claude-code or --ai \"<command>\""
            );
        };
        Ok(Anthropic {
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            effort: effort.map(str::to_string),
            auth,
        })
    }

    /// With an API key the caller holds (the app reads it from the system's
    /// credential store).
    pub fn with_key(key: &str, model: Option<&str>, effort: Option<&str>) -> Anthropic {
        Anthropic {
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            effort: effort.map(str::to_string),
            auth: Auth::ApiKey(key.to_string()),
        }
    }

    /// Whether the server-side refusal fallback is available for this model.
    fn has_fallback(&self) -> bool {
        [
            "claude-fable-5-1",
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-sonnet-5-5",
        ]
        .contains(&self.model.as_str())
    }
}

/// A short-lived token from the `ant` CLI, when it is installed and signed in.
fn ant_token() -> Option<String> {
    let out = Command::new("ant")
        .args(["auth", "print-credentials", "--access-token"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let token = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !token.is_empty()).then_some(token)
}

impl Model for Anthropic {
    fn name(&self) -> String {
        self.model.clone()
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        let mut content = Vec::new();
        if !shared.is_empty() {
            // Cached: the same for every hook helper, so later calls read it cheaply.
            content.push(serde_json::json!({
                "type": "text",
                "text": shared,
                "cache_control": { "type": "ephemeral" },
            }));
        }
        content.push(serde_json::json!({ "type": "text", "text": prompt }));
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 16000,
            "system": system,
            "messages": [{ "role": "user", "content": content }],
        });
        if let Some(effort) = &self.effort {
            body["output_config"] = serde_json::json!({ "effort": effort });
        }
        let mut betas: Vec<&str> = Vec::new();
        if self.has_fallback() {
            // When a safety classifier declines, the server retries on a model
            // that can answer, chosen by the refusal's category.
            body["fallbacks"] = "default".into();
            betas.push("server-side-fallback-2026-07-01");
        }
        if matches!(self.auth, Auth::Bearer(_)) {
            betas.push("oauth-2025-04-20");
        }
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(600)))
            .build()
            .new_agent();
        let mut wait = 2;
        for attempt in 0..4 {
            let mut req = agent
                .post("https://api.anthropic.com/v1/messages")
                .header("content-type", "application/json")
                .header("anthropic-version", "2026-10-21");
            req = match &self.auth {
                Auth::ApiKey(k) => req.header("x-api-key", k),
                Auth::Bearer(t) => req.header("authorization", &format!("Bearer {t}")),
            };
            if !betas.is_empty() {
                req = req.header("anthropic-beta", &betas.join(","));
            }
            let mut resp = match req.send_json(&body) {
                Ok(r) => r,
                Err(e) if attempt < 3 => {
                    eprintln!("  the API did not answer ({e}); trying again in {wait} s");
                    std::thread::sleep(std::time::Duration::from_secs(wait));
                    wait *= 2;
                    continue;
                }
                Err(e) => return Err(e).context("calling the Claude API"),
            };
            let status = resp.status().as_u16();
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let json: serde_json::Value = resp
                .body_mut()
                .read_json()
                .context("reading the Claude API's answer")?;
            if status == 429 || status >= 500 {
                if attempt < 3 {
                    let secs = retry_after.unwrap_or(wait);
                    eprintln!("  the API is busy ({status}); trying again in {secs} s");
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    wait *= 2;
                    continue;
                }
                bail!("the Claude API is busy ({status}): {}", api_error(&json));
            }
            if status != 200 {
                bail!(
                    "the Claude API refused the request ({status}): {}",
                    api_error(&json)
                );
            }
            match json["stop_reason"].as_str() {
                Some("refusal") => bail!(
                    "Claude declined this one ({})",
                    json["stop_details"]["category"]
                        .as_str()
                        .unwrap_or("no category")
                ),
                Some("max_tokens") => bail!("the answer was cut off at 16000 tokens"),
                _ => {}
            }
            let text = json["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            // Every input token, the cached ones too (read from or written to the cache).
            let usage = &json["usage"];
            let tokens = |k: &str| usage[k].as_u64().unwrap_or(0);
            return Ok(Reply {
                text,
                input_tokens: tokens("input_tokens")
                    + tokens("cache_creation_input_tokens")
                    + tokens("cache_read_input_tokens"),
                output_tokens: tokens("output_tokens"),
            });
        }
        unreachable!("the loop returns")
    }
}

fn api_error(json: &serde_json::Value) -> String {
    json["error"]["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| json.to_string())
}

/// Claude Code's `claude` command in print mode, signed in as you.
pub struct ClaudeCode {
    pub model: Option<String>,
}

impl Model for ClaudeCode {
    fn name(&self) -> String {
        format!(
            "claude-code ({})",
            self.model.as_deref().unwrap_or("its default model")
        )
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        let prompt = join(shared, prompt);
        let mut cmd = Command::new("claude");
        cmd.args(["-p", "--output-format", "json", "--system-prompt", system]);
        if let Some(m) = &self.model {
            cmd.args(["--model", m]);
        }
        let out = run(cmd, &prompt).context("running `claude` (is Claude Code installed?)")?;
        let json: serde_json::Value =
            serde_json::from_str(&out).context("reading `claude`'s JSON answer")?;
        if json["is_error"].as_bool() == Some(true) {
            bail!("claude: {}", json["result"].as_str().unwrap_or("failed"));
        }
        Ok(Reply {
            text: json["result"].as_str().unwrap_or_default().to_string(),
            input_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0),
        })
    }
}

/// Any command: the system prompt and the prompt on stdin, the answer on stdout.
pub struct External {
    pub command: String,
}

impl Model for External {
    fn name(&self) -> String {
        self.command.clone()
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        let prompt = join(shared, prompt);
        let mut words = self.command.split_whitespace();
        let program = words.next().context("an empty --ai command")?;
        let mut cmd = Command::new(program);
        cmd.args(words);
        let text = run(cmd, &format!("{system}\n\n{prompt}"))
            .with_context(|| format!("running `{}`", self.command))?;
        Ok(Reply {
            text,
            ..Default::default()
        })
    }
}

/// The shared part and the prompt, as one text.
fn join(shared: &str, prompt: &str) -> String {
    if shared.is_empty() {
        prompt.to_string()
    } else {
        format!("{shared}\n\n{prompt}")
    }
}

/// Runs `cmd` with `input` on stdin; its stdout, or an error with its stderr.
fn run(mut cmd: Command, input: &str) -> Result<String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    let input = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut out = String::new();
    child
        .stdout
        .take()
        .context("no stdout")?
        .read_to_string(&mut out)?;
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let _ = writer.join();
    let status = child.wait()?;
    if !status.success() {
        bail!("it failed ({status}): {}", err.trim());
    }
    Ok(out)
}

/// A model running on this computer through Ollama (ollama.com): free, and
/// nothing leaves the computer. `OLLAMA_HOST` names another server.
pub struct Ollama {
    pub model: String,
    host: String,
}

impl Ollama {
    pub fn new(model: Option<&str>) -> Result<Ollama> {
        let host = ollama_host();
        let Some(model) = model else {
            let installed = ollama_models(&host)
                .map(|m| {
                    if m.is_empty() {
                        "none yet: `ollama pull <model>` gets one".to_string()
                    } else {
                        m.join(", ")
                    }
                })
                .unwrap_or_else(|e| format!("could not ask Ollama at {host}: {e:#}"));
            bail!("name the local model with --model (installed: {installed})");
        };
        Ok(Ollama {
            model: model.to_string(),
            host,
        })
    }
}

/// The models installed in the Ollama on this computer (or at `OLLAMA_HOST`).
pub fn local_models() -> Result<Vec<String>> {
    ollama_models(&ollama_host())
}

/// Where Ollama answers: `OLLAMA_HOST`, else its default port here.
fn ollama_host() -> String {
    std::env::var("OLLAMA_HOST")
        .ok()
        .filter(|h| !h.is_empty())
        .map(|h| {
            if h.starts_with("http") {
                h
            } else {
                format!("http://{h}")
            }
        })
        .unwrap_or_else(|| "http://localhost:11434".into())
}

/// The models an Ollama server has.
fn ollama_models(host: &str) -> Result<Vec<String>> {
    let json: serde_json::Value = ureq::get(&format!("{host}/api/tags"))
        .call()
        .context("is Ollama running?")?
        .body_mut()
        .read_json()?;
    Ok(json["models"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|x| x["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

impl Model for Ollama {
    fn name(&self) -> String {
        format!("{} (local)", self.model)
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(1800)))
            .build()
            .new_agent();
        let body = serde_json::json!({
            "model": self.model,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": join(shared, prompt) },
            ],
            // Room for a whole mod class and the module around it.
            "options": { "num_ctx": 32768 },
        });
        let mut resp = agent
            .post(&format!("{}/api/chat", self.host))
            .send_json(&body)
            .with_context(|| format!("calling Ollama at {} (is it running?)", self.host))?;
        let status = resp.status().as_u16();
        let json: serde_json::Value = resp
            .body_mut()
            .read_json()
            .context("reading Ollama's answer")?;
        if status != 200 {
            bail!(
                "Ollama refused the request ({status}): {}",
                json["error"].as_str().unwrap_or("no message")
            );
        }
        Ok(Reply {
            text: json["message"]["content"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            input_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0),
            output_tokens: json["eval_count"].as_u64().unwrap_or(0),
        })
    }
}

/// The model `--ai` names: `anthropic` (the default), `ollama` (local, free),
/// `claude-code`, or a command.
pub fn from_choice(
    choice: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<Box<dyn Model>> {
    Ok(match choice {
        "anthropic" | "claude" | "api" => Box::new(Anthropic::from_env(model, effort)?),
        "ollama" | "local" => Box::new(Ollama::new(model)?),
        "claude-code" => Box::new(ClaudeCode {
            model: model.map(str::to_string),
        }),
        command => Box::new(External {
            command: command.to_string(),
        }),
    })
}
