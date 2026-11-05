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
            Kind::Text => "String",
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
            Kind::Text => "Text",
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
            .filter(|b| has_word(&reg.args, &b.to_ascii_uppercase()))
            .map(|b| format!("{ns}:{b}"))
            .collect();
        if its_blocks.is_empty() && blocks.contains(&name) {
            its_blocks.push(format!("{ns}:{name}"));
        }
        let class = reg.creates.as_deref().and_then(|simple| {
            let mut found = scan
                .classes
                .iter()
                .filter(|c| c.name.rsplit('.').next() == Some(simple));
            let first = found.next()?;
            found.next().is_none().then_some(first)
        });
        let snake = rust_ident(name);
        let mut be = BlockEntity {
            id: format!("{ns}:{name}"),
            blocks: its_blocks,
            class: class.map(|c| c.name.clone()),
            rust: camel(&snake),
            snake,
            fields: Vec::new(),
            ticks: false,
            left_out: Vec::new(),
        };
        if let Some(class) = class {
            state_of(class, &mut be);
        }
        out.push(be);
    }
    out
}

/// Fills in the state fields and whether it ticks, from the Java class.
fn state_of(class: &ClassInfo, be: &mut BlockEntity) {
    be.ticks = class
        .method_names
        .iter()
        .any(|m| m.to_ascii_lowercase().contains("tick"));
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
/// helpers call.
pub fn runtime(entities: &[BlockEntity]) -> String {
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
                Kind::Text => format!("self.{}.clone()", f.name),
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

        if be.ticks {
            let _ = write!(
                r,
                "\n/// A server tick of the `{id}` block entity at `pos`, 20 a second. What it\n/// changes in `state` is saved with the block.\n///\n/// TODO: port the tick of `{java}`.\npub fn tick_{snake}(pos: [i32; 3], state: &mut {ty}) {{\n    let _ = (pos, state);\n}}\n",
                id = be.id,
                snake = be.snake,
                ty = be.rust
            );
        }
    }

    // What the helpers call.
    r.push_str("\n/// The saved values a new block entity `id` starts with.\npub fn block_entity_defaults(id: &str) -> Vec<(&'static str, Saved)> {\n    match id {\n");
    for be in entities {
        let _ = writeln!(r, "        \"{}\" => {}::default().save(),", be.id, be.rust);
    }
    r.push_str("        _ => Vec::new(),\n    }\n}\n");

    r.push_str("\n/// A server tick of block entity `id` at `pos`: `get` reads its saved compound.\n/// Returns the values to save when the tick changed them.\npub fn tick_block_entity(\n    id: &str,\n    pos: [i32; 3],\n    get: impl Fn(&str) -> Option<Saved>,\n) -> Option<Vec<(&'static str, Saved)>> {\n");
    let ticking: Vec<&BlockEntity> = entities.iter().filter(|b| b.ticks).collect();
    if ticking.is_empty() {
        r.push_str("    let _ = (id, pos, get);\n    None\n}\n");
    } else {
        r.push_str("    match id {\n");
        for be in ticking {
            let _ = writeln!(
                r,
                "        \"{id}\" => {{\n            let before = {ty}::load(get);\n            let mut state = before.clone();\n            tick_{snake}(pos, &mut state);\n            (state != before).then(|| state.save())\n        }}",
                id = be.id,
                ty = be.rust,
                snake = be.snake
            );
        }
        r.push_str("        _ => None,\n    }\n}\n");
    }
    r
}

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
        assert_eq!(Kind::from_put("putBoolean"), Some(Kind::Bool));
    }
}
