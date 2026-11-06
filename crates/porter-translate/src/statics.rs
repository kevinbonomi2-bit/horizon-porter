// The Java mod's own static fields, carried into runtime.rs so every ported
// function shares them: `SteadyView.pickups` becomes `steady_view::PICKUPS`.
// Constants (static final with a literal) stay constants; fields the Java
// changes become atomics, or a `Mutex` for floats and text.

use std::collections::BTreeSet;
use std::fmt::Write;

use porter_model::ModScan;

use crate::block_entity::Kind;

/// One static field.
#[derive(Debug, Clone, PartialEq)]
pub struct Static {
    /// `SteadyView.pickups`.
    pub java: String,
    /// The Java declaration: `static int pickups`.
    pub declared: String,
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `pub const NAME: T = literal;`
    Const { ty: &'static str, literal: String },
    /// `pub static NAME: Atomic.. = Atomic..::new(literal);`
    Atomic { ty: &'static str, literal: String },
    /// `pub static NAME: Mutex<T> = Mutex::new(literal);`
    Locked { ty: &'static str, literal: String },
    /// Its initializer is no literal the porter can carry: left to port.
    LeftOut { why: String },
}

/// A class's statics, as one Rust module.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// The class, fully qualified.
    pub class: String,
    /// `steady_view`.
    pub name: String,
    pub statics: Vec<Static>,
}

/// The statics of the mod's classes in `roots` (Mixins, handlers, block
/// entities) and of the mod's classes they refer to.
pub fn find(scan: &ModScan, roots: &BTreeSet<String>) -> Vec<Module> {
    let mut classes: BTreeSet<&str> = BTreeSet::new();
    for class in scan.classes.iter().filter(|c| roots.contains(&c.name)) {
        classes.insert(&class.name);
        for r in &class.references {
            if scan.classes.iter().any(|c| c.name == *r) {
                classes.insert(r);
            }
        }
    }
    let mut names = BTreeSet::new();
    let mut out = Vec::new();
    for class in scan
        .classes
        .iter()
        .filter(|c| classes.contains(c.name.as_str()) && !c.is_mixin)
    {
        let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
        let statics: Vec<Static> = class
            .fields
            .iter()
            .filter(|f| f.is_static)
            .filter_map(|f| {
                let kind = Kind::from_java(&f.ty)?;
                Some(Static {
                    java: format!("{simple}.{}", f.name),
                    declared: format!(
                        "static {}{} {}{}",
                        if f.is_final { "final " } else { "" },
                        f.ty,
                        f.name,
                        f.init
                            .as_deref()
                            .map(|i| format!(" = {}", one_line(i)))
                            .unwrap_or_default()
                    ),
                    name: screaming(&f.name),
                    value: value(kind, f.is_final, f.init.as_deref()),
                })
            })
            .collect();
        // Two Java fields can make one Rust name (`MAX_OVERHANG` and
        // `maxOverhang`): the later ones get a number.
        let mut statics = statics;
        let mut taken: BTreeSet<String> = BTreeSet::new();
        for s in &mut statics {
            let base = s.name.clone();
            let mut n = 2;
            while !taken.insert(s.name.clone()) {
                s.name = format!("{base}_{n}");
                n += 1;
            }
        }
        let name = snake(simple);
        if statics.is_empty() || !names.insert(name.clone()) {
            continue;
        }
        out.push(Module {
            class: class.name.clone(),
            name,
            statics,
        });
    }
    out
}

/// The Rust for a static of `kind`.
fn value(kind: Kind, is_final: bool, init: Option<&str>) -> Value {
    let literal = match init {
        Some(i) => literal(kind, i),
        // Java's defaults.
        None => Some(match kind {
            Kind::Bool => "false".into(),
            Kind::Float | Kind::Double => "0.0".into(),
            Kind::Text => "String::new()".into(),
            _ => "0".into(),
        }),
    };
    let Some(literal) = literal else {
        return Value::LeftOut {
            why: format!(
                "its value, `{}`, is no literal",
                one_line(init.unwrap_or(""))
            ),
        };
    };
    if is_final {
        let (ty, literal) = match kind {
            Kind::Text => ("&str", literal),
            _ => (kind.rust(), literal),
        };
        if kind == Kind::Text && init.is_none() {
            return Value::LeftOut {
                why: "a final text field with no value here".into(),
            };
        }
        return Value::Const { ty, literal };
    }
    match kind {
        Kind::Bool => Value::Atomic {
            ty: "AtomicBool",
            literal,
        },
        Kind::Byte => Value::Atomic {
            ty: "AtomicI8",
            literal,
        },
        Kind::Short => Value::Atomic {
            ty: "AtomicI16",
            literal,
        },
        Kind::Int => Value::Atomic {
            ty: "AtomicI32",
            literal,
        },
        Kind::Long => Value::Atomic {
            ty: "AtomicI64",
            literal,
        },
        Kind::Float => Value::Locked { ty: "f32", literal },
        Kind::Double => Value::Locked { ty: "f64", literal },
        Kind::Text => {
            if init.is_some() && literal != "String::new()" {
                // A Mutex<String> must start empty in a static: the text is set on first use.
                Value::LeftOut {
                    why: format!("a changing text field that starts as {literal}"),
                }
            } else {
                Value::Locked {
                    ty: "String",
                    literal: "String::new()".into(),
                }
            }
        }
    }
}

/// A Java literal as Rust, for a field of `kind`: `70.0f` → `70.0`, `20 * 60` → `20 * 60`.
fn literal(kind: Kind, init: &str) -> Option<String> {
    let t = init.trim();
    match kind {
        Kind::Bool => (t == "true" || t == "false").then(|| t.to_string()),
        Kind::Text => {
            let inner = t.strip_prefix('"')?.strip_suffix('"')?;
            java_string(inner).map(|s| format!("\"{s}\""))
        }
        Kind::Float | Kind::Double => {
            let n = t.trim_end_matches(['f', 'F', 'd', 'D']);
            let v: f64 = n.parse().ok()?;
            Some(if n.contains(['.', 'e', 'E']) {
                n.to_string()
            } else {
                format!("{v:.1}")
            })
        }
        _ => {
            // An integer, or arithmetic on integers (`20 * 60`).
            let ok = !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || " +-*/()_xXabcdefABCDEFlL".contains(c))
                && t.chars().any(|c| c.is_ascii_digit());
            ok.then(|| t.replace(['l', 'L'], ""))
        }
    }
}

/// The inside of a Java string literal as the inside of a Rust one: `§` is
/// `\u{00A7}`, `\b` and `\f` become `\u{8}` and `\u{c}`. None for what Rust can't
/// write the same (octal escapes, an unescaped quote).
fn java_string(inner: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => return None,
            '\\' => match chars.next()? {
                'u' => {
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    if hex.len() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                        return None;
                    }
                    out.push_str(&format!("\\u{{{hex}}}"));
                }
                'b' => out.push_str("\\u{8}"),
                'f' => out.push_str("\\u{c}"),
                e @ ('n' | 't' | 'r' | '"' | '\'' | '\\') => {
                    out.push('\\');
                    out.push(e);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// runtime.rs's section for the statics.
pub fn runtime(modules: &[Module]) -> String {
    if modules.is_empty() {
        return String::new();
    }
    let mut r = String::from(
        "\n// The Java mod's own static fields, shared by every function here and by the\n\
         // hook helpers: `SteadyView.pickups` is `steady_view::PICKUPS`. Constants stay\n\
         // constants; fields the Java changes are atomics, or a `Mutex` for numbers with\n\
         // a fraction and for text.\n",
    );
    for m in modules {
        let _ = write!(
            r,
            "\n/// `{}`'s static fields.\npub mod {} {{\n",
            m.class, m.name
        );
        for s in &m.statics {
            if let Value::LeftOut { why } = &s.value {
                let _ = writeln!(
                    r,
                    "    // `{}`: {} is left to port: {why}.",
                    s.declared, s.name
                );
                continue;
            }
            let _ = writeln!(r, "    /// `{}`.", s.declared);
            match &s.value {
                Value::Const { ty, literal } => {
                    let _ = writeln!(r, "    pub const {}: {ty} = {literal};", s.name);
                }
                Value::Atomic { ty, literal } => {
                    let _ = writeln!(
                        r,
                        "    pub static {}: std::sync::atomic::{ty} = std::sync::atomic::{ty}::new({literal});",
                        s.name
                    );
                }
                Value::Locked { ty, literal } => {
                    let _ = writeln!(
                        r,
                        "    pub static {}: std::sync::Mutex<{ty}> = std::sync::Mutex::new({literal});",
                        s.name
                    );
                }
                Value::LeftOut { why } => {
                    let _ = writeln!(r, "    // {} is left to port: {why}.", s.name);
                }
            }
        }
        r.push_str("}\n");
    }
    r
}

/// `SteadyView` → `steady_view`.
fn snake(name: &str) -> String {
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
            prev_lower = true;
        } else {
            out.push('_');
            prev_lower = false;
        }
    }
    match out.as_str() {
        "type" | "mod" | "use" | "fn" | "impl" | "crate" | "self" | "super" | "match" | "ref" => {
            format!("{out}_")
        }
        _ => out,
    }
}

/// `pickups` → `PICKUPS`, `maxSpeed` → `MAX_SPEED`, `FOV` → `FOV`.
fn screaming(name: &str) -> String {
    if name.chars().all(|c| !c.is_ascii_lowercase()) {
        return name.to_string();
    }
    snake(name).to_ascii_uppercase()
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_java_literals() {
        assert_eq!(literal(Kind::Float, "70.0f").as_deref(), Some("70.0"));
        assert_eq!(literal(Kind::Float, "256").as_deref(), Some("256.0"));
        assert_eq!(literal(Kind::Int, "20 * 60").as_deref(), Some("20 * 60"));
        assert_eq!(literal(Kind::Long, "100L").as_deref(), Some("100"));
        assert_eq!(literal(Kind::Int, "Config.MAX").as_deref(), None);
        assert_eq!(
            literal(Kind::Text, "\"steadyview\"").as_deref(),
            Some("\"steadyview\"")
        );
        assert_eq!(
            literal(
                Kind::Text,
                &format!("\"{bs}n{bs}u00A77(Hidden)\"", bs = '\\')
            )
            .as_deref(),
            Some(r#""\n\u{00A7}7(Hidden)""#)
        );
        assert_eq!(literal(Kind::Text, r#""a\012""#), None);
        assert_eq!(screaming("maxSpeed"), "MAX_SPEED");
        assert_eq!(screaming("FOV"), "FOV");
        assert_eq!(snake("SteadyView"), "steady_view");
    }

    #[test]
    fn chooses_rust_for_each_field() {
        assert_eq!(
            value(Kind::Int, false, None),
            Value::Atomic {
                ty: "AtomicI32",
                literal: "0".into()
            }
        );
        assert_eq!(
            value(Kind::Float, true, Some("70.0f")),
            Value::Const {
                ty: "f32",
                literal: "70.0".into()
            }
        );
        assert!(matches!(
            value(Kind::Int, true, Some("compute()")),
            Value::LeftOut { .. }
        ));
    }
}
