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
            size: [
                screen.and_then(|s| s.width).unwrap_or(176),
                screen.and_then(|s| s.height).unwrap_or(166),
            ],
            texture: screen.and_then(|s| texture(s, ns)),
            approximate: menu.approximate,
        });
    }
    out
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
    recipes: &'a dyn Fn(&str) -> Vec<Recipe>,
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
            recipes,
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
    /// with room for it.
    pub fn fits(&self, slot: usize, stack: &Stack) -> bool {
        match self.slots.get(slot) {
            Some(None) => stack.count <= MAX_STACK,
            Some(Some(s)) => s.item == stack.item && s.count + stack.count <= MAX_STACK,
            None => false,
        }
    }

    /// Puts `stack` into slot `slot`, as much as fits; returns what did not.
    pub fn insert(&mut self, slot: usize, stack: Stack) -> Option<Stack> {
        let Some(cell) = self.slots.get_mut(slot) else {
            return Some(stack);
        };
        match cell {
            None => {
                let put = stack.count.min(MAX_STACK);
                *cell = Some(Stack::new(&stack.item, put));
                (put < stack.count).then(|| Stack::new(&stack.item, stack.count - put))
            }
            Some(s) if s.item == stack.item => {
                let put = stack.count.min(MAX_STACK.saturating_sub(s.count));
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
            "\n/// What `{menu}`'s screen draws under its slots and items, from the menu's data\n/// slots (`data`, empty until the server sends them; read them with `get`), as\n/// `renderBg` does: progress arrows, flames, gauges.\n///\n/// TODO: port `renderBg` of `{java}`.\npub fn {snake}(data: &[i32]) -> Vec<Sprite> {{\n{background}}}\n",
            menu = m.menu_type,
        );
    }
    r.push_str("\n/// What block `block`'s screen draws.\npub fn screen_sprites(block: &str, data: &[i32]) -> Vec<Sprite> {\n    match block {\n");
    for m in machines {
        let blocks = m
            .blocks
            .iter()
            .map(|b| format!("\"{b}\""))
            .collect::<Vec<_>>()
            .join(" | ");
        if !blocks.is_empty() {
            let _ = writeln!(r, "        {blocks} => {}(data),", draw_name(m));
        }
    }
    r.push_str(
        "        _ => {\n            let _ = data;\n            Vec::new()\n        }\n    }\n}\n",
    );
    r
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
