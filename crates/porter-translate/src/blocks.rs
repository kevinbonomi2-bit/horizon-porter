//! A mod's blocks that act in code: placed by their own rule (`getStateForPlacement`),
//! used with a right click (`useItemOn`, `useWithoutItem`), reshaped as their
//! neighbours change (`updateShape`). Their template can't do what that code does
//! (Macaw's shutter picks its hinge from where it is clicked, its blinds open and
//! close, its portcullis joins the one beside it), so each block class that has such
//! a method gets a function in runtime.rs to port it into, called from Horizon's
//! `state_for_placement`, `right_click_block` and `update_shape` hooks. Until it is
//! ported the function answers nothing and the template's own behaviour stands.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use porter_model::ModScan;

/// What a block's Java does that its template can't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `getStateForPlacement`: the state it is placed in.
    Place,
    /// `useItemOn` / `useWithoutItem`: what a right click does.
    Use,
    /// `updateShape`: the state it takes when a neighbour changes.
    Shape,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Place, Kind::Use, Kind::Shape];

    /// The Java methods that make it.
    pub fn methods(self) -> &'static [&'static str] {
        match self {
            Kind::Place => &["getStateForPlacement"],
            Kind::Use => &["useItemOn", "useWithoutItem"],
            Kind::Shape => &["updateShape"],
        }
    }

    /// The Horizon hook it runs from.
    pub fn hook(self) -> &'static str {
        match self {
            Kind::Place => "state_for_placement",
            Kind::Use => "right_click_block",
            Kind::Shape => "update_shape",
        }
    }

    fn word(self) -> &'static str {
        match self {
            Kind::Place => "place",
            Kind::Use => "use",
            Kind::Shape => "shape",
        }
    }
}

/// One Java class's method of one kind, and the blocks it runs for (its own
/// and its subclasses' that don't override it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behaviour {
    pub kind: Kind,
    /// The class that defines the method, fully qualified.
    pub class: String,
    /// The methods of the kind it defines.
    pub methods: Vec<String>,
    /// The blocks it runs for: `mcwwindows:oak_shutter`.
    pub blocks: Vec<String>,
    /// Its function's name in runtime.rs: `shutter_use`.
    pub function: String,
}

/// The block classes of `blocks` (the content mod's) whose Java, or a superclass
/// of the mod's own, defines a method of each kind.
pub fn find(scan: &ModScan, ns: &str, blocks: &[porter_content::Block]) -> Vec<Behaviour> {
    let mut by: BTreeMap<(Kind, String), Vec<String>> = BTreeMap::new();
    for b in blocks {
        let Some(class) = b.class.as_deref() else {
            continue;
        };
        for kind in Kind::ALL {
            if let Some(owner) = owner(scan, class, kind) {
                by.entry((kind, owner))
                    .or_default()
                    .push(format!("{ns}:{}", b.name));
            }
        }
    }
    let mut used = BTreeSet::new();
    by.into_iter()
        .map(|((kind, class), mut blocks)| {
            // Sorted, for the hooks to find a block by binary search.
            blocks.sort();
            blocks.dedup();
            let methods = scan
                .classes
                .iter()
                .find(|c| c.name == class)
                .map(|c| {
                    kind.methods()
                        .iter()
                        .filter(|m| c.method_names.iter().any(|n| n == *m))
                        .map(|m| m.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let simple = class.rsplit(['.', '$']).next().unwrap_or(&class);
            let base = format!("{}_{}", snake(simple), kind.word());
            let mut function = base.clone();
            let mut n = 2;
            while !used.insert(function.clone()) {
                function = format!("{base}_{n}");
                n += 1;
            }
            Behaviour {
                kind,
                class,
                methods,
                blocks,
                function,
            }
        })
        .collect()
}

/// The nearest of `class` and its superclasses (the mod's own) that defines a
/// method of `kind`.
fn owner(scan: &ModScan, class: &str, kind: Kind) -> Option<String> {
    let mut name = class.to_string();
    for _ in 0..16 {
        let c = scan.classes.iter().find(|c| c.name == name)?;
        if c.method_names
            .iter()
            .any(|m| kind.methods().contains(&m.as_str()))
        {
            return Some(c.name.clone());
        }
        name = c.superclass.clone()?;
    }
    None
}

/// The hooks the behaviours need, with the classes behind each.
pub fn hooks(behaviours: &[Behaviour]) -> Vec<(&'static str, BTreeSet<String>)> {
    let mut out: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    for b in behaviours {
        out.entry(b.kind.hook())
            .or_default()
            .insert(b.class.clone());
    }
    out.into_iter().collect()
}

/// The runtime.rs part: the types the ported code reads and changes blocks
/// through, the dispatch from a block to its class's function, and a function
/// to port into for each behaviour. `blocks` are all the mod's blocks, by id.
pub fn runtime(behaviours: &[Behaviour], blocks: &[(String, porter_content::Block)]) -> String {
    if behaviours.is_empty() {
        return String::new();
    }
    let mut r = String::from(RUNTIME_TYPES);
    let ns = behaviours[0].blocks[0]
        .split_once(':')
        .map_or("", |(ns, _)| ns);
    let _ = write!(
        r,
        "\n/// The mod's blocks' namespace: the world hooks run for every block in the world,\n/// and this turns away all others with one comparison.\nconst NAMESPACE: &str = \"{ns}:\";\n"
    );

    r.push_str(
        "\n/// Quarter turns clockwise from the `facing` each block stores to the one its Java\n/// knows: its blockstate is turned so each draws what Java draws.\nconst FACING_TURNS: &[(&str, u8)] = &[\n",
    );
    for (id, b) in blocks {
        if b.facing_turns != 0 {
            let _ = writeln!(r, "    ({id:?}, {}),", b.facing_turns);
        }
    }
    r.push_str("];\n");

    // Every property the mod's blocks have, with its values: a hook that gets a
    // block state reads its properties by them.
    let mut values: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (_, b) in blocks {
        for (k, vals) in &b.states {
            values
                .entry(k.as_str())
                .or_default()
                .extend(vals.iter().map(String::as_str));
        }
    }
    r.push_str("\n/// The state properties of the mod's blocks, with their values.\npub const PROPERTY_VALUES: &[(&str, &[&str])] = &[\n");
    for (k, vals) in &values {
        let list = vals
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(r, "    ({k:?}, &[{list}]),");
    }
    r.push_str("];\n");

    for b in behaviours {
        let _ = write!(r, "\nconst {}: &[&str] = &[\n", consts(&b.function));
        for id in &b.blocks {
            let _ = writeln!(r, "    {id:?},");
        }
        r.push_str("];\n");
    }

    for kind in Kind::ALL {
        let of: Vec<&Behaviour> = behaviours.iter().filter(|b| b.kind == kind).collect();
        let (check, dispatch, sig, call, none, ret, block_of) = match kind {
            Kind::Place => (
                "places_its_own_way",
                "state_for_placement",
                "p: &Placement, world: &dyn Blocks",
                "p, world",
                "None",
                "Option<Vec<(String, String)>>",
                "p.block",
            ),
            Kind::Use => (
                "acts_on_use",
                "use_block",
                "u: &BlockUse, world: &mut dyn World",
                "u, world",
                "false",
                "bool",
                "u.block",
            ),
            Kind::Shape => (
                "shapes_by_neighbours",
                "update_shape",
                "u: &ShapeUpdate, world: &dyn Blocks",
                "u, world",
                "None",
                "Option<Vec<(String, String)>>",
                "u.block",
            ),
        };
        let _ = write!(
            r,
            "\n/// Whether `block` runs its own code from Horizon's `{hook}`.\npub fn {check}(block: &str) -> bool {{\n",
            hook = kind.hook()
        );
        if of.is_empty() {
            r.push_str("    let _ = block;\n    false\n}\n");
        } else {
            let any = of
                .iter()
                .map(|b| format!("{}.binary_search(&block).is_ok()", consts(&b.function)))
                .collect::<Vec<_>>()
                .join("\n            || ");
            let _ = writeln!(
                r,
                "    block.starts_with(NAMESPACE)\n        && ({any})\n}}"
            );
        }
        r.push_str(DISPATCH_DOCS[kind as usize]);
        let _ = writeln!(r, "pub fn {dispatch}({sig}) -> {ret} {{");
        let _ = writeln!(r, "    let block = {block_of}.as_str();");
        for b in &of {
            let _ = writeln!(
                r,
                "    if {}.binary_search(&block).is_ok() {{\n        return {}({call});\n    }}",
                consts(&b.function),
                b.function
            );
        }
        if of.is_empty() {
            let _ = writeln!(r, "    let _ = ({call}, block);");
        }
        let _ = writeln!(r, "    {none}\n}}");
        for b in &of {
            let simple = b.class.rsplit(['.', '$']).next().unwrap_or(&b.class);
            let methods = b
                .methods
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let what = match kind {
                Kind::Place => {
                    "the state its blocks are placed in: return the properties Java sets on\n/// `defaultBlockState()` (`facing` as Java names it: Horizon turns it to the one\n/// stored), or None to leave the template's placement"
                }
                Kind::Use => {
                    "what a right click on its blocks does: read and change blocks through\n/// `world`, and return true where Java returns `SUCCESS` or `CONSUME`, false for `PASS`"
                }
                Kind::Shape => {
                    "the state its blocks take as a neighbour changes: return the properties\n/// Java's `updateShape` changes (`state.setValue(..)`), or None to keep the state"
                }
            };
            let _ = write!(
                r,
                "\n/// `{simple}`'s {methods}: {what}.\n///\n/// TODO: port from {class} ({methods}).\nfn {f}({sig}) -> {ret} {{\n    let _ = {discard};\n    {none}\n}}\n",
                class = b.class,
                f = b.function,
                discard = if call.contains(',') {
                    format!("({call})")
                } else {
                    call.to_string()
                },
            );
        }
    }
    r
}

/// Tests for the runtime part.
pub fn tests(behaviours: &[Behaviour]) -> String {
    if behaviours.is_empty() {
        return String::new();
    }
    r#"
    #[test]
    fn facings_turn_back_and_forth() {
        for (block, _) in super::FACING_TURNS {
            for d in ["north", "east", "south", "west", "up"] {
                assert_eq!(super::stored_facing(block, &super::java_facing(block, d)), d);
            }
        }
    }

    #[test]
    fn clicks_land_on_the_blocks_boxes() {
        // Looking straight down at a thin panel against the west side of block (4, 0, 7).
        let panel = [[0.0, 0.0, 0.0, 0.125, 1.0, 1.0]];
        let hit = super::ray_hit([4.05, 2.0, 7.5], [0.0, -1.0, 0.0], [4, 0, 7], [0, 1, 0], &panel);
        assert!((hit[1] - 1.0).abs() < 1e-9 && (hit[0] - 4.05).abs() < 1e-9, "{hit:?}");
        // Missing every box: the middle of the face clicked.
        let miss = super::ray_hit([4.9, 2.0, 7.5], [0.0, -1.0, 0.0], [4, 0, 7], [0, 1, 0], &panel);
        assert_eq!(miss, [4.5, 1.0, 7.5]);
    }

    #[test]
    fn placement_knows_where_the_player_looks() {
        let mut p = super::Placement {
            block: String::new(),
            pos: [0, 0, 0],
            clicked: [0, -1, 0],
            face: "up".into(),
            hit: [0.5, 0.0, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            sneaking: false,
            replace_clicked: false,
        };
        assert_eq!(p.horizontal_direction(), "south");
        p.yaw = 90.0;
        assert_eq!(p.horizontal_direction(), "west");
        p.yaw = -180.0;
        assert_eq!(p.horizontal_direction(), "north");
        p.yaw = 270.0;
        assert_eq!(p.horizontal_direction(), "east");
    }
"#
    .into()
}

/// The helper for one of the block hooks: (file name, source).
pub fn helper(hook: &str, id: &str, m: &str, name: &str) -> Option<(&'static str, String)> {
    let h = porter_map::hooks::hook(hook)?;
    let params = h
        .bindings()?
        .iter()
        .map(|b| format!("{}: {}", b.name, b.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let (file, body) = match hook {
        "right_click_block" => ("right_click_block.rs", USE_HELPER),
        "state_for_placement" => ("state_for_placement.rs", PLACE_HELPER),
        "update_shape" => ("update_shape.rs", SHAPE_HELPER),
        _ => return None,
    };
    let body = body
        .replace("{name}", name)
        .replace("{id}", id)
        .replace("{m}", m)
        .replace("{params}", &params);
    Some((file, body))
}

const USE_HELPER: &str = r#"// {name}'s blocks that act on a right click (`useItemOn`, `useWithoutItem`): runtime.rs's
// `use_block` runs their code; a click it takes runs neither the template's use nor the item's.
fn {id}_right_click_block({params}) {
    use crate::game::{m} as m;
    if *canceled || !game.native_on(m::ID) {
        return;
    }
    let block = game.native_block(pos);
    if !m::acts_on_use(&block) {
        return;
    }
    let held = game
        .native_held()
        .map_or_else(|| "minecraft:air".to_string(), |(item, _, _)| item);
    // Where the click landed: the player's look from their eyes into the block's boxes.
    let hit = match game.native_acting_player() {
        Some(p) => {
            let eye = [p.position[0], p.position[1] + 1.62, p.position[2]];
            let (yaw, pitch) = (f64::from(p.yaw).to_radians(), f64::from(p.pitch).to_radians());
            let look = [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()];
            m::ray_hit(eye, look, pos, face, &game.native_block_collision(pos))
        }
        None => m::ray_hit([0.0; 3], [0.0; 3], pos, face, &[]),
    };
    // The world as the block's code sees it, `facing` as Java names it.
    struct Here<'g, 'a> {
        game: &'g mut crate::game::Game<'a>,
    }
    impl m::Blocks for Here<'_, '_> {
        fn block(&self, pos: [i32; 3]) -> String {
            self.game.native_block(pos)
        }
        fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
            let value = self.game.native_block_property(pos, key)?;
            Some(if key == "facing" {
                m::java_facing(&self.game.native_block(pos), &value)
            } else {
                value
            })
        }
    }
    impl m::World for Here<'_, '_> {
        fn set_block(&mut self, pos: [i32; 3], block: &str, properties: &[(&str, &str)]) -> bool {
            // `block.defaultBlockState().setValue(..)`, from the registry Horizon's
            // own `native_set_block` reads (no helper names a block's states yet).
            let state = {
                let registry = &self.game.pack.assets.registry;
                let Some(mut state) = registry.default_state(block) else {
                    return false;
                };
                for (k, v) in properties {
                    let v = if *k == "facing" {
                        m::stored_facing(block, v)
                    } else {
                        v.to_string()
                    };
                    if let Some(next) = with_property(registry, state, k, &v) {
                        state = next;
                    }
                }
                state
            };
            let dimension = self.game.native_dimension();
            // Level.setBlock(pos, state, Block.UPDATE_ALL): neighbours hear it.
            self.game.native_set_block(dimension, pos, state, 3)
        }
        fn random(&mut self) -> f32 {
            let dimension = self.game.native_dimension();
            self.game
                .native_level_random(dimension)
                .map_or(0.5, horizon_core::random::RandomSource::next_float)
        }
        fn set_property(&mut self, pos: [i32; 3], key: &str, value: &str) -> bool {
            let value = if key == "facing" {
                m::stored_facing(&self.game.native_block(pos), value)
            } else {
                value.to_string()
            };
            // Level.setBlock(pos, state, Block.UPDATE_ALL)
            self.game.native_set_block_property_with(pos, key, &value, 3)
        }
        fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32) {
            let at = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ];
            // SoundSource.BLOCKS
            let _ = self.game.native_level_play_sound(sound, at, 4, volume, pitch);
        }
    }
    let used = m::BlockUse {
        block,
        pos,
        face,
        hit,
        held,
    };
    if m::use_block(&used, &mut Here { game }) {
        *canceled = true;
    }
}
"#;

const PLACE_HELPER: &str = r#"// {name}'s blocks placed by their own code (`getStateForPlacement`): runtime.rs's
// `state_for_placement` gives the properties; left empty, the template places them.
fn {id}_state_for_placement({params}) {
    use crate::game::{m} as m;
    let _ = (dimension, fluid);
    if answer.is_some() {
        return;
    }
    let Some(block) = game.native_state_block(*state) else {
        return;
    };
    if !m::places_its_own_way(block) {
        return;
    }
    // The level as the block's code reads it, `facing` as Java names it.
    struct Level<'g, 'a> {
        game: &'g crate::game::native_hooks::GameLevel<'a>,
    }
    impl m::Blocks for Level<'_, '_> {
        fn block(&self, pos: [i32; 3]) -> String {
            let state = horizon_client::mod_api::LevelHost::block_state(self.game, pos);
            self.game
                .native_state_block(state)
                .unwrap_or("minecraft:air")
                .to_string()
        }
        fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
            let state = horizon_client::mod_api::LevelHost::block_state(self.game, pos);
            let block = self.game.native_state_block(state)?;
            let (_, values) = m::PROPERTY_VALUES.iter().find(|(k, _)| *k == key)?;
            let value = values
                .iter()
                .find(|v| self.game.native_state_with_property(state, key, v) == Some(state))?;
            Some(if key == "facing" {
                m::java_facing(block, value)
            } else {
                value.to_string()
            })
        }
    }
    let placement = m::Placement {
        block: block.to_string(),
        pos: [context.pos.x, context.pos.y, context.pos.z],
        clicked: [context.hit_pos.x, context.hit_pos.y, context.hit_pos.z],
        face: format!("{:?}", context.face).to_lowercase(),
        hit: [context.hit.x, context.hit.y, context.hit.z],
        yaw: context.yaw,
        pitch: context.pitch,
        sneaking: context.sneaking,
        replace_clicked: context.replace_clicked,
    };
    let Some(set) = m::state_for_placement(&placement, &Level { game }) else {
        return;
    };
    let mut placed = *state;
    for (key, value) in set {
        let value = if key == "facing" {
            m::stored_facing(block, &value)
        } else {
            value
        };
        if let Some(next) = game.native_state_with_property(placed, &key, &value) {
            placed = next;
        }
    }
    *answer = Some(Some(placed));
}
"#;

const SHAPE_HELPER: &str = r#"// {name}'s blocks that change with their neighbours (`updateShape`): runtime.rs's
// `update_shape` gives the properties that change; left empty, the template's rule runs.
fn {id}_update_shape({params}) {
    use crate::game::{m} as m;
    let _ = (ticks, dimension);
    if new.is_some() {
        return;
    }
    let Some(block) = game.native_state_block(*state) else {
        return;
    };
    if !m::shapes_by_neighbours(block) {
        return;
    }
    // The level as the block's code reads it, `facing` as Java names it.
    struct Level<'g, 'a> {
        game: &'g crate::game::native_hooks::GameLevel<'a>,
    }
    impl m::Blocks for Level<'_, '_> {
        fn block(&self, pos: [i32; 3]) -> String {
            let state = horizon_client::mod_api::LevelHost::block_state(self.game, pos);
            self.game
                .native_state_block(state)
                .unwrap_or("minecraft:air")
                .to_string()
        }
        fn property(&self, pos: [i32; 3], key: &str) -> Option<String> {
            let state = horizon_client::mod_api::LevelHost::block_state(self.game, pos);
            let block = self.game.native_state_block(state)?;
            let (_, values) = m::PROPERTY_VALUES.iter().find(|(k, _)| *k == key)?;
            let value = values
                .iter()
                .find(|v| self.game.native_state_with_property(state, key, v) == Some(state))?;
            Some(if key == "facing" {
                m::java_facing(block, value)
            } else {
                value.to_string()
            })
        }
    }
    // A state's properties: each value of the mod's properties that, set on it,
    // leaves it as it is.
    let properties = |of, name: &str| -> Vec<(String, String)> {
        m::PROPERTY_VALUES
            .iter()
            .filter_map(|(key, values)| {
                let value = values
                    .iter()
                    .find(|v| game.native_state_with_property(of, key, v) == Some(of))?;
                let value = if *key == "facing" {
                    m::java_facing(name, value)
                } else {
                    value.to_string()
                };
                Some((key.to_string(), value))
            })
            .collect()
    };
    let neighbour_block = game
        .native_state_block(*neighbour)
        .unwrap_or("minecraft:air")
        .to_string();
    let update = m::ShapeUpdate {
        block: block.to_string(),
        pos: [pos.x, pos.y, pos.z],
        properties: properties(*state, block),
        direction: format!("{direction:?}").to_lowercase(),
        neighbour_pos: [neighbour_pos.x, neighbour_pos.y, neighbour_pos.z],
        neighbour_properties: properties(*neighbour, &neighbour_block),
        neighbour: neighbour_block,
    };
    let Some(set) = m::update_shape(&update, &Level { game }) else {
        return;
    };
    let mut shaped = *state;
    for (key, value) in set {
        let value = if key == "facing" {
            m::stored_facing(block, &value)
        } else {
            value
        };
        if let Some(next) = game.native_state_with_property(shaped, &key, &value) {
            shaped = next;
        }
    }
    *new = Some(shaped);
}
"#;

const DISPATCH_DOCS: [&str; 3] = [
    "\n/// The properties a block is placed with (Java's `getStateForPlacement` setting them\n/// on `defaultBlockState()`), from its class's function. None leaves its template's\n/// placement.\n",
    "\n/// A right click on a block, run by its class's function. True when it took the click\n/// (`InteractionResult.SUCCESS` or `CONSUME`): neither its template's use nor the held\n/// item's runs then.\n",
    "\n/// The properties a block changes as its neighbour in `u.direction` changes (Java's\n/// `updateShape`), from its class's function. None keeps it as it is.\n",
];

const RUNTIME_TYPES: &str = r#"
// The mod's blocks that act in code. Their functions below are what their Java
// classes' `getStateForPlacement`, `useItemOn` and `updateShape` do; the hook
// helpers call them. Directions are named as Java names them: `north`, `up`.

/// The world as a block's code reads it (Java's `LevelReader`): where it is
/// placed, or as a neighbour changes.
pub trait Blocks {
    /// The block at `pos`: `mymod:oak_shutter`, `minecraft:air`
    /// (`level.getBlockState(pos).getBlock()`).
    fn block(&self, pos: [i32; 3]) -> String;
    /// Property `key` of the block at `pos` (`level.getBlockState(pos).getValue(..)`):
    /// one of the properties the mod's blocks have, `facing` as Java names it.
    fn property(&self, pos: [i32; 3], key: &str) -> Option<String>;
}

/// The world as a block's code reads and changes it on a right click (Java's
/// `Level`).
pub trait World: Blocks {
    /// Sets property `key` of the block at `pos` (`level.setBlock(pos,
    /// state.setValue(..), 3)`); false when it has no such property or value.
    fn set_property(&mut self, pos: [i32; 3], key: &str, value: &str) -> bool;
    /// Plays `sound` (`minecraft:block.wooden_door.open`, or one of the mod's own)
    /// at the middle of `pos` for the players near it (`level.playSound`).
    fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32);
    /// The level's random, 0 to 1 (`level.random.nextFloat()`).
    fn random(&mut self) -> f32;
    /// Puts `block` at `pos` with `properties` (`level.setBlock(pos,
    /// block.defaultBlockState().setValue(..), 3)`); `minecraft:air` removes what
    /// is there. Its neighbours are not told. False when it couldn't.
    fn set_block(&mut self, pos: [i32; 3], block: &str, properties: &[(&str, &str)]) -> bool;
}

/// A right click on one of the mod's blocks (`useItemOn`'s arguments). Horizon
/// doesn't say whether the player sneaks (`player.isCrouching()`).
pub struct BlockUse {
    pub block: String,
    pub pos: [i32; 3],
    /// The face clicked: `[0, 1, 0]` is the top (`hit.getDirection()`).
    pub face: [i32; 3],
    /// Where the click landed, in world coordinates (`hit.getLocation()`).
    pub hit: [f64; 3],
    /// The item in the player's main hand, `minecraft:air` when empty.
    pub held: String,
}

/// Where a look from `eye` along `look` first meets `boxes` (block-local, of the
/// block at `pos`): Java's `BlockHitResult.getLocation()`. Without boxes, the
/// whole block; without a meeting, the middle of `face`.
pub fn ray_hit(eye: [f64; 3], look: [f64; 3], pos: [i32; 3], face: [i32; 3], boxes: &[[f64; 6]]) -> [f64; 3] {
    let whole = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
    let boxes = if boxes.is_empty() { &whole[..] } else { boxes };
    let base = pos.map(f64::from);
    let mut best: Option<f64> = None;
    for b in boxes {
        let (mut near, mut far) = (f64::NEG_INFINITY, f64::INFINITY);
        let mut misses = false;
        for i in 0..3 {
            let (lo, hi) = (base[i] + b[i], base[i] + b[i + 3]);
            if look[i].abs() < 1e-9 {
                if eye[i] < lo || eye[i] > hi {
                    misses = true;
                }
                continue;
            }
            let (a, c) = ((lo - eye[i]) / look[i], (hi - eye[i]) / look[i]);
            near = near.max(a.min(c));
            far = far.min(a.max(c));
        }
        if !misses && near <= far && far >= 0.0 {
            let t = near.max(0.0);
            best = Some(best.map_or(t, |b: f64| b.min(t)));
        }
    }
    match best {
        Some(t) => [0, 1, 2].map(|i| eye[i] + look[i] * t),
        None => [0, 1, 2].map(|i| base[i] + 0.5 + f64::from(face[i]) * 0.5),
    }
}

/// Where and how one of the mod's blocks is being placed (`BlockPlaceContext`).
pub struct Placement {
    pub block: String,
    /// Where it goes (`getClickedPos`).
    pub pos: [i32; 3],
    /// The block that was clicked (`getHitResult().getBlockPos()`).
    pub clicked: [i32; 3],
    /// The face clicked (`getClickedFace`): `up`, `north`...
    pub face: String,
    /// Where the click landed, in world coordinates (`getClickLocation`).
    pub hit: [f64; 3],
    /// The placer's rotation in degrees (`getRotation`, `getPlayer().getXRot()`).
    pub yaw: f32,
    pub pitch: f32,
    /// The placer is sneaking (`isSecondaryUseActive`).
    pub sneaking: bool,
    /// The clicked block is being replaced (`replacingClickedOnBlock`).
    pub replace_clicked: bool,
}

impl Placement {
    /// The way the placer looks, flat (`getHorizontalDirection`, Java's
    /// `Direction.fromYRot`): `north`, `east`, `south` or `west`.
    pub fn horizontal_direction(&self) -> &'static str {
        let i = ((f64::from(self.yaw) / 90.0 + 0.5).floor() as i64).rem_euclid(4);
        ["south", "west", "north", "east"][i as usize]
    }
}

/// A neighbour of one of the mod's blocks changed (`updateShape`'s arguments).
pub struct ShapeUpdate {
    pub block: String,
    pub pos: [i32; 3],
    /// The block's state properties, `facing` as Java names it.
    pub properties: Vec<(String, String)>,
    /// Where the neighbour is: `north`, `up`...
    pub direction: String,
    pub neighbour_pos: [i32; 3],
    /// The neighbour's block (`minecraft:air`) and the properties of it the
    /// mod's blocks also have.
    pub neighbour: String,
    pub neighbour_properties: Vec<(String, String)>,
}

impl ShapeUpdate {
    /// One of the block's properties (`state.getValue(..)`).
    pub fn property(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    /// One of the neighbour's (`neighborState.getValue(..)`).
    pub fn neighbour_property(&self, key: &str) -> Option<&str> {
        self.neighbour_properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// The `facing` Java's code knows for the one `block` stores.
pub fn java_facing(block: &str, stored: &str) -> String {
    turn_facing(stored, facing_turns(block))
}

/// The `facing` `block` stores for the one Java's code names.
pub fn stored_facing(block: &str, java: &str) -> String {
    turn_facing(java, (4 - facing_turns(block)) % 4)
}

fn facing_turns(block: &str) -> u8 {
    FACING_TURNS
        .iter()
        .find(|(b, _)| *b == block)
        .map_or(0, |(_, t)| *t)
}

fn turn_facing(facing: &str, turns: u8) -> String {
    const RING: [&str; 4] = ["north", "east", "south", "west"];
    match RING.iter().position(|d| *d == facing) {
        Some(i) => RING[(i + turns as usize) % 4].to_string(),
        None => facing.to_string(),
    }
}
"#;

/// `ConnectedWindow` → `connected_window`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    // One underscore between words (`Bridge_Block` is `bridge_block`), none at the ends.
    let mut joined = String::new();
    for part in out.split('_').filter(|p| !p.is_empty()) {
        if !joined.is_empty() {
            joined.push('_');
        }
        joined.push_str(part);
    }
    joined
}

/// The const naming a behaviour's blocks: `SHUTTER_USE`.
fn consts(function: &str) -> String {
    function.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_functions_after_classes() {
        assert_eq!(snake("ConnectedWindow"), "connected_window");
        assert_eq!(snake("Bridge_Block"), "bridge_block");
        assert_eq!(snake("XMLBlock"), "x_m_l_block");
        assert_eq!(consts("shutter_use"), "SHUTTER_USE");
    }
}
