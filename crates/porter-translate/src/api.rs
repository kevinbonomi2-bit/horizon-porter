//! The port as a trait mod on Horizon's mod API (`mod_api`, the arismoko/horizon fork:
//! Parts, a prebuilt game, ~1 s builds), beside the horizon-mixins bundle: `api/` holds
//! `horizon-mod.toml` (the content mod's, with `[native]` pins), `Cargo.toml`, the
//! content's `assets/` and `data/`, `src/runtime.rs` (a copy of the bundle's, written
//! with it, so the translated code is the same) and `src/lib.rs`, the glue written here:
//! a `BlockPart` and an `ItemPart` that turn the API's events into runtime.rs's inputs
//! and its `World` into the host's level writes.
//!
//! The glue covers the behaviours runtime.rs dispatches by kind (placement, clicks,
//! shape updates, ticks, survival, comparators, item use, tooltips) and the block
//! entities (their state, ticks with their `Machine`, slots and what clients get of
//! them). Renderers, screens, shift-clicks and the mod's own hooks are the bundle's alone
//! for now.

use std::collections::BTreeMap;
use std::fmt::Write;

use crate::blocks::{Behaviour, Kind};

/// The prebuilt game the glue was checked against (`prebuilt-<commit>` on
/// arismoko/horizon): the API changes daily, so a port pins one.
pub const API_HORIZON: &str = "62c91f44bfd1f0a40c236083702e456305796f3a";
/// The horizon-mixins commit that prebuilt's launcher pins.
pub const API_MIXINS: &str = "5a64af938ec86b00d53d16a408aabc0038fb3e57";

/// `api/`'s generated files (runtime.rs aside: `bundle::write` copies the bundle's).
/// `content_manifest` is the content mod's `horizon-mod.toml`; `entities`, whether
/// runtime.rs has block entities (its `World` needs `BlockEntities` then); `machines`,
/// whether it has machines (`MACHINES`).
#[allow(clippy::too_many_arguments)]
pub fn files(
    id: &str,
    content_manifest: &str,
    behaviours: &[Behaviour],
    entities: bool,
    machines: bool,
    renderers: bool,
    plants: bool,
    particles: &[String],
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert(
        "api/horizon-mod.toml".into(),
        format!(
            "{}\n# The trait mod's build: the prebuilt game it compiles against.\n[native]\ngame = \"26.3\"\nhorizon = \"{API_HORIZON}\"\nmixins = \"{API_MIXINS}\"\n",
            content_manifest.trim_end()
        ),
    );
    out.insert(
        "api/Cargo.toml".into(),
        format!(
            "[package]\nname = \"mod-{id}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n# The launcher points this at the Horizon source it builds the game from.\nhorizon-mod-api = {{ git = \"https://github.com/arismoko/horizon\", rev = \"{API_HORIZON}\" }}\n"
        ),
    );
    out.insert(
        "api/src/lib.rs".into(),
        lib_rs(behaviours, entities, machines, renderers, plants, particles),
    );
    out
}

/// Whether a mod has anything for the glue to run: a behaviour of a kind it covers, or
/// block entities.
pub fn wanted(behaviours: &[Behaviour], entities: bool) -> bool {
    entities || behaviours.iter().any(|b| covered(b.kind))
}

fn covered(kind: Kind) -> bool {
    !matches!(kind, Kind::Placed | Kind::Destroyed)
}

fn lib_rs(
    behaviours: &[Behaviour],
    entities: bool,
    machines: bool,
    renderers: bool,
    plants: bool,
    particles: &[String],
) -> String {
    let has = |k: Kind| behaviours.iter().any(|b| b.kind == k);
    let block_kinds = [
        Kind::Place,
        Kind::Use,
        Kind::Shape,
        Kind::RandomTick,
        Kind::Tick,
        Kind::Survive,
        Kind::Analog,
        Kind::Neighbour,
    ];
    let item_kinds = [Kind::ItemUseOn, Kind::ItemUse, Kind::Tooltip];
    let mut blocks: Vec<&str> = Vec::new();
    let mut items: Vec<&str> = Vec::new();
    for b in behaviours {
        let list = if block_kinds.contains(&b.kind) {
            &mut blocks
        } else if item_kinds.contains(&b.kind) {
            &mut items
        } else {
            continue;
        };
        list.extend(b.blocks.iter().map(String::as_str));
    }
    blocks.sort_unstable();
    blocks.dedup();
    items.sort_unstable();
    items.dedup();

    let mut r = String::from(HEADER);
    let list =
        |names: &[&str]| -> String { names.iter().map(|n| format!("    {n:?},\n")).collect() };
    let _ = write!(
        r,
        "\n/// The mod's blocks whose Java has code the port runs.\nconst BLOCKS: &[&str] = &[\n{}];\n\n/// The mod's items whose Java has code the port runs.\nconst ITEMS: &[&str] = &[\n{}];\n",
        list(&blocks),
        list(&items)
    );
    r.push_str(COMMON);
    if entities {
        r.push_str(
            &ENTITIES
                .replace(
                    "__FROM_TAG__",
                    &crate::block_entity::FROM_TAG.replace("m::", "runtime::"),
                )
                .replace(
                    "__TO_TAG__",
                    &crate::block_entity::TO_TAG.replace("m::", "runtime::"),
                )
                .replace(
                    "__MACHINE_CONTAINER__",
                    if machines { MACHINE_CONTAINER } else { "" },
                )
                .replace("__SUBMIT__", if renderers { SUBMIT } else { "" })
                .replace("__SCREENS__", if machines { SCREENS } else { "" }),
        );
    }
    if has(Kind::Use) || has(Kind::ItemUseOn) || has(Kind::ItemUse) || plants {
        r.push_str(HERE);
        if entities {
            r.push_str(HERE_ENTITIES);
        }
    }
    if has(Kind::RandomTick) || has(Kind::Tick) || has(Kind::Neighbour) {
        r.push_str(LEVEL);
        if entities {
            r.push_str(&no_entities("Level<'_>"));
        }
    }

    r.push_str("\n/// The port's code for one of the mod's blocks.\nstruct PortedBlock;\n\n#[mod_api::horizon_part]\nimpl BlockPart for PortedBlock {\n");
    if has(Kind::Place) {
        r.push_str(PART_PLACE);
    }
    if has(Kind::Shape) {
        r.push_str(PART_SHAPE);
    }
    if has(Kind::Use) {
        r.push_str(PART_USE);
    }
    if has(Kind::RandomTick) {
        r.push_str(PART_RANDOM_TICK);
    }
    if has(Kind::Tick) || has(Kind::Neighbour) {
        r.push_str(&PART_TICK.replace(
            "__NEIGHBOUR__",
            if has(Kind::Neighbour) {
                NEIGHBOUR_IN_TICK
            } else {
                ""
            },
        ));
    }
    if has(Kind::Neighbour) {
        r.push_str(PART_NEIGHBOUR);
    }
    if has(Kind::Survive) {
        r.push_str(PART_SURVIVE);
    }
    if has(Kind::Analog) {
        r.push_str(PART_ANALOG);
    }
    r.push_str("}\n");
    if has(Kind::Neighbour) {
        r.push_str(NEIGHBOUR_PENDING);
    }
    if has(Kind::Use) {
        r.push_str(CLICK_FN);
    }

    r.push_str("\n/// The port's code for one of the mod's items.\nstruct PortedItem;\n\n#[mod_api::horizon_part]\nimpl ItemPart for PortedItem {\n");
    if has(Kind::ItemUseOn) || plants {
        r.push_str(USE_ON_HEAD);
        if plants {
            r.push_str(USE_ON_PLANT);
        }
        r.push_str(if has(Kind::ItemUseOn) {
            PART_USE_ON
        } else {
            "        Flow::Continue\n    }\n\n"
        });
    }
    if has(Kind::ItemUse) {
        r.push_str(PART_USE_ITEM);
    }
    if has(Kind::Tooltip) {
        r.push_str(PART_TOOLTIP);
    }
    r.push_str("}\n");
    if has(Kind::ItemUseOn) || has(Kind::ItemUse) {
        r.push_str(ITEM_USE_FN);
    }
    if plants {
        r.push_str(PLANT_FN);
    }
    r.push_str(&register(entities, plants, particles));
    r
}

/// The mod's `Hooks`: its parts attached at registration, its block entity types
/// declared with their blocks, and their containers.
fn register(entities: bool, plants: bool, particles: &[String]) -> String {
    let mut r = String::from(
        "
#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn register(&self, registrar: &mut dyn Registrar, _context: &RegistrationContext) {
        for block in BLOCKS {
            let key = registrar.block_key(block);
            registrar.block_part(key, PortedBlock);
        }
        for item in ITEMS {
            let key = registrar.item_key(item);
            registrar.item_part(key, PortedItem);
        }
",
    );
    if plants {
        r.push_str(
            "        // Items that plant a block of another name (seeds).
        for (item, _, _) in runtime::PLACES {
            if !ITEMS.contains(item) {
                let key = registrar.item_key(item);
                registrar.item_part(key, PortedItem);
            }
        }
",
        );
    }
    {
        // The particles its code may send: vanilla's plain ones and the mod's own.
        let mut names: Vec<String> = VANILLA_PARTICLES
            .iter()
            .map(|p| format!("minecraft:{p}"))
            .collect();
        names.extend(particles.iter().cloned());
        let _ = write!(
            r,
            "        let mut keys = Vec::new();
        for name in [{}] {{
            keys.push((name.to_string(), registrar.particle_key(name)));
        }}
        let _ = PARTICLES.set(keys);
",
            names
                .iter()
                .map(|n| format!("{n:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if entities {
        r.push_str(
            "        // Each block entity type, declared with the blocks that have it.
        let mut types: Vec<(&str, Vec<&str>)> = Vec::new();
        for (block, id, _) in runtime::BLOCK_ENTITIES {
            match types.iter_mut().find(|(t, _)| t == id) {
                Some((_, blocks)) => blocks.push(block),
                None => types.push((id, vec![block])),
            }
        }
        for (id, blocks) in types {
            let name = id.split_once(':').map_or(id, |(_, n)| n);
            let kind = registrar.block_entity_type(name).blocks(&blocks).build();
            let _ = registrar.block_entity_part(&kind, PortedEntity);
        }
",
        );
    }
    r.push_str(
        "    }
",
    );
    if entities {
        r.push_str(
            "
    fn block_entity_container(
        &self,
        _host: &dyn mod_api::Host,
        event: &mut mod_api::BlockEntityContainerEvent<'_>,
    ) -> Option<mod_api::ModContainer> {
        container_of(event.block)
    }
",
        );
    }
    r.push_str(
        "}
",
    );
    r
}

/// An impl of runtime.rs's `BlockEntities` for `ty`: the API glue doesn't reach block
/// entities yet, so the code that reads them finds none.
fn no_entities(ty: &str) -> String {
    format!(
        r#"
// Block entities aren't wired in the API glue yet: the code that reads one finds none.
impl runtime::BlockEntities for {ty} {{
    fn entity(&mut self, pos: [i32; 3]) -> Option<runtime::EntityValues> {{
        let _ = pos;
        None
    }}
    fn save_entity(&mut self, pos: [i32; 3], values: &[(&str, runtime::Saved)]) -> bool {{
        let _ = (pos, values);
        false
    }}
    fn entity_slots(&mut self, pos: [i32; 3]) -> usize {{
        let _ = pos;
        0
    }}
    fn entity_item(&mut self, pos: [i32; 3], slot: usize) -> Option<(String, u32)> {{
        let _ = (pos, slot);
        None
    }}
    fn set_entity_item(&mut self, pos: [i32; 3], slot: usize, item: Option<(&str, u32)>) -> bool {{
        let _ = (pos, slot, item);
        false
    }}
    fn recipes(&mut self, kind: &str) -> Vec<runtime::Recipe> {{
        let _ = kind;
        Vec::new()
    }}
}}
"#
    )
}

const HEADER: &str = r#"//! The mod on Horizon's mod API, written by horizon-porter. `runtime.rs` is the port's
//! module (its functions translated from the Java, the same file as the mixin bundle's);
//! this file is the glue: Parts that turn the API's events into runtime.rs's inputs, and
//! its `World` into the host's level writes. Regenerated by `horizon-porter port`.
// Helpers a mod's kinds leave unused stay: the glue is the same for every mod.
#![allow(dead_code, clippy::all)]

#[allow(dead_code, unused_variables, unused_imports, clippy::all)]
mod runtime;

#[allow(unused_imports)]
use mod_api::{
    self, BlockPart, BlockState, Direction, Flow, Hooks, ItemPart, LevelHost, Registrar,
    RegistrationContext, ServerHost,
};
// Names of the pinned prebuilt game's API (they lose the `Native` prefix in later ones).
#[allow(unused_imports)]
use mod_api::{
    NativeBlockClick as BlockClick, NativeBlockUpdateFlags as BlockUpdateFlags,
    NativeInteractionResult as InteractionResult,
};
"#;

const COMMON: &str = r#"
/// A state's block and properties, from its full name:
/// `mymod:oak_gate[facing=north,open=false]`.
fn parse_state(text: &str) -> (String, Vec<(String, String)>) {
    let Some((block, rest)) = text.split_once('[') else {
        return (text.to_string(), Vec::new());
    };
    let props = rest
        .trim_end_matches(']')
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    (block.to_string(), props)
}

/// A state's full name, from its block and properties.
fn state_text(block: &str, props: &[(String, String)]) -> String {
    if props.is_empty() {
        return block.to_string();
    }
    let list: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{block}[{}]", list.join(","))
}

/// A state's block and properties as the Java names them (`facing` turned back).
fn java_properties(level: &dyn LevelHost, state: BlockState) -> (String, Vec<(String, String)>) {
    let (block, props) = parse_state(&level.state_string(state).unwrap_or_default());
    let props = props
        .into_iter()
        .map(|(k, v)| {
            let v = if k == "facing" { runtime::java_facing(&block, &v) } else { v };
            (k, v)
        })
        .collect();
    (block, props)
}

/// `state` with the properties the port's code set (`facing` as Java names it).
fn apply(level: &dyn LevelHost, state: BlockState, set: &[(String, String)]) -> BlockState {
    let block = level.state_block(state).unwrap_or_default().to_string();
    let mut out = state;
    for (key, value) in set {
        let value = if key == "facing" { runtime::stored_facing(&block, value) } else { value.clone() };
        if let Some(next) = level.state_with_property(out, key, &value) {
            out = next;
        }
    }
    out
}

fn direction_name(d: Direction) -> String {
    format!("{d:?}").to_lowercase()
}

/// Java's `setBlock` flags as the API's.
fn flags(bits: i32) -> BlockUpdateFlags {
    let mut f = BlockUpdateFlags::NONE;
    for (bit, flag) in [
        (1, BlockUpdateFlags::NEIGHBORS),
        (2, BlockUpdateFlags::CLIENTS),
        (4, BlockUpdateFlags::INVISIBLE),
        (16, BlockUpdateFlags::KNOWN_SHAPE),
        (32, BlockUpdateFlags::SUPPRESS_DROPS),
        (64, BlockUpdateFlags::MOVE_BY_PISTON),
        (128, BlockUpdateFlags::SKIP_SHAPE_UPDATE_ON_WIRE),
        (256, BlockUpdateFlags::SKIP_BLOCK_ENTITY_SIDE_EFFECTS),
        (512, BlockUpdateFlags::SKIP_ON_PLACE),
    ] {
        if bits & bit != 0 {
            f = f | flag;
        }
    }
    f
}

/// The dimension's id, as the host names levels.
fn dimension_id(d: mod_api::LevelDimension) -> &'static str {
    match d {
        mod_api::LevelDimension::Nether => "minecraft:the_nether",
        mod_api::LevelDimension::End => "minecraft:the_end",
        _ => "minecraft:overworld",
    }
}

/// xorshift64*, for code reached where the level's own random isn't (a click).
fn next_random(seed: &mut u64) -> f32 {
    let mut x = *seed;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *seed = x;
    ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
}

fn seed_of(pos: [i32; 3]) -> u64 {
    ((pos[0] as u64).wrapping_mul(73_856_093)
        ^ (pos[1] as u64).wrapping_mul(19_349_663)
        ^ (pos[2] as u64).wrapping_mul(83_492_791))
        | 1
}

/// Item ids by key, as a host named them: what clients get of a block entity is written
/// where no host is at hand.
static ITEM_IDS: std::sync::Mutex<Vec<(mod_api::ItemKey, String)>> = std::sync::Mutex::new(Vec::new());

/// A stack's item and count.
fn stack_of(host: &dyn mod_api::Host, stack: &mod_api::ItemStack) -> Option<(String, u32)> {
    let key = stack.item();
    let id = key.id(host);
    remember_item(key, &id);
    Some((id, stack.count()))
}

fn remember_item(key: mod_api::ItemKey, id: &str) {
    if let Ok(mut ids) = ITEM_IDS.lock() {
        if !ids.iter().any(|(k, _)| *k == key) {
            ids.push((key, id.to_string()));
        }
    }
}

/// A stack's item and count, by an id a host named before.
fn stack_known(stack: &mod_api::ItemStack) -> Option<(String, u32)> {
    let key = stack.item();
    let known = ITEM_IDS.lock().ok().and_then(|ids| ids.iter().find(|(k, _)| *k == key).map(|(_, id)| id.clone()));
    // An item no host named yet (a chunk sent as the world loads): the API's own table,
    // a game-side helper.
    let id = known.or_else(|| mod_api::__private::item_key_id(key).map(str::to_string))?;
    Some((id, stack.count()))
}

/// The particle keys the mod may send, requested at registration: vanilla's plain ones
/// and the mod's own.
static PARTICLES: std::sync::OnceLock<Vec<(String, mod_api::ParticleKey)>> = std::sync::OnceLock::new();

fn particle(kind: &str) -> Option<mod_api::ParticleKey> {
    let name = if kind.contains(':') { kind.to_string() } else { format!("minecraft:{kind}") };
    PARTICLES.get()?.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

/// A stack of `count` `item`, as the game makes one.
fn item_stack(host: &dyn mod_api::Host, item: &str, count: u32) -> Option<mod_api::ItemStack> {
    let key = host.item_key(&mod_api::Identifier::parse(item).ok()?)?;
    // What the block entity's clients are sent names it by this id.
    remember_item(key, item);
    mod_api::ItemStack::new(key, count)
}

/// An item's stack size and what it leaves in a crafting grid.
fn item_facts(host: &dyn mod_api::Host, item: &str) -> Option<(u32, Option<String>)> {
    let rest = runtime::REMAINDERS
        .iter()
        .find(|(i, _)| *i == item)
        .map(|(_, r)| r.to_string());
    let max = mod_api::Identifier::parse(item)
        .ok()
        .and_then(|id| host.item_key(&id))
        .and_then(|key| host.max_stack_size(key))
        .unwrap_or(64);
    Some((max, rest))
}

/// Whether `id` (a block or item, by `kind`) is in tag `tag`.
fn in_tag(host: &dyn mod_api::Host, kind: &str, tag: &str, id: &str) -> bool {
    let tag = tag.trim_start_matches('#');
    if kind == "block" {
        return host
            .block_state_id(id)
            .and_then(|s| host.block_in_tag(s.block(), tag))
            .unwrap_or(false);
    }
    let (Ok(item), Ok(tag)) = (mod_api::Identifier::parse(id), mod_api::Identifier::parse(tag)) else {
        return false;
    };
    let tags = host.tags();
    match (host.item_key(&item), tags.key(&tag)) {
        (Some(item), Some(tag)) => tags.is_in_tag(item, tag).unwrap_or(false),
        _ => false,
    }
}

/// The level, as the port's code reads it.
struct Read<'a> {
    level: &'a dyn LevelHost,
}

impl runtime::Blocks for Read<'_> {
    fn block(&self, pos: [i32; 3]) -> String {
        self.level
            .block_state(pos)
            .and_then(|s| self.level.state_block(s).map(str::to_string))
            .unwrap_or_else(|| "minecraft:air".into())
    }
    fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
        let state = self.level.block_state(pos)?;
        let (_, props) = java_properties(self.level, state);
        props.into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn in_tag(&self, pos: [i32; 3], tag: &str) -> bool {
        let tag = tag.trim_start_matches('#');
        self.level
            .block_state(pos)
            .and_then(|s| self.level.block_in_tag(s.block(), tag))
            .unwrap_or_else(|| runtime::block_in_tag(&runtime::Blocks::block(self, pos), tag))
    }
    fn light(&self, pos: [i32; 3]) -> u8 {
        self.level.raw_brightness(pos, 0).clamp(0, 15) as u8
    }
}
"#;

const HERE: &str = r#"
/// The world as a click's or an item's code changes it: its level, through the server
/// host. States go by their full names, as `/setblock` reads them.
struct Here<'h> {
    host: &'h mut dyn ServerHost,
    level: Option<mod_api::LevelRef>,
    dimension: &'static str,
    /// The player who clicked or used the item, and the hand they used.
    player: Option<mod_api::PlayerRef>,
    hand: mod_api::Hand,
    seed: u64,
}

impl Here<'_> {
    fn state(&self, pos: [i32; 3]) -> Option<(String, Vec<(String, String)>)> {
        let state = self.level?.block_state(&*self.host, pos)?;
        Some(parse_state(&self.host.block_state_string(state)?))
    }

    fn write(&mut self, pos: [i32; 3], block: &str, props: &[(String, String)], bits: i32) -> bool {
        let text = state_text(block, props);
        let (Some(level), Some(state)) = (self.level, self.host.block_state_id(&text)) else {
            return false;
        };
        level.set_block(self.host, pos, state, flags(bits)).unwrap_or(false)
    }
}

impl runtime::Blocks for Here<'_> {
    fn block(&self, pos: [i32; 3]) -> String {
        self.state(pos).map_or_else(|| "minecraft:air".into(), |(b, _)| b)
    }
    fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
        let (block, props) = self.state(pos)?;
        let v = props.into_iter().find(|(k, _)| k == key)?.1;
        Some(if key == "facing" { runtime::java_facing(&block, &v) } else { v })
    }
    fn in_tag(&self, pos: [i32; 3], tag: &str) -> bool {
        in_tag(&*self.host, "block", tag, &runtime::Blocks::block(self, pos))
    }
    fn light(&self, pos: [i32; 3]) -> u8 {
        let Some(level) = self.level else {
            return 15;
        };
        let of = |layer| level.brightness(&*self.host, layer, pos).unwrap_or(0);
        of(mod_api::LightLayer::Sky).max(of(mod_api::LightLayer::Block)).clamp(0, 15) as u8
    }
}

impl runtime::World for Here<'_> {
    fn set_property_flags(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {
        let Some((block, mut props)) = self.state(pos) else {
            return false;
        };
        let value = if key == "facing" { runtime::stored_facing(&block, value) } else { value.to_string() };
        let Some(slot) = props.iter_mut().find(|(k, _)| k == key) else {
            return false;
        };
        if slot.1 == value {
            return false;
        }
        slot.1 = value;
        self.write(pos, &block, &props, flags)
    }
    fn set_block_flags(&mut self, pos: [i32; 3], block: &str, properties: &[(&str, &str)], flags: i32) -> bool {
        let props: Vec<(String, String)> = properties
            .iter()
            .map(|(k, v)| {
                let v = if *k == "facing" { runtime::stored_facing(block, v) } else { v.to_string() };
                (k.to_string(), v)
            })
            .collect();
        self.write(pos, block, &props, flags)
    }
    fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32) {
        if let Some(level) = self.level {
            let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]) + 0.5, f64::from(pos[2]) + 0.5];
            let _ = level.play_sound(self.host, sound, at, mod_api::SoundSource::Blocks, volume, pitch);
        }
    }
    fn random(&mut self) -> f32 {
        next_random(&mut self.seed)
    }
    fn give(&mut self, item: &str, count: u32) -> u32 {
        use mod_api::HostTransaction;
        let (Some(player), Some(stack)) = (self.player, item_stack(&*self.host, item, count)) else {
            return 0;
        };
        let Some(items) = player.entity().capability(self.host, &mod_api::ENTITY_ITEMS, ()) else {
            return 0;
        };
        let resource = mod_api::ItemResource::of(&stack);
        self.host
            .transaction(|host, tx| Ok::<u32, ()>(items.insert_any(host, tx, &resource, count)))
            .unwrap_or(0)
    }
    fn drop(&mut self, item: &str, count: u32, at: [f64; 3], motion: [f64; 3]) {
        use mod_api::Tag;
        if count == 0 || item == "minecraft:air" {
            return;
        }
        let mut stack = mod_api::Compound::new();
        stack.insert("id".into(), Tag::String(item.into()));
        stack.insert("count".into(), Tag::Int(count.min(99) as i32));
        let mut nbt = mod_api::Compound::new();
        nbt.insert("Item".into(), Tag::Compound(stack));
        // `ItemEntity.setDefaultPickUpDelay`: it pops out before anyone can take it.
        nbt.insert("PickupDelay".into(), Tag::Short(10));
        nbt.insert(
            "Motion".into(),
            Tag::List(motion.iter().map(|v| Tag::Double(v.to_bits())).collect()),
        );
        let _ = self.host.summon(self.dimension, "minecraft:item", at, nbt);
    }
    // A player's hands are a living entity's; `LivingRef::new` is the API's only way
    // from a player to its living handle at the pinned prebuilt.
    fn set_held(&mut self, item: &str, count: u32) {
        let Some(player) = self.player else {
            return;
        };
        let stack = if count == 0 || item == "minecraft:air" {
            None
        } else {
            item_stack(&*self.host, item, count)
        };
        let _ = mod_api::LivingRef::new(player.entity()).set_hand_item(self.host, self.hand, stack);
    }
    fn shrink_held(&mut self, n: u32) {
        let Some(player) = self.player else {
            return;
        };
        let living = mod_api::LivingRef::new(player.entity());
        let Some(mut stack) = living.hand_item(&*self.host, self.hand) else {
            return;
        };
        let left = stack.count().saturating_sub(n);
        let next = if left == 0 {
            None
        } else {
            let _ = stack.set_count(left);
            Some(stack)
        };
        let _ = living.set_hand_item(self.host, self.hand, next);
    }
    fn hurt_held(&mut self, n: u32) -> bool {
        let Some(player) = self.player else {
            return false;
        };
        let slot = match self.hand {
            mod_api::Hand::Off => mod_api::EquipmentSlot::OffHand,
            _ => mod_api::EquipmentSlot::MainHand,
        };
        let living = mod_api::LivingRef::new(player.entity());
        let _ = living.hurt_and_break(self.host, slot, n as i32);
        living.hand_item(&*self.host, self.hand).is_none()
    }
    fn particles(&mut self, kind: &str, at: [f64; 3], spread: [f64; 3], speed: f64, count: i32) {
        if let (Some(level), Some(key)) = (self.level, particle(kind)) {
            let options = mod_api::ParticleOptions::simple(key);
            let _ = level.send_particles(self.host, &options, None, at, spread, speed, count);
        }
    }
    fn block_is(&mut self, pos: [i32; 3], tag: &str) -> bool {
        let block = runtime::Blocks::block(self, pos);
        in_tag(&*self.host, "block", tag, &block)
    }
    fn item_is(&mut self, item: &str, tag: &str) -> bool {
        in_tag(&*self.host, "item", tag, item)
    }
    fn remainder(&mut self, item: &str) -> Option<String> {
        item_facts(&*self.host, item).and_then(|(_, r)| r)
    }
    fn max_stack(&mut self, item: &str) -> u32 {
        item_facts(&*self.host, item).map_or(64, |(n, _)| n)
    }
    fn powered(&mut self, pos: [i32; 3]) -> bool {
        self.level.is_some_and(|l| l.has_neighbor_signal(&*self.host, pos))
    }
    fn xp(&mut self, at: [f64; 3], amount: u32) {
        if let Some(level) = self.level {
            let _ = level.award_experience(self.host, at[0], at[1], at[2], amount);
        }
    }
}
"#;

const LEVEL: &str = r#"
/// The server level as a tick's code changes it: blocks, sounds, its random, scheduled
/// ticks and tags.
struct Level<'a> {
    level: &'a mut dyn mod_api::LevelWriteHost,
}

impl runtime::Blocks for Level<'_> {
    fn block(&self, pos: [i32; 3]) -> String {
        runtime::Blocks::block(&Read { level: &*self.level }, pos)
    }
    fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
        runtime::Blocks::property(&Read { level: &*self.level }, pos, key)
    }
    fn in_tag(&self, pos: [i32; 3], tag: &str) -> bool {
        runtime::Blocks::in_tag(&Read { level: &*self.level }, pos, tag)
    }
    fn light(&self, pos: [i32; 3]) -> u8 {
        runtime::Blocks::light(&Read { level: &*self.level }, pos)
    }
}

impl runtime::World for Level<'_> {
    fn set_property_flags(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {
        let value = if key == "facing" {
            let block = runtime::Blocks::block(self, pos);
            runtime::stored_facing(&block, value)
        } else {
            value.to_string()
        };
        self.level.set_block_property(pos, key, &value, flags)
    }
    fn set_block_flags(&mut self, pos: [i32; 3], block: &str, properties: &[(&str, &str)], bits: i32) -> bool {
        let props: Vec<(String, String)> = properties
            .iter()
            .map(|(k, v)| {
                let v = if *k == "facing" { runtime::stored_facing(block, v) } else { v.to_string() };
                (k.to_string(), v)
            })
            .collect();
        let Some(state) = self.level.block_state_id(&state_text(block, &props)) else {
            return false;
        };
        self.level.set_block(pos, state, flags(bits))
    }
    fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32) {
        let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]) + 0.5, f64::from(pos[2]) + 0.5];
        // SoundSource.BLOCKS.
        let _ = self.level.play_sound(sound, at, 4, volume, pitch);
    }
    fn random(&mut self) -> f32 {
        use mod_api::RandomSource;
        self.level.with_random(|r| r.next_float()).unwrap_or(0.5)
    }
    fn block_is(&mut self, pos: [i32; 3], tag: &str) -> bool {
        let Some(state) = self.level.block_state(pos) else {
            return false;
        };
        self.level.block_in_tag(state.block(), tag).unwrap_or(false)
    }
    fn schedule_tick(&mut self, pos: [i32; 3], block: &str, delay: i32) -> bool {
        match self.level.block_state_id(block) {
            Some(state) => self.level.schedule_block_tick(pos, state.block(), delay),
            None => false,
        }
    }
    fn powered(&mut self, pos: [i32; 3]) -> bool {
        self.level.has_neighbor_signal(pos)
    }
}
"#;

const PART_PLACE: &str = r#"    fn state_for_placement(
        &self,
        level: &dyn LevelHost,
        event: &mut mod_api::StateForPlacementEvent<'_>,
    ) -> Option<Option<BlockState>> {
        let block = level.state_block(event.state)?.to_string();
        if !runtime::places_its_own_way(&block) {
            return None;
        }
        let c = event.context;
        let p = runtime::Placement {
            block,
            pos: [c.pos.x, c.pos.y, c.pos.z],
            clicked: [c.hit_pos.x, c.hit_pos.y, c.hit_pos.z],
            face: direction_name(c.face),
            hit: c.hit,
            yaw: c.yaw,
            pitch: c.pitch,
            sneaking: c.sneaking,
            replace_clicked: c.replace_clicked,
            water: false,
            water_source: false,
        };
        let set = runtime::state_for_placement(&p, &Read { level })?;
        if set.iter().any(|(k, _)| k == runtime::CANCEL) {
            return Some(None);
        }
        Some(Some(apply(level, event.state, &set)))
    }

"#;

const PART_SHAPE: &str = r#"    fn update_shape(
        &self,
        level: &mut dyn mod_api::LevelTicksHost,
        event: &mut mod_api::UpdateShapeEvent,
    ) -> Option<BlockState> {
        let reader: &dyn LevelHost = &*level;
        let (block, props) = java_properties(reader, event.state);
        if !runtime::shapes_by_neighbours(&block) {
            return None;
        }
        let (neighbour, neighbour_props) = java_properties(reader, event.neighbour);
        let u = runtime::ShapeUpdate {
            block,
            pos: [event.pos.x, event.pos.y, event.pos.z],
            properties: props,
            direction: direction_name(event.direction),
            neighbour_pos: [event.neighbour_pos.x, event.neighbour_pos.y, event.neighbour_pos.z],
            neighbour,
            neighbour_properties: neighbour_props,
        };
        let set = runtime::update_shape(&u, &Read { level: reader })?;
        Some(apply(reader, event.state, &set))
    }

"#;

const PART_USE: &str = r#"    fn use_item_on(
        &self,
        host: &mut dyn ServerHost,
        event: &mut mod_api::UseItemOnEvent<'_>,
    ) -> Option<InteractionResult> {
        click(host, event.state, event.click)
    }

    fn use_without_item(
        &self,
        host: &mut dyn ServerHost,
        event: &mut mod_api::UseWithoutItemEvent<'_>,
    ) -> Option<InteractionResult> {
        click(host, event.state, event.click)
    }

"#;

const PART_RANDOM_TICK: &str = r#"    fn random_tick(&self, level: &mut dyn mod_api::LevelWriteHost, event: &mut mod_api::RandomTickEvent) -> Flow {
        let Some(block) = level.state_block(event.state).map(str::to_string) else {
            return Flow::Continue;
        };
        if !runtime::random_ticks_its_own_way(&block) {
            return Flow::Continue;
        }
        let t = runtime::BlockTick { block, pos: [event.pos.x, event.pos.y, event.pos.z] };
        if runtime::random_tick(&t, &mut Level { level }) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

"#;

const PART_TICK: &str = r#"    fn tick_block(&self, level: &mut dyn mod_api::LevelWriteHost, event: &mut mod_api::TickBlockEvent) -> Flow {
        let Some(block) = level.state_block(event.state).map(str::to_string) else {
            return Flow::Continue;
        };
__NEIGHBOUR__        if !runtime::ticks_its_own_way(&block) {
            return Flow::Continue;
        }
        let t = runtime::BlockTick { block, pos: [event.pos.x, event.pos.y, event.pos.z] };
        if runtime::scheduled_tick(&t, &mut Level { level }) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

"#;

const PART_SURVIVE: &str = r#"    fn can_survive(&self, level: &dyn LevelHost, event: &mut mod_api::CanSurviveEvent) -> Option<bool> {
        let block = level.state_block(event.state)?.to_string();
        if !runtime::decides_survival(&block) {
            return None;
        }
        let at = runtime::BlockAt { block, pos: [event.pos.x, event.pos.y, event.pos.z], face: None };
        runtime::can_survive(&at, &Read { level })
    }

"#;

const PART_ANALOG: &str = r#"    fn has_analog_output(&self, level: &dyn LevelHost, event: &mut mod_api::HasAnalogOutputEvent) {
        if level.state_block(event.state).is_some_and(runtime::has_comparator_output) {
            event.output = true;
        }
    }

    fn analog_output(&self, level: &dyn LevelHost, event: &mut mod_api::AnalogOutputEvent) {
        let Some(block) = level.state_block(event.state).map(str::to_string) else {
            return;
        };
        if !runtime::has_comparator_output(&block) {
            return;
        }
        let at = runtime::BlockAt {
            block,
            pos: [event.pos.x, event.pos.y, event.pos.z],
            face: Some(direction_name(event.face)),
        };
        if let Some(signal) = runtime::comparator_output(&at, &Read { level }) {
            event.output = signal.clamp(0, 15);
        }
    }

"#;

const CLICK_FN: &str = r#"
/// A right click on one of the mod's blocks, run by runtime.rs's `use_block`.
fn click(host: &mut dyn ServerHost, state: BlockState, click: &BlockClick<'_>) -> Option<InteractionResult> {
    let (block, _) = parse_state(&host.block_state_string(state)?);
    if !runtime::acts_on_use(&block) {
        return None;
    }
    let (held, held_count) = match click.stack {
        Some(stack) => (stack.item().id(&*host).to_string(), stack.count()),
        None => ("minecraft:air".to_string(), 0),
    };
    // Java's client runs the block's `useItemOn` too, and never sends the off hand's use
    // once the main hand's took the click. Horizon's client can't run it, so the off hand's
    // use of the same click arrives here after the main hand's: it answers as that did.
    if click.hand == mod_api::Hand::Off
        && MAIN_HAND_TOOK
            .lock()
            .ok()
            .and_then(|mut took| took.take())
            .is_some_and(|(player, pos)| player == click.player && pos == click.pos)
    {
        return Some(InteractionResult::Consume);
    }
    let player = host.entity(click.player_entity_id as u32);
    let yaw = player.and_then(|p| p.rotation(&*host)).map_or(0.0, |r| r[0]);
    let sneaking = player.and_then(|p| p.is_crouching(&*host)).unwrap_or(false);
    let creative = host
        .player(click.player)
        .and_then(|p| p.is_creative(&*host))
        .unwrap_or(false);
    let used = runtime::BlockUse {
        block,
        pos: click.pos,
        face: click.hit.face,
        hit: click.hit.point,
        held,
        held_count,
        facing: runtime::yaw_direction(yaw).to_string(),
        creative,
        sneaking,
    };
    let dimension = dimension_id(click.dimension);
    let level = host.level(dimension);
    let player = host.player(click.player);
    let mut here = Here { host, level, dimension, player, hand: click.hand, seed: seed_of(click.pos) };
    let took = runtime::use_block(&used, &mut here);
    if click.hand == mod_api::Hand::Main
        && let Ok(mut last) = MAIN_HAND_TOOK.lock()
    {
        *last = took.then_some((click.player, click.pos));
    }
    took.then_some(InteractionResult::Success)
}

/// The player and block whose click the main hand took last, until the off hand's use of
/// that click comes.
static MAIN_HAND_TOOK: std::sync::Mutex<Option<(u128, [i32; 3])>> = std::sync::Mutex::new(None);
"#;

const USE_ON_HEAD: &str = r#"    fn use_on(&self, host: &mut dyn ServerHost, event: &mut mod_api::UseOnEvent<'_>) -> Flow {
        let item = event.stack.item().id(&*host).to_string();
"#;

const USE_ON_PLANT: &str = r#"        if let Some((_, block, rule)) = runtime::PLACES.iter().find(|(i, _, _)| *i == item) {
            return if plant(host, event, block, rule) { Flow::Cancel } else { Flow::Continue };
        }
"#;

const PLANT_FN: &str = r#"
/// An item that plants a block of another name (seeds) used on a block: the block is
/// placed as Java's `BlockItem.place` does, where it can stand (`rule`: `farmland`,
/// `soil`, `water` or `any`), and one seed is used up.
fn plant(host: &mut dyn ServerHost, event: &mod_api::UseOnEvent<'_>, block: &str, rule: &str) -> bool {
    fn name(host: &dyn mod_api::Host, level: mod_api::LevelRef, pos: [i32; 3]) -> String {
        level
            .block_state(host, pos)
            .and_then(|s| host.block_state_string(s))
            .map_or_else(|| "minecraft:air".to_string(), |t| parse_state(&t).0)
    }
    let Some(player) = host.player(event.player) else {
        return false;
    };
    let Some(level) = player.entity().level(&*host) else {
        return false;
    };
    let pos = event.pos;
    let clicked = name(&*host, level, pos);
    // Grass and ferns are replaced; anything else gets the block on its clicked face.
    let replaced = matches!(clicked.as_str(), "minecraft:short_grass" | "minecraft:tall_grass" | "minecraft:fern");
    let face = event.face;
    let target = if replaced { pos } else { [pos[0] + face[0], pos[1] + face[1], pos[2] + face[2]] };
    let here = name(&*host, level, target);
    let below = name(&*host, level, [target[0], target[1] - 1, target[2]]);
    let room = here == "minecraft:air" || (rule == "water" && here == "minecraft:water");
    let stands = match rule {
        "farmland" => below == "minecraft:farmland" || below.ends_with("_farmland"),
        "soil" => matches!(
            below.as_str(),
            "minecraft:grass_block" | "minecraft:dirt" | "minecraft:coarse_dirt" | "minecraft:podzol"
                | "minecraft:rooted_dirt" | "minecraft:moss_block" | "minecraft:mud" | "minecraft:farmland"
        ) || below.ends_with("_farmland") || below.ends_with("soil"),
        "water" => here == "minecraft:water",
        _ => below != "minecraft:air",
    };
    // Where its Java decides whether it can stay (`canSurvive`), that decides.
    let stands = if runtime::decides_survival(block) {
        let at = runtime::BlockAt { block: block.to_string(), pos: target, face: None };
        let here = Here { host: &mut *host, level: Some(level), dimension: "", player: Some(player), hand: event.hand, seed: 0 };
        runtime::can_survive(&at, &here).unwrap_or(stands)
    } else {
        stands
    };
    if !(room && stands) {
        return false;
    }
    let Some(state) = host.block_state_id(block) else {
        return false;
    };
    if !level.set_block(host, target, state, flags(3)).unwrap_or(false) {
        return false;
    }
    if !player.is_creative(&*host).unwrap_or(false) {
        let living = mod_api::LivingRef::new(player.entity());
        if let Some(mut stack) = living.hand_item(&*host, event.hand) {
            let left = stack.count().saturating_sub(1);
            let next = if left == 0 {
                None
            } else {
                let _ = stack.set_count(left);
                Some(stack)
            };
            let _ = living.set_hand_item(host, event.hand, next);
        }
    }
    let at = [f64::from(target[0]) + 0.5, f64::from(target[1]) + 0.5, f64::from(target[2]) + 0.5];
    let _ = level.play_sound(host, "minecraft:item.crop.plant", at, mod_api::SoundSource::Blocks, 1.0, 1.0);
    true
}
"#;

const PART_USE_ON: &str = r#"        if !runtime::item_acts_on_block(&item) {
            return Flow::Continue;
        }
        let block = host
            .players()
            .acting_player()
            .and_then(|p| p.entity().level(&*host))
            .and_then(|l| l.block_state(&*host, event.pos))
            .and_then(|s| host.block_state_string(s))
            .map(|t| parse_state(&t).0)
            .unwrap_or_else(|| "minecraft:air".into());
        let on = runtime::UseOn { block, pos: event.pos, face: event.face, hit: event.hit.point };
        if item_use(host, event.stack, Some(on), event.player_entity_id, runtime::use_item_on) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

"#;

const PART_USE_ITEM: &str = r#"    fn use_item(&self, host: &mut dyn ServerHost, event: &mut mod_api::UseItemEvent<'_>) -> Flow {
        let item = event.stack.item().id(&*host).to_string();
        if !runtime::item_acts_in_air(&item) {
            return Flow::Continue;
        }
        let entity = host
            .players()
            .acting_player()
            .map_or(-1, |p| p.entity().id() as i32);
        if item_use(host, event.stack, None, entity, runtime::use_item) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

"#;

const PART_TOOLTIP: &str = r#"    fn tooltip(&self, host: &dyn mod_api::ClientHost, event: &mut mod_api::TooltipEvent<'_>) {
        let item = event.stack.item().id(host).to_string();
        if !runtime::adds_tooltip(&item) {
            return;
        }
        let of = runtime::TooltipFor {
            item,
            count: event.stack.count(),
            damage: 0,
            max_damage: 0,
        };
        // `0xRRGGBB` as a line's colour packs it; none keeps the text's own.
        let packed = |rgb: Option<u32>| match rgb {
            Some(c) => 0xFF00_0000 | (c & 0xFF) << 16 | (c >> 8 & 0xFF) << 8 | (c >> 16 & 0xFF),
            None => u32::MAX,
        };
        // Under the name, as Java's `appendHoverText` lines follow it.
        let mut at = 1.min(event.lines.len());
        for line in runtime::tooltip_lines(&of) {
            let (text, color) = match line {
                runtime::TooltipLine::Translated { key, args, color } => (
                    mod_api::Text::translate(key, args.into_iter().map(mod_api::Text::literal).collect()),
                    packed(color),
                ),
                runtime::TooltipLine::Text { text, color } => (mod_api::Text::literal(text), packed(color)),
            };
            event.lines.insert(at, (text, color));
            at += 1;
        }
    }

"#;

const ITEM_USE_FN: &str = r#"
/// An item used (on a block, or in the air), run by `run` (runtime.rs's
/// `use_item_on` or `use_item`). True when the port's code took the use.
fn item_use(
    host: &mut dyn ServerHost,
    stack: &mod_api::ItemStack,
    on: Option<runtime::UseOn>,
    entity: i32,
    run: fn(&runtime::ItemUse, &mut dyn runtime::World) -> bool,
) -> bool {
    let item = stack.item().id(&*host).to_string();
    let player = host.entity(entity as u32);
    let sneaking = player.and_then(|p| p.is_crouching(&*host)).unwrap_or(false);
    let acting = host.players().acting_player();
    let creative = acting.and_then(|p| p.is_creative(&*host)).unwrap_or(false);
    let dimension = host.dimension();
    let level = acting.and_then(|p| p.entity().level(&*host)).or_else(|| host.level(dimension));
    let pos = on.as_ref().map_or([0, 0, 0], |o| o.pos);
    let u = runtime::ItemUse {
        item,
        count: stack.count(),
        damage: 0,
        on,
        creative,
        sneaking,
    };
    let mut here = Here {
        host,
        level,
        dimension,
        player: acting,
        hand: mod_api::Hand::Main,
        seed: seed_of(pos),
    };
    run(&u, &mut here)
}
"#;

const ENTITIES: &str = r##"
// ---- Block entities: their state, ticks, items and what clients get of them ----

fn saved_of(tag: &mod_api::Tag) -> Option<runtime::Saved> {
    use mod_api::Tag;
    __FROM_TAG__
}

fn tag_of(value: runtime::Saved) -> mod_api::Tag {
    use mod_api::Tag;
    __TO_TAG__
}

/// The value saved under `key` in `nbt`.
fn compound_get<'a>(nbt: &'a mod_api::Compound, key: &str) -> Option<&'a mod_api::Tag> {
    nbt.iter().find(|(k, _)| k.to_string_lossy() == key).map(|(_, v)| v)
}

/// The block entity block `block` has (runtime.rs's `BLOCK_ENTITIES`).
fn entity_of(block: &str) -> Option<&'static str> {
    runtime::BLOCK_ENTITIES.iter().find(|(b, _, _)| *b == block).map(|(_, id, _)| *id)
}

/// A tool by what it does (`{"type": "item_ability", "action": "axe_strip"}`): the tag of
/// the tools that do it.
fn ability_tag(action: &str) -> Option<&'static str> {
    Some(match action.split('_').next()? {
        "axe" => "#minecraft:axes",
        "pickaxe" => "#minecraft:pickaxes",
        "shovel" => "#minecraft:shovels",
        "hoe" => "#minecraft:hoes",
        "sword" => "#minecraft:swords",
        "shears" => "#c:tools/shears",
        "brush" => "#c:tools/brush",
        _ => return None,
    })
}

/// The recipes of type `kind`, ingredients resolved to items: the mod's own types, and the
/// cooking table the porter wrote for vanilla's.
fn recipes(host: &dyn mod_api::Host, kind: &str) -> Vec<runtime::Recipe> {
    let items = |json: &str| -> Vec<String> {
        host.recipe_ingredient(json).ok().flatten().map(|i| i.order).unwrap_or_default()
    };
    if runtime::COOKING.iter().any(|(k, ..)| *k == kind) {
        return runtime::COOKING
            .iter()
            .filter(|(k, ..)| *k == kind)
            .map(|(_, input, result, count, ticks)| runtime::Recipe {
                id: format!("{kind}/{result}"),
                ingredients: vec![items(&format!("{input:?}"))],
                result: Some(runtime::Stack::new(result, *count)),
                values: vec![("cookingtime".into(), ticks.to_string())],
            })
            .collect();
    }
    host.recipes_of_type(kind)
        .iter()
        .map(|r| {
            let mut ingredients: Vec<Vec<String>> = r.ingredients.iter().map(|i| i.order.clone()).collect();
            // A mod's own recipe type, whose fields the game doesn't know: its ingredients
            // as the file writes them (`ingredient`, `ingredients`, `input`, `inputs`).
            if ingredients.is_empty() {
                for key in ["ingredient", "input"] {
                    if let Some(v) = r.json.get(key) {
                        ingredients.push(items(&v.to_string()));
                    }
                }
                for key in ["ingredients", "inputs"] {
                    if let Some(list) = r.json.get(key).and_then(|v| v.as_array()) {
                        ingredients.extend(list.iter().map(|v| items(&v.to_string())));
                    }
                }
            }
            // The tool or catalyst it takes besides (a cutting board's knife), last.
            for key in ["tool", "catalyst"] {
                if let Some(v) = r.json.get(key) {
                    let json = match v.get("action").and_then(|a| a.as_str()).and_then(ability_tag) {
                        Some(tag) => format!("{tag:?}"),
                        None => v.to_string(),
                    };
                    ingredients.push(items(&json));
                }
            }
            // Its result: the game's, else the file's (`"result": "id"`, `{"id": ..}`,
            // `{"item": ..}`, or a list of them: the first).
            let result = r.results.first().and_then(|s| stack_of(host, s)).map(|(i, c)| runtime::Stack::new(&i, c)).or_else(|| {
                let v = r.json.get("result")?;
                let v = v.as_array().and_then(|a| a.first()).unwrap_or(v);
                let json = match (v.as_str(), v.get("id"), v.get("item")) {
                    (Some(id), _, _) => format!("{{\"id\": {id:?}}}"),
                    // `{"item": {"id": .., "count": ..}}` (Farmer's Delight's cutting).
                    (None, None, Some(item)) if item.is_object() => item.to_string(),
                    (None, None, Some(item)) => {
                        let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(1);
                        format!("{{\"id\": {item}, \"count\": {count}}}")
                    }
                    _ => v.to_string(),
                };
                let stack = host.recipe_result(&json).ok().flatten()?;
                stack_of(host, &stack).map(|(i, c)| runtime::Stack::new(&i, c))
            });
            let mut values: Vec<(String, String)> = r
                .json
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| match v.as_str() {
                    Some(s) => Some((k.clone(), s.to_string())),
                    None if v.is_number() || v.is_boolean() => Some((k.clone(), v.to_string())),
                    None => None,
                })
                .collect();
            if let Some(t) = r.cooking_time {
                if !values.iter().any(|(k, _)| k == "cookingtime") {
                    values.push(("cookingtime".into(), t.to_string()));
                }
            }
            runtime::Recipe { id: r.id.clone(), ingredients, result, values }
        })
        .collect()
}

/// Block `block`'s item slots and the menu they show in: a chest-like block's, or a
/// machine's (as a furnace: hoppers put in from above and the sides, take from below).
fn container_of(block: &str) -> Option<mod_api::ModContainer> {
    if let Some((_, menu, slots)) = runtime::VANILLA_CONTAINERS.iter().find(|(b, _, _)| *b == block) {
        // One without a menu (a skillet) only holds its items: no side reaches them.
        let all: Vec<usize> = if menu.is_empty() { Vec::new() } else { (0..*slots).collect() };
        return Some(mod_api::ModContainer {
            slots: *slots,
            faces: std::array::from_fn(|_| all.clone()),
            inputs: all.clone(),
            outputs: all,
            menu: (!menu.is_empty()).then(|| (*menu).into()),
        });
    }
__MACHINE_CONTAINER__    None
}

/// The port's code for one of the mod's block entities.
struct PortedEntity;

// A block entity's part is a block part too.
#[mod_api::horizon_part]
impl BlockPart for PortedEntity {}

#[mod_api::horizon_part]
impl mod_api::BlockEntityPart for PortedEntity {
    fn block_entity(&self, _level: mod_api::LevelSide, event: &mut mod_api::BlockEntityEvent<'_>) -> Option<mod_api::Compound> {
        let id = entity_of(event.block)?;
        let mut nbt = mod_api::Compound::new();
        for (key, value) in runtime::block_entity_defaults(id) {
            nbt.insert(key.into(), tag_of(value));
        }
        Some(nbt)
    }

    fn block_entity_ticks(&self, _host: &dyn mod_api::Host, event: &mut mod_api::BlockEntityTicksEvent<'_>) {
        if runtime::BLOCK_ENTITIES.iter().any(|(b, _, t)| *b == event.block && *t) {
            event.ticks = true;
        }
    }

    fn block_entity_tick(&self, host: &mut dyn ServerHost, event: &mut mod_api::BlockEntityTickEvent<'_>) {
        let id = event.entity.id().to_string();
        if !runtime::BLOCK_ENTITIES.iter().any(|(_, e, _)| *e == id) {
            return;
        }
        let pos = event.pos;
        let dimension = host.dimension();
        let level = host.level(dimension);
        let (block, props) = level
            .and_then(|l| l.block_state(&*host, pos))
            .and_then(|s| host.block_state_string(s))
            .map(|t| parse_state(&t))
            .unwrap_or_default();
        let slots: Vec<Option<runtime::Stack>> =
            event.entity.items.iter().map(|s| s.as_ref().and_then(|s| stack_of(&*host, s)).map(|(i, c)| runtime::Stack::new(&i, c))).collect();
        let data = event.entity.data.clone();
        // The dropped items its tick looked at, by the id it was given.
        let seen: std::cell::RefCell<Vec<mod_api::EntityRef>> = std::cell::RefCell::new(Vec::new());
        let (changed, mut done) = {
            let reader: &dyn mod_api::Host = &*host;
            let dropped = |min: [f64; 3], max: [f64; 3]| -> Vec<runtime::Dropped> {
                let Some(level) = level else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                for entity in level.entities_in(reader, min, max).unwrap_or_default() {
                    let Some(item) = reader.item_entity(entity) else {
                        continue;
                    };
                    let (Some(stack), Some(at)) = (item.stack(reader), entity.position(reader)) else {
                        continue;
                    };
                    let Some((id, count)) = stack_of(reader, &stack) else {
                        continue;
                    };
                    let mut seen = seen.borrow_mut();
                    out.push(runtime::Dropped { id: seen.len() as u32, item: id, count, at });
                    seen.push(entity);
                }
                out
            };
            let recipes_of = |kind: &str| recipes(reader, kind);
            let tags = |kind: &str, tag: &str, id: &str| in_tag(reader, kind, tag, id);
            let items = |item: &str| item_facts(reader, item);
            let mut machine = runtime::Machine::new(slots.clone(), data.clone(), &recipes_of);
            machine.tags = Some(&tags);
            machine.items = Some(&items);
            machine.block = block.clone();
            // The blocks beside it, and their block entities' saved values where its tick
            // reads them (runtime.rs's `NEIGHBOURS`).
            if let Some(level) = level {
                let reads = runtime::NEIGHBOURS.contains(&id.as_str());
                machine.neighbours = runtime::Side::ALL
                    .iter()
                    .map(|side| {
                        let [dx, dy, dz] = side.offset();
                        let at = [pos[0] + dx, pos[1] + dy, pos[2] + dz];
                        let block = level
                            .block_state(reader, at)
                            .and_then(|s| reader.block_state_string(s))
                            .map_or_else(|| "minecraft:air".to_string(), |t| parse_state(&t).0);
                        let found = if reads { reader.block_entity(level, at) } else { None };
                        let entity = found.and_then(|e| e.kind(reader)).map(|k| k.id(reader));
                        let values = found
                            .and_then(|e| e.data(reader))
                            .map(|data| {
                                data.iter()
                                    .filter_map(|(k, t)| saved_of(t).map(|v| (k.to_string_lossy(), v)))
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(runtime::Neighbour { block, entity, values })
                    })
                    .collect();
            }
            // Its `facing`, as the block stores it, its state, its power, the items near.
            machine.facing = props.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.clone());
            machine.properties = props.clone();
            machine.powered = level.is_some_and(|l| l.has_neighbor_signal(reader, pos));
            machine.dropped = Some(&dropped);
            // Its random: a different draw each tick and block.
            thread_local! {
                static TICKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
            }
            let n = TICKS.with(|t| {
                t.set(t.get().wrapping_add(1));
                t.get()
            });
            machine.seed = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed_of(pos);
            let nbt = &event.entity.nbt;
            let changed = runtime::tick_block_entity(&id, pos, |key| saved_of(compound_get(nbt, key)?), &mut machine);
            (changed, machine.into_parts())
        };
        if done.sync {
            host.block_entity_sync(pos);
        }
        // The tick's sounds, particles and drops.
        for effect in std::mem::take(&mut done.effects) {
            match effect {
                runtime::Effect::Sound { sound, at, volume, pitch } => {
                    if let Some(level) = level {
                        let _ = level.play_sound(host, &sound, at, mod_api::SoundSource::Blocks, volume, pitch);
                    }
                }
                runtime::Effect::Particles { kind, at, spread, speed, count } => {
                    if let (Some(level), Some(key)) = (level, particle(&kind)) {
                        let options = mod_api::ParticleOptions::simple(key);
                        let _ = level.send_particles(host, &options, None, at, spread, speed, count);
                    }
                }
                runtime::Effect::Drop { item, count, at, motion } => {
                    use mod_api::Tag;
                    let mut stack = mod_api::Compound::new();
                    stack.insert("id".into(), Tag::String(item.as_str().into()));
                    stack.insert("count".into(), Tag::Int(count.min(99) as i32));
                    let mut nbt = mod_api::Compound::new();
                    nbt.insert("Item".into(), Tag::Compound(stack));
        // `ItemEntity.setDefaultPickUpDelay`: it pops out before anyone can take it.
        nbt.insert("PickupDelay".into(), Tag::Short(10));
                    nbt.insert(
                        "Motion".into(),
                        Tag::List(motion.iter().map(|v| Tag::Double(v.to_bits())).collect()),
                    );
                    let _ = host.summon(dimension, "minecraft:item", at, nbt);
                }
                runtime::Effect::TakeDropped { id, count } => {
                    let Some(entity) = seen.borrow().get(id as usize).copied() else {
                        continue;
                    };
                    let Some(item) = host.item_entity(entity) else {
                        continue;
                    };
                    let Some(mut stack) = item.stack(&*host) else {
                        continue;
                    };
                    if count >= stack.count() {
                        let _ = entity.discard(host);
                    } else if stack.set_count(stack.count() - count).is_ok() {
                        let _ = item.set_stack(host, stack);
                    }
                }
                runtime::Effect::Xp { at, amount } => {
                    if let Some(level) = level {
                        let _ = level.award_experience(host, at[0], at[1], at[2], amount);
                    }
                }
                runtime::Effect::SetState { key, value } => {
                    let mut now = props.clone();
                    if let Some(p) = now.iter_mut().find(|(k, _)| *k == key) {
                        p.1 = value;
                    }
                    let (Some(level), Some(state)) = (level, host.block_state_id(&state_text(&block, &now))) else {
                        continue;
                    };
                    let _ = level.set_block(host, pos, state, flags(2));
                }
            }
        }
        // What changed is written back: each write marks it for saving and sending.
        if let Some(values) = changed {
            for (key, value) in values {
                event.entity.nbt.insert(key.into(), tag_of(value));
            }
        }
        if done.slots != slots {
            for (i, new) in done.slots.iter().enumerate() {
                if slots.get(i) == Some(new) {
                    continue;
                }
                let made = new.as_ref().and_then(|s| item_stack(&*host, &s.item, s.count));
                if let Some(cell) = event.entity.items.get_mut(i) {
                    *cell = made;
                }
            }
        }
        if done.data != data {
            event.entity.data = done.data.clone();
        }
    }

    fn block_entity_update_tag(&self, event: &mut mod_api::BlockEntityUpdateTagEvent<'_>) -> Option<mod_api::Compound> {
        if !runtime::BLOCK_ENTITIES.iter().any(|(_, id, _)| *id == event.entity.id()) {
            return None;
        }
        use mod_api::Tag;
        let mut nbt = event.entity.nbt.clone();
        // Its items, which its renderer draws (a skillet's food): `item.N` and `count.N`;
        // an emptied slot too, since a client merges what it gets into what it had.
        for (i, slot) in event.entity.items.iter().enumerate() {
            let (item, count) = match slot.as_ref().and_then(stack_known) {
                Some((i, c)) => (i, c as i32),
                None => (String::new(), 0),
            };
            nbt.insert(format!("item.{i}").as_str().into(), Tag::String(item.as_str().into()));
            nbt.insert(format!("count.{i}").as_str().into(), Tag::Int(count));
        }
        Some(nbt)
    }

    // Broken, a block entity's items scatter (`Containers.dropItemStack`, as Java's
    // `preRemoveSideEffects` drops them); the game drops a container's itself.
    fn pre_remove_side_effects(
        &self,
        level: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::PreRemoveSideEffectsEvent<'_>,
    ) {
        let block = level.state_block(event.state).map(str::to_string).unwrap_or_default();
        if container_of(&block).is_some() {
            return;
        }
        event.drops.extend(event.entity.items.iter().flatten().cloned());
    }
__SUBMIT____SCREENS__}
"##;

const MACHINE_CONTAINER: &str = r#"    if let Some(machine) = runtime::MACHINES.iter().find(|x| x.block == block) {
        let inputs: Vec<usize> = (0..machine.slots).filter(|s| !machine.results.contains(s)).collect();
        let outputs: Vec<usize> = if machine.results.is_empty() {
            (0..machine.slots).collect()
        } else {
            machine.results.to_vec()
        };
        return Some(mod_api::ModContainer {
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
        });
    }
"#;

const HERE_ENTITIES: &str = r#"
// A click's or an item's code reaches the mod's block entities through the API's handles.
impl runtime::BlockEntities for Here<'_> {
    fn entity(&mut self, pos: [i32; 3]) -> Option<runtime::EntityValues> {
        let entity = self.host.block_entity(self.level?, pos)?;
        let data = entity.data(&*self.host)?;
        Some(runtime::EntityValues(
            data.iter()
                .filter_map(|(k, t)| saved_of(t).map(|v| (k.to_string_lossy(), v)))
                .collect(),
        ))
    }
    fn save_entity(&mut self, pos: [i32; 3], values: &[(&str, runtime::Saved)]) -> bool {
        let Some(entity) = self.level.and_then(|l| self.host.block_entity(l, pos)) else {
            return false;
        };
        let mut nbt = mod_api::Compound::new();
        for (key, value) in values {
            nbt.insert((*key).into(), tag_of(value.clone()));
        }
        let saved = entity.merge_data(self.host, nbt).unwrap_or(false);
        self.host.block_entity_sync(pos);
        saved
    }
    fn entity_slots(&mut self, pos: [i32; 3]) -> usize {
        let block = runtime::Blocks::block(self, pos);
        container_of(&block).map_or(0, |c| c.slots)
    }
    fn entity_item(&mut self, pos: [i32; 3], slot: usize) -> Option<(String, u32)> {
        let entity = self.host.block_entity(self.level?, pos)?;
        let stack = entity.item_slot(&*self.host, slot as u32).ok()??;
        stack_of(&*self.host, &stack)
    }
    fn set_entity_item(&mut self, pos: [i32; 3], slot: usize, item: Option<(&str, u32)>) -> bool {
        use mod_api::HostTransaction;
        let Some(entity) = self.level.and_then(|l| self.host.block_entity(l, pos)) else {
            return false;
        };
        let stack = match item {
            Some((id, count)) if count > 0 => match item_stack(&*self.host, id, count) {
                Some(s) => Some(s),
                None => return false,
            },
            _ => None,
        };
        let set = self
            .host
            .transaction(|host, tx| entity.set_item_slot(host, tx, slot as u32, stack))
            .unwrap_or(false);
        self.host.block_entity_sync(pos);
        set
    }
    fn recipes(&mut self, kind: &str) -> Vec<runtime::Recipe> {
        recipes(&*self.host, kind)
    }
}
"#;

/// Vanilla's particles that need no options, which a tick may send by name.
const VANILLA_PARTICLES: &[&str] = &[
    "smoke",
    "large_smoke",
    "white_smoke",
    "flame",
    "soul_fire_flame",
    "small_flame",
    "campfire_cosy_smoke",
    "campfire_signal_smoke",
    "bubble",
    "bubble_pop",
    "bubble_column_up",
    "splash",
    "cloud",
    "poof",
    "happy_villager",
    "angry_villager",
    "heart",
    "crit",
    "enchanted_hit",
    "enchant",
    "portal",
    "reverse_portal",
    "lava",
    "note",
    "witch",
    "effect",
    "instant_effect",
    "ash",
    "white_ash",
    "crimson_spore",
    "warped_spore",
    "spore_blossom_air",
    "falling_water",
    "dripping_water",
    "dripping_lava",
    "falling_lava",
    "dripping_honey",
    "falling_honey",
    "landing_honey",
    "composter",
    "electric_spark",
    "scrape",
    "wax_on",
    "wax_off",
    "glow",
    "end_rod",
    "totem_of_undying",
    "sneeze",
    "dolphin",
    "fishing",
    "underwater",
    "mycelium",
    "snowflake",
    "sculk_soul",
    "soul",
    "egg_crack",
    "cherry_leaves",
    "firefly",
];

const SUBMIT: &str = r#"
    fn block_entity_submit(&self, host: &dyn mod_api::ClientHost, event: &mut mod_api::BlockEntitySubmitEvent<'_>) {
        let id = event.entity.id().to_string();
        if !runtime::BLOCK_ENTITIES.iter().any(|(_, e, _)| *e == id) {
            return;
        }
        let nbt = &event.entity.nbt;
        // Its items, as the update tag carries them (an empty name is an emptied slot).
        let text = |key: String| match compound_get(nbt, &key) {
            Some(mod_api::Tag::String(s)) => Some(s.to_string_lossy()).filter(|s| !s.is_empty()),
            _ => None,
        };
        let count = |key: String| match compound_get(nbt, &key) {
            Some(mod_api::Tag::Int(n)) => (*n).max(0) as u32,
            _ => 1,
        };
        let slots = (0..64).rev().find(|i| text(format!("item.{i}")).is_some()).map_or(0, |i| i + 1);
        let props = host.block_state_string(event.state).map(|t| parse_state(&t).1).unwrap_or_default();
        let frame = runtime::Frame {
            pos: event.pos,
            block: event.block.to_string(),
            facing: props.iter().find(|(k, _)| k == "facing").map(|(_, v)| v.clone()),
            partial: event.draw.partial(),
            game_time: event.draw.game_time(),
            items: (0..slots)
                .map(|i| text(format!("item.{i}")).map(|item| (item, count(format!("count.{i}")))))
                .collect(),
        };
        let mut ops = runtime::Draw::default();
        runtime::render_block_entity(&id, |key| saved_of(compound_get(nbt, key)?), &frame, &mut ops);
        let draw = &mut *event.draw;
        for op in ops.ops {
            match op {
                runtime::DrawOp::Push => draw.push(),
                runtime::DrawOp::Pop => draw.pop(),
                runtime::DrawOp::Translate(by) => draw.translate(by),
                runtime::DrawOp::Rotate { axis, degrees, pivot } => draw.rotate_about(axis, degrees, pivot),
                runtime::DrawOp::Scale(by) => draw.scale(by),
                runtime::DrawOp::Block { block, properties } => {
                    if let Some(state) = host.block_state_id(&block) {
                        let properties: Vec<(&str, &str)> =
                            properties.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                        let _ = draw.block(state.block(), &properties);
                    }
                }
                runtime::DrawOp::Cube { from, to, sprite, tint } => {
                    // Horizon names vanilla's textures without `minecraft:` (`block/stone`).
                    let sprite = sprite.strip_prefix("minecraft:").unwrap_or(&sprite);
                    let face = Some(mod_api::CubeFace::new(sprite));
                    let _ = draw.cube(from, to, [face; 6], tint);
                }
                runtime::DrawOp::Item { item, count } => {
                    if let Some(stack) = item_stack(host, &item, count) {
                        let _ = draw.item(&stack, mod_api::NativeItemDisplayContext::Fixed, None, 0);
                    }
                }
                runtime::DrawOp::Keep => draw.keep(),
                // A model baked on its own: not reached through the API glue yet.
                runtime::DrawOp::Model { .. } => {}
            }
        }
    }
"#;

const SCREENS: &str = r#"
    // A machine's menu screen: where each slot sits, as its Java menu places them.
    fn screen(&self, host: &dyn mod_api::Host, event: &mut mod_api::ScreenEvent<'_>) -> Option<mod_api::NativeScreen> {
        let _ = host;
        let machine = runtime::MACHINES.iter().find(|x| x.block == event.block)?;
        Some(mod_api::NativeScreen {
            width: machine.size[0],
            height: machine.size[1],
            slots: machine.layout.to_vec(),
            inventory_y: machine.inventory[1],
            inventory_x: machine.inventory[0],
            own_labels: false,
            // The mod's own texture instead of the plain panel.
            own_background: machine.texture.is_some(),
        })
    }

    // Draws a machine's screen under its slots and items.
    fn screen_draw(&self, host: &dyn mod_api::ClientHost, event: &mut mod_api::ScreenDrawEvent<'_>) {
        let level = host.level(host.dimension());
        let read = |p: [i32; 3]| {
            level
                .and_then(|l| l.block_state(host, p))
                .and_then(|s| host.block_state_string(s))
                .map(|t| parse_state(&t).0)
                .unwrap_or_else(|| "minecraft:air".into())
        };
        // The machine's place: the block under the crosshair, whose menu is open.
        let pos = host
            .raycast(6.0)
            .map(|hit| [hit.block_x, hit.block_y, hit.block_z])
            .filter(|p| read(*p) == event.block);
        let tags = |kind: &str, tag: &str, id: &str| in_tag(host, kind, tag, id);
        let at = runtime::ScreenAt { pos, block: &read, tags: &tags };
        let origin = event.origin;
        for sprite in runtime::screen_sprites(event.block, event.data, &at) {
            let [x, y, w, h] = sprite.dst;
            let _ = event.draw.sprite(sprite.name, sprite.src, [origin[0] + x, origin[1] + y, w, h], None);
        }
    }

    // A shift-click in a machine's menu: the Java menu's `quickMoveStack` decides.
    fn menu_quick_move(&self, host: &mut dyn ServerHost, event: &mut mod_api::MenuQuickMoveEvent<'_>) -> Flow {
        let player = host.players().acting_player();
        let Some(level) = player.and_then(|p| p.entity().level(&*host)) else {
            return Flow::Continue;
        };
        let block = level
            .block_state(&*host, event.pos)
            .and_then(|s| host.block_state_string(s))
            .map(|t| parse_state(&t).0)
            .unwrap_or_default();
        let Some(menu) = runtime::MACHINES.iter().find(|x| x.block == block).map(|x| x.menu) else {
            return Flow::Continue;
        };
        if !runtime::moves_on_shift_click(menu) {
            return Flow::Continue;
        }
        let container_slots = event.container_slots;
        let before: Vec<Option<runtime::Stack>> = (0..container_slots)
            .map(|i| {
                event.entity.items.get(i).and_then(Option::as_ref).and_then(|s| stack_of(&*host, s))
                    .map(|(item, count)| runtime::Stack::new(&item, count))
            })
            .collect();
        let Some((item, count)) = stack_of(&*host, event.stack) else {
            return Flow::Continue;
        };
        // Stack sizes, read before `give` borrows the host.
        let mut sizes: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for s in before.iter().flatten().map(|s| s.item.clone()).chain(std::iter::once(item.clone())) {
            let n = item_facts(&*host, &s).map_or(64, |(n, _)| n);
            sizes.insert(s, n);
        }
        let max_stack = |item: &str| sizes.get(item).copied().unwrap_or(64);
        let q = runtime::QuickMove { slot: event.slot, container_slots };
        // The API's stacks are never empty, so a move that takes the whole stack can't
        // leave its slot empty: tried first without giving, that one goes the game's way.
        let mut moving = runtime::Stack::new(&item, count);
        let mut keep = |_: &str, count: u32| count;
        let mut slots = runtime::MenuSlots::new(before.clone(), &max_stack, &mut keep);
        if !runtime::quick_move(menu, &q, &mut moving, &mut slots) || moving.count == 0 {
            return Flow::Continue;
        }
        let dimension = host.dimension();
        let mut here = Here {
            host: &mut *host,
            level: Some(level),
            dimension,
            player,
            hand: mod_api::Hand::Main,
            seed: seed_of(event.pos),
        };
        let mut give = |item: &str, count: u32| runtime::World::give(&mut here, item, count);
        let mut moving = runtime::Stack::new(&item, count);
        let mut slots = runtime::MenuSlots::new(before.clone(), &max_stack, &mut give);
        if !runtime::quick_move(menu, &q, &mut moving, &mut slots) || moving.count == 0 {
            return Flow::Continue;
        }
        // The block's slots the move changed; the clicked slot holds what is left.
        let mut after = slots.container;
        drop(give);
        if event.slot < container_slots {
            after[event.slot] = Some(moving.clone()).filter(|s| s.count > 0);
        }
        if event.entity.items.len() < container_slots {
            event.entity.items.resize_with(container_slots, || None);
        }
        for (i, (was, now)) in before.iter().zip(&after).enumerate() {
            if was != now {
                event.entity.items[i] = now.as_ref().and_then(|s| item_stack(&*here.host, &s.item, s.count));
            }
        }
        let _ = event.stack.set_count(moving.count);
        Flow::Cancel
    }
"#;

/// A block tick the neighbour part asked for: the Java's `neighborChanged` runs in it.
const NEIGHBOUR_IN_TICK: &str = r#"        let pos = [event.pos.x, event.pos.y, event.pos.z];
        let asked = NEIGHBOUR_CHANGED
            .lock()
            .ok()
            .and_then(|mut p| p.iter().position(|x| *x == pos).map(|i| p.swap_remove(i)))
            .is_some();
        if asked {
            let t = runtime::BlockTick { block: block.clone(), pos };
            let _ = runtime::neighbour_changed(&t, &mut Level { level });
            if !runtime::ticks_its_own_way(&block) {
                return Flow::Cancel;
            }
        }
"#;

/// `neighborChanged`. Horizon's neighbour hook can't change the world, so it asks for a
/// block tick at once, and the Java's code runs in that.
const PART_NEIGHBOUR: &str = r#"    fn neighbor_changed(&self, level: &mut dyn mod_api::LevelTicksHost, event: &mut mod_api::NeighborChangedEvent) {
        let Some(block) = level.state_block(event.state).map(str::to_string) else {
            return;
        };
        if !runtime::acts_on_neighbour_change(&block) {
            return;
        }
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if let Ok(mut pending) = NEIGHBOUR_CHANGED.lock() {
            if pending.contains(&pos) {
                return;
            }
            pending.push(pos);
        }
        let _ = level.schedule_block_tick(pos, event.state.block(), 1);
    }

"#;

const NEIGHBOUR_PENDING: &str = r#"
/// Blocks whose neighbours changed, until their block tick runs the Java's
/// `neighborChanged`.
static NEIGHBOUR_CHANGED: std::sync::Mutex<Vec<[i32; 3]>> = std::sync::Mutex::new(Vec::new());
"#;
