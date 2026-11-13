// Block entities: each Java block entity type becomes a state struct in runtime.rs,
// read from and written to the block's saved compound, with a tick stub. Horizon's
// `native_block_entity` gives the mod's blocks their block entity,
// `native_block_entity_ticks` says which tick, and `native_block_entity_tick` runs
// the stub on the server, 20 times a second.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use porter_model::{ClassInfo, ModScan};

/// The hook points version that gives a block entity to a block on any template
/// (before 129 the template needed a block entity of its own).
pub const HOOKS_VERSION: u32 = 129;

/// A block entity type of the mod.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    /// Its id: `demo:crank`.
    pub id: String,
    /// The blocks that have it: `demo:crank`.
    pub blocks: Vec<String>,
    /// The Java class, fully qualified, when found.
    pub class: Option<String>,
    /// The state struct: `Crank`.
    pub rust: String,
    /// The name its functions use: `crank` in `tick_crank`.
    pub snake: String,
    pub fields: Vec<StateField>,
    /// Whether it ticks (the Java class has a tick method).
    pub ticks: bool,
    /// Its Java renderer (`BlockEntityRenderer`), fully qualified, when it has one.
    pub renderer: Option<String>,
    /// Its fluid tanks' capacities, in millibuckets.
    pub tanks: Vec<i32>,
    /// Its energy buffer: capacity, most one insert moves, most one extract moves.
    pub energy: Option<[i32; 3]>,
    /// How many item slots it holds (`new ItemStackHandler(1)`), 0 for none.
    pub items: usize,
    /// Its item handlers by field, when it has several, their slots one after another:
    /// `("inputInv", 1)`, `("outputInv", 9)`.
    pub inventories: Vec<(String, usize)>,
    /// The class of its block whose `animateTick` plays effects on the client
    /// (a pot's bubbles, a skillet's sizzle), fully qualified: its tick sends them.
    pub animate: Option<String>,
    /// Whether its Java code looks at the blocks beside it (it uses `Direction`):
    /// its tick then gets them (`machine.neighbour(side)`).
    pub neighbours: bool,
    /// Its blocks' class with a `neighborChanged` (redstone turning it on or off).
    pub redstone: Option<String>,
    /// Its blocks' class that acts on what lands on or enters the block, and the
    /// method (`updateEntityAfterFallOn`, `entityInside`): a millstone taking in the
    /// items dropped on it.
    pub lands: Option<(String, &'static str)>,
    /// Java fields the state leaves out, with why.
    pub left_out: Vec<String>,
    /// The mod's own base classes it takes state or its tick from (Create's
    /// `KineticBlockEntity` for a shaft), nearest first.
    pub bases: Vec<String>,
    /// Those of its blocks whose own model Java keeps out of the world
    /// (`getRenderShape`), for it to draw.
    pub drawn: Vec<String>,
    /// Whether its Java looks two steps away (`pos.above(2)`): its tick then reaches
    /// the block entities there too (`machine.at(pos)`).
    pub far: bool,
    /// Whether its Java walks the 3×3×3 around it (`BlockPos.betweenClosed(.., new
    /// BlockPos(-1, -1, -1), new BlockPos(1, 1, 1))`, as Create does to mesh cogwheels
    /// diagonally): its tick then reaches the block entities at the twenty places that
    /// aren't beside it too (`machine.at(pos)`).
    pub around: bool,
    /// The classes of the mod's own it keeps part of itself in, by the field that
    /// holds each: (`pressingBehaviour`, `com.x.PressingBehaviour`). Their saved
    /// values and lasting fields are in its state, their ticks part of its tick.
    pub held: Vec<(String, String)>,
    /// Of those, the ones made with code of the block entity's own, by the field and how
    /// Java makes it: (`basinChecker`, `new DeferralBehaviour(this, this::updateBasin)`).
    /// The kept part's tick calls that code, so it is part of this block entity's tick.
    pub made: Vec<(String, String)>,
    /// The classes of other block entities that take something it holds and call the
    /// code it gave it: a depot's `DepotBehaviour`, for a press.
    pub called_by: Vec<Caller>,
    /// The nearest class of its own or above it whose tick is ported once for every
    /// block entity that inherits it (`shared_ticks`): `com.x.KineticBlockEntity`.
    pub shared: Option<String>,
    /// With a shared tick: the methods of its classes that this block entity's own
    /// classes override and do something in (`initialize`, `lazyTick`). The shared
    /// tick reports the ones it called; this one's tick does what each override adds.
    pub overrides: Vec<String>,
}

/// A class several block entities inherit, with a tick of its own: Create's
/// `KineticBlockEntity`, above sixty of them. Its tick is ported once, as a function
/// over the part of a block entity's state the class keeps, and each block entity's
/// own tick calls it where its Java calls `super.tick()`. Otherwise the same code is
/// translated again for each, differs between them, and makes each tick too long.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedTick {
    /// The class: `com.x.KineticBlockEntity`.
    pub class: String,
    /// The mod's classes above it that its part takes fields or a tick from, up to
    /// the next shared one, nearest first.
    pub above: Vec<String>,
    /// The next shared class above it: its `super.tick()`.
    pub next: Option<String>,
    /// Its part of a block entity's state: the fields of it and of every class above.
    pub fields: Vec<StateField>,
    /// The block entities that inherit it, by id.
    pub users: Vec<String>,
    /// Methods of it and of the classes above that classes below override.
    pub overridden: Vec<String>,
    /// Every method of them a class below overrides: those the tick's own classes call
    /// on the block entity and that answer something are questions to it, ported once
    /// (`questions::find`).
    pub asks: Vec<String>,
    /// Of them, those that are such questions: the Java method and the function that
    /// answers it (`getGeneratedSpeed`, `kinetic_block_entity_get_generated_speed`).
    pub answered: Vec<(String, String)>,
    /// What the ticks of those block entities reach, any of them.
    pub neighbours: bool,
    pub far: bool,
    pub around: bool,
    pub drawn: bool,
}

impl SharedTick {
    /// Two of the methods classes below override, as an example of what its tick
    /// returns: `"onSpeedChanged"`, `"lazyTick"`.
    fn examples(&self) -> String {
        if self.overridden.is_empty() {
            return "`\"onChanged\"`".to_string();
        }
        self.overridden
            .iter()
            .take(2)
            .map(|m| format!("`\"{m}\"`"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Its simple name: `KineticBlockEntity`.
    pub fn simple(&self) -> &str {
        self.class.rsplit(['.', '$']).next().unwrap_or(&self.class)
    }

    /// The function its tick is ported as: `kinetic_block_entity_tick`.
    pub fn function(&self) -> String {
        format!("{}_tick", shared_snake(&self.class))
    }
}

/// `kinetic_block_entity` for `com.x.KineticBlockEntity`.
fn shared_snake(class: &str) -> String {
    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
    let mut out = String::new();
    for (i, c) in simple.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// The shared ticks of a mod's block entities, and each block entity told which is
/// its nearest (`BlockEntity::shared`). A class is shared when two or more block
/// entities whose tick is the plain kind (a state and a `Machine`) have it as their
/// own class or above it, and it declares a `tick` itself.
pub fn shared_ticks(
    scan: &ModScan,
    entities: &mut [BlockEntity],
    machines: &[crate::machine::Machine],
) -> Vec<SharedTick> {
    let class = |name: &str| scan.classes.iter().find(|c| c.name == name);
    let ticks_itself =
        |name: &str| class(name).is_some_and(|c| c.method_names.iter().any(|m| m == "tick"));
    // Each one's classes, its own first; none for one whose tick is another kind.
    let chains: Vec<Vec<String>> = entities
        .iter()
        .map(|be| {
            let plain =
                be.ticks && takes_machine(be) && !machines.iter().any(|m| m.entity == be.id);
            if plain {
                be.class.iter().chain(&be.bases).cloned().collect()
            } else {
                Vec::new()
            }
        })
        .collect();
    let mut users: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, chain) in chains.iter().enumerate() {
        for c in chain {
            users.entry(c.as_str()).or_default().push(i);
        }
    }
    let shared: BTreeSet<&str> = users
        .iter()
        .filter(|(c, of)| of.len() >= 2 && ticks_itself(c))
        .map(|(c, _)| *c)
        .collect();
    let mut out: Vec<SharedTick> = Vec::new();
    for c in &shared {
        let of = &users[c];
        // The one that has the most of its fields stands for all (another may have
        // lost one to a field of its own with the same name).
        let part = |i: usize| -> Vec<StateField> {
            let chain = &chains[i];
            let at = chain.iter().position(|x| x == c).unwrap_or(0);
            entities[i]
                .fields
                .iter()
                .filter(|f| chain[at..].contains(&f.from))
                .cloned()
                .collect()
        };
        let Some(most) = of.iter().copied().max_by_key(|i| part(*i).len()) else {
            continue;
        };
        let chain = &chains[most];
        let at = chain.iter().position(|x| x == c).unwrap_or(0);
        let rest = &chain[at + 1..];
        let next = rest.iter().find(|x| shared.contains(x.as_str())).cloned();
        let above: Vec<String> = rest
            .iter()
            .take_while(|x| !shared.contains(x.as_str()))
            .cloned()
            .collect();
        // What classes below it override of it and of those above.
        let mut theirs: BTreeSet<&str> = BTreeSet::new();
        for name in std::iter::once(*c).chain(rest.iter().map(String::as_str)) {
            if let Some(k) = class(name) {
                theirs.extend(k.method_names.iter().map(String::as_str));
            }
        }
        let mut overridden: BTreeMap<&str, usize> = BTreeMap::new();
        for i in of {
            let below = chains[*i].iter().take_while(|x| x.as_str() != *c);
            for k in below.filter_map(|n| class(n)) {
                for m in &k.method_names {
                    // (Not the tick itself, nor saving and making, which no tick calls.)
                    let skip = [
                        "tick",
                        "write",
                        "read",
                        "saveAdditional",
                        "loadAdditional",
                        "addBehaviours",
                    ];
                    if theirs.contains(m.as_str()) && !skip.contains(&m.as_str()) {
                        *overridden.entry(m.as_str()).or_default() += 1;
                    }
                }
            }
        }
        // Those that do something first, the ones most classes override before the rest;
        // a method that only answers (`getGeneratedSpeed`, `isSource`) after them.
        let answers = |m: &str| {
            ["get", "is", "has", "can", "should", "calculate"]
                .iter()
                .any(|p| {
                    m.strip_prefix(p)
                        .is_some_and(|r| r.starts_with(char::is_uppercase))
                })
        };
        let mut overridden: Vec<(&str, usize)> = overridden.into_iter().collect();
        overridden.sort_by(|a, b| {
            answers(a.0)
                .cmp(&answers(b.0))
                .then(b.1.cmp(&a.1))
                .then(a.0.cmp(b.0))
        });
        out.push(SharedTick {
            class: c.to_string(),
            above,
            next,
            fields: part(most),
            users: of.iter().map(|i| entities[*i].id.clone()).collect(),
            asks: overridden.iter().map(|(m, _)| m.to_string()).collect(),
            answered: Vec::new(),
            overridden: overridden
                .into_iter()
                .take(14)
                .map(|(m, _)| m.to_string())
                .collect(),
            neighbours: of.iter().any(|i| entities[*i].neighbours),
            far: of.iter().any(|i| entities[*i].far),
            around: of.iter().any(|i| entities[*i].around),
            drawn: of.iter().any(|i| entities[*i].renderer.is_some()),
        });
    }
    for (be, chain) in entities.iter_mut().zip(&chains) {
        be.shared = chain.iter().find(|x| shared.contains(x.as_str())).cloned();
        // What its own classes (below the shared one) override of that class and the
        // classes above it.
        if let Some(at) = chain.iter().position(|x| shared.contains(x.as_str())) {
            let theirs: BTreeSet<&str> = chain[at..]
                .iter()
                .filter_map(|n| class(n))
                .flat_map(|k| k.method_names.iter().map(String::as_str))
                .collect();
            let skip = [
                "tick",
                "write",
                "read",
                "saveAdditional",
                "loadAdditional",
                "addBehaviours",
            ];
            let mut own: Vec<String> = Vec::new();
            for k in chain[..at].iter().filter_map(|n| class(n)) {
                for m in &k.method_names {
                    if theirs.contains(m.as_str())
                        && !skip.contains(&m.as_str())
                        && !own.contains(m)
                    {
                        own.push(m.clone());
                    }
                }
            }
            be.overrides = own;
        }
    }
    out
}

/// What a tick's `machine` reaches of the world around the block entity, a phrase each.
fn reach(neighbours: bool, far: bool, around: bool, drawn: bool) -> Vec<String> {
    let mut has = Vec::new();
    if neighbours {
        has.push("the blocks beside it (`machine.neighbour(Side::Up)`, with their saved state; `machine.set_neighbour(...)` changes one after the tick)".to_string());
    }
    if far {
        has.push("the mod block entities two steps away along each axis too (`machine.at(pos)` for `level.getBlockEntity(pos.above(2))`, each with its block, state, saved values and item slots; `machine.set_at(pos, key, value)` changes a saved value of one after the tick, `machine.set_item_at(pos, slot, stack)` one of its item slots)".to_string());
    }
    if around {
        has.push("the mod block entities at the other places of the 3×3×3 around it too, diagonals included (`machine.at(pos)` for what `BlockPos.betweenClosed(pos.offset(-1, -1, -1), pos.offset(1, 1, 1))` walks, each with its block, state, saved values and item slots)".to_string());
    }
    if drawn {
        has.push("`machine.sync()`, which sends clients its state again for its renderer (`sendBlockUpdated`)".to_string());
    }
    has
}

/// The two functions that give a state struct's shared part and take it back:
/// `struct_fields` are the struct's own, `shared` the class whose part it is.
fn shared_part_impl(ty: &str, struct_fields: &[StateField], shared: &SharedTick) -> String {
    let snake = shared_snake(&shared.class);
    let base = format!("{}Base", shared.simple());
    let both: Vec<&StateField> = shared
        .fields
        .iter()
        .filter(|f| {
            struct_fields
                .iter()
                .any(|g| g.name == f.name && g.kind == f.kind)
        })
        .collect();
    let mut r = String::new();
    let _ = write!(
        r,
        "\nimpl {ty} {{\n    /// The part of it `{simple}` and the classes above keep, for `{function}`.\n    pub fn {snake}_base(&self) -> {base} {{\n",
        simple = shared.simple(),
        function = shared.function(),
    );
    if both.is_empty() {
        let _ = writeln!(r, "        {base}::default()");
    } else {
        let _ = writeln!(r, "        {base} {{");
        for f in &both {
            let value = match f.kind {
                Kind::Text | Kind::Item => format!("self.{}.clone()", f.name),
                _ => format!("self.{}", f.name),
            };
            let _ = writeln!(r, "            {}: {value},", f.name);
        }
        if both.len() < shared.fields.len() {
            r.push_str("            ..Default::default()\n");
        }
        r.push_str("        }\n");
    }
    let _ = write!(
        r,
        "    }}\n\n    /// Takes that part back, as `{function}` left it.\n    pub fn set_{snake}_base(&mut self, base: {base}) {{\n",
        function = shared.function(),
    );
    if both.is_empty() {
        r.push_str("        let _ = base;\n");
    }
    for f in &both {
        let _ = writeln!(r, "        self.{0} = base.{0};", f.name);
    }
    r.push_str("    }\n}\n");
    r
}

/// The runtime.rs code of the shared ticks: each one's part of the state, how a part
/// gives the part of the next one up, and the function its tick is ported as.
fn shared_runtime(shared: &[SharedTick]) -> String {
    let mut r = String::new();
    for t in shared {
        let base = format!("{}Base", t.simple());
        let function = t.function();
        let above: Vec<String> = t
            .above
            .iter()
            .map(|c| format!("`{}`", c.rsplit(['.', '$']).next().unwrap_or(c)))
            .collect();
        let with = if above.is_empty() {
            String::new()
        } else {
            format!(" (with {} above it)", above.join(", "))
        };
        let _ = write!(
            r,
            "\n/// The part of a block entity's state that `{simple}` keeps{with}, and every class above:\n/// what `{function}` works on, for each of the {n} block entities that inherit it. A block\n/// entity's state gives it (`state.{snake}_base()`) and takes it back\n/// (`state.set_{snake}_base(base)`).\n#[derive(Clone, Debug, Default, PartialEq)]\npub struct {base} {{\n",
            simple = t.simple(),
            n = t.users.len(),
            snake = shared_snake(&t.class),
        );
        for f in &t.fields {
            let how = if f.saved_by_java {
                format!("saved as `{}`", one_line(&f.key))
            } else {
                format!("kept as `{}`", one_line(&f.key))
            };
            // What its text holds, as each block entity's own state says it.
            let how = if f.java.starts_with("BlockPos ") {
                format!(
                    "{how}; the position as `x y z` (`4 64 -2`: written with `pos_text(pos)`, read with `text_pos(&text)`), empty for none (null)"
                )
            } else if f.java.starts_with("Recipe") {
                format!(
                    "{how}; the recipe's id (`recipe.id`, as `grind:crush_cobblestone`), empty for none (null): the recipe itself is found again among `machine.recipes(kind)` by that id"
                )
            } else {
                how
            };
            let _ = writeln!(
                r,
                "    /// `{}`, {how}.\n    pub {}: {},",
                f.java,
                f.name,
                f.kind.rust()
            );
        }
        r.push_str("}\n");
        let next = t
            .next
            .as_ref()
            .and_then(|n| shared.iter().find(|s| s.class == *n));
        if let Some(next) = next {
            r.push_str(&shared_part_impl(&base, &t.fields, next));
        }
        // Its tick.
        let mut has = reach(t.neighbours, t.far, t.around, t.drawn);
        if has.is_empty() {
            has.push("what a tick has".to_string());
        }
        let some: Vec<String> = t.users.iter().take(6).map(|u| format!("`{u}`")).collect();
        let more = if t.users.len() > 6 {
            format!(" and {} more", t.users.len() - 6)
        } else {
            String::new()
        };
        let overridden = if t.overridden.is_empty() {
            String::new()
        } else {
            format!(
                "\n/// Classes below override these methods of them: {}. Where the tick calls one on the block\n/// entity itself: one that only answers is asked of `me` where this module has it ported (named\n/// with this function's Java), else answered as these classes do; one that changes the block\n/// entity's own state is done as these classes do it and its name returned.",
                t.overridden
                    .iter()
                    .map(|m| format!("`{m}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        // What a class below answers its own way, by the function that asks it.
        let overridden = if t.answered.is_empty() {
            overridden
        } else {
            format!(
                "{overridden}\n/// What a class below answers its own way is asked of `me`, by the function of this module that\n/// has every class's answer: {}. Never answer one of these as these classes alone would.",
                t.answered
                    .iter()
                    .map(|(java, rust)| format!("`{java}(..)` is `{rust}(me, ..)`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let up = match next {
            Some(next) => format!(
                "\n/// Its `super.tick()` is the tick of `{}`, ported once too: `let mut above = base.{snake}_base();`\n/// `let mut called = {function}(pos, &mut above, me, machine); base.set_{snake}_base(above);`, and what it\n/// returned is returned with this one's.",
                next.simple(),
                snake = shared_snake(&next.class),
                function = next.function(),
            ),
            None => String::new(),
        };
        let java = if t.above.is_empty() {
            t.class.clone()
        } else {
            format!(
                "{}` and what it inherits from `{}",
                t.class,
                t.above.join("`, `")
            )
        };
        let _ = write!(
            r,
            "\n/// The tick of `{simple}`{with}, ported once for the {n} block entities that inherit it\n/// ({some}{more}). `base` is the part of the block entity's state these classes keep; `me` is the\n/// whole block entity as a view (its block, its state properties, and its saved values as the tick\n/// began), for what its own class answers; `machine` has {has}. What the tick changes in `base`\n/// and through `machine` is kept.\n/// It returns the methods of these classes it called on the block entity that a class below may\n/// override with effects of its own, by name ({examples}), having done what\n/// these classes themselves do for each: the caller adds what its own class does.{overridden}{up}\n/// The parts a block entity keeps in classes of its own (a behaviour it holds) are ticked by its\n/// own tick, not here.\n///\n/// TODO: port the tick of `{java}`.\npub fn {function}(pos: [i32; 3], base: &mut {base}, me: &Neighbour, machine: &mut Machine<'_>) -> Vec<&'static str> {{\n    let _ = (pos, base, me, machine);\n    Vec::new()\n}}\n",
            simple = t.simple(),
            n = t.users.len(),
            some = some.join(", "),
            has = has.join("; "),
            examples = t.examples(),
        );
    }
    r
}

/// A class of the mod that takes something a block entity holds and calls the code
/// that block entity gave it: a depot's `DepotBehaviour`, which gets the
/// `BeltProcessingBehaviour` of the press two above it and calls its handlers.
#[derive(Debug, Clone, PartialEq)]
pub struct Caller {
    /// What it takes: `com.x.BeltProcessingBehaviour`.
    pub takes: String,
    /// The class that calls it: `com.x.DepotBehaviour`.
    pub class: String,
    /// How it gets it, as written: `BeltProcessingBehaviour processingBehaviour =
    /// BlockEntityBehaviour.get(world, pos.above(2), BeltProcessingBehaviour.TYPE)`.
    pub gets: String,
    /// The block entities that class is part of, each with its item slots
    /// ("`heldItem` is slot 0", empty for none): (`demo:depot`, ..).
    pub entities: Vec<(String, String)>,
}

/// A field of the state struct.
#[derive(Debug, Clone, PartialEq)]
pub struct StateField {
    pub name: String,
    pub kind: Kind,
    /// The key it is saved under.
    pub key: String,
    /// Whether the Java mod saves it (`saveAdditional`); others are kept under their
    /// own name so they last between ticks.
    pub saved_by_java: bool,
    /// The Java field, or the saved expression.
    pub java: String,
    /// What it starts as, where the Java gives it a plain value (`int timer = 20`,
    /// `generatedSpeed.value = DEFAULT_SPEED`): a Rust expression. None starts it at
    /// zero, false or empty.
    pub init: Option<String>,
    /// The class that declares it: the block entity's own, one above it, or one it
    /// keeps part of itself in.
    pub from: String,
}

/// The NBT kinds a state field can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Text,
    /// An `ItemStack` (a pot's meal container, a shell's flower): the item's id,
    /// empty for `ItemStack.EMPTY`.
    Item,
}

impl Kind {
    /// The kind for a Java type: `float` → Float, `String` → Text.
    pub fn from_java(ty: &str) -> Option<Kind> {
        Some(match ty.rsplit('.').next().unwrap_or(ty) {
            "boolean" | "Boolean" => Kind::Bool,
            "byte" | "Byte" => Kind::Byte,
            "short" | "Short" => Kind::Short,
            "int" | "Integer" => Kind::Int,
            "long" | "Long" => Kind::Long,
            "float" | "Float" => Kind::Float,
            "double" | "Double" => Kind::Double,
            "String" => Kind::Text,
            // A position (a source, a controller, a link): kept as text, `x y z`.
            "BlockPos" => Kind::Text,
            // A recipe it found (a basin operator's `currentRecipe`): kept by its id, and
            // found again among its type's recipes. Without a place for it, the tick
            // that finds the recipe can't hand it to the one that applies it.
            "Recipe" | "Recipe<?>" | "RecipeHolder<?>" => Kind::Text,
            "ItemStack" | "ItemStackTemplate" => Kind::Item,
            _ => return None,
        })
    }

    /// `putFloat` → Float.
    fn from_put(method: &str) -> Option<Kind> {
        Some(match method {
            "putBoolean" => Kind::Bool,
            "putByte" => Kind::Byte,
            "putShort" => Kind::Short,
            "putInt" => Kind::Int,
            "putLong" => Kind::Long,
            "putFloat" => Kind::Float,
            "putDouble" => Kind::Double,
            "putString" => Kind::Text,
            // `output.store("Container", ItemStack.CODEC, stack)`, read as `storeItem`.
            "storeItem" => Kind::Item,
            _ => return None,
        })
    }

    pub fn rust(self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Byte => "i8",
            Kind::Short => "i16",
            Kind::Int => "i32",
            Kind::Long => "i64",
            Kind::Float => "f32",
            Kind::Double => "f64",
            Kind::Text | Kind::Item => "String",
        }
    }

    /// The `Saved` variant it is stored as.
    fn saved(self) -> &'static str {
        match self {
            Kind::Bool | Kind::Byte => "Byte",
            Kind::Short => "Short",
            Kind::Int => "Int",
            Kind::Long => "Long",
            Kind::Float => "Float",
            Kind::Double => "Double",
            Kind::Text | Kind::Item => "Text",
        }
    }
}

/// The mod's block entity types, linked to their blocks. `ns` is the namespace of
/// the mod's ids.
pub fn find(scan: &ModScan, ns: &str) -> Vec<BlockEntity> {
    let blocks: Vec<&str> = scan
        .registrations
        .iter()
        .filter(|r| r.registry == "block")
        .filter_map(|r| r.id.as_deref())
        .collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for reg in scan
        .registrations
        .iter()
        .filter(|r| r.registry == "block_entity_type")
    {
        let Some(name) = reg.id.as_deref() else {
            continue;
        };
        if !seen.insert(name.to_string()) {
            continue;
        }
        // `BlockEntityType.Builder.of(CrankBlockEntity::new, CRANK.get())`: the
        // blocks are the constants named after them; else the block of the same name.
        let mut its_blocks: Vec<String> = blocks
            .iter()
            .filter(|b| {
                // `CRANK`, or `OAK_HORTICULTURIST_TABLE_BLOCK` (Villagers Plus).
                let upper = b.to_ascii_uppercase();
                has_word(&reg.args, &upper) || has_word(&reg.args, &format!("{upper}_BLOCK"))
            })
            .map(|b| format!("{ns}:{b}"))
            .collect();
        // No constant of the call is named after a block: the block of the type's own name.
        let by_name = its_blocks.is_empty() && blocks.contains(&name);
        if by_name {
            its_blocks.push(format!("{ns}:{name}"));
        }
        // Then the blocks of that block's Java class make the same block entity
        // (`newBlockEntity`): an infuser and its advanced kind, named `INFUSER_BLOCK` and
        // `ADVANCED_INFUSER_BLOCK` where their ids are `enchanting_infuser` and
        // `advanced_enchanting_infuser`.
        let class_of_block = |id: &str| {
            scan.registrations
                .iter()
                .find(|r| r.registry == "block" && r.id.as_deref() == Some(id))
                .and_then(|r| r.creates.clone())
        };
        let classes: BTreeSet<String> = its_blocks
            .iter()
            .filter_map(|b| class_of_block(b.split_once(':').map_or(b.as_str(), |(_, n)| n)))
            .collect();
        for b in blocks.iter().filter(|_| by_name) {
            let id = format!("{ns}:{b}");
            if !its_blocks.contains(&id) && class_of_block(b).is_some_and(|c| classes.contains(&c))
            {
                its_blocks.push(id);
            }
        }
        // `new BlockEntityType(HorticulturistTableBlockEntity::new, ..)` creates the
        // type: the block entity is the class whose constructor it takes.
        let creates = match reg.creates.as_deref() {
            Some(c)
                if c.ends_with("BlockEntityType")
                    || c == "Builder"
                    || c.ends_with("TypeBuilder") =>
            {
                reg.args.split("::new").next().and_then(|before| {
                    let name: String = before
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    (!name.is_empty() && reg.args.contains("::new")).then_some(name)
                })
            }
            other => other.map(str::to_string),
        };
        let class = creates.as_deref().and_then(|simple| {
            let mut found = scan
                .classes
                .iter()
                .filter(|c| c.name.rsplit('.').next() == Some(simple));
            let first = found.next()?;
            found.next().is_none().then_some(first)
        });
        // Its blocks' class with an `animateTick` (its own or a superclass of the mod's).
        let animate = scan
            .registrations
            .iter()
            .filter(|r| r.registry == "block")
            .filter(|r| {
                r.id.as_deref()
                    .is_some_and(|id| its_blocks.iter().any(|b| b.ends_with(&format!(":{id}"))))
            })
            .filter_map(|r| r.creates.as_deref())
            .find_map(|simple| {
                let mut c = scan
                    .classes
                    .iter()
                    .find(|c| c.name.rsplit('.').next() == Some(simple))?;
                for _ in 0..8 {
                    if c.method_names.iter().any(|m| m == "animateTick") {
                        return Some(c.name.clone());
                    }
                    let sup = c.superclass.as_deref()?;
                    c = scan.classes.iter().find(|k| k.name == sup)?;
                }
                None
            });
        let snake = rust_ident(name);
        let redstone = block_class_with(scan, &its_blocks, "neighborChanged");
        let lands = ["updateEntityAfterFallOn", "entityInside"]
            .into_iter()
            .find_map(|m| block_class_with(scan, &its_blocks, m).map(|c| (c, m)));
        let drawn: Vec<String> = its_blocks
            .iter()
            .filter(|id| {
                let name = id.split_once(':').map_or(id.as_str(), |(_, n)| n);
                porter_content::drawn_by_its_entity(scan, name)
            })
            .cloned()
            .collect();
        let mut be = BlockEntity {
            id: format!("{ns}:{name}"),
            blocks: its_blocks,
            class: class.map(|c| c.name.clone()),
            rust: camel(&snake),
            snake,
            fields: Vec::new(),
            ticks: false,
            renderer: None,
            tanks: Vec::new(),
            energy: None,
            items: 0,
            inventories: Vec::new(),
            redstone,
            lands,
            animate,
            neighbours: false,
            left_out: Vec::new(),
            bases: Vec::new(),
            drawn,
            far: false,
            around: false,
            held: Vec::new(),
            called_by: Vec::new(),
            shared: None,
            overrides: Vec::new(),
            made: Vec::new(),
        };
        if let Some(class) = class {
            state_of(scan, class, &mut be);
            // What it inherits from the mod's own base classes: their saved values, the
            // fields that last between ticks, and the tick itself when it is theirs
            // (a shaft's speed and its spreading are `KineticBlockEntity`'s).
            let mut parent = class.superclass.clone();
            for _ in 0..6 {
                let Some(base) = parent
                    .take()
                    .and_then(|n| scan.classes.iter().find(|c| c.name == n))
                else {
                    break;
                };
                let mut theirs = be.clone();
                theirs.fields.clear();
                theirs.left_out.clear();
                state_of(scan, base, &mut theirs);
                let mut took = theirs.ticks && !be.ticks;
                be.ticks |= theirs.ticks;
                // A base class that looks at the blocks beside it (it uses `Direction`).
                be.neighbours |= base
                    .references
                    .iter()
                    .any(|r| r == "net.minecraft.core.Direction");
                for f in theirs.fields {
                    if !be.fields.iter().any(|x| x.name == f.name || x.key == f.key) {
                        be.fields.push(f);
                        took = true;
                    }
                }
                // (And the fields of it that have no place in the state: a tick that calls
                // a method of the base class reads them. Its constants are its own.)
                for l in theirs.left_out {
                    if l.contains("no Rust type for it yet") && !be.left_out.contains(&l) {
                        be.left_out.push(l);
                    }
                }
                if took {
                    be.bases.push(base.name.clone());
                }
                parent = base.superclass.clone();
            }
            let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
            be.renderer = scan
                .renderers
                .iter()
                .find(|r| r.entity == simple)
                .map(|r| r.class.clone());
            // Registrate names the renderer in the type's own chain, for any entity it
            // fits: `.renderer(() -> ShaftRenderer::new)`.
            if let Some(i) = reg.args.find(".renderer(") {
                let named: String = reg.args[i + ".renderer(".len()..]
                    .trim_start_matches(|c: char| !c.is_ascii_alphabetic())
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let mut of_name = scan
                    .classes
                    .iter()
                    .filter(|c| c.name.rsplit(['.', '$']).next() == Some(named.as_str()));
                if let (Some(found), None) = (of_name.next(), of_name.next()) {
                    be.renderer = Some(found.name.clone());
                }
            }
            be.neighbours |= class
                .references
                .iter()
                .any(|r| r == "net.minecraft.core.Direction");
            if let Some(s) = scan.storages.iter().find(|s| s.class == class.name) {
                be.tanks = s.tanks.clone();
                be.energy = s.energy;
                be.items = s.items;
                be.inventories = s.inventories.clone();
            }
            // What it keeps part of itself in (a behaviour, a component).
            let mut owners = vec![class];
            for base in &be.bases {
                if let Some(c) = scan.classes.iter().find(|c| c.name == *base) {
                    owners.push(c);
                }
            }
            hold(scan, &owners, &mut be);
            // Whether any of that looks two steps away: `worldPosition.below(2)`.
            let reaches = |c: &ClassInfo| {
                std::fs::read_to_string(std::path::Path::new(&scan.input).join(&c.path)).is_ok_and(
                    |text| {
                        [
                            ".above(2)",
                            ".below(2)",
                            ".north(2)",
                            ".south(2)",
                            ".east(2)",
                            ".west(2)",
                        ]
                        .iter()
                        .any(|far| text.contains(far))
                    },
                )
            };
            be.far = owners.iter().any(|c| reaches(c))
                || be.held.iter().any(|(_, name)| {
                    scan.classes
                        .iter()
                        .find(|c| c.name == *name)
                        .is_some_and(reaches)
                });
            be.neighbours |= be.far;
            // Or walks the cube around it.
            let cube = |c: &ClassInfo| {
                std::fs::read_to_string(std::path::Path::new(&scan.input).join(&c.path)).is_ok_and(
                    |text| {
                        let text: String = text.split_whitespace().collect();
                        text.contains("newBlockPos(-1,-1,-1)")
                            && text.contains("newBlockPos(1,1,1)")
                    },
                )
            };
            be.around = owners.iter().any(|c| cube(c));
            be.neighbours |= be.around;
            // A class it keeps that holds item lists (a grave's `Death`, with the dead
            // player's inventory): its items are the block entity's own item slots.
            if be.items == 0 {
                for f in class.fields.iter().filter(|f| !f.is_static) {
                    let ty = f.ty.split(['<', '[']).next().unwrap_or(&f.ty).trim();
                    let mut of_type = scan
                        .classes
                        .iter()
                        .filter(|c| c.name.rsplit(['.', '$']).next() == Some(ty));
                    let (Some(held), None) = (of_type.next(), of_type.next()) else {
                        continue;
                    };
                    let Some(s) = scan
                        .storages
                        .iter()
                        .find(|s| s.class == held.name && s.items > 0)
                    else {
                        continue;
                    };
                    // Room for each of its lists: the largest, and what rides along.
                    be.items = (s.items + 18).next_multiple_of(9).max(54);
                    let was = format!("`{} {}`", f.ty, f.name);
                    be.left_out.retain(|l| !l.starts_with(&was));
                    be.left_out.push(format!(
                        "{was}: its item lists are this block entity's own {} item slots (`world.entity_item(pos, slot)`, `world.set_entity_item(pos, slot, ..)`), kept and dropped with the block; its other values have no place yet",
                        be.items
                    ));
                    break;
                }
            }
        }
        // Its block's model is kept out of the world for a renderer the scan didn't
        // find (a library's, a visual): it still gets a render function, which shows
        // the block's own model until that is ported, rather than nothing.
        if be.renderer.is_none() && !be.drawn.is_empty() {
            be.renderer = Some("its Java renderer (not found)".to_string());
        }
        out.push(be);
    }
    called_by(scan, &mut out);
    out
}

/// Finds, for each block entity, the classes of other block entities that take
/// something it holds and call the code it gave it (`Caller`). Java has the one call
/// the other; here each tick is ported alone, so the one that gave the code is told
/// who would call it, and does that work itself.
fn called_by(scan: &ModScan, entities: &mut [BlockEntity]) {
    let class = |name: &str| scan.classes.iter().find(|c| c.name == name);
    let simple = |name: &str| name.rsplit(['.', '$']).next().unwrap_or(name).to_string();
    // What each block entity is made of: its class, the mod's above it, what it holds.
    let parts: Vec<Vec<String>> = entities
        .iter()
        .map(|be| {
            be.class
                .iter()
                .chain(&be.bases)
                .cloned()
                .chain(be.held.iter().map(|(_, c)| c.clone()))
                .collect()
        })
        .collect();
    // What can be taken from one: a class it holds, or one of the mod's above that
    // (short of a base of many kinds), which keeps code it is given (a field of a
    // callback type) and has methods that run it (`handleReceivedItem`, whose body is
    // `return onItemEnter.apply(stack, inventory)`).
    let read = |c: &ClassInfo| {
        std::fs::read_to_string(std::path::Path::new(&scan.input).join(&c.path)).ok()
    };
    let runs_code = |c: &ClassInfo| -> Vec<String> {
        const FUNCTIONS: &[&str] = &[
            "Runnable",
            "Supplier",
            "BooleanSupplier",
            "Consumer",
            "BiConsumer",
            "Function",
            "BiFunction",
            "Predicate",
            "BiPredicate",
            "UnaryOperator",
            "BinaryOperator",
        ];
        let Some(source) = read(c) else {
            return Vec::new();
        };
        let given: Vec<&str> = c
            .fields
            .iter()
            .filter(|f| !f.is_static)
            .filter(|f| {
                let ty = simple(f.ty.split(['<', '[']).next().unwrap_or(&f.ty).trim());
                FUNCTIONS.contains(&ty.as_str())
                    || has_word(&source, &format!("interface {ty}"))
                    || scan.classes.iter().any(|o| {
                        o.kind == porter_model::ClassKind::Interface
                            && simple(&o.name) == ty
                            && o.methods == 1
                    })
            })
            .map(|f| f.name.as_str())
            .collect();
        c.method_names
            .iter()
            .filter(|m| {
                crate::questions::method_body(&source, m).is_some_and(|body| {
                    given
                        .iter()
                        .any(|f| has_word(&body, f) && body.contains(&format!("{f}.")))
                })
            })
            .cloned()
            .collect()
    };
    // Each with the methods of its that run the code it was given.
    let mut taken: Vec<Vec<(&ClassInfo, Vec<String>)>> = Vec::new();
    for be in entities.iter() {
        let mut mine: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
        // What it holds, and what its own fields are of (a spout's plain
        // `BeltProcessingBehaviour`, which saves nothing and so isn't in `held`).
        let mut holds: Vec<&ClassInfo> = be.held.iter().filter_map(|(_, c)| class(c)).collect();
        for owner in be.class.iter().chain(&be.bases).filter_map(|c| class(c)) {
            for f in owner.fields.iter().filter(|f| !f.is_static) {
                let ty = simple(f.ty.split(['<', '[']).next().unwrap_or(&f.ty).trim());
                let mut of = scan.classes.iter().filter(|c| simple(&c.name) == ty);
                if let (Some(c), None) = (of.next(), of.next())
                    && c.kind == porter_model::ClassKind::Class
                    && !holds.iter().any(|h| h.name == c.name)
                {
                    holds.push(c);
                }
            }
        }
        for held in holds {
            let mut next = Some(held);
            for _ in 0..3 {
                let Some(c) = next else {
                    break;
                };
                let kinds = scan
                    .classes
                    .iter()
                    .filter(|o| o.superclass.as_deref() == Some(c.name.as_str()))
                    .count();
                if kinds > 6 {
                    break;
                }
                if !mine.iter().any(|(m, _)| m.name == c.name) {
                    let runs = runs_code(c);
                    if !runs.is_empty() {
                        mine.push((c, runs));
                    }
                }
                next = c.superclass.as_deref().and_then(class);
            }
        }
        taken.push(mine);
    }
    if taken.iter().all(|t| t.is_empty()) {
        return;
    }
    // The sources of the classes block entities are made of, read once.
    let mut sources: BTreeMap<&str, String> = BTreeMap::new();
    for name in parts.iter().flatten() {
        if sources.contains_key(name.as_str()) {
            continue;
        }
        if let Some(text) = class(name).and_then(read) {
            sources.insert(name.as_str(), text);
        }
    }
    // How `source` takes a `ty` and calls it: a local or a parameter of that type,
    // with one of the type's methods called on it. Not a class that makes one or
    // keeps one of its own: that is another one giving it code.
    let gets = |source: &str, ty: &ClassInfo, runs: &[String]| -> Option<String> {
        let word = simple(&ty.name);
        if source.contains(&format!("new {word}(")) {
            return None;
        }
        for (at, _) in source.match_indices(&word) {
            let before = source[..at].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            let rest = &source[at + word.len()..];
            let Some(name) = rest.strip_prefix(' ') else {
                continue;
            };
            let end = name
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(name.len());
            let (name, after) = name.split_at(end);
            // (`x instanceof Handler h` is a look at something it was handed, not a take.)
            if !name.starts_with(|c: char| c.is_ascii_lowercase())
                || !after.trim_start().starts_with(['=', ',', ')'])
                || source[..at].trim_end().ends_with("instanceof")
            {
                continue;
            }
            // Called with plain values: a call that hands it code of its own (a lambda
            // to run over its items) is the caller doing its own work through it.
            let called = runs.iter().any(|m| {
                let call = format!("{name}.{m}(");
                source.match_indices(&call).any(|(i, _)| {
                    let args = &source[i + call.len()..];
                    let mut depth = 1;
                    let end = args
                        .char_indices()
                        .find(|(_, c)| {
                            match c {
                                '(' => depth += 1,
                                ')' => depth -= 1,
                                _ => {}
                            }
                            depth == 0
                        })
                        .map_or(args.len(), |(i, _)| i);
                    !args[..end].contains("->") && !args[..end].contains("::")
                })
            });
            if !called {
                continue;
            }
            // The statement, for where it looks: up to its `;`, or the parameter alone.
            let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
            let text = if after.trim_start().starts_with('=') {
                let stop = source[at..].find(';').map_or(source.len(), |i| at + i);
                one_line(&source[line_start..stop])
            } else {
                format!("{word} {name}")
            };
            return Some(text.chars().take(200).collect());
        }
        None
    };
    for i in 0..entities.len() {
        let mut found: Vec<Caller> = Vec::new();
        for (ty, runs) in &taken[i] {
            let fields_of = |c: &ClassInfo| {
                c.fields.iter().any(|f| {
                    simple(f.ty.split(['<', '[']).next().unwrap_or(&f.ty).trim())
                        == simple(&ty.name)
                })
            };
            for (name, source) in &sources {
                if parts[i].iter().any(|p| p == name) || *name == ty.name {
                    continue;
                }
                let Some(c) = class(name) else {
                    continue;
                };
                // One that extends it or keeps one is giving code too, not calling it.
                if c.superclass.as_deref() == Some(ty.name.as_str()) || fields_of(c) {
                    continue;
                }
                let Some(how) = gets(source, ty, runs) else {
                    continue;
                };
                let of: Vec<(String, String)> = entities
                    .iter()
                    .zip(&parts)
                    .filter(|(_, p)| p.iter().any(|p| p == name))
                    .map(|(be, _)| (be.id.clone(), slot_map(be)))
                    .collect();
                if of.is_empty() {
                    continue;
                }
                found.push(Caller {
                    takes: ty.name.clone(),
                    class: name.to_string(),
                    gets: how,
                    entities: of,
                });
            }
        }
        found.truncate(4);
        // Where it sits from them may be two steps off (`pos.above(2)` in their Java).
        if found.iter().any(|c| {
            [
                ".above(2)",
                ".below(2)",
                ".north(2)",
                ".south(2)",
                ".east(2)",
                ".west(2)",
            ]
            .iter()
            .any(|far| c.gets.contains(far))
        }) {
            entities[i].far = true;
        }
        if !found.is_empty() {
            entities[i].neighbours = true;
        }
        entities[i].called_by = found;
    }
}

/// Fills in the state fields and whether it ticks, from the Java class.
fn state_of(scan: &ModScan, class: &ClassInfo, be: &mut BlockEntity) {
    // A tick method (`tick`, `serverTick`), `updateServer` and the like, or a
    // ticking interface of the mod's own (`TickerUtil.TickableServer`).
    be.ticks = class.method_names.iter().any(|m| {
        let m = m.to_ascii_lowercase();
        m.contains("tick") || m == "updateserver" || m == "serverupdate"
    }) || class.interfaces.iter().any(|i| {
        i.rsplit(['.', '$'])
            .next()
            .is_some_and(|s| s.contains("Tick"))
    });
    let mut names = BTreeSet::new();
    // What saveAdditional writes, in its order.
    for saved in &class.saved {
        let Some(kind) = Kind::from_put(&saved.method) else {
            be.left_out.push(format!(
                "`{}` (saved with `{}`): not a plain value",
                saved.key, saved.method
            ));
            continue;
        };
        let value = saved.value.trim().trim_start_matches("this.");
        let field = class
            .fields
            .iter()
            .find(|f| !f.is_static && f.name == value);
        let name = rust_ident(field.map_or(saved.key.as_str(), |f| f.name.as_str()));
        if !names.insert(name.clone()) {
            continue;
        }
        be.fields.push(StateField {
            name,
            kind,
            key: saved.key.clone(),
            saved_by_java: true,
            java: field.map_or_else(|| saved.value.clone(), |f| format!("{} {}", f.ty, f.name)),
            init: field
                .and_then(|f| f.init.as_deref())
                .and_then(|i| initial(kind, i, class)),
            from: class.name.clone(),
        });
    }
    // Fields saveAdditional leaves out still have to last from tick to tick.
    let keys: BTreeSet<&str> = class.saved.iter().map(|s| s.key.as_str()).collect();
    for f in &class.fields {
        if f.is_static {
            continue;
        }
        let name = rust_ident(&f.name);
        if names.contains(&name) {
            continue;
        }
        // An enum of the mod's own is kept as its constant's name.
        let is_enum = is_mod_enum(scan, class, &f.ty);
        let Some(kind) = Kind::from_java(&f.ty).or(is_enum.then_some(Kind::Text)) else {
            be.left_out.push(format!(
                "`{} {}`: no Rust type for it yet; where the Java fills it by looking something up (a recipe it found), look it up again where the Java reads it",
                f.ty, f.name
            ));
            continue;
        };
        let key = if keys.contains(f.name.as_str()) {
            format!("porter_{}", f.name)
        } else {
            f.name.clone()
        };
        names.insert(name.clone());
        be.fields.push(StateField {
            name,
            kind,
            key,
            saved_by_java: false,
            java: if is_enum {
                format!("enum {} {}", f.ty, f.name)
            } else {
                format!("{} {}", f.ty, f.name)
            },
            init: f
                .init
                .as_deref()
                .filter(|_| !is_enum)
                .and_then(|i| initial(kind, i, class)),
            from: class.name.clone(),
        });
    }
    for f in class.fields.iter().filter(|f| f.is_static) {
        be.left_out.push(format!(
            "`static {} {}`: a constant; copy its value into the tick",
            f.ty, f.name
        ));
    }
    // They go into comments: one line each.
    for f in &mut be.fields {
        f.java = one_line(&f.java);
    }
    for l in &mut be.left_out {
        *l = one_line(l);
    }
}

/// The Rust for a Java field's first value, where that is a plain one of the field's
/// kind: a number, `true`, a string, or a constant of `class` that is one
/// (`DEFAULT_SPEED`). None for anything built (`new ArrayList<>()`, a call).
fn initial(kind: Kind, java: &str, class: &ClassInfo) -> Option<String> {
    let text = java.trim();
    // A constant of the class: its own value.
    if text
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && text.starts_with(|c: char| c.is_ascii_uppercase())
    {
        let constant = class
            .fields
            .iter()
            .find(|f| f.is_static && f.name == text)?
            .init
            .as_deref()?;
        if constant.trim() == text {
            return None;
        }
        return initial(kind, constant, class);
    }
    let number = text.replace('_', "");
    match kind {
        // False is what it starts as anyway.
        Kind::Bool => (text == "true").then(|| text.to_string()),
        Kind::Byte | Kind::Short | Kind::Int | Kind::Long => {
            let digits = number.trim_end_matches(['L', 'l']);
            let value: i64 = match digits
                .strip_prefix("0x")
                .or_else(|| digits.strip_prefix("0X"))
            {
                Some(hex) => i64::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            let fits = match kind {
                Kind::Byte => i8::try_from(value).is_ok(),
                Kind::Short => i16::try_from(value).is_ok(),
                Kind::Int => i32::try_from(value).is_ok(),
                _ => true,
            };
            (fits && value != 0).then(|| value.to_string())
        }
        Kind::Float | Kind::Double => {
            let digits = number.trim_end_matches(['f', 'F', 'd', 'D']);
            let value: f64 = digits.parse().ok()?;
            (value != 0.0 && value.is_finite()).then(|| format!("{value:?}"))
        }
        Kind::Text => {
            let inner = text.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.is_empty() && !inner.contains(['"', '\\']))
                .then(|| format!("{inner:?}.to_string()"))
        }
        Kind::Item => None,
    }
}

/// Whether `ty` is an enum of the mod's own: one of its classes, or one nested in
/// `class` (`PressingBehaviour.Mode`, written `Mode` there).
fn is_mod_enum(scan: &ModScan, class: &ClassInfo, ty: &str) -> bool {
    let simple = ty.rsplit(['.', '$']).next().unwrap_or(ty);
    if simple.is_empty() || !simple.starts_with(|c: char| c.is_ascii_uppercase()) {
        return false;
    }
    let nested = format!("{}${simple}", class.name);
    let nested_dot = format!("{}.{simple}", class.name);
    scan.classes.iter().any(|c| {
        c.kind == porter_model::ClassKind::Enum
            && (c.name == nested
                || c.name == nested_dot
                || c.name.rsplit(['.', '$']).next() == Some(simple))
    })
}

/// What a block entity keeps part of itself in: the classes of the mod's own that its
/// fields hold and that save values of their own, or hold items (Create's behaviours:
/// a press's `PressingBehaviour`, a depot's `DepotBehaviour`, a motor's
/// `ScrollValueBehaviour`). Their saved values and lasting fields join its state, named
/// after the field; what they hold of items joins its item slots; their ticks are part
/// of its tick. `owners` are its own class and the mod's above it.
fn hold(scan: &ModScan, owners: &[&ClassInfo], be: &mut BlockEntity) {
    let unique = |ty: &str| {
        let simple = ty.split(['<', '[']).next().unwrap_or(ty).trim();
        let simple = simple.rsplit(['.', '$']).next().unwrap_or(simple);
        let mut found = scan
            .classes
            .iter()
            .filter(|c| c.name.rsplit(['.', '$']).next() == Some(simple));
        match (found.next(), found.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    // A class that wraps one stack with data of its own (a belt's
    // `TransportedItemStack`): one item slot.
    let one_stack = |c: &ClassInfo| {
        c.fields
            .iter()
            .filter(|f| !f.is_static && f.ty == "ItemStack")
            .count()
            == 1
    };
    let own_slots = be.items;
    let mut slots: Vec<(String, usize)> = Vec::new();
    for owner in owners {
        for f in owner.fields.iter().filter(|f| !f.is_static) {
            if be.held.len() >= 6 {
                break;
            }
            let Some(held) = unique(&f.ty) else {
                continue;
            };
            let simple = held.name.rsplit(['.', '$']).next().unwrap_or(&held.name);
            if held.kind != porter_model::ClassKind::Class
                || simple.ends_with("BlockEntity")
                || simple.ends_with("TileEntity")
                || held.is_mixin
                || owners.iter().any(|o| o.name == held.name)
            {
                continue;
            }
            // Its own state, and what the mod's classes above it add.
            let mut chain = vec![held];
            let mut parent = held.superclass.clone();
            for _ in 0..3 {
                let Some(base) = parent
                    .take()
                    .and_then(|n| scan.classes.iter().find(|c| c.name == n))
                else {
                    break;
                };
                chain.push(base);
                parent = base.superclass.clone();
            }
            // It keeps something: values it saves, or items.
            let storage = scan.storages.iter().find(|s| s.class == held.name);
            let saves = chain.iter().any(|c| !c.saved.is_empty());
            let stacks: Vec<&porter_model::FieldInfo> = held
                .fields
                .iter()
                .filter(|g| !g.is_static)
                .filter(|g| unique(&g.ty).is_some_and(one_stack) && !g.ty.contains('<'))
                // (Not what it keeps for drawing: a belt's `lazyClientItem`.)
                .filter(|g| !g.name.to_ascii_lowercase().contains("client"))
                .collect();
            if !saves && storage.is_none_or(|s| s.items == 0) && stacks.is_empty() {
                continue;
            }
            let mut theirs = be.clone();
            theirs.fields.clear();
            theirs.left_out.clear();
            let mut ticks = false;
            for c in &chain {
                let mut part = theirs.clone();
                part.fields.clear();
                state_of(scan, c, &mut part);
                ticks |= part.ticks && c.name == held.name;
                for x in part.fields {
                    if !theirs
                        .fields
                        .iter()
                        .any(|y| y.name == x.name || y.key == x.key)
                    {
                        theirs.fields.push(x);
                    }
                }
            }
            // What its holder starts it with: `generatedSpeed.value = DEFAULT_SPEED;`.
            if let Ok(source) =
                std::fs::read_to_string(std::path::Path::new(&scan.input).join(&owner.path))
            {
                for x in &mut theirs.fields {
                    let member = x.java.rsplit(' ').next().unwrap_or_default();
                    let set = format!("{}.{member} = ", f.name);
                    // Or through its setter, for Create's scroll values:
                    // `angleInput.setValue(45);` starts `value` at 45.
                    let setter = format!("{}.setValue(", f.name);
                    let value = if let Some(at) = source.find(&set) {
                        source[at + set.len()..].split(';').next()
                    } else if member == "value"
                        && let Some(at) = source.find(&setter)
                    {
                        source[at + setter.len()..]
                            .split(';')
                            .next()
                            .and_then(|v| v.trim_end().strip_suffix(')'))
                    } else {
                        None
                    };
                    let Some(value) = value else {
                        continue;
                    };
                    if let Some(init) = initial(x.kind, value, owner) {
                        x.init = Some(init);
                    }
                }
            }
            let prefix = rust_ident(&f.name);
            let mut took = false;
            for mut x in theirs.fields.into_iter().take(24) {
                x.name = format!("{prefix}_{}", x.name.trim_start_matches("r#"));
                if !x.saved_by_java {
                    x.key = format!("{}.{}", f.name, x.key);
                }
                x.java = format!("{simple} {}: {}", f.name, x.java);
                if !be.fields.iter().any(|y| y.name == x.name || y.key == x.key) {
                    be.fields.push(x);
                    took = true;
                }
            }
            // Its items, in the order it declares what holds them.
            let mut its_slots = Vec::new();
            for g in held.fields.iter().filter(|g| !g.is_static) {
                if let Some((_, n)) =
                    storage.and_then(|s| s.inventories.iter().find(|(name, _)| *name == g.name))
                {
                    its_slots.push((format!("{}.{}", f.name, g.name), *n));
                } else if stacks.iter().any(|s| s.name == g.name) {
                    its_slots.push((format!("{}.{}", f.name, g.name), 1));
                }
            }
            if !took && its_slots.is_empty() {
                continue;
            }
            slots.extend(its_slots);
            be.ticks |= ticks;
            be.held.push((f.name.clone(), held.name.clone()));
            // How Java makes it, where it hands it code of the block entity.
            if let Some(made) =
                std::fs::read_to_string(std::path::Path::new(&scan.input).join(&owner.path))
                    .ok()
                    .and_then(|text| made_with(&text, &f.name, simple))
                    .filter(|m| m.contains("::") || m.contains("->"))
            {
                be.made.push((f.name.clone(), made));
            }
            let was = format!("`{} {}`", f.ty, f.name);
            be.left_out.retain(|l| !l.starts_with(&was));
        }
    }
    if !slots.is_empty() {
        // After its own slots, where it has any.
        let mut all = be.inventories.clone();
        if all.is_empty() && own_slots > 0 {
            all.push(("its own".to_string(), own_slots));
        }
        all.extend(slots);
        be.items = all.iter().map(|(_, n)| n).sum();
        be.inventories = all;
    }
}

/// Text for a comment: on one line, whitespace collapsed.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text for a Rust string literal.
fn lit(s: &str) -> String {
    s.escape_default().to_string()
}

/// The runtime.rs code for the block entities: the `Saved` value type, the table of
/// blocks, a state struct per type, its tick stub, and the two functions the hook
/// helpers call. A machine's tick also gets its slots and data slots (`machines`).
pub fn runtime(
    entities: &[BlockEntity],
    machines: &[crate::machine::Machine],
    recipe_types: &[String],
    shared: &[SharedTick],
) -> String {
    let machine_of = |be: &BlockEntity| machines.iter().find(|m| m.entity == be.id);
    let mut r = String::from(
        r#"
/// A value in a block entity's saved compound, as NBT holds it.
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Text(String),
}

/// A position as a block entity keeps one in its saved values (a `BlockPos` field: its
/// source, its controller): `x y z`, as `4 64 -2`. Every function writes one with this
/// and reads it with `text_pos`, so what one saves another can read.
pub fn pos_text(pos: [i32; 3]) -> String {
    format!("{} {} {}", pos[0], pos[1], pos[2])
}

/// The position `text` holds (`pos_text`); none where it is empty (null) or isn't one.
pub fn text_pos(text: &str) -> Option<[i32; 3]> {
    let mut parts = text.split_whitespace().map(str::parse::<i32>);
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(x)), Some(Ok(y)), Some(Ok(z)), None) => Some([x, y, z]),
        _ => None,
    }
}

/// The mod's blocks with a block entity: (block, block entity id, whether it ticks).
pub const BLOCK_ENTITIES: &[(&str, &str, bool)] = &[
"#,
    );
    for be in entities {
        for block in &be.blocks {
            let _ = writeln!(r, "    (\"{block}\", \"{}\", {}),", be.id, be.ticks);
        }
    }
    r.push_str("];\n");
    r.push_str(&shared_runtime(shared));

    for be in entities {
        // Its class, with the base classes it takes state or its tick from.
        let mut java = match &be.class {
            Some(class) if be.bases.is_empty() => class.clone(),
            Some(class) => format!(
                "{class}` and what it inherits from `{}",
                be.bases.join("`, `")
            ),
            None => "its Java class (not found)".to_string(),
        };
        // And what it keeps part of itself in, whose ticks are part of its tick.
        if !be.held.is_empty() {
            let held: Vec<&str> = be.held.iter().map(|(_, c)| c.as_str()).collect();
            let _ = write!(java, "` and what it keeps in `{}", held.join("`, `"));
        }
        let java = java.as_str();
        let _ = write!(
            r,
            "\n/// The state of a `{id}` block entity: `{java}`'s fields, saved with the block.{slots}\n#[derive(Clone, Debug, {default}PartialEq)]\npub struct {ty} {{\n",
            id = be.id,
            ty = be.rust,
            default = if be.fields.iter().any(|f| f.init.is_some()) {
                ""
            } else {
                "Default, "
            },
            slots = if be.inventories.len() > 1 {
                format!(
                    "\n/// Its item handlers share its item slots, one after another: {} (`world.entity_item(pos, slot)`\n/// in a click, `machine.slots[slot]` in its tick).",
                    slot_map(be)
                )
            } else {
                String::new()
            }
        );
        for f in &be.fields {
            let how = if f.saved_by_java {
                format!("saved as `{}`", one_line(&f.key))
            } else {
                format!(
                    "not saved by the Java mod: kept as `{}` so it lasts between ticks",
                    one_line(&f.key)
                )
            };
            let how = if f.java.contains("enum ") {
                format!("{how}; the constant's name (`BELT`), empty before it is set")
            } else if f.java.starts_with("BlockPos ") {
                format!(
                    "{how}; the position as `x y z` (`4 64 -2`: written with `pos_text(pos)`, read with `text_pos(&text)`), empty for none (null)"
                )
            } else if f.java.starts_with("Recipe") {
                format!(
                    "{how}; the recipe's id (`recipe.id`, as `grind:crush_cobblestone`), empty for none (null): the recipe itself is found again among `machine.recipes(kind)` by that id"
                )
            } else if f.kind == Kind::Item {
                format!("{how}; the item's id (`minecraft:bowl`), empty for none")
            } else {
                how
            };
            let how = match &f.init {
                Some(init) => format!("{how}; it starts as `{init}`"),
                None => how,
            };
            let _ = writeln!(
                r,
                "    /// `{}`, {how}.\n    pub {}: {},",
                f.java,
                f.name,
                f.kind.rust()
            );
        }
        r.push_str("}\n");
        // What the Java starts its fields with.
        if be.fields.iter().any(|f| f.init.is_some()) {
            let _ = write!(
                r,
                "\nimpl Default for {} {{\n    /// As a new one is in Java: its fields' own first values.\n    fn default() -> Self {{\n        Self {{\n",
                be.rust
            );
            for f in &be.fields {
                let value = f
                    .init
                    .clone()
                    .unwrap_or_else(|| "Default::default()".to_string());
                let _ = writeln!(r, "            {}: {value},", f.name);
            }
            r.push_str("        }\n    }\n}\n");
        }
        if !be.left_out.is_empty() {
            let _ = writeln!(r, "\n// Left out of {}, to port by hand:", be.rust);
            for l in &be.left_out {
                let _ = writeln!(r, "// - {l}");
            }
        }

        // load and save
        let _ = write!(
            r,
            "\nimpl {ty} {{\n    /// Reads it from the saved compound (`get` reads one key), as `loadAdditional` does;\n    /// a missing key keeps the default.\n    pub fn load(get: impl Fn(&str) -> Option<Saved>) -> Self {{\n",
            ty = be.rust
        );
        if be.fields.is_empty() {
            r.push_str("        let _ = get;\n        Self::default()\n    }\n");
        } else {
            r.push_str("        let mut state = Self::default();\n");
            for f in &be.fields {
                let set = match f.kind {
                    Kind::Bool => format!("state.{} = v != 0", f.name),
                    _ => format!("state.{} = v", f.name),
                };
                let _ = writeln!(
                    r,
                    "        if let Some(Saved::{}(v)) = get(\"{}\") {{\n            {set};\n        }}",
                    f.kind.saved(),
                    lit(&f.key)
                );
            }
            r.push_str("        state\n    }\n");
        }
        r.push_str("\n    /// Its saved values, as `saveAdditional` writes them.\n    pub fn save(&self) -> Vec<(&'static str, Saved)> {\n        vec![\n");
        for f in &be.fields {
            let value = match f.kind {
                Kind::Bool => format!("i8::from(self.{})", f.name),
                Kind::Text | Kind::Item => format!("self.{}.clone()", f.name),
                _ => format!("self.{}", f.name),
            };
            let _ = writeln!(
                r,
                "            (\"{}\", Saved::{}({value})),",
                lit(&f.key),
                f.kind.saved()
            );
        }
        r.push_str("        ]\n    }\n}\n");
        let its_shared = be
            .shared
            .as_ref()
            .and_then(|c| shared.iter().find(|t| t.class == *c));
        if let Some(t) = its_shared {
            r.push_str(&shared_part_impl(&be.rust, &be.fields, t));
        }

        let storage = storage_text(be);
        if be.ticks {
            if machine_of(be).is_none() && takes_machine(be) {
                let mut has = Vec::new();
                if !storage.is_empty() {
                    has.push(storage.clone());
                }
                has.extend(reach(
                    be.neighbours,
                    be.far,
                    be.around,
                    be.renderer.is_some(),
                ));
                if be.blocks.len() > 1 {
                    has.push(format!(
                        "`machine.block`, its own block ({})",
                        be.blocks
                            .iter()
                            .map(|b| format!("`{b}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                let extras = animate_doc(be)
                    + &redstone_doc(be)
                    + &lands_doc(be)
                    + &called_doc(be)
                    + &made_doc(be);
                let extras_todo =
                    animate_todo(be) + &redstone_todo(be) + &lands_todo(be) + &called_todo(be);
                // Its tick is the shared one and nothing more: written here, whole.
                let only_shared = its_shared.filter(|t| {
                    be.class.as_deref() == Some(t.class.as_str())
                        && be.held.is_empty()
                        && extras.is_empty()
                });
                if let Some(t) = only_shared {
                    let _ = write!(
                        r,
                        "\n/// A server tick of the `{id}` block entity at `pos`, 20 a second: the tick of `{simple}`,\n/// its class, which is ported once as `{function}`.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}, machine: &mut Machine<'_>) {{\n    let me = machine.me(pos, &state.save());\n    let mut base = state.{part}_base();\n    let _ = {function}(pos, &mut base, &me, machine);\n    state.set_{part}_base(base);\n}}\n",
                        id = be.id,
                        simple = t.simple(),
                        function = t.function(),
                        part = shared_snake(&t.class),
                        snake = be.snake,
                        ty = be.rust
                    );
                    continue;
                }
                // With a shared tick above it: that is called, and only what is its
                // own is to port.
                let (above_doc, tick_java) = match its_shared {
                    Some(t) => {
                        let part = shared_snake(&t.class);
                        let function = t.function();
                        let doc = format!(
                            "\n/// The tick of `{simple}` and the classes above it is ported once, as `{function}`. Where\n/// this block entity's Java calls `super.tick()` into them, do:\n/// `let me = machine.me(pos, &state.save());`\n/// `let mut base = state.{part}_base();`\n/// `let called = {function}(pos, &mut base, &me, machine);`\n/// `state.set_{part}_base(base);`\n/// `called` names the methods of those classes it called on this block entity\n/// ({examples}): for each one a class of this block entity overrides, do what the override adds\n/// (what those classes do for it is done).{overrides} Port nothing of those classes' ticks here: their\n/// counters, and their walk over the blocks around, are that function's. A method of those classes\n/// that this class's own code calls outside their tick (`applyRecipe()` from its own `tick()`) is\n/// ported here, where it is called, from the Java shown for it.",
                            simple = t.simple(),
                            examples = if be.overrides.is_empty() {
                                t.examples()
                            } else {
                                be.overrides
                                    .iter()
                                    .take(2)
                                    .map(|m| format!("`\"{m}\"`"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            },
                            overrides = if be.overrides.is_empty() {
                                " This block entity's own classes override none of the methods that tick calls."
                                    .to_string()
                            } else {
                                format!(
                                    "\n/// This block entity's own classes override these of them: {}. Each is to be done here when\n/// `called` names it, whatever else the tick does.",
                                    be.overrides
                                        .iter()
                                        .map(|m| format!("`{m}`"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            },
                        );
                        // Its own class and those between, where it isn't the shared one itself.
                        let own = be.class.clone().unwrap_or_default();
                        let between: Vec<&String> =
                            be.bases.iter().take_while(|b| **b != t.class).collect();
                        let mut subject = if own == t.class {
                            format!("this block entity around `{function}`")
                        } else if between.is_empty() {
                            format!("`{own}` (the classes above it are `{function}`)")
                        } else {
                            format!(
                                "`{own}` and what it inherits from `{}` (the classes above them are `{function}`)",
                                between
                                    .iter()
                                    .map(|b| b.as_str())
                                    .collect::<Vec<_>>()
                                    .join("`, `")
                            )
                        };
                        if !be.held.is_empty() {
                            let held: Vec<&str> = be.held.iter().map(|(_, c)| c.as_str()).collect();
                            let _ =
                                write!(subject, " and what it keeps in `{}`", held.join("`, `"));
                        }
                        (doc, subject)
                    }
                    None => (String::new(), format!("`{java}`")),
                };
                let _ = write!(
                    r,
                    "\n/// A server tick of the `{id}` block entity at `pos`, 20 a second. `machine` has\n/// {has}{no_slots}. What the tick changes in them and in `state` is\n/// kept.{above_doc}{extras}\n///\n/// TODO: port the tick of {tick_java}{extras_todo}.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}, machine: &mut Machine<'_>) {{\n    let _ = (pos, state, machine);\n}}\n",
                    id = be.id,
                    has = has.join("; "),
                    no_slots = if be.items == 0 {
                        " (it has no item slots)"
                    } else {
                        ""
                    },
                    snake = be.snake,
                    ty = be.rust
                );
            } else if let Some(m) = machine_of(be) {
                let slots = if m.results.is_empty() {
                    format!("{} slots", m.slots)
                } else {
                    format!(
                        "{} slots (players only take from {})",
                        m.slots,
                        m.results
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let storage = if storage.is_empty() {
                    String::new()
                } else {
                    format!(", {storage}")
                };
                let recipes = if recipe_types.is_empty() {
                    storage
                } else {
                    format!(
                        "{storage}, and the mod's recipes ({})",
                        recipe_types
                            .iter()
                            .map(|t| format!("`{t}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let _ = write!(
                    r,
                    "\n/// A server tick of the `{id}` machine at `pos`, 20 a second. `machine` has its\n/// {slots}, its menu's {data} data slots (`ContainerData`: what its screen\n/// shows){recipes}. What the tick changes in them and in `state` is kept;\n/// `machine.push(slot, Side::Down, 64)` and `machine.pull(...)` move items to and from\n/// the blocks beside it, as a hopper does.{animate}\n///\n/// TODO: port the tick of `{java}`{animate_todo}.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}, machine: &mut Machine<'_>) {{\n    let _ = (pos, state, machine);\n}}\n",
                    id = be.id,
                    data = m.data_slots,
                    animate = animate_doc(be),
                    animate_todo = animate_todo(be),
                    snake = be.snake,
                    ty = be.rust
                );
            } else {
                let _ = write!(
                    r,
                    "\n/// A server tick of the `{id}` block entity at `pos`, 20 a second. What it\n/// changes in `state` is saved with the block.\n///\n/// TODO: port the tick of `{java}`.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}) {{\n    let _ = (pos, state);\n}}\n",
                    id = be.id,
                    snake = be.snake,
                    ty = be.rust
                );
            }
        }
    }

    // What the helpers call.
    r.push_str("\n/// The saved values a new block entity `id` starts with.\npub fn block_entity_defaults(id: &str) -> Vec<(&'static str, Saved)> {\n    match id {\n");
    for be in entities {
        let _ = writeln!(r, "        \"{}\" => {}::default().save(),", be.id, be.rust);
    }
    r.push_str("        _ => Vec::new(),\n    }\n}\n");

    r.push_str("\n/// A server tick of block entity `id` at `pos`: `get` reads its saved compound, and\n/// `machine` has its slots and data slots (none for one without). Returns the values\n/// to save when the tick changed them.\npub fn tick_block_entity(\n    id: &str,\n    pos: [i32; 3],\n    get: impl Fn(&str) -> Option<Saved>,\n    machine: &mut Machine<'_>,\n) -> Option<Vec<(&'static str, Saved)>> {\n");
    let ticking: Vec<&BlockEntity> = entities.iter().filter(|b| b.ticks).collect();
    if ticking.is_empty() {
        r.push_str("    let _ = (id, pos, get, machine);\n    None\n}\n");
    } else {
        r.push_str("    match id {\n");
        for be in ticking {
            let machine_arg = if machine_of(be).is_some() || takes_machine(be) {
                ", machine"
            } else {
                ""
            };
            let _ = writeln!(
                r,
                "        \"{id}\" => {{\n            let before = {ty}::load(get);\n            let mut state = before.clone();\n            tick_{snake}(pos, &mut state{machine_arg});\n            (state != before).then(|| state.save())\n        }}",
                id = be.id,
                ty = be.rust,
                snake = be.snake
            );
        }
        r.push_str(
            "        _ => {\n            let _ = machine;\n            None\n        }\n    }\n}\n",
        );
    }
    // Each block entity's tanks and energy buffer, for the tick and the hooks.
    r.push_str("\n/// Block entities' fluid tanks (capacities in millibuckets) and energy buffer\n/// (capacity, most one insert moves, most one extract moves).\npub const STORAGE: &[(&str, &[i32], Option<[i32; 3]>)] = &[\n");
    for be in entities.iter().filter(|b| !storage_text(b).is_empty()) {
        let _ = writeln!(
            r,
            "    ({:?}, &[{}], {}),",
            be.id,
            be.tanks
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            be.energy.map_or("None".to_string(), |[c, i, o]| format!(
                "Some([{c}, {i}, {o}])"
            ))
        );
    }
    r.push_str("];\n");
    r.push_str("\n/// Block entities whose tick looks at the blocks beside it.\npub const NEIGHBOURS: &[&str] = &[");
    r.push_str(
        &entities
            .iter()
            .filter(|b| b.neighbours)
            .map(|b| format!("{:?}", b.id))
            .collect::<Vec<_>>()
            .join(", "),
    );
    r.push_str("];\n");
    r.push_str("\n/// Block entities whose tick reaches every place of the 3×3×3 around it (`machine.at`).\npub const AROUND: &[&str] = &[");
    r.push_str(
        &entities
            .iter()
            .filter(|b| b.around)
            .map(|b| format!("{:?}", b.id))
            .collect::<Vec<_>>()
            .join(", "),
    );
    r.push_str("];\n");
    r.push_str("\n/// Block entities whose tick reaches the ones two steps away along an axis (`machine.at`).\npub const FAR: &[&str] = &[");
    r.push_str(
        &entities
            .iter()
            .filter(|b| b.far)
            .map(|b| format!("{:?}", b.id))
            .collect::<Vec<_>>()
            .join(", "),
    );
    r.push_str("];\n");
    let drawn: Vec<&BlockEntity> = entities.iter().filter(|b| b.renderer.is_some()).collect();
    if !drawn.is_empty() {
        r.push_str(DRAW_TYPES);
        for be in &drawn {
            let _ = write!(
                r,
                "\n/// Draws block entity `{id}` on the client, once a frame while it is in view, from\n/// what the server sent of its state. `draw` starts at the block's lower north-west\n/// corner, in blocks; `frame` has the partial tick and game time to animate with.\n/// Draw nothing to leave just the block's own model.\n///\n/// TODO: port the `submit` (or `render`) of `{java}`.\npub fn render_{snake}(state: &{ty}, frame: &Frame, draw: &mut Draw) {{\n    let _ = (state, frame, draw);\n}}\n",
                id = be.id,
                java = be.renderer.as_deref().unwrap_or_default(),
                snake = be.snake,
                ty = be.rust
            );
        }
        r.push_str("\n/// Draws block entity `id`: `get` reads what the server sent of its state.\npub fn render_block_entity(\n    id: &str,\n    get: impl Fn(&str) -> Option<Saved>,\n    frame: &Frame,\n    draw: &mut Draw,\n) {\n    RENDER_TIME.with(|t| t.set(frame.time()));\n    match id {\n");
        for be in &drawn {
            let _ = writeln!(
                r,
                "        \"{id}\" => render_{snake}(&{ty}::load(get), frame, draw),",
                id = be.id,
                snake = be.snake,
                ty = be.rust
            );
        }
        r.push_str("        _ => {}\n    }\n");
        // A block whose model Java keeps out of the world shows it from here until
        // its renderer is ported (or where that draws nothing).
        r.push_str("    // A block Java draws from its block entity alone (`getRenderShape`) shows its own\n    // model until its renderer is ported.\n    if draw.ops.is_empty() && DRAWN_BY_ENTITY.contains(&frame.block.as_str()) {\n        let properties: Vec<(&str, &str)> =\n            frame.properties.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();\n        draw.block(&frame.block, &properties);\n    }\n}\n");
        let mut kept_out: Vec<&str> = entities
            .iter()
            .flat_map(|b| b.drawn.iter())
            .map(String::as_str)
            .collect();
        kept_out.sort_unstable();
        kept_out.dedup();
        let list: Vec<String> = kept_out.iter().map(|b| format!("{b:?}")).collect();
        let _ = writeln!(
            r,
            "\n/// The blocks whose own model stays out of the world (`getRenderShape`), for their\n/// block entity to draw.\npub const DRAWN_BY_ENTITY: &[&str] = &[{}];",
            list.join(", ")
        );
    }
    r.push_str(crate::machine::MACHINE_TYPES);
    if !machines.is_empty() {
        r.push_str(&crate::machine::runtime(machines, recipe_types));
    }
    r
}

/// The tick's word on its block's client effects: Horizon has no hook for
/// `animateTick`, so the tick sends them.
fn animate_doc(be: &BlockEntity) -> String {
    match &be.animate {
        Some(class) => format!(
            "\n/// Its block's `animateTick` ({}) plays effects on the client in Java (steam,\n/// bubbles, a sizzle): port them here, sent by the server to the players near with\n/// `machine.particles(..)` and `machine.sound(..)`, with Java's random chances\n/// (`machine.random()`).",
            class.rsplit('.').next().unwrap_or(class)
        ),
        None => String::new(),
    }
}

/// The class of one of `blocks` (or one it is built on, of the mod's) that has `method`.
fn block_class_with(scan: &ModScan, blocks: &[String], method: &str) -> Option<String> {
    scan.registrations
        .iter()
        .filter(|r| r.registry == "block")
        .filter(|r| {
            r.id.as_deref()
                .is_some_and(|id| blocks.iter().any(|b| b.ends_with(&format!(":{id}"))))
        })
        .filter_map(|r| r.creates.as_deref())
        .find_map(|simple| {
            let mut c = scan
                .classes
                .iter()
                .find(|c| c.name.rsplit('.').next() == Some(simple))?;
            for _ in 0..8 {
                if c.method_names.iter().any(|m| m == method) {
                    return Some(c.name.clone());
                }
                let sup = c.superclass.as_deref()?;
                c = scan.classes.iter().find(|k| k.name == sup)?;
            }
            None
        })
}

/// The tick's word on its block's `neighborChanged`: Horizon's neighbour hook can't
/// change blocks everywhere, so the tick keeps the block in step with redstone.
fn redstone_doc(be: &BlockEntity) -> String {
    match &be.redstone {
        Some(class) => format!(
            "\n/// Its block's `neighborChanged` ({}) keeps a state property in step with redstone in\n/// Java (`level.hasNeighborSignal(pos)` is `machine.powered`): do the same at the start of\n/// this tick, comparing `machine.state(..)` and changing it with `machine.set_state(..)`.",
            class.rsplit('.').next().unwrap_or(class)
        ),
        None => String::new(),
    }
}

/// The TODO's mention of its block's `neighborChanged`, so translate reads that class.
fn redstone_todo(be: &BlockEntity) -> String {
    match &be.redstone {
        Some(class) => format!(", and what `{class}`'s `neighborChanged` does"),
        None => String::new(),
    }
}

/// The tick's word on what lands on its block: Horizon gives ported code no hook
/// there, so a block that takes in dropped items does it from its tick.
fn lands_doc(be: &BlockEntity) -> String {
    match &be.lands {
        Some((class, method)) => format!(
            "\n/// Its block's `{method}` ({}) acts on what lands on it in Java, which Horizon gives\n/// ported code no hook for. Where that is taking in dropped items (`ItemEntity`), do it\n/// here each tick: `machine.dropped_in(min, max)` over the block and the one above it,\n/// the items put in its slots as the Java's insert would, and `machine.take_dropped(id,\n/// count)` for what went in. An item resting on top of the block has landed on it: take\n/// what the box holds, without asking which block an item's own position falls in (a\n/// hollow Java lets items drop into may be solid here).",
            class.rsplit('.').next().unwrap_or(class)
        ),
        None => String::new(),
    }
}

/// The TODO's mention of its block's landing method, so translate reads that class.
fn lands_todo(be: &BlockEntity) -> String {
    match &be.lands {
        Some((class, method)) => {
            format!(", and what `{class}`'s `{method}` does with dropped items")
        }
        None => String::new(),
    }
}

/// The tick's word on the block entities that call code it gave: nothing calls across
/// block entities here, so it does that work on them itself.
/// How Java makes `field`, a `class`: `new DeferralBehaviour(this, this::updateBasin)`,
/// from `field = new Class(..)` in `source`, on one line.
fn made_with(source: &str, field: &str, class: &str) -> Option<String> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let at = source
        .match_indices(&format!("{field} = new {class}"))
        .find_map(|(at, m)| {
            let before = source[..at].chars().next_back();
            let after = source[at + m.len()..].chars().next();
            (!before.is_some_and(ident) && matches!(after, Some('(' | '<'))).then_some(at)
        })?;
    let made = &source[at + field.len() + 3..];
    let open = made.find('(')?;
    let mut depth = 0usize;
    let close = made[open..].char_indices().find_map(|(i, c)| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        (depth == 0).then_some(open + i)
    })?;
    let made = one_line(&made[..=close]);
    (made.len() <= 240).then_some(made)
}

/// For a tick: what the block entity keeps that was made with code of its own.
fn made_doc(be: &BlockEntity) -> String {
    if be.made.is_empty() {
        return String::new();
    }
    let made: Vec<String> = be
        .made
        .iter()
        .map(|(field, how)| format!("`{field}` is `{how}`"))
        .collect();
    format!(
        "\n/// What it keeps was made with code of this block entity: {}. A kept part's `tick()` calls\n/// the code it was made with, and nothing else here does: port that tick and the method it is\n/// handed into this tick, each tick, whichever class the method is declared in.",
        made.join("; ")
    )
}

fn called_doc(be: &BlockEntity) -> String {
    if be.called_by.is_empty() {
        return String::new();
    }
    let simple = |name: &str| name.rsplit(['.', '$']).next().unwrap_or(name).to_string();
    let who: Vec<String> = be
        .called_by
        .iter()
        .map(|c| {
            let ids: Vec<String> = c
                .entities
                .iter()
                .take(6)
                .map(|(id, _)| format!("`{id}`"))
                .collect();
            let slots = c
                .entities
                .iter()
                .map(|(_, s)| s.as_str())
                .find(|s| !s.is_empty())
                .map(|s| format!("; item slots: {s}"))
                .unwrap_or_default();
            format!(
                "`{}` of {} takes its `{}` (`{}`{slots})",
                simple(&c.class),
                ids.join(", "),
                simple(&c.takes),
                c.gets
            )
        })
        .collect();
    format!(
        "\n/// Other block entities of the mod call code this one gave: {}. Nothing calls across\n/// block entities here, so this tick does that work itself, each tick: it finds that block\n/// entity where it would look for this one from (`machine.at(..)`; `pos.above(2)` there is\n/// `pos.below(2)` here), and does to it and to itself what that code does when called.",
        who.join("; ")
    )
}

/// The TODO's mention of what calls it, so translate reads those classes.
fn called_todo(be: &BlockEntity) -> String {
    let mut out = String::new();
    for c in &be.called_by {
        out.push_str(&format!(
            ", and what `{}` is called for by `{}`",
            c.takes, c.class
        ));
    }
    out
}

/// Which of its slots each of its item handlers has: "`inputInv` is slot 0, `outputInv`
/// slots 1 to 9". Nothing for one handler.
fn slot_map(be: &BlockEntity) -> String {
    let mut at = 0;
    let mut parts = Vec::new();
    for (field, n) in &be.inventories {
        parts.push(match n {
            0 => continue,
            1 => format!("`{field}` is slot {at}"),
            _ => format!("`{field}` slots {at} to {}", at + n - 1),
        });
        at += n;
    }
    parts.join(", ")
}

/// The TODO's mention of its block's `animateTick`, so translate reads that class.
fn animate_todo(be: &BlockEntity) -> String {
    match &be.animate {
        Some(class) => format!(", and the effects of `{class}`'s `animateTick`"),
        None => String::new(),
    }
}

/// Whether its tick gets a `Machine`: it has slots, tanks, energy, a renderer to
/// send its state to, or neighbours it looks at.
pub fn takes_machine(be: &BlockEntity) -> bool {
    !be.tanks.is_empty()
        || be.energy.is_some()
        || be.renderer.is_some()
        || be.neighbours
        || be.items > 0
        || be.lands.is_some()
}

/// "its 2 fluid tanks (4000 mB each) and its energy (10000 FE)", or nothing.
pub fn storage_text(be: &BlockEntity) -> String {
    let mut parts = Vec::new();
    if be.items > 0 {
        parts.push(if be.items == 1 {
            "its item slot (`machine.slots[0]`, the stack its handler holds)".to_string()
        } else if be.inventories.len() > 1 {
            format!(
                "its {} item slots (`machine.slots`: {})",
                be.items,
                slot_map(be)
            )
        } else {
            format!("its {} item slots (`machine.slots`)", be.items)
        });
    }
    if !be.tanks.is_empty() {
        let caps: Vec<String> = be.tanks.iter().map(|c| format!("{c} mB")).collect();
        parts.push(if be.tanks.len() == 1 {
            format!("its fluid tank ({}, `machine.tanks[0]`)", caps[0])
        } else {
            format!(
                "its {} fluid tanks ({}, `machine.tanks`, in the order its Java makes them)",
                be.tanks.len(),
                caps.join(", ")
            )
        });
    }
    if let Some([cap, i, o]) = be.energy {
        parts.push(format!(
            "its energy (`machine.energy`, up to {cap} FE; {i} in and {o} out at most each time)"
        ));
    }
    parts.join(" and ")
}

/// What a block entity renderer draws with: a pose stack and parts, recorded so the
/// renderer is plain code (and testable); the `block_entity_submit` helper draws
/// them with Horizon's `BlockEntityDraw`.
pub const DRAW_TYPES: &str = r#"
thread_local! {
    // The time of the frame being drawn: see `render_time`.
    static RENDER_TIME: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// The time of the frame being drawn, in ticks and smooth between them (`Frame::time`),
/// for a function called while drawing that isn't handed the frame: a helper that
/// works out a shaft's angle. What Java reads from a clock of the client's own
/// (`AnimationTickHolder.getRenderTime(level)`, `level.getGameTime() + partialTicks`).
pub fn render_time() -> f32 {
    RENDER_TIME.with(|t| t.get())
}

/// A frame of a block entity drawn on the client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub pos: [i32; 3],
    /// The block (`grind:crusher`) and its `facing`, if it has one.
    pub block: String,
    pub facing: Option<String>,
    /// Its state properties (`be.getBlockState().getValue(AXIS)`): see `state`.
    pub properties: Vec<(String, String)>,
    /// The frame's partial tick, 0 to 1, and the level's game time.
    pub partial: f32,
    pub game_time: i64,
    /// Its item slots as the server sent them (`getItem(slot)`): what a renderer
    /// draws on it (a skillet's food, a cutting board's ingredient). (item, count).
    pub items: Vec<Option<(String, u32)>>,
}

impl Frame {
    /// Its block's state property `key` (`getValue(AXIS)`): `"y"` for a pillar's
    /// `axis`, `"true"` for `lit`.
    pub fn state(&self, key: &str) -> Option<&str> {
        self.properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Ticks since the world began, smooth between ticks: for animations.
    pub fn time(&self) -> f32 {
        (self.game_time % 24_000) as f32 + self.partial
    }

    /// Degrees to turn a model that faces north to face `facing`
    /// (`Direction.toYRot`), about the Y axis.
    pub fn facing_degrees(&self) -> f32 {
        match self.facing.as_deref() {
            Some("south") => 180.0,
            Some("west") => 90.0,
            Some("east") => 270.0,
            _ => 0.0,
        }
    }
}

/// One step of drawing.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    Push,
    Pop,
    Translate([f32; 3]),
    /// Degrees about `axis` (counterclockwise seen from its positive end) through
    /// `pivot`, in blocks.
    Rotate { axis: [f32; 3], degrees: f32, pivot: [f32; 3] },
    Scale([f32; 3]),
    /// A block's model in a state: `("minecraft:grindstone", [("face", "floor")])`.
    Block { block: String, properties: Vec<(String, String)> },
    /// A standalone model the mod bakes (`grind:block/crusher_wheel`, in
    /// `STANDALONE_MODELS`), its tinted faces times 0xRRGGBB `tint`.
    Model { model: String, tint: u32 },
    /// A box in texels (16 a block), every face with `sprite` (`minecraft:block/stone`).
    Cube { from: [f32; 3], to: [f32; 3], sprite: String, tint: u32 },
    /// An item as it sits in an item frame.
    Item { item: String, count: u32 },
    /// What was drawn doesn't move by itself: Horizon keeps it until the state changes.
    Keep,
}

/// A pose stack and the parts drawn under it (`PoseStack`, `SubmitNodeCollector`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draw {
    pub ops: Vec<DrawOp>,
}

impl Draw {
    /// `pose.pushPose()`.
    pub fn push(&mut self) {
        self.ops.push(DrawOp::Push);
    }
    /// `pose.popPose()`.
    pub fn pop(&mut self) {
        self.ops.push(DrawOp::Pop);
    }
    /// `pose.translate(x, y, z)`, in blocks.
    pub fn translate(&mut self, by: [f32; 3]) {
        self.ops.push(DrawOp::Translate(by));
    }
    /// `pose.mulPose(Axis.YP.rotationDegrees(d))` is `rotate([0.0, 1.0, 0.0], d)`: about
    /// the pose's origin as it is now (after a `translate`, where that moved it).
    pub fn rotate(&mut self, axis: [f32; 3], degrees: f32) {
        self.rotate_about(axis, degrees, [0.0; 3]);
    }
    /// A turn through `pivot`, counted from the pose's origin as it is now: `[0.5; 3]` is
    /// the block's centre while nothing has moved the origin. It is TransformStack's
    /// `rotateCentered` (and `center()`, a turn, `uncenter()`); after `translate([0.5, ..])`
    /// a turn Java makes with `mulPose` is `rotate`, about the origin it moved to.
    ///
    /// As with a `PoseStack`, the turn made last reaches the model first, and each
    /// later one is about the axes the earlier ones left. So where Java turns a model
    /// to its facing and then spins it about a world axis (a buffer made already
    /// turned, `partialFacing(model, state)` or a block state's own model, then
    /// `rotateCentered(angle, axis)` or `kineticRotationTransform`), make the spin
    /// first and the turn to the facing after it: the other way round the spin goes
    /// about the model's own axis, and it tumbles.
    pub fn rotate_about(&mut self, axis: [f32; 3], degrees: f32, pivot: [f32; 3]) {
        self.ops.push(DrawOp::Rotate { axis, degrees, pivot });
    }
    /// `pose.scale(x, y, z)`.
    pub fn scale(&mut self, by: [f32; 3]) {
        self.ops.push(DrawOp::Scale(by));
    }
    /// A block's model (`renderSingleBlock`, `submitBlock`): any block, any state.
    pub fn block(&mut self, block: &str, properties: &[(&str, &str)]) {
        self.ops.push(DrawOp::Block {
            block: block.to_string(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        });
    }
    /// A part's own model (Flywheel's `PartialModel`, a standalone model), as a
    /// block's model is drawn: `draw.model("grind:block/crusher_wheel", 0xFFFFFF)`.
    pub fn model(&mut self, model: &str, tint: u32) {
        self.ops.push(DrawOp::Model { model: model.to_string(), tint });
    }
    /// A box from `from` to `to` in texels, its faces all `sprite`, tinted 0xRRGGBB.
    pub fn cube(&mut self, from: [f32; 3], to: [f32; 3], sprite: &str, tint: u32) {
        self.ops.push(DrawOp::Cube { from, to, sprite: sprite.to_string(), tint });
    }
    /// An item, as one on a shelf or in a frame.
    pub fn item(&mut self, item: &str, count: u32) {
        self.ops.push(DrawOp::Item { item: item.to_string(), count });
    }
    /// What this frame drew stands still until the state changes (an idle machine).
    pub fn keep(&mut self) {
        self.ops.push(DrawOp::Keep);
    }
}
"#;

/// Tests for the generated module: each state survives a save and a load.
pub fn runtime_tests(entities: &[BlockEntity]) -> String {
    let mut t = String::new();
    for be in entities {
        if let Some(shared) = &be.shared {
            let _ = write!(
                t,
                "\n    #[test]\n    fn {snake}_gives_and_takes_its_shared_part() {{\n        let state = super::{ty}::default();\n        let mut back = state.clone();\n        back.set_{part}_base(state.{part}_base());\n        assert_eq!(back, state);\n    }}\n",
                snake = be.snake,
                ty = be.rust,
                part = shared_snake(shared)
            );
        }
        let _ = write!(
            t,
            "\n    #[test]\n    fn {snake}_state_saves_and_loads() {{\n        let state = super::{ty}::default();\n        let saved = state.save();\n        let loaded = super::{ty}::load(|key| {{\n            saved.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())\n        }});\n        assert_eq!(loaded, state);\n        assert_eq!(super::block_entity_defaults(\"{id}\"), saved);\n    }}\n",
            snake = be.snake,
            ty = be.rust,
            id = be.id
        );
        if be.renderer.is_some() {
            // Every pose it pushes, it pops: at rest and mid-animation.
            let _ = write!(
                t,
                "\n    #[test]\n    fn {snake}_draws_with_balanced_poses() {{\n        use super::DrawOp;\n        for game_time in [0, 37, 23_999] {{\n            let frame = super::Frame {{\n                game_time,\n                partial: 0.5,\n                block: \"{id}\".into(),\n                facing: Some(\"east\".into()),\n                ..Default::default()\n            }};\n            let mut draw = super::Draw::default();\n            super::render_{snake}(&super::{ty}::default(), &frame, &mut draw);\n            let mut depth = 0i32;\n            for op in &draw.ops {{\n                match op {{\n                    DrawOp::Push => depth += 1,\n                    DrawOp::Pop => depth -= 1,\n                    _ => {{}}\n                }}\n                assert!(depth >= 0, \"popped a pose it never pushed\");\n            }}\n            assert_eq!(depth, 0, \"pushed a pose it never popped\");\n        }}\n    }}\n",
                snake = be.snake,
                ty = be.rust,
                id = be.id
            );
        }
    }
    t
}

/// How a helper turns a `Tag` into a `Saved` (`m` is the module).
pub const FROM_TAG: &str = "match tag {
            Tag::Byte(v) => Some(m::Saved::Byte(*v)),
            Tag::Short(v) => Some(m::Saved::Short(*v)),
            Tag::Int(v) => Some(m::Saved::Int(*v)),
            Tag::Long(v) => Some(m::Saved::Long(*v)),
            Tag::Float(v) => Some(m::Saved::Float(f32::from_bits(*v))),
            Tag::Double(v) => Some(m::Saved::Double(f64::from_bits(*v))),
            Tag::String(v) => Some(m::Saved::Text(v.to_string_lossy())),
            _ => None,
        }";

/// How a helper turns a `Saved` into a `Tag`.
pub const TO_TAG: &str = "match value {
            m::Saved::Byte(v) => Tag::Byte(v),
            m::Saved::Short(v) => Tag::Short(v),
            m::Saved::Int(v) => Tag::Int(v),
            m::Saved::Long(v) => Tag::Long(v),
            m::Saved::Float(v) => Tag::Float(v.to_bits()),
            m::Saved::Double(v) => Tag::Double(v.to_bits()),
            m::Saved::Text(v) => Tag::String(v.as_str().into()),
        }";

fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        let part = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        !part(before) && !part(after)
    })
}

/// A Rust identifier from a Java name or an id: `maxSpeed` → `max_speed`.
pub(crate) fn rust_ident(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.is_empty() {
        "state".to_string()
    } else {
        out
    };
    let out = if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("n{out}")
    } else {
        out
    };
    // Rust keywords a Java field could be named.
    match out.as_str() {
        "type" | "match" | "mod" | "move" | "ref" | "self" | "use" | "where" | "fn" | "impl"
        | "let" | "loop" | "mut" | "pub" | "crate" | "dyn" | "trait" | "unsafe" | "async"
        | "await" | "box" | "in" | "as" | "struct" | "enum" | "extern" | "gen" => {
            format!("r#{out}")
        }
        _ => out,
    }
}

/// `crank_shaft` → `CrankShaft`.
fn camel(snake: &str) -> String {
    let s: String = snake
        .trim_start_matches("r#")
        .split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("N{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_rust_things() {
        assert_eq!(rust_ident("maxSpeed"), "max_speed");
        assert_eq!(rust_ident("Speed"), "speed");
        assert_eq!(rust_ident("type"), "r#type");
        assert_eq!(rust_ident("crank.v2"), "crank_v2");
        assert_eq!(camel("crank_shaft"), "CrankShaft");
        assert_eq!(camel("r#type"), "Type");
        assert!(has_word("of(X::new, CRANK.get())", "CRANK"));
        assert!(!has_word("of(X::new, CRANK_2.get())", "CRANK"));
    }

    #[test]
    fn reads_a_fields_first_value() {
        let class = ClassInfo {
            name: "a.Motor".into(),
            kind: porter_model::ClassKind::Class,
            path: String::new(),
            superclass: None,
            interfaces: Vec::new(),
            references: Vec::new(),
            side: porter_model::Side::Common,
            is_mixin: false,
            lines: 0,
            methods: 0,
            fields: vec![porter_model::FieldInfo {
                name: "DEFAULT_SPEED".into(),
                ty: "int".into(),
                is_static: true,
                is_final: true,
                init: Some("16".into()),
            }],
            method_names: Vec::new(),
            saved: Vec::new(),
        };
        assert_eq!(initial(Kind::Int, "20", &class).as_deref(), Some("20"));
        assert_eq!(
            initial(Kind::Int, " DEFAULT_SPEED", &class).as_deref(),
            Some("16")
        );
        assert_eq!(
            initial(Kind::Float, "70.0f", &class).as_deref(),
            Some("70.0")
        );
        assert_eq!(initial(Kind::Float, "1f", &class).as_deref(), Some("1.0"));
        assert_eq!(initial(Kind::Bool, "true", &class).as_deref(), Some("true"));
        assert_eq!(
            initial(Kind::Text, "\"idle\"", &class).as_deref(),
            Some("\"idle\".to_string()")
        );
        // Zero is what it starts as anyway; a thing built is not a plain value.
        assert_eq!(initial(Kind::Int, "0", &class), None);
        assert_eq!(initial(Kind::Int, "new Random().nextInt()", &class), None);
        assert_eq!(initial(Kind::Int, "OTHER", &class), None);
    }

    #[test]
    fn maps_java_types() {
        assert_eq!(Kind::from_java("float"), Some(Kind::Float));
        assert_eq!(Kind::from_java("java.lang.String"), Some(Kind::Text));
        assert_eq!(Kind::from_java("ItemStackHandler"), None);
        assert_eq!(Kind::from_java("ItemStack"), Some(Kind::Item));
        // A recipe a block entity found is kept by its id.
        assert_eq!(Kind::from_java("Recipe<?>"), Some(Kind::Text));
        assert_eq!(Kind::from_java("RecipeHolder<?>"), Some(Kind::Text));
        assert_eq!(Kind::from_java("List<Recipe<?>>"), None);
        assert_eq!(Kind::from_put("putBoolean"), Some(Kind::Bool));
    }
}
