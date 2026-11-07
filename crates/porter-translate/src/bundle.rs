// Generates a horizon-mixins bundle (a Horizon native mod) for a client mod: the
// manifest, compatibility.json, a Cargo.toml for `cargo test`, the mod's module
// (runtime.rs) and one helper per hook point, plus PORTING.md.
//
// The helpers follow the patterns of Horizon's own native mods (horizon-mods'
// native/zoom) and the documented hook signatures, for the hooks in
// porter_map::WIRED_HOOKS. Other mapped hooks are listed in PORTING.md.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::Path;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use porter_model::{ModScan, Verdict};

use crate::block_entity::{self, BlockEntity};
use crate::blocks::{self, Behaviour};
use crate::content;
use crate::machine::{self, Machine};
use crate::netconf::{self, Message, Setting};
use crate::screens::{self, ModScreen};
use porter_map::{hooks, ledger};

/// The Horizon commit a port names, and the horizon-mixins commit Horizon's own
/// native mods were tested on (horizon-mods native/zoom, 2026-11). The Horizon one
/// is the commit `data/horizon-hooks.json` was read from (Nightly 2026-11-05), so
/// every hook a port uses is in it. The launcher builds a port on the Horizon the
/// player plays when its hook points version is new enough; this is the fallback.
pub const HORIZON_REVISION: &str = "66f3981f522b24c250b21523a70080965b940424";
pub const HORIZON_MIXINS_REVISION: &str = "d9c83d7096b7ea5b83678da2efdfd6161f74997e";

/// A generated bundle: files by path relative to the output folder.
pub struct Bundle {
    /// The mod id used in Horizon: `a-z`, `0-9` and `_`.
    pub id: String,
    pub files: BTreeMap<String, String>,
    /// Files copied from the Java mod (its resources), by path in the output folder.
    pub copies: BTreeMap<String, PathBuf>,
    /// Whether it has a native half (a horizon-mixins bundle). A mod with only blocks,
    /// items and resources is a content mod alone, which needs no build.
    pub native: bool,
    /// The hook points version the native half needs.
    pub hooks: u32,
    /// The content mod under `content/`: its blocks, items and what was left out.
    pub content: content::Content,
    pub block_entities: Vec<BlockEntity>,
    /// Block entities with slots and a menu.
    pub machines: Vec<Machine>,
    /// Hooks wired completely, with the Java classes that need each.
    pub wired: BTreeMap<&'static str, BTreeSet<String>>,
    /// Hooks with a generated helper whose body is left to port: the Java classes
    /// and Mixins that need each, and what in them.
    pub stubbed: BTreeMap<&'static str, Uses>,
    /// Hooks the mod needs that Horizon's hook table doesn't have, or a helper
    /// can't take: wired by hand.
    pub unwired: BTreeMap<&'static str, Uses>,
}

/// Who needs a hook: the Java classes (or Mixins), and what in them (the API or
/// the Mixin member).
pub type Uses = (BTreeSet<String>, BTreeSet<String>);

/// Translation keys and their English text, from the mod's `en_us.json` files.
pub type Lang = BTreeMap<String, String>;

/// Reads `assets/<namespace>/lang/en_us.json` under each of the scan's resource roots.
pub fn load_lang(scan: &ModScan) -> Lang {
    let mut lang = Lang::new();
    let root = Path::new(&scan.input);
    for res in &scan.resources.roots {
        for ns in &scan.resources.namespaces {
            let path = root
                .join(res)
                .join("assets")
                .join(ns)
                .join("lang/en_us.json");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(&text) {
                for (k, v) in map {
                    if let Some(s) = v.as_str() {
                        lang.entry(k).or_insert_with(|| s.to_string());
                    }
                }
            }
        }
    }
    lang
}

/// Generates the bundle for a scanned mod.
pub fn generate(scan: &ModScan, lang: &Lang) -> Bundle {
    generate_with(scan, lang, None)
}

/// [`generate`], reading a Horizon source checkout (`horizon`) to point each Mixin
/// no hook stands for at the Horizon functions that port its target.
pub fn generate_with(scan: &ModScan, lang: &Lang, horizon: Option<&Path>) -> Bundle {
    let meta = scan.main_metadata().filter(|m| m.id.is_some());
    let java_id = meta
        .and_then(|m| m.id.clone())
        .unwrap_or_else(|| "ported".into());
    let id = sanitize_id(&java_id);
    let name = meta
        .and_then(|m| m.name.clone())
        .unwrap_or_else(|| java_id.clone());
    let version = meta
        .and_then(|m| m.version.clone())
        .filter(|v| !v.contains('$') && !v.is_empty())
        .map_or_else(|| "0.1.0".into(), |v| semver(&v));
    let description = meta
        .and_then(|m| m.description.clone())
        .unwrap_or_else(|| format!("{name}, ported to Horizon."));

    // Which hooks the mod needs, and which of its classes need each.
    let mut wired: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    let mut unwired: BTreeMap<&'static str, Uses> = BTreeMap::new();
    for class in &scan.classes {
        for r in &class.references {
            if let Some(mapping) = porter_map::client_hooks(r) {
                for hook in mapping.hooks {
                    if porter_map::WIRED_HOOKS.contains(hook) {
                        wired.entry(hook).or_default().insert(class.name.clone());
                    } else {
                        let entry = unwired.entry(hook).or_default();
                        entry.0.insert(class.name.clone());
                        entry.1.insert(mapping.java.to_string());
                    }
                }
                continue;
            }
            // Loader events Horizon's hook docs name (`LivingEvent.LivingJumpEvent`).
            for hook in hooks::event_hooks(r) {
                let entry = unwired.entry(hook.name.as_str()).or_default();
                entry.0.insert(class.name.clone());
                entry.1.insert(r.clone());
            }
        }
    }
    // Mixins whose target method a hook stands for.
    for (mx, member, hook, how) in mixin_hooks(scan) {
        // A hook the porter wires completely (the HUD, the tick...) calls runtime.rs.
        if let Some(wired_hook) = porter_map::WIRED_HOOKS.iter().find(|w| **w == hook.name) {
            wired.entry(wired_hook).or_default().insert(mx.name.clone());
            continue;
        }
        let entry = unwired.entry(hook.name.as_str()).or_default();
        entry.0.insert(mx.name.clone());
        entry.1.insert(format!(
            "`@{}` {} on {} ({})",
            member.annotation,
            member.member,
            member
                .target_methods
                .iter()
                .map(|t| format!("`{}`", t.split('(').next().unwrap_or(t)))
                .collect::<Vec<_>>()
                .join(", "),
            how.describe()
        ));
    }
    for key in &scan.client.keys {
        wired.entry("key").or_default().insert(key.class.clone());
    }
    if !scan.client.commands.is_empty() {
        wired.entry("command").or_default();
    }
    // Every mod has its row in Options > Mods.
    wired.entry("mods").or_default();
    if wired.contains_key("settings") {
        wired.entry("settings_click").or_default();
    }
    // Without keys there is nothing for a key hook to match.
    if scan.client.keys.is_empty()
        && let Some(classes) = wired.remove("key")
    {
        let entry = unwired.entry("key").or_default();
        entry.0.extend(classes);
        entry
            .1
            .insert("key input (no key bindings found to match)".into());
    }

    // Blocks, items and resources: a content mod. Block entities: native hooks.
    let ns = content::namespace(&java_id, &id).to_string();
    let block_entities = block_entity::find(scan, &ns);
    let mut machines = machine::find(scan, &ns, &id, &block_entities);
    let recipe_types = machine::recipe_types(scan, &ns);
    let machine_blocks: BTreeMap<String, String> = machines
        .iter()
        .flat_map(|m| {
            let menu = m.menu_class.rsplit('.').next().unwrap_or(&m.menu_class);
            let prefix = format!("{ns}:");
            m.blocks.iter().filter_map(move |b| {
                let name = b.strip_prefix(&prefix)?;
                Some((name.to_string(), menu.to_string()))
            })
        })
        .collect();
    let mut content = content::generate_with(
        scan,
        &id,
        &java_id,
        &name,
        &version,
        &description,
        &machine_blocks,
    );
    if !content.is_empty() {
        content.files.extend(machine::content_files(&machines));
    }
    // Blocks and items that act in code: placed, used or reshaped by their own Java.
    let behaviours = blocks::find(scan, &ns, &content.blocks, &content.items);
    for (hook, classes) in blocks::hooks(&behaviours) {
        wired.entry(hook).or_default().extend(classes);
    }
    let behaviour_blocks: Vec<(String, porter_content::Block)> = content
        .blocks
        .iter()
        .map(|b| (format!("{ns}:{}", b.name), b.clone()))
        .collect();
    // A screen texture the mod doesn't ship: Horizon draws its plain panel instead.
    for m in &mut machines {
        let shipped = m.texture.as_deref().is_some_and(|t| {
            let (tns, path) = t.split_once(':').unwrap_or((&ns, t));
            content
                .copies
                .contains_key(&format!("assets/{tns}/textures/{path}.png"))
        });
        if !shipped {
            m.texture = None;
        }
    }
    let mut hooks = 1;
    let mut stubbed: BTreeMap<&'static str, Uses> = BTreeMap::new();
    if !block_entities.is_empty() {
        hooks = block_entity::HOOKS_VERSION;
        let classes: BTreeSet<String> = block_entities
            .iter()
            .filter_map(|b| b.class.clone())
            .collect();
        wired.insert("block_entity", classes.clone());
        if block_entities.iter().any(|b| b.ticks) {
            wired.insert("block_entity_ticks", classes.clone());
            wired.insert("block_entity_tick", classes);
        }
    }
    // Block entities with fluid tanks or an energy buffer.
    let tanked: BTreeSet<String> = block_entities
        .iter()
        .filter(|b| !b.tanks.is_empty())
        .filter_map(|b| b.class.clone())
        .collect();
    if !tanked.is_empty() {
        wired.insert("block_entity_fluids", tanked);
    }
    let powered: BTreeSet<String> = block_entities
        .iter()
        .filter(|b| b.energy.is_some())
        .filter_map(|b| b.class.clone())
        .collect();
    if !powered.is_empty() {
        wired.insert("block_entity_energy", powered);
    }
    // Block entities with a renderer: drawn on the client from what the server
    // sends of their state.
    let drawn: BTreeSet<String> = block_entities
        .iter()
        .filter_map(|b| b.renderer.clone())
        .collect();
    if !drawn.is_empty() {
        wired.insert("block_entity_update_tag", drawn.clone());
        wired.insert("block_entity_submit", drawn);
        // The models its renderers draw as parts, baked beside the blocks'.
        if !scan.standalone_models.is_empty() {
            wired.insert("standalone_models", BTreeSet::new());
        }
    }
    if !machines.is_empty() && !content.is_empty() {
        let menus: BTreeSet<String> = machines.iter().map(|m| m.menu_class.clone()).collect();
        let screens: BTreeSet<String> = machines
            .iter()
            .filter_map(|m| m.screen_class.clone())
            .collect();
        wired.insert("block_entity_container", menus);
        wired.insert("screen", screens.clone());
        wired.insert("screen_draw", screens);
    }
    // Network messages: decoded where they arrive, handled in runtime.rs.
    let messages = netconf::messages(&scan.payloads, &id);
    for (hook, wanted) in [("net_from_client", true), ("net_from_server", false)] {
        let classes: BTreeSet<String> = messages
            .iter()
            .filter(|m| if wanted { m.to_server } else { m.to_client })
            .map(|m| m.class.clone())
            .collect();
        if !classes.is_empty() {
            wired.insert(hook, classes);
        }
    }
    // The mod's own screens: opened over the world, drawn and clicked through the
    // screen hooks.
    let mod_screens = screens::find(&scan.custom_screens);
    if !mod_screens.is_empty() {
        let classes: BTreeSet<String> = mod_screens.iter().map(|s| s.class.clone()).collect();
        for hook in screens::HOOKS {
            wired.insert(hook, classes.clone());
        }
    }
    // What the mod draws over game screens and the clicks it takes there
    // (`ScreenEvent.Render.Post`, Fabric's `ScreenEvents`, Mixins into
    // `Screen.render`): ported as plain drawing, like its own screens.
    let overlays: BTreeSet<String> = ["screen_render_post", "screen_mouse_button_pressed_pre"]
        .iter()
        .filter_map(|h| unwired.get(h))
        .flat_map(|(classes, _)| classes.iter().cloned())
        .collect();
    if !overlays.is_empty() {
        for hook in ["screen_render_post", "screen_mouse_button_pressed_pre"] {
            wired
                .entry(hook)
                .or_default()
                .extend(overlays.iter().cloned());
        }
    }
    // The config: kept in Horizon's mod settings, changed on the mod's page.
    let settings = netconf::settings(&scan.config);
    if !settings.is_empty() {
        let classes: BTreeSet<String> = scan.config.iter().map(|c| c.class.clone()).collect();
        wired.entry("settings").or_default().extend(classes);
        wired.entry("settings_click").or_default();
    }
    // Every other hook Horizon has gets a helper with its bindings, to port into.
    for (hook, uses) in std::mem::take(&mut unwired) {
        if wired.contains_key(hook) {
            continue;
        }
        let bindable = hooks::hook(hook).and_then(|h| h.bindings()).is_some();
        if bindable {
            stubbed.insert(hook, uses);
        } else {
            unwired.insert(hook, uses);
        }
    }
    for name in wired.keys().chain(stubbed.keys()) {
        if let Some(h) = hooks::hook(name) {
            hooks = hooks.max(h.needs());
        }
    }
    // The Java statics the hooked code shares (`SteadyView.pickups`).
    let roots: BTreeSet<String> = wired
        .values()
        .flatten()
        .chain(stubbed.values().flat_map(|(classes, _)| classes))
        .chain(unwired.values().flat_map(|(classes, _)| classes))
        .cloned()
        .chain(block_entities.iter().filter_map(|b| b.class.clone()))
        .collect();
    let java_statics = crate::statics::find(scan, &roots);
    // Only an Options > Mods row: the content mod is the whole port.
    let native = content.is_empty() || !stubbed.is_empty() || wired.keys().any(|h| *h != "mods");

    let ctx = Ctx {
        id: &id,
        module: format!("ported_{id}"),
        name: &name,
        version: &version,
        description: &description,
        scan,
        lang,
        wired: &wired,
        stubbed: &stubbed,
        hooks,
        block_entities: &block_entities,
        machines: &machines,
        recipe_types: &recipe_types,
        messages: &messages,
        settings: &settings,
        screens: &mod_screens,
        overlays: &overlays,
        behaviours: &behaviours,
        behaviour_blocks: &behaviour_blocks,
        statics: &java_statics,
        ledger: horizon.map(|src| (src.to_path_buf(), unmapped_ports(scan, src))),
    };
    let mut files = BTreeMap::new();
    if native {
        let dir = format!("mixins/{id}");
        files.insert("Cargo.toml".into(), ctx.cargo_toml());
        files.insert("compatibility.json".into(), compatibility_json(hooks));
        // Makes the folder an unpacked .hnmod that needs a hook points version
        // instead of pinning a commit, so the launcher builds it on the Horizon the
        // player plays.
        files.insert(
            "horizon-native.json".into(),
            native_manifest(
                &id,
                &version,
                HORIZON_REVISION,
                HORIZON_MIXINS_REVISION,
                hooks,
            ),
        );
        files.insert(format!("{dir}/horizon-mixin.toml"), ctx.manifest());
        files.insert(format!("{dir}/runtime.rs"), ctx.runtime());
        for hook in wired.keys() {
            let (file, body) = ctx.helper(hook);
            files.insert(format!("{dir}/{file}"), body);
        }
        for (hook, uses) in &stubbed {
            if let Some((file, body)) = ctx.stub_helper(hook, uses) {
                files.insert(format!("{dir}/{file}"), body);
            }
        }
    }
    for (rel, text) in &content.files {
        files.insert(format!("content/{rel}"), text.clone());
    }
    let copies = content
        .copies
        .iter()
        .map(|(rel, src)| (format!("content/{rel}"), src.clone()))
        .collect();
    let bundle = Bundle {
        id: id.clone(),
        files,
        copies,
        native,
        hooks,
        content,
        block_entities: block_entities.clone(),
        machines: machines.clone(),
        wired: if native {
            wired.clone()
        } else {
            BTreeMap::new()
        },
        stubbed: stubbed.clone(),
        unwired,
    };
    let porting = ctx.porting_md(&bundle);
    let mut bundle = bundle;
    bundle.files.insert("PORTING.md".into(), porting);
    // Where the Java came from, for `translate`.
    bundle.files.insert(
        ".porter/port.json".into(),
        serde_json::to_string_pretty(&serde_json::json!({
            "input": std::path::absolute(&scan.input)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| scan.input.clone()),
            "id": bundle.id,
            "tool": env!("CARGO_PKG_VERSION"),
        }))
        .unwrap_or_default()
            + "\n",
    );
    bundle
}

/// Writes the bundle into `out`. Refuses a folder that already has files, unless
/// `overwrite` is set; then only the bundle's own files are replaced.
pub fn write(bundle: &Bundle, out: &Path, overwrite: bool) -> Result<()> {
    if out.exists() {
        let has_files = fs::read_dir(out)
            .with_context(|| format!("reading {}", out.display()))?
            .next()
            .is_some();
        if has_files && !overwrite {
            bail!(
                "{} already has files; choose an empty folder, or pass --force to replace the generated files in it",
                out.display()
            );
        }
        // The content mod is all generated: a file an earlier port wrote that this
        // one doesn't (a block the porter no longer declares) would still load.
        let content = out.join("content");
        if overwrite && content.join("horizon-mod.toml").is_file() {
            fs::remove_dir_all(&content)
                .with_context(|| format!("clearing {}", content.display()))?;
        }
    }
    for (rel, content) in &bundle.files {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        // Writing a mod's Rust again keeps each function already ported in it, by
        // hand or by `translate`: only the stubs are written over.
        let content = match fs::read_to_string(&path) {
            Ok(existing) if rel.ends_with(".rs") => {
                crate::ai::units::keep_ported(content, &existing)
            }
            _ => content.clone(),
        };
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    }
    for (rel, src) in &bundle.copies {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::copy(src, &path)
            .with_context(|| format!("copying {} to {}", src.display(), path.display()))?;
    }
    Ok(())
}

/// A Horizon mod id: lowercase `a-z`, `0-9` and `_`, starting with a letter.
pub fn sanitize_id(java_id: &str) -> String {
    let mut id: String = java_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        id.insert_str(0, "mod_");
    }
    id
}

fn compatibility_json(hooks: u32) -> String {
    format!(
        "{{\n  \"game\": \"26.3\",\n  \"hooks\": {hooks},\n  \"horizon_repository\": \"https://github.com/horizon-rust/horizon\",\n  \"horizon_revision\": \"{HORIZON_REVISION}\",\n  \"horizon_mixins_repository\": \"https://github.com/horizon-rust/horizon-mixins\",\n  \"horizon_mixins_revision\": \"{HORIZON_MIXINS_REVISION}\"\n}}\n"
    )
}

/// `horizon-native.json`, as horizon-mods' `native.py package` writes it: schema 1,
/// keys sorted, two-space indent. `horizon` is the commit the mod was tested on;
/// `hooks` is the hook points version it needs.
pub fn native_manifest(id: &str, version: &str, horizon: &str, mixins: &str, hooks: u32) -> String {
    let manifest = serde_json::json!({
        "game": "26.3",
        "hooks": hooks,
        "horizon": horizon,
        "id": id,
        "mixins": mixins,
        "schema": 1,
        "version": version,
    });
    // serde_json keeps object keys sorted (BTreeMap), as `sort_keys=True` does.
    serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n"
}

/// Packs a bundle folder (as `port` writes it, edited or not) into a `.hnmod`, the
/// way horizon-mods' `native.py package` does: `horizon-native.json`, the `.rs` and
/// `.toml` files under `mixins/<id>/` and the folder's LICENSE, sorted, with fixed
/// timestamps so the same sources give the same file. The content mod under
/// `content/`, if there is one, becomes a `.hmod` beside it. Returns their paths.
pub fn package(dir: &Path, out_dir: &Path) -> Result<Vec<PathBuf>> {
    let native = dir.join("mixins").is_dir();
    let content = dir.join("content").join("horizon-mod.toml").is_file();
    if !native && !content {
        bail!(
            "{} has neither a mixins/ folder nor content/horizon-mod.toml: is it a folder `port` wrote?",
            dir.display()
        );
    }
    let mut out = Vec::new();
    if native {
        out.push(package_native(dir, out_dir)?);
    }
    if content {
        out.push(package_content(&dir.join("content"), out_dir)?);
    }
    Ok(out)
}

/// Packs a content mod folder (`horizon-mod.toml`, `assets/`, `data/`) into a `.hmod`.
fn package_content(dir: &Path, out_dir: &Path) -> Result<PathBuf> {
    let manifest: toml::Table = toml::from_str(&fs::read_to_string(dir.join("horizon-mod.toml"))?)
        .with_context(|| format!("reading {}", dir.join("horizon-mod.toml").display()))?;
    let field = |key: &str| {
        manifest
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .with_context(|| format!("content/horizon-mod.toml has no {key}"))
    };
    let (id, version) = (field("id")?, field("version")?);
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(
        "horizon-mod.toml".into(),
        fs::read(dir.join("horizon-mod.toml"))?,
    );
    for top in ["assets", "data"] {
        let mut stack = vec![dir.join(top)];
        while let Some(d) = stack.pop() {
            let Ok(entries) = fs::read_dir(&d) else {
                continue;
            };
            for entry in entries {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.insert(rel, fs::read(&path)?);
                }
            }
        }
    }
    let out = out_dir.join(format!("{id}-{version}.hmod"));
    write_zip(&out, &files)?;
    Ok(out)
}

/// Packs the native half into a `.hnmod`.
fn package_native(dir: &Path, out_dir: &Path) -> Result<PathBuf> {
    let mixins = dir.join("mixins");
    let mut bundles = fs::read_dir(&mixins)
        .with_context(|| {
            format!(
                "{} has no mixins/ folder: is it a bundle `port` wrote?",
                dir.display()
            )
        })?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("horizon-mixin.toml").is_file())
        .map(|e| e.path())
        .collect::<Vec<_>>();
    bundles.sort();
    let [bundle] = bundles.as_slice() else {
        bail!(
            "{} should hold exactly one bundle with a horizon-mixin.toml, found {}",
            mixins.display(),
            bundles.len()
        );
    };
    let manifest_text = fs::read_to_string(bundle.join("horizon-mixin.toml"))?;
    let manifest: toml::Table = toml::from_str(&manifest_text)
        .with_context(|| format!("reading {}", bundle.join("horizon-mixin.toml").display()))?;
    let id = manifest
        .get("mod")
        .and_then(|m| m.get("id"))
        .and_then(|v| v.as_str())
        .context("horizon-mixin.toml has no [mod] id")?;
    let version = manifest
        .get("mod")
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str())
        .context("horizon-mixin.toml has no [mod] version")?;
    let pinned = manifest
        .get("compatibility")
        .and_then(|c| c.get("horizon_revision"))
        .and_then(|v| v.as_str())
        .unwrap_or(HORIZON_REVISION);
    let compat: serde_json::Value = fs::read_to_string(dir.join("compatibility.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if let Some(rev) = compat.get("horizon_revision").and_then(|v| v.as_str())
        && rev != pinned
    {
        bail!(
            "horizon-mixin.toml names commit {pinned} but compatibility.json names {rev}: make them the same"
        );
    }
    let mixins_rev = compat
        .get("horizon_mixins_revision")
        .and_then(|v| v.as_str())
        .unwrap_or(HORIZON_MIXINS_REVISION);
    let hooks = compat
        .get("hooks")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(1);

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert(
        "horizon-native.json".into(),
        native_manifest(id, version, pinned, mixins_rev, hooks).into_bytes(),
    );
    let mut stack = vec![bundle.clone()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "toml")
            ) {
                let rel = path
                    .strip_prefix(bundle)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(format!("mixins/{id}/{rel}"), fs::read(&path)?);
            }
        }
    }
    if let Ok(licence) = fs::read(dir.join("LICENSE")) {
        files
            .entry(format!("mixins/{id}/LICENSE"))
            .or_insert(licence);
    }

    let out = out_dir.join(format!("{id}-{version}.hnmod"));
    write_zip(&out, &files)?;
    Ok(out)
}

/// Writes `files` into a zip at `out`, sorted, with fixed times and modes, so the
/// same files give the same zip.
fn write_zip(out: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut zip = zip::ZipWriter::new(fs::File::create(out)?);
    let time = zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0)
        .map_err(|e| anyhow::anyhow!("zip time: {e:?}"))?;
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(time)
        .unix_permissions(0o644);
    for (name, content) in files {
        zip.start_file(name.as_str(), options)?;
        std::io::Write::write_all(&mut zip, content)?;
    }
    zip.finish()?;
    Ok(())
}

struct Ctx<'a> {
    id: &'a str,
    module: String,
    name: &'a str,
    version: &'a str,
    description: &'a str,
    scan: &'a ModScan,
    lang: &'a Lang,
    wired: &'a BTreeMap<&'static str, BTreeSet<String>>,
    stubbed: &'a BTreeMap<&'static str, Uses>,
    hooks: u32,
    block_entities: &'a [BlockEntity],
    machines: &'a [Machine],
    /// The mod's own recipe types: `grind:crushing`.
    recipe_types: &'a [String],
    /// Its network messages.
    messages: &'a [Message],
    /// Its config values.
    settings: &'a [Setting],
    /// Its own screens.
    screens: &'a [ModScreen],
    /// Its classes that draw over game screens.
    overlays: &'a BTreeSet<String>,
    /// Its blocks that act in code, and every block (with its id).
    behaviours: &'a [Behaviour],
    behaviour_blocks: &'a [(String, porter_content::Block)],
    /// The Java statics the mod's code shares.
    statics: &'a [crate::statics::Module],
    /// The Horizon checkout read, and the functions that port each unmapped
    /// Mixin's target (`FarmBlock.turnToDirt`).
    ledger: Option<(PathBuf, BTreeMap<String, Vec<ledger::Port>>)>,
}

impl Ctx<'_> {
    fn uses(&self, hook: &str) -> bool {
        self.wired.contains_key(hook)
    }

    /// The Java classes a hook replaces, for comments.
    fn java_for(&self, hook: &str) -> String {
        match self.wired.get(hook) {
            Some(classes) if !classes.is_empty() => classes
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            _ => "the mod".into(),
        }
    }

    fn cargo_toml(&self) -> String {
        format!(
            r#"[package]
name = "horizon-port-{dash}"
version = "{version}"
edition = "2024"
publish = false
description = "{name}, ported to Horizon with horizon-porter."

# The mod's logic and its tests: the same file the bundle adds to the game.
[lib]
path = "mixins/{id}/runtime.rs"

# Standalone: `cargo test` works wherever this folder is.
[workspace]
"#,
            dash = self.id.replace('_', "-"),
            version = self.version,
            name = toml_str(self.name),
            id = self.id,
        )
    }

    fn manifest(&self) -> String {
        let mut m = format!(
            r#"# {name}, ported from Java with horizon-porter {tool}.
[mod]
id = "{id}"
version = "{version}"
priority = 100

# The commit it was made for. It uses only Horizon's native hook points (hooks {hooks}
# in compatibility.json), so the launcher builds it on any Horizon that has them.
[compatibility]
horizon_revision = "{HORIZON_REVISION}"

# runtime.rs becomes the module crate::game::{module}.
[[module]]
id = "runtime"
file = "crates/horizon-client/src/game.rs"
name = "{module}"
source = "runtime.rs"
"#,
            name = self.name,
            tool = env!("CARGO_PKG_VERSION"),
            id = self.id,
            version = self.version,
            module = self.module,
            hooks = self.hooks,
        );
        for hook in self.wired.keys() {
            let (file, _) = self.helper(hook);
            let line = |n: &str, ty: &str, v: &str| {
                format!("  {{ name = \"{n}\", ty = \"{ty}\", value = \"{v}\" }},\n")
            };
            // A hook the porter wires with its catalog signature (a block's
            // `update_shape`, on the level) takes its target and bindings from there.
            let catalog = hooks::hook(hook)
                .filter(|_| bindings(hook).is_empty())
                .and_then(|h| Some((h.target.clone(), h.bindings()?)));
            let (target, bindings) = match catalog {
                Some((target, found)) => (
                    target,
                    found
                        .iter()
                        .map(|b| line(&b.name, &b.ty, &b.value))
                        .collect::<String>(),
                ),
                None => (
                    format!("horizon_client::game::native_hooks::Game<'_>::native_{hook}"),
                    bindings(hook)
                        .iter()
                        .map(|(n, ty, v)| line(n, ty, v))
                        .collect::<String>(),
                ),
            };
            let _ = write!(
                m,
                "\n[[injection]]\nid = \"{id}\"\nkind = \"HEAD\"\ntarget = \"{target}\"\nhelper = \"{file}\"\nbindings = [\n{bindings}]\n",
                id = hook.replace('_', "-"),
            );
        }
        for hook in self.stubbed.keys() {
            let Some(h) = hooks::hook(hook) else {
                continue;
            };
            let Some(bindings) = h.bindings() else {
                continue;
            };
            let bindings = bindings
                .iter()
                .map(|b| {
                    format!(
                        "  {{ name = \"{}\", ty = \"{}\", value = \"{}\" }},\n",
                        b.name, b.ty, b.value
                    )
                })
                .collect::<String>();
            let _ = write!(
                m,
                "\n[[injection]]\nid = \"{id}\"\nkind = \"HEAD\"\ntarget = \"{target}\"\nhelper = \"{hook}.rs\"\nbindings = [\n{bindings}]\n",
                id = hook.replace('_', "-"),
                target = h.target,
            );
        }
        m
    }

    /// A helper's line that loads the config the first time, if the mod has one.
    fn ensure_config(&self, game: &str) -> String {
        if self.settings.is_empty() {
            String::new()
        } else {
            format!(
                "    {{\n        let game: &crate::game::Game<'_> = {game};\n        m::ensure_config(|key| game.native_setting(m::ID, key));\n    }}\n"
            )
        }
    }

    fn stub_helper(&self, hook: &str, uses: &Uses) -> Option<(String, String)> {
        hook_helper(self.id, &self.module, hook, uses)
    }

    fn runtime(&self) -> String {
        let mut r = format!(
            r#"// {name}, ported from Java with horizon-porter {tool}.
//
// This file is both the mod's Cargo library (its tests run with `cargo test`) and
// the module the bundle adds to the game as crate::game::{module}. The hook helpers
// next to it call the functions below. Each one marked TODO names the Java code to
// port into it; PORTING.md lists everything left.

pub const ID: &str = "{id}";
pub const VERSION: &str = "{version}";
pub const TITLE: &str = "{title}";
pub const DESCRIPTION: &str = "{description}";
"#,
            name = self.name,
            tool = env!("CARGO_PKG_VERSION"),
            module = self.module,
            id = self.id,
            version = self.version,
            title = rust_str(self.name),
            description = rust_str(self.description),
        );

        r.push_str(
            "\n/// Key bindings: (name, label, default key). Players rebind them in Options > Mods.\npub const KEYS: &[(&str, &str, &str)] = &[\n",
        );
        for key in &self.scan.client.keys {
            let label = self
                .lang
                .get(&key.name)
                .cloned()
                .unwrap_or_else(|| key.name.clone());
            let _ = writeln!(
                r,
                "    (\"{}\", \"{}\", \"{}\"),",
                rust_str(&key.name),
                rust_str(&label),
                rust_str(&key.default_key)
            );
        }
        r.push_str("];\n");
        r.push_str(PLAYER_TYPE);
        r.push_str(&crate::statics::runtime(self.statics));

        if self.uses("command") {
            r.push_str("\n/// Commands this mod answers, without the slash.\npub const COMMANDS: &[&str] = &[");
            r.push_str(
                &self
                    .scan
                    .client
                    .commands
                    .iter()
                    .map(|c| format!("\"{}\"", rust_str(c)))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            r.push_str("];\n");
        }

        if self.uses("key") {
            let _ = write!(
                r,
                "\n/// One of KEYS went down or up.\n///\n/// TODO: port from {}.\npub fn on_key(name: &str, pressed: bool, player: Option<Player>) {{\n    let _ = (name, pressed, player);\n}}\n",
                self.java_for("key")
            );
        }
        if self.uses("command") {
            let _ = write!(
                r,
                "\n/// The player typed `/{{command}} {{args}}` for one of COMMANDS. Returns the chat\n/// reply, if any.\n///\n/// TODO: port from {}.\npub fn on_command(command: &str, args: &str, player: Option<Player>) -> Option<String> {{\n    let _ = (args, player);\n    Some(format!(\"/{{command}} is not ported yet\"))\n}}\n",
                self.java_for("command")
            );
        }
        if self.uses("tick") {
            let _ = write!(
                r,
                "\n/// Once a client tick (20 a second), with the player when in a world.\n///\n/// TODO: port from {}.\npub fn tick(player: Option<Player>) {{\n    let _ = player;\n}}\n",
                self.java_for("tick")
            );
        }
        if self.uses("hud") {
            let _ = write!(
                r,
                "\n/// Lines of text for the mod's HUD box (top right), while in a world; none hides it.\n///\n/// TODO: port from {}.\npub fn hud_lines(player: Player) -> Vec<String> {{\n    let _ = player;\n    Vec::new()\n}}\n",
                self.java_for("hud")
            );
        }
        if self.uses("wheel") {
            let _ = write!(
                r,
                "\n/// The mouse wheel turned while playing. Return true to keep the steps from\n/// the hotbar.\n///\n/// TODO: port from {}.\npub fn on_wheel(steps: f32) -> bool {{\n    let _ = steps;\n    false\n}}\n",
                self.java_for("wheel")
            );
        }
        if !self.settings.is_empty() {
            r.push_str(&netconf::config_runtime(
                self.settings,
                &self.java_for("settings"),
            ));
        } else if self.uses("settings") {
            let _ = write!(
                r,
                "\n/// The labels of the buttons on the mod's settings page (Options > Mods).\n/// `get` reads a saved setting.\n///\n/// TODO: port the config screen of {}.\npub fn settings_buttons(get: impl Fn(&str) -> Option<String>) -> Vec<String> {{\n    let _ = get;\n    Vec::new()\n}}\n\n/// A settings button was clicked: returns the setting to save, as (key, value).\npub fn settings_click(button: usize, get: impl Fn(&str) -> Option<String>) -> Option<(String, String)> {{\n    let _ = (button, get);\n    None\n}}\n",
                self.java_for("settings")
            );
        }

        if !self.block_entities.is_empty() {
            r.push_str(&block_entity::runtime(
                self.block_entities,
                self.machines,
                self.recipe_types,
            ));
        }
        if self.uses("standalone_models") {
            r.push_str("\n/// Models baked on their own, for renderers to draw as parts (`draw.model`).\npub const STANDALONE_MODELS: &[&str] = &[\n");
            for model in &self.scan.standalone_models {
                let _ = writeln!(r, "    {model:?},");
            }
            r.push_str("];\n");
        }
        r.push_str(&netconf::net_runtime(self.messages));
        r.push_str(&screens::runtime(self.screens, self.overlays));
        r.push_str(&blocks::runtime(self.behaviours, self.behaviour_blocks));

        r.push_str(
            r#"
#[cfg(test)]
mod tests {
    #[test]
    fn keys_use_vanilla_names() {
        for (name, label, default) in super::KEYS {
            assert!(!name.is_empty() && !label.is_empty());
            assert!(default.starts_with("key.keyboard.") || default.starts_with("key.mouse."));
        }
    }
"#,
        );
        r.push_str(&block_entity::runtime_tests(self.block_entities));
        if !self.block_entities.is_empty() {
            r.push_str(&machine::runtime_tests(self.machines));
        }
        r.push_str(&netconf::net_tests(self.messages));
        r.push_str(&netconf::config_tests(self.settings));
        r.push_str(&screens::tests(self.screens));
        r.push_str(&blocks::tests(self.behaviours));
        r.push_str("}\n");
        r
    }

    /// The helper file for a hook: (file name, source).
    fn helper(&self, hook: &str) -> (&'static str, String) {
        let id = self.id;
        let m = &self.module;
        let name = self.name;
        match hook {
            "mods" => (
                "list.rs",
                format!(
                    r#"// {name}'s row in Options > Mods, with its keys.
fn {id}_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {{
    use crate::game::{m} as m;
    mods.push(crate::game::native_hooks::NativeMod {{
        id: m::ID.into(),
        title: format!("{{}} {{}}", m::TITLE, m::VERSION),
        description: m::DESCRIPTION.into(),
        keys: m::KEYS
            .iter()
            .map(|(name, label, default)| [(*name).into(), (*label).into(), (*default).into()])
            .collect(),
    }});
}}
"#
                ),
            ),
            "key" => (
                "key.rs",
                format!(
                    r#"// A key went down or up: {name}'s keys, as the player bound them in Options > Mods.
fn {id}_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {{
    use crate::game::{m} as m;
    // Turned off in Options > Mods: nothing until the next build leaves it out.
    if !game.native_on(m::ID) {{
        return;
    }}
    {PLAYER}
    for (name, _, default) in m::KEYS {{
        if key == game.native_mod_key(m::ID, name, default) {{
            m::on_key(name, pressed, player);
        }}
    }}
}}
"#
                ),
            ),
            "command" => (
                "command.rs",
                format!(
                    r#"// {name}'s commands; every other command is left alone.
fn {id}_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {{
    use crate::game::{m} as m;
    if *ran || !game.native_on(m::ID) {{
        return;
    }}
    let (name, args) = command.split_once(' ').unwrap_or((command, ""));
    if !m::COMMANDS.contains(&name) {{
        return;
    }}
    *ran = true;
    {PLAYER}
    if let Some(reply) = m::on_command(name, args.trim(), player) {{
        game.native_chat(&reply);
    }}
}}
"#
                ),
            ),
            "hud" => (
                "hud.rs",
                format!(
                    r#"// {name}'s HUD box in the top right corner, under other mods' boxes there.
fn {id}_hud(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
    corners: &mut horizon_client::extras_hud::Corners,
) {{
    use crate::game::{m} as m;
    use horizon_client::extras::Corner;
    if !game.native_on(m::ID) {{
        return;
    }}
    {PLAYER}
    let Some(player) = player else {{
        return;
    }};
    let lines = m::hud_lines(player);
    if lines.is_empty() {{
        return;
    }}
    let w = lines
        .iter()
        .map(|l| game.native_text_width(l))
        .fold(0.0, f32::max)
        + 4.0;
    let h = lines.len() as f32 * 10.0 + 2.0;
    let (x, y) = corners.place(Corner::TopRight, w, h);
    game.native_fill(batch, [x, y, w, h], 0x7800_0000);
    for (i, line) in lines.iter().enumerate() {{
        game.native_text(batch, line, [x + 2.0, y + 2.0 + i as f32 * 10.0], 0xFFFFFF, true);
    }}
}}
"#
                ),
            ),
            "tick" => (
                "tick.rs",
                format!(
                    r#"// {name}'s client tick.
fn {id}_tick(game: &mut crate::game::Game<'_>) {{
    use crate::game::{m} as m;
    // In single player the server's game ticks too: client code runs in the window's.
    if !game.native_on(m::ID) || !game.native_window() {{
        return;
    }}
    {PLAYER}
    m::tick(player);
}}
"#
                ),
            ),
            "wheel" => (
                "wheel.rs",
                format!(
                    r#"// The mouse wheel turned while playing.
fn {id}_wheel(game: &crate::game::Game<'_>, steps: f32, claimed: &mut bool) {{
    use crate::game::{m} as m;
    if *claimed || !game.native_on(m::ID) {{
        return;
    }}
    if m::on_wheel(steps) {{
        *claimed = true;
    }}
}}
"#
                ),
            ),
            "settings" => (
                "settings.rs",
                format!(
                    r#"// {name}'s settings page in Options > Mods.
fn {id}_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {{
    use crate::game::{m} as m;
    if mod_id != m::ID {{
        return;
    }}
    let game = &*game;
    buttons.extend(m::settings_buttons(|key| game.native_setting(m::ID, key)));
}}
"#
                ),
            ),
            "settings_click" => (
                "click.rs",
                format!(
                    r#"// A click on {name}'s settings page: save what the mod says to save.
fn {id}_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {{
    use crate::game::{m} as m;
    if mod_id != m::ID {{
        return;
    }}
    let save = {{
        let game = &*game;
        m::settings_click(button, |key| game.native_setting(m::ID, key))
    }};
    if let Some((key, value)) = save {{
        game.native_set_setting(m::ID, &key, &value);
    }}
}}
"#
                ),
            ),
            "block_entity" => (
                "block_entity.rs",
                format!(
                    r#"// {name}'s blocks get their block entity, with their state's defaults saved.
fn {id}_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {{
    use crate::game::{m} as m;
    use horizon_world::nbt::Tag;
    if entity.is_some() {{
        return;
    }}
    let Some((_, id, _)) = m::BLOCK_ENTITIES.iter().find(|(b, _, _)| *b == block) else {{
        return;
    }};
    let mut nbt = horizon_world::nbt::Compound::new();
    for (key, value) in m::block_entity_defaults(id) {{
        let tag = {to_tag};
        nbt.insert(key.into(), tag);
    }}
    *entity = Some(((*id).into(), nbt));
}}
"#,
                    to_tag = block_entity::TO_TAG,
                ),
            ),
            "block_entity_ticks" => (
                "block_entity_ticks.rs",
                format!(
                    r#"// Which of {name}'s block entities tick.
fn {id}_block_entity_ticks(block: &str, ticks: &mut bool) {{
    use crate::game::{m} as m;
    if m::BLOCK_ENTITIES.iter().any(|(b, _, t)| *b == block && *t) {{
        *ticks = true;
    }}
}}
"#
                ),
            ),
            "block_entity_tick" => (
                "block_entity_tick.rs",
                format!(
                    r##"// A server tick of one of {name}'s block entities: runs its tick on the state
// saved with it, its slots and its data slots, and keeps what changed.
fn {id}_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {{
    use crate::game::{m} as m;
    use horizon_world::nbt::{{JavaString, Tag}};
    if !m::BLOCK_ENTITIES.iter().any(|(_, id, _)| *id == entity.id) {{
        return;
    }}
    // Its own block, for a block entity several blocks share.
    let block_here = game.native_block(pos);
    // The blocks beside it, for a tick that looks at them: read before the tick,
    // since a mod block entity can only be reached outside one.
    let neighbours: Vec<Option<m::Neighbour>> = if m::NEIGHBOURS.contains(&entity.id.as_str()) {{
        let read = |tag: &Tag| -> Option<m::Saved> {{ {from_tag} }};
        m::Side::ALL
            .iter()
            .map(|side| {{
                let [dx, dy, dz] = side.offset();
                let at = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                let block = game.native_block(at);
                let found = game.native_with_mod_block_entity(at, |e| {{
                    let values: Vec<(String, m::Saved)> = e
                        .nbt
                        .iter()
                        .filter_map(|(k, tag)| read(tag).map(|v| (k.to_string_lossy(), v)))
                        .collect();
                    (e.id.clone(), values)
                }});
                Some(m::Neighbour {{
                    block,
                    entity: found.as_ref().map(|(id, _)| id.clone()),
                    values: found.map(|(_, v)| v).unwrap_or_default(),
                }})
            }})
            .collect()
    }} else {{
        (0..6).map(|_| None).collect()
    }};
    // An ingredient as Horizon reads one: an item, a `#tag`, or a list of them.
    // Older recipe files write `{{"item": ...}}` or `{{"tag": ...}}`.
    fn names(v: &serde_json::Value) -> Option<serde_json::Value> {{
        use serde_json::Value;
        let one = |v: &Value| match v {{
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => o
                .get("item")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| o.get("tag").and_then(Value::as_str).map(|t| format!("#{{t}}"))),
            _ => None,
        }};
        match v {{
            Value::Array(a) => Some(Value::Array(a.iter().filter_map(one).map(Value::String).collect())),
            other => one(other).map(Value::String),
        }}
    }}
    let recipes = |kind: &str| -> Vec<m::Recipe> {{
        game.native_recipes_of_type(kind)
            .iter()
            .map(|r| {{
                let json = &*r.json;
                let items = |v: &serde_json::Value| -> Vec<String> {{
                    names(v)
                        .and_then(|n| game.native_recipe_ingredient(&n))
                        .map(|i| i.order)
                        .unwrap_or_default()
                }};
                let mut ingredients = Vec::new();
                for key in ["ingredient", "input"] {{
                    if let Some(v) = json.get(key) {{
                        ingredients.push(items(v));
                    }}
                }}
                for key in ["ingredients", "inputs"] {{
                    if let Some(serde_json::Value::Array(list)) = json.get(key) {{
                        ingredients.extend(list.iter().map(|v| items(v)));
                    }}
                }}
                let result = json.get("result").and_then(|v| {{
                    let v = match v {{
                        serde_json::Value::String(s) => serde_json::json!({{ "id": s }}),
                        serde_json::Value::Object(o) if !o.contains_key("id") && o.contains_key("item") => {{
                            let mut o = o.clone();
                            if let Some(item) = o.remove("item") {{
                                o.insert("id".into(), item);
                            }}
                            serde_json::Value::Object(o)
                        }}
                        other => other.clone(),
                    }};
                    let stack = game.native_recipe_result(&v)?;
                    Some(m::Stack::new(&stack.item, stack.count))
                }});
                let values = json
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(k, v)| match v {{
                        serde_json::Value::String(s) => Some((k.clone(), s.clone())),
                        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Some((k.clone(), v.to_string())),
                        _ => None,
                    }})
                    .collect();
                m::Recipe {{
                    id: r.id.clone(),
                    ingredients,
                    result,
                    values,
                }}
            }})
            .collect()
    }};
    let slots: Vec<Option<m::Stack>> = entity
        .items
        .iter()
        .map(|s| s.as_ref().map(|s| m::Stack::new(&s.item, s.count)))
        .collect();
    let data = entity.data.clone();
    let mut machine = m::Machine::new(slots.clone(), data.clone(), &recipes);
    machine.neighbours = neighbours;
    machine.block = block_here;
    // Its tanks and energy, as Horizon keeps them: `fluids` (a list of
    // `{{Fluid, Amount}}`) and the int `energy`.
    let storage = m::STORAGE.iter().find(|(id, _, _)| *id == entity.id);
    if let Some((_, capacities, energy)) = storage {{
        let list = match entity.nbt.get(&JavaString::from("fluids")) {{
            Some(Tag::List(list)) => list.clone(),
            _ => Vec::new(),
        }};
        machine.tanks = capacities
            .iter()
            .enumerate()
            .map(|(i, capacity)| {{
                let (fluid, amount) = match list.get(i) {{
                    Some(Tag::Compound(map)) => (
                        match map.get(&JavaString::from("Fluid")) {{
                            Some(Tag::String(f)) if f.to_string_lossy() != "minecraft:empty" => Some(f.to_string_lossy()),
                            _ => None,
                        }},
                        match map.get(&JavaString::from("Amount")) {{
                            Some(Tag::Int(a)) => *a,
                            _ => 0,
                        }},
                    ),
                    _ => (None, 0),
                }};
                m::Tank {{ fluid, amount, capacity: *capacity }}
            }})
            .collect();
        machine.energy_capacity = energy.map_or(0, |e| e[0]);
        machine.energy = match entity.nbt.get(&JavaString::from("energy")) {{
            Some(Tag::Int(e)) => (*e).clamp(0, machine.energy_capacity),
            _ => 0,
        }};
    }}
    let tanks = machine.tanks.clone();
    let energy = machine.energy;
    let changed = {{
        let nbt = &entity.nbt;
        m::tick_block_entity(
            &entity.id,
            pos,
            |key| {{
                let tag = nbt.get(&JavaString::from(key))?;
                {from_tag}
            }},
            &mut machine,
        )
    }};
    let mut done = machine.into_parts();
    // What the tick sent to or took from the blocks beside it (`machine.push`,
    // `machine.pull`): this block entity is out of the world while it ticks, so
    // they move through its slots here.
    if !done.transfers.is_empty() {{
        use horizon_core::geometry::Direction;
        let dimension = game.native_dimension();
        for transfer in std::mem::take(&mut done.transfers) {{
            let (slot, side, max, push) = match transfer {{
                m::Transfer::Push {{ slot, side, max }} => (slot, side, max, true),
                m::Transfer::Pull {{ slot, side, max }} => (slot, side, max, false),
            }};
            let [dx, dy, dz] = side.offset();
            let next = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
            // The neighbour's side that faces this block.
            let face = match side {{
                m::Side::Down => Direction::Up,
                m::Side::Up => Direction::Down,
                m::Side::North => Direction::South,
                m::Side::South => Direction::North,
                m::Side::West => Direction::East,
                m::Side::East => Direction::West,
            }};
            if push {{
                let Some(Some(stack)) = done.slots.get(slot).cloned() else {{
                    continue;
                }};
                let n = stack.count.min(max);
                let left = game
                    .native_insert_item(next, face, horizon_client::items::ItemStack::new(&stack.item, n))
                    .map_or(0, |s| s.count);
                let moved = n - left.min(n);
                if let Some(Some(s)) = done.slots.get_mut(slot) {{
                    s.count -= moved;
                    if s.count == 0 {{
                        done.slots[slot] = None;
                    }}
                }}
            }} else {{
                let Some(held) = done.slots.get(slot).cloned() else {{
                    continue;
                }};
                let Some(found) = game.native_container_slots(dimension, next, face) else {{
                    continue;
                }};
                for source in found {{
                    let Some(have) = source.stack.as_ref().filter(|_| source.can_take) else {{
                        continue;
                    }};
                    // Only what fits: the same item, or anything into an empty slot.
                    let room = match &held {{
                        Some(h) if *h.item != *have.item => 0,
                        Some(h) => m::MAX_STACK.saturating_sub(h.count),
                        None => m::MAX_STACK,
                    }};
                    let want = room.min(max).min(have.count);
                    if want == 0 {{
                        continue;
                    }}
                    if let Some(got) = game.native_extract_item(dimension, next, face, source.slot, want) {{
                        let s = done.slots[slot].get_or_insert_with(|| m::Stack::new(&got.item, 0));
                        s.count += got.count;
                    }}
                    break;
                }}
            }}
        }}
    }}
    // Saved values the tick changed in the block entities beside it.
    for (side, key, value) in std::mem::take(&mut done.neighbour_writes) {{
        let [dx, dy, dz] = side.offset();
        let at = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
        let tag = {to_tag};
        if game
            .native_with_mod_block_entity(at, |e| {{
                e.nbt.insert(JavaString::from(key.as_str()), tag);
            }})
            .is_some()
        {{
            game.native_block_entity_sync(at);
        }}
    }}
    if done.sync {{
        game.native_block_entity_sync(pos);
    }}
    // Only a change touches the entity: each write marks it for saving and sending.
    if let Some(values) = changed {{
        for (key, value) in values {{
            let tag = {to_tag};
            entity.nbt.insert(key.into(), tag);
        }}
    }}
    if done.slots != slots {{
        let items = &mut entity.items;
        for (i, new) in done.slots.iter().enumerate() {{
            if slots.get(i) == Some(new) {{
                continue;
            }}
            let Some(cell) = items.get_mut(i) else {{
                continue;
            }};
            *cell = match (new, cell.take()) {{
                (None, _) => None,
                // The same item: keep its components, change its count.
                (Some(n), Some(mut old)) if *old.item == *n.item => {{
                    old.count = n.count;
                    Some(old)
                }}
                (Some(n), _) => Some(horizon_client::items::ItemStack::new(&n.item, n.count)),
            }};
        }}
    }}
    if done.data != data {{
        entity.data = done.data.clone();
    }}
    if storage.is_some() && done.tanks != tanks {{
        let list = done
            .tanks
            .iter()
            .map(|t| {{
                let mut map = horizon_world::nbt::Compound::new();
                map.insert(
                    JavaString::from("Fluid"),
                    Tag::String(JavaString::from(t.fluid.as_deref().unwrap_or("minecraft:empty"))),
                );
                map.insert(JavaString::from("Amount"), Tag::Int(t.amount));
                Tag::Compound(map)
            }})
            .collect();
        entity.nbt.insert(JavaString::from("fluids"), Tag::List(list));
    }}
    if storage.is_some() && done.energy != energy {{
        entity.nbt.insert(JavaString::from("energy"), Tag::Int(done.energy));
    }}
}}
"##,
                    from_tag = block_entity::FROM_TAG,
                    to_tag = block_entity::TO_TAG,
                ),
            ),
            "block_entity_container" => (
                "block_entity_container.rs",
                format!(
                    r#"// {name}'s machines have item slots, and open their menu when used.
fn {id}_block_entity_container(block: &str, container: &mut Option<horizon_client::containers::ModContainer>) {{
    use crate::game::{m} as m;
    if container.is_some() {{
        return;
    }}
    let Some(machine) = m::MACHINES.iter().find(|x| x.block == block) else {{
        return;
    }};
    let inputs: Vec<usize> = (0..machine.slots).filter(|s| !machine.results.contains(s)).collect();
    let outputs: Vec<usize> = if machine.results.is_empty() {{
        (0..machine.slots).collect()
    }} else {{
        machine.results.to_vec()
    }};
    // As a furnace: hoppers put in from above and the sides, and take from below.
    *container = Some(horizon_client::containers::ModContainer {{
        slots: machine.slots,
        faces: [
            outputs.clone(),
            inputs.clone(),
            inputs.clone(),
            inputs.clone(),
            inputs.clone(),
            inputs.clone(),
        ],
        inputs,
        outputs,
        menu: Some(machine.menu.into()),
    }});
}}
"#
                ),
            ),
            "screen" => (
                "screen.rs",
                format!(
                    r#"// The screens of {name}'s machines: where each slot sits, as their Java menus place them.
fn {id}_screen(
    game: &crate::game::Game<'_>,
    block: &str,
    screen: &mut Option<horizon_client::mod_api::NativeScreen>,
) {{
    use crate::game::{m} as m;
    if screen.is_some() {{
        return;
    }}
    let Some(machine) = m::MACHINES.iter().find(|x| x.block == block) else {{
        return;
    }};
    // The mod's own texture, when the game has it, instead of the plain panel.
    let own_background = machine
        .texture
        .is_some_and(|t| game.native_sprite_size(t).is_some());
    *screen = Some(horizon_client::mod_api::NativeScreen {{
        width: machine.size[0],
        height: machine.size[1],
        slots: machine.layout.to_vec(),
        inventory_y: machine.inventory[1],
        inventory_x: machine.inventory[0],
        own_labels: false,
        own_background,
    }});
}}
"#
                ),
            ),
            "screen_draw" => (
                "screen_draw.rs",
                format!(
                    r#"// Draws a screen of {name}'s machines, under its slots and items.
fn {id}_screen_draw(
    game: &crate::game::Game<'_>,
    block: &str,
    data: &[i32],
    batch: &mut horizon_client::hud::HudBatch,
    origin: [f32; 2],
) {{
    use crate::game::{m} as m;
    for sprite in m::screen_sprites(block, data) {{
        let [x, y, w, h] = sprite.dst;
        game.native_sprite(batch, sprite.name, sprite.src, [origin[0] + x, origin[1] + y, w, h]);
    }}
}}
"#
                ),
            ),
            "frame" => (
                "frame.rs",
                format!(
                    r#"// Once a frame: opens or closes {name}'s own screen when its code asked, and
// notices when Escape (or another screen) closed it.
fn {id}_frame(game: &mut crate::game::Game<'_>, elapsed: std::time::Duration) {{
    use crate::game::{m} as m;
    let _ = elapsed;
    if let Some(request) = m::take_screen_request() {{
        match request {{
            Some(screen) => {{
                if game.native_mod_screen_open(m::ID) {{
                    m::set_open_screen(Some(screen));
                }}
            }}
            None => {{
                game.native_mod_screen_close(m::ID);
                m::set_open_screen(None);
            }}
        }}
    }}
    if m::screen_is_open() && game.native_mod_screen() != Some(m::ID) {{
        m::set_open_screen(None);
    }}
}}
"#
                ),
            ),
            "screen_render_post" => (
                "screen_render_post.rs",
                screens::render_post_helper(
                    name,
                    id,
                    m,
                    !self.screens.is_empty(),
                    !self.overlays.is_empty(),
                ),
            ),
            "screen_mouse_button_pressed_pre" => (
                "screen_mouse_button_pressed_pre.rs",
                screens::click_helper(
                    name,
                    id,
                    m,
                    !self.screens.is_empty(),
                    !self.overlays.is_empty(),
                ),
            ),
            "screen_key_pressed_pre" => (
                "screen_key_pressed_pre.rs",
                format!(
                    r#"// A key on {name}'s own screen.
fn {id}_screen_key_pressed_pre(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    key: &str,
    repeat: bool,
    modifiers: u32,
    canceled: &mut bool,
) {{
    use crate::game::{m} as m;
    let _ = (screen, repeat, modifiers);
    if game.native_mod_screen() != Some(m::ID) || !m::screen_is_open() {{
        return;
    }}
    if m::key_open_screen(key) {{
        *canceled = true;
    }}
}}
"#
                ),
            ),
            "screen_mouse_scrolled_pre" => (
                "screen_mouse_scrolled_pre.rs",
                format!(
                    r#"// The wheel over {name}'s own screen.
fn {id}_screen_mouse_scrolled_pre(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    scroll: [f64; 2],
    modifiers: u32,
    canceled: &mut bool,
) {{
    use crate::game::{m} as m;
    let _ = (screen, modifiers);
    if game.native_mod_screen() != Some(m::ID) || !m::screen_is_open() {{
        return;
    }}
    if m::scroll_open_screen(scroll[1] as f32) {{
        *canceled = true;
    }}
}}
"#
                ),
            ),
            "block_entity_fluids" => (
                "block_entity_fluids.rs",
                format!(
                    r#"// The fluid tanks of {name}'s block entities: every side reaches every tank.
fn {id}_block_entity_fluids(
    block: &str,
    fluids: &mut Option<horizon_client::containers::ModFluidTanks>,
) {{
    use crate::game::{m} as m;
    let Some((_, entity, _)) = m::BLOCK_ENTITIES.iter().find(|(b, _, _)| *b == block) else {{
        return;
    }};
    let Some((_, tanks, _)) = m::STORAGE.iter().find(|(id, _, _)| id == entity) else {{
        return;
    }};
    if tanks.is_empty() {{
        return;
    }}
    let all: Vec<usize> = (0..tanks.len()).collect();
    *fluids = Some(horizon_client::containers::ModFluidTanks {{
        tanks: tanks.to_vec(),
        faces: std::array::from_fn(|_| all.clone()),
        inputs: all.clone(),
        outputs: all,
    }});
}}
"#
                ),
            ),
            "block_entity_energy" => (
                "block_entity_energy.rs",
                format!(
                    r#"// The energy buffers of {name}'s block entities: every side takes and gives.
fn {id}_block_entity_energy(
    block: &str,
    energy: &mut Option<horizon_client::containers::ModEnergyBuffer>,
) {{
    use crate::game::{m} as m;
    let Some((_, entity, _)) = m::BLOCK_ENTITIES.iter().find(|(b, _, _)| *b == block) else {{
        return;
    }};
    let Some((_, _, Some([capacity, max_insert, max_extract]))) =
        m::STORAGE.iter().find(|(id, _, _)| id == entity)
    else {{
        return;
    }};
    *energy = Some(horizon_client::containers::ModEnergyBuffer {{
        capacity: *capacity,
        max_insert: *max_insert,
        max_extract: *max_extract,
        insert_faces: [true; 6],
        extract_faces: [true; 6],
    }});
}}
"#
                ),
            ),
            "standalone_models" => (
                "standalone_models.rs",
                format!(
                    r#"// The models {name}'s renderers draw as parts: baked beside the block models.
fn {id}_standalone_models(models: &mut Vec<String>) {{
    use crate::game::{m} as m;
    models.extend(m::STANDALONE_MODELS.iter().map(|s| s.to_string()));
}}
"#
                ),
            ),
            "block_entity_update_tag" => (
                "block_entity_update_tag.rs",
                format!(
                    r#"// What clients get of one of {name}'s block entities (`getUpdateTag`): its
// saved state, which its renderer draws from.
fn {id}_block_entity_update_tag(
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
    tag: &mut Option<horizon_world::nbt::Compound>,
) {{
    use crate::game::{m} as m;
    let _ = pos;
    if m::BLOCK_ENTITIES.iter().any(|(_, id, _)| *id == entity.id) {{
        *tag = Some(entity.nbt.clone());
    }}
}}
"#
                ),
            ),
            "block_entity_submit" => (
                "block_entity_submit.rs",
                format!(
                    r##"// Draws one of {name}'s block entities on this client: its renderer, ported,
// records what to draw; this draws it with Horizon's pose stack.
fn {id}_block_entity_submit(
    game: &crate::game::Game<'_>,
    pos: [i32; 3],
    block: &str,
    state: horizon_world::registry::BlockStateId,
    entity: &horizon_client::containers::ModBlockEntity,
    draw: &mut crate::game::BlockEntityDraw,
) {{
    use crate::game::{m} as m;
    use horizon_world::nbt::{{JavaString, Tag}};
    if !m::BLOCK_ENTITIES.iter().any(|(_, id, _)| *id == entity.id) {{
        return;
    }}
    let frame = m::Frame {{
        pos,
        block: block.to_string(),
        facing: game.native_block_state_property(state, "facing").map(str::to_string),
        partial: draw.partial,
        game_time: draw.game_time,
    }};
    let mut ops = m::Draw::default();
    {{
        let nbt = &entity.nbt;
        m::render_block_entity(
            &entity.id,
            |key| {{
                let tag = nbt.get(&JavaString::from(key))?;
                {from_tag}
            }},
            &frame,
            &mut ops,
        );
    }}
    for op in ops.ops {{
        match op {{
            m::DrawOp::Push => draw.push(),
            m::DrawOp::Pop => draw.pop(),
            m::DrawOp::Translate(by) => draw.translate(by),
            m::DrawOp::Rotate {{ axis, degrees, pivot }} => draw.rotate_about(axis, degrees, pivot),
            m::DrawOp::Scale(by) => draw.scale(by),
            m::DrawOp::Block {{ block, properties }} => {{
                let properties: Vec<(&str, &str)> =
                    properties.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                game.native_draw_block(draw, &block, &properties);
            }}
            m::DrawOp::Cube {{ from, to, sprite, tint }} => {{
                // Horizon names vanilla's textures without `minecraft:` (`block/stone`).
                let sprite = sprite.strip_prefix("minecraft:").unwrap_or(&sprite);
                let face = Some(crate::game::native_block_entity_draw::CubeFace {{
                    sprite,
                    uv: None,
                }});
                game.native_draw_cube(draw, from, to, [face; 6], tint);
            }}
            m::DrawOp::Item {{ item, count }} => {{
                let stack = horizon_client::items::ItemStack::new(&item, count);
                game.native_draw_item(
                    draw,
                    &stack,
                    horizon_assets::item_model::DisplayContext::Fixed,
                    None,
                    0,
                );
            }}
            m::DrawOp::Keep => draw.keep(),
            {model_arm}
        }}
    }}
}}
"##,
                    from_tag = block_entity::FROM_TAG,
                    model_arm = if self.uses("standalone_models") {
                        "m::DrawOp::Model { model, tint } => {\n                game.native_draw_model(draw, &model, tint);\n            }"
                    } else {
                        "// No standalone models: Horizon bakes none for this mod.\n            m::DrawOp::Model { .. } => {}"
                    },
                ),
            ),
            "net_from_client" => (
                "net_from_client.rs",
                format!(
                    r#"// A message from a player's client to {name}: its handler runs, and what it
// sends goes out.
fn {id}_net_from_client(
    game: &mut crate::game::Game<'_>,
    player: u128,
    channel: &str,
    data: &[u8],
) {{
    use crate::game::{m} as m;
{config}    let mut out = m::Outbox::default();
    m::net_from_client(player, channel, data, &mut out);
    for (channel, data) in out.to_server {{
        let _ = game.native_net_send_to_server(channel, &data);
    }}
    for (to, channel, data) in out.to_players {{
        let _ = game.native_net_send_to_player(to, channel, &data);
    }}
}}
"#,
                    config = self.ensure_config("&*game"),
                ),
            ),
            "net_from_server" => (
                "net_from_server.rs",
                format!(
                    r#"// A message from the server to {name}: its handler runs, and what it sends
// goes out.
fn {id}_net_from_server(game: &mut crate::game::Game<'_>, channel: &str, data: &[u8]) {{
    use crate::game::{m} as m;
{config}    let mut out = m::Outbox::default();
    m::net_from_server(channel, data, &mut out);
    for (channel, data) in out.to_server {{
        let _ = game.native_net_send_to_server(channel, &data);
    }}
    for (to, channel, data) in out.to_players {{
        let _ = game.native_net_send_to_player(to, channel, &data);
    }}
}}
"#,
                    config = self.ensure_config("&*game"),
                ),
            ),
            "right_click_block"
            | "state_for_placement"
            | "update_shape"
            | "use_item_on"
            | "use_item"
                if !self.behaviours.is_empty() =>
            {
                blocks::helper(hook, id, m, name).expect("a block hook's bindings")
            }
            other => unreachable!("no helper for hook {other}"),
        }
    }

    /// PORTING.md's sections on the content mod and the block entities.
    fn content_md(&self, bundle: &Bundle) -> String {
        let mut p = String::new();
        let c = &bundle.content;
        if !c.is_empty() {
            let _ = write!(
                p,
                "\n## Content\n\n`content/` is a Horizon content mod: `horizon-mod.toml`, the Java mod's own \
                 assets and data ({} files), and a file for each block and item. Each copies a vanilla block or \
                 item (its template) and changes what the Java code sets: check the template fits, since it \
                 decides the block's shape, states and behaviour.\n",
                c.copies.len()
            );
            if !c.blocks.is_empty() {
                p.push_str(
                    "\n| Block | Template | Why | Properties |\n| --- | --- | --- | --- |\n",
                );
                for b in &c.blocks {
                    let props = if b.properties.is_empty() {
                        "-".to_string()
                    } else {
                        b.properties
                            .iter()
                            .map(|(k, v)| format!("`{k}` {v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let _ = writeln!(
                        p,
                        "| `{}` | `{}` | {} | {props} |",
                        b.name, b.template, b.template_from
                    );
                }
            }
            if !c.items.is_empty() {
                p.push_str("\n| Item | Template |\n| --- | --- |\n");
                for i in &c.items {
                    let _ = writeln!(
                        p,
                        "| `{}` | `{}` |",
                        i.name,
                        i.json["template"].as_str().unwrap_or("-")
                    );
                }
            }
            if !c.skipped.is_empty() {
                p.push_str("\nLeft out:\n\n");
                for s in &c.skipped {
                    let _ = writeln!(p, "- {s}");
                }
            }
        } else if !self.scan.registrations.is_empty() {
            let _ = write!(
                p,
                "\n## Content\n\nThe mod registers {} entries, but none of them is a block or item the porter could \
                 read an id for, and it has no resources to carry over.\n",
                self.scan.registrations.len()
            );
        }
        if !self.block_entities.is_empty() {
            let _ = write!(
                p,
                "\n## Block entities\n\nEach block entity type is a state struct in `mixins/{id}/runtime.rs`, saved \
                 with its block: the fields `saveAdditional` writes, under the same keys, and the other plain \
                 fields, so they last from tick to tick. `native_block_entity` gives the blocks their block entity \
                 (it needs hook points version {hooks}), and on the server `native_block_entity_tick` runs the \
                 `tick_` function for each one, 20 times a second, and saves what it changed. \
                 `/data get block <x y z>` shows the saved values.\n\n\
                 | Block entity | Blocks | Java class | State | Ticks |\n| --- | --- | --- | --- | --- |\n",
                id = self.id,
                hooks = self.hooks,
            );
            for be in self.block_entities {
                let _ = writeln!(
                    p,
                    "| `{}` | {} | {} | `{}` ({} fields) | {} |",
                    be.id,
                    be.blocks
                        .iter()
                        .map(|b| format!("`{b}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    be.class
                        .as_deref()
                        .map_or("not found".to_string(), |c| format!("`{c}`")),
                    be.rust,
                    be.fields.len(),
                    if be.ticks {
                        format!("`tick_{}`", be.snake)
                    } else {
                        "no".into()
                    }
                );
            }
            let left: Vec<_> = self
                .block_entities
                .iter()
                .flat_map(|b| b.left_out.iter().map(move |l| (b, l)))
                .collect();
            if !left.is_empty() {
                p.push_str("\nLeft out of the states, to port by hand:\n\n");
                for (b, l) in left {
                    let _ = writeln!(p, "- `{}`: {l}", b.rust);
                }
            }
            if self.block_entities.iter().any(|b| b.blocks.is_empty()) {
                p.push_str("\nA block entity with no blocks was registered for blocks the porter could not match: add them to `BLOCK_ENTITIES`.\n");
            }
        }
        if !self.machines.is_empty() {
            p.push_str(
                "\n## Machines\n\nEach block entity that opens a menu is a machine. Its menu is a menu type of the content \
                 mod (`content/data/<id>/horizon/menu/`), with the Java menu's slots, data slots and result slots. \
                 `native_block_entity_container` gives the block its slots (hoppers put in from above and the sides, \
                 and take results from below) and opens the menu; its block copies a furnace or a barrel, whose \
                 block opens a menu. `native_screen` places the slots where the Java menu does, and \
                 `native_screen_draw` draws what the `draw_` function returns under them. The tick gets a `Machine`: \
                 its slots, its menu's data slots (`ContainerData`: set them for the screen to show progress) and \
                 the mod's recipes, read from their JSON files.\n\n\
                 | Machine | Menu type | Slots | Results | Data slots | Screen | Texture |\n| --- | --- | --- | --- | --- | --- | --- |\n",
            );
            for m in self.machines {
                let _ = writeln!(
                    p,
                    "| `{}` | `{}` | {}{} | {} | {} | {} | {} |",
                    m.entity,
                    m.menu_type,
                    m.slots,
                    if m.approximate {
                        " (some placed in a loop: check `MACHINES`)"
                    } else {
                        ""
                    },
                    if m.results.is_empty() {
                        "-".to_string()
                    } else {
                        m.results
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    m.data_slots,
                    m.screen_class
                        .as_deref()
                        .map_or("not found".into(), |s| format!("`{s}`")),
                    m.texture
                        .as_deref()
                        .map_or("Horizon's plain panel".into(), |t| format!("`{t}`")),
                );
            }
            if !self.recipe_types.is_empty() {
                let _ = writeln!(
                    p,
                    "\nThe mod's recipe types, which `Machine::recipes` reads: {}. Each recipe's \
                     ingredients are resolved to items (tags included), its result read, and its other plain \
                     values kept (`recipe.number(\"time\")`).",
                    self.recipe_types
                        .iter()
                        .map(|t| format!("`{t}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        if !self.messages.is_empty() {
            p.push_str(
                "\n## Network messages\n\nEach `CustomPacketPayload` record is a struct in `runtime.rs` with its channel and \
                 an encoding of its own (both ends run this port, so it needn't match Java's). \
                 `native_net_from_client` and `native_net_from_server` decode what arrives and call its \
                 `on_<name>_from_client` or `on_<name>_from_server` handler: port the Java handler into it. A \
                 handler, or any code given an `Outbox`, sends with `out.send_to_server(&msg)` or \
                 `out.send_to_player(player, &msg)`. Horizon caps a message at 30 000 bytes and a player at 32 a tick.\n\n\
                 | Message | Channel | Fields | Sent |\n| --- | --- | --- | --- |\n",
            );
            for m in self.messages {
                let sent = match (m.to_server, m.to_client) {
                    (true, true) => "both ways",
                    (true, false) => "client to server",
                    _ => "server to client",
                };
                let fields = m
                    .fields
                    .iter()
                    .map(|(f, _, java)| format!("`{f}` ({java})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    p,
                    "| `{}` | `{}` | {} | {sent} |",
                    m.name,
                    m.channel,
                    if fields.is_empty() {
                        "-".into()
                    } else {
                        fields
                    }
                );
            }
        }
        let drawn: Vec<&BlockEntity> = self
            .block_entities
            .iter()
            .filter(|b| b.renderer.is_some())
            .collect();
        if !drawn.is_empty() {
            p.push_str(
                "\n## Drawing block entities\n\nEach block entity with a Java renderer has a `render_<name>` function in \
                 `runtime.rs`: port its `submit` (or `render`) into it. It draws under a pose stack (`push`, `pop`, \
                 `translate`, `rotate`, `rotate_about`, `scale`, as `PoseStack` does) with block models \
                 (`draw.block(\"minecraft:grindstone\", &[])`), cubes, and items, animated with `frame.time()`. \
                 `native_block_entity_submit` draws it every frame; `native_block_entity_update_tag` sends clients \
                 the state it reads. Parts with a model of their own (`PartialModel`, a standalone model) are \
                 drawn with `draw.model(...)`: Horizon bakes the ones in `STANDALONE_MODELS`. Call `draw.keep()` \
                 when nothing moves, and Horizon reuses the frame.\n\n\
                 | Block entity | Renderer |\n| --- | --- |\n",
            );
            for be in drawn {
                let _ = writeln!(
                    p,
                    "| `{}` | `{}` |",
                    be.id,
                    be.renderer.as_deref().unwrap_or_default()
                );
            }
        }
        if !self.screens.is_empty() {
            p.push_str(
                "\n## Screens\n\nEach screen of the mod's own (`extends Screen`) is a struct in `runtime.rs` with \
                 `draw_`, `click_`, `key_` and `scroll_` functions to port from its `render`, `mouseClicked`, \
                 `keyPressed` and `mouseScrolled`. Drawing records rectangles, text, sprites and items in GUI \
                 pixels from the screen's top left. Where the Java code calls `setScreen(new ...)`, call \
                 `open_screen(...)`; Horizon shows it over the world (`native_mod_screen_open`) and Escape closes it.\n\n\
                 | Screen | Opened by |\n| --- | --- |\n",
            );
            for s in self.screens {
                let _ = writeln!(
                    p,
                    "| `{}` | {} |",
                    s.class,
                    if s.opened_by.is_empty() {
                        "not found".to_string()
                    } else {
                        s.opened_by
                            .iter()
                            .map(|c| format!("`{c}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                );
            }
        }
        // Libraries large mods are built on, and what becomes of each.
        let mut libraries: BTreeMap<&str, (&str, BTreeSet<&str>)> = BTreeMap::new();
        for class in &self.scan.classes {
            for r in &class.references {
                if let Some((name, note)) = porter_map::library(r) {
                    libraries
                        .entry(name)
                        .or_insert((note, BTreeSet::new()))
                        .1
                        .insert(class.name.as_str());
                }
            }
        }
        if !libraries.is_empty() {
            p.push_str(
                "\n## Libraries\n\n| Library | Used by | In the port |\n| --- | --- | --- |\n",
            );
            for (name, (note, classes)) in &libraries {
                let _ = writeln!(
                    p,
                    "| {name} | {} classes | {} |",
                    classes.len(),
                    one_line(note)
                );
            }
        }
        if !self.settings.is_empty() {
            p.push_str(
                "\n## Config\n\nThe mod's config values are a `Config` struct in `runtime.rs`, kept in Horizon's mod \
                 settings under their Java keys. The mod's page in Options > Mods has a button for each: on and off \
                 toggle, a choice moves to the next one, a number steps through its range. Ported code reads them \
                 with `config()`; the hook helpers load them the first time they run. Text values are edited in \
                 their setting files.\n\n| Key | Type | Default | Range |\n| --- | --- | --- | --- |\n",
            );
            for s in self.settings {
                let e = &s.entry;
                let range = match (&e.min, &e.max) {
                    (Some(lo), Some(hi)) => format!("{lo} to {hi}"),
                    _ if !e.values.is_empty() => e.values.join(", "),
                    _ => "-".into(),
                };
                let _ = writeln!(
                    p,
                    "| `{}` | {:?} | `{}` | {range} |",
                    s.key, e.kind, e.default
                );
            }
        }
        p
    }

    /// For a Mixin no hook stands for: the Horizon functions to inject into, when a
    /// Horizon checkout was read.
    fn horizon_ports_cell(
        &self,
        mx: &porter_model::MixinClass,
        m: &porter_model::MixinMember,
    ) -> String {
        let Some((_, ports)) = &self.ledger else {
            return "none: inject into Horizon's code".into();
        };
        let mut found: Vec<String> = Vec::new();
        for key in member_keys(mx, m) {
            for port in ports.get(&key).into_iter().flatten().take(2) {
                found.push(format!(
                    "`{}:{}` `{}` ({})",
                    port.file,
                    port.line,
                    port.function,
                    port.rank.describe()
                ));
            }
        }
        if found.is_empty() {
            "none, and no Horizon function names the method yet".into()
        } else {
            format!("none; inject into {}", found.join(", "))
        }
    }

    fn porting_md(&self, bundle: &Bundle) -> String {
        let scan = self.scan;
        if !bundle.native {
            let mut p = format!(
                "# Porting {name} to Horizon\n\nGenerated by horizon-porter {tool} from `{input}`.\n\n\
                 Everything the porter found in this mod is content: blocks, items and resources, with no code \
                 that needs Horizon's hooks. `content/` is the whole port, a Horizon content mod, and it needs \
                 no build.\n",
                name = self.name,
                tool = env!("CARGO_PKG_VERSION"),
                input = scan.input,
            );
            p.push_str(&self.content_md(bundle));
            let _ = write!(
                p,
                "\n## Play and share\n\n\
                 1. `horizon-porter build . --play` (or **Play** in the Horizon Porter app) starts Horizon in its own \
                 instance, **{id} port**, with `content/` loaded as a development mod.\n\
                 2. `horizon-porter package .` writes `{id}-<version>.hmod`, the file players add on the launcher's \
                 Mods page.\n",
                id = self.id
            );
            return p;
        }
        let mut p = format!(
            "# Porting {name} to Horizon\n\nGenerated by horizon-porter {tool} from `{input}`.\n\n\
             This folder is a Horizon native mod: a [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) \
             bundle whose hooks are wired and whose logic is stubbed. The Java logic still has to be ported \
             into `mixins/{id}/runtime.rs`, where each function marked TODO names the Java code it replaces.\n\n",
            name = self.name,
            tool = env!("CARGO_PKG_VERSION"),
            input = scan.input,
            id = self.id,
        );

        p.push_str("## Files\n\n| File | What it is |\n| --- | --- |\n");
        p.push_str("| `Cargo.toml` | Runs the module's tests: `cargo test` |\n");
        p.push_str("| `compatibility.json` | The Minecraft version, hook points version and the Horizon commits the bundle targets |\n");
        let _ = writeln!(
            p,
            "| `horizon-native.json` | The mod's registry manifest: it needs hook points version {}, so it builds on the Horizon the player plays |",
            self.hooks
        );
        if !bundle.content.is_empty() {
            p.push_str("| `content/` | The content mod: blocks, items and the Java mod's assets and data |\n");
        }
        let _ = writeln!(
            p,
            "| `mixins/{id}/horizon-mixin.toml` | The manifest: the mod, its module and one injection per hook |",
            id = self.id
        );
        let _ = writeln!(
            p,
            "| `mixins/{id}/runtime.rs` | The mod's own code (`crate::game::{module}`): constants, and the stubs to fill in |",
            id = self.id,
            module = self.module
        );
        for hook in bundle.wired.keys() {
            let (file, _) = self.helper(hook);
            let _ = writeln!(
                p,
                "| `mixins/{id}/{file}` | Hooks `native_{hook}` |",
                id = self.id
            );
        }

        p.push_str("\n## Wired hooks\n\nThese hook points are wired completely; port the Java code into the runtime function each one calls.\n\n| Hook | Calls | Java code to port |\n| --- | --- | --- |\n");
        for (hook, classes) in &bundle.wired {
            let calls = match *hook {
                "mods" => "`TITLE`, `DESCRIPTION`, `KEYS`",
                "key" => "`on_key`",
                "command" => "`on_command`",
                "hud" => "`hud_lines`",
                "tick" => "`tick`",
                "wheel" => "`on_wheel`",
                "settings" => "`settings_buttons`",
                "settings_click" => "`settings_click`",
                "block_entity" => "`BLOCK_ENTITIES`, `block_entity_defaults`",
                "block_entity_ticks" => "`BLOCK_ENTITIES`",
                "block_entity_tick" => "`tick_block_entity`",
                _ => "",
            };
            let java = if classes.is_empty() {
                "(generated)".to_string()
            } else {
                classes
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let _ = writeln!(p, "| `native_{hook}` | {calls} | {java} |");
        }

        if !bundle.stubbed.is_empty() {
            let _ = write!(
                p,
                "\n## Hooks to port into\n\nThese hook points have a helper with Horizon's own bindings (read from Horizon {rev}'s \
                 source); the logic is yours to port into each. A helper runs inside the hooked function, so it \
                 reaches the game through its parameters. A Mixin's target is mapped to the hook that does its job \
                 where Horizon has one: a hook point holds across Horizon versions, a Mixin into Horizon's own \
                 functions would pin the bundle to one commit.\n\n\
                 | Hook | File | What it is | Port here |\n| --- | --- | --- | --- |\n",
                rev = &hooks::catalog().horizon[..7]
            );
            for (hook, (_, java)) in &bundle.stubbed {
                let doc = hooks::hook(hook)
                    .map(|h| one_line(&h.doc))
                    .unwrap_or_default();
                let _ = writeln!(
                    p,
                    "| `native_{hook}` | `mixins/{id}/{hook}.rs` | {doc} | {} |",
                    java.iter()
                        .map(|j| one_line(j))
                        .collect::<Vec<_>>()
                        .join("; "),
                    id = self.id,
                );
            }
        }
        if !bundle.unwired.is_empty() {
            p.push_str("\n## Hooks to wire by hand\n\nHorizon has hook points for these, but horizon-porter does not generate them yet. Look each one up in Horizon's `native-hooks.json` (its target and contract) and add an `[[injection]]` with a helper, like the ones generated here.\n\n| Horizon hook | Replaces | Used in |\n| --- | --- | --- |\n");
            for (hook, (classes, java)) in &bundle.unwired {
                let _ = writeln!(
                    p,
                    "| `native_{hook}` | {} | {} |",
                    java.iter()
                        .map(|j| if j.starts_with('`') {
                            j.clone()
                        } else {
                            format!("`{j}`")
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    classes
                        .iter()
                        .map(|c| format!("`{c}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }

        let mixin_members: Vec<_> = scan
            .mixins
            .iter()
            .flat_map(|mx| mx.members.iter().map(move |m| (mx, m)))
            .filter(|(_, m)| !matches!(m.verdict, Verdict::NotNeeded(_)))
            .collect();
        if !mixin_members.is_empty() {
            p.push_str(
                "\n## Mixins\n\nA Mixin whose target a Horizon hook stands for is ported into that hook's helper \
                 (the Horizon hook column): hook points hold across Horizon versions. The others become a \
                 horizon-mixins `[[injection]]` into the Horizon function that does the job of the Java method, \
                 written by hand; they target Horizon's own functions, so the bundle then builds only on the \
                 commit in `horizon_revision`.\n",
            );
            let mapped = mixin_hooks(scan);
            if let Some((dir, _)) = &self.ledger {
                let commit = dir
                    .file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                let _ = writeln!(
                    p,
                    "\nThe Horizon functions named are from the Horizon source at `{}` ({}): their comments name the vanilla method.",
                    dir.display(),
                    &commit[..commit.len().min(7)]
                );
            }
            p.push_str(
                "\n| Mixin | Target | Member | Horizon hook | horizon-mixins | Verdict |\n| --- | --- | --- | --- | --- | --- |\n",
            );
            for (mx, m) in mixin_members {
                let hooks_for: Vec<String> = mapped
                    .iter()
                    .filter(|(x, member, _, _)| x.name == mx.name && member.member == m.member)
                    .map(|(_, _, h, _)| format!("`native_{}`", h.name))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let hook_cell = if hooks_for.is_empty() {
                    self.horizon_ports_cell(mx, m)
                } else {
                    hooks_for.join(", ")
                };
                let _ = writeln!(
                    p,
                    "| `{}` | {} | `@{}` {} | {hook_cell} | {} | {} |",
                    short(&mx.name),
                    mx.targets
                        .iter()
                        .map(|t| format!("`{}`", short(t)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    m.annotation,
                    m.target_methods.join(", "),
                    m.horizon_kind.as_deref().unwrap_or("-"),
                    m.verdict
                );
            }
        }

        p.push_str(&self.content_md(bundle));

        p.push_str("\n## Every Java class\n\n");
        for class in &scan.classes {
            let mut roles: Vec<String> = Vec::new();
            for (hook, classes) in &bundle.wired {
                if classes.contains(&class.name) {
                    roles.push(format!("`native_{hook}`"));
                }
            }
            for (hook, (classes, _)) in &bundle.stubbed {
                if classes.contains(&class.name) {
                    roles.push(format!("`native_{hook}` (helper to port into)"));
                }
            }
            for (hook, (classes, _)) in &bundle.unwired {
                if classes.contains(&class.name) {
                    roles.push(format!("`native_{hook}` (by hand)"));
                }
            }
            if class.is_mixin {
                roles.push("Mixin".into());
            }
            let role = if roles.is_empty() {
                "logic for runtime.rs".to_string()
            } else {
                roles.join(", ")
            };
            let _ = writeln!(p, "- [ ] `{}` ({}): {role}", class.name, class.path);
        }

        let _ = write!(
            p,
            "\n## Build and test\n\n\
             1. `cargo test` in this folder runs the module's tests.\n\
             2. Build it into Horizon with your Horizon launcher: `horizon-porter build . --trust-native-code` \
             (or **Build and play** in the Horizon Porter app). It goes into its own instance, **{id} port**, so your \
             other worlds never run it, and `--play` starts it when the build is done. The first build downloads \
             Rust and takes several minutes; later ones are quicker. Native mods are not sandboxed: build only code \
             you have read.\n\
             3. Share it: `horizon-porter package .` writes `{id}-<version>.hnmod`, the file Horizon's mod registry takes{hmod}.\n\n\
             `horizon-native.json` says the mod needs hook points version {hooks} rather than one Horizon commit, so the \
             launcher builds it on the Horizon the instance plays. A rebuild stays on that Horizon; \
             `horizon-launcher native-build --update` moves an instance to a newer one.\n",
            id = self.id,
            hooks = self.hooks,
            hmod = if bundle.content.is_empty() {
                String::new()
            } else {
                format!(
                    ", and `{}-<version>.hmod` from `content/`, which players add on the launcher's Mods page. \
                     The game loads both; `build --play` loads `content/` as a development mod",
                    self.id
                )
            },
        );
        p
    }
}

/// The player type of the generated module: Horizon's `NativePlayer`, as plain data,
/// so runtime.rs compiles and tests on its own.
const PLAYER_TYPE: &str = r#"
/// The player, as Horizon's `native_player` gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Degrees; 0 faces south, 90 west.
    pub yaw: f32,
    pub pitch: f32,
    /// `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`.
    pub dimension: &'static str,
}

impl Player {
    /// The direction the player faces, as Java's `Direction.fromYRot(yaw)`:
    /// `south`, `west`, `north` or `east`.
    pub fn facing(&self) -> &'static str {
        let quarter = (f64::from(self.yaw) / 90.0).round() as i64;
        ["south", "west", "north", "east"][quarter.rem_euclid(4) as usize]
    }
}
"#;

/// How a helper reads the player and turns it into the module's `Player`.
const PLAYER: &str = "let player = game.native_player().map(|p| m::Player {
        x: p.x,
        y: p.y,
        z: p.z,
        yaw: p.yaw,
        pitch: p.pitch,
        dimension: p.dimension,
    });";

/// Each hook's helper parameters: (name, type, value in the hooked function). The
/// same as Horizon's own native mods use.
fn bindings(hook: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    const GAME: (&str, &str, &str) = ("game", "&mut crate::game::Game<'_>", "&mut *self");
    match hook {
        "mods" => &[(
            "mods",
            "&mut Vec<crate::game::native_hooks::NativeMod>",
            "&mut *mods",
        )],
        "key" => &[GAME, ("key", "&str", "key"), ("pressed", "bool", "pressed")],
        "command" => &[
            GAME,
            ("command", "&str", "command"),
            ("ran", "&mut bool", "&mut *ran"),
        ],
        "hud" => &[
            GAME,
            ("batch", "&mut horizon_client::hud::HudBatch", "&mut *batch"),
            (
                "corners",
                "&mut horizon_client::extras_hud::Corners",
                "&mut *corners",
            ),
        ],
        "tick" => &[GAME],
        "wheel" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            ("steps", "f32", "steps"),
            ("claimed", "&mut bool", "&mut *claimed"),
        ],
        "settings" => &[
            GAME,
            ("mod_id", "&str", "mod_id"),
            ("buttons", "&mut Vec<String>", "&mut *buttons"),
        ],
        "settings_click" => &[
            GAME,
            ("mod_id", "&str", "mod_id"),
            ("button", "usize", "button"),
        ],
        "block_entity" => &[
            ("block", "&str", "block"),
            (
                "entity",
                "&mut Option<(String, horizon_world::nbt::Compound)>",
                "&mut *entity",
            ),
        ],
        "block_entity_ticks" => &[
            ("block", "&str", "block"),
            ("ticks", "&mut bool", "&mut *ticks"),
        ],
        "block_entity_tick" => &[
            GAME,
            ("pos", "[i32; 3]", "pos"),
            (
                "entity",
                "&mut horizon_client::containers::ModBlockEntity",
                "&mut *entity",
            ),
        ],
        "block_entity_container" => &[
            ("block", "&str", "block"),
            (
                "container",
                "&mut Option<horizon_client::containers::ModContainer>",
                "&mut *container",
            ),
        ],
        "screen" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            ("block", "&str", "block"),
            (
                "screen",
                "&mut Option<horizon_client::mod_api::NativeScreen>",
                "&mut *screen",
            ),
        ],
        "frame" => &[GAME, ("elapsed", "std::time::Duration", "elapsed")],
        "screen_render_post" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            ("batch", "&mut horizon_client::hud::HudBatch", "&mut *batch"),
            (
                "screen",
                "&horizon_client::mod_api::NativeScreenView<'_>",
                "screen",
            ),
        ],
        "screen_mouse_button_pressed_pre" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            (
                "screen",
                "&horizon_client::mod_api::NativeScreenView<'_>",
                "screen",
            ),
            ("button", "i32", "button"),
            ("modifiers", "u32", "modifiers"),
            ("canceled", "&mut bool", "&mut *canceled"),
        ],
        "screen_key_pressed_pre" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            (
                "screen",
                "&horizon_client::mod_api::NativeScreenView<'_>",
                "screen",
            ),
            ("key", "&str", "key"),
            ("repeat", "bool", "repeat"),
            ("modifiers", "u32", "modifiers"),
            ("canceled", "&mut bool", "&mut *canceled"),
        ],
        "screen_mouse_scrolled_pre" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            (
                "screen",
                "&horizon_client::mod_api::NativeScreenView<'_>",
                "screen",
            ),
            ("scroll", "[f64; 2]", "scroll"),
            ("modifiers", "u32", "modifiers"),
            ("canceled", "&mut bool", "&mut *canceled"),
        ],
        "standalone_models" => &[("models", "&mut Vec<String>", "&mut *models")],
        "block_entity_fluids" => &[
            ("block", "&str", "block"),
            (
                "fluids",
                "&mut Option<horizon_client::containers::ModFluidTanks>",
                "&mut *fluids",
            ),
        ],
        "block_entity_energy" => &[
            ("block", "&str", "block"),
            (
                "energy",
                "&mut Option<horizon_client::containers::ModEnergyBuffer>",
                "&mut *energy",
            ),
        ],
        "block_entity_update_tag" => &[
            ("pos", "[i32; 3]", "pos"),
            (
                "entity",
                "&horizon_client::containers::ModBlockEntity",
                "entity",
            ),
            (
                "tag",
                "&mut Option<horizon_world::nbt::Compound>",
                "&mut *tag",
            ),
        ],
        "block_entity_submit" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            ("pos", "[i32; 3]", "pos"),
            ("block", "&str", "block"),
            ("state", "horizon_world::registry::BlockStateId", "state"),
            (
                "entity",
                "&horizon_client::containers::ModBlockEntity",
                "entity",
            ),
            ("draw", "&mut crate::game::BlockEntityDraw", "&mut *draw"),
        ],
        "net_from_client" => &[
            GAME,
            ("player", "u128", "player"),
            ("channel", "&str", "channel"),
            ("data", "&[u8]", "data"),
        ],
        "net_from_server" => &[
            GAME,
            ("channel", "&str", "channel"),
            ("data", "&[u8]", "data"),
        ],
        "screen_draw" => &[
            ("game", "&crate::game::Game<'_>", "&*self"),
            ("block", "&str", "block"),
            ("data", "&[i32]", "data"),
            ("batch", "&mut horizon_client::hud::HudBatch", "&mut *batch"),
            ("origin", "[f32; 2]", "origin"),
        ],
        _ => &[],
    }
}

/// The helper for a hook from Horizon's hook table, for mod `id` (whose runtime
/// module is `crate::game::<module>`): its bindings, and a body left to port, with
/// the Java code that needs it named. Its file name and source.
pub fn hook_helper(id: &str, module: &str, hook: &str, uses: &Uses) -> Option<(String, String)> {
    let h = hooks::hook(hook)?;
    let bindings = h.bindings()?;
    let params = bindings
        .iter()
        .map(|b| format!("{}: {}", b.name, b.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let names = bindings
        .iter()
        .map(|b| b.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let contract = match h.contract.as_deref() {
        Some("cancel") => "cancel: set its flag to stop what the game would do".to_string(),
        Some("replace") => format!(
            "replace: set `{}` to change the result",
            h.out.as_deref().unwrap_or("its output")
        ),
        Some("collect") => format!(
            "collect: add to `{}`",
            h.out.as_deref().unwrap_or("its list")
        ),
        _ => "listen: it hears the event, and changes what its `&mut` parameters hold".into(),
    };
    let mut src = format!(
        "// Horizon's `native_{hook}`: {doc}\n// Contract: {contract}.\n//\n// TODO: port into this function:\n",
        doc = one_line(&h.doc),
    );
    for java in &uses.1 {
        let _ = writeln!(src, "// - {}", one_line(java));
    }
    if !uses.0.is_empty() {
        let _ = writeln!(
            src,
            "// (in {})",
            uses.0
                .iter()
                .map(|c| short(c))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let _ = writeln!(src, "fn {id}_{hook}({params}) {{", id = id);
    if bindings
        .first()
        .is_some_and(|b| b.name == "game" && b.ty.ends_with("crate::game::Game<'_>"))
    {
        let _ = writeln!(
            src,
            "    // Turned off in Options > Mods: nothing until the next build leaves it out.\n    if !game.native_on(crate::game::{module}::ID) {{\n        return;\n    }}",
            module = module
        );
    }
    if bindings.len() == 1 {
        let _ = writeln!(src, "    let _ = {names};");
    } else if !names.is_empty() {
        let _ = writeln!(src, "    let _ = ({names});");
    }
    src.push_str("}\n");
    Some((format!("{hook}.rs"), src))
}

/// The Mixin members whose target method a Horizon hook stands for, with the hook
/// and how it matched. Accessors, shadows and members that need nothing are left out.
fn mixin_hooks(
    scan: &ModScan,
) -> Vec<(
    &porter_model::MixinClass,
    &porter_model::MixinMember,
    &'static hooks::HorizonHook,
    hooks::Match,
)> {
    let mut out = Vec::new();
    for mx in &scan.mixins {
        for member in &mx.members {
            if matches!(member.verdict, Verdict::NotNeeded(_))
                || ["Shadow", "Accessor", "Invoker", "Unique", "merged"]
                    .contains(&member.annotation.as_str())
            {
                continue;
            }
            for target in &mx.targets {
                for method in &member.target_methods {
                    for (hook, how) in hooks::method_hooks(target, method) {
                        out.push((mx, member, hook, how));
                    }
                }
            }
        }
    }
    out
}

/// `Class.method` for each target of a Mixin member, as Horizon's comments name them.
fn member_keys(mx: &porter_model::MixinClass, m: &porter_model::MixinMember) -> Vec<String> {
    let mut keys = Vec::new();
    for target in &mx.targets {
        for method in &m.target_methods {
            let method = method.split('(').next().unwrap_or(method).trim();
            if !method.is_empty() && !method.starts_with('<') {
                keys.push(format!("{}.{method}", hooks::doc_class(target)));
            }
        }
    }
    keys
}

/// The Horizon functions that port the targets of the Mixins no hook stands for.
fn unmapped_ports(scan: &ModScan, horizon: &Path) -> BTreeMap<String, Vec<ledger::Port>> {
    let mapped = mixin_hooks(scan);
    let mut keys = BTreeSet::new();
    for mx in &scan.mixins {
        for m in &mx.members {
            if matches!(m.verdict, Verdict::NotNeeded(_))
                || mapped
                    .iter()
                    .any(|(x, member, _, _)| x.name == mx.name && member.member == m.member)
            {
                continue;
            }
            keys.extend(member_keys(mx, m));
        }
    }
    ledger::ports(horizon, &keys.into_iter().collect::<Vec<_>>(), 3)
}

/// A mod's version as Cargo and Horizon take one, `major.minor.patch`: its first
/// three numbers, and the rest as build metadata (`31.17.0.109` → `31.17.0+109`,
/// `2.31` → `2.31.0`, `26.3-v2.21.0` → `26.3.0+v2.21.0`).
pub fn semver(version: &str) -> String {
    let v = version.trim().trim_start_matches(['v', 'V']);
    let mut numbers = Vec::new();
    let mut rest = v;
    while numbers.len() < 3 {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            break;
        }
        // Leading zeros aren't allowed: `08` is 8.
        numbers.push(digits.trim_start_matches('0').to_string());
        rest = &rest[digits.len()..];
        match rest.strip_prefix('.') {
            Some(r) if r.starts_with(|c: char| c.is_ascii_digit()) && numbers.len() < 3 => rest = r,
            _ => break,
        }
    }
    if numbers.is_empty() {
        return "0.1.0".into();
    }
    numbers.resize(3, "0".into());
    let numbers: Vec<String> = numbers
        .into_iter()
        .map(|n| if n.is_empty() { "0".into() } else { n })
        .collect();
    let build: String = rest
        .trim_start_matches(['.', '-', '+', '_'])
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '.'
            }
        })
        .collect::<String>()
        .split('.')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    let core = numbers.join(".");
    if build.is_empty() {
        core
    } else {
        format!("{core}+{build}")
    }
}

/// Text for a comment or a table cell: on one line.
fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

/// Text for a Rust string literal.
fn rust_str(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c => vec![c],
        })
        .collect()
}

/// Text for a TOML basic string.
fn toml_str(s: &str) -> String {
    rust_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_become_semver() {
        assert_eq!(semver("1.2.0"), "1.2.0");
        assert_eq!(semver("31.17.0.109"), "31.17.0+109");
        assert_eq!(semver("2.31"), "2.31.0");
        assert_eq!(semver("26.3-v2.21.0"), "26.3.0+v2.21.0");
        assert_eq!(
            semver("0.7.7+fix3+26.3+neoforge"),
            "0.7.7+fix3.26.3.neoforge"
        );
        assert_eq!(semver("v08.1"), "8.1.0");
        assert_eq!(semver("beta"), "0.1.0");
    }

    #[test]
    fn sanitizes_ids() {
        assert_eq!(sanitize_id("compass-hud"), "compass_hud");
        assert_eq!(sanitize_id("ModMenu"), "modmenu");
        assert_eq!(sanitize_id("2fast"), "mod_2fast");
    }

    #[test]
    fn player_facing_matches_java() {
        // The same arithmetic as the generated Player::facing.
        let facing = |yaw: f32| {
            let quarter = (f64::from(yaw) / 90.0).round() as i64;
            ["south", "west", "north", "east"][quarter.rem_euclid(4) as usize]
        };
        assert_eq!(facing(0.0), "south");
        assert_eq!(facing(90.0), "west");
        assert_eq!(facing(-90.0), "east");
        assert_eq!(facing(180.0), "north");
        assert_eq!(facing(-180.0), "north");
        assert_eq!(facing(44.0), "south");
        assert_eq!(facing(46.0), "west");
        assert_eq!(facing(359.0), "south");
        assert!(PLAYER_TYPE.contains("let quarter = (f64::from(self.yaw) / 90.0).round() as i64;"));
    }

    #[test]
    fn escapes_strings() {
        assert_eq!(rust_str(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }
}
