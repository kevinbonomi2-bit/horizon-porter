//! The content half of a port: a Horizon content mod (`horizon-mod.toml`, `assets/`,
//! `data/`) with the Java mod's own resources and a block or item file for each
//! registration. Horizon blocks and items copy a vanilla one (their template) and
//! change what differs (playhorizon.dev docs: mods.md, modpacks.md "Registries 0.2").

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use porter_model::{ModScan, Registration};

/// The data folders a content mod may carry (playhorizon.dev/docs/content-mods).
const DATA_KINDS: &[&str] = &["recipe", "loot_table", "tags", "structure", "worldgen"];
/// Asset folders Horizon renders without them (it draws with wgpu).
const SKIPPED_ASSETS: &[&str] = &["shaders"];
/// Namespaces a content mod can't list as its own.
const SHARED_NAMESPACES: &[&str] = &["minecraft", "c", "horizon"];

/// A block the content mod adds.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The name under the mod's id: `crank` for `demo:crank`.
    pub name: String,
    pub template: String,
    /// Why that template: `Blocks.IRON_BLOCK in its properties`, `extends SlabBlock`...
    pub template_from: String,
    pub properties: serde_json::Map<String, serde_json::Value>,
    /// The Java block class, fully qualified, when found.
    pub class: Option<String>,
}

/// An item the content mod adds (block items come with their blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub json: serde_json::Value,
}

/// The content mod, as files under `content/`.
#[derive(Debug, Default)]
pub struct Content {
    /// Generated text files, by path under `content/`.
    pub files: BTreeMap<String, String>,
    /// Files copied from the Java mod, by path under `content/`.
    pub copies: BTreeMap<String, PathBuf>,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    /// Resource files left out, with why.
    pub skipped: Vec<String>,
}

impl Content {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.items.is_empty() && self.copies.is_empty()
    }
}

/// Builds the content mod for `scan`, named `id` (the port's id). `java_id` is the
/// Java mod's id: its blocks and items keep it as their namespace, so recipes, tags
/// and saves that name them still match.
pub fn generate(
    scan: &ModScan,
    id: &str,
    java_id: &str,
    name: &str,
    version: &str,
    description: &str,
) -> Content {
    generate_with(
        scan,
        id,
        java_id,
        name,
        version,
        description,
        &BTreeMap::new(),
    )
}

/// [`generate`], with the blocks that open a menu (`machines`: block name → its
/// menu class): Horizon opens a mod's menu only from a block whose template opens
/// one, so they copy a furnace (a horizontal `facing`) or a barrel.
pub fn generate_with(
    scan: &ModScan,
    id: &str,
    java_id: &str,
    name: &str,
    version: &str,
    description: &str,
    machines: &BTreeMap<String, String>,
) -> Content {
    let mut content = Content::default();
    let root = Path::new(&scan.input);
    let mut namespaces = BTreeSet::new();
    let ns = namespace(java_id, id);
    if ns != id {
        namespaces.insert(ns.to_string());
    }

    // The Java mod's own resources, from every resource root.
    for res in &scan.resources.roots {
        let base = root.join(res);
        for (rel, src) in walk(&base) {
            let parts: Vec<&str> = rel.split('/').collect();
            let [kind, ns, category, ..] = parts.as_slice() else {
                continue;
            };
            let keep = match *kind {
                // Vanilla's atlases take added sources; other vanilla assets stay vanilla's.
                "assets" if *ns == "minecraft" => *category == "atlases",
                "assets" => !SKIPPED_ASSETS.contains(category) && !SHARED_NAMESPACES.contains(ns),
                "data" => {
                    if *ns == "minecraft" || *ns == "c" {
                        *category == "tags"
                    } else {
                        DATA_KINDS.contains(category)
                            && *ns != "neoforge"
                            && *ns != "fabric"
                            && *ns != "forge"
                    }
                }
                _ => false,
            };
            if !keep {
                continue;
            }
            if !valid_path(&rel) {
                content
                    .skipped
                    .push(format!("{rel}: not a resource name Horizon accepts"));
                continue;
            }
            if !SHARED_NAMESPACES.contains(ns) && *ns != id {
                namespaces.insert(ns.to_string());
            }
            content.copies.entry(rel).or_insert(src);
        }
    }

    // A block file for each block registration, an item file for each item that is
    // not a block's.
    let class_of = |simple: &str| {
        let mut found = scan
            .classes
            .iter()
            .filter(|c| c.name.rsplit('.').next() == Some(simple));
        let first = found.next()?;
        found.next().is_none().then(|| first.name.clone())
    };
    let mut seen = BTreeSet::new();
    let mut pipes: Vec<(String, Option<String>)> = Vec::new();
    // Blocks and items the code registers in a way the scan can't read (built
    // from a list at run time, a library's registrar): their resources name them.
    // `assets/<ns>/blockstates/<name>.json` is a block, `items/<name>.json` an item.
    let registered = |registry: &str, name: &str| {
        scan.registrations
            .iter()
            .any(|r| r.registry == registry && r.id.as_deref() == Some(name))
    };
    let from_resources = |dir: &str, registry: &str| -> Vec<Registration> {
        let prefix = format!("assets/{ns}/{dir}/");
        content
            .copies
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix)?.strip_suffix(".json"))
            .filter(|n| !n.contains('/') && valid_name(n) && !registered(registry, n))
            .map(|n| Registration {
                registry: registry.into(),
                id: Some(n.to_string()),
                via: "resources".into(),
                class: String::new(),
                creates: None,
                args: String::new(),
            })
            .collect()
    };
    let resource_blocks = from_resources("blockstates", "block");
    let resource_items: Vec<Registration> = from_resources("items", "item")
        .into_iter()
        .filter(|r| {
            let n = r.id.as_deref().unwrap_or_default();
            !registered("block", n) && !resource_blocks.iter().any(|b| b.id.as_deref() == Some(n))
        })
        .collect();
    for reg in scan
        .registrations
        .iter()
        .filter(|r| r.registry == "block")
        .chain(resource_blocks.iter())
    {
        let Some(block_name) = reg.id.as_deref().filter(|n| valid_name(n)) else {
            continue;
        };
        if !seen.insert(block_name.to_string()) {
            continue;
        }
        let class = reg.creates.as_deref().and_then(class_of);
        let (mut template, mut template_from) = block_template(scan, reg, class.as_deref());
        let mut properties = block_properties(&reg.args);
        // The state properties its blockstate file draws it by.
        let states = content
            .copies
            .get(&format!("assets/{ns}/blockstates/{block_name}.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .map(|j| blockstate_properties(&j))
            .unwrap_or_default();
        if let Some(menu) = machines.get(block_name) {
            let horizontal = states.get("facing").is_some_and(|v| {
                v.iter()
                    .all(|x| ["north", "south", "east", "west"].contains(&x.as_str()))
            });
            let (t, like) = if horizontal {
                (
                    "minecraft:furnace",
                    "a furnace, which turns the same four ways",
                )
            } else {
                ("minecraft:barrel", "a barrel")
            };
            template = t.to_string();
            template_from =
                format!("it opens a menu (`{menu}`): its template must open one, as {like} does");
        } else if template == DEFAULT_TEMPLATE
            && let Some((t, why)) = template_for_states(&states, block_name)
        {
            template = t.to_string();
            template_from = why;
        }
        if reg.via == "resources" {
            template_from.push_str(
                "; found by its blockstate file (the code registers it in a way the scan can't read): check its template",
            );
        }
        // Not a full cube: it hides none of its neighbours' faces (Java culls by its
        // shape), and a pipe joins its neighbours as Horizon's connecting block does.
        // Its own shape, or one its mod superclasses define (a terminal's panel).
        let shape = class.as_deref().and_then(|c| {
            let mut name = c.to_string();
            for _ in 0..8 {
                if let Some(s) = scan.block_shapes.iter().find(|s| s.class == name) {
                    return Some(s);
                }
                name = scan
                    .classes
                    .iter()
                    .find(|k| k.name == name)?
                    .superclass
                    .clone()?;
            }
            None
        });
        let mut connecting = None;
        if let Some(s) = shape {
            if let Some(thickness) = s.pipe_thickness {
                template = "minecraft:chorus_plant".into();
                template_from = format!(
                    "a pipe (`PipeBlock`, {thickness} px): Horizon's connecting block joins it to the blocks of `#{ns}:{block_name}_connects`"
                );
                connecting = Some(thickness.clamp(2.0, 14.0));
            }
            properties.insert("occludes".into(), false.into());
        }
        let (declared, default_state) =
            declared_states(&template, &states, block_name, &mut content.skipped);
        let mut json = serde_json::json!({ "template": template });
        if let Some(thickness) = connecting {
            json["connects_to"] = format!("{ns}:{block_name}_connects").into();
            json["thickness"] = thickness.into();
            pipes.push((block_name.to_string(), class.clone()));
        }
        if !properties.is_empty() {
            json["properties"] = serde_json::Value::Object(properties.clone());
        }
        if !declared.is_empty() {
            template_from.push_str(&format!(
                "; declares {}",
                declared.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
            json["state_properties"] = serde_json::Value::Object(declared);
            json["default_state"] = serde_json::Value::Object(default_state);
        }
        content.files.insert(
            format!("data/{ns}/horizon/block/{block_name}.json"),
            pretty(&json),
        );
        content.blocks.push(Block {
            name: block_name.to_string(),
            template,
            template_from,
            properties,
            class,
        });
    }
    // What each pipe joins: the mod's blocks that share its most particular
    // interface of the mod's own, the one fewest of its blocks have (Tom's
    // Storage's `IInventoryCable`, not the `BlockWithTooltip` they all have);
    // else all of the mod's blocks.
    let implements = |class: &Option<String>, interface: &str| {
        let mut name = class.clone();
        for _ in 0..8 {
            let Some(k) = name
                .as_deref()
                .and_then(|n| scan.classes.iter().find(|k| k.name == n))
            else {
                return false;
            };
            if k.interfaces.iter().any(|i| i == interface) {
                return true;
            }
            name = k.superclass.clone();
        }
        false
    };
    for (pipe, class) in &pipes {
        let candidates: Vec<&str> = class
            .as_deref()
            .and_then(|c| scan.classes.iter().find(|k| k.name == c))
            .map(|c| {
                c.interfaces
                    .iter()
                    .map(String::as_str)
                    .filter(|i| scan.classes.iter().any(|k| k.name == *i))
                    .collect()
            })
            .unwrap_or_default();
        let count = |i: &str| {
            content
                .blocks
                .iter()
                .filter(|b| implements(&b.class, i))
                .count()
        };
        // Named like the pipe (`IInventoryCable` for `InventoryCableBlock`) first.
        let simple = |n: &str| n.rsplit(['.', '$']).next().unwrap_or(n).to_string();
        let pipe_class = class.as_deref().map(simple).unwrap_or_default();
        let named_like = |i: &str| {
            let s = simple(i);
            let core = s
                .strip_prefix('I')
                .filter(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))
                .unwrap_or(&s);
            !core.is_empty() && pipe_class.contains(core)
        };
        let own_interfaces: BTreeSet<&str> = candidates
            .iter()
            .copied()
            .filter(|i| count(i) > 1)
            .min_by_key(|i| (!named_like(i), count(i)))
            .into_iter()
            .collect();
        let joins: Vec<String> = content
            .blocks
            .iter()
            .filter(|b| {
                own_interfaces.is_empty()
                    || b.name == *pipe
                    || own_interfaces.iter().any(|i| implements(&b.class, i))
            })
            .map(|b| format!("{ns}:{}", b.name))
            .collect();
        content.files.insert(
            format!("data/{ns}/tags/block/{pipe}_connects.json"),
            pretty(&serde_json::json!({ "values": joins })),
        );
    }
    let blocks: BTreeSet<String> = content.blocks.iter().map(|b| b.name.clone()).collect();
    let mut seen = BTreeSet::new();
    for reg in scan
        .registrations
        .iter()
        .filter(|r| r.registry == "item")
        .chain(resource_items.iter())
    {
        let Some(item_name) = reg.id.as_deref().filter(|n| valid_name(n)) else {
            continue;
        };
        if blocks.contains(item_name) || !seen.insert(item_name.to_string()) {
            continue;
        }
        let json = item_json(&reg.args);
        content.files.insert(
            format!("data/{ns}/horizon/item/{item_name}.json"),
            pretty(&json),
        );
        content.items.push(Item {
            name: item_name.to_string(),
            json,
        });
    }

    // Assets alone (a client mod's lang file, its HUD textures) make no content mod:
    // there must be blocks, items or data for the game to load.
    if content.blocks.is_empty()
        && content.items.is_empty()
        && !content.copies.keys().any(|k| k.starts_with("data/"))
    {
        return Content::default();
    }
    let mut manifest = format!(
        "# {name}'s blocks, items and resources, ported with horizon-porter.\n\
         id = \"{id}\"\nname = \"{}\"\nversion = \"{version}\"\ndescription = \"{}\"\n\
         authors = [\"{} authors\"]\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
        toml_text(name),
        toml_text(description),
        toml_text(name),
    );
    if !namespaces.is_empty() {
        manifest.push_str(&format!(
            "# The Java mod's files use these namespaces too.\nnamespaces = [{}]\n",
            namespaces
                .iter()
                .map(|n| format!("\"{n}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    content.files.insert("horizon-mod.toml".into(), manifest);
    content
}

/// The namespace of the port's blocks and items: the Java mod's id when Horizon
/// accepts it as one, so the recipes and tags that name them still match.
pub fn namespace<'a>(java_id: &'a str, id: &'a str) -> &'a str {
    if !java_id.is_empty() && !java_id.contains('/') && valid_path(java_id) {
        java_id
    } else {
        id
    }
}

/// The vanilla block a Java block copies, and why.
fn block_template(scan: &ModScan, reg: &Registration, class: Option<&str>) -> (String, String) {
    // `Properties.ofFullCopy(Blocks.IRON_BLOCK)`, `.copy(Blocks.STONE)`...
    if let Some(block) = vanilla_constant(&reg.args, "Blocks.") {
        return (
            format!("minecraft:{block}"),
            format!("Blocks.{} in its properties", block.to_ascii_uppercase()),
        );
    }
    // The first Minecraft class the block class extends.
    let mut next = class.map(str::to_string);
    let mut guard = 0;
    while let Some(name) = next.take() {
        guard += 1;
        if guard > 16 {
            break;
        }
        let simple = name.rsplit('.').next().unwrap_or(&name);
        if name.starts_with("net.minecraft.") {
            if let Some(template) = superclass_template(simple) {
                return (template.to_string(), format!("extends {simple}"));
            }
            break;
        }
        // A mod class named for what it is (`FirLeavesBlock`) counts too.
        if let Some(template) = superclass_template(simple).filter(|_| guard > 1 || class.is_some())
        {
            return (template.to_string(), format!("{simple} is one"));
        }
        next = scan
            .classes
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.superclass.clone());
    }
    // A vanilla class made directly: `p -> new StandingSignBlock(WoodType.OAK, p)`.
    if let Some(simple) = reg.creates.as_deref()
        && let Some(template) = superclass_template(simple)
    {
        return (template.to_string(), format!("makes a {simple}"));
    }
    (DEFAULT_TEMPLATE.into(), "no template found: stone".into())
}

const DEFAULT_TEMPLATE: &str = "minecraft:stone";

/// The state properties of each template the porter picks itself, so it knows
/// which ones a block has to declare. Templates named by the Java code
/// (`Blocks.GOLD_ORE`) are not here: their properties are not known.
fn template_properties(template: &str) -> Option<&'static [&'static str]> {
    Some(match template.strip_prefix("minecraft:")? {
        "stone" | "glass" | "poppy" | "torch" | "white_carpet" | "diamond_ore" | "iron_block" => {
            &[]
        }
        "stone_slab" => &["type", "waterlogged"],
        "stone_stairs" => &["facing", "half", "shape", "waterlogged"],
        "cobblestone_wall" => &["east", "north", "south", "up", "waterlogged", "west"],
        "oak_fence" | "iron_bars" => &["east", "north", "south", "waterlogged", "west"],
        "oak_fence_gate" => &["facing", "in_wall", "open", "powered"],
        "oak_door" => &["facing", "half", "hinge", "open", "powered"],
        "oak_trapdoor" => &["facing", "half", "open", "powered", "waterlogged"],
        "stone_pressure_plate" => &["powered"],
        "stone_button" => &["face", "facing", "powered"],
        "oak_log" => &["axis"],
        "oak_leaves" => &["distance", "persistent", "waterlogged"],
        "oak_sapling" => &["stage"],
        "wheat" => &["age"],
        "lantern" => &["hanging", "waterlogged"],
        "ladder" => &["facing", "waterlogged"],
        "carved_pumpkin" => &["facing"],
        "chest" => &["facing", "type", "waterlogged"],
        "barrel" => &["facing", "open"],
        "furnace" => &["facing", "lit"],
        _ => return None,
    })
}

/// A template that has a block's main state property, when it has no template.
fn template_for_states(
    states: &BTreeMap<String, BTreeSet<String>>,
    name: &str,
) -> Option<(&'static str, String)> {
    let only = |prop: &str, allowed: &[&str]| {
        states
            .get(prop)
            .is_some_and(|v| v.iter().all(|x| allowed.contains(&x.as_str())))
    };
    let has = |prop: &str| states.contains_key(prop);
    // Stone or wood, by its name: the sounds, tools and burning follow.
    let stony = [
        "stone",
        "brick",
        "cobble",
        "tile",
        "deepslate",
        "basalt",
        "blackstone",
        "sandstone",
        "marble",
        "granite",
        "slate",
        "concrete",
        "mud",
        "rock",
    ]
    .iter()
    .any(|w| name.contains(w));
    let kind = |stone: &'static str, wood: &'static str, what: &str| {
        Some((
            if stony { stone } else { wood },
            format!("its blockstates are a {what}'s"),
        ))
    };
    if has("type") && only("type", &["top", "bottom", "double"]) {
        return kind("minecraft:stone_slab", "minecraft:oak_slab", "slab");
    }
    if has("half") && has("shape") && has("facing") {
        return kind("minecraft:stone_stairs", "minecraft:oak_stairs", "stairs");
    }
    if has("hinge") && has("open") {
        return kind("minecraft:iron_door", "minecraft:oak_door", "door");
    }
    if has("in_wall") {
        return Some((
            "minecraft:oak_fence_gate",
            "its blockstates are a fence gate's".into(),
        ));
    }
    if has("open") && has("half") && has("facing") {
        return kind(
            "minecraft:iron_trapdoor",
            "minecraft:oak_trapdoor",
            "trapdoor",
        );
    }
    if has("up") && only("north", &["none", "low", "tall"]) {
        return Some((
            "minecraft:cobblestone_wall",
            "its blockstates are a wall's".into(),
        ));
    }
    if only("north", &["true", "false"]) && has("east") && !has("up") {
        return if name.contains("pane") || name.contains("bars") {
            Some((
                "minecraft:glass_pane",
                "its blockstates are a pane's".into(),
            ))
        } else {
            kind(
                "minecraft:nether_brick_fence",
                "minecraft:oak_fence",
                "fence",
            )
        };
    }
    if has("face") && has("powered") {
        return kind("minecraft:stone_button", "minecraft:oak_button", "button");
    }
    if has("powered") && states.len() == 1 {
        return kind(
            "minecraft:stone_pressure_plate",
            "minecraft:oak_pressure_plate",
            "pressure plate",
        );
    }
    if has("distance") && has("persistent") {
        return Some(("minecraft:oak_leaves", "its blockstates are leaves'".into()));
    }
    if has("stage") && states.len() == 1 {
        return Some((
            "minecraft:oak_sapling",
            "its blockstates are a sapling's".into(),
        ));
    }
    if has("age") && states.len() == 1 {
        return Some(("minecraft:wheat", "its blockstates are a crop's".into()));
    }
    if states.is_empty()
        && let Some(t) = name_template(name)
    {
        return Some((t, format!("its name (`{name}`) says what it is")));
    }
    if only("axis", &["x", "y", "z"]) {
        return Some(("minecraft:oak_log", "its blockstates turn on `axis`".into()));
    }
    if only("facing", &["north", "south", "east", "west"]) {
        return Some((
            "minecraft:carved_pumpkin",
            "its blockstates turn on a horizontal `facing`".into(),
        ));
    }
    None
}

/// The state properties to declare for a block: those its blockstate file uses
/// that its template lacks, with a default state. Only for templates whose own
/// properties are known; for others, a note in `skipped` to check by hand.
fn declared_states(
    template: &str,
    states: &BTreeMap<String, BTreeSet<String>>,
    block: &str,
    skipped: &mut Vec<String>,
) -> (
    serde_json::Map<String, serde_json::Value>,
    serde_json::Map<String, serde_json::Value>,
) {
    let mut declared = serde_json::Map::new();
    let mut default_state = serde_json::Map::new();
    let Some(own) = template_properties(template) else {
        if !states.is_empty() {
            skipped.push(format!(
                "`{block}`: check that `{template}` has the state properties its blockstate file uses ({})",
                states.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        return (declared, default_state);
    };
    for (prop, values) in states {
        if own.contains(&prop.as_str()) {
            continue;
        }
        let (decl, default) = declare(values);
        declared.insert(prop.clone(), decl);
        default_state.insert(prop.clone(), default.into());
    }
    let combinations: usize = declared
        .values()
        .map(|d| d["values"].as_array().map_or(2, Vec::len))
        .product();
    if combinations > 1 << 16 {
        skipped.push(format!(
            "`{block}`: its blockstate file's properties make {combinations} states, too many to declare"
        ));
        declared.clear();
        default_state.clear();
    }
    (declared, default_state)
}

/// The state properties a blockstate file uses, with the values it names:
/// `"axis=x,powered=true"` variants and multipart `when`s.
fn blockstate_properties(json: &serde_json::Value) -> BTreeMap<String, BTreeSet<String>> {
    fn add(out: &mut BTreeMap<String, BTreeSet<String>>, prop: &str, values: &str) {
        let prop = prop.trim();
        if !state_name(prop) {
            return;
        }
        out.entry(prop.to_string()).or_default().extend(
            values
                .split('|')
                .map(|v| v.trim().to_string())
                .filter(|v| state_name(v)),
        );
    }
    fn when(out: &mut BTreeMap<String, BTreeSet<String>>, w: &serde_json::Value) {
        let Some(map) = w.as_object() else {
            return;
        };
        for (k, v) in map {
            match v {
                serde_json::Value::Array(list) if k == "OR" || k == "AND" => {
                    for item in list {
                        when(out, item);
                    }
                }
                serde_json::Value::String(values) => add(out, k, values),
                serde_json::Value::Bool(b) => add(out, k, &b.to_string()),
                serde_json::Value::Number(n) => add(out, k, &n.to_string()),
                _ => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    if let Some(variants) = json.get("variants").and_then(|v| v.as_object()) {
        for key in variants.keys() {
            for pair in key.split(',') {
                if let Some((prop, value)) = pair.split_once('=') {
                    add(&mut out, prop, value);
                }
            }
        }
    }
    if let Some(parts) = json.get("multipart").and_then(|v| v.as_array()) {
        for w in parts.iter().filter_map(|p| p.get("when")) {
            when(&mut out, w);
        }
    }
    out.retain(|_, v| !v.is_empty());
    out
}

/// A declared state property for the values a blockstate file names, and its
/// default: `boolean` (false) for true and false, else an `enum` of the values,
/// its first the default.
fn declare(values: &BTreeSet<String>) -> (serde_json::Value, String) {
    if values.iter().all(|v| v == "true" || v == "false") {
        return (serde_json::json!({ "type": "boolean" }), "false".into());
    }
    let mut list: Vec<&String> = values.iter().collect();
    // Numbers in order (1, 2, 10), and north first, as vanilla has them.
    list.sort_by_key(|v| {
        (
            v.parse::<u32>().unwrap_or(u32::MAX),
            v.as_str() != "north",
            v.to_string(),
        )
    });
    let default = list.first().map(|v| v.to_string()).unwrap_or_default();
    (
        serde_json::json!({ "type": "enum", "values": list }),
        default,
    )
}

/// A state property name or value: lower case letters, digits and underscores.
fn state_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A vanilla template for each kind of Minecraft block class.
fn superclass_template(simple: &str) -> Option<&'static str> {
    Some(match simple {
        "SlabBlock" => "minecraft:stone_slab",
        "StairBlock" => "minecraft:stone_stairs",
        "WallBlock" => "minecraft:cobblestone_wall",
        "FenceBlock" => "minecraft:oak_fence",
        "FenceGateBlock" => "minecraft:oak_fence_gate",
        "DoorBlock" => "minecraft:oak_door",
        "TrapDoorBlock" => "minecraft:oak_trapdoor",
        "PressurePlateBlock" => "minecraft:stone_pressure_plate",
        "ButtonBlock" => "minecraft:stone_button",
        "RotatedPillarBlock" => "minecraft:oak_log",
        "LeavesBlock" => "minecraft:oak_leaves",
        "SaplingBlock" => "minecraft:oak_sapling",
        "FlowerBlock" => "minecraft:poppy",
        "CropBlock" => "minecraft:wheat",
        "TransparentBlock" | "HalfTransparentBlock" => "minecraft:glass",
        "IronBarsBlock" => "minecraft:iron_bars",
        "LanternBlock" => "minecraft:lantern",
        "TorchBlock" => "minecraft:torch",
        "CarpetBlock" => "minecraft:white_carpet",
        "LadderBlock" => "minecraft:ladder",
        "HorizontalDirectionalBlock" => "minecraft:carved_pumpkin",
        "DropExperienceBlock" => "minecraft:diamond_ore",
        "ChestBlock" => "minecraft:chest",
        "BarrelBlock" => "minecraft:barrel",
        "AbstractFurnaceBlock" | "FurnaceBlock" => "minecraft:furnace",
        // A mod's own subclass, by its name's ending (`TintedParticleLeavesBlock`),
        // the most particular first.
        _ => {
            const ENDINGS: &[(&str, &str)] = &[
                ("WallHangingSignBlock", "minecraft:oak_wall_hanging_sign"),
                ("CeilingHangingSignBlock", "minecraft:oak_hanging_sign"),
                ("HangingSignBlock", "minecraft:oak_hanging_sign"),
                ("WallSignBlock", "minecraft:oak_wall_sign"),
                ("StandingSignBlock", "minecraft:oak_sign"),
                ("SignBlock", "minecraft:oak_sign"),
                ("ShelfBlock", "minecraft:oak_shelf"),
                ("DoublePlantBlock", "minecraft:tall_grass"),
                ("TallFlowerBlock", "minecraft:sunflower"),
                ("TallGrassBlock", "minecraft:short_grass"),
                ("BushBlock", "minecraft:short_grass"),
                ("VegetationBlock", "minecraft:short_grass"),
                ("FlowerBedBlock", "minecraft:pink_petals"),
                ("PinkPetalsBlock", "minecraft:pink_petals"),
                ("LeafLitterBlock", "minecraft:leaf_litter"),
                ("WebBlock", "minecraft:cobweb"),
                ("WaterlilyBlock", "minecraft:lily_pad"),
                ("HugeMushroomBlock", "minecraft:brown_mushroom_block"),
                ("AmethystClusterBlock", "minecraft:amethyst_cluster"),
                ("GrowingPlantHeadBlock", "minecraft:weeping_vines"),
                ("GrowingPlantBodyBlock", "minecraft:weeping_vines_plant"),
                ("FenceGateBlock", "minecraft:oak_fence_gate"),
                ("TrapDoorBlock", "minecraft:oak_trapdoor"),
                ("TrapdoorBlock", "minecraft:oak_trapdoor"),
                ("PressurePlateBlock", "minecraft:stone_pressure_plate"),
                ("LeavesBlock", "minecraft:oak_leaves"),
                ("SaplingBlock", "minecraft:oak_sapling"),
                ("FlowerBlock", "minecraft:poppy"),
                ("SlabBlock", "minecraft:stone_slab"),
                ("StairBlock", "minecraft:stone_stairs"),
                ("StairsBlock", "minecraft:stone_stairs"),
                ("WallBlock", "minecraft:cobblestone_wall"),
                ("FenceBlock", "minecraft:oak_fence"),
                ("DoorBlock", "minecraft:oak_door"),
                ("ButtonBlock", "minecraft:stone_button"),
                ("PillarBlock", "minecraft:oak_log"),
                ("LogBlock", "minecraft:oak_log"),
                ("CropBlock", "minecraft:wheat"),
                ("CropsBlock", "minecraft:wheat"),
                ("VineBlock", "minecraft:vine"),
                ("MushroomBlock", "minecraft:brown_mushroom"),
                ("TorchBlock", "minecraft:torch"),
                ("LanternBlock", "minecraft:lantern"),
                ("CarpetBlock", "minecraft:white_carpet"),
                ("LadderBlock", "minecraft:ladder"),
                ("GlassBlock", "minecraft:glass"),
                ("PaneBlock", "minecraft:glass_pane"),
            ];
            return ENDINGS
                .iter()
                .find(|(end, _)| simple.ends_with(end))
                .map(|(_, t)| *t);
        }
    })
}

/// A block known only by its id: a guess from its name's ending.
fn name_template(name: &str) -> Option<&'static str> {
    const ENDINGS: &[(&str, &str)] = &[
        ("_leaves", "minecraft:oak_leaves"),
        ("_sapling", "minecraft:oak_sapling"),
        ("_planks", "minecraft:oak_planks"),
        ("_log", "minecraft:oak_log"),
        ("_wood", "minecraft:oak_wood"),
        ("_stem", "minecraft:crimson_stem"),
        ("_hyphae", "minecraft:crimson_hyphae"),
        ("_glass", "minecraft:glass"),
        ("_glass_pane", "minecraft:glass_pane"),
        ("_wool", "minecraft:white_wool"),
        ("_carpet", "minecraft:white_carpet"),
        ("_ore", "minecraft:iron_ore"),
        ("_sand", "minecraft:sand"),
        ("_dirt", "minecraft:dirt"),
        ("_flower", "minecraft:poppy"),
        ("_mushroom", "minecraft:brown_mushroom"),
        ("_vine", "minecraft:vine"),
        ("_vines", "minecraft:vine"),
        ("_torch", "minecraft:torch"),
        ("_lantern", "minecraft:lantern"),
        ("_ladder", "minecraft:ladder"),
    ];
    ENDINGS
        .iter()
        .find(|(end, _)| name.ends_with(end))
        .map(|(_, t)| *t)
}

/// `Blocks.IRON_BLOCK` in `text` → `iron_block` (the first one).
fn vanilla_constant(text: &str, prefix: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = text[from..].find(prefix).map(|i| from + i) {
        let before_ok = i == 0
            || !text[..i]
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let name: String = text[i + prefix.len()..]
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        if before_ok && !name.is_empty() {
            let name = name.to_ascii_lowercase();
            // A colored set (26.x): `Blocks.CARPET.white()` is `white_carpet`.
            let after = &text[i + prefix.len() + name.len()..];
            let color: String = after
                .strip_prefix('.')
                .map(|r| r.chars().take_while(|c| c.is_ascii_alphabetic()).collect())
                .unwrap_or_default();
            const COLORS: &[&str] = &[
                "white",
                "orange",
                "magenta",
                "lightBlue",
                "yellow",
                "lime",
                "pink",
                "gray",
                "lightGray",
                "cyan",
                "purple",
                "blue",
                "brown",
                "green",
                "red",
                "black",
            ];
            if COLORS.contains(&color.as_str()) && after[1 + color.len()..].starts_with("()") {
                let snake = color
                    .replace("lightBlue", "light_blue")
                    .replace("lightGray", "light_gray");
                return Some(format!("{snake}_{name}"));
            }
            return Some(name);
        }
        from = i + prefix.len();
    }
    None
}

/// Block properties from a `BlockBehaviour.Properties` chain in the registration.
fn block_properties(args: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut p = serde_json::Map::new();
    if let Some(values) = call_numbers(args, ".strength(")
        && let Some(h) = values.first()
    {
        p.insert("hardness".into(), (*h).into());
        p.insert(
            "explosion_resistance".into(),
            (*values.get(1).unwrap_or(h)).into(),
        );
    }
    if let Some(h) = call_numbers(args, ".destroyTime(").and_then(|v| v.first().copied()) {
        p.insert("hardness".into(), h.into());
    }
    if let Some(r) = call_numbers(args, ".explosionResistance(").and_then(|v| v.first().copied()) {
        p.insert("explosion_resistance".into(), r.into());
    }
    // `.lightLevel(state -> 7)`: a constant level only.
    if let Some(i) = args.find(".lightLevel(") {
        let rest = &args[i + ".lightLevel(".len()..];
        if let Some(arrow) = rest.find("->") {
            let level: String = rest[arrow + 2..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(level) = level.parse::<u8>() {
                p.insert("light".into(), level.min(15).into());
            }
        }
    }
    if args.contains(".requiresCorrectToolForDrops()") {
        p.insert("requires_tool".into(), true.into());
    }
    // Its neighbours' faces are drawn and light passes it (a shaft, glass, a pipe).
    if args.contains(".noOcclusion()") {
        p.insert("occludes".into(), false.into());
    }
    if let Some(f) = call_numbers(args, ".friction(").and_then(|v| v.first().copied()) {
        p.insert("friction".into(), f.into());
    }
    if let Some(sound) = vanilla_constant(args, "SoundType.").and_then(|s| sound_block(&s)) {
        p.insert("sounds".into(), sound.into());
    }
    p
}

/// A vanilla block that makes each `SoundType`'s sounds.
fn sound_block(sound_type: &str) -> Option<&'static str> {
    Some(match sound_type {
        "stone" => "minecraft:stone",
        "wood" => "minecraft:oak_planks",
        "gravel" => "minecraft:gravel",
        "grass" => "minecraft:grass_block",
        "metal" => "minecraft:iron_block",
        "glass" => "minecraft:glass",
        "wool" => "minecraft:white_wool",
        "sand" => "minecraft:sand",
        "snow" => "minecraft:snow_block",
        "ladder" => "minecraft:ladder",
        "anvil" => "minecraft:anvil",
        "slime_block" => "minecraft:slime_block",
        "honey_block" => "minecraft:honey_block",
        "bamboo" => "minecraft:bamboo",
        "bamboo_wood" => "minecraft:bamboo_planks",
        "cherry_wood" => "minecraft:cherry_planks",
        "netherite_block" => "minecraft:netherite_block",
        "copper" => "minecraft:copper_block",
        "amethyst" => "minecraft:amethyst_block",
        "deepslate" => "minecraft:deepslate",
        "lantern" => "minecraft:lantern",
        "nether_bricks" => "minecraft:nether_bricks",
        "bone_block" => "minecraft:bone_block",
        "calcite" => "minecraft:calcite",
        "tuff" => "minecraft:tuff",
        "mud" => "minecraft:mud",
        "mud_bricks" => "minecraft:mud_bricks",
        "netherrack" => "minecraft:netherrack",
        "basalt" => "minecraft:basalt",
        "soul_sand" => "minecraft:soul_sand",
        "moss" => "minecraft:moss_block",
        "sculk" => "minecraft:sculk",
        _ => return None,
    })
}

/// An item file from the registration's `Item.Properties`.
fn item_json(args: &str) -> serde_json::Value {
    let template = if let Some(item) = vanilla_constant(args, "Items.") {
        format!("minecraft:{item}")
    } else if args.contains(".sword(") {
        "minecraft:iron_sword".into()
    } else if args.contains(".pickaxe(") {
        "minecraft:iron_pickaxe".into()
    } else if args.contains(".axe(") {
        "minecraft:iron_axe".into()
    } else if args.contains(".shovel(") {
        "minecraft:iron_shovel".into()
    } else if args.contains(".hoe(") {
        "minecraft:iron_hoe".into()
    } else if args.contains(".food(")
        || args.contains("foodItem(")
        || args.contains("FoodValues.")
        || args.contains("Foods.")
    {
        // Food, however the mod builds it (Farmer's Delight's `foodItem(FoodValues.CABBAGE)`).
        "minecraft:bread".into()
    } else {
        "minecraft:iron_ingot".into()
    };
    let mut json = serde_json::json!({ "template": template });
    if let Some(n) = call_numbers(args, ".stacksTo(").and_then(|v| v.first().copied()) {
        json["max_stack_size"] = (n.clamp(1.0, 99.0) as u32).into();
    }
    if let Some(r) = vanilla_constant(args, "Rarity.")
        && ["common", "uncommon", "rare", "epic"].contains(&r.as_str())
    {
        json["rarity"] = r.into();
    }
    json
}

/// The numbers passed to the first `call` in `text`: `.strength(3.5f, 6.0f)` → [3.5, 6].
fn call_numbers(text: &str, call: &str) -> Option<Vec<f64>> {
    let start = text.find(call)? + call.len();
    let end = start + text[start..].find(')')?;
    let numbers: Vec<f64> = text[start..end]
        .split(',')
        .filter_map(|n| {
            n.trim()
                .trim_end_matches(['f', 'F', 'd', 'D'])
                .parse::<f64>()
                .ok()
        })
        .collect();
    (!numbers.is_empty()).then_some(numbers)
}

/// Every file under `base`, by path relative to it with `/` separators.
fn walk(base: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push((rel.to_string_lossy().replace('\\', "/"), path));
            }
        }
    }
    out.sort();
    out
}

/// A content path Horizon accepts: parts of `[a-z0-9_.-]`.
fn valid_path(rel: &str) -> bool {
    rel.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
            })
    })
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-' | '/')
        })
}

fn pretty(json: &serde_json::Value) -> String {
    serde_json::to_string_pretty(json).unwrap_or_default() + "\n"
}

fn toml_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_block_properties() {
        let p = block_properties(
            "() -> new X(BlockBehaviour.Properties.ofFullCopy(Blocks.IRON_BLOCK).strength(3.5f, 6.0f).lightLevel(state -> 7).sound(SoundType.COPPER).requiresCorrectToolForDrops())",
        );
        assert_eq!(p["hardness"], 3.5);
        assert_eq!(p["explosion_resistance"], 6.0);
        assert_eq!(p["light"], 7);
        assert_eq!(p["sounds"], "minecraft:copper_block");
        assert_eq!(p["requires_tool"], true);
        assert_eq!(
            block_properties("Properties.of().strength(2)")["explosion_resistance"],
            2.0
        );
        assert!(block_properties("CrankBlock::new").is_empty());
    }

    #[test]
    fn guesses_templates_from_states_names_and_classes() {
        let states = |pairs: &[(&str, &[&str])]| -> BTreeMap<String, BTreeSet<String>> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.iter().map(|x| x.to_string()).collect()))
                .collect()
        };
        let slab = states(&[("type", &["top", "bottom", "double"])]);
        assert_eq!(
            template_for_states(&slab, "fir_slab").unwrap().0,
            "minecraft:oak_slab"
        );
        assert_eq!(
            template_for_states(&slab, "orpiment_brick_slab").unwrap().0,
            "minecraft:stone_slab"
        );
        let door = states(&[("hinge", &["left", "right"]), ("open", &["true", "false"])]);
        assert_eq!(
            template_for_states(&door, "fir_door").unwrap().0,
            "minecraft:oak_door"
        );
        assert_eq!(
            template_for_states(&states(&[]), "fir_leaves").unwrap().0,
            "minecraft:oak_leaves"
        );
        assert_eq!(
            superclass_template("TintedParticleLeavesBlock"),
            Some("minecraft:oak_leaves")
        );
        assert_eq!(
            superclass_template("CeilingHangingSignBlock"),
            Some("minecraft:oak_hanging_sign")
        );
        assert_eq!(
            superclass_template("FenceGateBlock"),
            Some("minecraft:oak_fence_gate")
        );
        assert_eq!(superclass_template("SomethingElse"), None);
    }

    #[test]
    fn reads_blockstate_properties() {
        let variants = serde_json::json!({ "variants": {
            "axis=x,powered=false": {}, "axis=y,powered=true": {}, "axis=z,powered=false": {} } });
        let p = blockstate_properties(&variants);
        assert_eq!(p["axis"].len(), 3);
        assert_eq!(
            template_for_states(&p, "log").unwrap().0,
            "minecraft:oak_log"
        );
        let mut skipped = Vec::new();
        let (declared, default) = declared_states("minecraft:oak_log", &p, "shaft", &mut skipped);
        assert_eq!(declared.keys().collect::<Vec<_>>(), ["powered"]);
        assert_eq!(declared["powered"]["type"], "boolean");
        assert_eq!(default["powered"], "false");
        // A template named by the Java code: its properties are unknown, so a note.
        let (declared, _) = declared_states("minecraft:gold_ore", &p, "ore", &mut skipped);
        assert!(declared.is_empty() && skipped.len() == 1);

        let multipart = serde_json::json!({ "multipart": [
            { "apply": {} },
            { "when": { "north": "true" }, "apply": {} },
            { "when": { "OR": [{ "level": "1|2|10" }, { "facing": "up" }] }, "apply": {} } ] });
        let p = blockstate_properties(&multipart);
        assert_eq!(p.keys().collect::<Vec<_>>(), ["facing", "level", "north"]);
        // `up` is no horizontal facing: no template from it.
        assert!(template_for_states(&p, "thing").is_none());
        let (decl, default) = declare(&p["level"]);
        assert_eq!(decl["values"], serde_json::json!(["1", "2", "10"]));
        assert_eq!(default, "1");
        let (_, default) = declare(&["south".to_string(), "north".to_string()].into());
        assert_eq!(default, "north");
    }

    #[test]
    fn reads_vanilla_constants() {
        assert_eq!(
            vanilla_constant("ofFullCopy(Blocks.IRON_BLOCK)", "Blocks.").as_deref(),
            Some("iron_block")
        );
        assert_eq!(vanilla_constant("ModBlocks.CRANK", "Blocks."), None);
        assert_eq!(vanilla_constant("x", "Blocks."), None);
    }

    #[test]
    fn reads_item_properties() {
        let j = item_json("new Item(new Item.Properties().stacksTo(16).rarity(Rarity.RARE))");
        assert_eq!(j["template"], "minecraft:iron_ingot");
        assert_eq!(j["max_stack_size"], 16);
        assert_eq!(j["rarity"], "rare");
        assert_eq!(
            item_json("new Item(new Item.Properties().food(Foods.APPLE))")["template"],
            "minecraft:bread"
        );
    }

    #[test]
    fn checks_resource_paths() {
        assert!(valid_path("assets/demo/textures/block/crank.png"));
        assert!(!valid_path("assets/demo/textures/block/Crank.png"));
        assert!(!valid_path("assets/demo/../x"));
    }
}
