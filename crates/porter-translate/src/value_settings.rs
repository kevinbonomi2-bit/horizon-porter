//! Create's value settings (`ValueSettingsBehaviour`, `ScrollValueBehaviour` and the
//! classes that extend it): a number or an option set in a box on a block. A creative
//! motor's speed, a speed controller's target, an ejector's stack size, a bearing's
//! movement mode, a brass diode's delay. Java shows the value in a box on the block
//! when it is looked at (`ScrollValueRenderer`), with a tip; holding the use key on
//! the box opens a board (`ValueSettingsScreen`) to drag the value along, and letting
//! go sends the choice to the server (`ValueSettingsPacket`), which sets the value.
//!
//! What all of them share is generated here, as Create's own code does it: the held
//! use key, the board and its drawing, the message, setting the value on the server
//! (clamped to its range, its callback run, sent to the clients), and the box with its
//! value and tip when looked at. What each has of its own is ported from its Java:
//! - per value a block entity holds (the motor's `generatedSpeed`): what its
//!   `addBehaviours` sets up (label, range, formatter, when it shows), where its box
//!   is on the block (its `ValueBoxTransform`), and its callback;
//! - per behaviour class: its board (`createBoard`), where its value is on the board
//!   (`getValueSettings`) and the value a choice sets (`setValueSettings`).
//!
//! Create's add-ons build on the same classes, so a port of one gets the same.

use std::fmt::Write;

use porter_model::{ClassInfo, ModScan};

use crate::block_entity::BlockEntity;

/// A value one of the mod's block entities holds in a box on its block.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueBox {
    /// Its block entity: `create:motor`, its state struct `Motor`, `motor` in its
    /// functions' names.
    pub entity: String,
    pub rust: String,
    pub snake: String,
    /// The Java field that holds it: `generatedSpeed`.
    pub field: String,
    /// The state's field its value is kept in: `generated_speed_value`.
    pub value: String,
    /// The class whose `addBehaviours` makes it (the block entity's own, or one of
    /// the mod's above it), fully qualified.
    pub owner: String,
    /// The class it is made as: `new KineticScrollValueBehaviour(..)`.
    pub class: String,
    /// The mod's other classes its making names: its `ValueBoxTransform`
    /// (`new MotorValueBox()`), an option enum (`RotationMode.class`).
    pub named: Vec<String>,
    /// Whether its block entity gives it a callback (`withCallback(..)`).
    pub callback: bool,
    /// The class of its own or above it that defines each of `createBoard`,
    /// `getValueSettings` and `setValueSettings`: None where none of the mod's does
    /// (Create's own defaults then).
    pub methods: [Option<String>; 3],
    /// Where `class` is declared, when it is nested in another class (`Outer.Inner`,
    /// which the scan doesn't list): the outer class, whose source has it.
    pub nested_in: Option<String>,
}

impl ValueBox {
    /// The name its functions start with: `motor_generated_speed`.
    pub fn function(&self) -> String {
        format!("{}_{}", self.snake, snake(&self.field))
    }
}

/// The three methods of a behaviour class a board needs, and what each is ported as.
const METHODS: [(&str, &str); 3] = [
    ("createBoard", "board"),
    ("getValueSettings", "settings"),
    ("setValueSettings", "set"),
];

/// The hooks the value settings use.
pub const HOOKS: &[&str] = &[
    "interaction_key",
    "key",
    "frame",
    "hud",
    "screen_render_post",
    "screen_mouse_button_released_pre",
    "screen_mouse_scrolled_pre",
    "net_from_client",
];

/// The values in boxes the mod's block entities hold.
pub fn find(scan: &ModScan, block_entities: &[BlockEntity]) -> Vec<ValueBox> {
    let mut out = Vec::new();
    for be in block_entities {
        let Some(own) = &be.class else {
            continue;
        };
        // Where a field is made: its own class first, then the mod's above it.
        let owners: Vec<&ClassInfo> = std::iter::once(own)
            .chain(be.bases.iter())
            .filter_map(|name| scan.classes.iter().find(|c| &c.name == name))
            .collect();
        for (field, held) in &be.held {
            if !is_value_setting(scan, held) {
                continue;
            }
            let value = format!("{}_value", crate::block_entity::rust_ident(field));
            if !be.fields.iter().any(|f| f.name == value) {
                continue;
            }
            // `generatedSpeed = new KineticScrollValueBehaviour(..)`, in the class that
            // makes it; else the class the field is declared as.
            // A class below the maker whose own `addBehaviours` doesn't call
            // `super.addBehaviours` never adds it (an elevator pulley has no movement
            // mode, though the linear actuator above it makes one).
            let mut made = None;
            let mut skipped = false;
            for owner in &owners {
                let Ok(text) =
                    std::fs::read_to_string(std::path::Path::new(&scan.input).join(&owner.path))
                else {
                    continue;
                };
                if let Some(m) = making(&text, field) {
                    made = Some((owner.name.clone(), text, m));
                    break;
                }
                if let Some(body) = body_after(&text, "void addBehaviours(")
                    && !body.contains("super.addBehaviours(")
                {
                    skipped = true;
                    break;
                }
            }
            if skipped {
                continue;
            }
            let (owner, text, making) = match made {
                Some(m) => m,
                None => (own.clone(), String::new(), String::new()),
            };
            let made_as = making
                .split_once("new ")
                .map(|(_, rest)| {
                    rest.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                        .next()
                        .unwrap_or_default()
                        .to_string()
                })
                .unwrap_or_default();
            // A class of the mod's; or one declared inside the class that makes it
            // (`ValveHandleBlockEntity.ValveHandleScrollValueBehaviour`), which the scan
            // doesn't list: named `Outer.Inner`, its methods read from the outer source.
            let mut inner = None;
            let class = match class_named(scan, &made_as) {
                Some(c) if is_value_setting(scan, &c.name) => c.name.clone(),
                Some(_) => held.clone(),
                None => match nested_class(scan, &text, &made_as) {
                    Some(n) if is_value_setting(scan, &n.parent) => {
                        let name = format!("{owner}.{made_as}");
                        inner = Some(n);
                        name
                    }
                    _ => held.clone(),
                },
            };
            // The mod's classes the making names besides it: `new MotorValueBox()`,
            // `RotationMode.class`; and those a method it calls makes
            // (`getMovementModeSlot()`, a default method of `IBearingBlockEntity`, makes a
            // `DirectionalExtenderScrollOptionSlot`).
            let mut words: Vec<String> = making
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map(str::to_string)
                .collect();
            // (The class that declares such a method is named too: its rule for which
            // sides have the box is there, `PulleyBlockEntity`'s `d == Direction.UP`.)
            let mut named = Vec::new();
            for call in called_methods(&making) {
                if let Some((declared_in, body)) = method_body(scan, &owners, &call) {
                    words.extend(
                        body.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                            .map(str::to_string),
                    );
                    if declared_in != owner && !named.contains(&declared_in) {
                        named.push(declared_in);
                    }
                }
            }
            for word in &words {
                if !word.starts_with(|c: char| c.is_ascii_uppercase()) {
                    continue;
                }
                if let Some(c) = class_named(scan, word)
                    && c.name != class
                    && !named.contains(&c.name)
                    && c.name != owner
                    && !c.name.ends_with("BlockEntity")
                {
                    named.push(c.name.clone());
                } else if class_named(scan, word).is_none()
                    && let Some(outer) =
                        nested_in(scan, &text, word).or_else(|| declared_above(scan, &owner, word))
                    && outer != owner
                    && !named.contains(&outer)
                {
                    // A type nested in another (`IControlContraption.RotationMode`, whose
                    // options' lang keys are there): the scan has the class it is in.
                    named.push(outer);
                }
            }
            // And the classes those extend, to Create's own `ValueBoxTransform`: where a
            // box is often lives there (`CenteredSideValueBoxTransform.getSouthLocation`).
            let mut i = 0;
            while i < named.len() && named.len() < 8 {
                let mut parent = scan
                    .classes
                    .iter()
                    .find(|c| c.name == named[i])
                    .and_then(|c| c.superclass.clone());
                while let Some(p) = parent.take() {
                    let Some(c) = scan.classes.iter().find(|c| c.name == p) else {
                        break;
                    };
                    if simple(&c.name) == "Sided" || simple(&c.name) == "ValueBoxTransform" {
                        if !named.contains(&c.name) {
                            named.push(c.name.clone());
                        }
                        break;
                    }
                    if !named.contains(&c.name) {
                        named.push(c.name.clone());
                    }
                    parent = c.superclass.clone();
                }
                i += 1;
            }
            let callback = text.contains(&format!("{field}.withCallback("))
                || making.contains(".withCallback(");
            // A nested class's own methods are in its body; the rest its parent's.
            let methods = METHODS.map(|(method, _)| match &inner {
                Some(n) if declares(&n.body, method) => Some(class.clone()),
                Some(n) => defining(scan, &n.parent, method),
                None => defining(scan, &class, method),
            });
            let nested_in = inner.as_ref().map(|_| owner.clone());
            out.push(ValueBox {
                entity: be.id.clone(),
                rust: be.rust.clone(),
                snake: be.snake.clone(),
                field: field.clone(),
                value,
                owner,
                class,
                named,
                callback,
                methods,
                nested_in,
            });
        }
    }
    out
}

/// Whether `class` is one of Create's scroll values: a `ScrollValueBehaviour` (or a
/// class extending it), which is a `ValueSettingsBehaviour`.
fn is_value_setting(scan: &ModScan, class: &str) -> bool {
    let mut at = scan.classes.iter().find(|c| c.name == class);
    let mut scroll = false;
    let mut settings = false;
    for _ in 0..8 {
        let Some(c) = at else {
            break;
        };
        scroll |= simple(&c.name) == "ScrollValueBehaviour";
        settings |= c
            .interfaces
            .iter()
            .any(|i| simple(i.split('<').next().unwrap_or(i)) == "ValueSettingsBehaviour");
        at = c
            .superclass
            .as_deref()
            .and_then(|s| scan.classes.iter().find(|c| c.name == s));
    }
    scroll && settings
}

/// The statement that makes `field`: from `field = new` to its `;`.
fn making(text: &str, field: &str) -> Option<String> {
    let mut from = 0;
    while let Some(at) = text[from..].find(field) {
        let start = from + at;
        from = start + field.len();
        let before = text[..start].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            continue;
        }
        let rest = text[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        if rest.starts_with('=') {
            continue;
        }
        let rest = rest.trim_start();
        if !rest.starts_with("new ") {
            continue;
        }
        return Some(rest.split(';').next().unwrap_or(rest).to_string());
    }
    None
}

/// A class declared inside another, as its source has it.
struct Nested {
    /// The mod's class it extends, fully qualified.
    parent: String,
    /// Its body, between its braces.
    body: String,
}

/// Class `name` as `text` (its outer class's source) declares it: `class
/// ValveHandleScrollValueBehaviour extends ScrollValueBehaviour {`.
fn nested_class(scan: &ModScan, text: &str, name: &str) -> Option<Nested> {
    if name.is_empty() {
        return None;
    }
    let head = format!("class {name} extends ");
    let at = text.find(&head)?;
    let rest = &text[at + head.len()..];
    let parent: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let parent = class_named(scan, &parent)?.name.clone();
    let body = body_after(&text[at..], &head)?;
    Some(Nested { parent, body })
}

/// Whether a class body declares method `name` (`public ValueSettingsBoard createBoard(`).
fn declares(body: &str, name: &str) -> bool {
    let call = format!(" {name}(");
    body.match_indices(&call).any(|(at, _)| {
        // A declaration: a type before it, a block after its parameters.
        let after = &body[at + call.len()..];
        after
            .find(')')
            .is_some_and(|close| after[close + 1..].trim_start().starts_with('{'))
            && !body[..at].trim_end().ends_with(['=', '(', ',', '.'])
    })
}

/// The class a type nested in it is reached through from `text`: by its import
/// (`import com.x.IControlContraption.RotationMode;`) or as `Outer.Inner`.
fn nested_in(scan: &ModScan, text: &str, inner: &str) -> Option<String> {
    let tail = format!(".{inner}");
    for line in text.lines() {
        let Some(path) = line
            .trim()
            .strip_prefix("import ")
            .and_then(|p| p.strip_suffix(';'))
        else {
            continue;
        };
        if let Some(outer) = path.strip_suffix(&tail)
            && scan.classes.iter().any(|c| c.name == outer)
        {
            return Some(outer.to_string());
        }
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(&tail) {
        let end = from + at;
        from = end + tail.len();
        let outer: String = text[..end]
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if outer.starts_with(|c: char| c.is_ascii_uppercase())
            && let Some(c) = class_named(scan, &outer)
        {
            return Some(c.name.clone());
        }
    }
    None
}

/// The class or interface above `class` (its own included) whose source declares type
/// `inner` inside it: a type a class reaches by inheriting it (`MechanicalBearingBlockEntity`
/// names `RotationMode`, declared in `IControlContraption`, which it implements through
/// `IBearingBlockEntity`).
fn declared_above(scan: &ModScan, class: &str, inner: &str) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut todo: std::collections::VecDeque<String> = [class.to_string()].into();
    let heads = [
        format!("enum {inner} "),
        format!("class {inner} "),
        format!("interface {inner} "),
    ];
    while let Some(next) = todo.pop_front() {
        if seen.contains(&next) || seen.len() > 24 {
            continue;
        }
        seen.push(next.clone());
        let Some(c) = scan.classes.iter().find(|c| c.name == next) else {
            continue;
        };
        if let Ok(text) = std::fs::read_to_string(std::path::Path::new(&scan.input).join(&c.path))
            && heads.iter().any(|h| text.contains(h.as_str()))
        {
            return Some(c.name.clone());
        }
        todo.extend(c.superclass.iter().cloned());
        todo.extend(
            c.interfaces
                .iter()
                .map(|i| i.split('<').next().unwrap_or(i).to_string()),
        );
    }
    None
}

/// The body of the first method whose declaration contains `head` (`void
/// addBehaviours(`), between its braces.
fn body_after(text: &str, head: &str) -> Option<String> {
    let start = text.find(head)?;
    let open = start + text[start..].find('{')?;
    let mut depth = 0;
    for (i, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[open + 1..open + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// The methods a statement calls with no arguments on its own object
/// (`getMovementModeSlot()`).
fn called_methods(statement: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = statement;
    while let Some(at) = rest.find("()") {
        let before = &rest[..at];
        let name: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let preceded = before[..before.len() - name.len()].chars().next_back();
        if name.starts_with(|c: char| c.is_ascii_lowercase())
            && preceded != Some('.')
            && !out.contains(&name)
        {
            out.push(name);
        }
        rest = &rest[at + 2..];
    }
    out
}

/// The class and body of method `name` as the nearest of `owners` (the block entity's
/// own class first), the classes above them or the interfaces any of them has declares
/// it with a body (a default method).
fn method_body(scan: &ModScan, owners: &[&ClassInfo], name: &str) -> Option<(String, String)> {
    let mut seen: Vec<String> = Vec::new();
    let mut todo: std::collections::VecDeque<String> =
        owners.iter().map(|c| c.name.clone()).collect();
    while let Some(next) = todo.pop_front() {
        if seen.contains(&next) || seen.len() > 16 {
            continue;
        }
        seen.push(next.clone());
        let Some(c) = scan.classes.iter().find(|c| c.name == next) else {
            continue;
        };
        todo.extend(c.superclass.iter().cloned());
        todo.extend(
            c.interfaces
                .iter()
                .map(|i| i.split('<').next().unwrap_or(i).to_string()),
        );
        if !c.method_names.iter().any(|m| m == name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(std::path::Path::new(&scan.input).join(&c.path))
        else {
            continue;
        };
        let decl = format!("{name}() {{");
        let Some(start) = text.find(&decl) else {
            continue;
        };
        let body = &text[start + decl.len()..];
        let mut depth = 1;
        for (i, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((c.name.clone(), body[..i].to_string()));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The mod's one class with this simple name (`MotorValueBox`, nested ones too).
fn class_named<'a>(scan: &'a ModScan, name: &str) -> Option<&'a ClassInfo> {
    let mut found = scan.classes.iter().filter(|c| simple(&c.name) == name);
    match (found.next(), found.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The class from `class` up that defines `method`.
fn defining(scan: &ModScan, class: &str, method: &str) -> Option<String> {
    let mut at = scan.classes.iter().find(|c| c.name == class);
    for _ in 0..8 {
        let c = at?;
        if c.method_names.iter().any(|m| m == method) {
            return Some(c.name.clone());
        }
        at = c
            .superclass
            .as_deref()
            .and_then(|s| scan.classes.iter().find(|c| c.name == s));
    }
    None
}

fn simple(name: &str) -> &str {
    name.rsplit(['.', '$']).next().unwrap_or(name)
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A behaviour class's function for one of `METHODS`: `kinetic_scroll_value_behaviour_board`.
fn class_function(class: &str, what: &str) -> String {
    format!("{}_{what}", snake(simple(class)))
}

/// The generated part of `runtime.rs`: the types, each value's and each behaviour
/// class's functions to port, and what all of them share.
pub fn runtime(boxes: &[ValueBox], id: &str) -> String {
    if boxes.is_empty() {
        return String::new();
    }
    let mut r = String::from(TYPES);
    let _ = write!(
        r,
        "\n/// The channel a choice on a value board goes to the server on.\npub const VALUE_CHANNEL: &str = \"{id}:value_settings\";\n"
    );
    r.push_str("\n/// Each block entity's values in boxes, in the order its `addBehaviours` adds them\n/// (`netId`): (block entity, field).\npub const VALUE_BOXES: &[(&str, &str)] = &[\n");
    for b in boxes {
        let _ = writeln!(r, "    ({:?}, {:?}),", b.entity, b.field);
    }
    r.push_str("];\n");

    // Each behaviour class's board, ported once.
    let mut done: Vec<String> = Vec::new();
    for b in boxes {
        for (i, (method, what)) in METHODS.iter().enumerate() {
            let Some(class) = &b.methods[i] else {
                continue;
            };
            let f = class_function(class, what);
            if done.contains(&f) {
                continue;
            }
            done.push(f.clone());
            let doc = match *what {
                "board" => format!(
                    "/// The board `{s}` opens for `v` (its `{method}`): its title, the largest value along a\n/// row, every how many values a milestone is marked, its rows' labels, and what its formatter\n/// shows for each choice (`texts[row][value]`, for every value from 0 to `max_value`).\n/// A label is a lang key or text: `CreateLang.translateDirect(\"x.y\")` is the key\n/// `create.x.y`, `Component.literal(\"⟳\")` the text `⟳`.\n",
                    s = simple(class)
                ),
                "settings" => format!(
                    "/// Where `v`'s value is on its board (`{method}` of `{s}`): the row and the value along it.\n",
                    s = simple(class)
                ),
                _ => format!(
                    "/// The value a choice on the board sets (`{method}` of `{s}`: what it passes to\n/// `setValue`, before it is clamped to `v.min`..=`v.max`); None where it sets nothing.\n/// Its sound is played for it.\n",
                    s = simple(class)
                ),
            };
            let (sig, body) = match *what {
                "board" => ("(v: &ScrollValue) -> Board", "    default_board(v)"),
                "settings" => (
                    "(v: &ScrollValue) -> ValueSettings",
                    "    ValueSettings { row: 0, value: v.value }",
                ),
                _ => (
                    "(v: &ScrollValue, chosen: ValueSettings) -> Option<i32>",
                    "    (chosen != ValueSettings { row: 0, value: v.value }).then_some(chosen.value)",
                ),
            };
            // A nested class is found through the class it is in.
            let of = match boxes
                .iter()
                .find(|b| &b.class == class)
                .and_then(|b| b.nested_in.as_ref())
            {
                Some(outer) => format!("`{}`, a class inside `{outer}`", simple(class)),
                None => format!("`{class}`"),
            };
            let _ = write!(
                r,
                "\n{doc}///\n/// TODO: port `{method}` of {of}.\npub fn {f}{sig} {{\n{body}\n}}\n"
            );
        }
    }

    // Each value's own functions.
    for b in boxes {
        let f = b.function();
        let named = if b.named.is_empty() {
            String::new()
        } else {
            format!(
                ", with {}",
                b.named
                    .iter()
                    .map(|c| format!("`{c}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let _ = write!(
            r,
            r#"
/// `{field}` of a `{entity}` (a `{class_simple}`): what `addBehaviours` sets up in it and what it is
/// now, read from `state` and the block (`me`). `label` is its label's lang key
/// (`CreateLang.translateDirect("x.y")` is `create.x.y`); `min` and `max` are what
/// `between` gives (a config value is its default; a `ScrollOptionBehaviour`'s are 0 and its
/// options' count less one), not the state's `_min` and `_max` fields, which only keep what
/// the Java fields start as; `value` is `state.{value}`;
/// `text` is what `formatValue()` gives (`withFormatter`, else the number); `active` is
/// `isActive()` (`onlyActiveWhen`); `options` are a `ScrollOptionBehaviour`'s options'
/// lang keys (`getTranslationKey()` of each of the enum's constants, in order).
///
/// TODO: port `{field}` of `{owner}` (made in its `addBehaviours`) as a `{class}`{named}.
pub fn {f}(state: &{rust}, me: &Neighbour) -> ScrollValue {{
    let _ = me;
    ScrollValue {{
        label: String::new(),
        value: state.{value},
        min: i32::MIN,
        max: i32::MAX,
        needs_wrench: false,
        active: true,
        text: state.{value}.to_string(),
        options: Vec::new(),
    }}
}}

/// Where the box of `{field}` is on its block seen from `side` (the face looked at): its
/// `ValueBoxTransform`'s `getLocalOffset` (a `Sided` one turned to `side` with
/// `fromSide`), in blocks from the block's corner; None where it isn't on that side
/// (`isSideActive`, `shouldRender`). A `Sided` box's `super.getLocalOffset(..)` is
/// `sided_offset(getSouthLocation(), side)`; `VecHelper.voxelSpace(x, y, z)` is
/// `[x / 16.0, y / 16.0, z / 16.0]`; `VecHelper.rotateCentered` is `rotate_centered` and
/// `VecHelper.rotate` (about the origin) `vec_rotate`; `Vec3.add` adds each coordinate;
/// `AngleHelper.horizontalAngle` and `verticalAngle` are `side_horizontal_angle` and
/// `side_vertical_angle`. The block's state properties are in `me.properties`.
///
/// TODO: port the value box of `{field}` of `{owner}` (made in its `addBehaviours`){named}.
pub fn {f}_box(me: &Neighbour, side: Side) -> Option<[f64; 3]> {{
    let _ = (me, side);
    None
}}
"#,
            field = b.field,
            entity = b.entity,
            class_simple = simple(&b.class),
            class = b.class,
            owner = b.owner,
            rust = b.rust,
            value = b.value,
        );
        if b.callback {
            let _ = write!(
                r,
                r#"
/// What `{field}` changing to `value` does (its `withCallback`), on the server: `state`
/// already has the new value.
///
/// TODO: port the callback `addBehaviours` of `{owner}` gives `{field}` (`withCallback`).
pub fn {f}_changed(state: &mut {rust}, value: i32) {{
    let _ = (state, value);
}}
"#,
                field = b.field,
                owner = b.owner,
                rust = b.rust,
            );
        }
    }

    // What dispatches to them.
    r.push_str(
        "\n/// Value `field` of a block entity `entity` (`me` its block, with its saved values).\npub fn scroll_value(entity: &str, field: &str, me: &Neighbour) -> Option<ScrollValue> {\n    let get = |k: &str| me.values.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());\n    match (entity, field) {\n",
    );
    for b in boxes {
        let _ = writeln!(
            r,
            "        ({:?}, {:?}) => Some(option_range({}(&{}::load(get), me))),",
            b.entity,
            b.field,
            b.function(),
            b.rust
        );
    }
    r.push_str("        _ => {\n            let _ = get;\n            None\n        }\n    }\n}\n");
    r.push_str(
        "\n/// Where the box of value `field` of `entity` is, seen from `side` (blocks from the corner).\npub fn value_box_at(entity: &str, field: &str, me: &Neighbour, side: Side) -> Option<[f64; 3]> {\n    match (entity, field) {\n",
    );
    for b in boxes {
        let _ = writeln!(
            r,
            "        ({:?}, {:?}) => {}_box(me, side),",
            b.entity,
            b.field,
            b.function()
        );
    }
    r.push_str("        _ => None,\n    }\n}\n");
    for (i, (_, what)) in METHODS.iter().enumerate() {
        let (sig, args, default) = match *what {
            "board" => (
                "v: &ScrollValue) -> Board",
                "v",
                "default_board(v)".to_string(),
            ),
            "settings" => (
                "v: &ScrollValue) -> ValueSettings",
                "v",
                "ValueSettings { row: 0, value: v.value }".to_string(),
            ),
            _ => (
                "v: &ScrollValue, chosen: ValueSettings) -> Option<i32>",
                "v, chosen",
                "(chosen != ValueSettings { row: 0, value: v.value }).then_some(chosen.value)"
                    .to_string(),
            ),
        };
        // An option value's board picks by icons, as `ScrollOptionSettingsFormatter`
        // makes every `ScrollOptionBehaviour`'s, whatever its port said.
        let (open, close) = if *what == "board" {
            (
                "    let mut board = match (entity, field) {\n",
                "    };\n    board.icons |= !v.options.is_empty();\n    board\n}\n",
            )
        } else {
            ("    match (entity, field) {\n", "    }\n}\n")
        };
        let _ = write!(
            r,
            "\n/// `{what}` of the class value `field` of `entity` is made as.\npub fn value_{what}(entity: &str, field: &str, {sig} {{\n{open}"
        );
        for b in boxes {
            if let Some(class) = &b.methods[i] {
                let _ = writeln!(
                    r,
                    "        ({:?}, {:?}) => {}({args}),",
                    b.entity,
                    b.field,
                    class_function(class, what)
                );
            }
        }
        let _ = write!(r, "        _ => {default},\n{close}");
    }
    r.push_str(
        "\n/// A choice on the board of value `field` of the block entity `me` is, on the server\n/// (`setValueSettings`, then `setValue`): its values to save when it changed, else None.\npub fn choose_value(entity: &str, field: &str, me: &Neighbour, chosen: ValueSettings) -> Option<Vec<(&'static str, Saved)>> {\n    let get = |k: &str| me.values.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());\n    match (entity, field) {\n",
    );
    for b in boxes {
        let changed = if b.callback {
            format!("\n            {}_changed(&mut state, to);", b.function())
        } else {
            String::new()
        };
        let _ = writeln!(
            r,
            "        ({e:?}, {fl:?}) => {{\n            let mut state = {rust}::load(get);\n            let v = option_range({f}(&state, me));\n            let to = value_set(entity, field, &v, chosen)?.clamp(v.min.min(v.max), v.max);\n            if to == state.{value} {{\n                return None;\n            }}\n            state.{value} = to;{changed}\n            Some(state.save())\n        }}",
            e = b.entity,
            fl = b.field,
            rust = b.rust,
            f = b.function(),
            value = b.value,
        );
    }
    r.push_str("        _ => {\n            let _ = (get, chosen);\n            None\n        }\n    }\n}\n");
    r.push_str(SHARED);
    r
}

/// The tests of the generated part.
pub fn tests(boxes: &[ValueBox]) -> String {
    let Some(first) = boxes.first() else {
        return String::new();
    };
    format!(
        r#"
    #[test]
    fn value_boards_find_the_value_they_show() {{
        use super::*;
        let v = ScrollValue {{ max: 256, min: -256, value: 16, ..Default::default() }};
        for board in [default_board(&v), value_board({e:?}, {f:?}, &v)] {{
            let layout = board_layout(&board, [427.0, 240.0], &|t: &str| t.len() as f32 * 6.0);
            for row in 0..board.rows.len() as i32 {{
                for value in [0, 1, board.max_value / 2, board.max_value] {{
                    let at = coordinate_of_value(&layout, &board, row, value);
                    assert_eq!(closest(&layout, &board, at, false), ValueSettings {{ row, value }});
                }}
            }}
            let mut ops = Vec::new();
            draw_board(&board, &layout, ValueSettings {{ row: 0, value: 3 }}, 10.0, &mut ops, &|t: &str| t.len() as f32 * 6.0, &|k: &str, _: &[&str]| k.to_string());
            assert!(!ops.is_empty());
        }}
    }}

    #[test]
    fn a_value_choice_goes_to_the_server_and_back() {{
        use super::*;
        let data = encode_choice([4, -60, 9], 1, ValueSettings {{ row: 1, value: 200 }});
        assert_eq!(decode_choice(&data), Some(([4, -60, 9], 1, ValueSettings {{ row: 1, value: 200 }})));
        assert_eq!(decode_choice(&data[..5]), None);
        assert_eq!(value_field({e:?}, 0), Some({f:?}));
        assert_eq!(value_index({e:?}, {f:?}), Some(0));
        // Options span all of them, whatever the port said.
        let v = option_range(ScrollValue {{ options: vec!["a".into(), "b".into(), "c".into()], max: 1, ..Default::default() }});
        assert_eq!((v.min, v.max), (0, 2));
    }}

    #[test]
    fn the_side_looked_at_is_the_face_hit() {{
        use super::*;
        let full = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        assert_eq!(hit_side([0.5, 1.0, 0.5], [0.0, -1.0, 0.0], &full), Side::Up);
        assert_eq!(hit_side([0.0, 0.4, 0.5], [1.0, -0.2, 0.0], &full), Side::West);
        assert_eq!(hit_side([0.5, 0.4, 1.0], [0.0, -0.2, -1.0], &full), Side::South);
        // A box on the south face, turned to each side as `Sided` turns it.
        let south = [0.5, 0.5, 12.5 / 16.0];
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9);
        assert!(near(sided_offset(south, Side::South), south));
        assert!(near(sided_offset(south, Side::East), [12.5 / 16.0, 0.5, 0.5]));
        assert!(near(sided_offset(south, Side::West), [3.5 / 16.0, 0.5, 0.5]));
        assert!(near(sided_offset(south, Side::North), [0.5, 0.5, 3.5 / 16.0]));
        assert!(near(sided_offset(south, Side::Up), [0.5, 12.5 / 16.0, 0.5]));
        assert!(near(sided_offset(south, Side::Down), [0.5, 3.5 / 16.0, 0.5]));
    }}
"#,
        e = first.entity,
        f = first.field,
    )
}

const TYPES: &str = r#"
/// A choice on a value board (`ValueSettingsBehaviour.ValueSettings`): its row, and its
/// value along the row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValueSettings {
    pub row: i32,
    pub value: i32,
}

/// A value set in a box on a block (Create's `ScrollValueBehaviour` and the classes
/// that extend it), as its block entity's `addBehaviours` sets it up and as it is now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollValue {
    /// Its label: a lang key (`create.kinetics.creative_motor.rotation_speed`) or text.
    pub label: String,
    pub value: i32,
    /// `between(min, max)`.
    pub min: i32,
    pub max: i32,
    /// `requiresWrench()`: shown and set only with a wrench in hand.
    pub needs_wrench: bool,
    /// `isActive()` (`onlyActiveWhen(..)`): false hides it.
    pub active: bool,
    /// What its box shows: `formatValue()` (`withFormatter(..)`, else the number).
    pub text: String,
    /// A `ScrollOptionBehaviour`'s options, in order: each one's lang key
    /// (`getTranslationKey()`). Empty for a number.
    pub options: Vec<String>,
}

/// A value board (`ValueSettingsBoard`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    /// Its title: a lang key or text.
    pub title: String,
    /// The largest value along a row (`maxValue`), from 0.
    pub max_value: i32,
    /// Every how many values a milestone is marked (`milestoneInterval`).
    pub milestone_interval: i32,
    /// Its rows' labels (`rows`): lang keys or text.
    pub rows: Vec<String>,
    /// What its formatter shows for each choice: `texts[row][value]`
    /// (`formatter.format(new ValueSettings(row, value))`), lang keys or text.
    pub texts: Vec<Vec<String>>,
    /// Whether it picks among options shown by their icons
    /// (`ScrollOptionSettingsFormatter`): wider milestones, no row labels.
    pub icons: bool,
}

impl Board {
    /// What its formatter shows for `s`; the number where it has nothing.
    pub fn text(&self, s: ValueSettings) -> String {
        self.texts
            .get(s.row.max(0) as usize)
            .and_then(|r| r.get(s.value.max(0) as usize))
            .cloned()
            .unwrap_or_else(|| s.value.to_string())
    }
}

/// `ScrollValueBehaviour`'s own board: `new ValueSettingsBoard(label, max, 10,
/// ImmutableList.of(Component.literal("Value")), new ValueSettingsFormatter(ValueSettings::format))`.
pub fn default_board(v: &ScrollValue) -> Board {
    Board {
        title: v.label.clone(),
        max_value: v.max.max(0),
        milestone_interval: 10,
        rows: vec!["Value".to_string()],
        texts: vec![(0..=v.max.clamp(0, 4096)).map(|i| i.to_string()).collect()],
        icons: false,
    }
}
"#;

const SHARED: &str = r#"
/// An option value's range as `ScrollOptionBehaviour`'s constructor sets it,
/// `between(0, options.length - 1)`, whatever a port of its block entity said.
/// Its box shows the option chosen (Java draws that option's icon; here its name).
pub fn option_range(mut v: ScrollValue) -> ScrollValue {
    if !v.options.is_empty() {
        v.min = 0;
        v.max = v.options.len() as i32 - 1;
        v.text = v.options[v.value.clamp(0, v.max) as usize].clone();
    }
    v
}

/// Value `index` of block entity `entity` (`netId`), and back.
pub fn value_field(entity: &str, index: usize) -> Option<&'static str> {
    VALUE_BOXES.iter().filter(|(e, _)| *e == entity).nth(index).map(|(_, f)| *f)
}

pub fn value_index(entity: &str, field: &str) -> Option<usize> {
    VALUE_BOXES.iter().filter(|(e, _)| *e == entity).position(|(_, f)| *f == field)
}

/// Whether block entity `entity` holds values in boxes.
pub fn has_value_boxes(entity: &str) -> bool {
    VALUE_BOXES.iter().any(|(e, _)| *e == entity)
}

/// A choice as the client sends it: the block, which of its values, the row and the value.
pub fn encode_choice(pos: [i32; 3], index: usize, s: ValueSettings) -> Vec<u8> {
    let mut out = Vec::with_capacity(21);
    for v in [pos[0], pos[1], pos[2], index as i32, s.row, s.value] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn decode_choice(data: &[u8]) -> Option<([i32; 3], usize, ValueSettings)> {
    if data.len() != 24 {
        return None;
    }
    let at = |i: usize| i32::from_le_bytes([data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]]);
    let index = usize::try_from(at(3)).ok()?;
    Some(([at(0), at(1), at(2)], index, ValueSettings { row: at(4), value: at(5) }))
}

/// The face a look along `look` hit at `local` (blocks from the block's corner), of a
/// block with collision boxes `boxes`.
pub fn hit_side(local: [f64; 3], look: [f64; 3], boxes: &[[f64; 6]]) -> Side {
    const EPS: f64 = 1e-3;
    let mut best: Option<(f64, Side)> = None;
    let full = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
    let boxes = if boxes.is_empty() { &full[..] } else { boxes };
    for b in boxes {
        for axis in 0..3 {
            let (low, high) = match axis {
                0 => (Side::West, Side::East),
                1 => (Side::Down, Side::Up),
                _ => (Side::North, Side::South),
            };
            // The face the look comes in through, where the hit lies on it.
            let (plane, side, facing) = if look[axis] > 0.0 {
                (b[axis], low, look[axis])
            } else {
                (b[axis + 3], high, -look[axis])
            };
            if (local[axis] - plane).abs() > EPS || facing <= 0.0 {
                continue;
            }
            if best.is_none_or(|(f, _)| facing > f) {
                best = Some((facing, side));
            }
        }
    }
    if let Some((_, side)) = best {
        return side;
    }
    // Not on a box's face: the side of the block it is nearest.
    let d = [local[0] - 0.5, local[1] - 0.5, local[2] - 0.5];
    let axis = (0..3).max_by(|a, b| d[*a].abs().total_cmp(&d[*b].abs())).unwrap_or(1);
    match (axis, d[axis] > 0.0) {
        (0, false) => Side::West,
        (0, true) => Side::East,
        (1, false) => Side::Down,
        (1, true) => Side::Up,
        (_, false) => Side::North,
        _ => Side::South,
    }
}

/// A value box on a block looked at: which value, what it is, where its box is.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub field: &'static str,
    pub value: ScrollValue,
    /// Its box's middle, in blocks from the block's corner, and the face it is on.
    pub at: [f64; 3],
    pub side: Side,
    /// Whether the look is on it (`testHit`): its tip shows, and a held use key opens
    /// its board.
    pub highlight: bool,
}

/// The value boxes of block entity `entity` (`me`) seen when looking at `local` on
/// face `side`, as `ScrollValueRenderer.tick` picks them: the first the look is on is
/// highlighted. `wrench` is whether the main hand holds a wrench.
pub fn looked_at(entity: &str, me: &Neighbour, local: [f64; 3], side: Side, wrench: bool) -> Vec<Shown> {
    let mut out = Vec::new();
    let mut found = false;
    for (e, field) in VALUE_BOXES {
        if *e != entity {
            continue;
        }
        let Some(value) = scroll_value(entity, field, me) else {
            continue;
        };
        if !value.active || (value.needs_wrench && !wrench) {
            continue;
        }
        let Some(at) = value_box_at(entity, field, me, side) else {
            continue;
        };
        let d = [local[0] - at[0], local[1] - at[1], local[2] - at[2]];
        // `ValueBoxTransform.testHit`: within half its scale (0.5) of the box's middle.
        let on = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 0.25;
        let highlight = on && !found;
        found |= highlight;
        out.push(Shown { field, value, at, side, highlight });
    }
    out
}

/// Where a board is drawn (`ValueSettingsScreen.init`), in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoardLayout {
    pub left: f32,
    pub top: f32,
    pub label_width: f32,
    pub bar_width: f32,
    pub width: f32,
    pub height: f32,
    pub milestone: f32,
    pub scale: f32,
}

/// `ValueSettingsScreen.init`: the board's size, in the middle of a screen `screen`
/// pixels wide and high. `width_of` measures text.
pub fn board_layout(board: &Board, screen: [f32; 2], width_of: &dyn Fn(&str) -> f32) -> BoardLayout {
    let max = board.max_value.max(1);
    let interval = board.milestone_interval.max(1);
    let milestones = max / interval + 1;
    let scale = if max > 128 { 1.0 } else { 2.0 };
    let milestone = if board.icons { 8.0 } else { 4.0 };
    let label_width = if board.icons {
        -18.0
    } else {
        board.rows.iter().map(|r| width_of(r)).fold(0.0, f32::max)
    };
    let bar_width = (max + 1) as f32 * scale + 1.0 + milestones as f32 * milestone;
    let width = (label_width + 14.0) + (bar_width + 10.0);
    let height = board.rows.len() as f32 * 11.0;
    BoardLayout {
        left: ((screen[0] - width) / 2.0).floor(),
        top: ((screen[1] - height) / 2.0).floor(),
        label_width,
        bar_width,
        width,
        height,
        milestone,
        scale,
    }
}

/// `getCoordinateOfValue`: where value `column` of row `row` is on the board.
pub fn coordinate_of_value(l: &BoardLayout, board: &Board, row: i32, column: i32) -> [f32; 2] {
    let interval = board.milestone_interval.max(1);
    let mut x = l.left + ((column.max(1) - 1) / interval) as f32 * l.milestone + column as f32 * l.scale + 1.5;
    x += l.label_width + 14.0 + 4.0;
    if column % interval == 0 {
        x += l.milestone / 2.0;
    }
    if column > 0 {
        x += l.milestone;
    }
    let y = l.top + (row as f32 + 0.5) * 11.0 - 0.5;
    [x, y]
}

/// `getClosestCoordinate`: the choice nearest `at`; with `milestones_only` (shift held)
/// only the marked values.
pub fn closest(l: &BoardLayout, board: &Board, at: [f32; 2], milestones_only: bool) -> ValueSettings {
    let rows = board.rows.len().max(1) as i32;
    let interval = board.milestone_interval.max(1);
    let mut row = 0;
    let mut best = f32::MAX;
    while row < rows {
        let diff = (coordinate_of_value(l, board, row, 0)[1] - at[1]).abs();
        if best < diff {
            break;
        }
        best = diff;
        row += 1;
    }
    row -= 1;
    let mut column = 0;
    best = f32::MAX;
    while column <= board.max_value {
        let c = if milestones_only { column * interval } else { column };
        let diff = (coordinate_of_value(l, board, row, c)[0] - at[0]).abs();
        if best < diff {
            break;
        }
        best = diff;
        column += 1;
    }
    column -= 1;
    let value = if milestones_only { (column * interval).min(board.max_value) } else { column };
    ValueSettings { row, value: value.clamp(0, board.max_value.max(0)) }
}

/// The board's texture (Create's `textures/gui/value_settings.png`) and its parts
/// (`AllGuiTextures`): `[x, y, width, height]`.
pub const BOARD_TEXTURE: &str = "create:gui/value_settings";
const BRASS_TL: [f32; 4] = [65.0, 9.0, 4.0, 4.0];
const BRASS_TR: [f32; 4] = [70.0, 9.0, 4.0, 4.0];
const BRASS_BL: [f32; 4] = [65.0, 19.0, 4.0, 4.0];
const BRASS_BR: [f32; 4] = [70.0, 19.0, 4.0, 4.0];
const BRASS_LEFT: [f32; 4] = [65.0, 14.0, 3.0, 4.0];
const BRASS_RIGHT: [f32; 4] = [71.0, 14.0, 3.0, 4.0];
const BRASS_TOP: [f32; 4] = [0.0, 24.0, 256.0, 3.0];
const BRASS_BOTTOM: [f32; 4] = [0.0, 27.0, 256.0, 3.0];
const MILESTONE: [f32; 4] = [0.0, 0.0, 7.0, 8.0];
const WIDE_MILESTONE: [f32; 4] = [75.0, 14.0, 13.0, 8.0];
const BAR: [f32; 4] = [7.0, 0.0, 249.0, 8.0];
const BAR_BG: [f32; 4] = [75.0, 9.0, 1.0, 1.0];
const OUTER_BG: [f32; 4] = [80.0, 9.0, 1.0, 1.0];
const CURSOR_LEFT: [f32; 4] = [0.0, 9.0, 3.0, 14.0];
const CURSOR: [f32; 4] = [4.0, 9.0, 56.0, 14.0];
const CURSOR_RIGHT: [f32; 4] = [61.0, 9.0, 3.0, 14.0];
const LABEL_BG: [f32; 4] = [0.0, 31.0, 161.0, 11.0];
const CURSOR_ICON: [f32; 4] = [0.0, 44.0, 22.0, 20.0];

fn stretched(ops: &mut Vec<GuiOp>, part: [f32; 4], dst: [f32; 4]) {
    if dst[2] > 0.0 && dst[3] > 0.0 {
        ops.push(GuiOp::Sprite { name: BOARD_TEXTURE.to_string(), src: part, dst });
    }
}

/// `UIRenderHelper.drawCropped`: the part's top left corner, as wide and high as `dst`.
/// Wider than the part, Java's texture repeats it (a 294-pixel frame from a 256-pixel
/// strip), so it is drawn again from its start.
fn cropped(ops: &mut Vec<GuiOp>, part: [f32; 4], dst: [f32; 4]) {
    if dst[2] <= 0.0 || dst[3] <= 0.0 {
        return;
    }
    let mut x = 0.0;
    while x < dst[2] {
        let w = (dst[2] - x).min(part[2].max(1.0));
        ops.push(GuiOp::Sprite {
            name: BOARD_TEXTURE.to_string(),
            src: [part[0], part[1], w, dst[3]],
            dst: [dst[0] + x, dst[1], w, dst[3]],
        });
        x += w;
    }
}

fn whole(ops: &mut Vec<GuiOp>, part: [f32; 4], at: [f32; 2]) {
    ops.push(GuiOp::Sprite { name: BOARD_TEXTURE.to_string(), src: part, dst: [at[0], at[1], part[2], part[3]] });
}

fn text(ops: &mut Vec<GuiOp>, text: &str, at: [f32; 2], rgb: u32) {
    ops.push(GuiOp::Text { text: text.to_string(), at, rgb, shadow: false });
}

/// `renderBrassFrame`.
fn brass_frame(ops: &mut Vec<GuiOp>, x: f32, y: f32, w: f32, h: f32) {
    whole(ops, BRASS_TL, [x, y]);
    whole(ops, BRASS_TR, [x + w - 4.0, y]);
    whole(ops, BRASS_BL, [x, y + h - 4.0]);
    whole(ops, BRASS_BR, [x + w - 4.0, y + h - 4.0]);
    if h > 8.0 {
        stretched(ops, BRASS_LEFT, [x, y + 4.0, 3.0, h - 8.0]);
        stretched(ops, BRASS_RIGHT, [x + w - 3.0, y + 4.0, 3.0, h - 8.0]);
    }
    if w > 8.0 {
        cropped(ops, BRASS_TOP, [x + 4.0, y, w - 8.0, 3.0]);
        cropped(ops, BRASS_BOTTOM, [x + 4.0, y + h - 3.0, w - 8.0, 3.0]);
    }
}

/// `ValueSettingsScreen.renderBackground` and `renderWindow`: the board with the cursor on
/// `chosen`, `ticks_open` ticks after it opened. `width_of` measures text, `tr` turns a
/// lang key (with its arguments) into text.
#[allow(clippy::too_many_arguments)]
pub fn draw_board(
    board: &Board,
    l: &BoardLayout,
    chosen: ValueSettings,
    ticks_open: f32,
    ops: &mut Vec<GuiOp>,
    width_of: &dyn Fn(&str) -> f32,
    tr: &dyn Fn(&str, &[&str]) -> String,
) {
    let (x, y) = (l.left, l.top);
    let max = board.max_value.max(1);
    let interval = board.milestone_interval.max(1);
    let milestones = max / interval + 1;
    let title = tr(&board.title, &[]);
    // (`Component.keybind("key.use")`: the use key's button, which is the right one.)
    let tip = tr("create.gui.value_settings.release_to_confirm", &[&tr("key.mouse.right", &[])]);
    let fade_in = (ticks_open / 4.0).clamp(0.0, 1.0);
    let mut fattest = width_of(&tip).max(width_of(&title));
    if board.icons {
        for i in 0..=board.max_value.max(0) {
            fattest = fattest.max(width_of(&tr(&board.text(ValueSettings { row: 0, value: i }), &[])));
        }
    }
    let fat_offset = (((fattest + 10.0 - (l.width + 13.0)).max(0.0)) / 2.0).floor();
    let bg_width = (l.width + 13.0).max(fattest + 10.0);
    let fade_width = (bg_width * fade_in).floor();
    let fade_start = ((bg_width - fade_width) / 2.0).floor() - fat_offset;
    let extra = if board.icons { 46.0 } else { 33.0 };

    stretched(ops, OUTER_BG, [x - 11.0 + fade_start, y - 17.0, fade_width, l.height + extra]);
    stretched(ops, OUTER_BG, [x - 10.0 + fade_start, y - 18.0, fade_width - 2.0, 1.0]);
    if fade_width > fattest {
        let middle = x - 11.0 - fat_offset + (bg_width / 2.0).floor();
        text(ops, &title, [middle - (width_of(&title) / 2.0).floor(), y - 14.0], 0xDDDDDD);
        text(ops, &tip, [middle - (width_of(&tip) / 2.0).floor(), y + l.height + extra - 27.0], 0xDDDDDD);
    }
    let rows = board.rows.len() as f32;
    brass_frame(ops, x + l.label_width + 14.0, y - 3.0, l.bar_width + 8.0, rows * 11.0 + 5.0);
    stretched(ops, BAR_BG, [x + l.label_width + 17.0, y, l.bar_width + 2.0, rows * 11.0 - 1.0]);
    let mut row_y = y;
    for label in &board.rows {
        let bar_x = x + l.label_width + 14.0 + 4.0;
        if !board.icons {
            cropped(ops, LABEL_BG, [x - 4.0, row_y, l.label_width + 8.0, 11.0]);
            let mut w = 0.0;
            while w < l.bar_width {
                cropped(ops, BAR, [bar_x + w, row_y + 1.0, (BAR[2] - 1.0).min(l.bar_width - w), 8.0]);
                w += BAR[2] - 1.0;
            }
            text(ops, &tr(label, &[]), [x, row_y + 1.0], 0x442000);
        }
        let mut mx = bar_x;
        for _ in 0..milestones {
            whole(ops, if board.icons { WIDE_MILESTONE } else { MILESTONE }, [mx, row_y + 1.0]);
            mx += l.milestone + interval as f32 * l.scale;
        }
        row_y += 11.0;
    }
    if !board.icons {
        brass_frame(ops, x - 7.0, y - 3.0, l.label_width + 14.0, rows * 11.0 + 5.0);
    }
    if ticks_open < 1.0 {
        return;
    }
    // The cursor, on the choice.
    let at = coordinate_of_value(l, board, chosen.row, chosen.value);
    let shown = tr(&board.text(chosen), &[]);
    if board.icons {
        // An option's cursor holds its icon (not drawn here: Create's icon sheet), and
        // the option's name is under the board.
        let cx = at[0].floor() - 9.0;
        let cy = at[1].floor() - 7.0;
        whole(ops, CURSOR_ICON, [cx - 2.0, cy - 3.0]);
        if fade_width > fattest {
            let tx = x - 11.0 - fat_offset + ((bg_width - width_of(&shown)) / 2.0).floor();
            text(ops, &shown, [tx, y + l.height + extra - 40.0], 0xFBDC7D);
        }
        return;
    }
    let cursor_w = ((width_of(&shown) / 2.0).floor()) * 2.0 + 3.0;
    let cx = at[0].floor() - (cursor_w / 2.0).floor();
    let cy = at[1].floor() - 7.0;
    whole(ops, CURSOR_LEFT, [cx - 3.0, cy]);
    cropped(ops, CURSOR, [cx, cy, cursor_w, 14.0]);
    whole(ops, CURSOR_RIGHT, [cx + cursor_w, cy]);
    text(ops, &shown, [cx + 2.0, cy + 3.0], 0x442000);
}

/// The value settings on this client: the use key held on a box, the board open, and the
/// box looked at.
#[derive(Clone, Debug, Default)]
pub struct ValueClient {
    /// Seconds since the game started, from the frames.
    pub clock: f64,
    /// Whether the use key is down.
    pub use_down: bool,
    /// The use key held on a box since `since` (`interactHeldTicks`).
    pub hold: Option<ValueHold>,
    /// The board open.
    pub board: Option<OpenBoard>,
    /// A choice to send to the server, and close the board.
    pub send: Option<Vec<u8>>,
    /// When the look first came on the box highlighted now (`hoverWarmup`).
    pub hover_since: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueHold {
    pub pos: [i32; 3],
    pub entity: String,
    pub index: usize,
    pub since: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenBoard {
    pub pos: [i32; 3],
    pub entity: String,
    pub index: usize,
    pub board: Board,
    /// Where its value was when it opened: the cursor starts there.
    pub initial: ValueSettings,
    pub opened: f64,
    /// The mouse when it opened, and where the cursor stood then: the cursor moves with
    /// the mouse from there (Java puts the mouse on the value).
    pub mouse_at_open: Option<[f32; 2]>,
    pub cursor_at_open: Option<[f32; 2]>,
    /// What the wheel moved the cursor by.
    pub scrolled: [f32; 2],
}

static VALUE_CLIENT: std::sync::Mutex<Option<ValueClient>> = std::sync::Mutex::new(None);

/// Runs `f` on the client's value settings.
pub fn value_client<R>(f: impl FnOnce(&mut ValueClient) -> R) -> R {
    let mut lock = VALUE_CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    f(lock.get_or_insert_with(ValueClient::default))
}

impl OpenBoard {
    /// Where the cursor is on board `l`: on the value it had when it opened, moved as the
    /// mouse has moved since (Java puts the mouse itself there).
    pub fn cursor(&mut self, l: &BoardLayout, mouse: [f32; 2]) -> [f32; 2] {
        if self.cursor_at_open.is_none() {
            self.cursor_at_open = Some(coordinate_of_value(l, &self.board, self.initial.row, self.initial.value));
        }
        let start = self.cursor_at_open.unwrap_or(mouse);
        let origin = *self.mouse_at_open.get_or_insert(mouse);
        [
            start[0] + mouse[0] - origin[0] + self.scrolled[0],
            start[1] + mouse[1] - origin[1] + self.scrolled[1],
        ]
    }
}

/// `VecHelper.rotate(v, degrees, axis)`: `v` turned about the origin (not the block's
/// middle), `axis` `'x'`, `'y'` or `'z'`, as Create turns it.
pub fn vec_rotate(v: [f64; 3], degrees: f64, axis: char) -> [f64; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    let [x, y, z] = v;
    match axis {
        'x' => [x, y * c - z * s, z * c + y * s],
        'y' => [x * c + z * s, y, z * c - x * s],
        _ => [x * c - y * s, y * c + x * s, z],
    }
}

/// `VecHelper.rotateCentered(v, degrees, axis)`: `v` (blocks from the corner) turned
/// about the block's middle.
pub fn rotate_centered(v: [f64; 3], degrees: f64, axis: char) -> [f64; 3] {
    let [x, y, z] = vec_rotate([v[0] - 0.5, v[1] - 0.5, v[2] - 0.5], degrees, axis);
    [x + 0.5, y + 0.5, z + 0.5]
}

/// `AngleHelper.horizontalAngle(side)`: south 0, west -90, north 180, east -270; up and
/// down 0.
pub fn side_horizontal_angle(side: Side) -> f64 {
    match side {
        Side::South => 0.0,
        Side::West => -90.0,
        Side::North => 180.0,
        Side::East => -270.0,
        Side::Up | Side::Down => 0.0,
    }
}

/// `AngleHelper.verticalAngle(side)`: up -90, down 90, the rest 0.
pub fn side_vertical_angle(side: Side) -> f64 {
    match side {
        Side::Up => -90.0,
        Side::Down => 90.0,
        _ => 0.0,
    }
}

/// `ValueBoxTransform.Sided.getLocalOffset`: its `getSouthLocation()` (`south`, in
/// blocks) turned to face `side`.
pub fn sided_offset(south: [f64; 3], side: Side) -> [f64; 3] {
    let v = rotate_centered(south, side_horizontal_angle(side), 'y');
    rotate_centered(v, side_vertical_angle(side), 'x')
}
"#;

/// The client half of a held use key on a value box: what the crosshair is on. Pasted
/// into the hook helpers that need it, with `game` a `&mut Game`; gives `target`:
/// `Option<(pos, entity, Neighbour, local hit, side)>`.
const TARGET: &str = r#"    // What the crosshair is on: one of the mod's block entities with values in boxes.
    let target = (|| {
        use horizon_world::nbt::Tag;
        let (_, hit) = game.native_view_hit()?;
        let pos = [hit.block_x, hit.block_y, hit.block_z];
        let (entity, values) = game.native_with_mod_block_entity(pos, |e| {
            let values: Vec<(String, m::Saved)> = e
                .nbt
                .iter()
                .filter_map(|(k, tag)| {
                    let v = match tag {
                        Tag::Byte(v) => m::Saved::Byte(*v),
                        Tag::Short(v) => m::Saved::Short(*v),
                        Tag::Int(v) => m::Saved::Int(*v),
                        Tag::Long(v) => m::Saved::Long(*v),
                        Tag::Float(v) => m::Saved::Float(f32::from_bits(*v)),
                        Tag::Double(v) => m::Saved::Double(f64::from_bits(*v)),
                        Tag::String(v) => m::Saved::Text(v.to_string_lossy()),
                        _ => return None,
                    };
                    Some((k.to_string_lossy(), v))
                })
                .collect();
            (e.id.clone(), values)
        })?;
        if !m::has_value_boxes(&entity) {
            return None;
        }
        let local = [hit.x - f64::from(pos[0]), hit.y - f64::from(pos[1]), hit.z - f64::from(pos[2])];
        let look = match game.native_player() {
            Some(p) => {
                let (yaw, pitch) = (f64::from(p.yaw).to_radians(), f64::from(p.pitch).to_radians());
                [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()]
            }
            None => [0.0, -1.0, 0.0],
        };
        let side = m::hit_side(local, look, &game.native_block_collision(pos));
        let me = m::Neighbour {
            block: game.native_block(pos),
            pos,
            properties: m::STATE_KEYS
                .iter()
                .filter_map(|key| Some((key.to_string(), game.native_block_property(pos, key)?)))
                .collect(),
            entity: Some(entity.clone()),
            values,
            items: Vec::new(),
        };
        Some((pos, entity, me, local, side))
    })();
    // Whether the main hand holds a wrench (`requiresWrench`).
    let wrench = game.native_held().is_some_and(|(item, _, _)| item.ends_with(":wrench"));
    // What Java's `canInteract` allows: not crouching.
    let crouching = game
        .player
        .as_ref()
        .is_some_and(|p| matches!(p.pose, crate::pose::Pose::Crouching));
"#;

/// The helper part for `hook`, pasted after `use crate::game::<module> as m;` in a
/// helper of the mod's own, or the whole helper where the mod has none for it.
pub fn part(hook: &str) -> Option<&'static str> {
    Some(match hook {
        "key" => KEY_PART,
        "frame" => FRAME_PART,
        "hud" => HUD_PART,
        "screen_render_post" => RENDER_PART,
        "screen_mouse_scrolled_pre" => SCROLL_PART,
        "net_from_client" => NET_PART,
        _ => return None,
    })
}

const KEY_PART: &str = r#"    // Create's value settings: whether the use key is down (`keyUse.isDown()`).
    if key == "key.mouse.right" {
        m::value_client(|c| c.use_down = pressed);
    }
"#;

const FRAME_PART: &str = r#"    // Create's value settings: the use key held on a value box opens its board after
    // five ticks (`ValueSettingsClient.tick`); a choice made on it goes to the server.
    if game.native_on(m::ID) {
        let now = m::value_client(|c| {
            c.clock += elapsed.as_secs_f64();
            c.clock
        });
        if let Some(data) = m::value_client(|c| c.send.take()) {
            let _ = game.native_net_send_to_server(m::VALUE_CHANNEL, &data);
            game.native_mod_screen_close(m::ID);
            m::value_client(|c| c.board = None);
        }
        // Escape (or another screen) closed it.
        if game.native_mod_screen() != Some(m::ID) {
            m::value_client(|c| c.board = None);
        }
        if let Some(hold) = m::value_client(|c| c.hold.clone()) {
            __TARGET__
            let field = m::value_field(&hold.entity, hold.index);
            let shown = match (&target, field) {
                (Some((pos, entity, me, local, side)), Some(field)) if *pos == hold.pos && *entity == hold.entity => {
                    m::looked_at(entity, me, *local, *side, wrench)
                        .into_iter()
                        .find(|s| s.field == field && s.highlight)
                        .map(|s| (me.clone(), s))
                }
                _ => None,
            };
            let use_down = m::value_client(|c| c.use_down);
            match shown {
                None => m::value_client(|c| c.hold = None),
                Some(_) if crouching => m::value_client(|c| c.hold = None),
                // Let go before the board opened: a short click, which does nothing more.
                Some(_) if !use_down => m::value_client(|c| c.hold = None),
                Some((_, s)) if now - hold.since >= 0.25 => {
                    let field = s.field;
                    let board = m::value_board(&hold.entity, field, &s.value);
                    let initial = m::value_settings(&hold.entity, field, &s.value);
                    m::value_client(|c| c.hold = None);
                    if game.native_mod_screen_open(m::ID) {
                        m::value_client(|c| {
                            c.board = Some(m::OpenBoard {
                                pos: hold.pos,
                                entity: hold.entity.clone(),
                                index: hold.index,
                                board,
                                initial,
                                opened: now,
                                mouse_at_open: None,
                                cursor_at_open: None,
                                scrolled: [0.0, 0.0],
                            })
                        });
                    }
                }
                Some(_) => {}
            }
        }
    }
"#;

const HUD_PART: &str = r#"    // Create's value settings: the boxes on the block looked at, with their values, and
    // the tip of the one the look is on (`ScrollValueRenderer`, `ValueSettingsClient.render`).
    if game.native_on(m::ID) && game.native_mod_screen().is_none() {
        __TARGET__
        let now = m::value_client(|c| c.clock);
        let shown = match &target {
            Some((_, entity, me, local, side)) if !crouching => m::looked_at(entity, me, *local, *side, wrench),
            _ => Vec::new(),
        };
        let tr = |key: &str| horizon_client::text::Text::translate(key, &[]).plain();
        let [w, h] = [batch.width, batch.height];
        // Each box's corners on the screen, from the camera.
        let camera = game.native_camera();
        let eye = match (&camera, game.native_player()) {
            (Some(c), _) => Some(c.position),
            (None, Some(p)) => Some([p.x, p.y + 1.62, p.z]),
            _ => None,
        };
        let (yaw, pitch, fov) = match (&camera, game.native_player()) {
            (Some(c), _) => (c.yaw, c.pitch, c.fov_degrees),
            (None, Some(p)) => (p.yaw, p.pitch, 70.0),
            _ => (0.0, 0.0, 70.0),
        };
        let project = |p: [f64; 3]| -> Option<[f32; 2]> {
            let eye = eye?;
            let (y, x) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
            let forward = [-y.sin() * x.cos(), -x.sin(), y.cos() * x.cos()];
            let right = [-y.cos(), 0.0, -y.sin()];
            let up = [
                right[1] * forward[2] - right[2] * forward[1],
                right[2] * forward[0] - right[0] * forward[2],
                right[0] * forward[1] - right[1] * forward[0],
            ];
            let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            let dot = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
            let z = dot(forward);
            if z < 0.05 {
                return None;
            }
            let t = (f64::from(fov).to_radians() / 2.0).tan();
            let aspect = f64::from(w) / f64::from(h.max(1.0));
            let nx = dot(right) / (z * t * aspect);
            let ny = dot(up) / (z * t);
            Some([((nx + 1.0) / 2.0 * f64::from(w)) as f32, ((1.0 - ny) / 2.0 * f64::from(h)) as f32])
        };
        let mut tip = None;
        if let Some((pos, _, _, _, _)) = &target {
            for s in &shown {
                // The box: half a block square on its face, lifted off it a little.
                let n = s.side.offset();
                let (u, v) = match n {
                    [0, _, 0] => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
                    [_, 0, 0] => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
                    _ => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                };
                let c = [
                    f64::from(pos[0]) + s.at[0] + f64::from(n[0]) * 0.01,
                    f64::from(pos[1]) + s.at[1] + f64::from(n[1]) * 0.01,
                    f64::from(pos[2]) + s.at[2] + f64::from(n[2]) * 0.01,
                ];
                let corner = |a: f64, b: f64| [c[0] + u[0] * a + v[0] * b, c[1] + u[1] * a + v[1] * b, c[2] + u[2] * a + v[2] * b];
                let corners = [corner(-0.25, -0.25), corner(0.25, -0.25), corner(0.25, 0.25), corner(-0.25, 0.25)];
                let points: Option<Vec<[f32; 2]>> = corners.iter().map(|p| project(*p)).collect();
                let Some(points) = points else {
                    continue;
                };
                // Darkened under the value, which Java draws large with a dark outline:
                // here it is the font's size, and would sit on the block's own texture.
                game.native_quad(batch, [points[0], points[1], points[2], points[3]], 0x9000_0000);
                if s.highlight {
                    // Its outline, white.
                    for i in 0..4 {
                        let a = points[i];
                        let b = points[(i + 1) % 4];
                        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                        let len = (dx * dx + dy * dy).sqrt().max(0.001);
                        let (ox, oy) = (-dy / len * 0.75, dx / len * 0.75);
                        game.native_quad(batch, [[a[0] - ox, a[1] - oy], [b[0] - ox, b[1] - oy], [b[0] + ox, b[1] + oy], [a[0] + ox, a[1] + oy]], 0xFFFF_FFFF);
                    }
                }
                let Some(middle) = project(c) else {
                    continue;
                };
                let text = if s.value.options.is_empty() {
                    s.value.text.clone()
                } else {
                    tr(&s.value.text)
                };
                let tw = game.native_text_width(&text);
                game.native_text(batch, &text, [middle[0] - tw / 2.0, middle[1] - 4.0], if s.highlight { 0xFFFFFF } else { 0xDDDDDD }, true);
                if s.highlight {
                    tip = Some(vec![tr(&s.value.label), tr("create.gui.value_settings.hold_to_edit")]);
                }
            }
        }
        // The tip, fading in after the look has rested on the box a moment.
        let since = m::value_client(|c| {
            if tip.is_some() {
                *c.hover_since.get_or_insert(now)
            } else {
                c.hover_since = None;
                now
            }
        });
        if let Some(lines) = tip {
            let alpha = (((now - since - 0.15) / 0.25).clamp(0.0, 1.0) * 255.0) as u32;
            if alpha > 8 {
                let mut y = h - 75.0 - lines.len() as f32 * 12.0;
                for (i, line) in lines.iter().enumerate() {
                    let tw = game.native_text_width(line);
                    let rgb = if i == 0 { 0xFBDC7D } else { 0xFFFFFF };
                    let _ = alpha;
                    game.native_text(batch, line, [w / 2.0 - (tw / 2.0).floor(), y], rgb, true);
                    y += 12.0;
                }
            }
        }
    }
"#;

const RENDER_PART: &str = r#"    // Create's value settings: the board open (`ValueSettingsScreen`).
    if game.native_mod_screen() == Some(m::ID) {
        let now = m::value_client(|c| c.clock);
        let drawn = m::value_client(|c| {
            let open = c.board.as_mut()?;
            let size = [screen.rect[2], screen.rect[3]];
            let width_of = |t: &str| game.native_text_width(t);
            let tr = |key: &str, args: &[&str]| {
                let args: Vec<horizon_client::text::Text> =
                    args.iter().map(|a| horizon_client::text::Text::literal(*a)).collect();
                horizon_client::text::Text::translate(key, &args).plain()
            };
            let layout = m::board_layout(&open.board, size, &|t: &str| width_of(&tr(t, &[])));
            let mouse = [screen.mouse[0] - screen.rect[0], screen.mouse[1] - screen.rect[1]];
            let cursor = open.cursor(&layout, mouse);
            let chosen = m::closest(&layout, &open.board, cursor, false);
            let ticks = ((now - open.opened) * 20.0) as f32;
            let mut ops = Vec::new();
            // `renderBackground`: the world darkened as it opens.
            let a = ((0x50 as f32) * (ticks / 20.0).min(1.0)) as u32;
            ops.push(m::GuiOp::Fill { rect: [0.0, 0.0, size[0], size[1]], argb: (a << 24) | 0x10_1010 });
            m::draw_board(&open.board, &layout, chosen, ticks, &mut ops, &width_of, &tr);
            Some(ops)
        });
        if let Some(ops) = drawn {
            let [x, y] = [screen.rect[0], screen.rect[1]];
            for op in ops {
                match op {
                    m::GuiOp::Fill { rect: [a, b, c, d], argb } => game.native_fill(batch, [x + a, y + b, c, d], argb),
                    m::GuiOp::Text { text, at, rgb, shadow } => {
                        game.native_text(batch, &text, [x + at[0], y + at[1]], rgb, shadow);
                    }
                    m::GuiOp::Sprite { name, src, dst: [a, b, c, d] } => {
                        game.native_sprite(batch, &name, src, [x + a, y + b, c, d]);
                    }
                    m::GuiOp::Item { item, at, size } => {
                        game.native_item(batch, &item, [x + at[0], y + at[1]], size);
                    }
                }
            }
            return;
        }
    }
"#;

const SCROLL_PART: &str = r#"    // Create's value settings: the wheel moves the board's cursor a value (a milestone
    // with shift held), as `ValueSettingsScreen.mouseScrolled` moves the mouse.
    if game.native_mod_screen() == Some(m::ID) {
        let moved = m::value_client(|c| {
            let open = c.board.as_mut()?;
            let size = [screen.rect[2], screen.rect[3]];
            let width_of = |t: &str| game.native_text_width(&horizon_client::text::Text::translate(t, &[]).plain());
            let layout = m::board_layout(&open.board, size, &width_of);
            let mouse = [screen.mouse[0] - screen.rect[0], screen.mouse[1] - screen.rect[1]];
            let cursor = open.cursor(&layout, mouse);
            let now = m::closest(&layout, &open.board, cursor, false);
            let step = if modifiers & 1 != 0 { open.board.milestone_interval } else { 1 };
            let column = (now.value + step * scroll[1].signum() as i32).clamp(0, open.board.max_value);
            let to = m::coordinate_of_value(&layout, &open.board, now.row, column);
            open.scrolled[0] += to[0] - cursor[0];
            Some(())
        });
        if moved.is_some() {
            *canceled = true;
            return;
        }
    }
"#;

/// The helper for a release over the board: the choice is kept for the next frame,
/// which sends it (a screen hook can't).
const RELEASED: &str = r#"// Create's value settings: letting go of the use key on the board picks the value
// under the cursor (`ValueSettingsScreen.mouseReleased`, shift for milestones only).
fn __ID___screen_mouse_button_released_pre(
    game: &crate::game::Game<'_>,
    screen: &horizon_client::mod_api::NativeScreenView<'_>,
    button: i32,
    modifiers: u32,
    canceled: &mut bool,
) {
    use crate::game::__M__ as m;
    if button != 1 || game.native_mod_screen() != Some(m::ID) {
        return;
    }
    let size = [screen.rect[2], screen.rect[3]];
    let width_of = |t: &str| game.native_text_width(&horizon_client::text::Text::translate(t, &[]).plain());
    let sent = m::value_client(|c| {
        let open = c.board.as_mut()?;
        let layout = m::board_layout(&open.board, size, &width_of);
        let mouse = [screen.mouse[0] - screen.rect[0], screen.mouse[1] - screen.rect[1]];
        let cursor = open.cursor(&layout, mouse);
        let chosen = m::closest(&layout, &open.board, cursor, modifiers & 1 != 0);
        let data = m::encode_choice(open.pos, open.index, chosen);
        c.send = Some(data);
        c.use_down = false;
        Some(())
    });
    if sent.is_some() {
        *canceled = true;
    }
}
"#;

/// The helper for the use key: on a value box it starts the hold, and the block's own
/// use doesn't happen (`ValueSettingsInputHandler.onBlockActivated`).
const INTERACTION: &str = r#"// Create's value settings: the use key on a value box holds it there, and opens its
// board when held (`ValueSettingsInputHandler`); the block isn't used.
fn __ID___interaction_key(
    game: &mut crate::game::Game<'_>,
    button: i32,
    key_mapping: &str,
    hand: &horizon_client::first_person::Hand,
    swing_hand: &mut bool,
    canceled: &mut bool,
) {
    use crate::game::__M__ as m;
    let _ = button;
    if *canceled || key_mapping != "key.use" || !game.native_on(m::ID) {
        return;
    }
    __TARGET__
    let Some((pos, entity, me, local, side)) = target else {
        return;
    };
    if crouching {
        return;
    }
    let Some(shown) = m::looked_at(&entity, &me, local, side, wrench).into_iter().find(|s| s.highlight) else {
        return;
    };
    *canceled = true;
    *swing_hand = false;
    // Held already (`cancelIfWarmupAlreadyStarted`), or the off hand's turn: taken, no more.
    if *hand != horizon_client::first_person::Hand::Main || m::value_client(|c| c.hold.is_some()) {
        return;
    }
    let Some(index) = m::value_index(&entity, shown.field) else {
        return;
    };
    m::value_client(|c| {
        c.use_down = true;
        c.hold = Some(m::ValueHold { pos, entity: entity.clone(), index, since: c.clock });
    });
}
"#;

const NET_PART: &str = r#"    // Create's value settings: a choice made on a value board (`ValueSettingsPacket`):
    // the value is set, its callback run, and the clients told.
    if channel == m::VALUE_CHANNEL {
        use horizon_world::nbt::Tag;
        let Some((pos, index, chosen)) = m::decode_choice(data) else {
            return;
        };
        let block = game.native_block(pos);
        let properties: Vec<(String, String)> = m::STATE_KEYS
            .iter()
            .filter_map(|key| Some((key.to_string(), game.native_block_property(pos, key)?)))
            .collect();
        let changed = game.native_with_mod_block_entity(pos, |e| {
            let field = m::value_field(&e.id, index)?;
            let values: Vec<(String, m::Saved)> = e
                .nbt
                .iter()
                .filter_map(|(k, tag)| {
                    let v = match tag {
                        Tag::Byte(v) => m::Saved::Byte(*v),
                        Tag::Short(v) => m::Saved::Short(*v),
                        Tag::Int(v) => m::Saved::Int(*v),
                        Tag::Long(v) => m::Saved::Long(*v),
                        Tag::Float(v) => m::Saved::Float(f32::from_bits(*v)),
                        Tag::Double(v) => m::Saved::Double(f64::from_bits(*v)),
                        Tag::String(v) => m::Saved::Text(v.to_string_lossy()),
                        _ => return None,
                    };
                    Some((k.to_string_lossy(), v))
                })
                .collect();
            let me = m::Neighbour {
                block,
                pos,
                properties,
                entity: Some(e.id.clone()),
                values,
                items: Vec::new(),
            };
            let saved = m::choose_value(&e.id, field, &me, chosen)?;
            for (key, value) in saved {
                let tag = match value {
                    m::Saved::Byte(v) => Tag::Byte(v),
                    m::Saved::Short(v) => Tag::Short(v),
                    m::Saved::Int(v) => Tag::Int(v),
                    m::Saved::Long(v) => Tag::Long(v),
                    m::Saved::Float(v) => Tag::Float(v.to_bits()),
                    m::Saved::Double(v) => Tag::Double(v.to_bits()),
                    m::Saved::Text(v) => Tag::String(v.as_str().into()),
                };
                e.nbt.insert(key.into(), tag);
            }
            Some(())
        });
        if changed.flatten().is_some() {
            game.native_block_entity_sync(pos);
            // `playFeedbackSound`.
            let at = [f64::from(pos[0]) + 0.5, f64::from(pos[1]) + 0.5, f64::from(pos[2]) + 0.5];
            // SoundSource.BLOCKS, to everyone near.
            @@LEVEL_CALL(game; native_level_play_sound; "minecraft:entity.item_frame.add_item", at, 4, 0.25, 2.0)@@
            @@LEVEL_CALL(game; native_level_play_sound; "minecraft:block.note_block.iron_xylophone", at, 4, 0.03, 1.125)@@
        }
        let _ = player;
        return;
    }
"#;

/// A part with the crosshair's target pasted in.
pub fn with_target(part: &str) -> String {
    let mut out = String::new();
    for line in part.split_inclusive('\n') {
        let Some(at) = line.find("__TARGET__") else {
            out.push_str(line);
            continue;
        };
        // Indented as the line it stands on (TARGET is written four spaces in).
        let indent = &line[..at];
        for t in TARGET.lines() {
            if t.is_empty() {
                out.push('\n');
            } else {
                out.push_str(indent);
                out.push_str(t.strip_prefix("    ").unwrap_or(t));
                out.push('\n');
            }
        }
    }
    out
}

/// The whole helper for a hook only the value settings use.
pub fn helper(hook: &str, id: &str, m: &str) -> Option<String> {
    let fill = |s: &str| with_target(s).replace("__ID__", id).replace("__M__", m);
    Some(match hook {
        "interaction_key" => fill(INTERACTION),
        "screen_mouse_button_released_pre" => fill(RELEASED),
        "key" => fill(&format!(
            "// Create's value settings: whether the use key is down.\nfn __ID___key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {{\n    use crate::game::__M__ as m;\n    let _ = game;\n{KEY_PART}}}\n"
        )),
        "frame" => fill(&format!(
            "// Create's value settings, once a frame.\nfn __ID___frame(game: &mut crate::game::Game<'_>, elapsed: std::time::Duration) {{\n    use crate::game::__M__ as m;\n    if !game.native_on(m::ID) {{\n        return;\n    }}\n{FRAME_PART}}}\n"
        )),
        "hud" => fill(&format!(
            "// Create's value settings: the value boxes looked at.\nfn __ID___hud(\n    game: &mut crate::game::Game<'_>,\n    batch: &mut horizon_client::hud::HudBatch,\n    corners: &mut horizon_client::extras_hud::Corners,\n) {{\n    use crate::game::__M__ as m;\n    let _ = corners;\n    if !game.native_on(m::ID) {{\n        return;\n    }}\n{HUD_PART}}}\n"
        )),
        "screen_render_post" => fill(&format!(
            "// Create's value settings: the board.\nfn __ID___screen_render_post(\n    game: &crate::game::Game<'_>,\n    batch: &mut horizon_client::hud::HudBatch,\n    screen: &horizon_client::mod_api::NativeScreenView<'_>,\n) {{\n    use crate::game::__M__ as m;\n{RENDER_PART}}}\n"
        )),
        "screen_mouse_scrolled_pre" => fill(&format!(
            "// Create's value settings: the wheel on the board.\nfn __ID___screen_mouse_scrolled_pre(\n    game: &crate::game::Game<'_>,\n    screen: &horizon_client::mod_api::NativeScreenView<'_>,\n    scroll: [f64; 2],\n    modifiers: u32,\n    canceled: &mut bool,\n) {{\n    use crate::game::__M__ as m;\n{SCROLL_PART}}}\n"
        )),
        "net_from_client" => fill(&format!(
            "// Create's value settings: a choice made on a value board.\nfn __ID___net_from_client(\n    game: &mut crate::game::Game<'_>,\n    player: u128,\n    channel: &str,\n    data: &[u8],\n) {{\n    use crate::game::__M__ as m;\n{NET_PART}}}\n"
        )),
        _ => return None,
    })
}

/// Pastes the value settings' part for `hook` into a helper of the mod's own for it,
/// after its `use crate::game::<module> as m;`.
pub fn into_helper(source: &str, hook: &str, m: &str) -> String {
    let Some(part) = part(hook) else {
        return source.to_string();
    };
    let line = format!("    use crate::game::{m} as m;\n");
    match source.find(&line) {
        Some(at) => {
            let end = at + line.len();
            format!("{}{}{}", &source[..end], with_target(part), &source[end..])
        }
        None => source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_statement_that_makes_a_field() {
        let java = "int max = MAX;\n\t\tgeneratedSpeed = new KineticScrollValueBehaviour(label,\n\t\t\tthis, new MotorValueBox());\n\t\tgeneratedSpeed.between(-max, max);";
        assert_eq!(
            making(java, "generatedSpeed").as_deref(),
            Some("new KineticScrollValueBehaviour(label,\n\t\t\tthis, new MotorValueBox())")
        );
        assert_eq!(making(java, "speed"), None);
        let added = "behaviours.add(angleInput = new ValveHandleScrollValueBehaviour(this).between(-180, 180));";
        assert!(
            making(added, "angleInput")
                .unwrap()
                .starts_with("new ValveHandleScrollValueBehaviour(this)")
        );
    }

    #[test]
    fn reads_which_methods_a_nested_class_declares() {
        let body = "\n\t\t@Override\n\t\tpublic ValueSettingsBoard createBoard(Player player, BlockHitResult hit) {\n\t\t\treturn board;\n\t\t}\n\t\tvoid other() { x.setValueSettings(a, b); }\n";
        assert!(declares(body, "createBoard"));
        assert!(!declares(body, "setValueSettings"));
        assert!(!declares(body, "getValueSettings"));
    }

    #[test]
    fn finds_the_methods_a_making_calls() {
        assert_eq!(
            called_methods(
                "new ScrollOptionBehaviour<>(RotationMode.class, label, this, getMovementModeSlot())"
            ),
            vec!["getMovementModeSlot".to_string()]
        );
        assert!(called_methods("new X(this, other.getSlot())").is_empty());
    }

    #[test]
    fn parts_go_after_the_module_line() {
        let own = "fn x_key(game: &mut Game, key: &str, pressed: bool) {\n    use crate::game::ported_x as m;\n    m::on_key(key);\n}\n";
        let both = into_helper(own, "key", "ported_x");
        assert!(both.contains("as m;\n    // Create's value settings"));
        assert!(both.ends_with("    m::on_key(key);\n}\n"));
        assert!(!helper("frame", "x", "ported_x").unwrap().contains("__"));
        assert!(!helper("hud", "x", "ported_x").unwrap().contains("__"));
    }
}
