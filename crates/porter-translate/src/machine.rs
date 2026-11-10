// Machines: block entities with item slots and a menu (a crusher, a furnace-like
// block). The Java menu's slots, the screen's size and texture, and the mod's recipe
// types become:
//
// - a menu type of the content mod (`data/<id>/horizon/menu/<name>.json`): its slot
//   count, data slots and result slots;
// - `native_block_entity_container`: the block's slots, which faces reach which, and
//   the menu it opens;
// - `native_screen` and `native_screen_draw`: where each slot sits on the screen, and
//   the screen's own texture under them;
// - a `Machine` the block entity's tick gets: its slots, the menu's data slots, and the
//   mod's recipes read from their JSON files, ingredients resolved to items.

use std::collections::BTreeMap;
use std::fmt::Write;

use porter_model::{ClassInfo, MenuInfo, ModScan, ScreenInfo};

use crate::block_entity::BlockEntity;

/// A block entity with a menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    /// The block entity's id: `grind:crusher`.
    pub entity: String,
    /// The blocks that open it: `grind:crusher`.
    pub blocks: Vec<String>,
    /// The menu type the content mod declares: `grind:crusher`.
    pub menu_type: String,
    /// Its file under `content/`: `data/grind/horizon/menu/crusher.json`.
    pub menu_file: String,
    /// The Java menu class, fully qualified.
    pub menu_class: String,
    /// The Java screen class, when found.
    pub screen_class: Option<String>,
    pub slots: usize,
    /// Slots players only take from (a furnace's result).
    pub results: Vec<usize>,
    pub data_slots: usize,
    /// Where each slot sits on the screen, by slot number; none hides it.
    pub layout: Vec<Option<[i32; 2]>>,
    /// The player inventory's first row's left slot.
    pub inventory: [i32; 2],
    pub size: [i32; 2],
    /// The screen's texture as a GUI sprite: `grind:gui/crusher`.
    pub texture: Option<String>,
    /// Slots the scanner could not place (added in a loop it could not unroll).
    pub approximate: bool,
    /// The class whose `quickMoveStack` decides a shift-click (the menu's, or a
    /// superclass of the mod's own), when it has one.
    pub quick_move: Option<String>,
}

/// The mod's machines: its block entities whose class opens one of its menus.
/// `ns` is the namespace of its ids, `id` the content mod's (its menu types').
pub fn find(scan: &ModScan, ns: &str, id: &str, entities: &[BlockEntity]) -> Vec<Machine> {
    let mut out = Vec::new();
    for be in entities {
        let class = be
            .class
            .as_deref()
            .and_then(|c| scan.classes.iter().find(|k| k.name == c));
        let Some(menu) = menu_of(scan, be, class) else {
            continue;
        };
        let menu_simple = simple(&menu.class);
        // The menu's registered name, else the block entity's.
        let name = scan
            .registrations
            .iter()
            .filter(|r| r.registry == "menu")
            .find(|r| r.creates.as_deref() == Some(menu_simple) || r.args.contains(menu_simple))
            .and_then(|r| r.id.clone())
            .unwrap_or_else(|| be.id.rsplit(':').next().unwrap_or(&be.id).to_string());
        let screen = scan
            .screens
            .iter()
            .find(|s| s.menu.as_deref() == Some(menu_simple));
        let slots = slot_count(menu, class);
        let mut layout = vec![None; slots];
        for (i, s) in menu.slots.iter().enumerate() {
            let at = s.index.map_or(i, |n| n.max(0) as usize);
            if let Some(cell) = layout.get_mut(at) {
                *cell = Some([s.x, s.y]);
            }
        }
        let mut results: Vec<usize> = menu
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.result)
            .map(|(i, s)| s.index.map_or(i, |n| n.max(0) as usize))
            .filter(|i| *i < slots)
            .collect();
        results.sort_unstable();
        results.dedup();
        out.push(Machine {
            entity: be.id.clone(),
            blocks: be.blocks.clone(),
            menu_type: format!("{id}:{name}"),
            menu_file: format!("data/{id}/horizon/menu/{name}.json"),
            menu_class: menu.class.clone(),
            screen_class: screen.map(|s| s.class.clone()),
            slots,
            results,
            data_slots: menu.data_slots,
            layout,
            inventory: menu.inventory.unwrap_or([8, 84]),
            // Not known: what its inventory needs, as vanilla lays a screen out (the
            // inventory 8 in from each side, its hotbar's foot 7 above the bottom).
            size: {
                let [ix, iy] = menu.inventory.unwrap_or([8, 84]);
                [
                    screen
                        .and_then(|s| s.width)
                        .unwrap_or((ix * 2 + 160).max(176)),
                    screen.and_then(|s| s.height).unwrap_or((iy + 82).max(166)),
                ]
            },
            texture: screen.and_then(|s| texture(s, ns)),
            approximate: menu.approximate,
            quick_move: quick_move_owner(scan, &menu.class),
        });
    }
    out
}

/// The nearest of `class` and its superclasses (the mod's own) that defines
/// `quickMoveStack`.
fn quick_move_owner(scan: &ModScan, class: &str) -> Option<String> {
    let mut name = class.to_string();
    for _ in 0..16 {
        let c = scan.classes.iter().find(|c| c.name == name)?;
        if c.method_names.iter().any(|m| m == "quickMoveStack") {
            return Some(c.name.clone());
        }
        name = c.superclass.clone()?;
    }
    None
}

/// The menu a block entity opens: one its class names (`createMenu` returns
/// `new CrusherMenu(...)`), else the one registered under its name.
fn menu_of<'a>(
    scan: &'a ModScan,
    be: &BlockEntity,
    class: Option<&ClassInfo>,
) -> Option<&'a MenuInfo> {
    if let Some(class) = class
        && let Some(m) = scan
            .menus
            .iter()
            .find(|m| class.references.contains(&m.class))
    {
        return Some(m);
    }
    let name = be.id.rsplit(':').next()?;
    let menu = scan
        .registrations
        .iter()
        .find(|r| r.registry == "menu" && r.id.as_deref() == Some(name))?
        .creates
        .clone()?;
    scan.menus.iter().find(|m| simple(&m.class) == menu)
}

/// How many slots the machine has: the most its menu shows, or its block entity's
/// item handler holds (`new ItemStackHandler(2)`), whichever is more.
fn slot_count(menu: &MenuInfo, class: Option<&ClassInfo>) -> usize {
    let shown = menu
        .slots
        .iter()
        .enumerate()
        .map(|(i, s)| s.index.map_or(i, |n| n.max(0) as usize) + 1)
        .max()
        .unwrap_or(0);
    let held = class
        .into_iter()
        .flat_map(|c| &c.fields)
        .filter_map(|f| f.init.as_deref())
        .filter_map(|init| {
            ["ItemStackHandler(", "SimpleContainer(", "withSize("]
                .iter()
                .find_map(|p| {
                    let at = init.find(p)? + p.len();
                    let digits: String = init[at..]
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect();
                    digits.parse::<usize>().ok()
                })
        })
        .max()
        .unwrap_or(0);
    shown.max(held).clamp(1, 256)
}

/// `textures/gui/crusher.png` → `grind:gui/crusher`.
fn texture(screen: &ScreenInfo, ns: &str) -> Option<String> {
    let path = screen.texture.as_deref()?;
    let sprite = path.strip_prefix("textures/")?.strip_suffix(".png")?;
    Some(format!("{ns}:{sprite}"))
}

fn simple(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

/// The menu type file of a machine.
pub fn menu_json(m: &Machine) -> String {
    let mut json = serde_json::json!({ "slots": m.slots });
    if m.data_slots > 0 {
        json["data_slots"] = m.data_slots.into();
    }
    if !m.results.is_empty() {
        json["results"] = m.results.clone().into();
    }
    serde_json::to_string_pretty(&json).unwrap_or_default() + "\n"
}

/// The mod's recipe types: `grind:crushing`.
pub fn recipe_types(scan: &ModScan, ns: &str) -> Vec<String> {
    let mut types: Vec<String> = scan
        .registrations
        .iter()
        .filter(|r| r.registry == "recipe_type")
        .filter_map(|r| r.id.as_deref())
        .map(|n| format!("{ns}:{n}"))
        .collect();
    types.dedup();
    types
}

/// The runtime.rs types a block entity's tick works with: `Stack`, `Recipe` and
/// `Machine`. Every mod with block entities has them: `tick_block_entity` takes a
/// `Machine` (empty for a block entity without slots).
pub const MACHINE_TYPES: &str = r#"
/// A shift-click in a machine's menu (`quickMoveStack(player, index)`).
pub struct QuickMove {
    /// The slot clicked, as the menu numbers it: the block's slots first
    /// (`container_slots` of them), then the player's 27 and their hotbar's 9.
    pub slot: usize,
    pub container_slots: usize,
}

/// A machine menu's slots, as a shift-click moves between them: the block's own,
/// then the player's inventory (27 slots and the hotbar's 9, reached by giving).
pub struct MenuSlots<'a> {
    /// The block's slots.
    pub container: Vec<Option<Stack>>,
    max_stack: &'a dyn Fn(&str) -> u32,
    give: &'a mut dyn FnMut(&str, u32) -> u32,
}

impl<'a> MenuSlots<'a> {
    /// `container` the block's slots, `max_stack` an item's stack size, `give` what
    /// puts items in the player's inventory (how many it took).
    pub fn new(
        container: Vec<Option<Stack>>,
        max_stack: &'a dyn Fn(&str) -> u32,
        give: &'a mut dyn FnMut(&str, u32) -> u32,
    ) -> Self {
        Self { container, max_stack, give }
    }

    /// How many slots the block has (the first menu index of the player's inventory).
    pub fn container_slots(&self) -> usize {
        self.container.len()
    }

    /// The block's slot `i` (`this.slots.get(i).getItem()`, for `i` below
    /// `container_slots`).
    pub fn item(&self, i: usize) -> Option<&Stack> {
        self.container.get(i).and_then(Option::as_ref)
    }

    /// Puts `stack` in the block's slot `i` (`slot.set(..)`).
    pub fn set(&mut self, i: usize, stack: Option<Stack>) {
        if let Some(cell) = self.container.get_mut(i) {
            *cell = stack.filter(|s| s.count > 0);
        }
    }

    /// Java's `moveItemStackTo(stack, start, end, reverse)`: moves what fits of `stack`
    /// into menu slots `start..end` (indices as `QuickMove::slot` numbers them), onto
    /// stacks of the same item first, then into empty slots, from the end when
    /// `reverse`. True when anything moved; `stack` keeps the rest.
    pub fn move_item_stack_to(&mut self, stack: &mut Stack, start: usize, end: usize, reverse: bool) -> bool {
        let n = self.container.len();
        let before = stack.count;
        let own = start.min(n)..end.min(n);
        let player = end > n && start < n + 36;
        if reverse && player {
            self.to_player(stack);
        }
        let max = (self.max_stack)(&stack.item).clamp(1, 99);
        let order: Vec<usize> = if reverse { own.clone().rev().collect() } else { own.clone().collect() };
        for &i in &order {
            if stack.count == 0 {
                break;
            }
            if let Some(Some(s)) = self.container.get_mut(i) {
                if s.item == stack.item && s.count < max {
                    let moved = (max - s.count).min(stack.count);
                    s.count += moved;
                    stack.count -= moved;
                }
            }
        }
        for &i in &order {
            if stack.count == 0 {
                break;
            }
            if let Some(cell @ None) = self.container.get_mut(i) {
                let moved = max.min(stack.count);
                *cell = Some(Stack::new(&stack.item, moved));
                stack.count -= moved;
            }
        }
        if !reverse && player && stack.count > 0 {
            self.to_player(stack);
        }
        stack.count < before
    }

    fn to_player(&mut self, stack: &mut Stack) {
        if stack.count > 0 {
            let given = (self.give)(&stack.item, stack.count).min(stack.count);
            stack.count -= given;
        }
    }
}

/// The level around a machine whose screen is open, as its Java screen reads it on
/// the client (`menu.blockEntity.getLevel()`): its block's position, the blocks
/// around it, tags. A pot's heat icon shows whether the block below heats it.
pub struct ScreenAt<'a> {
    /// The machine's position, when the client can tell (the block under the
    /// crosshair, whose menu is open).
    pub pos: Option<[i32; 3]>,
    pub block: &'a dyn Fn([i32; 3]) -> String,
    pub tags: &'a dyn Fn(&str, &str, &str) -> bool,
}

impl ScreenAt<'_> {
    /// The block at `pos`: `minecraft:campfire`.
    pub fn block(&self, pos: [i32; 3]) -> String {
        (self.block)(pos)
    }
    /// The block under the machine (`pos.below()`), when its position is known.
    pub fn below(&self) -> Option<String> {
        let [x, y, z] = self.pos?;
        Some((self.block)([x, y - 1, z]))
    }
    /// Whether `block` is in block tag `tag` (`state.is(ModTags.HEAT_SOURCES)`).
    pub fn block_is(&self, block: &str, tag: &str) -> bool {
        (self.tags)("block", tag, block)
    }
}

/// An item stack in a machine's slot: `minecraft:cobblestone` × 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: u32,
}

impl Stack {
    pub fn new(item: &str, count: u32) -> Self {
        Self {
            item: item.to_string(),
            count,
        }
    }
}

/// The most items a slot holds (most items stack to 64).
pub const MAX_STACK: u32 = 64;

/// A recipe of one of the mod's own types, read from its JSON file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recipe {
    /// Its id: `grind:crush_cobblestone`.
    pub id: String,
    /// What each ingredient accepts (`ingredient`, or each of `ingredients`), as
    /// items: tags are resolved.
    pub ingredients: Vec<Vec<String>>,
    /// Its `result`.
    pub result: Option<Stack>,
    /// Its other plain values (`"time": 100`), as written.
    pub values: Vec<(String, String)>,
}

impl Recipe {
    /// Whether ingredient `n` takes `stack`'s item.
    pub fn accepts(&self, n: usize, stack: &Stack) -> bool {
        self.ingredients
            .get(n)
            .is_some_and(|items| items.iter().any(|i| *i == stack.item))
    }

    /// A number among its values: `recipe.number("time")`.
    pub fn number(&self, key: &str) -> Option<f64> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.parse().ok())
    }
}

/// What a machine's tick reaches besides its saved state: its slots, its menu's
/// data slots (`ContainerData`, which the screen reads), and the mod's recipes.
/// What the tick changes in the slots and the data slots is kept.
pub struct Machine<'a> {
    pub slots: Vec<Option<Stack>>,
    pub data: Vec<i32>,
    /// Its fluid tanks, as `FluidTank`s or a `FluidStacksResourceHandler`.
    pub tanks: Vec<Tank>,
    /// Its energy, and the most it holds (`SimpleEnergyHandler`).
    pub energy: i32,
    pub energy_capacity: i32,
    /// What the tick sends to and takes from the blocks beside it, done after it.
    pub transfers: Vec<Transfer>,
    /// Its own block (`getBlockState().getBlock()`): `spin:crank`, for a block
    /// entity several blocks share.
    pub block: String,
    /// The blocks beside it, by `Side` (read before the tick), for a block entity
    /// whose tick looks at them.
    pub neighbours: Vec<Option<Neighbour>>,
    /// Saved values to change in the block entities beside it, after the tick.
    pub neighbour_writes: Vec<(Side, String, Saved)>,
    /// Whether to send clients its state again.
    pub sync: bool,
    /// Particles and sounds to play after the tick: see `particles` and `sound`.
    pub effects: Vec<Effect>,
    recipes: &'a dyn Fn(&str) -> Vec<Recipe>,
    /// Tag membership: (`block` or `item`, tag, id).
    pub tags: Option<&'a dyn Fn(&str, &str, &str) -> bool>,
    /// The random's state: `machine.random()` draws from it.
    pub seed: u64,
    /// Its block's `facing` (`getBlockState().getValue(FACING)`), when it has one:
    /// `north`.
    pub facing: Option<String>,
    /// An item's facts: (its stack size, its crafting remainder).
    pub items: Option<&'a dyn Fn(&str) -> Option<(u32, Option<String>)>>,
    /// Its block's state properties (`getBlockState().getValue(..)`): `enabled`,
    /// `lit`; see `state`.
    pub properties: Vec<(String, String)>,
    /// Whether a block beside it powers it (`level.hasNeighborSignal(pos)`).
    pub powered: bool,
    /// The dropped items in a box: see `dropped_in`.
    pub dropped: Option<&'a dyn Fn([f64; 3], [f64; 3]) -> Vec<Dropped>>,
    /// The living things in a box: see `mobs_in`.
    pub mobs: Option<&'a dyn Fn([f64; 3], [f64; 3]) -> Vec<NearMob>>,
}

/// A particle burst or a sound a tick plays, near its block.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// `serverLevel.sendParticles(type, x, y, z, count, dx, dy, dz, speed)`: a
    /// vanilla type (`minecraft:smoke`) or one of the mod's own (`farmersdelight:steam`).
    Particles {
        kind: String,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: i32,
    },
    /// An item dropped into the world (`level.addFreshEntity(new ItemEntity(..))`,
    /// `Containers.dropItemStack`): a skillet's cooked food, a stove's.
    Drop {
        item: String,
        count: u32,
        at: [f64; 3],
        motion: [f64; 3],
    },
    /// `level.playSound(null, x, y, z, sound, SoundSource.BLOCKS, volume, pitch)`.
    Sound {
        sound: String,
        at: [f64; 3],
        volume: f32,
        pitch: f32,
    },
    /// `count` taken from dropped item `id` (`machine.dropped_in`): it shrinks, and goes
    /// when it is empty (`ItemEntity.getItem().shrink(..)`, `discard()`).
    TakeDropped { id: u32, count: u32 },
    /// Its block's state property `key` set to `value` (`level.setBlock(pos,
    /// state.setValue(..), 3)`).
    SetState { key: String, value: String },
    /// `ExperienceOrb.award(level, pos, amount)`.
    Xp { at: [f64; 3], amount: u32 },
    /// An entity spawned (`type.create(level, ..)`, `addFreshEntity`), a baby when `baby`.
    Summon { kind: String, at: [f64; 3], baby: bool },
    /// Mob `id` (`machine.mobs_in`) put in love mode (`animal.setInLove(null)`).
    Love { id: u32 },
}

/// A living thing near the block (`Animal`, `Mob`), as `machine.mobs_in` finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct NearMob {
    /// Which one it is, for `machine.love`.
    pub id: u32,
    /// `minecraft:cow`, one of the mod's own.
    pub kind: String,
    pub at: [f64; 3],
    /// A baby (`isBaby()`, `getAge() < 0`).
    pub baby: bool,
}

/// A dropped item in the world (`ItemEntity`), as `machine.dropped_in` finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    /// Which one it is, for `machine.take_dropped`.
    pub id: u32,
    pub item: String,
    pub count: u32,
    /// Where it is (`getX()`, `getY()`, `getZ()`).
    pub at: [f64; 3],
}

/// A block beside a block entity: its block, and its block entity's id and saved
/// values when it is one of a mod's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Neighbour {
    pub block: String,
    pub entity: Option<String>,
    pub values: Vec<(String, Saved)>,
}

impl Neighbour {
    /// One of its saved values: `State::load(|k| neighbour.get(k))` reads its state.
    pub fn get(&self, key: &str) -> Option<Saved> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }
}

/// A side of a block (`Direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Side {
    pub const ALL: [Side; 6] = [Side::Down, Side::Up, Side::North, Side::South, Side::West, Side::East];

    /// The step to the block on this side (`Direction.getNormal`).
    pub fn offset(self) -> [i32; 3] {
        match self {
            Side::Down => [0, -1, 0],
            Side::Up => [0, 1, 0],
            Side::North => [0, 0, -1],
            Side::South => [0, 0, 1],
            Side::West => [-1, 0, 0],
            Side::East => [1, 0, 0],
        }
    }

    /// A block's `facing` (`north`...) as a side.
    pub fn from_name(name: &str) -> Option<Side> {
        Some(match name {
            "down" => Side::Down,
            "up" => Side::Up,
            "north" => Side::North,
            "south" => Side::South,
            "west" => Side::West,
            "east" => Side::East,
            _ => return None,
        })
    }
}

/// Items moved between a machine and the block beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// Up to `max` of what is in `slot`, into the block on `side`.
    Push { slot: usize, side: Side, max: u32 },
    /// Up to `max` into `slot`, from the block on `side`.
    Pull { slot: usize, side: Side, max: u32 },
}

/// A machine after its tick: what to keep, and the transfers to carry out.
pub struct MachineParts {
    pub slots: Vec<Option<Stack>>,
    pub data: Vec<i32>,
    pub tanks: Vec<Tank>,
    pub energy: i32,
    pub transfers: Vec<Transfer>,
    pub neighbour_writes: Vec<(Side, String, Saved)>,
    pub sync: bool,
    pub effects: Vec<Effect>,
}

/// A fluid tank: its fluid (`minecraft:water`), how much in millibuckets, and its
/// capacity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tank {
    pub fluid: Option<String>,
    pub amount: i32,
    pub capacity: i32,
}

impl Tank {
    /// `FluidTank.fill`: how much of `amount` of `fluid` went in.
    pub fn fill(&mut self, fluid: &str, amount: i32) -> i32 {
        if amount <= 0 || self.fluid.as_deref().is_some_and(|f| f != fluid) {
            return 0;
        }
        let moved = amount.min(self.capacity - self.amount).max(0);
        if moved > 0 {
            self.fluid = Some(fluid.to_string());
            self.amount += moved;
        }
        moved
    }

    /// `FluidTank.drain`: up to `amount` taken out, with its fluid.
    pub fn drain(&mut self, amount: i32) -> Option<(String, i32)> {
        let fluid = self.fluid.clone()?;
        let moved = amount.min(self.amount).max(0);
        if moved == 0 {
            return None;
        }
        self.amount -= moved;
        if self.amount == 0 {
            self.fluid = None;
        }
        Some((fluid, moved))
    }
}

impl<'a> Machine<'a> {
    pub fn new(
        slots: Vec<Option<Stack>>,
        data: Vec<i32>,
        recipes: &'a dyn Fn(&str) -> Vec<Recipe>,
    ) -> Self {
        Self {
            slots,
            data,
            tanks: Vec::new(),
            energy: 0,
            energy_capacity: 0,
            transfers: Vec::new(),
            block: String::new(),
            neighbours: (0..6).map(|_| None).collect(),
            neighbour_writes: Vec::new(),
            sync: false,
            effects: Vec::new(),
            recipes,
            tags: None,
            seed: 0x9E37_79B9_7F4A_7C15,
            facing: None,
            items: None,
            properties: Vec::new(),
            powered: false,
            dropped: None,
            mobs: None,
        }
    }

    /// Its block's state property `key` (`getBlockState().getValue(ENABLED)`): `"true"`.
    pub fn state(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Sets its block's state property `key` to `value` after the tick
    /// (`level.setBlock(pos, state.setValue(ENABLED, false), ..)`). What a block's
    /// `neighborChanged` keeps in step with `machine.powered` (a basket's or a
    /// hopper's `enabled`) can be done here, in its tick.
    pub fn set_state(&mut self, key: &str, value: &str) {
        if let Some(p) = self.properties.iter_mut().find(|(k, _)| k == key) {
            p.1 = value.to_string();
        }
        self.effects.push(Effect::SetState { key: key.to_string(), value: value.to_string() });
    }

    /// The living things, players aside, in the box from `min` to `max` (world
    /// coordinates): `level.getEntitiesOfClass(Animal.class, new AABB(pos).inflate(r))`.
    /// Pick the kinds the Java asks for by `NearMob::kind`. Empty where Horizon lists
    /// none.
    pub fn mobs_in(&self, min: [f64; 3], max: [f64; 3]) -> Vec<NearMob> {
        self.mobs.map(|f| f(min, max)).unwrap_or_default()
    }

    /// Puts mob `id` in love mode after the tick (`animal.setInLove(null)`), what a
    /// feeder does once it has spent the food.
    pub fn love(&mut self, id: u32) {
        self.effects.push(Effect::Love { id });
    }

    /// The dropped items (`ItemEntity`) whose position is in the box from `min` to `max`
    /// (world coordinates): `level.getEntitiesOfClass(ItemEntity.class, aabb,
    /// EntitySelector.ENTITY_STILL_ALIVE)`, what a hopper or a basket sucks in.
    pub fn dropped_in(&self, min: [f64; 3], max: [f64; 3]) -> Vec<Dropped> {
        self.dropped.map(|f| f(min, max)).unwrap_or_default()
    }

    /// Takes `count` from dropped item `id` after the tick (it goes when it is empty):
    /// what a hopper does once it has put the items in its slots.
    pub fn take_dropped(&mut self, id: u32, count: u32) {
        self.effects.push(Effect::TakeDropped { id, count });
    }

    /// What `item` leaves behind in a crafting grid (`stack.getCraftingRemainder()`,
    /// `Item.Properties.craftRemainder`): `minecraft:bowl` for a stew, a bucket for a
    /// milk bucket. A cooking pot's meal needs it as its container.
    pub fn remainder(&self, item: &str) -> Option<String> {
        self.items.and_then(|f| f(item)).and_then(|(_, r)| r)
    }

    /// The most of `item` a stack holds (`stack.getMaxStackSize()`): 64, 16, 1.
    pub fn max_stack(&self, item: &str) -> u32 {
        self.items.and_then(|f| f(item)).map_or(64, |(n, _)| n)
    }

    /// A random number from 0 to 1 (`level.random.nextFloat()`, `random.nextInt(10) == 0`
    /// as `machine.random() < 0.1`).
    pub fn random(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.seed;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Whether `block` (a neighbour's, `machine.neighbour(Side::Down)`) is in block
    /// tag `tag` (`state.is(BlockTags.CAMPFIRES)`, a mod's `farmersdelight:heat_sources`).
    pub fn block_is(&self, block: &str, tag: &str) -> bool {
        self.tags.is_some_and(|t| t("block", tag, block))
    }

    /// Whether `item` is in item tag `tag` (`stack.is(ItemTags.FLOWERS)`).
    pub fn item_is(&self, item: &str, tag: &str) -> bool {
        self.tags.is_some_and(|t| t("item", tag, item))
    }

    /// Sends particles to the players near `at` (world coordinates) after the tick
    /// (`serverLevel.sendParticles`). Effects Java plays on the client
    /// (`animateTick`: a pot's steam, a skillet's smoke) go here too: Horizon has no
    /// client hook for them, and the server sends them to every player near.
    pub fn particles(&mut self, kind: &str, at: [f64; 3], spread: [f64; 3], speed: f64, count: i32) {
        self.effects.push(Effect::Particles {
            kind: kind.to_string(),
            at,
            spread,
            speed,
            count,
        });
    }

    /// Drops `count` of `item` into the world at `at` (world coordinates), moving at
    /// `motion` blocks a tick, after the tick: what a skillet ejects when its food is
    /// cooked (`ItemEntity`, `Containers.dropItemStack`).
    pub fn drop(&mut self, item: &str, count: u32, at: [f64; 3], motion: [f64; 3]) {
        self.effects.push(Effect::Drop {
            item: item.to_string(),
            count,
            at,
            motion,
        });
    }

    /// Drops `amount` experience at `at` as orbs after the tick
    /// (`ExperienceOrb.award(level, pos, amount)`, a furnace's stored experience).
    pub fn xp(&mut self, at: [f64; 3], amount: u32) {
        if amount > 0 {
            self.effects.push(Effect::Xp { at, amount });
        }
    }

    /// Spawns entity `kind` (`minecraft:bee`, a mod's own) at `at` after the tick
    /// (`type.create(level, ..)`, `level.addFreshEntity(entity)`).
    pub fn summon(&mut self, kind: &str, at: [f64; 3]) {
        self.effects.push(Effect::Summon { kind: kind.to_string(), at, baby: false });
    }

    /// Spawns a baby `kind` at `at` after the tick (`setAge(-24000)`, `setBaby(true)`).
    pub fn summon_baby(&mut self, kind: &str, at: [f64; 3]) {
        self.effects.push(Effect::Summon { kind: kind.to_string(), at, baby: true });
    }

    /// Plays `sound` at `at` for the players near it after the tick (`level.playSound`,
    /// a skillet's sizzle).
    pub fn sound(&mut self, sound: &str, at: [f64; 3], volume: f32, pitch: f32) {
        self.effects.push(Effect::Sound {
            sound: sound.to_string(),
            at,
            volume,
            pitch,
        });
    }

    /// The block on `side` (`level.getBlockEntity(pos.relative(side))`): none when
    /// this block entity's tick doesn't look at its neighbours.
    pub fn neighbour(&self, side: Side) -> Option<&Neighbour> {
        let i = Side::ALL.iter().position(|s| *s == side)?;
        self.neighbours.get(i)?.as_ref()
    }

    /// Changes saved value `key` of the mod block entity on `side`, after the tick
    /// (a generator charging the machine beside it); clients get it again.
    pub fn set_neighbour(&mut self, side: Side, key: &str, value: Saved) {
        self.neighbour_writes.push((side, key.to_string(), value));
    }

    /// Sends clients this block entity's state again after the tick, for its
    /// renderer (`level.sendBlockUpdated`, `sendData`).
    pub fn sync(&mut self) {
        self.sync = true;
    }

    /// Sends up to `max` of what is in `slot` to the container on `side` (a chest,
    /// a hopper, another machine), after the tick, as a hopper would; what doesn't
    /// fit stays. `ItemHandlerHelper.insertItem` into the neighbour's
    /// `Capabilities.ItemHandler.BLOCK`, a machine's auto-output.
    pub fn push(&mut self, slot: usize, side: Side, max: u32) {
        self.transfers.push(Transfer::Push { slot, side, max });
    }

    /// Takes up to `max` items that fit in `slot` from the container on `side`,
    /// after the tick: the first stack it may take that fits. A machine's auto-input.
    pub fn pull(&mut self, slot: usize, side: Side, max: u32) {
        self.transfers.push(Transfer::Pull { slot, side, max });
    }

    /// What to keep after the tick, and the transfers to carry out.
    pub fn into_parts(self) -> MachineParts {
        MachineParts {
            slots: self.slots,
            data: self.data,
            tanks: self.tanks,
            energy: self.energy,
            transfers: self.transfers,
            neighbour_writes: self.neighbour_writes,
            sync: self.sync,
            effects: self.effects,
        }
    }

    /// Adds up to `amount` energy (`EnergyHandler.insert`); how much went in.
    pub fn add_energy(&mut self, amount: i32) -> i32 {
        let moved = amount.min(self.energy_capacity - self.energy).max(0);
        self.energy += moved;
        moved
    }

    /// Uses `amount` energy if there is that much (`extract` of exactly it).
    pub fn use_energy(&mut self, amount: i32) -> bool {
        if amount < 0 || self.energy < amount {
            return false;
        }
        self.energy -= amount;
        true
    }

    /// The recipes of type `kind` (`grind:crushing`), from the mod's and the
    /// world's data packs.
    pub fn recipes(&self, kind: &str) -> Vec<Recipe> {
        (self.recipes)(kind)
    }

    /// The first recipe of type `kind` whose first ingredient takes what is in
    /// slot `slot`, as `RecipeManager.getRecipeFor` finds one.
    pub fn recipe_for(&self, kind: &str, slot: usize) -> Option<Recipe> {
        let stack = self.slot(slot)?.clone();
        self.recipes(kind).into_iter().find(|r| r.accepts(0, &stack))
    }

    /// What is in slot `slot`.
    pub fn slot(&self, slot: usize) -> Option<&Stack> {
        self.slots.get(slot)?.as_ref()
    }

    /// Whether `stack` fits into slot `slot`: it is empty, or holds the same item
    /// with room for it (a slot holds at most the item's own stack size).
    pub fn fits(&self, slot: usize, stack: &Stack) -> bool {
        let max = self.max_stack(&stack.item).min(MAX_STACK);
        match self.slots.get(slot) {
            Some(None) => stack.count <= max,
            Some(Some(s)) => s.item == stack.item && s.count + stack.count <= max,
            None => false,
        }
    }

    /// Puts `stack` into slot `slot`, as much as fits; returns what did not.
    pub fn insert(&mut self, slot: usize, stack: Stack) -> Option<Stack> {
        let max = self.max_stack(&stack.item).min(MAX_STACK);
        let Some(cell) = self.slots.get_mut(slot) else {
            return Some(stack);
        };
        match cell {
            None => {
                let put = stack.count.min(max);
                *cell = Some(Stack::new(&stack.item, put));
                (put < stack.count).then(|| Stack::new(&stack.item, stack.count - put))
            }
            Some(s) if s.item == stack.item => {
                let put = stack.count.min(max.saturating_sub(s.count));
                s.count += put;
                (put < stack.count).then(|| Stack::new(&stack.item, stack.count - put))
            }
            Some(_) => Some(stack),
        }
    }

    /// Takes up to `count` items from slot `slot`, as `extractItem` does.
    pub fn take(&mut self, slot: usize, count: u32) -> Option<Stack> {
        let cell = self.slots.get_mut(slot)?;
        let s = cell.as_mut()?;
        let took = count.min(s.count);
        s.count -= took;
        let item = s.item.clone();
        if s.count == 0 {
            *cell = None;
        }
        (took > 0).then(|| Stack::new(&item, took))
    }

    /// Data slot `n` (0 when the menu has not set it yet).
    pub fn get(&self, n: usize) -> i32 {
        self.data.get(n).copied().unwrap_or(0)
    }

    /// Sets data slot `n`, as `ContainerData.set` does.
    pub fn set(&mut self, n: usize, value: i32) {
        if self.data.len() <= n {
            self.data.resize(n + 1, 0);
        }
        self.data[n] = value;
    }
}

/// Part of a GUI sprite, drawn on a machine's screen: `src` (x, y, w, h in the
/// sprite's pixels) stretched into `dst` (x, y, w, h from the screen's top left).
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    /// `grind:gui/crusher`: `assets/grind/textures/gui/crusher.png`.
    pub name: &'static str,
    pub src: [f32; 4],
    pub dst: [f32; 4],
}
"#;

/// The runtime.rs code for the machines: their table and a draw function for each
/// screen. `java_screens` names each machine's Java screen for the TODO.
pub fn runtime(machines: &[Machine], recipe_types: &[String]) -> String {
    let mut r = String::from(
        r#"
/// A machine's menu and screen: the block that opens it, its menu type, its slots
/// (those players only take from among them), its data slots, where each slot
/// sits on the screen, where the player's inventory starts, the screen's size and
/// its texture.
pub struct MachineMenu {
    pub block: &'static str,
    pub menu: &'static str,
    pub slots: usize,
    pub results: &'static [usize],
    pub data_slots: usize,
    pub layout: &'static [Option<[f32; 2]>],
    pub inventory: [f32; 2],
    pub size: [f32; 2],
    pub texture: Option<&'static str>,
}

/// The mod's machines.
pub const MACHINES: &[MachineMenu] = &[
"#,
    );
    for m in machines {
        let layout = m
            .layout
            .iter()
            .map(|p| match p {
                Some([x, y]) => format!("Some([{:.1}, {:.1}])", *x as f32, *y as f32),
                None => "None".into(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        for block in &m.blocks {
            let _ = writeln!(
                r,
                "    MachineMenu {{\n        block: \"{block}\",\n        menu: \"{menu}\",\n        slots: {slots},\n        results: &{results:?},\n        data_slots: {data},\n        layout: &[{layout}],\n        inventory: [{ix:.1}, {iy:.1}],\n        size: [{w:.1}, {h:.1}],\n        texture: {texture},\n    }},",
                menu = m.menu_type,
                slots = m.slots,
                results = m.results,
                data = m.data_slots,
                ix = m.inventory[0] as f32,
                iy = m.inventory[1] as f32,
                w = m.size[0] as f32,
                h = m.size[1] as f32,
                texture = m
                    .texture
                    .as_ref()
                    .map_or("None".to_string(), |t| format!("Some(\"{t}\")")),
            );
        }
    }
    r.push_str("];\n");
    let _ = writeln!(
        r,
        "\n/// The mod's own recipe types, which `Machine::recipes` reads.\npub const RECIPE_TYPES: &[&str] = &[{}];",
        recipe_types
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // A draw function per machine screen.
    for m in machines {
        let snake = draw_name(m);
        let java = m
            .screen_class
            .as_deref()
            .unwrap_or("its Java screen (not found)");
        let background = match &m.texture {
            Some(t) => format!(
                "    let _ = data;\n    // The screen's own texture, under the slots.\n    vec![Sprite {{\n        name: \"{t}\",\n        src: [0.0, 0.0, {w:.1}, {h:.1}],\n        dst: [0.0, 0.0, {w:.1}, {h:.1}],\n    }}]\n",
                w = m.size[0] as f32,
                h = m.size[1] as f32
            ),
            None => "    let _ = data;\n    Vec::new()\n".into(),
        };
        let _ = write!(
            r,
            "\n/// What `{menu}`'s screen draws under its slots and items, from the menu's data\n/// slots (`data`, empty until the server sends them; read them with `get`), as\n/// `renderBg` does: progress arrows, flames, gauges. What the Java reads of the level\n/// (a pot's heat from the block below) comes from `at`.\n///\n/// TODO: port `renderBg` of `{java}`.\npub fn {snake}(data: &[i32], at: &ScreenAt<'_>) -> Vec<Sprite> {{\n    let _ = at;\n{background}}}\n",
            menu = m.menu_type,
        );
    }
    r.push_str("\n/// What block `block`'s screen draws.\npub fn screen_sprites(block: &str, data: &[i32], at: &ScreenAt<'_>) -> Vec<Sprite> {\n    match block {\n");
    for m in machines {
        let blocks = m
            .blocks
            .iter()
            .map(|b| format!("\"{b}\""))
            .collect::<Vec<_>>()
            .join(" | ");
        if !blocks.is_empty() {
            let _ = writeln!(r, "        {blocks} => {}(data, at),", draw_name(m));
        }
    }
    r.push_str(
        "        _ => {\n            let _ = (data, at);\n            Vec::new()\n        }\n    }\n}\n",
    );

    // Shift-clicks: a function per menu whose Java decides them.
    r.push_str("\n/// Whether menu type `menu`'s Java decides where a shift-click moves a stack\n/// (`quickMoveStack`).\npub fn moves_on_shift_click(menu: &str) -> bool {\n    matches!(menu, ");
    let movers: Vec<&Machine> = machines.iter().filter(|m| m.quick_move.is_some()).collect();
    if movers.is_empty() {
        r.push_str("\"\"");
    } else {
        r.push_str(
            &movers
                .iter()
                .map(|m| format!("\"{}\"", m.menu_type))
                .collect::<Vec<_>>()
                .join(" | "),
        );
    }
    r.push_str(")\n}\n");
    r.push_str("\n/// A shift-click in menu type `menu`, run by its menu class's function. True when\n/// the Java decided it: the game's own routing is skipped then.\npub fn quick_move(menu: &str, q: &QuickMove, stack: &mut Stack, slots: &mut MenuSlots<'_>) -> bool {\n    match menu {\n");
    for m in &movers {
        let _ = writeln!(
            r,
            "        \"{}\" => {}(q, stack, slots),",
            m.menu_type,
            quick_move_name(m)
        );
    }
    r.push_str("        _ => {\n            let _ = (q, stack, slots);\n            false\n        }\n    }\n}\n");
    for m in &movers {
        let class = m.quick_move.as_deref().unwrap_or_default();
        let _ = write!(
            r,
            "\n/// A shift-click in `{menu}`'s menu (`quickMoveStack(player, index)`): move what\n/// `stack` holds (slot `q.slot`'s items) with `slots.move_item_stack_to(stack, start,\n/// end, reverse)` as Java's `moveItemStackTo` does, the menu's indices as Java numbers\n/// them (the block's slots, then the player's 27 and 9). What is left in `stack` stays\n/// in the slot. Return true where Java decides the click (it always does, once ported),\n/// false to let the game move it as for any container.\n///\n/// TODO: port `quickMoveStack` of {class}.\nfn {name}(q: &QuickMove, stack: &mut Stack, slots: &mut MenuSlots<'_>) -> bool {{\n    let _ = (q, stack, slots);\n    false\n}}\n",
            menu = m.menu_type,
            name = quick_move_name(m),
        );
    }
    r
}

/// `crusher_quick_move`.
fn quick_move_name(m: &Machine) -> String {
    draw_name(m).replacen("draw_", "", 1) + "_quick_move"
}

/// `draw_crusher`.
fn draw_name(m: &Machine) -> String {
    let name = m.menu_type.rsplit(':').next().unwrap_or(&m.menu_type);
    let snake: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("draw_{snake}")
}

/// Tests for the generated module: the machine helpers, and the table matches the
/// menu types.
pub fn runtime_tests(machines: &[Machine]) -> String {
    let mut t = String::from(
        r#"
    #[test]
    fn shift_clicks_move_as_java_does() {
        let size = |_: &str| 64;
        let mut taken = 0;
        let mut give = |_: &str, n: u32| {
            let n = n.min(10);
            taken += n;
            n
        };
        let mut slots = super::MenuSlots::new(
            vec![Some(super::Stack::new("demo:ore", 60)), None, None],
            &size,
            &mut give,
        );
        // Onto the same item first, then the first empty slot.
        let mut stack = super::Stack::new("demo:ore", 10);
        assert!(slots.move_item_stack_to(&mut stack, 0, 2, false));
        assert_eq!(stack.count, 0);
        assert_eq!(slots.item(0).map(|s| s.count), Some(64));
        assert_eq!(slots.item(1).map(|s| s.count), Some(6));
        // Into the player's inventory (indices from 3), which takes 10.
        let mut out = super::Stack::new("demo:dust", 15);
        assert!(slots.move_item_stack_to(&mut out, 3, 39, true));
        assert_eq!(out.count, 5);
        drop(slots);
        assert_eq!(taken, 10);
    }

    #[test]
    fn transfers_wait_for_after_the_tick() {
        let none = |_: &str| Vec::new();
        let mut m = super::Machine::new(vec![None, None], vec![], &none);
        m.push(1, super::Side::Down, 64);
        m.pull(0, super::Side::Up, 8);
        let parts = m.into_parts();
        assert_eq!(parts.transfers.len(), 2);
        for side in super::Side::ALL {
            let [x, y, z] = side.offset();
            assert_eq!(x.abs() + y.abs() + z.abs(), 1);
        }
        assert_eq!(super::Side::from_name("east"), Some(super::Side::East));
    }

    #[test]
    fn neighbours_are_read_and_written_by_side() {
        let none = |_: &str| Vec::new();
        let mut m = super::Machine::new(vec![], vec![], &none);
        m.neighbours[1] = Some(super::Neighbour {
            block: "demo:shaft".into(),
            entity: Some("demo:shaft".into()),
            values: vec![("Speed".into(), super::Saved::Float(16.0))],
        });
        let up = m.neighbour(super::Side::Up).expect("the block above");
        assert_eq!(up.get("Speed"), Some(super::Saved::Float(16.0)));
        assert!(m.neighbour(super::Side::Down).is_none());
        m.set_neighbour(super::Side::Up, "Speed", super::Saved::Float(8.0));
        m.sync();
        let parts = m.into_parts();
        assert_eq!(parts.neighbour_writes.len(), 1);
        assert!(parts.sync);
    }

    #[test]
    fn tanks_and_energy_fill_and_drain() {
        let none = |_: &str| Vec::new();
        let mut m = super::Machine::new(vec![], vec![], &none);
        m.tanks = vec![super::Tank { capacity: 1000, ..Default::default() }];
        assert_eq!(m.tanks[0].fill("minecraft:water", 1500), 1000);
        assert_eq!(m.tanks[0].fill("minecraft:lava", 10), 0);
        assert_eq!(m.tanks[0].drain(400), Some(("minecraft:water".to_string(), 400)));
        assert_eq!(m.tanks[0].drain(1000), Some(("minecraft:water".to_string(), 600)));
        assert_eq!(m.tanks[0].fluid, None);
        m.energy_capacity = 100;
        assert_eq!(m.add_energy(150), 100);
        assert!(m.use_energy(60) && !m.use_energy(60));
        assert_eq!(m.energy, 40);
    }

    #[test]
    fn machine_slots_take_and_fit() {
        let none = |_: &str| Vec::new();
        let mut m = super::Machine::new(vec![None, None], vec![], &none);
        assert_eq!(m.insert(0, super::Stack::new("minecraft:stone", 70)), Some(super::Stack::new("minecraft:stone", 6)));
        assert!(!m.fits(0, &super::Stack::new("minecraft:stone", 1)));
        assert!(m.fits(1, &super::Stack::new("minecraft:dirt", 1)));
        assert_eq!(m.take(0, 64), Some(super::Stack::new("minecraft:stone", 64)));
        assert_eq!(m.slot(0), None);
        m.set(1, 7);
        assert_eq!((m.get(0), m.get(1)), (0, 7));
    }
"#,
    );
    if machines.is_empty() {
        return t;
    }
    let _ = write!(
        t,
        "\n    #[test]\n    fn machines_place_their_slots() {{\n        for m in super::MACHINES {{\n            assert!(m.layout.len() <= m.slots);\n            assert!(m.results.iter().all(|r| *r < m.slots));\n        }}\n        assert_eq!(super::MACHINES.len(), {});\n    }}\n",
        machines.iter().map(|m| m.blocks.len()).sum::<usize>()
    );
    t
}

/// The machines' content files: their menu types.
pub fn content_files(machines: &[Machine]) -> BTreeMap<String, String> {
    machines
        .iter()
        .map(|m| (m.menu_file.clone(), menu_json(m)))
        .collect()
}
