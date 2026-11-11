//! A mod's own questions to a block: methods of an interface of the mod's that its block
//! classes answer, and that its code asks of a block it meets (Create's
//! `IRotate.hasShaftTowards`, asked of every neighbour a shaft looks at). Ported once
//! each, as a function over the block and its state properties, so the ticks and clicks
//! that ask don't each carry their own copy of every block's answer.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use porter_model::{ClassInfo, ClassKind, ModScan};

/// The most questions a mod gets, the most asked first.
const LIMIT: usize = 24;

/// A body longer than this isn't quoted: the class is read instead.
const BODY_LIMIT: usize = 600;

/// One method of one of the mod's interfaces, with what each block class answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// The interface, fully qualified.
    pub interface: String,
    /// The Java method: `hasShaftTowards`.
    pub method: String,
    /// Its declaration's parameters as written: `LevelReader world, BlockPos pos,
    /// BlockState state, Direction face`.
    pub java_params: String,
    /// The Rust function: `i_rotate_has_shaft_towards`.
    pub rust: String,
    /// The Rust parameters after `block` and `state`: (name, type).
    pub params: Vec<(String, &'static str)>,
    /// The Rust return type.
    pub returns: &'static str,
    /// The interface's own answer, when it gives one (`default`).
    pub default: Option<String>,
    /// What each block class answers.
    pub answers: Vec<Answer>,
    /// How many of the mod's classes ask it.
    pub asked: usize,
}

/// One block class's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The class that decides it, fully qualified.
    pub class: String,
    /// The blocks registered with it, or with a class that inherits the answer.
    pub blocks: Vec<String>,
    /// The method's body on one line; none when it is too long to quote.
    pub body: Option<String>,
}

/// The mod's questions to a block, the most asked first. `ns` is the namespace of its ids.
pub fn find(scan: &ModScan, ns: &str) -> Vec<Question> {
    let class = |name: &str| scan.classes.iter().find(|c| c.name == name);
    let source = |c: &ClassInfo| fs::read_to_string(Path::new(&scan.input).join(&c.path)).ok();
    // Each registered block's class, by simple name as registrations give it.
    let mut blocks: Vec<(String, &ClassInfo)> = Vec::new();
    for r in scan.registrations.iter().filter(|r| r.registry == "block") {
        let (Some(id), Some(creates)) = (r.id.as_deref(), r.creates.as_deref()) else {
            continue;
        };
        let mut found = scan.classes.iter().filter(|c| simple(&c.name) == creates);
        if let (Some(c), None) = (found.next(), found.next()) {
            let id = if id.contains(':') {
                id.to_string()
            } else {
                format!("{ns}:{id}")
            };
            blocks.push((id, c));
        }
    }
    if blocks.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Question> = Vec::new();
    for interface in scan
        .classes
        .iter()
        .filter(|c| c.kind == ClassKind::Interface && !c.is_mixin && !c.method_names.is_empty())
    {
        let mut text: Option<String> = None;
        for method in &interface.method_names {
            if out.iter().any(|q| q.method == *method) {
                continue;
            }
            // The class that decides it for each block: its own, or the nearest above.
            let mut by_class: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
            for (id, own) in &blocks {
                let mut next = Some(*own);
                for _ in 0..12 {
                    let Some(c) = next else {
                        break;
                    };
                    if c.kind != ClassKind::Interface && c.method_names.iter().any(|n| n == method)
                    {
                        match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                            Some((_, ids)) => ids.push(id.clone()),
                            None => by_class.push((c, vec![id.clone()])),
                        }
                        break;
                    }
                    next = c.superclass.as_deref().and_then(class);
                }
            }
            if by_class.is_empty() {
                continue;
            }
            let text = text.get_or_insert_with(|| source(interface).unwrap_or_default());
            let Some(decl) = declaration(text, method) else {
                continue;
            };
            let Some((params, returns)) = signature(&decl) else {
                continue;
            };
            let answers = by_class
                .into_iter()
                .map(|(c, ids)| Answer {
                    class: c.name.clone(),
                    blocks: ids,
                    body: source(c)
                        .and_then(|s| method_body(&s, method))
                        .filter(|b| b.len() <= BODY_LIMIT),
                })
                .collect();
            out.push(Question {
                interface: interface.name.clone(),
                method: method.clone(),
                java_params: decl.params_text.clone(),
                rust: format!("{}_{}", snake(&simple(&interface.name)), snake(method)),
                params,
                returns,
                default: decl.body.filter(|b| b.len() <= BODY_LIMIT),
                answers,
                asked: 0,
            });
        }
    }
    if out.is_empty() {
        return out;
    }
    // Asked by the mod's code of something other than itself: `x.method(`.
    for c in scan.classes.iter().filter(|c| !c.is_mixin) {
        let Some(text) = source(c) else {
            continue;
        };
        for q in out.iter_mut().filter(|q| q.interface != c.name) {
            if text.contains(&format!(".{}(", q.method)) {
                q.asked += 1;
            }
        }
    }
    out.retain(|q| q.asked > 0);
    out.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.rust.cmp(&b.rust)));
    out.truncate(LIMIT);
    out
}

/// runtime.rs's part: a function to port for each question.
pub fn runtime(questions: &[Question]) -> String {
    let mut r = String::new();
    for q in questions {
        let interface = simple(&q.interface);
        let _ = write!(
            r,
            "\n/// `{interface}.{method}({java})`: a question the mod's code asks a block it meets,\n/// answered here once for every block of the mod. `block` is the block asked\n/// (`{example}`) and `state` its state properties: `state.getValue(FACING)` is the value\n/// under `facing` (a `Direction` as `north`: `Side::from_name` makes it a `Side`),\n/// `getValue(AXIS)` the one under `axis` (`x`, `y` or `z`, as an `Axis` is here). The level\n/// isn't given.\n///\n/// What each block class answers, with its blocks and those of classes that inherit it:\n",
            method = q.method,
            java = q.java_params,
            example = q
                .answers
                .first()
                .and_then(|a| a.blocks.first())
                .map_or("", String::as_str),
        );
        let mut long: Vec<&str> = Vec::new();
        for a in &q.answers {
            let mut ids = a
                .blocks
                .iter()
                .take(12)
                .map(|b| format!("`{b}`"))
                .collect::<Vec<_>>()
                .join(", ");
            if a.blocks.len() > 12 {
                let _ = write!(ids, " and {} more", a.blocks.len() - 12);
            }
            match &a.body {
                Some(body) => {
                    let _ = writeln!(
                        r,
                        "/// - `{}` ({ids}): `{}`",
                        simple(&a.class),
                        one_line(body)
                    );
                }
                None => {
                    let _ = writeln!(
                        r,
                        "/// - `{}` ({ids}): too long to quote, read its `{}`",
                        simple(&a.class),
                        q.method
                    );
                    long.push(&a.class);
                }
            }
        }
        let _ = writeln!(
            r,
            "/// Any other block answers as the interface does: {}.",
            match &q.default {
                Some(body) => format!("`{}`", one_line(body)),
                None => "it gives none, so the plainest answer".to_string(),
            }
        );
        // The classes whose answer wasn't quoted are read with the interface.
        let mut from = format!("`{}`", q.interface);
        for class in long.iter().take(4) {
            let _ = write!(from, ", `{class}`");
        }
        let params: String = q
            .params
            .iter()
            .map(|(n, t)| format!(", {n}: {t}"))
            .collect();
        let names: String = q.params.iter().map(|(n, _)| format!(", {n}")).collect();
        let _ = write!(
            r,
            "///\n/// TODO: port from {from}.\npub fn {rust}(block: &str, state: &[(String, String)]{params}) -> {returns} {{\n    let _ = (block, state{names});\n    {value}\n}}\n",
            rust = q.rust,
            returns = q.returns,
            value = default_value(q.returns),
        );
    }
    r
}

/// The line that tells a function's translation its Java's `x.method(..)` is ported:
/// for each question of `module` (a runtime.rs) that `java` asks. Nothing when it asks
/// none, or when `function` is one of them itself.
pub fn ported(module: &str, java: &str, function: &str) -> String {
    let mut out = String::new();
    for (method, rust, signature) in in_module(module) {
        if rust == function || !java.contains(&format!(".{method}(")) {
            continue;
        }
        let _ = writeln!(
            out,
            "- `x.{method}(..)` asked of a block is `{rust}`, already ported for every block of the mod: `{signature}`. Pass the block asked and its state properties (a neighbour's `n.block` and `&n.properties`, this block's `machine.block` and `&machine.properties`)."
        );
    }
    out
}

/// Whether `function` is one of `module`'s questions.
pub fn is_question(module: &str, function: &str) -> bool {
    in_module(module)
        .iter()
        .any(|(_, rust, _)| rust == function)
}

/// The Java methods `module` has a question function for.
pub fn methods(module: &str) -> Vec<String> {
    in_module(module).into_iter().map(|(m, _, _)| m).collect()
}

/// The questions a runtime.rs carries: (Java method, Rust function, its signature line).
fn in_module(module: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut method: Option<String> = None;
    for line in module.lines() {
        if let Some(rest) = line.strip_prefix("/// `") {
            // "/// `IRotate.hasShaftTowards(..)`: a question the mod's code asks ..."
            if line.contains("a question the mod's code asks a block it meets") {
                method = rest
                    .split('(')
                    .next()
                    .and_then(|m| m.rsplit('.').next())
                    .map(str::to_string);
            }
        } else if let Some(rest) = line.strip_prefix("pub fn ") {
            if let Some(m) = method.take() {
                let name = rest.split('(').next().unwrap_or_default().to_string();
                let signature = line.trim_end_matches('{').trim().to_string();
                out.push((m, name, signature));
            }
        } else if !line.starts_with("///") {
            method = None;
        }
    }
    out
}

/// A method as an interface declares it.
struct Declaration {
    returns: String,
    /// (type, name), as written.
    params: Vec<(String, String)>,
    params_text: String,
    is_static: bool,
    /// Its `default` body, on one line.
    body: Option<String>,
}

/// How `source` declares method `name`: the first declaration of that name.
fn declaration(source: &str, name: &str) -> Option<Declaration> {
    let mut from = 0;
    while let Some(at) = source[from..].find(&format!(" {name}(")).map(|i| from + i) {
        from = at + 1;
        // What the statement says before the name: modifiers and the return type.
        let start = source[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
        let mut head: Vec<&str> = source[start..at]
            .split_whitespace()
            .filter(|w| !w.starts_with('@') && !w.starts_with("//"))
            .collect();
        let is_static = head.contains(&"static");
        head.retain(|w| {
            !matches!(
                *w,
                "public" | "default" | "static" | "abstract" | "final" | "private" | "protected"
            )
        });
        // A declaration has one word left, its return type; a call has none, or more.
        let [returns] = head[..] else {
            continue;
        };
        if matches!(returns, "return" | "new" | "throw" | "else") {
            continue;
        }
        let open = at + 1 + name.len();
        let inside = balanced(&source[open..])?;
        let after = source[open + inside.len() + 2..].trim_start();
        let body = after
            .starts_with('{')
            .then(|| method_body(&source[at..], name))
            .flatten();
        if !after.starts_with('{') && !after.starts_with(';') && !after.starts_with("throws") {
            continue;
        }
        let params = split_top(inside)
            .into_iter()
            .filter_map(|p| {
                let words: Vec<&str> = p
                    .split_whitespace()
                    .filter(|w| !w.starts_with('@') && *w != "final")
                    .collect();
                match words[..] {
                    [ty, name] => Some((ty.to_string(), name.to_string())),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        if params.len() != split_top(inside).len() {
            continue;
        }
        // A static helper of the same name (`isSmallCog(BlockState)`) isn't the
        // question: the one a block answers comes after it.
        if is_static && source[from..].contains(&format!(" {name}(")) {
            continue;
        }
        return Some(Declaration {
            returns: returns.to_string(),
            params_text: params
                .iter()
                .map(|(t, n)| format!("{t} {n}"))
                .collect::<Vec<_>>()
                .join(", "),
            params,
            is_static,
            body,
        });
    }
    None
}

/// The Rust parameters (after `block` and `state`) and return type for a declaration, when
/// every type of it has one here: a question about a block, not a command.
fn signature(decl: &Declaration) -> Option<(Vec<(String, &'static str)>, &'static str)> {
    if decl.is_static {
        return None;
    }
    let returns = match plain(&decl.returns) {
        "boolean" => "bool",
        "int" => "i32",
        "long" => "i64",
        "float" => "f32",
        "double" => "f64",
        "Axis" | "Direction.Axis" => "&'static str",
        "Direction" => "Side",
        _ => return None,
    };
    let mut params = Vec::new();
    let mut state = false;
    for (ty, name) in &decl.params {
        let rust = match plain(ty) {
            // The block asked: `block` and `state`.
            "BlockState" if !state => {
                state = true;
                continue;
            }
            // The level isn't given.
            "Level" | "LevelReader" | "LevelAccessor" | "BlockGetter" | "ServerLevel"
            | "BlockAndTintGetter" | "CollisionGetter" => continue,
            "BlockPos" => "[i32; 3]",
            "Direction" => "Side",
            "Axis" | "Direction.Axis" => "&str",
            "boolean" => "bool",
            "int" => "i32",
            "long" => "i64",
            "float" => "f32",
            "double" => "f64",
            "String" => "&str",
            _ => return None,
        };
        let name = snake(name);
        let name = if matches!(name.as_str(), "block" | "state" | "type" | "match" | "ref") {
            format!("{name}_")
        } else {
            name
        };
        params.push((name, rust));
    }
    Some((params, returns))
}

/// A type without its package or type arguments: `Direction.Axis`, `BlockState`.
fn plain(ty: &str) -> &str {
    let ty = ty.split('<').next().unwrap_or(ty);
    match ty.rsplit_once('.') {
        Some((before, last)) if before.ends_with("Direction") && last == "Axis" => "Direction.Axis",
        Some((_, last)) => last,
        None => ty,
    }
}

/// What a stub answers until it is ported.
fn default_value(returns: &str) -> &'static str {
    match returns {
        "bool" => "false",
        "i32" | "i64" => "0",
        "f32" | "f64" => "0.0",
        "Side" => "Side::Up",
        _ => "\"y\"",
    }
}

/// The body of method `name` as `source` declares it, on one line: `return face ==
/// state.getValue(FACING);`. None where it isn't found.
pub fn method_body(source: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = source[from..].find(&format!(" {name}(")).map(|i| from + i) {
        from = at + 1;
        let rest = &source[at..];
        let open = rest.find('{')?;
        // A declaration: its body opens before any statement ends.
        if rest[..open].contains(';') {
            continue;
        }
        let mut depth = 0usize;
        for (i, c) in rest[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let body = rest[open + 1..open + i]
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        return Some(body);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
    None
}

/// What is inside the parentheses `text` opens with.
fn balanced(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&text[1..i]);
                }
            }
            _ if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// `text` split at the commas outside any brackets; nothing for an empty list.
fn split_top(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

fn simple(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `hasShaftTowards` → `has_shaft_towards`, `IRotate` → `i_rotate`.
fn snake(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let after_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let before_lower = i > 0 && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            if after_lower || before_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(*c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERFACE: &str = "public interface IRotate extends IWrenchable {\n\n\tpublic boolean hasShaftTowards(LevelReader world, BlockPos pos, BlockState state, Direction face);\n\n\tpublic Axis getRotationAxis(BlockState state);\n\n\tdefault SpeedLevel getMinimumRequiredSpeedLevel() {\n\t\treturn SpeedLevel.NONE;\n\t}\n\n\tdefault boolean hideStressImpact() {\n\t\treturn false;\n\t}\n\n\tstatic boolean isIt(BlockState state) { return state.hasShaftTowards(null, null, state, null); }\n}";

    #[test]
    fn reads_a_declaration_and_gives_it_a_rust_shape() {
        let d = declaration(INTERFACE, "hasShaftTowards").expect("declared");
        assert_eq!(d.returns, "boolean");
        assert_eq!(
            d.params_text,
            "LevelReader world, BlockPos pos, BlockState state, Direction face"
        );
        let (params, returns) = signature(&d).expect("a question");
        assert_eq!(returns, "bool");
        assert_eq!(
            params,
            [
                ("pos".to_string(), "[i32; 3]"),
                ("face".to_string(), "Side")
            ]
        );
        // An axis comes back as its letter.
        let d = declaration(INTERFACE, "getRotationAxis").expect("declared");
        assert_eq!(signature(&d), Some((Vec::new(), "&'static str")));
        // A default answer is kept; a type with no shape here is no question.
        let d = declaration(INTERFACE, "hideStressImpact").expect("declared");
        assert_eq!(d.body.as_deref(), Some("return false;"));
        let d = declaration(INTERFACE, "getMinimumRequiredSpeedLevel").expect("declared");
        assert_eq!(signature(&d), None);
        // Nor is a static helper.
        let d = declaration(INTERFACE, "isIt").expect("declared");
        assert_eq!(signature(&d), None);
        // A static helper named like the question is passed over for the question.
        let cog = "interface ICogWheel {
	static boolean isLargeCog(BlockState state) {
		return state.getBlock() instanceof ICogWheel c && c.isLargeCog();
	}

	default boolean isLargeCog() {
		return false;
	}
}";
        let d = declaration(cog, "isLargeCog").expect("declared");
        assert!(!d.is_static);
        assert_eq!(signature(&d), Some((Vec::new(), "bool")));
        assert_eq!(d.body.as_deref(), Some("return false;"));
    }

    #[test]
    fn writes_a_function_to_port_and_finds_it_again() {
        let q = Question {
            interface: "com.spin.IRotate".into(),
            method: "hasShaftTowards".into(),
            java_params: "LevelReader world, BlockPos pos, BlockState state, Direction face".into(),
            rust: "i_rotate_has_shaft_towards".into(),
            params: vec![("pos".into(), "[i32; 3]"), ("face".into(), "Side")],
            returns: "bool",
            default: None,
            answers: vec![Answer {
                class: "com.spin.MotorBlock".into(),
                blocks: vec!["spin:motor".into()],
                body: Some("return face == state.getValue(FACING);".into()),
            }],
            asked: 3,
        };
        let text = runtime(&[q]);
        assert!(
            text.contains(
                "- `MotorBlock` (`spin:motor`): `return face == state.getValue(FACING);`"
            ),
            "{text}"
        );
        assert!(
            text.contains("TODO: port from `com.spin.IRotate`."),
            "{text}"
        );
        assert!(
            text.contains("pub fn i_rotate_has_shaft_towards(block: &str, state: &[(String, String)], pos: [i32; 3], face: Side) -> bool {"),
            "{text}"
        );
        assert_eq!(methods(&text), ["hasShaftTowards"]);
        assert!(is_question(&text, "i_rotate_has_shaft_towards"));
        // A function whose Java asks it is told; the question itself is not.
        let told = ported(
            &text,
            "if (def.hasShaftTowards(world, pos, state, face))",
            "tick_shaft",
        );
        assert!(told.contains("`i_rotate_has_shaft_towards`"), "{told}");
        assert!(ported(&text, "x.hasShaftTowards(", "i_rotate_has_shaft_towards").is_empty());
        assert!(ported(&text, "nothing asked", "tick_shaft").is_empty());
    }

    #[test]
    fn reads_a_methods_body_on_one_line() {
        let source = "class Motor {
    @Override
    public boolean hasShaftTowards(LevelReader world, BlockPos pos, BlockState state, Direction face) {
        return face == state.getValue(FACING);
    }
    void other() { hasShaftTowards(null, null, null, null); }
}";
        assert_eq!(
            method_body(source, "hasShaftTowards").as_deref(),
            Some("return face == state.getValue(FACING);")
        );
        assert_eq!(method_body(source, "getRotationAxis"), None);
    }

    #[test]
    fn names_things_in_snake_case() {
        assert_eq!(snake("IRotate"), "i_rotate");
        assert_eq!(snake("hasShaftTowards"), "has_shaft_towards");
        assert_eq!(snake("ICogWheel"), "i_cog_wheel");
    }
}
