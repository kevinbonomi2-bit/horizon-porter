// Horizon Porter's page. Commands run in Rust (src/main.rs); drops arrive as
// "drop-hover" and "dropped" events. Text from mods is only ever set as textContent.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

let busy = false;

function icon(name) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "i");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

function button(label, onClick, iconName, kind) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = kind ? `btn ${kind}` : "btn";
  if (iconName) b.append(icon(iconName));
  b.append(label);
  b.addEventListener("click", onClick);
  return b;
}

function showView(view) {
  $("welcome").hidden = view !== "welcome";
  $("result").hidden = view !== "result";
  $("mod").hidden = view !== "result";
  $("actions").hidden = view !== "result";
}

function setBusy(on, path) {
  $("drop-idle").hidden = on;
  $("drop-busy").hidden = !on;
  $("drop").setAttribute("aria-busy", on ? "true" : "false");
  for (const id of ["pick-folder", "pick-jar"]) $(id).disabled = on;
  if (on) {
    const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
    $("busy-title").textContent = `Reading ${name}`;
    $("busy-path").textContent = path;
  }
}

function welcomeError(message) {
  const el = $("welcome-error");
  el.hidden = !message;
  el.querySelector("span").textContent = message || "";
}

let ticking = null;

/** "1:05" from seconds. */
function clock(secs) {
  const m = Math.floor(secs / 60);
  const s = String(secs % 60).padStart(2, "0");
  return `${m}:${s}`;
}

/** A launcher progress line without its own clock: "[ 3:47] Compiling" → "Compiling". */
function progressText(line) {
  return line.replace(/^\[\s*\d+:\d\d\]\s*/, "");
}

/** A notice above the report: ok or bad, a title, a detail line and buttons. A busy
 * notice counts the seconds since it opened. */
function notice(kind, title, detail, actions = []) {
  const el = $("notice");
  clearInterval(ticking);
  ticking = null;
  el.replaceChildren();
  if (!kind) {
    el.hidden = true;
    return;
  }
  el.className = `notice${kind === "bad" ? " bad" : ""}${kind === "busy" ? " busy" : ""}`;
  if (kind === "busy") {
    const spinner = document.createElement("span");
    spinner.className = "spinner";
    spinner.setAttribute("aria-hidden", "true");
    el.append(spinner);
  } else {
    el.append(icon(kind === "bad" ? "alert" : "check"));
  }
  const text = document.createElement("div");
  text.className = "notice-text";
  const t = document.createElement("p");
  t.className = "notice-title";
  t.textContent = title;
  text.append(t);
  if (detail) {
    const d = document.createElement("p");
    d.className = "notice-detail";
    d.textContent = detail;
    d.title = detail;
    text.append(d);
  }
  el.append(text);
  if (kind === "busy") {
    const time = document.createElement("span");
    time.className = "notice-time";
    time.setAttribute("aria-label", "Time so far");
    const started = Date.now();
    time.textContent = clock(0);
    ticking = setInterval(() => {
      time.textContent = clock(Math.floor((Date.now() - started) / 1000));
    }, 1000);
    el.append(time);
  }
  if (actions.length) {
    const box = document.createElement("div");
    box.className = "notice-actions";
    box.append(...actions);
    el.append(box);
  }
  el.hidden = false;
}

async function read(path) {
  if (busy) return;
  busy = true;
  showView("welcome");
  welcomeError(null);
  setBusy(true, path);
  try {
    const view = await invoke("scan", { path });
    $("mod-title").textContent = view.title;
    $("mod-facts").textContent = view.facts.join(" · ");
    const port = $("port");
    port.disabled = view.isJar;
    port.title = view.isJar ? "Porting needs the mod's source folder; a jar can only be read" : "";
    $("report").srcdoc = view.report;
    notice(null);
    showView("result");
  } catch (e) {
    welcomeError(String(e));
  } finally {
    setBusy(false);
    busy = false;
  }
}

async function pick(jar) {
  const path = await invoke("pick", { jar });
  if (path) read(path);
}

async function openPath(path) {
  try {
    await invoke("open_path", { path });
  } catch (e) {
    notice("bad", "Could not open it", String(e));
  }
}

async function saveReport() {
  try {
    const path = await invoke("save_report");
    if (path) notice("ok", "Saved the report", path, [button("Open", () => openPath(path))]);
  } catch (e) {
    notice("bad", "Could not save the report", String(e));
  }
}

async function port(choose = false) {
  const b = $("port");
  const label = b.lastChild.previousSibling;
  b.disabled = true;
  b.classList.add("busy");
  label.textContent = "Porting";
  try {
    const done = await invoke("port", { choose });
    if (done) {
      const parts = [];
      if (done.blocks) parts.push(count(done.blocks, "block"));
      if (done.items) parts.push(count(done.items, "item"));
      if (done.blockEntities) parts.push(count(done.blockEntities, "block entity", "block entities"));
      if (done.native) {
        parts.push(`${done.wired.length} hooks wired`);
        if (done.stubbed.length) parts.push(`${done.stubbed.length} to port into`);
        if (done.unwired.length) parts.push(`${done.unwired.length} to wire by hand`);
      }
      lastPort = done;
      const actions = [
        button("Open folder", () => openPath(done.out), "folder"),
        button("Change folder", () => port(true)),
      ];
      if (done.native) actions.push(button("Translate with AI", () => estimate(done), "code", "primary"));
      actions.push(button("Try it", () => build(done), "play"));
      actions.push(button("Add to my Horizon", () => chooseInstance(done), "plus", done.native ? "" : "primary"));
      notice("ok", `Wrote ${done.id}-horizon: ${parts.join(", ")}`, done.out, actions);
    }
  } catch (e) {
    notice("bad", "Could not port the mod", String(e));
  } finally {
    label.textContent = "Port to Horizon";
    b.classList.remove("busy");
    b.disabled = false;
  }
}

/** "3 blocks", "1 block". */
function count(n, one, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

let building = false;
let translating = false;
let lastPort = null;

/** "$0.19", "under a cent". */
function dollars(n) {
  if (n == null) return "";
  return n < 0.01 ? "under a cent" : `about $${n.toFixed(2)}`;
}

/** What translating the ported mod would take, then a button to start. */
async function estimate(ported) {
  if (translating || building) return;
  notice("busy", "Counting what is left to port", ported.out);
  try {
    const est = await invoke("translate_estimate");
    if (!est.functions) {
      notice("ok", "Nothing left to translate", "Every function is ported", [
        button("Build and play", () => build(ported), "play", "primary"),
      ]);
      return;
    }
    const what = `${count(est.functions, "function")} to translate with ${est.model}`;
    if (est.blocker) {
      notice("bad", what, est.blocker, [button("Translation settings", openSettings, "settings")]);
      return;
    }
    const cost = est.local ? "Free: it runs on this computer" : `${dollars(est.dollars)} for one try each (a fix costs a little more)`;
    notice("ok", what, cost, [
      button("Settings", openSettings, "settings"),
      button("Translate", () => translate(ported), "code", "primary"),
    ]);
  } catch (e) {
    notice("bad", "Could not count what is left", String(e));
  }
}

/** Translates with the chosen model; progress arrives as "translate-progress". */
async function translate(ported) {
  if (translating || building) return;
  translating = true;
  $("port").disabled = true;
  notice("busy", `Translating ${ported.id}`, "Starting");
  try {
    const r = await invoke("translate");
    const done = r.ported + r.unchecked;
    const parts = [];
    if (r.ported) parts.push(`${r.ported} compiled and tested`);
    if (r.unchecked) parts.push(`${r.unchecked} checked when you build`);
    if (r.failed.length) parts.push(`${r.failed.length} left as they were`);
    if (r.inputTokens) parts.push(`${r.inputTokens.toLocaleString()} tokens in, ${r.outputTokens.toLocaleString()} out`);
    notice(r.failed.length && !done ? "bad" : "ok", `Ported ${count(done, "function")}`, parts.join(" · "), [
      button("Open folder", () => openPath(ported.out), "folder"),
      button("Try it", () => build(ported), "play"),
      button("Add to my Horizon", () => chooseInstance(ported), "plus", "primary"),
    ]);
    if (r.failed.length) $("notice").querySelector(".notice-detail").title = r.failed.join("\n");
  } catch (e) {
    notice("bad", "Translation stopped", String(e), [button("Translation settings", openSettings, "settings")]);
  } finally {
    translating = false;
    $("port").disabled = false;
  }
}

listen("translate-progress", (e) => {
  if (!translating) return;
  const detail = $("notice").querySelector(".notice-detail");
  if (detail) {
    detail.textContent = e.payload;
    detail.title = e.payload;
  }
});

/* Add to my Horizon: pick an instance, install the packaged mod there, then build
   the instance's native mods together. */
let installing = null;

async function chooseInstance(ported) {
  if (building || translating) return;
  installing = ported;
  const box = $("targets");
  box.querySelectorAll(".choice, .field-note").forEach((c) => c.remove());
  let targets = [];
  try {
    targets = await invoke("install_targets");
  } catch (e) {
    notice("bad", "Could not list your Horizon instances", String(e));
    return;
  }
  for (const t of targets) {
    const label = document.createElement("label");
    label.className = "choice";
    const input = document.createElement("input");
    input.type = "radio";
    input.name = "target";
    input.value = t.id;
    input.checked = t.selected;
    const name = document.createElement("span");
    name.className = "choice-name";
    name.textContent = t.name;
    const note = document.createElement("span");
    note.className = "choice-note";
    note.textContent = t.selected ? "The launcher's Play starts this one" : t.id.endsWith("-port") ? "A test instance Horizon Porter made" : "";
    label.append(input, name);
    if (note.textContent) label.append(note);
    box.append(label);
  }
  if (!box.querySelector("input:checked") && box.querySelector("input")) box.querySelector("input").checked = true;
  $("install").showModal();
}

$("install-go").addEventListener("click", async () => {
  const picked = $("targets").querySelector("input:checked");
  $("install").close();
  if (!picked || !installing) return;
  const ported = installing;
  notice("busy", `Adding ${ported.id}`, "Packing the mod");
  try {
    const r = await invoke("install_port", { id: picked.value });
    if (r.needsBuild) {
      notice("ok", `Added ${ported.id} to "${r.instance}"`, "Its code is built with the instance's other native mods before it plays", [
        button("Open folder", () => openPath(ported.out), "folder"),
        button(`Build "${r.instance}"`, () => buildInstance(r), "play", "primary"),
      ]);
    } else {
      notice("ok", `Added ${ported.id} to "${r.instance}"`, "Turned on: it loads the next time the instance starts", [
        button("Play", () => playInstance(r), "play", "primary"),
      ]);
    }
  } catch (e) {
    notice("bad", "Could not add the mod", String(e));
  }
});

async function buildInstance(r) {
  if (building || translating) return;
  building = true;
  $("port").disabled = true;
  notice("busy", `Building "${r.instance}"`, "Waiting for your answer");
  try {
    const result = await invoke("build_installed", { id: r.instanceId });
    if (result) {
      notice("ok", `Built "${r.instance}"`, result, [button("Play", () => playInstance(r), "play", "primary")]);
    } else {
      notice("ok", `Added to "${r.instance}", not built yet`, "Build it here or from the launcher's Mods page when you are ready", [
        button(`Build "${r.instance}"`, () => buildInstance(r), "play", "primary"),
      ]);
    }
  } catch (e) {
    const text = String(e);
    notice("bad", "The build failed", text.split("\n").filter(Boolean).slice(-1)[0] || text);
    $("notice").querySelector(".notice-detail").title = text;
  } finally {
    building = false;
    $("port").disabled = false;
  }
}

async function playInstance(r) {
  try {
    await invoke("play_installed", { id: r.instanceId });
  } catch (e) {
    notice("bad", "Could not start the game", String(e));
  }
}

/* Translation settings. The key goes to Rust once, into the credential store, and
   never comes back: the page only learns whether one is saved. */
let settings = null;

async function openSettings() {
  await renderSettings();
  const dialog = $("settings");
  if (!dialog.open) dialog.showModal();
}

async function renderSettings() {
  settings = await invoke("ai_settings");
  const box = $("choices");
  box.querySelectorAll(".choice").forEach((c) => c.remove());
  const options = settings.models.map((m) => ({ provider: "anthropic", id: m.id, name: m.name, note: m.note }));
  options.push({ provider: "ollama", id: "", name: "A model on this computer", note: "Free, and nothing leaves this computer. Needs Ollama (ollama.com) and a model pulled into it." });
  for (const o of options) {
    const label = document.createElement("label");
    label.className = "choice";
    const input = document.createElement("input");
    input.type = "radio";
    input.name = "who";
    input.value = `${o.provider}:${o.id}`;
    input.checked = settings.choice.provider === o.provider && (o.provider === "ollama" || settings.choice.model === o.id);
    input.addEventListener("change", () => choose(o.provider, o.id));
    const name = document.createElement("span");
    name.className = "choice-name";
    name.textContent = o.name;
    const note = document.createElement("span");
    note.className = "choice-note";
    note.textContent = o.note;
    label.append(input, name, note);
    box.append(label);
  }
  const local = settings.choice.provider === "ollama";
  $("local").hidden = !local;
  $("key-box").hidden = local;
  if (local) await renderLocal();
  $("key-saved").hidden = !settings.hasKey;
  $("key-entry").hidden = settings.hasKey;
  $("key-saved-text").textContent = `Saved in ${settings.store}`;
  $("key-note").textContent = settings.hasKey
    ? "Sent only to api.anthropic.com when you translate. Remove it here at any time."
    : `Create one at platform.claude.com, under API keys. It is kept in ${settings.store}, sent only to api.anthropic.com, and never shown again.${settings.envKey ? " ANTHROPIC_API_KEY is set, so that one is used until you save a key here." : ""}`;
}

async function renderLocal() {
  const select = $("local-model");
  select.replaceChildren();
  try {
    const models = await invoke("local_models");
    for (const m of models) {
      const opt = document.createElement("option");
      opt.value = m;
      opt.textContent = m;
      opt.selected = settings.choice.provider === "ollama" && settings.choice.model === m;
      select.append(opt);
    }
    select.disabled = !models.length;
    $("local-note").textContent = models.length
      ? "Larger coding models port better. Answers in runtime.rs are compiled and tested either way."
      : "Ollama has no models yet. Pull one (ollama pull <model>), then check again.";
    if (models.length && settings.choice.model !== select.value) await choose("ollama", select.value, true);
  } catch {
    select.disabled = true;
    $("local-note").textContent = "Ollama isn't running on this computer. Install it from ollama.com, start it, then check again.";
  }
}

async function choose(provider, model, quiet) {
  try {
    await invoke("save_choice", { choice: { provider, model: model || settings.choice.model } });
    if (!quiet) await renderSettings();
    else settings.choice = { provider, model };
  } catch (e) {
    $("local-note").textContent = String(e);
  }
}

$("open-settings").addEventListener("click", openSettings);
$("local-refresh").addEventListener("click", renderLocal);
$("local-model").addEventListener("change", (e) => choose("ollama", e.target.value, true));
$("key-save").addEventListener("click", async () => {
  const input = $("key-input");
  const key = input.value;
  input.value = "";
  if (!key.trim()) return;
  try {
    await invoke("save_key", { key });
    await renderSettings();
  } catch (e) {
    $("key-note").textContent = String(e);
  }
});
$("key-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    $("key-save").click();
  }
});
$("key-forget").addEventListener("click", async () => {
  try {
    await invoke("forget_key");
    await renderSettings();
  } catch (e) {
    $("key-note").textContent = String(e);
  }
});
$("settings").addEventListener("close", () => {
  $("key-input").value = "";
  // Back to the estimate, which may have been waiting on a key.
  if (lastPort && !translating && !building && $("notice").classList.contains("bad")) estimate(lastPort);
});

/** Builds the ported mod into Horizon with the player's launcher, after they
 * confirm they trust its code (asked in Rust, every time). */
async function build(ported) {
  if (building || translating) return;
  building = true;
  $("port").disabled = true;
  notice("busy", ported.native ? `Building ${ported.id}` : `Starting ${ported.id}`,
    ported.native ? "Waiting for your answer" : "Finding the Horizon launcher");
  try {
    const built = await invoke("build");
    if (built && !ported.native) {
      // Nothing to build: play straight away.
      notice("ok", `${ported.id} plays in "${built.instance}"`, built.result, [
        button("Open folder", () => openPath(ported.out), "folder"),
        button("Play again", play, "play"),
      ]);
      await play();
    } else if (built) {
      notice("ok", `Built ${ported.id} into "${built.instance}"`, built.result, [
        button("Open folder", () => openPath(ported.out), "folder"),
        button("Play", play, "play"),
        button("Add to my Horizon", () => chooseInstance(ported), "plus"),
      ]);
    } else {
      notice(null);
    }
  } catch (e) {
    const text = String(e);
    notice("bad", "The build failed", text.split("\n").filter(Boolean).slice(-1)[0] || text, [
      button("Open folder", () => openPath(ported.out), "folder"),
    ]);
    $("notice").querySelector(".notice-detail").title = text;
  } finally {
    building = false;
    $("port").disabled = false;
  }
}

async function play() {
  try {
    await invoke("play");
  } catch (e) {
    notice("bad", "Could not start the game", String(e));
  }
}

listen("build-progress", (e) => {
  if (!building) return;
  const detail = $("notice").querySelector(".notice-detail");
  if (detail) {
    detail.textContent = progressText(e.payload);
    detail.title = e.payload;
  }
});

$("pick-folder").addEventListener("click", () => pick(false));
$("pick-jar").addEventListener("click", () => pick(true));
$("drop").addEventListener("click", () => !busy && pick(false));
$("drop").addEventListener("keydown", (e) => {
  if ((e.key === "Enter" || e.key === " ") && !busy) {
    e.preventDefault();
    pick(false);
  }
});
$("again").addEventListener("click", () => {
  welcomeError(null);
  showView("welcome");
});
$("save").addEventListener("click", saveReport);
$("port").addEventListener("click", () => port(false));

listen("drop-hover", (e) => {
  document.body.classList.toggle("hovering", Boolean(e.payload));
  $("drop-title").textContent = e.payload ? "Release to read the mod" : "Drop a folder or .jar";
});
listen("dropped", (e) => {
  document.body.classList.remove("hovering");
  $("drop-title").textContent = "Drop a folder or .jar";
  const paths = e.payload || [];
  if (paths.length) read(paths[0]);
});
