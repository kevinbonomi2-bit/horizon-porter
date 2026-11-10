// The work left in a generated mod: each function `port` wrote with a TODO, the
// Java code it names, and where it sits in its file. Also the source surgery the
// translator needs: finding a function's extent, replacing it, and keeping the
// functions already ported when `port --force` writes a file again.

use std::collections::BTreeSet;
use std::fmt::Write;

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
    /// Already ported, and to port again (`translate --refresh`): its comment here is
    /// back to the TODO it was.
    pub again: bool,
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
            again: false,
        });
    }
    out
}

/// What `port --force` writes above a ported function it kept whose description
/// changed since: `translate --refresh` ports it again.
pub const OUTDATED: &str = "Outdated:";

/// What a translation writes where the function it was given couldn't reach something.
const GAPS: [&str; 9] = [
    "Missing:",
    "not reachable",
    "isn't reachable",
    "no way to",
    "cannot be ",
    "is not exposed",
    "does not expose",
    "not ported",
    "stands in",
];

/// The ported functions of a file worth porting again: those `port --force` marked
/// outdated, and, with `gaps`, those whose port says it couldn't reach something (what
/// a newer horizon-porter may now give them). Each with its comment back to its TODO.
pub fn find_again(file: &str, source: &str, gaps: bool) -> Vec<Unit> {
    let kind = if file.ends_with("runtime.rs") {
        Kind::Runtime
    } else {
        Kind::Helper
    };
    let mut out = Vec::new();
    for item in items(source) {
        let comment = &source[item.comment_start..item.fn_start];
        if !comment.contains(PORTED) {
            continue;
        }
        let body = &source[item.fn_start..item.end];
        let admits = gaps && GAPS.iter().any(|g| body.contains(g));
        if !comment.contains(OUTDATED) && !admits {
            continue;
        }
        let comment = todo_comment(comment);
        out.push(Unit {
            file: file.to_string(),
            function: item.name.clone(),
            kind,
            java: java_classes(&comment),
            members: mixin_members(&comment),
            comment,
            again: true,
        });
    }
    out
}

/// Whether a ported function's body says it couldn't reach something.
pub fn admits_gap(function: &str) -> bool {
    GAPS.iter().any(|g| function.contains(g))
}

/// A ported function's comment back to the TODO `port` wrote: its "Ported by" note a
/// TODO again, its outdated note gone.
pub fn todo_comment(comment: &str) -> String {
    let mut out = String::new();
    for line in comment.split_inclusive('\n') {
        if line.contains(OUTDATED) {
            continue;
        }
        match line.find(PORTED) {
            Some(i) => {
                let _ = writeln!(out, "{}{}", &line[..i], todo_of(&line[i..]));
            }
            None => out.push_str(line),
        }
    }
    out
}

/// "Ported by horizon-porter translate (model) from a.B. Review it." as the TODO it
/// was: "TODO: port from a.B.".
fn todo_of(note: &str) -> String {
    // "(claude-haiku-5-5) from these; review it:", "(model) from a.B. Review it.",
    // "(model): the tick of `a.B`. Review it."
    let rest = note[PORTED.len()..].trim();
    let rest = rest
        .strip_prefix('(')
        .and_then(|r| r.split_once(')'))
        .map_or(rest, |(_, r)| r)
        .trim_start_matches(':')
        .trim();
    let rest = rest.trim_end_matches("Review it.").trim();
    if rest == "from these; review it:" {
        format!("{TODO} into this function:")
    } else {
        format!("{TODO} {rest}")
    }
}

/// A function as `port` leaves it to port: its head, and a body that answers nothing.
pub fn stub_of(function: &str) -> String {
    let head = function.split('{').next().unwrap_or(function).trim_end();
    let args = parameter_names(head);
    let used = match args.len() {
        0 => String::new(),
        1 => format!("    let _ = {};\n", args[0]),
        _ => format!("    let _ = ({});\n", args.join(", ")),
    };
    format!("{head} {{\n{used}    todo!()\n}}\n")
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
    items_where(source, false)
}

/// The top-level items of `source` with a body: functions, and with `structs`
/// structs and enums too (`pub struct GuideScreen { ... }`).
fn items_where(source: &str, structs: bool) -> Vec<Item> {
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
        let name = fn_name(t).or_else(|| structs.then(|| type_name(t)).flatten());
        if indent == 0
            && let Some(name) = name
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

/// `pub struct GuideScreen {` → `GuideScreen` (structs and enums with a body).
fn type_name(line: &str) -> Option<String> {
    let rest = ["pub struct ", "struct ", "pub enum ", "enum "]
        .iter()
        .find_map(|p| line.strip_prefix(p))?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty() && rest[name.len()..].trim_start().starts_with('{')).then_some(name)
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

/// The comment block above function `name`.
pub fn function_comment(source: &str, name: &str) -> Option<String> {
    let item = items(source).into_iter().find(|i| i.name == name)?;
    Some(source[item.comment_start..item.fn_start].to_string())
}

/// The top-level functions of `source` that `translate` ported (a "Ported by
/// horizon-porter translate" note above each): their names.
pub fn ported_functions(source: &str) -> Vec<String> {
    items(source)
        .into_iter()
        .filter(|i| source[i.comment_start..i.fn_start].contains(PORTED))
        .map(|i| i.name)
        .collect()
}

/// The names of `source`'s top-level functions.
pub fn function_names(source: &str) -> Vec<String> {
    items(source).into_iter().map(|i| i.name).collect()
}

/// What `translate` writes above a function it ported.
pub const PORTED: &str = "Ported by horizon-porter translate";

/// `source` with the ported function `name` put back to what `port` left: its note
/// back to the TODO it was (so `translate` ports it again), `why` added below it,
/// and a body that answers nothing. None when it isn't there or wasn't ported.
pub fn unport(source: &str, name: &str, why: &str) -> Option<String> {
    let item = items(source).into_iter().find(|i| i.name == name)?;
    let comment = &source[item.comment_start..item.fn_start];
    if !comment.contains(PORTED) {
        return None;
    }
    let mut out = String::new();
    for line in comment.split_inclusive('\n') {
        let Some(i) = line.find(PORTED) else {
            out.push_str(line);
            continue;
        };
        let prefix = &line[..i];
        let todo = todo_of(&line[i..]);
        let why = why.lines().next().unwrap_or(why);
        let _ = writeln!(out, "{prefix}{todo}");
        let _ = writeln!(
            out,
            "{prefix}Its port was put back: the game's build refused it ({why})."
        );
    }
    let function = &source[item.fn_start..item.end];
    let head = function.split('{').next()?.trim_end();
    // No attributes (horizon-mixins refuses a helper with one): its arguments named
    // to leave none unused, as `port`'s own stubs do.
    let args = parameter_names(head);
    let used = match args.len() {
        0 => String::new(),
        1 => format!("    let _ = {};\n", args[0]),
        _ => format!("    let _ = ({});\n", args.join(", ")),
    };
    let stub = format!("{head} {{\n{used}    Default::default()\n}}\n");
    Some(format!(
        "{}{out}{stub}{}",
        &source[..item.comment_start],
        &source[item.end..]
    ))
}

/// The parameter names of a function's head (`fn f(&mut self, mut a: [i32; 3],
/// b: Option<(u8, u8)>)` → `self`, `a`, `b`).
fn parameter_names(head: &str) -> Vec<String> {
    let Some(open) = head.find('(') else {
        return Vec::new();
    };
    let mut depth = 0i32;
    let mut parts = vec![String::new()];
    for c in head[open + 1..].chars() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' if depth == 0 => break,
            ')' | ']' | '>' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(p) = parts.last_mut() {
            p.push(c);
        }
    }
    parts
        .iter()
        .filter_map(|p| {
            let p = p.trim();
            if p.ends_with("self") && !p.contains(':') {
                return Some("self".to_string());
            }
            let name = p.split(':').next()?.trim();
            let name = name.strip_prefix("mut ").unwrap_or(name).trim();
            (!name.is_empty() && name != "_").then(|| name.to_string())
        })
        .collect()
}

/// The signature of a function's source: everything before its body.
pub fn signature(function: &str) -> String {
    let head = function.split('{').next().unwrap_or(function);
    head.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `fresh` (a file `port` generated again) with each function or struct `port`
/// leaves to port (a TODO above it in `fresh`) kept as it is in `existing`, once
/// ported there (no TODO above it) and with the same signature. Everything `port`
/// generates whole is fresh. What `port --force` writes, so it never loses a port
/// and never keeps stale generated code.
pub fn keep_ported(fresh: &str, existing: &str) -> String {
    let old: Vec<Item> = items_where(existing, true);
    let mut out = fresh.to_string();
    for item in items_where(fresh, true) {
        let Some(prev) = old.iter().find(|o| o.name == item.name) else {
            continue;
        };
        let prev_comment = &existing[prev.comment_start..prev.fn_start];
        let prev_fn = &existing[prev.fn_start..prev.end];
        let new_fn = &fresh[item.fn_start..item.end];
        if prev_comment.contains(TODO) || signature(prev_fn) != signature(new_fn) {
            continue;
        }
        // Only what `port` left to port (a TODO in the fresh file) is yours to keep:
        // code it generates whole (a hook helper, a dispatch) always follows the
        // porter, so it matches the runtime it calls.
        let fresh_comment = &fresh[item.comment_start..item.fn_start];
        if !fresh_comment.contains(TODO) {
            continue;
        }
        // What `port` says of the function now, against what it said when it was ported:
        // a kept port that was given less (no item slots, no tag query) is marked, on
        // the new description, for `translate --refresh`.
        let described = |c: &str| -> Vec<String> {
            c.lines()
                .filter(|l| !l.contains(TODO) && !l.contains(PORTED) && !l.contains(OUTDATED))
                .map(|l| l.trim().to_string())
                .collect()
        };
        let was_ported = prev_comment.contains(PORTED);
        let stale = prev_comment.contains(OUTDATED)
            || (was_ported && described(prev_comment) != described(fresh_comment));
        let comment = if stale && was_ported {
            let note = prev_comment
                .lines()
                .find(|l| l.contains(PORTED))
                .unwrap_or("");
            let prefix = &note[..note.find(PORTED).unwrap_or(0)];
            let mut c = String::new();
            for line in fresh_comment.split_inclusive('\n') {
                if line.contains(TODO) {
                    let _ = writeln!(c, "{note}");
                    let _ = writeln!(
                        c,
                        "{prefix}{OUTDATED} horizon-porter describes this function differently now (it may give it more to work with); `horizon-porter translate --refresh` ports it again."
                    );
                } else {
                    c.push_str(line);
                }
            }
            c
        } else {
            prev_comment.to_string()
        };
        if let Some(merged) = replace_item(&out, &item.name, &comment, prev_fn) {
            out = merged;
        }
    }
    out
}

/// `replace_function` for any item with a body, structs included.
fn replace_item(source: &str, name: &str, comment: &str, item: &str) -> Option<String> {
    let found = items_where(source, true)
        .into_iter()
        .find(|i| i.name == name)?;
    let mut item = item.trim_end().to_string();
    item.push('\n');
    Some(format!(
        "{}{comment}{item}{}",
        &source[..found.comment_start],
        &source[found.end..]
    ))
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
    fn regeneration_keeps_filled_in_structs() {
        let fresh = "/// TODO: port the fields of `a.GuideScreen`.\n#[derive(Default)]\npub struct GuideScreen {}\n\n/// Kept as generated.\npub struct Fixed {\n    pub a: i32,\n}\n";
        let existing = "/// Ported by hand.\n#[derive(Default)]\npub struct GuideScreen {\n    pub page: i32,\n}\n\n/// Edited.\npub struct Fixed {\n    pub b: i32,\n}\n";
        let kept = keep_ported(fresh, existing);
        assert!(
            kept.contains("pub struct GuideScreen {\n    pub page: i32,\n}"),
            "{kept}"
        );
        // A struct `port` doesn't leave to fill in follows the Java mod.
        assert!(
            kept.contains("pub struct Fixed {\n    pub a: i32,\n}"),
            "{kept}"
        );
    }

    #[test]
    fn a_kept_port_described_differently_is_marked_and_found_again() {
        let existing = "/// A tick. `machine` has nothing (it has no item slots).
///
/// Ported by horizon-porter translate (m): the tick of `a.B`. Review it.
pub fn tick_b(pos: [i32; 3]) {
    // Missing: its slots.
    let _ = pos;
}
";
        let fresh = "/// A tick. `machine` has its 27 item slots.
///
/// TODO: port the tick of `a.B`.
pub fn tick_b(pos: [i32; 3]) {
    let _ = pos;
}
";
        let kept = keep_ported(fresh, existing);
        assert!(kept.contains("its 27 item slots"), "{kept}");
        assert!(kept.contains("// Missing: its slots."), "{kept}");
        assert!(kept.contains(OUTDATED), "{kept}");
        // Written again, it stays marked and isn't marked twice.
        let again = keep_ported(fresh, &kept);
        assert_eq!(again.matches(OUTDATED).count(), 1, "{again}");
        let units = find_again("mixins/m/runtime.rs", &again, false);
        assert_eq!(units.len(), 1);
        assert!(
            units[0].comment.contains("TODO: port the tick of `a.B`."),
            "{}",
            units[0].comment
        );
        assert!(!units[0].comment.contains(OUTDATED));
        // The same description: kept as it was, and found only by what it says is missing.
        let same = keep_ported(
            &existing.replace("Ported by horizon-porter translate (m):", "TODO: port"),
            existing,
        );
        assert!(!same.contains(OUTDATED), "{same}");
        assert!(find_again("mixins/m/runtime.rs", &same, false).is_empty());
        assert_eq!(find_again("mixins/m/runtime.rs", &same, true).len(), 1);
        assert!(
            stub_of(
                "pub fn tick_b(pos: [i32; 3]) {
    1;
}
"
            )
            .contains("todo!()")
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

    #[test]
    fn regeneration_refreshes_generated_code() {
        // A helper `port` writes whole (no TODO) follows the porter: a newer
        // runtime's new draw step must reach it, as the Grind demo's draw helper
        // once missed `DrawOp::Model`.
        let old = "// Draws it.\nfn demo_submit(op: Op) {\n    match op {\n        Op::Push => {}\n    }\n}\n";
        let fresh = "// Draws it.\nfn demo_submit(op: Op) {\n    match op {\n        Op::Push => {}\n        Op::Model => {}\n    }\n}\n";
        assert_eq!(keep_ported(fresh, old), fresh);
    }
}
