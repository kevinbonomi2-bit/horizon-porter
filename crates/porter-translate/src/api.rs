//! The port as a trait mod on Horizon's mod API (`mod_api`, the arismoko/horizon fork:
//! Parts, a prebuilt game, ~1 s builds), beside the horizon-mixins bundle: `api/` holds
//! `horizon-mod.toml` (the content mod's, with `[native]` pins), `Cargo.toml`, the
//! content's `assets/` and `data/`, `src/runtime.rs` (a copy of the bundle's, written
//! with it, so the translated code is the same) and `src/lib.rs`, the glue written here:
//! a `BlockPart` and an `ItemPart` that turn the API's events into runtime.rs's inputs
//! and its `World` into the host's level writes.
//!
//! The glue covers the behaviours runtime.rs dispatches by kind (placement, clicks,
//! shape updates, ticks, survival, comparators, item use, tooltips). Block entities,
//! machines, screens and the mod's own hooks are the bundle's alone for now.

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
/// runtime.rs has block entities (its `World` needs `BlockEntities` then).
pub fn files(
    id: &str,
    content_manifest: &str,
    behaviours: &[Behaviour],
    entities: bool,
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
    out.insert("api/src/lib.rs".into(), lib_rs(behaviours, entities));
    out
}

/// Whether a mod has anything for the glue to run: a behaviour of a kind it covers.
pub fn wanted(behaviours: &[Behaviour]) -> bool {
    behaviours.iter().any(|b| covered(b.kind))
}

fn covered(kind: Kind) -> bool {
    !matches!(kind, Kind::Placed | Kind::Destroyed)
}

fn lib_rs(behaviours: &[Behaviour], entities: bool) -> String {
    let has = |k: Kind| behaviours.iter().any(|b| b.kind == k);
    let block_kinds = [
        Kind::Place,
        Kind::Use,
        Kind::Shape,
        Kind::RandomTick,
        Kind::Tick,
        Kind::Survive,
        Kind::Analog,
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
    let list = |names: &[&str]| -> String {
        names.iter().map(|n| format!("    {n:?},\n")).collect()
    };
    let _ = write!(
        r,
        "\n/// The mod's blocks whose Java has code the port runs.\nconst BLOCKS: &[&str] = &[\n{}];\n\n/// The mod's items whose Java has code the port runs.\nconst ITEMS: &[&str] = &[\n{}];\n",
        list(&blocks),
        list(&items)
    );
    r.push_str(COMMON);
    if has(Kind::Use) || has(Kind::ItemUseOn) || has(Kind::ItemUse) {
        r.push_str(HERE);
        if entities {
            r.push_str(&no_entities("Here<'_>"));
        }
    }
    if has(Kind::RandomTick) || has(Kind::Tick) {
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
    if has(Kind::Tick) {
        r.push_str(PART_TICK);
    }
    if has(Kind::Survive) {
        r.push_str(PART_SURVIVE);
    }
    if has(Kind::Analog) {
        r.push_str(PART_ANALOG);
    }
    r.push_str("}\n");
    if has(Kind::Use) {
        r.push_str(CLICK_FN);
    }

    r.push_str("\n/// The port's code for one of the mod's items.\nstruct PortedItem;\n\n#[mod_api::horizon_part]\nimpl ItemPart for PortedItem {\n");
    if has(Kind::ItemUseOn) {
        r.push_str(PART_USE_ON);
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
    r.push_str(REGISTER);
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
}
"#;

const HERE: &str = r#"
/// The world as a click's or an item's code changes it: its level, through the server
/// host. States go by their full names, as `/setblock` reads them.
struct Here<'h> {
    host: &'h mut dyn ServerHost,
    level: Option<mod_api::LevelRef>,
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
        if !runtime::ticks_its_own_way(&block) {
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
    let level = host.level(dimension_id(click.dimension));
    let mut here = Here { host, level, seed: seed_of(click.pos) };
    runtime::use_block(&used, &mut here).then_some(InteractionResult::Success)
}
"#;

const PART_USE_ON: &str = r#"    fn use_on(&self, host: &mut dyn ServerHost, event: &mut mod_api::UseOnEvent<'_>) -> Flow {
        let item = event.stack.item().id(&*host).to_string();
        if !runtime::item_acts_on_block(&item) {
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
    let level = acting.and_then(|p| p.entity().level(&*host));
    let pos = on.as_ref().map_or([0, 0, 0], |o| o.pos);
    let u = runtime::ItemUse {
        item,
        count: stack.count(),
        damage: 0,
        on,
        creative,
        sneaking,
    };
    let mut here = Here { host, level, seed: seed_of(pos) };
    run(&u, &mut here)
}
"#;

const REGISTER: &str = r#"
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
    }
}
"#;
