// Compiles a generated bundle's helpers against a mock of Horizon's native hook API.
//
// The mock copies the real signatures (horizon-client's game/native_hooks.rs,
// mod_api.rs and extras_hud.rs, 2026-11) and splices each helper into its hook the
// way horizon-mixins does: the helper becomes a local function at the head of the
// hooked function, called with the manifest's bindings. Building Horizon itself is
// what `native.py check` does; this catches type and binding mistakes without it.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use porter_translate::bundle;

/// Hook signatures, as in horizon-client's native_hooks.rs.
const HOOKS: &[(&str, &str)] = &[
    ("native_mods", "&mut self, mods: &mut Vec<NativeMod>"),
    ("native_key", "&mut self, key: &str, pressed: bool"),
    ("native_command", "&mut self, command: &str, ran: &mut bool"),
    (
        "native_hud",
        "&mut self, batch: &mut HudBatch, corners: &mut Corners",
    ),
    ("native_tick", "&mut self"),
    ("native_wheel", "&mut self, steps: f32, claimed: &mut bool"),
    (
        "native_settings",
        "&mut self, mod_id: &str, buttons: &mut Vec<String>",
    ),
    (
        "native_settings_click",
        "&mut self, mod_id: &str, button: usize",
    ),
    (
        "native_block_entity",
        "&mut self, block: &str, entity: &mut Option<(String, Compound)>",
    ),
    (
        "native_block_entity_ticks",
        "&self, block: &str, ticks: &mut bool",
    ),
    (
        "native_block_entity_tick",
        "&mut self, pos: [i32; 3], entity: &mut ModBlockEntity",
    ),
    (
        "native_block_entity_container",
        "&self, block: &str, container: &mut Option<ModContainer>",
    ),
    (
        "native_screen",
        "&self, block: &str, screen: &mut Option<NativeScreen>",
    ),
    (
        "native_screen_draw",
        "&self, block: &str, data: &[i32], batch: &mut HudBatch, origin: [f32; 2]",
    ),
    (
        "native_net_from_client",
        "&mut self, player: u128, channel: &str, data: &[u8]",
    ),
    (
        "native_net_from_server",
        "&mut self, channel: &str, data: &[u8]",
    ),
    ("native_frame", "&mut self, elapsed: std::time::Duration"),
    ("native_standalone_models", "models: &mut Vec<String>"),
    (
        "native_block_entity_fluids",
        "&self, block: &str, fluids: &mut Option<ModFluidTanks>",
    ),
    (
        "native_block_entity_energy",
        "&self, block: &str, energy: &mut Option<ModEnergyBuffer>",
    ),
    (
        "native_screen_render_post",
        "&self, batch: &mut HudBatch, screen: &NativeScreenView<'_>",
    ),
    (
        "native_screen_mouse_button_pressed_pre",
        "&self, screen: &NativeScreenView<'_>, button: i32, modifiers: u32, canceled: &mut bool",
    ),
    (
        "native_screen_key_pressed_pre",
        "&self, screen: &NativeScreenView<'_>, key: &str, repeat: bool, modifiers: u32, canceled: &mut bool",
    ),
    (
        "native_screen_mouse_scrolled_pre",
        "&self, screen: &NativeScreenView<'_>, scroll: [f64; 2], modifiers: u32, canceled: &mut bool",
    ),
    (
        "native_block_entity_update_tag",
        "pos: [i32; 3], entity: &ModBlockEntity, tag: &mut Option<Compound>",
    ),
    (
        "native_block_entity_submit",
        "&self, pos: [i32; 3], block: &str, state: BlockStateId, entity: &ModBlockEntity, draw: &mut BlockEntityDraw",
    ),
    (
        "native_right_click_block",
        "&mut self, pos: [i32; 3], face: [i32; 3], canceled: &mut bool",
    ),
    (
        "native_use_item_on",
        "&mut self, stack: &ItemStack, pos: [i32; 3], face: [i32; 3], claimed: &mut bool",
    ),
    (
        "native_use_item",
        "&mut self, stack: &ItemStack, claimed: &mut bool",
    ),
];

/// The hooks on `GameLevel` (a server level), as in native_hooks.rs.
const LEVEL_HOOKS: &[(&str, &str)] = &[
    (
        "native_state_for_placement",
        "&self, dimension: LevelDimension, state: BlockStateId, context: &PlacementContext, fluid: FluidState, answer: &mut Option<Option<BlockStateId>>",
    ),
    (
        "native_update_shape",
        "&self, ticks: &mut Ticks, dimension: LevelDimension, pos: BlockPos, state: BlockStateId, direction: Direction, neighbour_pos: BlockPos, neighbour: BlockStateId, new: &mut Option<BlockStateId>",
    ),
];

const MOCK_LIB: &str = r#"
extern crate self as horizon_client;

// horizon-core's geometry: the six directions.
pub mod horizon_core {
    pub mod geometry {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Direction { Down, Up, North, South, West, East }
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Vec3 { pub x: f64, pub y: f64, pub z: f64 }
    }
    pub mod random {
        pub struct LegacyRandom;
        pub trait RandomSource: Sized {
            fn next_float(&mut self) -> f32;
        }
        impl RandomSource for LegacyRandom {
            fn next_float(&mut self) -> f32 { 0.5 }
        }
    }
    pub mod position {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct BlockPos { pub x: i32, pub y: i32, pub z: i32 }
    }
}

// horizon-assets' item display contexts (crates/horizon-assets/src/item_model.rs).
pub mod horizon_assets {
    pub mod item_model {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum DisplayContext { None, Gui, Ground, Fixed }
    }
}

// horizon-world's nbt module, as Horizon has it (crates/horizon-world/src/nbt.rs).
pub mod horizon_world {
    pub mod registry {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct BlockStateId(pub u32);
        pub struct BlockRegistry;
        impl BlockRegistry {
            pub fn default_state(&self, _name: &str) -> Option<BlockStateId> { None }
        }
    }
    pub mod nbt {
        use std::collections::BTreeMap;
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct JavaString(pub Vec<u16>);
        impl From<&str> for JavaString {
            fn from(value: &str) -> Self {
                Self(value.encode_utf16().collect())
            }
        }
        impl JavaString {
            pub fn to_string_lossy(&self) -> String {
                String::from_utf16_lossy(&self.0)
            }
        }
        pub type Compound = BTreeMap<JavaString, Tag>;
        #[derive(Clone, Debug, PartialEq)]
        pub enum Tag {
            End,
            Byte(i8),
            Short(i16),
            Int(i32),
            Long(i64),
            Float(u32),
            Double(u64),
            ByteArray(Vec<u8>),
            String(JavaString),
            List(Vec<Tag>),
            Compound(Compound),
            IntArray(Vec<i32>),
            LongArray(Vec<i64>),
        }
    }
}

// horizon-client's containers module: ModBlockEntity derefs to its fields, and
// each mutable access counts as a change.
pub mod containers {
    use crate::items::ItemStack;
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ModFluidTanks {
        pub tanks: Vec<i32>,
        pub faces: [Vec<usize>; 6],
        pub inputs: Vec<usize>,
        pub outputs: Vec<usize>,
    }
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ModEnergyBuffer {
        pub capacity: i32,
        pub max_insert: i32,
        pub max_extract: i32,
        pub insert_faces: [bool; 6],
        pub extract_faces: [bool; 6],
    }
    pub struct ModBlockEntityFields {
        pub id: String,
        pub nbt: crate::horizon_world::nbt::Compound,
        pub items: Vec<Option<ItemStack>>,
        pub data: Vec<i32>,
    }
    pub struct ModBlockEntity {
        fields: ModBlockEntityFields,
        pub changes: u64,
    }
    impl std::ops::Deref for ModBlockEntity {
        type Target = ModBlockEntityFields;
        fn deref(&self) -> &ModBlockEntityFields {
            &self.fields
        }
    }
    impl std::ops::DerefMut for ModBlockEntity {
        fn deref_mut(&mut self) -> &mut ModBlockEntityFields {
            self.changes += 1;
            &mut self.fields
        }
    }
    // containers.rs (horizon-client-render): a mod block's slots and menu.
    pub struct ModContainer {
        pub slots: usize,
        pub faces: [Vec<usize>; 6],
        pub inputs: Vec<usize>,
        pub outputs: Vec<usize>,
        pub menu: Option<String>,
    }
}

// horizon-items' ItemStack (items.rs, item_data.rs).
pub mod items {
    use std::sync::Arc;
    pub struct StackData;
    pub struct ItemStack {
        pub item: Arc<str>,
        pub count: u32,
        pub damage: u32,
        pub extra: Option<Box<StackData>>,
    }
    impl ItemStack {
        pub fn new(item: &str, count: u32) -> Self {
            Self { item: item.into(), count, damage: 0, extra: None }
        }
    }
}

// crafting.rs: a mod's recipe as its file says, and an ingredient's items.
pub mod crafting {
    use std::collections::HashSet;
    use std::sync::Arc;
    pub struct CustomRecipe {
        pub id: String,
        pub json: Arc<serde_json::Value>,
    }
    pub struct Ingredient {
        pub items: HashSet<String>,
        pub order: Vec<String>,
    }
}

// mod_api.rs: a mod block's screen.
// first_person.rs: the two hands.
pub mod first_person {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Hand {
        Main,
        Off,
    }
}

pub mod mod_api {
    pub struct NativeScreen {
        pub width: f32,
        pub height: f32,
        pub slots: Vec<Option<[f32; 2]>>,
        pub inventory_y: f32,
        pub inventory_x: f32,
        pub own_labels: bool,
        pub own_background: bool,
    }
    pub struct NativeScreenView<'a> {
        pub id: &'static str,
        pub menu: Option<&'a str>,
        pub rect: [f32; 4],
        pub slots: usize,
        pub mouse: [f32; 2],
        pub partial_tick: f32,
    }
    // A level read as vanilla's LevelReader (mod_api.rs), which GameLevel implements.
    pub trait LevelHost {
        fn block_state(&self, pos: [i32; 3]) -> crate::horizon_world::registry::BlockStateId;
    }
    // The player whose action a hook hears (`native_acting_player`).
    pub struct NativeServerPlayer {
        pub uuid: u128,
        pub name: String,
        pub dimension: String,
        pub position: [f64; 3],
        pub game_mode: i32,
        pub yaw: f32,
        pub pitch: f32,
    }
}

pub mod hud {
    pub struct HudBatch;
}
pub mod extras {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Corner { TopLeft, TopRight, BottomLeft, BottomRight }
}
pub mod extras_hud {
    pub struct Corners;
    impl Corners {
        pub fn place(&mut self, corner: crate::extras::Corner, w: f32, h: f32) -> (f32, f32) {
            let _ = (corner, w, h);
            (0.0, 0.0)
        }
    }
}
pub mod game {
    pub struct Game<'a> {
        _game: std::marker::PhantomData<&'a ()>,
        // The game's resources: its block registry (game.rs), which native_hooks reads.
        pub pack: PackResources,
        // The hand a use is for (game.rs), which native_hooks reads.
        pub hand: crate::first_person::Hand,
        // The game's resources (game.rs): its items' facts. Private there: native_hooks,
        // a child module, reads it.
        #[allow(dead_code)]
        res: std::sync::Arc<Resources>,
        #[allow(dead_code)]
        held: Option<crate::items::ItemStack>,
    }
    // game.rs: the stack in the hand that acts, private to game and its children.
    #[allow(dead_code)]
    impl Game<'_> {
        fn held_mut(&mut self) -> &mut Option<crate::items::ItemStack> {
            &mut self.held
        }
    }
    pub struct Resources;
    #[allow(dead_code)]
    impl Resources {
        pub fn items(&self) -> &Items {
            &Items
        }
    }
    // horizon-items' item facts.
    pub struct Items;
    pub struct ItemInfo {
        pub max_stack: u32,
        pub max_damage: u32,
    }
    #[allow(dead_code)]
    impl Items {
        pub fn get(&self, _id: &str) -> Option<&ItemInfo> {
            None
        }
    }
    pub struct PackResources {
        pub assets: Assets,
    }
    pub struct Assets {
        pub registry: crate::horizon_world::registry::BlockRegistry,
    }
    // native_world_edit.rs: bulk block edits, from block states as commands write them.
    pub mod native_world_edit {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum NativeHeightmap { WorldSurface, MotionBlocking, MotionBlockingNoLeaves, OceanFloor }
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct NativeBlockRun {
            pub x: i32,
            pub z: i32,
            pub y: i32,
            pub height: u32,
            pub surface: Option<NativeHeightmap>,
            pub block: u32,
            pub only: Option<u32>,
        }
        #[derive(Clone, Debug, PartialEq, Default)]
        pub struct NativeBlockEdit {
            pub palette: Vec<String>,
            pub filters: Vec<String>,
            pub runs: Vec<NativeBlockRun>,
        }
    }
    // game.rs: a server level, which the world hooks run on.
    pub struct GameLevel<'a> {
        _level: std::marker::PhantomData<&'a ()>,
    }
    // native_block_entity_draw.rs: the pose stack a block entity is drawn under.
    pub mod native_block_entity_draw {
        #[derive(Clone, Copy, Debug)]
        pub struct CubeFace<'a> {
            pub sprite: &'a str,
            pub uv: Option<[f32; 4]>,
        }
        pub struct BlockEntityDraw {
            pub partial: f32,
            pub game_time: i64,
            pub light: u32,
        }
        #[allow(unused_variables)]
        impl BlockEntityDraw {
            pub fn push(&mut self) {}
            pub fn pop(&mut self) {}
            pub fn translate(&mut self, by: [f32; 3]) {}
            pub fn rotate_about(&mut self, axis: [f32; 3], degrees: f32, pivot: [f32; 3]) {}
            pub fn scale(&mut self, by: [f32; 3]) {}
            pub fn keep(&mut self) {}
        }
    }
    pub use native_block_entity_draw::BlockEntityDraw;
    __MODULE__
    pub mod native_hooks {
        #![allow(unused_variables, dead_code)]
        use super::Game;
        use crate::containers::{ModBlockEntity, ModContainer, ModEnergyBuffer, ModFluidTanks};
        // Horizon's native_hooks.rs has it in scope: stubbed helpers name it bare.
        #[allow(unused_imports)]
        use std::time::Duration;
        use crate::crafting::{CustomRecipe, Ingredient};
        use crate::extras_hud::Corners;
        use crate::items::ItemStack;
        use crate::mod_api::{NativeScreen, NativeScreenView};
        use crate::horizon_world::nbt::Compound;
        use crate::horizon_world::registry::BlockStateId;
        use super::native_block_entity_draw::{BlockEntityDraw, CubeFace};
        use crate::hud::HudBatch;
        #[allow(unused_imports)]
        use crate::horizon_assets;
        #[allow(unused_imports)]
        use crate::horizon_core;
        use crate::horizon_core::geometry::Direction;
        pub struct NativeContainerSlot {
            pub slot: usize,
            pub stack: Option<ItemStack>,
            pub can_take: bool,
        }
        // In Horizon, horizon_world is a dependency: helpers name it by its crate path.
        #[allow(unused_imports)]
        use crate::horizon_world;

        use super::GameLevel;
        use crate::horizon_core::position::BlockPos;
        // horizon-world's level types the world hooks take.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum LevelDimension { Overworld, Nether, End }
        pub struct Ticks;
        #[derive(Clone, Copy, Debug)]
        pub struct FluidState;
        // horizon-world's level/place.rs: vanilla's BlockPlaceContext.
        pub struct PlacementContext {
            pub pos: BlockPos,
            pub hit_pos: BlockPos,
            pub face: Direction,
            pub hit: crate::horizon_core::geometry::Vec3,
            pub inside: bool,
            pub yaw: f32,
            pub pitch: f32,
            pub sneaking: bool,
            pub item: String,
            pub replace_clicked: bool,
        }

        // native_hooks.rs: `BlockState.setValue` on a registry's state.
        #[allow(dead_code)]
        fn with_property(registry: &crate::horizon_world::registry::BlockRegistry, state: BlockStateId, key: &str, value: &str) -> Option<BlockStateId> { None }

        impl crate::mod_api::LevelHost for GameLevel<'_> {
            fn block_state(&self, pos: [i32; 3]) -> BlockStateId { BlockStateId(0) }
        }

        impl GameLevel<'_> {
            pub fn native_state_block(&self, state: BlockStateId) -> Option<&str> { None }
            pub fn native_state_with_property(&self, state: BlockStateId, key: &str, value: &str) -> Option<BlockStateId> { None }
        }

        impl GameLevel<'_> {
__LEVEL_HOOKS__
        }

        pub struct NativeMod {
            pub id: String,
            pub title: String,
            pub description: String,
            pub keys: Vec<[String; 3]>,
        }
        pub struct NativePlayer {
            pub x: f64,
            pub y: f64,
            pub z: f64,
            pub yaw: f32,
            pub pitch: f32,
            pub dimension: &'static str,
        }

        impl Game<'_> {
            pub fn native_on(&self, mod_id: &str) -> bool { true }
            pub fn native_playing(&self) -> bool { true }
            pub fn native_window(&self) -> bool { true }
            pub fn native_text_width(&self, text: &str) -> f32 { 0.0 }
            pub fn native_fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32) {}
            pub fn native_text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], rgb: u32, shadow: bool) {}
            pub fn native_player(&self) -> Option<NativePlayer> { None }
            pub fn native_mod_key(&self, mod_id: &str, name: &str, default: &str) -> String { String::new() }
            pub fn native_chat(&mut self, text: &str) {}
            pub fn native_setting(&self, mod_id: &str, key: &str) -> Option<String> { None }
            pub fn native_set_setting(&self, mod_id: &str, key: &str, value: &str) {}
            pub fn native_sprite(&self, batch: &mut HudBatch, name: &str, src: [f32; 4], dst: [f32; 4]) -> bool { false }
            pub fn native_sprite_size(&self, name: &str) -> Option<[f32; 2]> { None }
            pub fn native_recipes_of_type(&self, kind: &str) -> &[CustomRecipe] { &[] }
            pub fn native_recipe_ingredient(&self, json: &serde_json::Value) -> Option<Ingredient> { None }
            pub fn native_recipe_result(&self, json: &serde_json::Value) -> Option<ItemStack> { None }
            pub fn native_dimension(&self) -> &'static str { "minecraft:overworld" }
            pub fn native_block(&self, pos: [i32; 3]) -> String { String::new() }
            pub fn native_with_mod_block_entity<R>(&mut self, pos: [i32; 3], change: impl FnOnce(&mut ModBlockEntity) -> R) -> Option<R> { None }
            pub fn native_block_entity_sync(&mut self, pos: [i32; 3]) {}
            pub fn native_insert_item(&mut self, pos: [i32; 3], face: Direction, stack: ItemStack) -> Option<ItemStack> { Some(stack) }
            pub fn native_container_slots(&self, dimension: &str, pos: [i32; 3], face: Direction) -> Option<Vec<NativeContainerSlot>> { None }
            pub fn native_extract_item(&mut self, dimension: &str, pos: [i32; 3], face: Direction, slot: usize, count: u32) -> Option<ItemStack> { None }
            pub fn native_mod_screen_open(&mut self, mod_id: &str) -> bool { false }
            pub fn native_mod_screen_close(&mut self, mod_id: &str) {}
            pub fn native_mod_screen(&self) -> Option<&str> { None }
            pub fn native_item(&self, batch: &mut HudBatch, item: &str, at: [f32; 2], size: f32) -> bool { false }
            pub fn native_draw_model(&self, draw: &mut BlockEntityDraw, model: &str, tint: u32) -> bool { false }
            pub fn native_draw_block(&self, draw: &mut BlockEntityDraw, block: &str, properties: &[(&str, &str)]) -> bool { false }
            pub fn native_draw_cube(&self, draw: &mut BlockEntityDraw, from: [f32; 3], to: [f32; 3], faces: [Option<CubeFace<'_>>; 6], tint: u32) -> bool { false }
            pub fn native_draw_item(&self, draw: &mut BlockEntityDraw, stack: &ItemStack, display: crate::horizon_assets::item_model::DisplayContext, owner_yaw: Option<f32>, seed: i32) -> bool { false }
            pub fn native_block_state_property(&self, state: BlockStateId, key: &str) -> Option<&str> { None }
            pub fn native_net_send_to_player(&mut self, player: u128, channel: &str, data: &[u8]) -> Result<(), String> { Ok(()) }
            pub fn native_net_send_to_server(&mut self, channel: &str, data: &[u8]) -> Result<(), String> { Ok(()) }
            pub fn native_held(&self) -> Option<(String, u32, bool)> { None }
            pub fn native_give_items(&mut self, item: &str, count: u32) -> u32 { 0 }
            pub fn native_set_block(&mut self, dimension: &str, pos: [i32; 3], state: BlockStateId, flags: i32) -> bool { false }
            pub fn native_edit_blocks(&mut self, dimension: &str, edit: &super::native_world_edit::NativeBlockEdit) -> Result<u64, String> { Ok(0) }
            pub fn native_acting_player(&self) -> Option<crate::mod_api::NativeServerPlayer> { None }
            pub fn native_block_collision(&self, pos: [i32; 3]) -> Vec<[f64; 6]> { Vec::new() }
            pub fn native_level_random(&mut self, dimension: &str) -> Option<&mut crate::horizon_core::random::LegacyRandom> { None }
            pub fn native_block_property(&self, pos: [i32; 3], key: &str) -> Option<String> { None }
            pub fn native_set_block_property_with(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool { false }
            // native_host_bridge.rs: Game's is `native_level_play_sound` (GameLevel has `native_play_sound`).
            pub fn native_level_play_sound(&mut self, sound: &str, at: [f64; 3], source: usize, volume: f32, pitch: f32) -> bool { false }
        }

        impl Game<'_> {
__HOOKS__
        }
    }
}
"#;

fn compass_scan() -> porter_model::ModScan {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/fabric-client-demo");
    porter_java::scan(&path).unwrap()
}

#[test]
fn helpers_compile_against_horizon_api() {
    let mut scan = compass_scan();
    // The fixture uses every wired hook but the wheel: add a wheel listener so the
    // check covers all of them.
    scan.classes[0]
        .references
        .push("net.neoforged.neoforge.client.event.InputEvent.MouseScrollingEvent".into());
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    assert_eq!(
        b.wired.len(),
        porter_map::WIRED_HOOKS.len(),
        "{:?}",
        b.wired.keys()
    );
    compiles_against_the_mock(&b, "client");
}

#[test]
fn block_entity_helpers_compile_against_horizon_api() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../porter-java/tests/fixtures/neoforge-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    for hook in [
        "block_entity",
        "block_entity_ticks",
        "block_entity_tick",
        "right_click_block",
    ] {
        assert!(b.wired.contains_key(hook), "{hook}");
    }
    // The crank's click reaches its block entity: ported as translate would, it
    // loads the entity's struct, winds it and saves it.
    let mut b = b;
    let file = format!("mixins/{}/runtime.rs", b.id);
    let runtime = b.files[&file].clone();
    assert!(
        runtime.contains("pub trait World: Blocks + BlockEntities {"),
        "{runtime}"
    );
    let stub = "fn crank_block_use(u: &BlockUse, world: &mut dyn World) -> bool {\n    let _ = (u, world);\n    false\n}";
    assert!(runtime.contains(stub), "{runtime}");
    let ported = runtime.replace(
        stub,
        "fn crank_block_use(u: &BlockUse, world: &mut dyn World) -> bool {
    let Some(values) = world.entity(u.pos) else {
        return false;
    };
    let mut crank = Crank::load(|k| values.get(k));
    crank.speed = (crank.speed + 16.0).min(256.0);
    world.save_entity(u.pos, &crank.save());
    if world.entity_slots(u.pos) > 0 && world.entity_item(u.pos, 0).is_none() {
        world.set_entity_item(u.pos, 0, Some((\"minecraft:stick\", 1)));
    }
    true
}",
    );
    b.files.insert(file, ported);
    compiles_against_the_mock(&b, "blocks");
}

#[test]
fn machine_helpers_compile_against_horizon_api() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-machine-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    for hook in [
        "block_entity_container",
        "screen",
        "screen_draw",
        "block_entity_tick",
        "net_from_client",
        "net_from_server",
        "settings",
        "settings_click",
        "block_entity_update_tag",
        "block_entity_submit",
        "frame",
        "standalone_models",
        "block_entity_fluids",
        "block_entity_energy",
        "screen_render_post",
        "screen_mouse_button_pressed_pre",
        "screen_key_pressed_pre",
        "screen_mouse_scrolled_pre",
    ] {
        assert!(b.wired.contains_key(hook), "{hook}");
    }
    // The cabinet opens a chest's menu over its own slots.
    let runtime = &b.files[&format!("mixins/{}/runtime.rs", b.id)];
    assert!(
        runtime.contains("(\"grind:cabinet\", \"minecraft:generic_9x3\", 27),"),
        "{runtime}"
    );
    compiles_against_the_mock(&b, "machines");
}

#[test]
fn kinetic_helpers_compile_against_horizon_api() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-kinetic-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    for hook in [
        "block_entity_tick",
        "block_entity_submit",
        "block_entity_update_tag",
    ] {
        assert!(b.wired.contains_key(hook), "{hook}");
    }
    compiles_against_the_mock(&b, "kinetic");
}

#[test]
fn block_behaviour_helpers_compile_against_horizon_api() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../porter-java/tests/fixtures/neoforge-shutter-demo");
    let scan = porter_java::scan(&path).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    for hook in [
        "state_for_placement",
        "right_click_block",
        "update_shape",
        "use_item_on",
        "use_item",
    ] {
        assert!(b.wired.contains_key(hook), "{hook}: {:?}", b.wired.keys());
    }
    let runtime = &b.files["mixins/shutters/runtime.rs"];
    // The gate places and opens as the shutter it extends does, and joins
    // its neighbour by its own updateShape.
    assert!(runtime.contains("TODO: port from net.shutters.Shutter (`getStateForPlacement`)"));
    assert!(runtime.contains("TODO: port from net.shutters.Shutter (`useWithoutItem`)"));
    assert!(runtime.contains("TODO: port from net.shutters.Gate (`updateShape`)"));
    // The wrench turns what it is used on; the whistle blows in the air.
    assert!(
        runtime.contains("TODO: port from net.shutters.Wrench (`useOn`)"),
        "{runtime}"
    );
    assert!(runtime.contains("TODO: port from net.shutters.Whistle (`use`)"));
    assert!(runtime.contains("const WRENCH_USE_ON: &[&str] = &[\n    \"shutters:wrench\",\n];"));
    assert!(
        runtime.contains(
            "const SHUTTER_USE: &[&str] = &[\n    \"shutters:iron_gate\",\n    \"shutters:oak_shutter\",\n];"
        ),
        "{runtime}"
    );
    compiles_against_the_mock(&b, "behaviour");
}

/// Splices the bundle's helpers into the mock's hooks and runs `cargo check` on it.
fn compiles_against_the_mock(b: &bundle::Bundle, name: &str) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("porter-horizon-mock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bundle_dir = dir.join("bundle");
    bundle::write(b, &bundle_dir, false).unwrap();
    folder_compiles_against_the_mock(&bundle_dir.join("mixins").join(&b.id), &dir);
}

/// A port folder's `mixins/<id>` (its helpers and runtime) against the mock, built
/// in `dir`.
fn folder_compiles_against_the_mock(mixins: &Path, dir: &Path) {
    let mixins = mixins.to_path_buf();
    let manifest: toml::Table =
        toml::from_str(&std::fs::read_to_string(mixins.join("horizon-mixin.toml")).unwrap())
            .unwrap();
    let module = manifest["module"][0]["name"].as_str().unwrap();

    let hooks = splice(HOOKS, &manifest, &mixins);
    let level_hooks = splice(LEVEL_HOOKS, &manifest, &mixins);
    let runtime = mixins.join("runtime.rs");
    let lib = MOCK_LIB
        .replace(
            "__MODULE__",
            &format!(
                "#[path = {:?}]\n    pub mod {module};",
                runtime.to_string_lossy()
            ),
        )
        .replace("__HOOKS__", &hooks)
        .replace("__LEVEL_HOOKS__", &level_hooks);
    check_mock(&lib, dir);
}

/// The mock's hook functions for `hooks`, each with the bundle's helper for it
/// pasted in and called, as horizon-mixins does.
fn splice(hooks_list: &[(&str, &str)], manifest: &toml::Table, mixins: &Path) -> String {
    let mut hooks = String::new();
    for (name, params) in hooks_list {
        let injection = manifest["injection"].as_array().unwrap().iter().find(|i| {
            i["target"]
                .as_str()
                .unwrap()
                .ends_with(&format!("::{name}"))
        });
        let _ = writeln!(hooks, "            pub fn {name}({params}) {{");
        if let Some(inj) = injection {
            let helper = mixins.join(inj["helper"].as_str().unwrap());
            let source = std::fs::read_to_string(&helper).unwrap();
            let fn_name = source
                .split("fn ")
                .nth(1)
                .and_then(|s| s.split('(').next())
                .unwrap()
                .trim()
                .to_string();
            let args: Vec<&str> = inj["bindings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["value"].as_str().unwrap())
                .collect();
            // Pasted in, as horizon-mixins does (include! cannot add items here).
            let _ = writeln!(
                hooks,
                "{source}\n                {fn_name}({});",
                args.join(", ")
            );
        }
        hooks.push_str("            }\n");
    }
    hooks
}

/// Builds the mock crate `lib` in `dir` with `cargo check`: it must compile clean.
fn check_mock(lib: &str, dir: &Path) {
    let mock = dir.join("mock");
    std::fs::create_dir_all(mock.join("src")).unwrap();
    std::fs::write(
        mock.join("Cargo.toml"),
        "[package]\nname = \"horizon-client\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nserde_json = \"1\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(mock.join("src/lib.rs"), lib).unwrap();

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["check", "--quiet", "--offline"])
        .current_dir(&mock)
        .env("CARGO_TARGET_DIR", dir.join("target"))
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the generated helpers do not compile against the Horizon API mock:\n{stderr}"
    );
    // Warnings in generated code count as failures too.
    assert!(!stderr.contains("warning"), "{stderr}");
    std::fs::remove_dir_all(dir).ok();
}

/// A port folder `port` (or the app) wrote, as it is now, translated or not:
/// `PORTER_PORT=<folder> cargo test -p porter-translate --test horizon_mock --
/// --ignored port_folder`.
#[test]
#[ignore]
fn port_folder_compiles_against_horizon_api() {
    let folder = PathBuf::from(std::env::var("PORTER_PORT").expect("PORTER_PORT names the folder"));
    let native: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("horizon-native.json")).unwrap())
            .unwrap();
    let id = native["id"].as_str().unwrap();
    let dir =
        std::env::temp_dir().join(format!("porter-horizon-mock-folder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    folder_compiles_against_the_mock(&folder.join("mixins").join(id), &dir);
}

/// Any mod's helpers against the mock: `PORTER_MOD=<source folder or jar>` and
/// `cargo test -p porter-translate --test horizon_mock -- --ignored any_mod`. For
/// trying a real mod's port before building Horizon with it.
#[test]
#[ignore]
fn any_mod_helpers_compile_against_horizon_api() {
    let path = std::env::var("PORTER_MOD").expect("PORTER_MOD names the mod to port");
    let scan = porter_java::scan(Path::new(&path)).unwrap();
    let lang = bundle::load_lang(&scan);
    let b = bundle::generate(&scan, &lang);
    compiles_against_the_mock(&b, "any");
}
