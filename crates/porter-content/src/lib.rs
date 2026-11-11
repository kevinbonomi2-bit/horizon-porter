//! The content half of a port: a Horizon content mod (`horizon-mod.toml`, `assets/`,
//! `data/`) with the Java mod's own resources and a block or item file for each
//! registration. Horizon blocks and items copy a vanilla one (their template) and
//! change what differs (playhorizon.dev docs: mods.md, modpacks.md "Registries 0.2").

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod equipment;

use porter_model::{ModScan, Registration};

/// The data folders a content mod may carry (playhorizon.dev/docs/content-mods).
const DATA_KINDS: &[&str] = &["recipe", "loot_table", "tags", "structure", "worldgen"];
/// Asset folders Horizon renders without them (it draws with wgpu).
const SKIPPED_ASSETS: &[&str] = &["shaders"];
/// Namespaces a content mod can't list as its own.
const SHARED_NAMESPACES: &[&str] = &["minecraft", "c", "horizon"];
/// Asset kinds that make a namespace the mod's own: it names, models or draws
/// blocks and items there.
const OWN_ASSETS: &[&str] = &["lang", "blockstates", "items", "models"];

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
    /// Whether it has an item (`"item": false` in its file when not).
    pub item: bool,
    /// Its state properties, its template's and its own, each with its values.
    pub states: BTreeMap<String, Vec<String>>,
    /// Quarter turns clockwise from the `facing` it stores to the one Java would
    /// (its blockstate is turned so each draws what Java draws): 0 for most.
    pub facing_turns: u8,
}

/// An item the content mod adds (block items come with their blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub json: serde_json::Value,
    /// The Java item class, fully qualified, when found.
    pub class: Option<String>,
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
    /// The mod's mobs, as `id (template)`.
    pub entities: Vec<String>,
    /// Items that plant a block of another name (`cabbage_seeds` plants `cabbages`,
    /// `BlockItem::new` with the crop): (item, block, what the block stands on:
    /// `farmland`, `soil`, `water` or `any`).
    pub places: Vec<(String, String, String)>,
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

    // The mod's own namespaces: its id's, and any it gives blocks, items or names
    // to. The rest are other mods' (Xaero's map icons, Create's tags, EMI's
    // recipe trees): Java merges them into those mods, Horizon gives each
    // namespace to one content mod, so two mods carrying the same compat files
    // keep each other from loading. They do nothing without that mod anyway.
    let mut own: BTreeSet<String> = BTreeSet::from([ns.to_string(), id.to_string()]);
    for res in &scan.resources.roots {
        for (rel, _) in walk(&root.join(res)) {
            let parts: Vec<&str> = rel.split('/').collect();
            if let ["assets", n, category, ..] = parts.as_slice()
                && OWN_ASSETS.contains(category)
            {
                own.insert(n.to_string());
            }
        }
    }
    let mut foreign = BTreeSet::new();

    // The Java mod's own resources, from every resource root.
    for res in &scan.resources.roots {
        let base = root.join(res);
        for (rel, src) in walk(&base) {
            let parts: Vec<&str> = rel.split('/').collect();
            let [kind, ns, category, ..] = parts.as_slice() else {
                continue;
            };
            if !SHARED_NAMESPACES.contains(ns) && !own.contains(*ns) {
                foreign.insert(ns.to_string());
                continue;
            }
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
            // JSON as Horizon reads it, strictly: Java's reader skips a UTF-8 byte
            // order mark (Macaw's tags have one) and Horizon's doesn't, and one file
            // it can't read keeps every content mod from loading.
            if rel.ends_with(".json") || rel.ends_with(".mcmeta") {
                if content.copies.contains_key(&rel) || content.files.contains_key(&rel) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&src) else {
                    continue;
                };
                let text =
                    String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))
                        .into_owned();
                if serde_json::from_str::<serde_json::Value>(&text).is_err() {
                    content
                        .skipped
                        .push(format!("{rel}: not JSON Horizon can read (left out)"));
                    continue;
                }
                if bytes.starts_with(b"\xEF\xBB\xBF") {
                    content.files.insert(rel, text);
                    continue;
                }
            }
            content.copies.entry(rel).or_insert(src);
        }
    }
    for n in &foreign {
        content.skipped.push(format!(
            "assets/{n}, data/{n}: another mod's ({n}) compat files, left out: they work only with that mod, and a second mod carrying them would keep this one from loading"
        ));
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
                // Variants registered in a loop (`andesite_waystone`, `mossy_waystone`...)
                // are named after their class: `WaystoneBlock`.
                creates: (registry == "block")
                    .then(|| class_named_like(n, "Block", &class_of))
                    .flatten(),
                args: String::new(),
            })
            .collect()
    };
    // A mod for 1.21.4 and later draws each of its items by `assets/<ns>/items/`.
    let draws_items = content
        .copies
        .keys()
        .any(|k| k.starts_with(&format!("assets/{ns}/items/")));
    let resource_blocks = from_resources("blockstates", "block");
    let resource_items: Vec<Registration> = from_resources("items", "item")
        .into_iter()
        .filter(|r| {
            let n = r.id.as_deref().unwrap_or_default();
            !registered("block", n) && !resource_blocks.iter().any(|b| b.id.as_deref() == Some(n))
        })
        .collect();
    let all_blocks: BTreeSet<String> = scan
        .registrations
        .iter()
        .filter(|r| r.registry == "block")
        .chain(resource_blocks.iter())
        .filter_map(|r| r.id.clone())
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
        // `.lightLevel(GlowGoopBlock.LIGHT_EMISSION)`: the level its function gives.
        if !properties.contains_key("light")
            && let Some(level) = light_from_field(scan, &reg.args)
        {
            properties.insert("light".into(), level.into());
        }
        // The state properties its blockstate file draws it by.
        let blockstate_path = format!("assets/{ns}/blockstates/{block_name}.json");
        let blockstate = resource_json(&content, &blockstate_path);
        let states = blockstate
            .as_ref()
            .map(blockstate_properties)
            .unwrap_or_default();
        let opens_menu = [
            "minecraft:barrel",
            "minecraft:chest",
            "minecraft:trapped_chest",
            "minecraft:furnace",
            "minecraft:blast_furnace",
            "minecraft:smoker",
            "minecraft:hopper",
            "minecraft:dispenser",
            "minecraft:dropper",
        ]
        .contains(&template.as_str());
        if let Some(menu) = machines.get(block_name).filter(|_| !opens_menu) {
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
        } else if let Some((t, why)) = template_for_states(&states, block_name)
            .filter(|_| template == DEFAULT_TEMPLATE || !has_states_of(&template, &states))
        {
            // A block that copies another's properties but draws as stairs, a slab or
            // a door (Macaw's grass roof: `ofFullCopy(Blocks.DIRT)`, stairs' states)
            // needs that shape's template to be placed facing the player and to join
            // its neighbours; it keeps the copied block's sounds.
            if template != DEFAULT_TEMPLATE {
                properties
                    .entry("sounds")
                    .or_insert_with(|| template.clone().into());
                template_from = format!("{why} ({template_from}: its sounds)");
            } else {
                template_from = why;
            }
            template = t.to_string();
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
        // Random ticks: its class (or a mod superclass) has a `randomTick` the port
        // runs from `native_random_tick`, which Horizon calls only for blocks that
        // tick randomly; a template that doesn't (dirt under compost) would never.
        let ticks_randomly = class.as_deref().is_some_and(|c| {
            let mut name = c.to_string();
            for _ in 0..8 {
                let Some(k) = scan.classes.iter().find(|k| k.name == name) else {
                    return false;
                };
                if k.method_names.iter().any(|m| m == "randomTick") {
                    return true;
                }
                match &k.superclass {
                    Some(sup) => name = sup.clone(),
                    None => return false,
                }
            }
            false
        });
        if ticks_randomly {
            properties.insert("random_ticks".into(), true.into());
        }
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
        // A template must be one of the game's blocks.
        if vanilla_block(&template).is_none() {
            template_from = format!("{template} is not a block of 26.3: stone ({template_from})");
            template = DEFAULT_TEMPLATE.into();
        }
        if let Some(stand_in) = growing_plant_stand_in(&template) {
            template_from =
                format!("{stand_in}: {template} grows only as itself in Horizon ({template_from})");
            template = stand_in.into();
        }
        // Java's own state definition: properties its code sets that the blockstate
        // never draws by (a garage door's `powered`), and its defaults (a garage
        // door is placed `part=top`, `open=true`), which the blockstate can't say.
        let (java_values, mut java_defaults) = java_states(scan, class.as_deref());
        // A boolean the Java declares and its `registerDefaultState` doesn't set starts
        // `true`: `StateDefinition.any()` takes each property's first value, and a
        // boolean's are `true, false` (a basket's `enabled`).
        for (name, values) in &java_values {
            if !java_defaults.contains_key(name)
                && !java_defaults.is_empty()
                && values.iter().all(|v| v == "true" || v == "false")
            {
                java_defaults.insert(name.clone(), "true".into());
            }
        }
        let mut java_values = java_values;
        // A block that always holds water (Java's `LiquidBlockContainer` with no
        // `waterlogged` of its own: rice, a mod's seagrass). Horizon reads a block's
        // water from `waterlogged`, so it gets one, and starts with it.
        if holds_water(scan, class.as_deref()) && !java_values.contains_key("waterlogged") {
            java_values.insert(
                "waterlogged".into(),
                ["false", "true"].map(String::from).into(),
            );
            java_defaults.insert("waterlogged".into(), "true".into());
        }
        let mut all_states = states.clone();
        let own = template_properties(&template).unwrap_or_default();
        for (name, values) in java_values {
            if !all_states.contains_key(&name) && !own.contains(&name.as_str()) {
                all_states.insert(name, values);
            }
        }
        let (declared, mut default_state) =
            declared_states(&template, &all_states, block_name, &mut content.skipped);
        for (name, value) in java_defaults {
            let valid = declared
                .get(&name)
                .is_some_and(|d| match d["type"].as_str() {
                    Some("boolean") => value == "true" || value == "false",
                    _ => d["values"]
                        .as_array()
                        .is_some_and(|v| v.iter().any(|x| x.as_str() == Some(&value))),
                });
            if valid {
                default_state.insert(name, value.into());
            }
        }
        let mut json = serde_json::json!({ "template": template });
        // A template without an item (a potted plant, a vine's body, a portal) gives
        // the block none: Horizon refuses a block item copying no item, and one
        // refused block keeps every content mod from loading.
        // Nor does a block Java gives no item: one the mod draws no item for (a
        // wall sign, which its sign item places; a potted plant; a vine's strand),
        // or a liquid's, which only a bucket places.
        let liquid = reg.creates.as_deref() == Some("LiquidBlock")
            || extends(scan, class.as_deref(), "LiquidBlock");
        let item = !liquid
            && vanilla_block(&template).is_none_or(|b| b.item)
            && (!draws_items
                || content
                    .copies
                    .contains_key(&format!("assets/{ns}/items/{block_name}.json")));
        if !item {
            json["item"] = false.into();
        } else if (stand_in(&template_from)
            // The carved pumpkin is what a block that only turns copies.
            || (template == "minecraft:carved_pumpkin" && !template_from.contains("CarvedPumpkin")))
            && let Some(v) = vanilla_block(&template)
        {
            // A template copied for its facing or shape only: its item is a plain
            // block item, not one worn on the head (a carved pumpkin's), composted
            // or holding a container's items, as the Java block's item isn't.
            let mut components = serde_json::Map::new();
            for k in &v.item_extras {
                components.insert(format!("!{k}"), serde_json::json!({}));
            }
            for (k, value) in &v.item_plain {
                components.insert(k.clone(), value.clone());
            }
            if !components.is_empty() {
                json["components"] = serde_json::Value::Object(components);
            }
        }
        // A sign's item places the mod's own wall form (`fir_wall_sign` beside
        // `fir_sign`), not the template's oak one.
        let wall =
            if template.ends_with("_hanging_sign") && !template.ends_with("_wall_hanging_sign") {
                // `fir_hanging_sign` → `fir_wall_hanging_sign`; Farmer's Delight's
                // `hanging_canvas_sign` → `wall_hanging_canvas_sign`.
                block_name
                    .strip_suffix("_hanging_sign")
                    .map(|w| format!("{w}_wall_hanging_sign"))
                    .or_else(|| {
                        if block_name.starts_with("hanging_") {
                            Some(format!("wall_{block_name}"))
                        } else {
                            block_name
                                .contains("_hanging_")
                                .then(|| block_name.replacen("_hanging_", "_wall_hanging_", 1))
                        }
                    })
            } else if template.ends_with("_sign") && !template.ends_with("_wall_sign") {
                block_name
                    .strip_suffix("_sign")
                    .map(|w| format!("{w}_wall_sign"))
            } else {
                None
            };
        if let Some(wall) = wall.filter(|w| all_blocks.contains(w.as_str())) {
            json["wall_block"] = format!("{ns}:{wall}").into();
        }
        if let Some(thickness) = connecting {
            json["connects_to"] = format!("#{ns}:{block_name}_connects").into();
            json["thickness"] = thickness.into();
            pipes.push((block_name.to_string(), class.clone()));
        }
        // `getRenderShape`: a block its block entity draws every frame (a turning
        // shaft) keeps its model out of the world, and hides nothing behind it.
        if !properties.contains_key("render_shape")
            && let Some(shape) = render_shape(scan, class.as_deref(), &reg.args)
        {
            properties.insert("render_shape".into(), shape.into());
            properties
                .entry("occludes".to_string())
                .or_insert(false.into());
            template_from.push_str("; drawn by its block entity");
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
        // A value the template stores that the blockstate never draws gets a variant.
        let mut blockstate = blockstate;
        let mut rewritten = false;
        let mut facing_turns = 0;
        // A loader's own variants (NeoForge custom models: Lootr's barrel) as plain ones.
        if let Some(plain) = blockstate.as_ref().and_then(plain_variants) {
            blockstate = Some(plain);
            rewritten = true;
        }
        if let Some(filled) = vanilla_block(&template)
            .zip(blockstate.as_ref())
            .and_then(|(v, b)| fill_variants(b, &v.properties))
        {
            blockstate = Some(filled);
            rewritten = true;
        }
        // Placed facing as Java places it: the template turns `facing` from the
        // player's look its own way (a carved pumpkin toward the player), the Java
        // block its way (Macaw's curtain a quarter turn clockwise), so each stored
        // facing draws what Java draws for the facing it would have stored.
        if let (Some(java), Some(own)) = (
            placement_of(scan, class.as_deref()),
            template_turns(&template),
        ) && java != own
            && let Some(turned) = blockstate
                .as_ref()
                .and_then(|b| turn_facing(b, (java + 4 - own) % 4))
        {
            blockstate = Some(turned);
            rewritten = true;
            facing_turns = (java + 4 - own) % 4;
            template_from.push_str(&format!(
                "; placed facing as Java places it ({} quarter turns from {template}'s)",
                (java + 4 - own) % 4
            ));
        }
        if rewritten && let Some(b) = blockstate.as_ref() {
            content.copies.remove(&blockstate_path);
            content.files.insert(blockstate_path.clone(), pretty(b));
        }
        // A block that isn't a whole block, on a template that is (a curtain on a
        // carved pumpkin, a bud on stone), takes its outline and collision from
        // its own models, as Java's shapes follow them.
        // A shutter on a trapdoor stands upright where the trapdoor lies flat.
        let shutter = template.ends_with("_trapdoor") && states.contains_key("hinge");
        // Its state properties: its template's and those it declares.
        let mut schema: BTreeMap<String, BTreeSet<String>> = vanilla_block(&template)
            .map(|v| {
                v.properties
                    .iter()
                    .map(|(k, vals)| (k.clone(), vals.iter().cloned().collect()))
                    .collect()
            })
            .unwrap_or_default();
        for (k, d) in json
            .get("state_properties")
            .and_then(|d| d.as_object())
            .into_iter()
            .flatten()
        {
            let values: BTreeSet<String> = match d["type"].as_str() {
                Some("boolean") => ["false", "true"].map(String::from).into(),
                _ => d["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
            };
            schema.insert(k.clone(), values);
        }
        if connecting.is_none()
            && vanilla_block(&template).is_some_and(|v| v.full_cube || shutter)
            && let Some(b) = blockstate.as_ref()
            && let Some((outline, collision)) = model_shapes(&content, b, &schema)
        {
            {
                // Clicked on as a whole block (`getInteractionShape` is the full block, a
                // basket's open top): the outline, which clicks hit, stays whole, and the
                // model's shape is what the player bumps into.
                let whole = {
                    let mut name = class.clone();
                    let mut found = false;
                    for _ in 0..8 {
                        let Some(n) = name.take() else { break };
                        if scan.full_interaction.contains(&n) {
                            found = true;
                            break;
                        }
                        name = scan
                            .classes
                            .iter()
                            .find(|c| c.name == n)
                            .and_then(|c| c.superclass.clone());
                    }
                    found
                };
                if whole {
                    json["collision_shapes"] = collision.unwrap_or(outline);
                } else {
                    json["shapes"] = outline;
                    if let Some(c) = collision {
                        json["collision_shapes"] = c;
                    }
                }
                if !properties.contains_key("occludes") {
                    properties.insert("occludes".into(), false.into());
                    json["properties"] = serde_json::Value::Object(properties.clone());
                }
                template_from.push_str("; its shape from its models");
                content.files.insert(
                    format!("data/{ns}/horizon/block/{block_name}.json"),
                    pretty(&json),
                );
            }
        }
        // `.noCollision()`: nothing bumps into it, whatever its template is (a copy of
        // stone would be solid).
        if (reg.args.contains(".noCollision()") || reg.args.contains(".noCollission()"))
            && json.get("collision_shapes").is_none()
        {
            json["collision_shapes"] = serde_json::json!({ "default": [] });
            if !properties.contains_key("occludes") {
                properties.insert("occludes".into(), false.into());
            }
            json["properties"] = serde_json::Value::Object(properties.clone());
            content.files.insert(
                format!("data/{ns}/horizon/block/{block_name}.json"),
                pretty(&json),
            );
        }
        content.blocks.push(Block {
            name: block_name.to_string(),
            template,
            template_from,
            properties,
            class,
            item,
            states: schema
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect(),
            facing_turns,
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
    // The mod's mobs: each copies the vanilla mob it is most like, with its own
    // default attributes. Horizon's templates are a zombie (hostile), a wandering
    // trader (neutral, all goals) and a villager.
    let mut seen_mobs = BTreeSet::new();
    // Horizon takes one thrown type per item.
    let mut thrown_items = BTreeSet::new();
    for reg in scan
        .registrations
        .iter()
        .filter(|r| r.registry == "entity_type")
    {
        let Some(name) = reg.id.as_deref().filter(|n| valid_name(n)) else {
            continue;
        };
        if !seen_mobs.insert(name.to_string()) {
            continue;
        }
        let class = reg.creates.as_deref().and_then(class_of);
        // Something thrown that flies as an item (a rotten tomato, a mod's egg): Horizon's
        // thrown item type, a snowball drawn as the mod's item. Using the item throws it.
        if let Some(item) = thrown_item(scan, name, reg, class.as_deref())
            .filter(|item| thrown_items.insert(item.clone()))
        {
            content.files.insert(
                format!("data/{ns}/horizon/entity_type/{name}.json"),
                pretty(&serde_json::json!({
                    "template": "minecraft:snowball",
                    "item": format!("{ns}:{item}"),
                })),
            );
            content.entities.push(format!(
                "{ns}:{name} (minecraft:snowball, thrown as {ns}:{item})"
            ));
            continue;
        }
        let Some((template, attributes_allowed)) = mob_template(scan, reg, class.as_deref()) else {
            content.skipped.push(format!(
                "entity `{name}`: not a mob Horizon has a template for (projectiles, vehicles and others are left out)"
            ));
            continue;
        };
        let mut attributes = serde_json::Map::new();
        if let Some(mob) = class
            .as_deref()
            .and_then(|c| scan.mobs.iter().find(|m| m.class == c))
        {
            for (attr, value) in &mob.attributes {
                if attributes_allowed.contains(&attr.as_str()) {
                    attributes.insert(attr.clone(), (*value).into());
                }
            }
        }
        let mut json = serde_json::json!({ "template": template });
        if !attributes.is_empty() {
            json["attributes"] = serde_json::Value::Object(attributes);
        }
        content.files.insert(
            format!("data/{ns}/horizon/entity_type/{name}.json"),
            pretty(&json),
        );
        content.entities.push(format!("{ns}:{name} ({template})"));
    }
    // The mod's mob effects: `new MobEffect(MobEffectCategory.HARMFUL, 0x8E3B46)`.
    for reg in scan
        .registrations
        .iter()
        .filter(|r| r.registry == "mob_effect")
    {
        let Some(name) = reg.id.as_deref().filter(|n| valid_name(n)) else {
            continue;
        };
        let category = ["BENEFICIAL", "HARMFUL", "NEUTRAL"]
            .into_iter()
            .find(|c| reg.args.contains(&format!("MobEffectCategory.{c}")));
        let color = reg.args.split([',', '(', ')', ' ']).find_map(|w| {
            let w = w.trim();
            w.strip_prefix("0x")
                .or_else(|| w.strip_prefix("0X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| {
                    (w.len() >= 4 && w.chars().all(|c| c.is_ascii_digit()))
                        .then(|| w.parse().ok())
                        .flatten()
                })
        });
        // Else its class's `super(MobEffectCategory.HARMFUL, 0x8E3B46)`.
        let from_class = reg
            .creates
            .as_deref()
            .and_then(class_of)
            .and_then(|c| scan.effects.iter().find(|(k, _, _)| *k == c));
        let (category, color) = match (category, color, from_class) {
            (Some(c), Some(col), _) => (Some(c.to_ascii_lowercase()), Some(col)),
            (_, _, Some((_, c, col))) => (Some(c.clone()), Some(*col)),
            _ => (None, None),
        };
        let (Some(category), Some(color)) = (category, color) else {
            content.skipped.push(format!(
                "mob effect `{name}`: its category or color isn't in its registration"
            ));
            continue;
        };
        content.files.insert(
            format!("data/{ns}/horizon/mob_effect/{name}.json"),
            pretty(&serde_json::json!({
                "category": category,
                "color": color.min(0xFF_FFFF),
            })),
        );
    }
    let blocks: BTreeSet<String> = content.blocks.iter().map(|b| b.name.clone()).collect();
    // Items that plant a block of another name: the block registered with the same
    // first argument (`ModBlockItemIds.CABBAGE_CROP`, a constant naming both).
    for reg in scan.registrations.iter().filter(|r| r.registry == "item") {
        let Some(item_name) = reg.id.as_deref().filter(|n| valid_name(n)) else {
            continue;
        };
        // A `BlockItem`, or a class of the mod's built on one (Farmer's Delight's `RiceItem`).
        let item_class = reg.creates.as_deref().and_then(class_of);
        let block_item =
            reg.args.contains("BlockItem") || extends(scan, item_class.as_deref(), "BlockItem");
        if !block_item {
            continue;
        }
        let key = reg.args.split(',').next().unwrap_or("").trim();
        if key.is_empty() || key.starts_with('"') {
            continue;
        }
        let Some(block) = scan
            .registrations
            .iter()
            .filter(|r| r.registry == "block")
            .find(|r| r.args.split(',').next().map(str::trim) == Some(key))
            .and_then(|r| r.id.clone())
            .filter(|b| blocks.contains(b))
        else {
            continue;
        };
        let template = content
            .blocks
            .iter()
            .find(|b| b.name == block)
            .map_or("", |b| b.template.as_str());
        // A crop whose Java checks the water it stands in (`fluid.is(FluidTags.WATER)` in
        // its `canSurvive`: rice) is planted in water.
        let block_class = scan
            .registrations
            .iter()
            .find(|r| r.registry == "block" && r.id.as_deref() == Some(block.as_str()))
            .and_then(|r| r.creates.as_deref())
            .and_then(class_of);
        let in_water = holds_water(scan, block_class.as_deref());
        let rule = match template {
            _ if in_water => "water",
            "minecraft:wheat"
            | "minecraft:carrots"
            | "minecraft:potatoes"
            | "minecraft:beetroots"
            | "minecraft:torchflower_crop"
            | "minecraft:pitcher_crop"
            | "minecraft:melon_stem"
            | "minecraft:pumpkin_stem" => "farmland",
            "minecraft:seagrass" | "minecraft:kelp" | "minecraft:tall_seagrass" => "water",
            t if t.ends_with("_sapling")
                || t.ends_with("bush")
                || t.contains("flower")
                || t.ends_with("_tulip") =>
            {
                "soil"
            }
            _ => "any",
        };
        // An item named as its block places it itself, except a crop's (rice): a crop's
        // template has no item that places it, so the port plants it.
        if blocks.contains(item_name) && !(in_water || rule == "farmland") {
            continue;
        }
        content
            .places
            .push((item_name.to_string(), block, rule.to_string()));
    }
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
        let mut json = item_json(&reg.args);
        // A tool, weapon or armor piece: its material's stats.
        if let Some((template, components)) = equipment::equipment(scan, ns, &reg.args) {
            json["template"] = template.into();
            // A damageable item never stacks (`Item.Properties.durability`); Horizon takes
            // the size beside the components.
            if components.get("minecraft:max_damage").is_some() {
                json["max_stack_size"] = 1.into();
            }
            json["components"] = components;
        } else if let Some(mob) = item_name.strip_suffix("_spawn_egg") {
            // A spawn egg (`SpawnEggItem`): vanilla's, spawning the mod's mob, or the
            // vanilla one of that name (Friends & Foes' illusioner).
            let own = format!("{ns}:{mob}");
            let id = if content
                .entities
                .iter()
                .any(|e| e.split(' ').next() == Some(own.as_str()))
            {
                own
            } else {
                format!("minecraft:{mob}")
            };
            json["template"] = "minecraft:pig_spawn_egg".into();
            json["components"] = serde_json::json!({ "minecraft:entity_data": { "id": id } });
        } else if reg.args.contains("drinkItem(")
            || reg.args.contains("DEFAULT_DRINK")
            || reg.args.contains("ItemUseAnimation.DRINK")
        {
            // A drink (`Consumables.DEFAULT_DRINK`): drunk, not eaten, from a bottle it
            // gives back; the honey bottle's own effect (curing poison) left out.
            json["template"] = "minecraft:honey_bottle".into();
            json["components"] = serde_json::json!({
                "minecraft:consumable": {
                    "animation": "drink",
                    "sound": "minecraft:entity.generic.drink",
                    "consume_seconds": 1.6,
                    "has_consume_particles": false,
                    "on_consume_effects": [],
                }
            });
        }
        // Its food and how it is eaten or drunk, from the mod's constants
        // (`FoodValues.BEEF_STEW`, `FoodValues.ConsumableValues.APPLE_CIDER`): the
        // template's own replaced.
        let named = |name: &str| {
            reg.args.match_indices(name).any(|(i, _)| {
                let after = reg.args[i + name.len()..].chars().next();
                let before = reg.args[..i].chars().next_back();
                !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        };
        let food = scan.foods.iter().find(|(n, _)| named(n));
        let consumable = scan.consumables.iter().find(|(n, _)| named(n));
        if food.is_some() || consumable.is_some() {
            let mut components = json["components"].as_object().cloned().unwrap_or_default();
            if let Some(f) =
                food.and_then(|(_, j)| serde_json::from_str::<serde_json::Value>(j).ok())
            {
                components.insert("minecraft:food".into(), f);
            }
            if let Some(c) =
                consumable.and_then(|(_, j)| serde_json::from_str::<serde_json::Value>(j).ok())
            {
                // The effect ids of the mod's own effects get its namespace.
                let mut c = c;
                if let Some(list) = c["on_consume_effects"].as_array_mut() {
                    for e in list {
                        if let Some(effects) = e["effects"].as_array_mut() {
                            for x in effects {
                                if let Some(id) = x["id"].as_str().filter(|i| !i.contains(':')) {
                                    x["id"] = format!("{ns}:{id}").into();
                                }
                            }
                        }
                    }
                }
                components.insert("minecraft:consumable".into(), c);
            }
            json["components"] = serde_json::Value::Object(components);
        }
        // What it leaves in a crafting grid (`.craftRemainder(Items.BOWL)`, read
        // through the mod's helpers): Horizon's crafting and the mod's code
        // (`getCraftingRemainder()`, a cooking pot's container) read it. A food or
        // drink also leaves it once eaten, as the mod's item class gives it back.
        let remainder = vanilla_constant(&reg.args, ".craftRemainder(Items.");
        if let Some(r) = &remainder {
            json["crafting_remainder"] = format!("minecraft:{r}").into();
            let eaten = json["components"].get("minecraft:food").is_some()
                || json["components"].get("minecraft:consumable").is_some();
            if eaten {
                let mut components = json["components"].as_object().cloned().unwrap_or_default();
                components
                    .entry("minecraft:use_remainder")
                    .or_insert_with(|| serde_json::json!({ "id": format!("minecraft:{r}") }));
                json["components"] = serde_json::Value::Object(components);
            }
        }
        // A bowl food (`bowlFoodItem(..)`, `usingConvertsTo(Items.BOWL)`): the bowl comes
        // back, and it stacks to 16.
        if remainder.is_none()
            && (reg.args.contains("bowlFoodItem(")
                || reg.args.contains("usingConvertsTo(Items.BOWL)"))
        {
            let mut components = json["components"].as_object().cloned().unwrap_or_default();
            components.insert(
                "minecraft:use_remainder".into(),
                serde_json::json!({ "id": "minecraft:bowl" }),
            );
            json["components"] = serde_json::Value::Object(components);
            if json.get("max_stack_size").is_none() {
                json["max_stack_size"] = 16.into();
            }
        }
        if json.get("components").is_some() {
        } else if reg.args.contains("fireResistant()") {
            // `Item.Properties.fireResistant()`: it doesn't burn in lava or fire.
            json["components"] = serde_json::json!({ "minecraft:damage_resistant": { "types": "#minecraft:is_fire" } });
        }
        content.files.insert(
            format!("data/{ns}/horizon/item/{item_name}.json"),
            pretty(&json),
        );
        content.items.push(Item {
            name: item_name.to_string(),
            json,
            class: reg.creates.as_deref().and_then(class_of),
        });
    }

    creative_tabs(scan, ns, name, &mut content);
    convention_tags(&mut content);
    item_definitions(ns, &mut content);

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

/// The mod's creative inventory tabs (`data/<ns>/horizon/creative_tab/`): Java
/// gives a mod tabs of its own, and Horizon lists them after vanilla's. Each tab
/// keeps the title, icon and items its code names; a mod whose tabs name none
/// (Biomes O' Plenty fills its tab by reflection) or that builds none gets one
/// tab of all its items.
fn creative_tabs(scan: &ModScan, ns: &str, name: &str, content: &mut Content) {
    let mut all: Vec<&str> = content
        .blocks
        .iter()
        .filter(|b| b.item)
        .map(|b| b.name.as_str())
        .collect();
    all.extend(content.items.iter().map(|i| i.name.as_str()));
    if all.is_empty() {
        return;
    }
    let lang_path = format!("assets/{ns}/lang/en_us.json");
    let lang: serde_json::Value = content
        .files
        .get(&lang_path)
        .cloned()
        .or_else(|| {
            content
                .copies
                .get(&lang_path)
                .and_then(|p| std::fs::read_to_string(p).ok())
        })
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    // A lang key the mod translates, or text that isn't a key; else the mod's name.
    let title = |t: Option<&String>| match t {
        Some(t) if lang.get(t).is_some() || t.contains(' ') || !t.contains('.') => t.clone(),
        _ => name.to_string(),
    };
    // (file name, title, icon, items)
    let mut tabs: Vec<(String, String, String, Vec<String>)> = Vec::new();
    for (i, tab) in scan.creative_tabs.iter().enumerate() {
        let items: Vec<String> = tab
            .items
            .iter()
            .filter(|x| all.contains(&x.as_str()))
            .map(|x| format!("{ns}:{x}"))
            .collect();
        let file = tab
            .id
            .clone()
            .filter(|id| valid_name(id))
            .unwrap_or_else(|| {
                if i == 0 {
                    ns.to_string()
                } else {
                    format!("tab_{i}")
                }
            });
        let icon = tab
            .icon
            .as_deref()
            .filter(|x| all.contains(x))
            .map(|x| format!("{ns}:{x}"))
            .unwrap_or_default();
        tabs.push((file, title(tab.title.as_ref()), icon, items));
    }
    if tabs.iter().all(|t| t.3.is_empty()) {
        let mut first = tabs
            .drain(..)
            .next()
            .unwrap_or_else(|| (ns.to_string(), name.to_string(), String::new(), Vec::new()));
        // Those the mod names: one it never names (a potted plant's item) is one
        // its own tab leaves out.
        let named = |x: &str| {
            lang.as_object().is_none_or(|l| l.is_empty())
                || ["block", "item"]
                    .iter()
                    .any(|k| lang.get(format!("{k}.{ns}.{x}")).is_some())
        };
        first.3 = all
            .iter()
            .filter(|x| named(x))
            .map(|x| format!("{ns}:{x}"))
            .collect();
        tabs = vec![first];
    }
    let mut written = BTreeSet::new();
    for (file, title, icon, items) in tabs {
        if items.is_empty() || !written.insert(file.clone()) {
            continue;
        }
        let icon = if icon.is_empty() {
            items[0].clone()
        } else {
            icon
        };
        content.files.insert(
            format!("data/{ns}/horizon/creative_tab/{file}.json"),
            pretty(&serde_json::json!({ "title": title, "icon": icon, "items": items })),
        );
    }
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
    // A vanilla class made directly that one vanilla block is named after
    // (`StrawBedBlock::new` makes `minecraft:straw_bed`): that block.
    if class.is_none()
        && let Some(simple) = reg.creates.as_deref()
        && let Some(base) = simple.strip_suffix("Block")
    {
        let mut snake = String::new();
        for (i, c) in base.chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        }
        let id = format!("minecraft:{snake}");
        if vanilla_block(&id).is_some() {
            return (id, format!("makes vanilla's {simple}"));
        }
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
/// A vanilla block: its state properties, and whether it has an item.
pub struct VanillaBlock {
    pub item: bool,
    /// Its state properties, each with its values.
    pub properties: BTreeMap<String, Vec<String>>,
    /// Whether every state's outline is the whole block.
    pub full_cube: bool,
    /// Components its item has that a plain block item's (stone's) hasn't: a carved
    /// pumpkin's `equippable`, a furnace's `container`.
    pub item_extras: Vec<String>,
    /// Components every block item has, with the plain item's value where its own
    /// differs (the pumpkin's `attribute_modifiers`, which hide its wearer).
    pub item_plain: serde_json::Map<String, serde_json::Value>,
}

/// The game's blocks (26.3), from `data/vanilla-blocks.json` (`tools/vanilla_blocks.py`).
fn vanilla_block(id: &str) -> Option<&'static VanillaBlock> {
    static BLOCKS: std::sync::OnceLock<BTreeMap<String, VanillaBlock>> = std::sync::OnceLock::new();
    BLOCKS
        .get_or_init(|| {
            let json: serde_json::Value =
                serde_json::from_str(include_str!("../data/vanilla-blocks.json"))
                    .unwrap_or_default();
            json.as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        VanillaBlock {
                            item: v["item"].as_bool().unwrap_or(true),
                            full_cube: v["full_cube"].as_bool().unwrap_or(true),
                            item_extras: v["item_extras"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect(),
                            item_plain: v["item_plain"].as_object().cloned().unwrap_or_default(),
                            properties: v["properties"]
                                .as_object()
                                .into_iter()
                                .flatten()
                                .map(|(k, vals)| {
                                    let vals = vals
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|x| x.as_str().map(str::to_string))
                                        .collect();
                                    (k.clone(), vals)
                                })
                                .collect(),
                        },
                    )
                })
                .collect()
        })
        .get(id)
}

/// A resource file of the content mod as JSON: generated, else copied.
fn resource_json(content: &Content, path: &str) -> Option<serde_json::Value> {
    let text = match content.files.get(path) {
        Some(t) => t.clone(),
        None => std::fs::read_to_string(content.copies.get(path)?).ok()?,
    };
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// A blockstate file's `variants` with one for each value of the template's state
/// properties it draws by but never names: that value draws as its opposite
/// (`south` as `north`), else as the first value it names. Java folds such values
/// in code (Macaw's windows store `north` for a window placed facing south, which
/// looks the same), where the template stores them as they are, and a state with
/// no variant draws nothing. None when nothing is missing.
fn fill_variants(
    blockstate: &serde_json::Value,
    template: &BTreeMap<String, Vec<String>>,
) -> Option<serde_json::Value> {
    let mut variants = blockstate.get("variants")?.as_object()?.clone();
    let parse = |key: &str| -> Vec<(String, String)> {
        key.split(',')
            .filter_map(|c| c.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect()
    };
    let opposite = |v: &str| match v {
        "north" => Some("south"),
        "south" => Some("north"),
        "east" => Some("west"),
        "west" => Some("east"),
        "up" => Some("down"),
        "down" => Some("up"),
        _ => None,
    };
    let mut changed = false;
    for (prop, values) in template {
        let named: BTreeSet<String> = variants
            .keys()
            .flat_map(|k| parse(k))
            .filter(|(k, _)| k == prop)
            .map(|(_, v)| v)
            .collect();
        let Some(first) = named.iter().next().cloned() else {
            continue;
        };
        for missing in values.iter().filter(|v| !named.contains(*v)) {
            let like = opposite(missing)
                .filter(|o| named.contains(*o))
                .map_or(first.clone(), str::to_string);
            let added: Vec<(String, serde_json::Value)> = variants
                .iter()
                .filter_map(|(key, model)| {
                    let parts = parse(key);
                    parts.iter().any(|(k, v)| k == prop && *v == like).then(|| {
                        let key = parts
                            .iter()
                            .map(|(k, v)| format!("{k}={}", if k == prop { missing } else { v }))
                            .collect::<Vec<_>>()
                            .join(",");
                        (key, model.clone())
                    })
                })
                .collect();
            for (key, model) in added {
                if !variants.contains_key(&key) {
                    variants.insert(key, model);
                    changed = true;
                }
            }
        }
    }
    changed.then(|| {
        let mut filled = blockstate.clone();
        filled["variants"] = serde_json::Value::Object(variants);
        filled
    })
}

/// How a Java block class turns its `facing` when placed (its own
/// `getStateForPlacement`, else the nearest superclass's): quarter turns
/// clockwise from the way the player looks.
fn placement_of(scan: &ModScan, class: Option<&str>) -> Option<u8> {
    let mut name = class.map(str::to_string);
    for _ in 0..16 {
        let n = name.take()?;
        if let Some((_, turns)) = scan.placements.iter().find(|(c, _)| *c == n) {
            return Some(*turns);
        }
        name = scan
            .classes
            .iter()
            .find(|c| c.name == n)?
            .superclass
            .clone();
    }
    None
}

/// The same for a template, when the porter knows it: the carved pumpkin and
/// the furnaces face the player, stairs, doors and gates the way they look.
fn template_turns(template: &str) -> Option<u8> {
    match template {
        "minecraft:carved_pumpkin"
        | "minecraft:jack_o_lantern"
        | "minecraft:furnace"
        | "minecraft:blast_furnace"
        | "minecraft:smoker"
        | "minecraft:oak_trapdoor"
        | "minecraft:iron_trapdoor" => Some(2),
        "minecraft:oak_stairs"
        | "minecraft:stone_stairs"
        | "minecraft:oak_door"
        | "minecraft:iron_door"
        | "minecraft:oak_fence_gate" => Some(0),
        _ => None,
    }
}

/// A blockstate whose variant for each horizontal `facing` is the one it had
/// for that facing turned `turns` quarter turns clockwise. None for one not
/// drawn by `facing` variants.
fn turn_facing(blockstate: &serde_json::Value, turns: u8) -> Option<serde_json::Value> {
    const RING: [&str; 4] = ["north", "east", "south", "west"];
    let variants = blockstate.get("variants")?.as_object()?;
    let mut out = serde_json::Map::new();
    let mut any = false;
    for (key, model) in variants {
        // This variant now draws the facing `turns` quarter turns counterclockwise.
        let parts: Vec<String> = key
            .split(',')
            .map(|c| match c.split_once('=') {
                Some((k, v)) if k.trim() == "facing" => {
                    match RING.iter().position(|d| *d == v.trim()) {
                        Some(i) => {
                            any = true;
                            format!("facing={}", RING[(i + 4 - turns as usize) % 4])
                        }
                        None => c.to_string(),
                    }
                }
                _ => c.to_string(),
            })
            .collect();
        out.insert(parts.join(","), model.clone());
    }
    any.then(|| {
        let mut turned = blockstate.clone();
        turned["variants"] = serde_json::Value::Object(out);
        turned
    })
}

/// A box in block units: `[minX, minY, minZ, maxX, maxY, maxZ]`.
type Box6 = [f64; 6];

/// Vanilla's block models: each one's parent and boxes, from
/// `data/vanilla-models.json` (`tools/vanilla_blocks.py`).
/// A vanilla model's parent, and its boxes when it has elements.
type VanillaModel = (Option<String>, Option<Vec<Box6>>);

fn vanilla_model(id: &str) -> Option<&'static VanillaModel> {
    static MODELS: std::sync::OnceLock<BTreeMap<String, VanillaModel>> = std::sync::OnceLock::new();
    MODELS
        .get_or_init(|| {
            let json: serde_json::Value =
                serde_json::from_str(include_str!("../data/vanilla-models.json"))
                    .unwrap_or_default();
            json.as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| {
                    let parent = v["parent"].as_str().map(str::to_string);
                    let elements = v["elements"].as_array().map(|a| {
                        a.iter()
                            .filter_map(|e| {
                                let n: Vec<f64> =
                                    e.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
                                <[f64; 6]>::try_from(n).ok()
                            })
                            .collect()
                    });
                    (k.clone(), (parent, elements))
                })
                .collect()
        })
        .get(id)
}

/// `block/stone` is `minecraft:block/stone`.
fn model_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// A model's boxes in block units, through its parents: the mod's models, else
/// vanilla's. None when no model in the chain has elements (a block entity's
/// `builtin/entity`) or the chain can't be read.
fn model_boxes(content: &Content, id: &str) -> Option<Vec<Box6>> {
    let mut id = model_id(id);
    for _ in 0..16 {
        let (ns, path) = id.split_once(':')?;
        let (parent, elements) =
            match resource_json(content, &format!("assets/{ns}/models/{path}.json")) {
                Some(j) => {
                    let elements = j.get("elements").and_then(|e| e.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|e| {
                                let corner = |k: &str| -> Option<Vec<f64>> {
                                    e.get(k)?.as_array()?.iter().map(|x| x.as_f64()).collect()
                                };
                                let (f, t) = (corner("from")?, corner("to")?);
                                (f.len() == 3 && t.len() == 3)
                                    .then(|| [f[0], f[1], f[2], t[0], t[1], t[2]])
                            })
                            .collect::<Vec<_>>()
                    });
                    (j["parent"].as_str().map(str::to_string), elements)
                }
                None => vanilla_model(&id).cloned()?,
            };
        if let Some(e) = elements {
            return Some(e.into_iter().map(|b| b.map(|v| v / 16.0)).collect());
        }
        // A model drawn by a block entity's renderer has no boxes to go by; one
        // whose chain ends with none draws nothing (raised blinds' lower half).
        let Some(parent) = parent else {
            return Some(Vec::new());
        };
        if parent.contains("builtin/") {
            return None;
        }
        id = model_id(&parent);
    }
    None
}

/// `b` turned as a blockstate variant turns its model: `x` degrees about the X
/// axis (up toward north), then `y` about the Y axis (north toward east).
fn rotate(b: Box6, x: i64, y: i64) -> Box6 {
    let turn = |p: [f64; 3]| {
        let mut p = p;
        for _ in 0..(x.rem_euclid(360) / 90) {
            p = [p[0], p[2], 1.0 - p[1]];
        }
        for _ in 0..(y.rem_euclid(360) / 90) {
            p = [1.0 - p[2], p[1], p[0]];
        }
        p
    };
    let (a, c) = (turn([b[0], b[1], b[2]]), turn([b[3], b[4], b[5]]));
    [
        a[0].min(c[0]),
        a[1].min(c[1]),
        a[2].min(c[2]),
        a[0].max(c[0]),
        a[1].max(c[1]),
        a[2].max(c[2]),
    ]
}

/// The shape a variant draws: its model's boxes, turned. Flat ones (a cross
/// plant's planes, a window's glass) don't count beside solid ones; a model of
/// planes only outlines their bounds and has no collision, as Java's plants.
/// None for a model with no boxes. The last flag says the model lies wholly
/// outside its block (an open shutter swung out), so its shape is a guess.
fn variant_shape(
    content: &Content,
    variant: &serde_json::Value,
) -> Option<(Vec<Box6>, bool, bool)> {
    let v = match variant {
        serde_json::Value::Array(a) => a.first()?,
        v => v,
    };
    let boxes = model_boxes(content, v["model"].as_str()?)?;
    let (x, y) = (v["x"].as_i64().unwrap_or(0), v["y"].as_i64().unwrap_or(0));
    let round = |b: Box6| b.map(|c| (c.clamp(0.0, 1.0) * 10000.0).round() / 10000.0);
    let has_volume = |b: &Box6| b[3] > b[0] && b[4] > b[1] && b[5] > b[2];
    let outside = boxes.iter().any(has_volume);
    let turned: Vec<Box6> = boxes.into_iter().map(|b| round(rotate(b, x, y))).collect();
    let solid: Vec<Box6> = turned.iter().copied().filter(has_volume).collect();
    if !solid.is_empty() {
        // Flat panes among solid parts (a window's glass, blinds' slats) are thin
        // walls: a click lands on them, not through. More parts than Horizon takes
        // (16) are merged on a voxel grid, so a bridge keeps its deck low and its
        // rails apart, and you walk on the deck as in Java.
        let parts: Vec<Box6> = turned.iter().map(|b| thicken(*b)).collect();
        return Some((merge_parts(&parts), true, false));
    }
    if turned.is_empty() {
        // Nothing drawn: nothing to target or bump into.
        return Some((Vec::new(), false, false));
    }
    Some((vec![bounds(&turned)], false, outside))
}

/// A box flat along an axis (a pane, a slat) made one pixel thick about its middle.
fn thicken(b: Box6) -> Box6 {
    let mut b = b;
    for i in 0..3 {
        if b[i + 3] - b[i] < 1.0 / 16.0 {
            let mid = (b[i] + b[i + 3]) / 2.0;
            b[i] = (mid - 1.0 / 32.0).clamp(0.0, 1.0 - 1.0 / 16.0);
            b[i + 3] = b[i] + 1.0 / 16.0;
        }
    }
    b
}

/// At most 16 boxes covering `parts`, as Horizon takes a shape: the parts
/// themselves when they are few, else their union on a voxel grid, merged into
/// boxes, the grid coarser until they fit; their bounds as a last resort.
fn merge_parts(parts: &[Box6]) -> Vec<Box6> {
    if parts.len() <= 16 {
        return parts.to_vec();
    }
    for n in [16usize, 8, 4, 2] {
        let merged = voxel_boxes(parts, n);
        if merged.len() <= 16 {
            return merged;
        }
    }
    vec![bounds(parts)]
}

/// The union of `parts` on an n³ grid (a cell is in when a part covers most of
/// it along each axis), greedily merged into boxes: runs along x, then rows along
/// z, then layers along y.
fn voxel_boxes(parts: &[Box6], n: usize) -> Vec<Box6> {
    let cell = 1.0 / n as f64;
    let covered = |lo: f64, hi: f64, i: usize| {
        let (a, b) = (i as f64 * cell, (i + 1) as f64 * cell);
        hi.min(b) - lo.max(a) >= cell / 2.0 - 1e-9
    };
    let mut on = vec![false; n * n * n];
    let at = |x: usize, y: usize, z: usize| (y * n + z) * n + x;
    for p in parts {
        for y in 0..n {
            if !covered(p[1], p[4], y) {
                continue;
            }
            for z in 0..n {
                if !covered(p[2], p[5], z) {
                    continue;
                }
                for x in 0..n {
                    if covered(p[0], p[3], x) {
                        on[at(x, y, z)] = true;
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                if !on[at(x, y, z)] {
                    continue;
                }
                // Grow along x, then z, then y while every cell is in.
                let mut x2 = x;
                while x2 + 1 < n && on[at(x2 + 1, y, z)] {
                    x2 += 1;
                }
                let row = |zz: usize, yy: usize| (x..=x2).all(|xx| on[at(xx, yy, zz)]);
                let mut z2 = z;
                while z2 + 1 < n && row(z2 + 1, y) {
                    z2 += 1;
                }
                let mut y2 = y;
                while y2 + 1 < n && (z..=z2).all(|zz| row(zz, y2 + 1)) {
                    y2 += 1;
                }
                for yy in y..=y2 {
                    for zz in z..=z2 {
                        for xx in x..=x2 {
                            on[at(xx, yy, zz)] = false;
                        }
                    }
                }
                let c = |i: usize| (i as f64 * cell * 10000.0).round() / 10000.0;
                out.push([c(x), c(y), c(z), c(x2 + 1), c(y2 + 1), c(z2 + 1)]);
            }
        }
    }
    out
}

fn bounds(boxes: &[Box6]) -> Box6 {
    let mut out = boxes[0];
    for b in boxes {
        for i in 0..3 {
            out[i] = out[i].min(b[i]);
            out[i + 3] = out[i + 3].max(b[i + 3]);
        }
    }
    out
}

/// A block's `shapes` (and `collision_shapes` when some states have none), from
/// the models its blockstate's variants draw, keyed by the variants' properties.
/// None when every state is a whole block, the blockstate is multipart, or its
/// variants don't all name the same properties. Only values `schema` (the
/// block's own) has are keyed: Horizon refuses a table naming any other.
fn model_shapes(
    content: &Content,
    blockstate: &serde_json::Value,
    schema: &BTreeMap<String, BTreeSet<String>>,
) -> Option<(serde_json::Value, Option<serde_json::Value>)> {
    let variants = blockstate.get("variants")?.as_object()?;
    let mut props: Option<Vec<String>> = None;
    let mut rows: Vec<(String, Vec<Box6>, bool)> = Vec::new();
    let mut outside: Vec<usize> = Vec::new();
    for (key, variant) in variants {
        let mut parts: Vec<(String, String)> = key
            .split(',')
            .filter_map(|c| c.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        if parts
            .iter()
            .any(|(k, v)| !schema.get(k).is_some_and(|vals| vals.contains(v)))
        {
            continue;
        }
        parts.sort();
        let names: Vec<String> = parts.iter().map(|(k, _)| k.clone()).collect();
        match &props {
            None => props = Some(names),
            Some(p) if *p != names => return None,
            _ => {}
        }
        let (boxes, collides, out) = variant_shape(content, variant)?;
        let values: Vec<&str> = parts.iter().map(|(_, v)| v.as_str()).collect();
        if out {
            outside.push(rows.len());
        }
        rows.push((values.join(","), boxes, collides));
    }
    let props = props?;
    // A state drawn wholly outside its block (an open shutter swung out) takes
    // the shape of one that differs from it in one property (the closed one):
    // Java's shapes don't follow a model past its block.
    for &i in &outside {
        let key: Vec<&str> = rows[i].0.split(',').collect();
        let near = rows.iter().enumerate().find(|(j, (k, b, c))| {
            !outside.contains(j)
                && *c
                && !b.is_empty()
                && k.split(',').zip(&key).filter(|(a, b)| a != *b).count() == 1
        });
        if let Some((_, (_, boxes, _))) = near {
            let boxes = boxes.clone();
            rows[i].1 = boxes;
            rows[i].2 = true;
        }
    }
    const WHOLE: Box6 = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    // Every state a whole block, or none drawing anything (a block its renderer
    // draws): the template's shape stands.
    if rows.iter().all(|(_, b, c)| *c && b.as_slice() == [WHOLE])
        || rows.iter().all(|(_, b, _)| b.is_empty())
    {
        return None;
    }
    let boxes_json = |b: &[Box6]| serde_json::json!(b);
    let table = |rows: &[(String, serde_json::Value)]| {
        if props.is_empty() {
            return serde_json::json!({ "default": rows[0].1 });
        }
        let values: serde_json::Map<String, serde_json::Value> =
            rows.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        // A state no variant names is drawn as a whole block.
        serde_json::json!({ "properties": props, "values": values, "default": [WHOLE] })
    };
    let outline: Vec<(String, serde_json::Value)> = rows
        .iter()
        .map(|(k, b, _)| (k.clone(), boxes_json(b)))
        .collect();
    let collision = rows.iter().any(|(_, _, c)| !c).then(|| {
        let rows: Vec<(String, serde_json::Value)> = rows
            .iter()
            .map(|(k, b, c)| {
                (
                    k.clone(),
                    if *c {
                        boxes_json(b)
                    } else {
                        serde_json::json!([])
                    },
                )
            })
            .collect();
        table(&rows)
    });
    Some((table(&outline), collision))
}

/// A Java block class's state definition (its own, else the nearest superclass's):
/// the values of each property whose kind its field says (a boolean, an integer
/// range), and the defaults `registerDefaultState` sets, by property name.
fn java_states(
    scan: &ModScan,
    class: Option<&str>,
) -> (BTreeMap<String, BTreeSet<String>>, BTreeMap<String, String>) {
    let chain: Vec<&porter_model::ClassInfo> = {
        let mut out = Vec::new();
        let mut name = class.map(str::to_string);
        while let Some(n) = name.take() {
            let Some(c) = scan.classes.iter().find(|c| c.name == n) else {
                break;
            };
            out.push(c);
            if out.len() > 16 {
                break;
            }
            name = c.superclass.clone();
        }
        out
    };
    let definition = |want_fields: bool| {
        chain.iter().find_map(|c| {
            scan.state_definitions.iter().find(|(k, f, d)| {
                *k == c.name
                    && if want_fields {
                        !f.is_empty()
                    } else {
                        !d.is_empty()
                    }
            })
        })
    };
    // A property field's name and values: `PART = EnumProperty.create("part", ..)`,
    // `POWERED = BlockStateProperties.POWERED`, `FACING` inherited from vanilla.
    let property = |field: &str| -> (String, Option<BTreeSet<String>>) {
        let init = chain
            .iter()
            .flat_map(|c| c.fields.iter())
            .find(|f| f.name == field)
            .and_then(|f| f.init.clone())
            .unwrap_or_default();
        let literal = init
            .split('"')
            .nth(1)
            .filter(|_| init.contains(".create(\""));
        let constant = init
            .rsplit('.')
            .next()
            .filter(|c| {
                !c.is_empty()
                    && c.chars()
                        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
            })
            .unwrap_or(field);
        let name = literal.map_or_else(|| vanilla_property_name(constant), str::to_string);
        let values = if init.starts_with("BooleanProperty")
            || (literal.is_none() && BOOLEAN_PROPERTIES.contains(&constant))
        {
            Some(["false", "true"].map(String::from).into())
        } else if init.starts_with("IntegerProperty.create(") {
            let nums: Vec<i64> = init
                .split([',', '(', ')'])
                .filter_map(|w| w.trim().parse().ok())
                .collect();
            match nums.as_slice() {
                [lo, hi] if lo <= hi && hi - lo < 64 => {
                    Some((*lo..=*hi).map(|n| n.to_string()).collect())
                }
                _ => None,
            }
        } else {
            None
        };
        (name, values)
    };
    let mut values = BTreeMap::new();
    if let Some((_, fields, _)) = definition(true) {
        for f in fields {
            if let (name, Some(v)) = property(f) {
                values.insert(name, v);
            }
        }
    }
    let mut defaults = BTreeMap::new();
    if let Some((_, _, d)) = definition(false) {
        for (f, v) in d {
            defaults.insert(property(f).0, v.clone());
        }
    }
    (values, defaults)
}

/// Vanilla's boolean state properties, as `BlockStateProperties` names them.
const BOOLEAN_PROPERTIES: &[&str] = &[
    "OPEN",
    "POWERED",
    "WATERLOGGED",
    "LIT",
    "ATTACHED",
    "OCCUPIED",
    "TRIGGERED",
    "ENABLED",
    "IN_WALL",
    "EXTENDED",
    "HANGING",
    "SNOWY",
    "INVERTED",
    "PERSISTENT",
    "UNSTABLE",
    "BOTTOM",
    "DISARMED",
    "CONDITIONAL",
    "DRAG",
    "EYE",
    "HAS_BOOK",
    "SIGNAL_FIRE",
    "BERRIES",
    "SHORT",
    "LOCKED",
    "CRACKED",
    "NATURAL",
    "TIP",
    "SHRIEKING",
    "CAN_SUMMON",
    "BLOOM",
];

/// The name a vanilla `BlockStateProperties` constant has in a blockstate:
/// `HORIZONTAL_FACING` is `facing`, `DOOR_HINGE` `hinge`, `OPEN` `open`.
fn vanilla_property_name(constant: &str) -> String {
    match constant {
        "HORIZONTAL_FACING" | "FACING_HOPPER" | "VERTICAL_DIRECTION" => "facing".into(),
        "DOOR_HINGE" => "hinge".into(),
        "DOUBLE_BLOCK_HALF" | "HALF" => "half".into(),
        "HORIZONTAL_AXIS" => "axis".into(),
        "STAIRS_SHAPE" | "RAIL_SHAPE" | "RAIL_SHAPE_STRAIGHT" => "shape".into(),
        "SLAB_TYPE" | "CHEST_TYPE" | "PISTON_TYPE" => "type".into(),
        "ATTACH_FACE" => "face".into(),
        "BED_PART" => "part".into(),
        "BELL_ATTACHMENT" => "attachment".into(),
        "NOTEBLOCK_INSTRUMENT" => "instrument".into(),
        c if c.starts_with("AGE_") => "age".into(),
        c if c.starts_with("LEVEL") => "level".into(),
        c => c.to_ascii_lowercase(),
    }
}

/// A blockstate whose variants a model loader of a mod's own draws (NeoForge's
/// custom block state models: `{"type": "lootr:custom", "unopened": ..,
/// "vanilla": .., "state": {"x": 180}}`) with plain variants Horizon reads: the
/// model a player first sees (`unopened`, else `vanilla`, `base`, `model`, else the
/// first block model it names), turned as its `state` says. None when every
/// variant is plain already.
fn plain_variants(blockstate: &serde_json::Value) -> Option<serde_json::Value> {
    let variants = blockstate.get("variants")?.as_object()?;
    let plain = |v: &serde_json::Value| -> Option<serde_json::Value> {
        let o = v.as_object()?;
        if o.get("model").is_some_and(|m| m.is_string()) && !o.contains_key("type") {
            return None;
        }
        let model = ["unopened", "vanilla", "base", "model"]
            .iter()
            .find_map(|k| o.get(*k)?.as_str())
            .or_else(|| {
                o.values()
                    .filter_map(|x| x.as_str())
                    .find(|x| x.contains("block/"))
            })?;
        let mut out = serde_json::Map::new();
        out.insert("model".into(), model.into());
        let turn = o.get("state").and_then(|s| s.as_object()).unwrap_or(o);
        for k in ["x", "y", "uvlock"] {
            if let Some(x) = turn.get(k).or_else(|| o.get(k)) {
                out.insert(k.into(), x.clone());
            }
        }
        Some(serde_json::Value::Object(out))
    };
    let mut changed = false;
    let mut out = serde_json::Map::new();
    for (key, v) in variants {
        let fixed = match v {
            serde_json::Value::Array(a) => {
                let items: Vec<serde_json::Value> = a
                    .iter()
                    .map(|x| {
                        plain(x)
                            .inspect(|_| changed = true)
                            .unwrap_or_else(|| x.clone())
                    })
                    .collect();
                serde_json::Value::Array(items)
            }
            other => plain(other)
                .inspect(|_| changed = true)
                .unwrap_or_else(|| other.clone()),
        };
        out.insert(key.clone(), fixed);
    }
    changed.then(|| {
        let mut b = blockstate.clone();
        b["variants"] = serde_json::Value::Object(out);
        b
    })
}

/// Whether a template was picked to stand in for the Java block's facing, shape or
/// menu (`template_from` says why), not because the block is one of its kind
/// (`extends X`, `Blocks.X in its properties`, a door's or leaves' states).
fn stand_in(template_from: &str) -> bool {
    [
        "its blockstates turn on",
        "it opens a menu",
        "a pipe",
        "no template found",
        "is not a block of 26.3",
        "shutter (one block",
    ]
    .iter()
    .any(|why| template_from.contains(why))
}

/// Whether `template` has any of the state properties a blockstate file draws by.
fn has_states_of(template: &str, states: &BTreeMap<String, BTreeSet<String>>) -> bool {
    template_properties(template)
        .is_some_and(|own| states.keys().any(|p| own.contains(&p.as_str())))
}

/// A block for a mod block to copy in place of a growing plant's (kelp, cave,
/// twisting and weeping vines, head or body). Horizon's survival rule for those
/// pairs a head with its body by the block's own name, and a world refuses to
/// open with a block copying one ("unknown growing plant family",
/// horizon-world `block_survival.rs`). The stand-ins hang, stand or sway alike.
fn growing_plant_stand_in(template: &str) -> Option<&'static str> {
    Some(match template {
        "minecraft:cave_vines"
        | "minecraft:cave_vines_plant"
        | "minecraft:weeping_vines"
        | "minecraft:weeping_vines_plant" => "minecraft:hanging_roots",
        "minecraft:twisting_vines" | "minecraft:twisting_vines_plant" => "minecraft:nether_sprouts",
        "minecraft:kelp" | "minecraft:kelp_plant" => "minecraft:seagrass",
        _ => return None,
    })
}

fn template_properties(template: &str) -> Option<Vec<&'static str>> {
    // Every vanilla block's, from the game's block facts.
    if let Some(b) = vanilla_block(template) {
        return Some(b.properties.keys().map(String::as_str).collect());
    }
    template_properties_known(template).map(<[&str]>::to_vec)
}

/// The templates the porter picks itself, for a game whose facts aren't known.
fn template_properties_known(template: &str) -> Option<&'static [&'static str]> {
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
    // A sign, by its name and states (Farmer's Delight's `hanging_canvas_sign`): it
    // takes text, as a sign's template does.
    if name.ends_with("sign") {
        let template = if name.contains("wall_hanging") {
            "minecraft:oak_wall_hanging_sign"
        } else if name.contains("hanging") {
            "minecraft:oak_hanging_sign"
        } else if name.contains("wall") {
            "minecraft:oak_wall_sign"
        } else if has("rotation") {
            "minecraft:oak_sign"
        } else {
            ""
        };
        if !template.is_empty() {
            return Some((
                template,
                format!("its name (`{name}`) and states are a sign's"),
            ));
        }
    }
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
    if has("hinge") && has("open") && has("half") {
        return kind("minecraft:iron_door", "minecraft:oak_door", "door");
    }
    // Hinged and opening but one block tall (Macaw's shutters): a door is two,
    // so it opens on a click as a trapdoor does.
    if has("hinge") && has("open") {
        return kind(
            "minecraft:iron_trapdoor",
            "minecraft:oak_trapdoor",
            "shutter (one block, hinged)",
        );
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
        // Clings to the faces around it, one property a face.
        "MultifaceBlock" | "MultifaceSpreadeableBlock" => "minecraft:glow_lichen",
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
                ("MultifaceBlock", "minecraft:glow_lichen"),
                ("WaterlilyBlock", "minecraft:lily_pad"),
                ("FlowerPotBlock", "minecraft:flower_pot"),
                ("HugeMushroomBlock", "minecraft:brown_mushroom_block"),
                ("AmethystClusterBlock", "minecraft:amethyst_cluster"),
                // Not the vines themselves: see `growing_plant_stand_in`.
                ("GrowingPlantHeadBlock", "minecraft:hanging_roots"),
                ("GrowingPlantBodyBlock", "minecraft:hanging_roots"),
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
/// Whether `class` (fully qualified) extends the Minecraft class `simple`.
fn extends(scan: &ModScan, class: Option<&str>, simple: &str) -> bool {
    let mut name = class.map(str::to_string);
    for _ in 0..16 {
        let Some(n) = name.take() else {
            return false;
        };
        if n.rsplit('.').next() == Some(simple) {
            return true;
        }
        name = scan
            .classes
            .iter()
            .find(|c| c.name == n)
            .and_then(|c| c.superclass.clone());
    }
    false
}

/// A mod from before 1.21.4 draws an item by `models/item/<name>.json` alone; the game
/// now asks `items/<name>.json` which model to draw. Each item model of the mod's without
/// one gets the plain definition that names it, so its items aren't blank in the hand.
/// (A model's `overrides` aren't carried into it: the item shows its base model.)
fn item_definitions(ns: &str, content: &mut Content) {
    // A mod that ships any is from 1.21.4 or later: its other item models are parts.
    let shipped = format!("assets/{ns}/items/");
    if content.copies.keys().any(|k| k.starts_with(&shipped)) {
        return;
    }
    let prefix = format!("assets/{ns}/models/item/");
    let names: Vec<String> = content
        .copies
        .keys()
        .chain(content.files.keys())
        .filter_map(|k| k.strip_prefix(&prefix)?.strip_suffix(".json"))
        .filter(|n| !n.contains('/'))
        .map(str::to_string)
        .collect();
    for name in names {
        let path = format!("assets/{ns}/items/{name}.json");
        if content.copies.contains_key(&path) || content.files.contains_key(&path) {
            continue;
        }
        content.files.insert(
            path,
            pretty(&serde_json::json!({
                "model": { "type": "minecraft:model", "model": format!("{ns}:item/{name}") }
            })),
        );
    }
}

/// The loader's common `c:` tags (`#c:crops/carrot`) the mod's data names but doesn't
/// ship, from Fabric API's (`data/convention-tags.json`, `tools/convention_tags.py`):
/// on Java the loader provides them, and a recipe naming one matches nothing without it.
fn convention_tags(content: &mut Content) {
    static TAGS: std::sync::OnceLock<BTreeMap<String, Vec<String>>> = std::sync::OnceLock::new();
    let table = TAGS.get_or_init(|| {
        serde_json::from_str(include_str!("../data/convention-tags.json")).unwrap_or_default()
    });
    // Every `#c:..` the mod's data files name.
    let mut wanted: Vec<String> = Vec::new();
    for (path, file) in &content.copies {
        if !path.starts_with("data/") || !path.ends_with(".json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for part in text.split('"').skip(1).step_by(2) {
            if let Some(tag) = part.strip_prefix("#c:")
                && !wanted.iter().any(|w| w == tag)
            {
                wanted.push(tag.to_string());
            }
        }
    }
    let shipped = |kind: &str, tag: &str, content: &Content| {
        let path = format!("data/c/tags/{kind}/{tag}.json");
        content.copies.contains_key(&path) || content.files.contains_key(&path)
    };
    while let Some(tag) = wanted.pop() {
        for kind in ["item", "block"] {
            let Some(values) = table.get(&format!("{kind}:c:{tag}")) else {
                continue;
            };
            if shipped(kind, &tag, content) {
                continue;
            }
            // Its own `#c:` entries come too; any the table lacks are left out.
            let values: Vec<String> = values
                .iter()
                .filter(|v| {
                    v.strip_prefix("#c:")
                        .is_none_or(|inner| table.contains_key(&format!("{kind}:c:{inner}")))
                })
                .cloned()
                .collect();
            for v in &values {
                if let Some(inner) = v.strip_prefix("#c:")
                    && !shipped(kind, inner, content)
                    && !wanted.iter().any(|w| w == inner)
                {
                    wanted.push(inner.to_string());
                }
            }
            content.files.insert(
                format!("data/c/tags/{kind}/{tag}.json"),
                pretty(&serde_json::json!({ "values": values })),
            );
        }
    }
}

fn name_template(name: &str) -> Option<&'static str> {
    // A flower pot holding the mod's plant (`potted_fir_sapling`), not the plant.
    if name.starts_with("potted_") {
        return Some("minecraft:flower_pot");
    }
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

/// The vanilla mob a mod's mob copies, and the attributes that template has:
/// hostile ones a zombie, villagers a villager, other creatures a wandering trader
/// (neutral, all goals). None for what isn't a mob (projectiles, vehicles).
fn mob_template(
    scan: &ModScan,
    reg: &Registration,
    class: Option<&str>,
) -> Option<(&'static str, &'static [&'static str])> {
    const ZOMBIE: &[&str] = &[
        "minecraft:max_health",
        "minecraft:movement_speed",
        "minecraft:attack_damage",
        "minecraft:armor",
        "minecraft:follow_range",
    ];
    const OTHER: &[&str] = &[
        "minecraft:max_health",
        "minecraft:movement_speed",
        "minecraft:armor",
    ];
    let chain = class_chain(scan, reg, class);
    let any = |words: &[&str]| chain.iter().any(|c| words.iter().any(|w| c.contains(w)));
    if any(&[
        "Projectile",
        "Arrow",
        "Boat",
        "Minecart",
        "Vehicle",
        "ItemEntity",
        "Display",
    ]) {
        return None;
    }
    if any(&["Villager"]) {
        return Some(("minecraft:villager", OTHER));
    }
    let hostile = reg.args.contains("MobCategory.MONSTER")
        || any(&[
            "Monster", "Zombie", "Skeleton", "Spider", "Creeper", "Slime", "Raider", "Illager",
            "Blaze", "Phantom",
        ]);
    if hostile {
        return Some(("minecraft:zombie", ZOMBIE));
    }
    let creature = [
        "CREATURE",
        "AMBIENT",
        "WATER_CREATURE",
        "WATER_AMBIENT",
        "UNDERGROUND_WATER_CREATURE",
        "AXOLOTLS",
    ]
    .iter()
    .any(|c| reg.args.contains(&format!("MobCategory.{c}")))
        || any(&[
            "Animal",
            "AgeableMob",
            "PathfinderMob",
            "AmbientCreature",
            "WaterAnimal",
            "Mob",
            "Golem",
            "Fish",
            "Squid",
            "Horse",
        ]);
    creature.then_some(("minecraft:wandering_trader", OTHER))
}

/// The mod's class a block found only by its resources is most likely made by: the one
/// named after the end of its id (`andesite_waystone` is a `WaystoneBlock`, `portstone` a
/// `PortstoneBlock`), the longest such name first. `known` tells whether the mod has
/// exactly one class of a simple name.
fn class_named_like(
    id: &str,
    suffix: &str,
    known: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let words: Vec<&str> = id.split('_').filter(|w| !w.is_empty()).collect();
    (0..words.len()).find_map(|from| {
        let mut name: String = words[from..]
            .iter()
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect();
        name.push_str(suffix);
        known(&name).map(|_| name)
    })
}

/// Whether the mod's block `name` (its id's path) keeps its model out of the world, for
/// its block entity to draw: what its `render_shape` in the content mod says.
pub fn drawn_by_its_entity(scan: &ModScan, name: &str) -> bool {
    scan.registrations
        .iter()
        .filter(|r| r.registry == "block" && r.id.as_deref() == Some(name))
        .any(|r| {
            let simple = r.creates.as_deref();
            let mut found = scan
                .classes
                .iter()
                .filter(|c| !c.is_mixin && c.name.rsplit('.').next() == simple);
            let class = match (found.next(), found.next()) {
                (Some(c), None) => Some(c.name.as_str()),
                _ => None,
            };
            render_shape(scan, class, &r.args).is_some()
        })
}

/// How a block's class says it is drawn (`getRenderShape`), from its own up through
/// the mod's superclasses, when not by its model: `ENTITYBLOCK_ANIMATED`, `INVISIBLE`.
fn render_shape(scan: &ModScan, class: Option<&str>, args: &str) -> Option<String> {
    // Its model swapped for a class of the mod's that draws nothing in the world
    // (`.onRegister(blockModel(() -> BracketedKineticBlockModel::new))`).
    let swapped = scan.render_shapes.iter().find(|(c, shape)| {
        let simple = c.rsplit('.').next().unwrap_or(c);
        shape != "MODEL"
            && (args.contains(&format!("{simple}::")) || args.contains(&format!("new {simple}(")))
    });
    if let Some((_, shape)) = swapped {
        return Some(shape.clone());
    }
    let mut next = class.map(str::to_string);
    for _ in 0..12 {
        let Some(name) = next.take() else {
            break;
        };
        if let Some((_, shape)) = scan.render_shapes.iter().find(|(c, _)| *c == name) {
            return (shape != "MODEL").then(|| shape.clone());
        }
        next = scan
            .classes
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.superclass.clone());
    }
    None
}

/// An entity type's classes by simple name, from its own up to the first vanilla one.
fn class_chain(scan: &ModScan, reg: &Registration, class: Option<&str>) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut next = class.map(str::to_string).or_else(|| reg.creates.clone());
    while let Some(name) = next.take() {
        let simple = name.rsplit('.').next().unwrap_or(&name).to_string();
        chain.push(simple);
        if chain.len() > 12 {
            break;
        }
        next = scan
            .classes
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.superclass.clone());
    }
    chain
}

/// The mod's item a thrown projectile type flies as (`ThrowableItemProjectile` and
/// vanilla's own: a snowball, an egg), when it is one: the item whose class throws it
/// (it names the projectile's class), or the item of the type's own name.
fn thrown_item(
    scan: &ModScan,
    name: &str,
    reg: &Registration,
    class: Option<&str>,
) -> Option<String> {
    let chain = class_chain(scan, reg, class);
    let thrown = [
        "ThrowableItemProjectile",
        "Snowball",
        "ThrownEgg",
        "ThrownEnderpearl",
        "ThrownExperienceBottle",
    ];
    if !chain.iter().any(|c| thrown.contains(&c.as_str())) {
        return None;
    }
    let own = chain.first()?;
    // The mod's items, with the class of each that has its own.
    let items = || {
        scan.registrations
            .iter()
            .filter(|r| r.registry == "item")
            .filter_map(|r| {
                Some((
                    r.id.as_deref().filter(|n| valid_name(n))?,
                    r.creates.as_deref(),
                ))
            })
    };
    let names = |simple: &str| {
        scan.classes.iter().any(|k| {
            k.name.rsplit('.').next() == Some(simple)
                && k.references
                    .iter()
                    .any(|r| r.rsplit(['.', '$']).next() == Some(own.as_str()))
        })
    };
    items()
        .find(|(_, creates)| creates.is_some_and(|c| names(c.rsplit('.').next().unwrap_or(c))))
        .or_else(|| items().find(|(id, _)| *id == name))
        .map(|(id, _)| id.to_string())
}

/// The light of `.lightLevel(SomeBlock.LIGHT_EMISSION)`, a function kept in a field
/// (`state -> state.getValue(GOOP) * 5`, `state -> 12`): the level it gives, for the
/// lowest state when it grows with a property. Content takes one level a block; a level
/// that is on or off by state (`getValue(LIT) ? 13 : 0`) is left out.
fn light_from_field(scan: &ModScan, args: &str) -> Option<u8> {
    let rest = &args[args.find(".lightLevel(")? + ".lightLevel(".len()..];
    let arg = rest[..rest.find(')')?].trim();
    if arg.contains("->") || arg.is_empty() {
        return None;
    }
    let mut path = arg.rsplit('.');
    let field = path.next()?;
    let owner = path.next();
    let init = scan
        .classes
        .iter()
        .filter(|c| owner.is_none_or(|o| c.name.rsplit(['.', '$']).next() == Some(o)))
        .flat_map(|c| c.fields.iter())
        .find(|f| f.name == field && f.init.as_deref().is_some_and(|i| i.contains("->")))?
        .init
        .as_deref()?;
    let body = init.split_once("->")?.1.trim();
    if body.contains('?') {
        return None;
    }
    // `.. * 5`, or the number alone.
    let number = body.rsplit('*').next()?.trim().trim_end_matches(';');
    number.parse::<u8>().ok().map(|n| n.min(15))
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
    // It ticks randomly whatever its template does (a crop, a spreading soil).
    if args.contains(".randomTicks()") {
        p.insert("random_ticks".into(), true.into());
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
    // `.instabreak()`: broken at a touch (a flower, a torch).
    if args.contains(".instabreak()") {
        p.insert("hardness".into(), 0.into());
        p.insert("explosion_resistance".into(), 0.into());
    }
    // `.mapColor(MapColor.WOOD)`, `.mapColor(DyeColor.RED)`: its color on maps.
    if let Some(id) = vanilla_constant(args, "MapColor.")
        .and_then(|c| map_color(&c))
        .or_else(|| {
            let dye = vanilla_constant(args, "DyeColor.")?;
            map_color(match dye.as_str() {
                "white" => "snow",
                "lime" => "color_light_green",
                other => return map_color(&format!("color_{other}")),
            })
        })
    {
        p.insert("map_color".into(), id.into());
    }
    p
}

/// A `MapColor` constant's id (`MapColor.WOOD` is 13), as 26.3 numbers them.
fn map_color(name: &str) -> Option<u8> {
    const NAMES: [&str; 62] = [
        "none",
        "grass",
        "sand",
        "wool",
        "fire",
        "ice",
        "metal",
        "plant",
        "snow",
        "clay",
        "dirt",
        "stone",
        "water",
        "wood",
        "quartz",
        "color_orange",
        "color_magenta",
        "color_light_blue",
        "color_yellow",
        "color_light_green",
        "color_pink",
        "color_gray",
        "color_light_gray",
        "color_cyan",
        "color_purple",
        "color_blue",
        "color_brown",
        "color_green",
        "color_red",
        "color_black",
        "gold",
        "diamond",
        "lapis",
        "emerald",
        "podzol",
        "nether",
        "terracotta_white",
        "terracotta_orange",
        "terracotta_magenta",
        "terracotta_light_blue",
        "terracotta_yellow",
        "terracotta_light_green",
        "terracotta_pink",
        "terracotta_gray",
        "terracotta_light_gray",
        "terracotta_cyan",
        "terracotta_purple",
        "terracotta_blue",
        "terracotta_brown",
        "terracotta_green",
        "terracotta_red",
        "terracotta_black",
        "crimson_nylium",
        "crimson_stem",
        "crimson_hyphae",
        "warped_nylium",
        "warped_stem",
        "warped_hyphae",
        "warped_wart_block",
        "deepslate",
        "raw_iron",
        "glow_lichen",
    ];
    NAMES.iter().position(|n| *n == name).map(|i| i as u8)
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
    // A vanilla item it copies (`Items.HONEY_BOTTLE`), not one it leaves behind
    // (`.craftRemainder(Items.BOWL)`) or is mended with.
    let own = args
        .replace(".craftRemainder(Items.", ".craftRemainder(")
        .replace("usingConvertsTo(Items.", "usingConvertsTo(")
        .replace(".repairable(Items.", ".repairable(");
    let template = if let Some(item) = vanilla_constant(&own, "Items.") {
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
    // A basic string: one line, so a description's line breaks are escaped.
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Whether a block of `class` (fully qualified) always holds water: its class, or one it
/// is built on, is a `LiquidBlockContainer` (rice) or one of vanilla's water plants.
fn holds_water(scan: &ModScan, class: Option<&str>) -> bool {
    let mut name = class.map(str::to_string);
    for _ in 0..16 {
        let Some(n) = name.take() else {
            return false;
        };
        let simple = n.rsplit('.').next().unwrap_or(&n);
        if matches!(
            simple,
            "SeagrassBlock" | "TallSeagrassBlock" | "KelpBlock" | "KelpPlantBlock"
        ) {
            return true;
        }
        let Some(c) = scan.classes.iter().find(|c| c.name == n) else {
            return false;
        };
        if c.interfaces
            .iter()
            .any(|i| i.rsplit('.').next() == Some("LiquidBlockContainer"))
        {
            return true;
        }
        name = c.superclass.clone();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_mods_item_models_get_definitions() {
        let mut content = Content::default();
        content
            .copies
            .insert("assets/spin/models/item/shaft.json".into(), "x".into());
        content
            .copies
            .insert("assets/spin/models/item/wand/core.json".into(), "x".into());
        item_definitions("spin", &mut content);
        let made = content
            .files
            .get("assets/spin/items/shaft.json")
            .expect("a definition");
        assert!(made.contains("\"spin:item/shaft\""), "{made}");
        // A part in a folder is no item.
        assert_eq!(content.files.len(), 1);
        // A mod that ships its own definitions is left alone.
        let mut newer = Content::default();
        newer
            .copies
            .insert("assets/spin/models/item/shaft.json".into(), "x".into());
        newer
            .copies
            .insert("assets/spin/items/cog.json".into(), "x".into());
        item_definitions("spin", &mut newer);
        assert!(newer.files.is_empty());
    }

    #[test]
    fn a_variant_is_named_after_its_class() {
        let known = |n: &str| {
            ["WaystoneBlock", "PortstoneBlock"]
                .contains(&n)
                .then(|| format!("m.{n}"))
        };
        assert_eq!(
            class_named_like("mossy_andesite_waystone", "Block", &known).as_deref(),
            Some("WaystoneBlock")
        );
        assert_eq!(
            class_named_like("portstone", "Block", &known).as_deref(),
            Some("PortstoneBlock")
        );
        assert_eq!(class_named_like("warp_plate", "Block", &known), None);
    }

    #[test]
    fn map_color_and_instabreak() {
        let p = block_properties(
            "Properties.of().mapColor(MapColor.WOOD).instabreak().sound(SoundType.WOOD)",
        );
        assert_eq!(p["map_color"], 13);
        assert_eq!(p["hardness"], 0);
        let p = block_properties("Properties.of().mapColor(DyeColor.RED).strength(2.0F)");
        assert_eq!(p["map_color"], 28);
        assert_eq!(map_color("glow_lichen"), Some(61));
    }

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
    fn reads_a_loaders_own_variants_as_plain_ones() {
        let barrel = serde_json::json!({ "variants": {
            "facing=down,open=false": {
                "type": "lootr:custom", "open": false,
                "opened": "lootr:block/opened_barrel",
                "state": { "x": 180 },
                "unopened": "lootr:block/barrel_unopened",
                "vanilla": "minecraft:block/barrel"
            },
            "facing=up,open=false": { "model": "minecraft:block/barrel" }
        }});
        let plain = plain_variants(&barrel).unwrap();
        assert_eq!(
            plain["variants"]["facing=down,open=false"],
            serde_json::json!({ "model": "lootr:block/barrel_unopened", "x": 180 })
        );
        assert_eq!(
            plain["variants"]["facing=up,open=false"],
            barrel["variants"]["facing=up,open=false"]
        );
        // Plain already: nothing to do.
        assert!(plain_variants(&plain).is_none());
    }

    #[test]
    fn turns_facing_to_place_as_java() {
        // Java's curtain faces a quarter turn clockwise from the look; a carved
        // pumpkin stores the opposite of it: a turn of three quarters between them.
        let curtain = serde_json::json!({ "variants": {
            "facing=north,open=false": { "model": "m:n" },
            "facing=east,open=false": { "model": "m:e" },
            "facing=south,open=false": { "model": "m:s" },
            "facing=west,open=false": { "model": "m:w" },
        }});
        let turned = turn_facing(&curtain, 3).unwrap();
        // Looking north the pumpkin stores south, where Java stores east.
        assert_eq!(
            turned["variants"]["facing=south,open=false"]["model"],
            "m:e"
        );
        assert_eq!(
            turned["variants"]["facing=north,open=false"]["model"],
            "m:w"
        );
        assert!(turn_facing(&serde_json::json!({ "variants": { "": {} } }), 1).is_none());
    }

    #[test]
    fn stand_ins_give_a_plain_item() {
        assert!(stand_in("its blockstates turn on a horizontal `facing`"));
        assert!(stand_in(
            "extends HorizontalDirectionalBlock; its blockstates turn on a horizontal `facing`"
        ));
        assert!(!stand_in("extends CarvedPumpkinBlock"));
        assert!(!stand_in("Blocks.OAK_PLANKS in its properties"));
        let pumpkin = vanilla_block("minecraft:carved_pumpkin").unwrap();
        assert!(
            pumpkin
                .item_extras
                .iter()
                .any(|k| k == "minecraft:equippable")
        );
        assert_eq!(
            pumpkin.item_plain["minecraft:attribute_modifiers"],
            serde_json::json!([])
        );
    }

    #[test]
    fn many_parts_merge_and_keep_their_gaps() {
        // A bridge: a deck and two rails of many small posts. Merged, the deck stays
        // low and the middle above it open, so the deck can be walked on.
        let mut parts = vec![[0.0, 0.0, 0.0, 1.0, 0.25, 1.0]];
        for i in 0..8 {
            let x = i as f64 / 8.0;
            parts.push([x, 0.25, 0.0, x + 0.0625, 1.0, 0.125]);
            parts.push([x, 0.25, 0.875, x + 0.0625, 1.0, 1.0]);
        }
        let merged = merge_parts(&parts);
        assert!(merged.len() <= 16, "{merged:?}");
        let open = |x: f64, y: f64, z: f64| {
            !merged
                .iter()
                .any(|b| b[0] <= x && x < b[3] && b[1] <= y && y < b[4] && b[2] <= z && z < b[5])
        };
        assert!(!open(0.5, 0.1, 0.5), "the deck");
        assert!(open(0.5, 0.5, 0.5), "above the deck: {merged:?}");
        assert!(!open(0.03, 0.5, 0.06), "a post");
        // A pane among solid parts becomes a thin wall.
        assert_eq!(
            thicken([0.0, 0.0, 0.5, 1.0, 1.0, 0.5]),
            [0.0, 0.0, 0.46875, 1.0, 1.0, 0.53125]
        );
        // Few parts stay as they are.
        assert_eq!(merge_parts(&parts[..3]), parts[..3].to_vec());
    }

    #[test]
    fn shapes_follow_the_models() {
        // A panel against the north side, turned as a blockstate turns it.
        let north = [0.0, 0.0, 0.0, 1.0, 1.0, 0.125];
        assert_eq!(rotate(north, 0, 90), [0.875, 0.0, 0.0, 1.0, 1.0, 1.0]);
        assert_eq!(rotate(north, 0, 180), [0.0, 0.0, 0.875, 1.0, 1.0, 1.0]);
        // A slab on the floor, turned up toward north, stands against the south side.
        assert_eq!(
            rotate([0.0, 0.0, 0.0, 1.0, 0.5, 1.0], 90, 0),
            [0.0, 0.0, 0.5, 1.0, 1.0, 1.0]
        );
        // A curtain model on a full-cube template: a shape for each facing.
        let mut content = Content::default();
        content.files.insert(
            "assets/m/models/block/curtain.json".into(),
            r#"{"elements":[{"from":[0,0,0],"to":[16,16,2]}]}"#.into(),
        );
        let blockstate = serde_json::json!({ "variants": {
            "facing=north": { "model": "m:block/curtain" },
            "facing=east": { "model": "m:block/curtain", "y": 90 },
            "facing=up": { "model": "m:block/curtain" },
        }});
        let schema = BTreeMap::from([(
            "facing".to_string(),
            ["north", "east"].map(String::from).into(),
        )]);
        let (outline, collision) = model_shapes(&content, &blockstate, &schema).unwrap();
        assert_eq!(outline["properties"], serde_json::json!(["facing"]));
        assert_eq!(
            outline["values"]["east"],
            serde_json::json!([[0.875, 0.0, 0.0, 1.0, 1.0, 1.0]])
        );
        // `up` isn't one of the block's values: left out.
        assert!(outline["values"].get("up").is_none());
        assert!(collision.is_none());
        // A cross plant (vanilla's planes): outlined, walked through.
        let plant = serde_json::json!({ "variants": { "": { "model": "minecraft:block/cross" } } });
        let (outline, collision) = model_shapes(&content, &plant, &BTreeMap::new()).unwrap();
        assert_eq!(outline["default"][0][1], 0.0);
        assert_eq!(collision.unwrap(), serde_json::json!({ "default": [] }));
        // A whole block needs no shape.
        let cube = serde_json::json!({ "variants": { "": { "model": "minecraft:block/stone" } } });
        assert!(model_shapes(&content, &cube, &BTreeMap::new()).is_none());
    }

    #[test]
    fn fills_the_variants_a_template_stores() {
        // Macaw's window draws north and east only; a carved pumpkin stores four ways.
        let window = serde_json::json!({ "variants": {
            "facing=north,part=base": { "model": "w:single" },
            "facing=east,part=base": { "model": "w:single", "y": 90 },
        }});
        let pumpkin = &vanilla_block("minecraft:carved_pumpkin")
            .unwrap()
            .properties;
        let filled = fill_variants(&window, pumpkin).unwrap();
        let v = &filled["variants"];
        assert_eq!(v["facing=south,part=base"], v["facing=north,part=base"]);
        assert_eq!(v["facing=west,part=base"]["y"], 90);
        // A file that draws every value is left as it is.
        let full = fill_variants(&filled, pumpkin);
        assert!(full.is_none());
        // One drawn by multipart, or not by `facing`, too.
        assert!(fill_variants(&serde_json::json!({ "variants": { "": {} } }), pumpkin).is_none());
    }

    #[test]
    fn knows_the_games_blocks() {
        let door = vanilla_block("minecraft:oak_door").unwrap();
        assert!(door.item);
        assert!(door.properties.contains_key("hinge"));
        assert_eq!(door.properties["half"], ["lower", "upper"]);
        // Horizon refuses a block item copying a block without one.
        assert!(!vanilla_block("minecraft:weeping_vines_plant").unwrap().item);
        assert!(!vanilla_block("minecraft:potted_poppy").unwrap().item);
        // A potted plant is a flower pot, whatever plant it holds.
        assert_eq!(
            name_template("potted_fir_sapling"),
            Some("minecraft:flower_pot")
        );
        assert!(vanilla_block("minecraft:wool").is_none());
        // A world won't open with a block copying a growing plant.
        for vines in [
            "minecraft:weeping_vines_plant",
            "minecraft:kelp",
            "minecraft:twisting_vines",
        ] {
            let stand_in = growing_plant_stand_in(vines).unwrap();
            assert!(vanilla_block(stand_in).unwrap().item, "{stand_in}");
        }
        assert!(growing_plant_stand_in("minecraft:vine").is_none());
        assert_eq!(
            template_properties("minecraft:stone").unwrap(),
            Vec::<&str>::new()
        );
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
        // Macaw's grass roof copies dirt but draws as stairs: dirt can't place it.
        let roof = states(&[
            ("facing", &["north", "east"]),
            ("half", &["top", "bottom"]),
            ("shape", &["straight", "outer_left"]),
        ]);
        assert!(!has_states_of("minecraft:dirt", &roof));
        assert!(has_states_of("minecraft:oak_stairs", &roof));
        assert!(has_states_of("minecraft:carved_pumpkin", &roof));
        let door = states(&[
            ("half", &["lower", "upper"]),
            ("hinge", &["left", "right"]),
            ("open", &["true", "false"]),
        ]);
        assert_eq!(
            template_for_states(&door, "fir_door").unwrap().0,
            "minecraft:oak_door"
        );
        // One block tall: a door would place a second half above it.
        let shutter = states(&[("hinge", &["left", "right"]), ("open", &["true", "false"])]);
        assert_eq!(
            template_for_states(&shutter, "oak_shutter").unwrap().0,
            "minecraft:oak_trapdoor"
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
        // A template named by the Java code: the game's facts say it has none of them.
        let (declared, _) = declared_states("minecraft:gold_ore", &p, "ore", &mut skipped);
        assert_eq!(declared.keys().collect::<Vec<_>>(), ["axis", "powered"]);
        assert!(skipped.is_empty());

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
