//! A mod's blocks and items that act in code: blocks placed by their own rule
//! (`getStateForPlacement`), used with a right click (`useItemOn`, `useWithoutItem`),
//! reshaped as their neighbours change (`updateShape`); items used on a block
//! (`useOn`) or in the air (`use`). Their template can't do what that code does
//! (Macaw's shutter picks its hinge from where it is clicked, its blinds open and
//! close, a wrench turns the block it is used on), so each class that has such a
//! method gets a function in runtime.rs to port it into, called from Horizon's
//! `state_for_placement`, `right_click_block`, `update_shape`, `use_item_on` and
//! `use_item` hooks. Until it is ported the function answers nothing and the
//! template's own behaviour stands.

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
    /// An item's `useOn`: what it does used on a block.
    ItemUseOn,
    /// An item's `use`: what it does used in the air (or on a block its `useOn`
    /// passed).
    ItemUse,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Place,
        Kind::Use,
        Kind::Shape,
        Kind::ItemUseOn,
        Kind::ItemUse,
    ];
    const BLOCK: [Kind; 3] = [Kind::Place, Kind::Use, Kind::Shape];
    const ITEM: [Kind; 2] = [Kind::ItemUseOn, Kind::ItemUse];

    /// The Java methods that make it.
    pub fn methods(self) -> &'static [&'static str] {
        match self {
            Kind::Place => &["getStateForPlacement"],
            Kind::Use => &["useItemOn", "useWithoutItem"],
            Kind::Shape => &["updateShape"],
            Kind::ItemUseOn => &["useOn"],
            Kind::ItemUse => &["use"],
        }
    }

    /// The Horizon hook it runs from.
    pub fn hook(self) -> &'static str {
        match self {
            Kind::Place => "state_for_placement",
            Kind::Use => "right_click_block",
            Kind::Shape => "update_shape",
            Kind::ItemUseOn => "use_item_on",
            Kind::ItemUse => "use_item",
        }
    }

    fn word(self) -> &'static str {
        match self {
            Kind::Place => "place",
            Kind::Use => "use",
            Kind::Shape => "shape",
            Kind::ItemUseOn => "use_on",
            Kind::ItemUse => "use",
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
    /// The blocks (or items, for an item's kinds) it runs for:
    /// `mcwwindows:oak_shutter`.
    pub blocks: Vec<String>,
    /// Its function's name in runtime.rs: `shutter_use`.
    pub function: String,
}

/// The classes of `blocks` and `items` (the content mod's) whose Java, or a
/// superclass of the mod's own, defines a method of each kind.
pub fn find(
    scan: &ModScan,
    ns: &str,
    blocks: &[porter_content::Block],
    items: &[porter_content::Item],
) -> Vec<Behaviour> {
    let mut by: BTreeMap<(Kind, String), Vec<String>> = BTreeMap::new();
    let classes = blocks
        .iter()
        .map(|b| (&b.name, b.class.as_deref(), &Kind::BLOCK[..]))
        .chain(
            items
                .iter()
                .map(|i| (&i.name, i.class.as_deref(), &Kind::ITEM[..])),
        );
    for (name, class, kinds) in classes {
        let Some(class) = class else {
            continue;
        };
        for &kind in kinds {
            if let Some(owner) = owner(scan, class, kind) {
                by.entry((kind, owner))
                    .or_default()
                    .push(format!("{ns}:{name}"));
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
///
/// `entities`: the mod has block entities (runtime.rs has their `Saved` values), and
/// the code that reads and changes blocks reaches them too.
pub fn runtime(
    behaviours: &[Behaviour],
    blocks: &[(String, porter_content::Block)],
    entities: bool,
) -> String {
    if behaviours.is_empty() {
        return String::new();
    }
    let mut r = if entities {
        RUNTIME_TYPES.replace(
            "pub trait World: Blocks {",
            "pub trait World: Blocks + BlockEntities {",
        ) + ENTITY_TYPES
    } else {
        RUNTIME_TYPES.to_string()
    };
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
            Kind::ItemUseOn => (
                "item_acts_on_block",
                "use_item_on",
                "u: &ItemUse, world: &mut dyn World",
                "u, world",
                "false",
                "bool",
                "u.item",
            ),
            Kind::ItemUse => (
                "item_acts_in_air",
                "use_item",
                "u: &ItemUse, world: &mut dyn World",
                "u, world",
                "false",
                "bool",
                "u.item",
            ),
        };
        let _ = write!(
            r,
            "\n/// Whether `block` (or the item) runs its own code from Horizon's `{hook}`.\npub fn {check}(block: &str) -> bool {{\n",
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
                    "what a right click on its blocks does: read and change blocks through\n/// `world`, and return true where Java returns `SUCCESS` or `CONSUME`, false for `PASS`.\n/// Where Java opens a menu (`player.openMenu(..)`), return false: the click goes on to\n/// the block's template (a barrel, a furnace), which opens the block's menu itself; true\n/// would stop it"
                }
                Kind::Shape => {
                    "the state its blocks take as a neighbour changes: return the properties\n/// Java's `updateShape` changes (`state.setValue(..)`), or None to keep the state"
                }
                Kind::ItemUseOn => {
                    "what its items do used on a block (`u.on`): read and change blocks and\n/// the stack in hand through `world`, and return true where Java returns `SUCCESS` or\n/// `CONSUME`, false for `PASS` (the item's `use` runs next)"
                }
                Kind::ItemUse => {
                    "what its items do used in the air: change blocks and the stack in hand\n/// through `world`, and return true where Java's `InteractionResult` is `SUCCESS` or\n/// `CONSUME`, false for `PASS`"
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
pub fn helper(
    hook: &str,
    id: &str,
    m: &str,
    name: &str,
    entities: bool,
) -> Option<(&'static str, String)> {
    let h = porter_map::hooks::hook(hook)?;
    let params = h
        .bindings()?
        .iter()
        .map(|b| format!("{}: {}", b.name, b.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let (file, body) = match hook {
        "right_click_block" => ("right_click_block.rs", USE_HELPER),
        "use_item_on" => ("use_item_on.rs", ITEM_USE_ON_HELPER),
        "use_item" => ("use_item.rs", ITEM_USE_HELPER),
        "state_for_placement" => ("state_for_placement.rs", PLACE_HELPER),
        "update_shape" => ("update_shape.rs", SHAPE_HELPER),
        _ => return None,
    };
    let body = body
        .replace("{world}", ITEM_WORLD)
        .replace(
            "{entities}",
            &if entities {
                ENTITY_WORLD
                    .replace("__FROM_TAG__", crate::block_entity::FROM_TAG)
                    .replace("__TO_TAG__", crate::block_entity::TO_TAG)
            } else {
                String::new()
            },
        )
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
    // Horizon asks once per hand. In Java a click the block takes never reaches the
    // off hand; here the client doesn't know the main hand's was taken and sends the
    // off hand's too, which would run the block's code a second time (a garage door
    // lowered, then raised straight back). The block's code runs for the main hand
    // only, and its off hand's click is taken without it.
    if game.hand != horizon_client::first_person::Hand::Main {
        *canceled = true;
        return;
    }
    let (held, held_count) = game
        .native_held()
        .map_or_else(|| ("minecraft:air".to_string(), 0), |(item, count, _)| (item, count));
    // Where the click landed: the player's look from their eyes into the block's boxes.
    let player = game.native_acting_player();
    let hit = match &player {
        Some(p) => {
            let eye = [p.position[0], p.position[1] + 1.62, p.position[2]];
            let (yaw, pitch) = (f64::from(p.yaw).to_radians(), f64::from(p.pitch).to_radians());
            let look = [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()];
            m::ray_hit(eye, look, pos, face, &game.native_block_collision(pos))
        }
        None => m::ray_hit([0.0; 3], [0.0; 3], pos, face, &[]),
    };
    let facing = m::yaw_direction(player.as_ref().map_or(0.0, |p| p.yaw)).to_string();
    let creative = player.is_some_and(|p| p.game_mode == 1);
    let used = m::BlockUse {
        block,
        pos,
        face,
        hit,
        held,
        held_count,
        facing,
        creative,
    };
    // The world as the block's code sees it, `facing` as Java names it.
{world}    if m::use_block(&used, &mut Here { game, creative, source: 4 }) {
        *canceled = true;
    }
}
"#;

/// The world and the acting player, as a block's click or an item's use sees them:
/// blocks, sounds, the level's random and the stack in hand. Shared by the three
/// use helpers.
const ITEM_WORLD: &str = r#"    struct Here<'g, 'a> {
        game: &'g mut crate::game::Game<'a>,
        creative: bool,
        // Its sounds' SoundSource: BLOCKS (4) for a block's click, PLAYERS (7) an item's.
        source: usize,
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
        fn set_block_flags(
            &mut self,
            pos: [i32; 3],
            block: &str,
            properties: &[(&str, &str)],
            flags: i32,
        ) -> bool {
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
            self.game.native_set_block(dimension, pos, state, flags)
        }
        fn random(&mut self) -> f32 {
            let dimension = self.game.native_dimension();
            self.game
                .native_level_random(dimension)
                .map_or(0.5, horizon_core::random::RandomSource::next_float)
        }
        fn set_property_flags(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {
            let value = if key == "facing" {
                m::stored_facing(&self.game.native_block(pos), value)
            } else {
                value.to_string()
            };
            self.game.native_set_block_property_with(pos, key, &value, flags)
        }
        fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32) {
            let at = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 0.5,
                f64::from(pos[2]) + 0.5,
            ];
            let _ = self.game.native_level_play_sound(sound, at, self.source, volume, pitch);
        }
        fn shrink_held(&mut self, n: u32) {
            // The hand's own slot (game.rs `held_mut`): the acting player's, swapped in.
            let slot = self.game.held_mut();
            if let Some(stack) = slot.as_mut() {
                stack.count = stack.count.saturating_sub(n);
                if stack.count == 0 {
                    *slot = None;
                }
            }
        }
        fn hurt_held(&mut self, n: u32) -> bool {
            if self.creative {
                return false;
            }
            let Some(item) = self.game.held_mut().as_ref().map(|s| s.item.clone()) else {
                return false;
            };
            let max = self.game.res.items().get(&item).map_or(0, |i| i.max_damage);
            if max == 0 {
                return false;
            }
            let slot = self.game.held_mut();
            let Some(stack) = slot.as_mut() else {
                return false;
            };
            stack.damage += n;
            if stack.damage >= max {
                *slot = None;
                return true;
            }
            false
        }
        fn give(&mut self, item: &str, count: u32) -> u32 {
            self.game.native_give_items(item, count)
        }
        fn set_held(&mut self, item: &str, count: u32) {
            *self.game.held_mut() = (count > 0 && item != "minecraft:air")
                .then(|| horizon_client::items::ItemStack::new(item, count));
        }
        fn particles(&mut self, kind: &str, at: [f64; 3], spread: [f64; 3], speed: f64, count: i32) {
            let options = if kind.contains(':') && !kind.starts_with("minecraft:") {
                Some(ParticleOptions::named(kind))
            } else {
                ParticleType::from_name(kind).map(ParticleOptions::simple)
            };
            if let Some(options) = options {
                let _ = self.game.native_send_particles(&options, None, at, spread, speed, count);
            }
        }
        fn item_is(&mut self, item: &str, tag: &str) -> bool {
            // A `#tag` ingredient, as a recipe names one, lists the tag's items.
            let tag = String::from('#') + tag.trim_start_matches('#');
            self.game
                .native_recipe_ingredient(&serde_json::Value::String(tag))
                .is_some_and(|i| i.order.iter().any(|x| x == item))
        }
    }
{entities}"#;

/// The block entities' part of the shared world, for a mod that has them.
const ENTITY_WORLD: &str = r#"    impl m::BlockEntities for Here<'_, '_> {
        fn entity(&mut self, pos: [i32; 3]) -> Option<m::EntityValues> {
            use horizon_world::nbt::Tag;
            let read = |tag: &Tag| -> Option<m::Saved> { __FROM_TAG__ };
            self.game.native_with_mod_block_entity(pos, |e| {
                m::EntityValues(
                    e.nbt
                        .iter()
                        .filter_map(|(k, tag)| read(tag).map(|v| (k.to_string_lossy(), v)))
                        .collect(),
                )
            })
        }
        fn save_entity(&mut self, pos: [i32; 3], values: &[(&str, m::Saved)]) -> bool {
            use horizon_world::nbt::{JavaString, Tag};
            let saved = self
                .game
                .native_with_mod_block_entity(pos, |e| {
                    for (key, value) in values {
                        let value = value.clone();
                        let tag = __TO_TAG__;
                        e.nbt.insert(JavaString::from(*key), tag);
                    }
                })
                .is_some();
            if saved {
                self.game.native_block_entity_sync(pos);
            }
            saved
        }
        fn entity_slots(&mut self, pos: [i32; 3]) -> usize {
            self.game
                .native_with_mod_block_entity(pos, |e| e.items.len())
                .unwrap_or(0)
        }
        fn entity_item(&mut self, pos: [i32; 3], slot: usize) -> Option<(String, u32)> {
            self.game
                .native_with_mod_block_entity(pos, |e| {
                    e.items
                        .get(slot)
                        .and_then(Option::as_ref)
                        .map(|s| (s.item.to_string(), s.count))
                })
                .flatten()
        }
        fn set_entity_item(&mut self, pos: [i32; 3], slot: usize, item: Option<(&str, u32)>) -> bool {
            let set = self
                .game
                .native_with_mod_block_entity(pos, |e| {
                    let Some(cell) = e.items.get_mut(slot) else {
                        return false;
                    };
                    *cell = item
                        .filter(|(_, count)| *count > 0)
                        .map(|(id, count)| horizon_client::items::ItemStack::new(id, count));
                    true
                })
                .unwrap_or(false);
            if set {
                self.game.native_block_entity_sync(pos);
            }
            set
        }
        fn recipes(&mut self, kind: &str) -> Vec<m::Recipe> {
            let game = &*self.game;
            let items = |v: serde_json::Value| -> Vec<String> {
                game.native_recipe_ingredient(&v).map(|i| i.order).unwrap_or_default()
            };
            // Cooking types: vanilla's and the mod's own recipes of them, from the
            // table the porter wrote (Horizon's lookup serves the mods' own types).
            if m::COOKING.iter().any(|(k, ..)| *k == kind) {
                return m::COOKING
                    .iter()
                    .filter(|(k, ..)| *k == kind)
                    .map(|(_, input, result, count, ticks)| m::Recipe {
                        id: format!("{kind}/{result}"),
                        ingredients: vec![items(serde_json::Value::String((*input).into()))],
                        result: Some(m::Stack::new(result, *count)),
                        values: vec![("cookingtime".into(), ticks.to_string())],
                    })
                    .collect();
            }
            // An ingredient as recipe files write it: an item, a `#tag`, `{"item": ..}`,
            // `{"tag": ..}`, or a list of them.
            fn names(v: &serde_json::Value) -> Option<serde_json::Value> {
                use serde_json::Value;
                let one = |v: &Value| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(o) => o
                        .get("item")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| o.get("tag").and_then(Value::as_str).map(|t| String::from('#') + t)),
                    _ => None,
                };
                match v {
                    Value::Array(a) => Some(Value::Array(a.iter().filter_map(one).map(Value::String).collect())),
                    other => one(other).map(Value::String),
                }
            }
            game.native_recipes_of_type(kind)
                .iter()
                .map(|r| {
                    let json = &*r.json;
                    let mut ingredients = Vec::new();
                    for key in ["ingredient", "input"] {
                        if let Some(v) = json.get(key).and_then(names) {
                            ingredients.push(items(v));
                        }
                    }
                    for key in ["ingredients", "inputs"] {
                        if let Some(serde_json::Value::Array(list)) = json.get(key) {
                            ingredients.extend(list.iter().filter_map(names).map(&items));
                        }
                    }
                    // `"result": "id"`, `{"id": ..}`, `{"item": ..}`, or a list of them
                    // (Farmer's Delight's cutting gives several): the first.
                    let result = json.get("result").and_then(|v| {
                        let v = match v {
                            serde_json::Value::Array(a) => a.first()?.clone(),
                            other => other.clone(),
                        };
                        let v = match v {
                            serde_json::Value::String(s) => serde_json::json!({ "id": s }),
                            serde_json::Value::Object(o) if !o.contains_key("id") => match o.get("item") {
                                Some(serde_json::Value::Object(inner)) => serde_json::Value::Object(inner.clone()),
                                Some(item) => {
                                    let mut o = o.clone();
                                    o.remove("item");
                                    o.insert("id".into(), item.clone());
                                    serde_json::Value::Object(o)
                                }
                                None => serde_json::Value::Object(o),
                            },
                            other => other,
                        };
                        let stack = game.native_recipe_result(&v)?;
                        Some(m::Stack::new(&stack.item, stack.count))
                    });
                    let values = json
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter_map(|(k, v)| match v {
                            serde_json::Value::String(s) => Some((k.clone(), s.clone())),
                            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Some((k.clone(), v.to_string())),
                            _ => None,
                        })
                        .collect();
                    m::Recipe {
                        id: r.id.clone(),
                        ingredients,
                        result,
                        values,
                    }
                })
                .collect()
        }
    }
"#;

/// The block entities' part of the world, for a mod that has them.
const ENTITY_TYPES: &str = r#"
/// A block entity's saved values, as `world.entity(pos)` reads them: its struct
/// loads from them (`SinkBlockEntity::load(|k| values.get(k))`).
pub struct EntityValues(pub Vec<(String, Saved)>);

impl EntityValues {
    /// The value saved under `key`.
    pub fn get(&self, key: &str) -> Option<Saved> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }
}

/// The mod's block entities, as a click reads and changes them (Java's
/// `level.getBlockEntity(pos)`).
pub trait BlockEntities {
    /// The saved values of the mod's block entity at `pos`, None when there is none
    /// (Java's `instanceof` check failing): load its struct from them.
    fn entity(&mut self, pos: [i32; 3]) -> Option<EntityValues>;
    /// Saves `values` into the block entity at `pos` (its struct's `save()` after a
    /// change, then Java's `setChanged()`), and sends it to the players; false when
    /// there is none.
    fn save_entity(&mut self, pos: [i32; 3], values: &[(&str, Saved)]) -> bool;
    /// How many item slots it has (`getContainerSize()`), 0 for none.
    fn entity_slots(&mut self, pos: [i32; 3]) -> usize;
    /// The stack in slot `slot` (`getItem(slot)`): (item, count), None when empty.
    fn entity_item(&mut self, pos: [i32; 3], slot: usize) -> Option<(String, u32)>;
    /// Puts `item` in slot `slot` (`setItem`), None or a count of 0 empties it; false
    /// when there is no such slot.
    fn set_entity_item(&mut self, pos: [i32; 3], slot: usize, item: Option<(&str, u32)>) -> bool;
    /// The recipes of type `kind` (`farmersdelight:cutting`, `minecraft:campfire_cooking`),
    /// ingredients resolved to items: what Java's `level.recipeAccess()` finds.
    fn recipes(&mut self, kind: &str) -> Vec<Recipe>;
}
"#;

const ITEM_USE_ON_HELPER: &str = r#"// {name}'s items that act used on a block (`useOn`): runtime.rs's `use_item_on` runs
// their code; a use it takes runs neither the template's use nor the item's `use`.
fn {id}_use_item_on({params}) {
    use crate::game::{m} as m;
    if *claimed || !game.native_on(m::ID) {
        return;
    }
    let item = stack.item.to_string();
    if !m::item_acts_on_block(&item) {
        return;
    }
    let player = game.native_acting_player();
    // Where the click landed: the player's look from their eyes into the block's boxes.
    let hit = match &player {
        Some(p) => {
            let eye = [p.position[0], p.position[1] + 1.62, p.position[2]];
            let (yaw, pitch) = (f64::from(p.yaw).to_radians(), f64::from(p.pitch).to_radians());
            let look = [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()];
            m::ray_hit(eye, look, pos, face, &game.native_block_collision(pos))
        }
        None => m::ray_hit([0.0; 3], [0.0; 3], pos, face, &[]),
    };
    let creative = player.is_some_and(|p| p.game_mode == 1);
    let used = m::ItemUse {
        item,
        count: stack.count,
        damage: stack.damage,
        on: Some(m::UseOn {
            block: game.native_block(pos),
            pos,
            face,
            hit,
        }),
        creative,
    };
{world}    if m::use_item_on(&used, &mut Here { game, creative, source: 7 }) {
        *claimed = true;
    }
}
"#;

const ITEM_USE_HELPER: &str = r#"// {name}'s items that act used in the air (`use`): runtime.rs's `use_item` runs their
// code; a use it takes doesn't run the template's own.
fn {id}_use_item({params}) {
    use crate::game::{m} as m;
    if *claimed || !game.native_on(m::ID) {
        return;
    }
    let item = stack.item.to_string();
    if !m::item_acts_in_air(&item) {
        return;
    }
    let creative = game
        .native_acting_player()
        .is_some_and(|p| p.game_mode == 1);
    let used = m::ItemUse {
        item,
        count: stack.count,
        damage: stack.damage,
        on: None,
        creative,
    };
{world}    if m::use_item(&used, &mut Here { game, creative, source: 7 }) {
        *claimed = true;
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

const DISPATCH_DOCS: [&str; 5] = [
    "\n/// The properties a block is placed with (Java's `getStateForPlacement` setting them\n/// on `defaultBlockState()`), from its class's function. None leaves its template's\n/// placement.\n",
    "\n/// A right click on a block, run by its class's function. True when it took the click\n/// (`InteractionResult.SUCCESS` or `CONSUME`): neither its template's use nor the held\n/// item's runs then. A click that opens a menu in Java (`player.openMenu(..)`) is false\n/// here: it goes on to the block's template (a barrel, a furnace), which opens the menu.\n",
    "\n/// The properties a block changes as its neighbour in `u.direction` changes (Java's\n/// `updateShape`), from its class's function. None keeps it as it is.\n",
    "\n/// One of the mod's items used on a block, run by its class's `useOn` function. True\n/// when it took the use: neither its template's use nor its `use` runs then.\n",
    "\n/// One of the mod's items used, run by its class's `use` function. True when it took\n/// the use: its template's own use doesn't run then.\n",
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
    fn set_property(&mut self, pos: [i32; 3], key: &str, value: &str) -> bool {
        self.set_property_flags(pos, key, value, 3)
    }
    /// The same with `level.setBlock`'s `flags` as Java passes them: 3 tells the
    /// neighbours and reshapes them (`Block.UPDATE_ALL`), 2 only sends it to the
    /// players, 18 (2 | 16, `UPDATE_KNOWN_SHAPE`) keeps neighbours from reshaping
    /// it straight back, as a block that cycles its own shape on a click does.
    fn set_property_flags(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool;
    /// Plays `sound` (`minecraft:block.wooden_door.open`, or one of the mod's own)
    /// at the middle of `pos` for the players near it (`level.playSound`).
    fn play_sound(&mut self, sound: &str, pos: [i32; 3], volume: f32, pitch: f32);
    /// The level's random, 0 to 1 (`level.random.nextFloat()`).
    fn random(&mut self) -> f32;
    /// Puts `block` at `pos` with `properties` (`level.setBlock(pos,
    /// block.defaultBlockState().setValue(..), 3)`); `minecraft:air` removes what
    /// is there. False when it couldn't.
    fn set_block(&mut self, pos: [i32; 3], block: &str, properties: &[(&str, &str)]) -> bool {
        self.set_block_flags(pos, block, properties, 3)
    }
    /// The same with `level.setBlock`'s `flags` (see `set_property_flags`).
    fn set_block_flags(
        &mut self,
        pos: [i32; 3],
        block: &str,
        properties: &[(&str, &str)],
        flags: i32,
    ) -> bool;
    /// `stack.shrink(n)` on the stack in the hand that clicked: it empties at 0.
    /// Java's own code checks `instabuild` itself (`if
    /// (!player.getAbilities().instabuild)`), so this doesn't.
    fn shrink_held(&mut self, n: u32) {
        let _ = n;
    }
    /// `stack.hurtAndBreak(n, player, slot)`: wears the stack in hand by `n`, never
    /// in creative; true when that broke it.
    fn hurt_held(&mut self, n: u32) -> bool {
        let _ = n;
        false
    }
    /// `player.getInventory().add(new ItemStack(item, count))`: how many fit
    /// (nothing is dropped).
    fn give(&mut self, item: &str, count: u32) -> u32 {
        let _ = (item, count);
        0
    }
    /// Puts `count` of `item` in the hand that clicked (`player.setItemInHand(hand,
    /// stack)`); `minecraft:air` or 0 empties it.
    fn set_held(&mut self, item: &str, count: u32) {
        let _ = (item, count);
    }
    /// Whether `item` is in item tag `tag` (`stack.is(ItemTags.FLOWERS)`, a mod's
    /// `TagKey<Item>`: `villagersplus:small_plantable_items`).
    fn item_is(&mut self, item: &str, tag: &str) -> bool {
        let _ = (item, tag);
        false
    }
    /// Sends particles to the players near `at` (world coordinates,
    /// `serverLevel.sendParticles`): a vanilla type (`minecraft:smoke`) or one of the
    /// mod's own (`farmersdelight:steam`). Client-side effects (`level.addParticle`)
    /// go here too: everyone near sees them.
    fn particles(&mut self, kind: &str, at: [f64; 3], spread: [f64; 3], speed: f64, count: i32) {
        let _ = (kind, at, spread, speed, count);
    }
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
    /// How many of it the hand holds (`stack.getCount()`), 0 when empty.
    pub held_count: u32,
    /// The way the player looks, flat (`player.getDirection()`): `north`, `east`,
    /// `south` or `west`.
    pub facing: String,
    /// Whether the player is in creative (`player.getAbilities().instabuild`).
    pub creative: bool,
}

/// The way a player at `yaw` looks, flat (Java's `Direction.fromYRot`): `north`,
/// `east`, `south` or `west`.
pub fn yaw_direction(yaw: f32) -> &'static str {
    let i = ((f64::from(yaw) / 90.0 + 0.5).floor() as i64).rem_euclid(4);
    ["south", "west", "north", "east"][i as usize]
}

/// A right click with one of the mod's items (`Item.use`, or `Item.useOn` with
/// its `UseOnContext`). Horizon doesn't say whether the player sneaks.
pub struct ItemUse {
    /// The item: `mymod:wrench`.
    pub item: String,
    /// The stack's count and damage (`stack.getCount()`, `stack.getDamageValue()`).
    pub count: u32,
    pub damage: u32,
    /// The block it is used on (`useOn`), None in the air (`use`).
    pub on: Option<UseOn>,
    /// Whether the player is in creative (`player.getAbilities().instabuild`).
    pub creative: bool,
}

/// The block an item is used on (`UseOnContext`).
pub struct UseOn {
    /// `context.getLevel().getBlockState(pos).getBlock()`: `minecraft:oak_log`.
    pub block: String,
    pub pos: [i32; 3],
    /// The face clicked: `[0, 1, 0]` is the top (`context.getClickedFace()`).
    pub face: [i32; 3],
    /// Where the click landed, in world coordinates (`context.getClickLocation()`).
    pub hit: [f64; 3],
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
        yaw_direction(self.yaw)
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
