// Horizon Porter's page. Commands run in Rust (src/main.rs); drops arrive as
// "drop-hover" and "dropped" events. Text from mods is only ever set as textContent.
//
// The flow a player sees: drop a mod, read the report, press "Make it work in
// Horizon", check one summary (what carries over, who translates its code and for
// how much, which instance it goes in) and say yes once. The app then ports,
// translates, adds and builds by itself, and ends on Play.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

let busy = false; // reading a mod
let running = null; // "port" | "translate" | "add" | "build" while making it work
let modName = "";
let isJar = false;

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

/** "3 blocks", "1 block". */
function count(n, one, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

/** "Demo Machines'", "Create's". */
function possessive(name) {
  return name.endsWith("s") ? `${name}'` : `${name}'s`;
}

/** "about $0.19", "under a cent". */
function dollars(n) {
  if (n == null) return "";
  return n < 0.01 ? "under a cent" : `about $${n.toFixed(2)}`;
}

let ticking = null;

/** "1:05" from seconds. */
function clock(secs) {
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/** A launcher progress line without its own clock: "[ 3:47] Compiling" → "Compiling". */
function progressText(line) {
  return line.replace(/^\[\s*\d+:\d\d\]\s*/, "");
}

/** A notice above the report: ok, bad or busy, a title, a detail line and buttons.
 * A busy notice counts the seconds since it opened. */
function notice(kind, title, detail, actions = []) {
  const el = $("notice");
  clearInterval(ticking);
  ticking = null;
  el.replaceChildren();
  if (!kind) {
    el.hidden = true;
    return;
  }
  el.className = `notice ${kind}`;
  if (kind === "busy") {
    const spinner = document.createElement("span");
    spinner.className = "spinner";
    spinner.setAttribute("aria-hidden", "true");
    el.append(spinner);
  } else {
    el.append(icon(kind === "ok" ? "check" : "alert"));
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

function setDetail(text, full) {
  const detail = $("notice").querySelector(".notice-detail");
  if (detail) {
    detail.textContent = text;
    detail.title = full || text;
  }
}

async function read(path) {
  if (busy || running) return;
  busy = true;
  showView("welcome");
  welcomeError(null);
  setBusy(true, path);
  try {
    const view = await invoke("scan", { path });
    $("mod-title").textContent = view.title;
    $("mod-facts").textContent = view.facts.join(" · ");
    // "Demo Machines 0.3.0 (demo)" → "Demo Machines".
    modName = view.title.replace(/\s*\([^)]*\)\s*$/, "").replace(/\s+v?\d[\w.+-]*$/, "") || view.title;
    isJar = view.isJar;
    const go = $("go");
    go.disabled = false;
    go.title = isJar ? "The jar is decompiled into source first (once), then ported" : "";
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

/* ---- Make it work in Horizon ---- */

let plan = null; // { ported, estimate, targets } while the summary is open

/** Ports the mod (quickly, into Documents/Horizon Porter), then shows what making
 * it work takes, for one yes. */
async function makeItWork() {
  if (running || busy) return;
  closeMenu();
  running = "port";
  $("go").disabled = true;
  notice("busy", `Getting ${modName} ready`, isJar ? "Reading the jar" : "Porting the mod");
  try {
    const ported = await invoke("port", { choose: false });
    if (!ported) {
      notice(null);
      return;
    }
    setDetail("Counting what is left to port and finding your Horizon");
    const [estimate, targets] = await Promise.all([
      ported.native ? invoke("translate_estimate") : Promise.resolve(null),
      invoke("install_targets"),
    ]);
    if (!targets.length) throw "No Horizon instance found: open the Horizon launcher once to make one";
    plan = { ported, estimate, targets };
    notice(null);
    renderSummary();
    $("go-sheet").showModal();
  } catch (e) {
    notice("bad", `Could not get ${modName} ready`, String(e));
  } finally {
    running = null;
    $("go").disabled = false;
  }
}

function renderSummary() {
  const { ported, estimate, targets } = plan;
  $("go-title").textContent = `Make ${modName} work in Horizon`;

  const carries = [];
  if (ported.blocks) carries.push(count(ported.blocks, "block"));
  if (ported.items) carries.push(count(ported.items, "item"));
  if (ported.blockEntities) carries.push(count(ported.blockEntities, "block entity", "block entities"));
  if (ported.native) carries.push("its code");
  $("go-carries").textContent = carries.length ? carries.join(", ") : "Its settings row in Options > Mods";

  const row = $("go-code-row");
  const code = $("go-code");
  const change = $("go-change");
  row.hidden = !ported.native;
  code.className = "";
  let blocked = false;
  if (ported.native) {
    if (!estimate || !estimate.functions) {
      code.textContent = "Already translated";
      change.hidden = true;
    } else if (estimate.blocker) {
      code.textContent = `${count(estimate.functions, "function")} to translate, but it can't be translated now`;
      code.className = "warn";
      change.textContent = "Set up";
      change.hidden = false;
      blocked = true;
    } else {
      const cost = estimate.local ? "free, on this computer" : dollars(estimate.dollars);
      code.textContent = `${count(estimate.functions, "function")} to translate with ${estimate.model}, ${cost}`;
      change.textContent = "Change";
      change.hidden = false;
    }
  }

  const select = $("go-target");
  const keep = select.value;
  select.replaceChildren();
  for (const t of targets) {
    const opt = document.createElement("option");
    opt.value = t.id;
    opt.textContent = t.selected ? `${t.name} (the one Play starts)` : t.name;
    opt.selected = keep ? keep === t.id : t.selected;
    select.append(opt);
  }

  $("go-trust").hidden = !ported.native;
  $("go-warn").hidden = !blocked;
  const reason = blocked ? estimate.blocker.charAt(0).toLowerCase() + estimate.blocker.slice(1) : "";
  $("go-warn-title").textContent = blocked ? `Its code won't be translated: ${reason}.` : "";
  $("go-skip").hidden = !blocked;
  const run = $("go-run");
  run.textContent = blocked ? "Set up translation" : ported.native ? "I trust it: make it work" : "Add to Horizon";
  run.disabled = false;
}

/** The summary's main button: set up translation when it can't run, else go. */
function onRun() {
  const { estimate } = plan;
  if (estimate && estimate.functions && estimate.blocker) openSettings();
  else run(false);
}

/** Translates (unless `skip`), adds and builds: the player said yes on the summary. */
async function run(skip) {
  const { ported, estimate } = plan;
  const id = $("go-target").value;
  const instanceName = plan.targets.find((t) => t.id === id)?.name || id;
  $("go-sheet").close();
  $("go").disabled = true;
  let translated = null;
  try {
    if (!skip && ported.native && estimate && estimate.functions && !estimate.blocker) {
      running = "translate";
      notice("busy", `Translating ${possessive(modName)} code`, "Starting");
      translated = await invoke("translate");
    }
    running = "add";
    notice("busy", `Adding ${modName} to "${instanceName}"`, "Packing the mod");
    const added = await invoke("install_port", { id });
    if (added.needsBuild) {
      running = "build";
      notice("busy", `Building "${added.instance}"`, "Starting");
      await invoke("build_installed", { id, trusted: true });
    }
    finish(ported, added, translated, estimate);
  } catch (e) {
    const text = String(e);
    const what = { translate: "Translation stopped", add: "Could not add the mod", build: "The build failed" }[running] || "It stopped";
    notice("bad", what, text.split("\n").filter(Boolean).slice(-1)[0] || text, [
      button("Open folder", () => openPath(ported.out), "folder"),
      button("Try again", makeItWork, "again", "primary"),
    ]);
    setDetail(text.split("\n").filter(Boolean).slice(-1)[0] || text, text);
  } finally {
    running = null;
    $("go").disabled = false;
  }
}

/** The ending: done, or in the instance with code still to translate, said plainly. */
function finish(ported, added, translated, estimate) {
  const left = translated ? translated.left : ported.native && estimate ? estimate.functions : 0;
  if (!left) {
    const facts = [];
    if (translated) facts.push(`${count(translated.ported + translated.unchecked, "function")} translated`);
    facts.push("turned on in Options > Mods");
    notice("ok", `${modName} is in "${added.instance}"`, facts.join(" · "), [
      button("Play", () => playInstance(added), "play", "primary"),
    ]);
    return;
  }
  let why;
  if (translated && translated.stopped) why = `Translation stopped: ${translated.stopped}`;
  else if (translated) why = `${count(left, "function")} couldn't be translated: its blocks and items work, what it does may not yet`;
  else why = `${count(left, "function")} not translated: its blocks and items work, what it does won't until it is`;
  notice("warn", `${modName} is in "${added.instance}", but its code isn't translated yet`, why, [
    button("Play", () => playInstance(added), "play"),
    button(translated && !translated.stopped ? "Try translating again" : "Translate now", () => translateNow(ported, added), "code", "primary"),
  ]);
}

/** Translates what is left, then adds and builds the instance again. */
async function translateNow(ported, added) {
  if (running) return;
  running = "port";
  $("go").disabled = true;
  notice("busy", `Getting ready to translate ${modName}`, "Checking the translator");
  let translated = null;
  try {
    const estimate = await invoke("translate_estimate");
    if (estimate.blocker) {
      running = null;
      notice("warn", `Can't translate ${possessive(modName)} code yet`, `${estimate.blocker}. Set it up, then translate.`, [
        button("Set up translation", openSettings, "settings", "primary"),
        button("Translate now", () => translateNow(ported, added), "code"),
      ]);
      return;
    }
    if (estimate.functions) {
      running = "translate";
      notice("busy", `Translating ${possessive(modName)} code`, `${count(estimate.functions, "function")}, ${estimate.local ? "free" : dollars(estimate.dollars)}`);
      translated = await invoke("translate");
    }
    running = "add";
    notice("busy", `Adding ${modName} to "${added.instance}"`, "Packing the mod");
    const again = await invoke("install_port", { id: added.instanceId });
    if (again.needsBuild) {
      running = "build";
      notice("busy", `Building "${again.instance}"`, "Waiting for your answer");
      // New code: the app asks again whether the player trusts it.
      const built = await invoke("build_installed", { id: again.instanceId, trusted: false });
      if (!built) {
        notice("warn", `${possessive(modName)} code is translated, not built yet`, "Build when you're ready", [
          button(`Build "${again.instance}"`, () => translateNow(ported, added), "play", "primary"),
        ]);
        return;
      }
    }
    finish(ported, again, translated || { ported: 0, unchecked: 0, failed: [], left: 0, stopped: null }, estimate);
  } catch (e) {
    const text = String(e);
    notice("bad", "It stopped", text.split("\n").filter(Boolean).slice(-1)[0] || text, [
      button("Try again", () => translateNow(ported, added), "again", "primary"),
    ]);
  } finally {
    running = null;
    $("go").disabled = false;
  }
}

async function playInstance(added) {
  try {
    await invoke("play_installed", { id: added.instanceId });
  } catch (e) {
    notice("bad", "Could not start the game", String(e));
  }
}

listen("port-progress", (e) => {
  if (running === "port") setDetail(e.payload);
});
listen("translate-progress", (e) => {
  if (running !== "translate") return;
  const line = e.payload.trim();
  const step = line.match(/^\[(\d+)\/(\d+)\]\s+(\S+)/);
  if (step) setDetail(`Function ${step[1]} of ${step[2]}: ${step[3]}`, line);
  else if (line.startsWith("attempt")) setDetail("Fixing what the compiler found", line);
});
listen("build-progress", (e) => {
  if (running === "build") setDetail(progressText(e.payload), e.payload);
});

/** Port to a folder: for mod developers, who port, then read and edit the code. */
async function portToFolder() {
  if (running) return;
  closeMenu();
  running = "port";
  try {
    const done = await invoke("port", { choose: true });
    if (done) {
      notice("ok", `Wrote ${done.id}-horizon`, done.out, [
        button("Open folder", () => openPath(done.out), "folder"),
        button("Open PORTING.md", () => openPath(done.porting)),
      ]);
    }
  } catch (e) {
    notice("bad", "Could not port the mod", String(e));
  } finally {
    running = null;
  }
}

/* ---- The More menu ---- */

function closeMenu() {
  $("more-menu").hidden = true;
  $("more").setAttribute("aria-expanded", "false");
}

$("more").addEventListener("click", (e) => {
  e.stopPropagation();
  const menu = $("more-menu");
  menu.hidden = !menu.hidden;
  $("more").setAttribute("aria-expanded", String(!menu.hidden));
  if (!menu.hidden) menu.querySelector("button:not(:disabled)")?.focus();
});
document.addEventListener("click", (e) => {
  if (!$("more-menu").hidden && !e.target.closest(".menu-wrap")) closeMenu();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !$("more-menu").hidden) {
    closeMenu();
    $("more").focus();
  }
});
$("m-save").addEventListener("click", () => {
  closeMenu();
  saveReport();
});
$("m-folder").addEventListener("click", portToFolder);
$("m-settings").addEventListener("click", () => {
  closeMenu();
  openSettings();
});

/* ---- Translation settings. The key goes to Rust once, into the credential store,
   and never comes back: the page only learns whether one is saved. ---- */

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
$("settings").addEventListener("close", async () => {
  $("key-input").value = "";
  // Back on the summary: the model, its price or the missing key may have changed.
  if (plan && $("go-sheet").open && plan.ported.native) {
    try {
      plan.estimate = await invoke("translate_estimate");
      renderSummary();
    } catch {
      /* the summary keeps what it showed */
    }
  }
});

/* ---- Wiring ---- */

$("go").addEventListener("click", makeItWork);
$("go-run").addEventListener("click", onRun);
$("go-skip").addEventListener("click", () => run(true));
$("go-change").addEventListener("click", openSettings);
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
  if (running) return;
  welcomeError(null);
  showView("welcome");
});

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
