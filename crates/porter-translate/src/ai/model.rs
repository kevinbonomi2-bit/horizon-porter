// The AI models the translator can ask: Claude through the Messages API (raw
// HTTP: Rust has no official Anthropic SDK), a model on this computer through
// Ollama, a service that speaks the chat completions API (Google's Gemini and
// OpenRouter have free tiers), Claude Code's `claude` command, or any command that
// reads a prompt on stdin and writes the answer on stdout.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// The default model: Claude Haiku 5.5, the cheapest Claude ($0.10 per million
/// input tokens, $0.50 per million output, while a prompt stays within
/// `LONG_PROMPT`). The compile-and-fix loop catches what it gets wrong in
/// runtime.rs; `--model claude-sonnet-5-5` or `claude-opus-5-5` for harder mods.
pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";

/// The longest prompt, in tokens, Claude Haiku 5.5 bills at its lower prices: one
/// token more and every token of the request, in and out, costs five times as much.
pub const LONG_PROMPT: u64 = 100_000;

/// Dollars per million input and output tokens for a request whose prompt is
/// `prompt_tokens` long.
pub fn price(model: &str, prompt_tokens: u64) -> Option<(f64, f64)> {
    Some(match model {
        "claude-haiku-5-5" if prompt_tokens > LONG_PROMPT => (0.50, 2.50),
        "claude-haiku-5-5" => (0.10, 0.50),
        "claude-sonnet-5-5" | "claude-sonnet-5" => (2.0, 10.0),
        "claude-opus-5-5" => (4.0, 20.0),
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" => (5.0, 25.0),
        "claude-fable-5-1" | "claude-fable-5" => (10.0, 50.0),
        _ => return None,
    })
}

/// The tokens of one request, as the API bills them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Input read at the full price.
    pub input: u64,
    /// Input written to the cache: a quarter more than the full price.
    pub cache_write: u64,
    /// Input read from the cache: a tenth of the full price.
    pub cache_read: u64,
    /// The answer, and the thinking before it.
    pub output: u64,
}

impl Usage {
    /// The whole prompt, which is what picks the rate card.
    pub fn prompt(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }

    /// What the request cost on `model`, when its prices are known.
    pub fn dollars(&self, model: &str) -> Option<f64> {
        let (input, output) = price(model, self.prompt())?;
        Some(
            (self.input as f64 + self.cache_write as f64 * 1.25 + self.cache_read as f64 * 0.1)
                / 1e6
                * input
                + self.output as f64 / 1e6 * output,
        )
    }
}

/// An answer, with what it cost when the provider says: every request it took,
/// those whose answer was cut off and asked again too.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub text: String,
    /// Every input token, the cached ones too.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Of the input, what was read from the cache.
    pub cached_tokens: u64,
    /// Dollars, where the model's prices are known.
    pub dollars: Option<f64>,
    /// Why there is no answer, when every try at one ran out of room: said with
    /// what the tries cost, which an error would lose.
    pub cut_off: Option<String>,
}

impl Reply {
    /// Adds one request's tokens and cost.
    fn spend(&mut self, usage: Usage, model: &str) {
        self.input_tokens += usage.prompt();
        self.output_tokens += usage.output;
        self.cached_tokens += usage.cache_read;
        if let Some(d) = usage.dollars(model) {
            self.dollars = Some(self.dollars.unwrap_or(0.0) + d);
        }
    }
}

/// Something that answers a prompt.
pub trait Model {
    /// How it is named in PORTING.md and the state file: `claude-opus-5-5`.
    fn name(&self) -> String;
    /// Answers `prompt`. `shared` comes first: the part every prompt of its kind
    /// has, which the Claude API caches between calls (it may be empty).
    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply>;
    /// How many tokens the prompt is to this model, where it can say without
    /// answering (the Claude API counts for free).
    fn count(&self, _system: &str, _shared: &str, _prompt: &str) -> Result<Option<u64>> {
        Ok(None)
    }
}

/// Claude through the Messages API.
pub struct Anthropic {
    pub model: String,
    /// How hard it thinks; the model's own default when none.
    pub effort: Option<String>,
    auth: Auth,
    /// Where the API answers: Anthropic's, unless `at` named another.
    base: String,
}

enum Auth {
    ApiKey(String),
    Bearer(String),
}

impl Anthropic {
    /// Credentials from `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`, else the
    /// `ant` CLI's signed-in profile (`ant auth login`), else the key saved in the
    /// app (the system's credential store).
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
        } else if let Some(key) = saved_key() {
            Auth::ApiKey(key)
        } else {
            bail!(
                "no Claude API credentials: set ANTHROPIC_API_KEY (an API key from platform.claude.com), \
                 or sign in with `ant auth login`, or save one in the app, or use --ai claude-code or --ai \"<command>\""
            );
        };
        Ok(Anthropic {
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            effort: effort.map(str::to_string),
            auth,
            base: "https://api.anthropic.com".into(),
        })
    }

    /// The same, asked at another address: a gateway in front of the API, or a test's
    /// stand-in for it.
    pub fn at(mut self, base: &str) -> Anthropic {
        self.base = base.trim_end_matches('/').to_string();
        self
    }

    /// With an API key the caller holds (the app reads it from the system's
    /// credential store).
    pub fn with_key(key: &str, model: Option<&str>, effort: Option<&str>) -> Anthropic {
        Anthropic {
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            effort: effort.map(str::to_string),
            auth: Auth::ApiKey(key.to_string()),
            base: "https://api.anthropic.com".into(),
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

/// The API key the app keeps in the system's credential store (Windows Credential
/// Manager, the macOS keychain, the Secret Service), when one is saved there.
fn saved_key() -> Option<String> {
    keyring::Entry::new(
        "io.github.kevinbonomi2bit.horizonporter",
        "anthropic-api-key",
    )
    .ok()?
    .get_password()
    .ok()
    .filter(|k| !k.trim().is_empty())
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

impl Anthropic {
    /// A request's body: the instructions and the shared part as the system prompt
    /// (the shared part cached: it is the same for every function of a kind, and what
    /// is in the system prompt stays cached when a later try turns thinking off), the
    /// function's own part as the message.
    fn body(&self, system: &str, shared: &str, prompt: &str) -> serde_json::Value {
        let system = if shared.is_empty() {
            serde_json::json!(system)
        } else {
            serde_json::json!([
                { "type": "text", "text": system },
                { "type": "text", "text": shared, "cache_control": { "type": "ephemeral" } },
            ])
        };
        serde_json::json!({
            "model": self.model,
            "system": system,
            "messages": [{ "role": "user", "content": [{ "type": "text", "text": prompt }] }],
        })
    }

    fn request(
        &self,
        agent: &ureq::Agent,
        path: &str,
    ) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
        let mut req = agent
            .post(&format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .header("anthropic-version", "2026-10-21");
        req = match &self.auth {
            Auth::ApiKey(k) => req.header("x-api-key", k),
            Auth::Bearer(t) => req.header("authorization", &format!("Bearer {t}")),
        };
        let mut betas: Vec<&str> = Vec::new();
        if self.has_fallback() && path == "/v1/messages" {
            betas.push("server-side-fallback-2026-07-01");
        }
        if matches!(self.auth, Auth::Bearer(_)) {
            betas.push("oauth-2025-04-20");
        }
        if !betas.is_empty() {
            req = req.header("anthropic-beta", &betas.join(","));
        }
        req
    }

    /// Whether thinking can be turned off for a last try, where every one before it
    /// spent its room thinking: Claude Haiku 5.5 thinks unless told not to, and takes
    /// being told not to.
    fn thinks_by_default(&self) -> bool {
        self.model == "claude-haiku-5-5"
    }
}

impl Model for Anthropic {
    fn name(&self) -> String {
        self.model.clone()
    }

    fn count(&self, system: &str, shared: &str, prompt: &str) -> Result<Option<u64>> {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build()
            .new_agent();
        let body = self.body(system, shared, prompt);
        let mut wait = 2;
        for attempt in 0..5 {
            let mut resp = self
                .request(&agent, "/v1/messages/count_tokens")
                .send_json(&body)
                .context("calling the Claude API to count tokens")?;
            let status = resp.status().as_u16();
            let json: serde_json::Value = resp
                .body_mut()
                .read_json()
                .context("reading the Claude API's count")?;
            if (status == 429 || status >= 500) && attempt < 4 {
                std::thread::sleep(std::time::Duration::from_secs(wait));
                wait *= 2;
                continue;
            }
            if status != 200 {
                bail!(
                    "the Claude API would not count the prompt ({status}): {}",
                    api_error(&json)
                );
            }
            return Ok(json["input_tokens"].as_u64());
        }
        unreachable!("the loop returns")
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        /// What an answer that ran past the limit twice is asked.
        const SHORTER: &str = "Your last two answers ran past the length an answer can have, and were lost. \
            Write the whole function again so that it ends within about 1,200 lines: write nothing out twice (a \
            nested `fn` or a closure for what repeats), comment only where a comment says something, and where it \
            still can't all fit, port the most central paths of the Java in full and leave the others out, each \
            with one `// Missing:` line naming it.";
        /// Room for an answer on the first try, and on the tries after one was cut off.
        const ROOM: u64 = 16_000;
        const MORE_ROOM: u64 = 48_000;
        let mut body = self.body(system, shared, prompt);
        body["max_tokens"] = ROOM.into();
        if let Some(effort) = &self.effort {
            body["output_config"] = serde_json::json!({ "effort": effort });
        }
        if self.has_fallback() {
            // When a safety classifier declines, the server retries on a model
            // that can answer, chosen by the refusal's category.
            body["fallbacks"] = "default".into();
        }
        // (A long function's thinking and answer can take a slow model a quarter of an
        // hour: a request given up on is paid for all the same, and sent again.)
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(1800)))
            .build()
            .new_agent();
        let mut wait = 2;
        let mut asked_shorter = false;
        // How many times it has been told to think less: 1, low effort; 2, none.
        let mut less_thinking = 0;
        // Every request is paid for, the ones whose answer was cut off too.
        let mut spent = Reply::default();
        let mut failures = 0;
        loop {
            let mut resp = match self.request(&agent, "/v1/messages").send_json(&body) {
                Ok(r) => r,
                Err(e) if failures < 3 => {
                    failures += 1;
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
                if failures < 3 {
                    failures += 1;
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
            let usage = &json["usage"];
            let tokens = |k: &str| usage[k].as_u64().unwrap_or(0);
            spent.spend(
                Usage {
                    input: tokens("input_tokens"),
                    cache_write: tokens("cache_creation_input_tokens"),
                    cache_read: tokens("cache_read_input_tokens"),
                    output: tokens("output_tokens"),
                },
                &self.model,
            );
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
            match json["stop_reason"].as_str() {
                Some("refusal") => bail!(
                    "Claude declined this one ({})",
                    json["stop_details"]["category"]
                        .as_str()
                        .unwrap_or("no category")
                ),
                Some("max_tokens") => {
                    let room = body["max_tokens"].as_u64().unwrap_or(ROOM);
                    if text.trim().is_empty() {
                        // All the room went on thinking (which comes back as an empty
                        // block): once more with room for a long answer and told to
                        // think less; then, where the model can, without thinking at
                        // all. (An answer written without any is often a poor one: it
                        // is the last thing tried, and `translate` sends back one that
                        // does nothing.)
                        less_thinking += 1;
                        body["max_tokens"] = MORE_ROOM.into();
                        match less_thinking {
                            1 => {
                                body["output_config"] = serde_json::json!({ "effort": "low" });
                                eprintln!(
                                    "  the answer was cut off at {room} tokens, all of them thinking; asking again with room for {MORE_ROOM} and less thinking"
                                );
                                continue;
                            }
                            2 if self.thinks_by_default() => {
                                body["thinking"] = serde_json::json!({ "type": "disabled" });
                                eprintln!(
                                    "  the answer was cut off at {room} tokens, all of them thinking again; asking once more with thinking off"
                                );
                                continue;
                            }
                            _ => {
                                spent.cut_off = Some(format!(
                                    "the answer was cut off at {room} tokens {less_thinking} times, with nothing written: all of it was thinking"
                                ));
                                return Ok(spent);
                            }
                        }
                    }
                    let lines = text.lines().count();
                    let distinct: std::collections::BTreeSet<&str> =
                        text.lines().map(str::trim).collect();
                    let wrote = format!(
                        "{lines} lines of answer, {} of them different",
                        distinct.len()
                    );
                    if room < MORE_ROOM {
                        // A long function: once more with room for it.
                        body["max_tokens"] = MORE_ROOM.into();
                        eprintln!(
                            "  the answer was cut off at {room} tokens ({wrote}); asking again with room for {MORE_ROOM}"
                        );
                        continue;
                    }
                    // Still too long (a tick with every path of its Java, or an answer
                    // that went round in circles): once more, told how long it may be.
                    if !asked_shorter {
                        asked_shorter = true;
                        eprintln!(
                            "  the answer was cut off at {MORE_ROOM} tokens too ({wrote}); asking once more for a shorter one"
                        );
                        if let Some(content) = body["messages"][0]["content"].as_array_mut() {
                            content.push(serde_json::json!({ "type": "text", "text": SHORTER }));
                        }
                        continue;
                    }
                    spent.cut_off = Some(format!(
                        "the answer was cut off at {MORE_ROOM} tokens again ({wrote})"
                    ));
                    return Ok(spent);
                }
                _ => {}
            }
            spent.text = text;
            return Ok(spent);
        }
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
            ..Default::default()
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
            ..Default::default()
        })
    }
}

/// A model behind a chat completions API (the one OpenAI's has, which most
/// services and local servers speak): Google's Gemini and OpenRouter by name, both
/// with models that cost nothing within a daily limit, or any other by its URL.
pub struct Compatible {
    pub model: String,
    /// How hard it thinks, where the service takes `reasoning_effort`.
    pub effort: Option<String>,
    /// What it is called in messages: `Gemini`, `OpenRouter`, or the URL.
    service: String,
    /// Where it answers, up to `/chat/completions`.
    base: String,
    key: Option<String>,
}

/// A service known by name: what `--ai` calls it, what it is called, where it
/// answers, the environment variable its key is read from, where a key comes from,
/// and the model asked when none is named.
struct Service {
    choice: &'static str,
    name: &'static str,
    base: &'static str,
    key: &'static str,
    key_from: &'static str,
    model: Option<&'static str>,
}

const SERVICES: &[Service] = &[
    Service {
        choice: "gemini",
        name: "Gemini",
        base: "https://generativelanguage.googleapis.com/v1beta/openai",
        key: "GEMINI_API_KEY",
        key_from: "aistudio.google.com/apikey (free, no card)",
        // Free of charge in and out on the free tier, with a context of a million
        // tokens (ai.google.dev/gemini-api/docs/pricing, November 2026).
        model: Some("gemini-3.8-flash"),
    },
    Service {
        choice: "openrouter",
        name: "OpenRouter",
        base: "https://openrouter.ai/api/v1",
        key: "OPENROUTER_API_KEY",
        key_from: "openrouter.ai/keys",
        // Its free models come and go: it lists them (those ending in `:free`).
        model: None,
    },
];

/// Whether `--ai` names a service with a free tier, and what it is called.
pub fn free_service(choice: &str) -> Option<&'static str> {
    service(choice).map(|s| s.name)
}

/// The service `--ai` names, if it is one known by name.
fn service(choice: &str) -> Option<&'static Service> {
    SERVICES
        .iter()
        .find(|s| s.choice == choice || (choice == "google" && s.choice == "gemini"))
}

/// The environment variable a named service's key is read from, and the model it is
/// asked when none is named.
pub fn service_defaults(choice: &str) -> Option<(&'static str, Option<&'static str>)> {
    service(choice).map(|s| (s.key, s.model))
}

impl Compatible {
    /// A service by name with a key the caller holds (the app reads it from the
    /// system's credential store).
    pub fn with_key(
        choice: &str,
        key: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Compatible> {
        let Some(service) = service(choice) else {
            bail!("{choice} is not a service known by name");
        };
        Self::keyed(service, key, model, effort)
    }

    /// A service by name (`gemini`, `openrouter`), its key from the environment.
    fn named(service: &Service, model: Option<&str>, effort: Option<&str>) -> Result<Compatible> {
        let Some(key) = std::env::var(service.key)
            .ok()
            .filter(|k| !k.trim().is_empty())
        else {
            bail!(
                "no {} key: set {} to a key from {}",
                service.name,
                service.key,
                service.key_from
            );
        };
        Self::keyed(service, &key, model, effort)
    }

    fn keyed(
        service: &Service,
        key: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<Compatible> {
        let mut it = Compatible {
            model: String::new(),
            effort: effort.map(str::to_string),
            service: service.name.to_string(),
            base: service.base.to_string(),
            key: Some(key.trim().to_string()),
        };
        it.model = match model.or(service.model) {
            Some(m) => m.to_string(),
            None => bail!("name the model with --model ({})", it.models_hint()),
        };
        Ok(it)
    }

    /// Any server by its URL (`http://localhost:1234/v1`, LM Studio's): the key, where
    /// it wants one, from `PORTER_AI_KEY`.
    pub fn at(url: &str, model: Option<&str>, effort: Option<&str>) -> Result<Compatible> {
        let base = url
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .to_string();
        let mut it = Compatible {
            model: String::new(),
            effort: effort.map(str::to_string),
            service: base.clone(),
            base,
            key: std::env::var("PORTER_AI_KEY")
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty()),
        };
        it.model = match model {
            Some(m) => m.to_string(),
            None => bail!("name the model with --model ({})", it.models_hint()),
        };
        Ok(it)
    }

    fn agent(seconds: u64) -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(seconds)))
            .build()
            .new_agent()
    }

    /// The models the service has, the free ones first where it marks them.
    pub fn models(&self) -> Result<Vec<String>> {
        let mut req = Self::agent(60).get(&format!("{}/models", self.base));
        if let Some(key) = &self.key {
            req = req.header("authorization", &format!("Bearer {key}"));
        }
        let mut resp = req
            .call()
            .with_context(|| format!("asking {} for its models", self.service))?;
        let status = resp.status().as_u16();
        let json: serde_json::Value = resp.body_mut().read_json()?;
        if status != 200 {
            bail!("{} ({status}): {}", self.service, compatible_error(&json));
        }
        let mut ids: Vec<String> = json["data"]
            .as_array()
            .map(|m| {
                m.iter()
                    .filter_map(|x| x["id"].as_str())
                    // (Gemini lists them as `models/gemini-...`.)
                    .map(|id| id.trim_start_matches("models/").to_string())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort_by_key(|id| !id.ends_with(":free"));
        Ok(ids)
    }

    /// The models to choose from, for a message.
    fn models_hint(&self) -> String {
        match self.models() {
            Ok(ids) if ids.is_empty() => "it lists none".into(),
            Ok(ids) => {
                let free: Vec<&String> = ids.iter().filter(|id| id.ends_with(":free")).collect();
                let shown: Vec<&str> = if free.is_empty() {
                    ids.iter().take(24).map(String::as_str).collect()
                } else {
                    free.iter().take(24).map(|s| s.as_str()).collect()
                };
                format!(
                    "{}{}: {}",
                    if free.is_empty() {
                        "it has"
                    } else {
                        "its free ones"
                    },
                    if shown.len() < ids.len() && free.is_empty() {
                        ", among others"
                    } else {
                        ""
                    },
                    shown.join(", ")
                )
            }
            Err(e) => format!("could not list them: {e:#}"),
        }
    }
}

/// What a chat completions API says went wrong: `{"error": {"message": ..}}`, which
/// Gemini wraps in a list.
fn compatible_error(json: &serde_json::Value) -> String {
    let error = if json.is_array() {
        &json[0]["error"]
    } else {
        &json["error"]
    };
    error["message"]
        .as_str()
        .or(error.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| json.to_string())
}

impl Model for Compatible {
    fn name(&self) -> String {
        format!("{} ({})", self.model, self.service)
    }

    fn complete(&self, system: &str, shared: &str, prompt: &str) -> Result<Reply> {
        // The shared part first and the same each time: services that cache a
        // prompt's start (Gemini, OpenRouter's providers) do it on their own.
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": join(shared, prompt) },
            ],
        });
        if let Some(effort) = &self.effort {
            body["reasoning_effort"] = effort.as_str().into();
        }
        let agent = Self::agent(900);
        // A free tier answers so many times a minute: wait for the next one, a few
        // times, and stop where the day's share is used up.
        let waits = [10, 20, 40, 60, 60];
        let mut tries = 0;
        loop {
            let mut req = agent
                .post(&format!("{}/chat/completions", self.base))
                .header("content-type", "application/json");
            if let Some(key) = &self.key {
                req = req.header("authorization", &format!("Bearer {key}"));
            }
            let mut resp = match req.send_json(&body) {
                Ok(r) => r,
                Err(e) if tries < 2 => {
                    tries += 1;
                    eprintln!("  {} did not answer ({e}); trying again", self.service);
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
                Err(e) => return Err(e).with_context(|| format!("calling {}", self.service)),
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
                .with_context(|| format!("reading {}'s answer", self.service))?;
            if status == 429 || status >= 500 {
                if let Some(wait) = waits.get(tries) {
                    tries += 1;
                    let secs = retry_after.unwrap_or(*wait).min(120);
                    eprintln!(
                        "  {} is at its limit or busy ({status}); trying again in {secs} s",
                        self.service
                    );
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    continue;
                }
                bail!(
                    "{} is still at its limit ({status}): {}. A free tier has so many requests a day: \
                     run translate again later, and what is ported stays ported",
                    self.service,
                    compatible_error(&json)
                );
            }
            if status != 200 {
                bail!(
                    "{} refused the request ({status}): {}",
                    self.service,
                    compatible_error(&json)
                );
            }
            let choice = &json["choices"][0];
            let usage = &json["usage"];
            let mut reply = Reply {
                text: choice["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                input_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
                cached_tokens: usage["prompt_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0),
                ..Default::default()
            };
            if choice["finish_reason"] == "length" {
                reply.cut_off = Some(format!(
                    "the answer was cut off at the length {} allows",
                    self.model
                ));
                reply.text.clear();
            }
            return Ok(reply);
        }
    }
}

/// The model `--ai` names: `anthropic` (the default), `ollama` (local, free),
/// `gemini` or `openrouter` (services with free models), a URL (any server with a
/// chat completions API), `claude-code`, or a command.
pub fn from_choice(
    choice: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<Box<dyn Model>> {
    if let Some(service) = service(choice) {
        return Ok(Box::new(Compatible::named(service, model, effort)?));
    }
    Ok(match choice {
        "anthropic" | "claude" | "api" => Box::new(Anthropic::from_env(model, effort)?),
        "ollama" | "local" => Box::new(Ollama::new(model)?),
        "claude-code" => Box::new(ClaudeCode {
            model: model.map(str::to_string),
        }),
        url if url.starts_with("http://") || url.starts_with("https://") => {
            Box::new(Compatible::at(url, model, effort)?)
        }
        command => Box::new(External {
            command: command.to_string(),
        }),
    })
}
