// The work left in a generated mod: each function `port` wrote with a TODO, the
// Java code it names, and where it sits in its file. Also the source surgery the
// translator needs: finding a function's extent, replacing it, and keeping the
// functions already ported when `port --force` writes a file again.

use std::collections::BTreeSet;

/// The marker `port` writes on each function left to port.
pub const TODO: &str = "TODO: port";

/// A function left to port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The file, relative to the mod folder: `mixins/demo/runtime.rs`.
    pub file: String,
    pub function: String,
    pub kind: Kind,
    /// The comment block above (or inside) it, TODO included.
    pub comment: String,
    /// Java classes it names, fully qualified or simple as written.
    pub java: Vec<String>,
    /// Mixin members it names: (Mixin class, member).
    pub members: Vec<(String, String)>,
}

impl Unit {
    /// Its key in the translation state: `mixins/demo/runtime.rs::tick_crank`.
    pub fn key(&self) -> String {
        format!("{}::{}", self.file, self.function)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// In runtime.rs: plain Rust, compiled and tested on its own (`cargo test`).
    Runtime,
    /// A hook helper: compiled inside Horizon, with Horizon's types.
    Helper,
}

/// The units left in a file's source.
pub fn find(file: &str, source: &str) -> Vec<Unit> {
    let kind = if file.ends_with("runtime.rs") {
        Kind::Runtime
    } else {
        Kind::Helper
    };
    let mut out = Vec::new();
    for item in items(source) {
        let comment = &source[item.comment_start..item.fn_start];
        if !comment.contains(TODO) {
            continue;
        }
        out.push(Unit {
            file: file.to_string(),
            function: item.name.clone(),
            kind,
            comment: comment.to_string(),
            java: java_classes(comment),
            members: mixin_members(comment),
        });
    }
    out
}

/// A function item: its comment block, its `fn` line and its end.
#[derive(Debug, Clone)]
struct Item {
    name: String,
    /// Where its comment block starts (its `fn` line when it has none).
    comment_start: usize,
    /// Where the line with `fn` starts.
    fn_start: usize,
    /// Just past its closing brace.
    end: usize,
}

/// The top-level function items of `source`, with the comment block above each.
fn items(source: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut comment_start: Option<usize> = None;
    let mut skip_to = 0;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if start < skip_to {
            continue;
        }
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if indent == 0 && t.starts_with("//") {
            comment_start.get_or_insert(start);
            continue;
        }
        if indent == 0 && t.starts_with("#[") {
            comment_start.get_or_insert(start);
            continue;
        }
        if indent == 0
            && let Some(name) = fn_name(t)
            && let Some(end) = item_end(source, start)
        {
            out.push(Item {
                name,
                comment_start: comment_start.unwrap_or(start),
                fn_start: start,
                end,
            });
            skip_to = end;
        }
        comment_start = None;
    }
    out
}

/// `pub fn tick_crank(` → `tick_crank`.
fn fn_name(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("pub fn ")
        .or_else(|| line.strip_prefix("fn "))
        .or_else(|| line.strip_prefix("pub(crate) fn "))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Just past the closing brace of the item starting at `start`: braces counted
/// outside strings, characters and comments.
fn item_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut depth = 0usize;
    let mut opened = false;
    // Parentheses and brackets before the body: `[i32; 3]` has a `;` of its own.
    let mut nest = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b'"' => {
                // A raw string r"..." / r#"..."#: find its end by its hashes.
                let hashes = source[start..i]
                    .bytes()
                    .rev()
                    .take_while(|b| *b == b'#')
                    .count();
                let raw = i > hashes && bytes[i - hashes - 1] == b'r';
                i += 1;
                if raw {
                    let close = format!("\"{}", "#".repeat(hashes));
                    i = source[i..]
                        .find(&close)
                        .map_or(bytes.len(), |p| i + p + close.len() - 1);
                } else {
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
            }
            b'\'' => {
                // A character literal ('a', '\n', '\''), not a lifetime ('a, '_).
                if bytes.get(i + 1) == Some(&b'\\') {
                    i += 2;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        i += 1;
                    }
                } else if bytes.get(i + 2) == Some(&b'\'') {
                    i += 2;
                } else if let Some(c) = source[i + 1..].chars().next()
                    && source[i + 1 + c.len_utf8()..].starts_with('\'')
                {
                    i += c.len_utf8() + 1;
                }
            }
            b'{' => {
                depth += 1;
                opened = true;
            }
            b'}' => {
                depth = depth.checked_sub(1)?;
                if opened && depth == 0 {
                    let mut end = i + 1;
                    if bytes.get(end) == Some(&b'\n') {
                        end += 1;
                    }
                    return Some(end);
                }
            }
            b'(' | b'[' if !opened => nest += 1,
            b')' | b']' if !opened => nest = nest.saturating_sub(1),
            b';' if !opened && depth == 0 && nest == 0 => return None, // a declaration, no body
            _ => {}
        }
        i += 1;
    }
    None
}

/// The source of function `name` in `source` (its comment block left out).
pub fn function_source(source: &str, name: &str) -> Option<String> {
    items(source)
        .into_iter()
        .find(|i| i.name == name)
        .map(|i| source[i.fn_start..i.end].to_string())
}

/// `source` with function `name` (and its comment block) replaced by `comment`
/// and `function`.
pub fn replace_function(source: &str, name: &str, comment: &str, function: &str) -> Option<String> {
    let item = items(source).into_iter().find(|i| i.name == name)?;
    let mut function = function.trim_end().to_string();
    function.push('\n');
    Some(format!(
        "{}{comment}{function}{}",
        &source[..item.comment_start],
        &source[item.end..]
    ))
}

/// The signature of a function's source: everything before its body.
pub fn signature(function: &str) -> String {
    let head = function.split('{').next().unwrap_or(function);
    head.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `fresh` (a file `port` generated again) with each function that is ported in
/// `existing` (no TODO above it) kept as it is there, when the function still has
/// the same signature. What `port --force` writes, so it never loses a port.
pub fn keep_ported(fresh: &str, existing: &str) -> String {
    let old: Vec<Item> = items(existing);
    let mut out = fresh.to_string();
    for item in items(fresh) {
        let Some(prev) = old.iter().find(|o| o.name == item.name) else {
            continue;
        };
        let prev_comment = &existing[prev.comment_start..prev.fn_start];
        let prev_fn = &existing[prev.fn_start..prev.end];
        let new_fn = &fresh[item.fn_start..item.end];
        if prev_comment.contains(TODO) || signature(prev_fn) != signature(new_fn) {
            continue;
        }
        if let Some(merged) = replace_function(&out, &item.name, prev_comment, prev_fn) {
            out = merged;
        }
    }
    out
}

/// Java classes a TODO comment names: `net.compasshud.CompassHudClient`, and the
/// classes in `(in PlayerMixin, GuiMixin)`.
fn java_classes(comment: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for word in
        comment.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '$'))
    {
        let word = word.trim_matches('.');
        let parts: Vec<&str> = word.split('.').collect();
        if parts.len() >= 2
            && parts[..parts.len() - 1]
                .iter()
                .all(|p| p.starts_with(|c: char| c.is_ascii_lowercase()))
            && parts
                .last()
                .is_some_and(|p| p.starts_with(|c: char| c.is_ascii_uppercase()))
        {
            out.insert(word.to_string());
        }
    }
    for line in comment.lines() {
        if let Some(rest) = line
            .trim()
            .trim_start_matches('/')
            .trim()
            .strip_prefix("(in ")
        {
            for c in rest.trim_end_matches(')').split(',') {
                let c = c.trim();
                if !c.is_empty() {
                    out.insert(c.to_string());
                }
            }
        }
    }
    out.into_iter().collect()
}

/// Mixin members a helper's TODO names: "- `@Inject` steadyview$countPickup on
/// `playerTouch` ..." with "(in ItemEntityMixin)" → (ItemEntityMixin, steadyview$countPickup).
fn mixin_members(comment: &str) -> Vec<(String, String)> {
    let mixins: Vec<String> = comment
        .lines()
        .filter_map(|l| l.trim().trim_start_matches('/').trim().strip_prefix("(in "))
        .flat_map(|r| {
            r.trim_end_matches(')')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = Vec::new();
    for line in comment.lines() {
        let t = line.trim().trim_start_matches('/').trim();
        let Some(rest) = t.strip_prefix("- `@") else {
            continue;
        };
        let Some(member) = rest
            .split_once("` ")
            .map(|(_, r)| r.split(' ').next().unwrap_or(""))
        else {
            continue;
        };
        for mx in &mixins {
            out.push((mx.clone(), member.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNTIME: &str = r#"pub const ID: &str = "demo";

/// A server tick.
///
/// TODO: port the tick of `com.demo.block.CrankBlockEntity`.
pub fn tick_crank(pos: [i32; 3], state: &mut Crank) {
    let _ = (pos, state);
}

/// Done by hand.
pub fn hud_lines(player: Player) -> Vec<String> {
    let s = "}{"; let c = '}'; let l: &'static str = "x";
    vec![format!("{s}{c}{l}")]
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {}
}
"#;

    #[test]
    fn finds_units_and_their_java() {
        let units = find("mixins/demo/runtime.rs", RUNTIME);
        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.function, "tick_crank");
        assert_eq!(u.kind, Kind::Runtime);
        assert_eq!(u.java, ["com.demo.block.CrankBlockEntity"]);
        assert_eq!(u.key(), "mixins/demo/runtime.rs::tick_crank");

        let helper = "// Horizon's `native_item_picked_up`: ...\n// TODO: port into this function:\n// - `@Inject` steadyview$countPickup on `playerTouch` (its doc names the method)\n// (in ItemEntityMixin)\nfn steadyview_item_picked_up(game: &mut crate::game::Game<'_>, count: u32) {\n    let _ = (game, count);\n}\n";
        let units = find("mixins/steadyview/item_picked_up.rs", helper);
        assert_eq!(units[0].kind, Kind::Helper);
        assert_eq!(units[0].java, ["ItemEntityMixin"]);
        assert_eq!(
            units[0].members,
            [(
                "ItemEntityMixin".to_string(),
                "steadyview$countPickup".to_string()
            )]
        );
    }

    #[test]
    fn edits_functions_in_place() {
        // Braces in strings and characters, and lifetimes, don't confuse it.
        let hud = function_source(RUNTIME, "hud_lines").unwrap();
        assert!(hud.ends_with("vec![format!(\"{s}{c}{l}\")]\n}\n"), "{hud}");
        let new = replace_function(
            RUNTIME,
            "tick_crank",
            "/// Ported.\n",
            "pub fn tick_crank(pos: [i32; 3], state: &mut Crank) {\n    state.speed += 1.0;\n}",
        )
        .unwrap();
        assert!(new.contains("/// Ported.\npub fn tick_crank(pos: [i32; 3], state: &mut Crank) {\n    state.speed += 1.0;\n}\n\n/// Done by hand."));
        assert!(find("mixins/demo/runtime.rs", &new).is_empty());
        assert_eq!(
            signature("pub fn f(a: u8) -> u8 {\n 1 }"),
            "pub fn f(a: u8) -> u8"
        );
    }

    #[test]
    fn regeneration_keeps_ported_functions() {
        let ported = replace_function(
            RUNTIME,
            "tick_crank",
            "/// Ported.\n",
            "pub fn tick_crank(pos: [i32; 3], state: &mut Crank) {\n    state.speed += 1.0;\n}",
        )
        .unwrap();
        // port --force writes the stub again: the port stays.
        let merged = keep_ported(RUNTIME, &ported);
        assert!(merged.contains("state.speed += 1.0;"));
        assert!(!merged.contains(TODO));
        // A changed signature means the old body no longer fits: the stub wins.
        let changed = RUNTIME.replace("state: &mut Crank", "state: &mut Crank2");
        let merged = keep_ported(&changed, &ported);
        assert!(merged.contains(TODO));
    }
}
