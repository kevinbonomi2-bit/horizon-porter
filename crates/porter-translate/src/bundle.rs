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
use crate::content;

/// The Horizon and horizon-mixins commits Horizon's own native mods were tested on
/// (horizon-mods native/zoom, 2026-11). Update them to a current Nightly.
pub const HORIZON_REVISION: &str = "6f8c7a0e5d531d3834c4d0be4659a1db8e7e5c2a";
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
    /// Hooks wired completely, with the Java classes that need each.
    pub wired: BTreeMap<&'static str, BTreeSet<String>>,
    /// Hooks the mod needs that must be wired by hand.
    pub unwired: BTreeMap<&'static str, (BTreeSet<String>, BTreeSet<&'static str>)>,
}

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
    let meta = scan.metadata.iter().find(|m| m.id.is_some());
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
        .unwrap_or_else(|| "0.1.0".into());
    let description = meta
        .and_then(|m| m.description.clone())
        .unwrap_or_else(|| format!("{name}, ported to Horizon."));

    // Which hooks the mod needs, and which of its classes need each.
    let mut wired: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    let mut unwired: BTreeMap<&'static str, (BTreeSet<String>, BTreeSet<&'static str>)> =
        BTreeMap::new();
    for class in &scan.classes {
        for r in &class.references {
            let Some(mapping) = porter_map::client_hooks(r) else {
                continue;
            };
            for hook in mapping.hooks {
                if porter_map::WIRED_HOOKS.contains(hook) {
                    wired.entry(hook).or_default().insert(class.name.clone());
                } else {
                    let entry = unwired.entry(hook).or_default();
                    entry.0.insert(class.name.clone());
                    entry.1.insert(mapping.java);
                }
            }
        }
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
        entry.1.insert("key input (no key bindings found to match)");
    }

    // Blocks, items and resources: a content mod. Block entities: native hooks.
    let content = content::generate(scan, &id, &java_id, &name, &version, &description);
    let ns = content::namespace(&java_id, &id).to_string();
    let block_entities = block_entity::find(scan, &ns);
    let mut hooks = 1;
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
    // Only an Options > Mods row: the content mod is the whole port.
    let native = content.is_empty() || wired.keys().any(|h| *h != "mods");

    let ctx = Ctx {
        id: &id,
        module: format!("ported_{id}"),
        name: &name,
        version: &version,
        description: &description,
        scan,
        lang,
        wired: &wired,
        hooks,
        block_entities: &block_entities,
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
        wired: if native {
            wired.clone()
        } else {
            BTreeMap::new()
        },
        unwired,
    };
    let porting = ctx.porting_md(&bundle);
    let mut bundle = bundle;
    bundle.files.insert("PORTING.md".into(), porting);
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
    }
    for (rel, content) in &bundle.files {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
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
    hooks: u32,
    block_entities: &'a [BlockEntity],
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
            let bindings = bindings(hook)
                .iter()
                .map(|(n, ty, v)| {
                    format!("  {{ name = \"{n}\", ty = \"{ty}\", value = \"{v}\" }},\n")
                })
                .collect::<String>();
            let _ = write!(
                m,
                "\n[[injection]]\nid = \"{id}\"\nkind = \"HEAD\"\ntarget = \"horizon_client::game::native_hooks::Game<'_>::native_{hook}\"\nhelper = \"{file}\"\nbindings = [\n{bindings}]\n",
                id = hook.replace('_', "-"),
            );
        }
        m
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
        if self.uses("settings") {
            let _ = write!(
                r,
                "\n/// The labels of the buttons on the mod's settings page (Options > Mods).\n/// `get` reads a saved setting.\n///\n/// TODO: port the config screen of {}.\npub fn settings_buttons(get: impl Fn(&str) -> Option<String>) -> Vec<String> {{\n    let _ = get;\n    Vec::new()\n}}\n\n/// A settings button was clicked: returns the setting to save, as (key, value).\npub fn settings_click(button: usize, get: impl Fn(&str) -> Option<String>) -> Option<(String, String)> {{\n    let _ = (button, get);\n    None\n}}\n",
                self.java_for("settings")
            );
        }

        if !self.block_entities.is_empty() {
            r.push_str(&block_entity::runtime(self.block_entities));
        }

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
                    r#"// A server tick of one of {name}'s block entities: runs its tick on the state
// saved with it, and saves what changed.
fn {id}_block_entity_tick(pos: [i32; 3], entity: &mut horizon_client::containers::ModBlockEntity) {{
    use crate::game::{m} as m;
    use horizon_world::nbt::{{JavaString, Tag}};
    if !m::BLOCK_ENTITIES.iter().any(|(_, id, _)| *id == entity.id) {{
        return;
    }}
    let changed = {{
        let nbt = &entity.nbt;
        m::tick_block_entity(&entity.id, pos, |key| {{
            let tag = nbt.get(&JavaString::from(key))?;
            {from_tag}
        }})
    }};
    // Only a change touches the entity: each write marks it for saving and sending.
    if let Some(values) = changed {{
        for (key, value) in values {{
            let tag = {to_tag};
            entity.nbt.insert(key.into(), tag);
        }}
    }}
}}
"#,
                    from_tag = block_entity::FROM_TAG,
                    to_tag = block_entity::TO_TAG,
                ),
            ),
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
        p
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

        if !bundle.unwired.is_empty() {
            p.push_str("\n## Hooks to wire by hand\n\nHorizon has hook points for these, but horizon-porter does not generate them yet. Look each one up in Horizon's `native-hooks.json` (its target and contract) and add an `[[injection]]` with a helper, like the ones generated here.\n\n| Horizon hook | Replaces | Used in |\n| --- | --- | --- |\n");
            for (hook, (classes, java)) in &bundle.unwired {
                let _ = writeln!(
                    p,
                    "| `native_{hook}` | {} | {} |",
                    java.iter()
                        .map(|j| format!("`{j}`"))
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
            p.push_str("\n## Mixins\n\nEach becomes a horizon-mixins `[[injection]]` into the Horizon function that does the job of the Java method. They target Horizon's own functions, so the bundle then builds only on the commit in `horizon_revision`.\n\n| Mixin | Target | Member | horizon-mixins | Verdict |\n| --- | --- | --- | --- | --- |\n");
            for (mx, m) in mixin_members {
                let _ = writeln!(
                    p,
                    "| `{}` | {} | `@{}` {} | {} | {} |",
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
            ("pos", "[i32; 3]", "pos"),
            (
                "entity",
                "&mut horizon_client::containers::ModBlockEntity",
                "&mut *entity",
            ),
        ],
        _ => &[],
    }
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
