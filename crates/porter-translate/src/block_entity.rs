// Block entities: each Java block entity type becomes a state struct in runtime.rs,
// read from and written to the block's saved compound, with a tick stub. Horizon's
// `native_block_entity` gives the mod's blocks their block entity,
// `native_block_entity_ticks` says which tick, and `native_block_entity_tick` runs
// the stub on the server, 20 times a second.

use std::collections::BTreeSet;
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
    /// The class of its block whose `animateTick` plays effects on the client
    /// (a pot's bubbles, a skillet's sizzle), fully qualified: its tick sends them.
    pub animate: Option<String>,
    /// Whether its Java code looks at the blocks beside it (it uses `Direction`):
    /// its tick then gets them (`machine.neighbour(side)`).
    pub neighbours: bool,
    /// Its blocks' class with a `neighborChanged` (redstone turning it on or off).
    pub redstone: Option<String>,
    /// Java fields the state leaves out, with why.
    pub left_out: Vec<String>,
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
        if its_blocks.is_empty() && blocks.contains(&name) {
            its_blocks.push(format!("{ns}:{name}"));
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
            redstone,
            animate,
            neighbours: false,
            left_out: Vec::new(),
        };
        if let Some(class) = class {
            state_of(class, &mut be);
            let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
            be.renderer = scan
                .renderers
                .iter()
                .find(|r| r.entity == simple)
                .map(|r| r.class.clone());
            be.neighbours = class
                .references
                .iter()
                .any(|r| r == "net.minecraft.core.Direction");
            if let Some(s) = scan.storages.iter().find(|s| s.class == class.name) {
                be.tanks = s.tanks.clone();
                be.energy = s.energy;
                be.items = s.items;
            }
        }
        out.push(be);
    }
    out
}

/// Fills in the state fields and whether it ticks, from the Java class.
fn state_of(class: &ClassInfo, be: &mut BlockEntity) {
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
        let Some(kind) = Kind::from_java(&f.ty) else {
            be.left_out
                .push(format!("`{} {}`: no Rust type for it yet", f.ty, f.name));
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
            java: format!("{} {}", f.ty, f.name),
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

    for be in entities {
        let java = be.class.as_deref().unwrap_or("its Java class (not found)");
        let _ = write!(
            r,
            "\n/// The state of a `{id}` block entity: `{java}`'s fields, saved with the block.\n#[derive(Clone, Debug, Default, PartialEq)]\npub struct {ty} {{\n",
            id = be.id,
            ty = be.rust
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
            let how = if f.kind == Kind::Item {
                format!("{how}; the item's id (`minecraft:bowl`), empty for none")
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

        let storage = storage_text(be);
        if be.ticks {
            if machine_of(be).is_none() && takes_machine(be) {
                let mut has = Vec::new();
                if !storage.is_empty() {
                    has.push(storage.clone());
                }
                if be.neighbours {
                    has.push("the blocks beside it (`machine.neighbour(Side::Up)`, with their saved state; `machine.set_neighbour(...)` changes one after the tick)".to_string());
                }
                if be.renderer.is_some() {
                    has.push("`machine.sync()`, which sends clients its state again for its renderer (`sendBlockUpdated`)".to_string());
                }
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
                let _ = write!(
                    r,
                    "\n/// A server tick of the `{id}` block entity at `pos`, 20 a second. `machine` has\n/// {has}{no_slots}. What the tick changes in them and in `state` is\n/// kept.{animate}\n///\n/// TODO: port the tick of `{java}`{animate_todo}.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}, machine: &mut Machine<'_>) {{\n    let _ = (pos, state, machine);\n}}\n",
                    id = be.id,
                    has = has.join("; "),
                    no_slots = if be.items == 0 {
                        " (it has no item slots)"
                    } else {
                        ""
                    },
                    animate = animate_doc(be) + &redstone_doc(be),
                    animate_todo = animate_todo(be) + &redstone_todo(be),
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
        r.push_str("\n/// Draws block entity `id`: `get` reads what the server sent of its state.\npub fn render_block_entity(\n    id: &str,\n    get: impl Fn(&str) -> Option<Saved>,\n    frame: &Frame,\n    draw: &mut Draw,\n) {\n    match id {\n");
        for be in &drawn {
            let _ = writeln!(
                r,
                "        \"{id}\" => render_{snake}(&{ty}::load(get), frame, draw),",
                id = be.id,
                snake = be.snake,
                ty = be.rust
            );
        }
        r.push_str("        _ => {}\n    }\n}\n");
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
}

/// "its 2 fluid tanks (4000 mB each) and its energy (10000 FE)", or nothing.
pub fn storage_text(be: &BlockEntity) -> String {
    let mut parts = Vec::new();
    if be.items > 0 {
        parts.push(if be.items == 1 {
            "its item slot (`machine.slots[0]`, the stack its handler holds)".to_string()
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
                "its {} fluid tanks ({}, `machine.tanks`)",
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
/// A frame of a block entity drawn on the client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub pos: [i32; 3],
    /// The block (`grind:crusher`) and its `facing`, if it has one.
    pub block: String,
    pub facing: Option<String>,
    /// The frame's partial tick, 0 to 1, and the level's game time.
    pub partial: f32,
    pub game_time: i64,
    /// Its item slots as the server sent them (`getItem(slot)`): what a renderer
    /// draws on it (a skillet's food, a cutting board's ingredient). (item, count).
    pub items: Vec<Option<(String, u32)>>,
}

impl Frame {
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
    /// `pose.mulPose(Axis.YP.rotationDegrees(d))` is `rotate([0.0, 1.0, 0.0], d)`.
    pub fn rotate(&mut self, axis: [f32; 3], degrees: f32) {
        self.rotate_about(axis, degrees, [0.0; 3]);
    }
    /// A turn through `pivot`: `[0.5; 3]` is the block's centre.
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
fn rust_ident(name: &str) -> String {
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
    fn maps_java_types() {
        assert_eq!(Kind::from_java("float"), Some(Kind::Float));
        assert_eq!(Kind::from_java("java.lang.String"), Some(Kind::Text));
        assert_eq!(Kind::from_java("ItemStackHandler"), None);
        assert_eq!(Kind::from_java("ItemStack"), Some(Kind::Item));
        assert_eq!(Kind::from_put("putBoolean"), Some(Kind::Bool));
    }
}
