//! A mod's own questions, ported once each for every function that asks them:
//!
//! - **to a block**: a method of one of the mod's interfaces that its block classes
//!   answer, asked of a block the code meets (Create's `IRotate.hasShaftTowards`, asked
//!   of every neighbour a shaft looks at);
//! - **to a block entity**: a method its block entity classes answer differently, asked
//!   of another block entity (`SplitShaftBlockEntity.getRotationSpeedModifier`: what a
//!   clutch or a gearshift passes on);
//! - **helpers**: a static function that takes block entities and answers with a value
//!   (`RotationPropagator.getRotationSpeedModifier(from, to)`), which the block
//!   entities' code hands its work to.
//!
//! Each becomes a function of runtime.rs over the block and its state properties, or
//! over a view of the block entity, so the ticks and clicks that ask don't each carry
//! their own copy of every class's answer.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use porter_model::{ClassInfo, ClassKind, ModScan};

use crate::block_entity::BlockEntity;

/// The most questions of each kind a mod gets, the most asked first.
const LIMIT: usize = 24;
const ENTITY_LIMIT: usize = 20;
/// And at most this many more that a shared tick asks of the block entity it runs for.
const OWN_LIMIT: usize = 24;
const HELPER_LIMIT: usize = 12;

/// A body longer than this isn't quoted: the class is read instead.
const BODY_LIMIT: usize = 600;

/// Whom a question is asked of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asks {
    /// A block: `(block, state, ..)`.
    Block,
    /// A block entity: `(be, ..)`.
    Entity,
    /// No one: a static helper that takes block entities.
    Helper,
}

/// One question, with what each class answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub asks: Asks,
    /// The interface, the base class or the helper class, fully qualified.
    pub owner: String,
    /// The Java method: `hasShaftTowards`.
    pub method: String,
    /// Its declaration's parameters as written: `LevelReader world, BlockPos pos,
    /// BlockState state, Direction face`.
    pub java_params: String,
    /// The Rust function: `i_rotate_has_shaft_towards`.
    pub rust: String,
    /// The Rust parameters after the one asked: (name, type).
    pub params: Vec<(String, String)>,
    /// The Rust return type.
    pub returns: &'static str,
    /// The owner's own answer, when it gives one (an interface's `default`, a helper's
    /// whole body when short).
    pub default: Option<String>,
    /// What each class answers.
    pub answers: Vec<Answer>,
    /// How many of the mod's classes ask it.
    pub asked: usize,
    /// For a question to a block: the blocks that are of the interface without a class
    /// of theirs answering (they take the interface's own answer). A block in neither
    /// list isn't of the interface, and Java never asks it.
    pub others: Vec<String>,
}

/// One class's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The class that decides it, fully qualified.
    pub class: String,
    /// The blocks (or block entities) of it, and of classes that inherit the answer.
    pub of: Vec<String>,
    /// The method's body on one line; none when it is too long to quote.
    pub body: Option<String>,
}

/// The mod's questions: to a block, to a block entity, and the helpers that take block
/// entities. `ns` is the namespace of its ids, `entities` its block entities.
pub fn find(
    scan: &ModScan,
    ns: &str,
    entities: &[BlockEntity],
    shared: &[crate::block_entity::SharedTick],
) -> Vec<Question> {
    let class = |name: &str| scan.classes.iter().find(|c| c.name == name);
    let mut sources: HashMap<&str, String> = HashMap::new();
    // How many classes call each method on something (`x.name(`).
    let mut asked: HashMap<String, usize> = HashMap::new();
    for c in scan.classes.iter().filter(|c| !c.is_mixin) {
        let Ok(text) = fs::read_to_string(Path::new(&scan.input).join(&c.path)) else {
            continue;
        };
        for m in called_methods(&text) {
            *asked.entry(m.to_string()).or_default() += 1;
        }
        sources.insert(c.name.as_str(), text);
    }
    let source = |c: &ClassInfo| sources.get(c.name.as_str());

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
    // A class and the mod's classes above it, its own first.
    let chain = |name: &str| -> Vec<&ClassInfo> {
        let mut out = Vec::new();
        let mut next = class(name);
        while let Some(c) = next {
            out.push(c);
            if out.len() >= 10 {
                break;
            }
            next = c.superclass.as_deref().and_then(class);
        }
        out
    };
    // Whether a class (by its chain) is of `interface`: one of its classes implements
    // it, or an interface that extends it (`ICogWheel extends IRotate`).
    let is_a = |chain: &[&ClassInfo], interface: &ClassInfo| -> bool {
        let wanted = simple(&interface.name);
        let mut names: Vec<String> = chain
            .iter()
            .flat_map(|c| c.interfaces.iter())
            .map(|i| simple(i.split('<').next().unwrap_or(i)))
            .collect();
        // What those interfaces extend, a few steps up.
        for _ in 0..3 {
            let more: Vec<String> = names
                .iter()
                .filter_map(|n| scan.classes.iter().find(|c| simple(&c.name) == *n))
                .flat_map(|c| c.interfaces.iter().chain(c.superclass.iter()))
                .map(|i| simple(i.split('<').next().unwrap_or(i)))
                .filter(|n| !names.contains(n))
                .collect();
            if more.is_empty() {
                break;
            }
            names.extend(more);
        }
        names.contains(&wanted)
    };
    let chains: Vec<(&BlockEntity, Vec<&ClassInfo>)> = entities
        .iter()
        .filter_map(|e| Some((e, chain(e.class.as_deref()?))))
        .collect();
    let entity_classes: BTreeSet<String> = chains
        .iter()
        .flat_map(|(_, c)| c.iter().map(|k| simple(&k.name)))
        .collect();

    // What a block is, by simple name: its classes, and the interfaces they are of.
    let mut block_types: BTreeSet<String> = BTreeSet::new();
    let mut block_chains: Vec<Vec<&ClassInfo>> = Vec::new();
    for (_, own) in &blocks {
        if block_types.insert(simple(&own.name)) {
            block_chains.push(chain(&own.name));
        }
    }
    for interface in scan
        .classes
        .iter()
        .filter(|c| c.kind == ClassKind::Interface && !c.is_mixin)
    {
        if block_chains.iter().any(|c| is_a(c, interface)) {
            block_types.insert(simple(&interface.name));
        }
    }
    for c in block_chains.iter().flatten() {
        block_types.insert(simple(&c.name));
    }

    let mut out: Vec<Question> = Vec::new();

    // To a block: an interface's methods its block classes answer.
    let mut to_blocks: Vec<Question> = Vec::new();
    for interface in scan
        .classes
        .iter()
        .filter(|c| c.kind == ClassKind::Interface && !c.is_mixin && !c.method_names.is_empty())
    {
        for method in &interface.method_names {
            if to_blocks.iter().any(|q| q.method == *method) || !asked.contains_key(method) {
                continue;
            }
            // The class that decides it for each block: its own, or the nearest above.
            let mut by_class: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
            for (id, own) in &blocks {
                let decides = chain(&own.name).into_iter().find(|c| {
                    c.kind != ClassKind::Interface && c.method_names.iter().any(|n| n == method)
                });
                if let Some(c) = decides {
                    match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                        Some((_, ids)) => ids.push(id.clone()),
                        None => by_class.push((c, vec![id.clone()])),
                    }
                }
            }
            if by_class.is_empty() {
                continue;
            }
            // The blocks of the interface none of those classes covers.
            let others: Vec<String> = blocks
                .iter()
                .filter(|(id, _)| !by_class.iter().any(|(_, ids)| ids.contains(id)))
                .filter(|(_, own)| is_a(&chain(&own.name), interface))
                .map(|(id, _)| id.clone())
                .collect();
            let Some(text) = source(interface) else {
                continue;
            };
            let Some((decl, params, returns)) =
                declarations(text, method).into_iter().find_map(|d| {
                    signature(&d, Asks::Block, &entity_classes, &block_types)
                        .map(|(p, r)| (d, p, r))
                })
            else {
                continue;
            };
            to_blocks.push(Question {
                asks: Asks::Block,
                owner: interface.name.clone(),
                method: method.clone(),
                java_params: decl.params_text.clone(),
                rust: format!("{}_{}", snake(&simple(&interface.name)), snake(method)),
                params,
                returns,
                default: decl.body.filter(|b| b.len() <= BODY_LIMIT),
                answers: answers(by_class, method, &source),
                asked: asked[method],
                others,
            });
        }
    }
    to_blocks.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.rust.cmp(&b.rust)));
    to_blocks.truncate(LIMIT);
    out.extend(to_blocks);

    // To a block entity: a method the block entities' classes answer differently
    // (two of them define it), asked of something.
    let mut defined: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for (_, classes) in &chains {
        for c in classes {
            for m in &c.method_names {
                defined
                    .entry(m.as_str())
                    .or_default()
                    .insert(c.name.as_str());
            }
        }
    }
    // Asked by the block entities' own classes, or by the helpers those call into:
    // elsewhere a name like `getState` is somebody else's.
    let mut called_into: BTreeSet<String> = BTreeSet::new();
    let mut core: HashMap<&str, usize> = HashMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (_, classes) in &chains {
        for c in classes {
            if seen.insert(c.name.as_str())
                && let Some(text) = source(c)
            {
                called_into.extend(static_receivers(text));
                for m in called_methods(text) {
                    *core.entry(m).or_default() += 1;
                }
            }
        }
    }
    for c in scan
        .classes
        .iter()
        .filter(|c| !c.is_mixin && called_into.contains(&simple(&c.name)))
    {
        if seen.insert(c.name.as_str())
            && let Some(text) = source(c)
        {
            for m in called_methods(text) {
                *core.entry(m).or_default() += 1;
            }
        }
    }
    // Asked by a tick several block entities share, of the one it runs for: a method
    // of its classes that a class below overrides, called there without a receiver
    // (`getGeneratedSpeed()` in `updateGeneratedRotation`). The shared tick is ported
    // once and can't know the answer; the block entity's own class has it.
    let mut of_itself: BTreeSet<&str> = BTreeSet::new();
    // (What is asked of a block entity the code meets, before these are counted.)
    let met: BTreeSet<&str> = core.keys().copied().collect();
    for t in shared {
        let classes: Vec<&ClassInfo> = std::iter::once(&t.class)
            .chain(&t.above)
            .filter_map(|n| class(n))
            .collect();
        for m in &t.asks {
            let calls: usize = classes
                .iter()
                .filter_map(|c| source(c))
                .map(|text| bare_calls(text, m).saturating_sub(declarations(text, m).len()))
                .sum();
            if calls > 0 {
                *core.entry(m.as_str()).or_default() += calls;
                if !met.contains(m.as_str()) {
                    of_itself.insert(m.as_str());
                }
            }
        }
    }
    // A name several other classes of the mod define too is somebody else's as well
    // (`getState`, `isValid`); one or two may be a helper of the same name.
    let elsewhere = |method: &str| {
        scan.classes
            .iter()
            .filter(|c| {
                !entity_classes.contains(&simple(&c.name))
                    && !called_into.contains(&simple(&c.name))
                    && c.method_names.iter().any(|n| n == method)
            })
            .count()
    };
    let mut to_entities: Vec<Question> = Vec::new();
    for (method, classes) in &defined {
        if classes.len() < 2
            || !core.contains_key(*method)
            || out.iter().any(|q| q.method == *method)
            || elsewhere(method) > 2
        {
            continue;
        }
        // The class each block entity's answer comes from: its own, or the nearest above.
        let mut by_class: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
        for (e, classes) in &chains {
            let decides = classes
                .iter()
                .find(|c| c.method_names.iter().any(|n| n == method));
            if let Some(c) = decides {
                match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                    Some((_, ids)) => ids.push(e.id.clone()),
                    None => by_class.push((c, vec![e.id.clone()])),
                }
            }
        }
        // Declared by the one highest up that has it.
        let Some(top) = chains.iter().find_map(|(_, classes)| {
            classes
                .iter()
                .rev()
                .find(|c| c.method_names.iter().any(|n| n == method))
        }) else {
            continue;
        };
        let Some(text) = source(top) else {
            continue;
        };
        let Some((decl, params, returns)) = declarations(text, method).into_iter().find_map(|d| {
            signature(&d, Asks::Entity, &entity_classes, &block_types).map(|(p, r)| (d, p, r))
        }) else {
            continue;
        };
        to_entities.push(Question {
            asks: Asks::Entity,
            owner: top.name.clone(),
            method: method.to_string(),
            java_params: decl.params_text.clone(),
            rust: format!("{}_{}", snake(&simple(&top.name)), snake(method)),
            params,
            returns,
            default: None,
            answers: answers(by_class, method, &source),
            asked: core[*method],
            others: Vec::new(),
        });
    }
    // The most answered first: what many classes override is what gets asked. Those a
    // shared tick asks of its own block entity are counted apart, so they take the
    // place of none that code asks of a block entity it meets.
    to_entities.sort_by(|a, b| {
        b.answers
            .len()
            .cmp(&a.answers.len())
            .then(b.asked.cmp(&a.asked))
            .then(a.rust.cmp(&b.rust))
    });
    let (mut own, mut to_entities): (Vec<Question>, Vec<Question>) = to_entities
        .into_iter()
        .partition(|q| of_itself.contains(q.method.as_str()));
    to_entities.truncate(ENTITY_LIMIT);
    own.truncate(OWN_LIMIT);
    out.extend(to_entities);
    out.extend(own);

    // Helpers: static functions that take block entities, of classes the block
    // entities' own classes call into.
    let mut helpers: Vec<Question> = Vec::new();
    for c in scan.classes.iter().filter(|c| {
        !c.is_mixin
            && c.kind != ClassKind::Interface
            && called_into.contains(&simple(&c.name))
            && !entity_classes.contains(&simple(&c.name))
    }) {
        let Some(text) = source(c) else {
            continue;
        };
        let mut names: Vec<&String> = c.method_names.iter().collect();
        names.sort();
        names.dedup();
        for method in names {
            let Some((decl, params, returns)) =
                declarations(text, method).into_iter().find_map(|d| {
                    signature(&d, Asks::Helper, &entity_classes, &block_types)
                        .map(|(p, r)| (d, p, r))
                })
            else {
                continue;
            };
            helpers.push(Question {
                asks: Asks::Helper,
                owner: c.name.clone(),
                method: method.clone(),
                java_params: decl.params_text.clone(),
                rust: format!("{}_{}", snake(&simple(&c.name)), snake(method)),
                params,
                returns,
                default: decl.body.filter(|b| b.len() <= BODY_LIMIT),
                answers: Vec::new(),
                // Asked inside its own class too, where it is called bare.
                asked: text.matches(&format!("{method}(")).count(),
                others: Vec::new(),
            });
        }
    }
    helpers.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.rust.cmp(&b.rust)));
    helpers.truncate(HELPER_LIMIT);
    out.extend(helpers);
    out
}

/// The answers of `by_class`, each with its body where that is short enough to quote.
fn answers<'a>(
    by_class: Vec<(&ClassInfo, Vec<String>)>,
    method: &str,
    source: &impl Fn(&ClassInfo) -> Option<&'a String>,
) -> Vec<Answer> {
    by_class
        .into_iter()
        .map(|(c, of)| Answer {
            class: c.name.clone(),
            of,
            body: source(c)
                .and_then(|s| method_body(s, method))
                .filter(|b| b.len() <= BODY_LIMIT),
        })
        .collect()
}

/// What a view of a block entity is, for the comments of what takes one.
const VIEW: &str = "its block's id (`be.block`), its own id as a block entity (`be.entity`: the ids listed with\n/// each class below are these, and tell the classes apart; a block's id is often another), that\n/// block's state properties (`be.state(\"powered\")` for `getBlockState().getValue(POWERED)`), where\n/// it is (`be.pos`), and its saved values by the key its state struct saves each under\n/// (`be.get(\"Speed\")`)";

/// runtime.rs's part: a function to port for each question.
pub fn runtime(questions: &[Question]) -> String {
    let mut r = String::new();
    for q in questions {
        let owner = simple(&q.owner);
        match q.asks {
            Asks::Block => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a question the mod's code asks a block it meets,\n/// answered here once for every block of the mod. `block` is the block asked\n/// (`{example}`) and `state` its state properties: `state.getValue(FACING)` is the value\n/// under `facing` (a `Direction` as `north`: `Side::from_name` makes it a `Side`),\n/// `getValue(AXIS)` the one under `axis` (`x`, `y` or `z`, as an `Axis` is here). The level\n/// isn't given.\n///\n/// What each block class answers, with its blocks and those of classes that inherit it:\n",
                    method = q.method,
                    java = q.java_params,
                    example = example(q),
                );
            }
            Asks::Entity => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a question the mod's code asks a block entity it meets,\n/// answered here once for every block entity of the mod. `be` is a view of the one asked\n/// (`{example}`): {VIEW}. A neighbour is such a view already\n/// (`machine.neighbour(side)`), and `machine.me(pos, &state.save())` makes one of the block\n/// entity a tick is for. Its fields are those saved values; a method of its own that the\n/// Java calls (`hasSource()`, `getSpeed()`) is worked out from them as its class does.\n///\n/// What each block entity class answers, with its block entities and those that inherit it:\n",
                    method = q.method,
                    java = q.java_params,
                    example = example(q),
                );
            }
            Asks::Helper => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a helper of the mod's code that takes block entities,\n/// ported here once for every function that calls it. A block entity it takes is a view:\n/// {VIEW}. A neighbour is such a view already (`machine.neighbour(side)`), and\n/// `machine.me(pos, &state.save())` makes one of the block entity a tick is for. What the\n/// Java reads of them from the level (`getBlockState()`, `getBlockPos()`) is in the views;\n/// the level isn't given. `instanceof` a class is told by the view's block.\n",
                    method = q.method,
                    java = q.java_params,
                );
            }
        }
        let mut long: Vec<&str> = Vec::new();
        for a in &q.answers {
            let mut ids =
                a.of.iter()
                    .take(12)
                    .map(|b| format!("`{b}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
            if a.of.len() > 12 {
                let _ = write!(ids, " and {} more", a.of.len() - 12);
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
        match q.asks {
            Asks::Block => {
                let default = match &q.default {
                    Some(body) => format!("`{}`", one_line(body)),
                    None => "it gives none, so the plainest answer".to_string(),
                };
                let interface = simple(&q.owner);
                if q.others.is_empty() {
                    let _ = writeln!(
                        r,
                        "/// No other block is a `{interface}` (what the interface itself answers is {default})."
                    );
                } else {
                    let ids: Vec<String> =
                        q.others.iter().take(40).map(|b| format!("`{b}`")).collect();
                    let _ = writeln!(
                        r,
                        "/// These are `{interface}`s too, and answer as the interface does ({default}): {}{}.",
                        ids.join(", "),
                        if q.others.len() > 40 { " and more" } else { "" }
                    );
                }
                let _ = writeln!(
                    r,
                    "/// A block that isn't a `{interface}` is never asked in Java (`block instanceof {interface} && ..`):\n/// for it the answer is the plainest one (`false`, zero, none), whatever the interface's own is."
                );
            }
            Asks::Entity => {
                let _ = writeln!(
                    r,
                    "/// A block entity none of them covers gives the plainest answer."
                );
            }
            Asks::Helper => {
                if let Some(body) = &q.default {
                    let _ = writeln!(r, "///\n/// In Java: `{}`", one_line(body));
                }
            }
        }
        // The classes whose answer wasn't quoted are read with the owner.
        let mut from = format!("`{}`", q.owner);
        for class in long.iter().take(4) {
            let _ = write!(from, ", `{class}`");
        }
        let (given, names) = match q.asks {
            Asks::Block => ("block: &str, state: &[(String, String)]", "block, state"),
            Asks::Entity => ("be: &Neighbour", "be"),
            Asks::Helper => ("", ""),
        };
        let mut params: Vec<String> = Vec::new();
        let mut used: Vec<String> = Vec::new();
        if !given.is_empty() {
            params.push(given.to_string());
            used.push(names.to_string());
        }
        for (n, t) in &q.params {
            params.push(format!("{n}: {t}"));
            used.push(n.clone());
        }
        let todo = match q.asks {
            Asks::Helper => format!("TODO: port `{}` of {from}.", q.method),
            _ => format!("TODO: port from {from}."),
        };
        let _ = write!(
            r,
            "///\n/// {todo}\npub fn {rust}({params}) -> {returns} {{\n    let _ = {used};\n    {value}\n}}\n",
            rust = q.rust,
            params = params.join(", "),
            returns = q.returns,
            // (`block, state` is two names in one entry: always a tuple unless it is one.)
            used = match &used[..] {
                [one] if !one.contains(',') => one.clone(),
                many => format!("({})", many.join(", ")),
            },
            value = default_value(q.returns),
        );
    }
    r
}

fn example(q: &Question) -> &str {
    q.answers
        .first()
        .and_then(|a| a.of.first())
        .map_or("", String::as_str)
}

/// The lines that tell a function's translation which of its Java's calls are ported
/// already: for each question of `module` (a runtime.rs) that `java` asks. Nothing when
/// it asks none; a question isn't told of itself.
pub fn ported(module: &str, java: &str, function: &str) -> String {
    let mut out = String::new();
    for q in in_module(module) {
        if q.rust == function {
            continue;
        }
        let Ported {
            asks,
            owner,
            method,
            rust,
            signature,
        } = &q;
        match asks {
            Asks::Block if java.contains(&format!(".{method}(")) => {
                let _ = writeln!(
                    out,
                    "- `x.{method}(..)` asked of a block is `{rust}`, already ported for every block of the mod: `{signature}`. Pass the block asked and its state properties (a neighbour's `n.block` and `&n.properties`, this block's `machine.block` and `&machine.properties`)."
                );
            }
            Asks::Entity if java.contains(&format!(".{method}(")) => {
                let _ = writeln!(
                    out,
                    "- `x.{method}(..)` asked of a block entity is `{rust}`, already ported for every block entity of the mod: `{signature}`. Pass a view of the one asked: a neighbour is one (`machine.neighbour(side)`), and `machine.me(pos, &state.save())` is this one."
                );
            }
            Asks::Helper
                if java.contains(&format!("{method}(")) && java.contains(owner.as_str()) =>
            {
                let _ = writeln!(
                    out,
                    "- `{owner}.{method}(..)` is `{rust}`, already ported: `{signature}`. A block entity it takes is a view: a neighbour is one (`machine.neighbour(side)`), and `machine.me(pos, &state.save())` is this one."
                );
            }
            _ => {}
        }
    }
    out
}

/// Whether `function` is one of `module`'s questions.
pub fn is_question(module: &str, function: &str) -> bool {
    in_module(module).iter().any(|q| q.rust == function)
}

/// The Java methods `module` has a question function for.
pub fn methods(module: &str) -> Vec<String> {
    in_module(module).into_iter().map(|q| q.method).collect()
}

/// A question as a runtime.rs carries it.
struct Ported {
    asks: Asks,
    /// The owner's simple name.
    owner: String,
    method: String,
    rust: String,
    /// Its `pub fn` line.
    signature: String,
}

/// The questions a runtime.rs carries.
fn in_module(module: &str) -> Vec<Ported> {
    let mut out = Vec::new();
    let mut head: Option<(Asks, String, String)> = None;
    for line in module.lines() {
        if let Some(rest) = line.strip_prefix("/// `") {
            // "/// `IRotate.hasShaftTowards(..)`: a question the mod's code asks ..."
            let asks = if line.contains("a question the mod's code asks a block entity it meets") {
                Some(Asks::Entity)
            } else if line.contains("a question the mod's code asks a block it meets") {
                Some(Asks::Block)
            } else if line.contains("a helper of the mod's code that takes block entities") {
                Some(Asks::Helper)
            } else {
                None
            };
            if let Some(asks) = asks {
                let name = rest.split('(').next().unwrap_or_default();
                if let Some((owner, method)) = name.rsplit_once('.') {
                    head = Some((asks, owner.to_string(), method.to_string()));
                }
            }
        } else if let Some(rest) = line.strip_prefix("pub fn ") {
            if let Some((asks, owner, method)) = head.take() {
                out.push(Ported {
                    asks,
                    owner,
                    method,
                    rust: rest.split('(').next().unwrap_or_default().to_string(),
                    signature: line.trim_end_matches('{').trim().to_string(),
                });
            }
        } else if !line.starts_with("///") {
            head = None;
        }
    }
    out
}

/// A method as its class declares it.
struct Declaration {
    returns: String,
    /// (type, name), as written.
    params: Vec<(String, String)>,
    params_text: String,
    is_static: bool,
    /// Its body, on one line.
    body: Option<String>,
}

/// Every way `source` declares a method named `name`.
fn declarations(source: &str, name: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = source[from..].find(&format!(" {name}(")).map(|i| from + i) {
        from = at + 1;
        // What the statement says before the name: modifiers and the return type.
        let start = source[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
        // Without the comment above it: a Javadoc block, or lines of `//`.
        let said = &source[start..at];
        let said = said.rfind("*/").map_or(said, |i| &said[i + 2..]);
        let mut head: Vec<&str> = said
            .lines()
            .map(|l| l.split("//").next().unwrap_or(l))
            .flat_map(str::split_whitespace)
            .filter(|w| !w.starts_with('@'))
            .collect();
        let is_static = head.contains(&"static");
        head.retain(|w| {
            !matches!(
                *w,
                "public"
                    | "default"
                    | "static"
                    | "abstract"
                    | "final"
                    | "private"
                    | "protected"
                    | "synchronized"
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
        let Some(inside) = balanced(&source[open..]) else {
            continue;
        };
        let after = source[open + inside.len() + 2..].trim_start();
        if !after.starts_with('{') && !after.starts_with(';') && !after.starts_with("throws") {
            continue;
        }
        let body = after
            .starts_with('{')
            .then(|| method_body(&source[at..], name))
            .flatten();
        let listed = split_top(inside);
        let params: Vec<(String, String)> = listed
            .iter()
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
            .collect();
        if params.len() != listed.len() {
            continue;
        }
        out.push(Declaration {
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
    out
}

/// The Rust parameters (after the one asked) and return type for a declaration, when
/// every type of it has one here: a question with an answer, not a command.
/// `entity_classes` are the block entities' classes, by simple name.
fn signature(
    decl: &Declaration,
    asks: Asks,
    entity_classes: &BTreeSet<String>,
    block_types: &BTreeSet<String>,
) -> Option<(Vec<(String, String)>, &'static str)> {
    if decl.is_static != (asks == Asks::Helper) {
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
        // Places: which blocks a cogwheel meshes with (`List<BlockPos>`).
        _ if places(&decl.returns) => "Vec<[i32; 3]>",
        _ => return None,
    };
    // A block's state hands over its block too (`state_block`): the block itself beside
    // it (`IRotate block, BlockState state`) says nothing more.
    let has_state = decl.params.iter().any(|(ty, _)| plain(ty) == "BlockState");
    let mut params: Vec<(String, String)> = Vec::new();
    let mut asked_block = false;
    let mut views = 0;
    for (ty, name) in &decl.params {
        let name = snake(name);
        let name = if matches!(
            name.as_str(),
            "block" | "state" | "be" | "type" | "match" | "ref" | "in" | "box" | "move" | "loop"
        ) {
            format!("{name}_")
        } else {
            name
        };
        let written = ty;
        let ty = plain(ty);
        let rust = match ty {
            // The block asked: `block` and `state`.
            "BlockState" if asks == Asks::Block && !asked_block => {
                asked_block = true;
                continue;
            }
            // Another block: its id and its state properties.
            "BlockState" => {
                params.push((format!("{name}_block"), "&str".to_string()));
                "&[(String, String)]"
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
            _ if places(written) => "&[[i32; 3]]",
            _ if asks != Asks::Block && has_state && block_types.contains(ty) => continue,
            // A block entity of the mod's: a view of it.
            _ if asks != Asks::Block && entity_classes.contains(ty) => {
                views += 1;
                "&Neighbour"
            }
            _ => return None,
        };
        params.push((name, rust.to_string()));
    }
    // A helper is offered for the block entities it takes.
    if asks == Asks::Helper && views == 0 {
        return None;
    }
    Some((params, returns))
}

/// How many times `text` has `method(` with nothing before it: a call on the object
/// itself, or the method's own declaration.
fn bare_calls(text: &str, method: &str) -> usize {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(&format!("{method}("))
        .filter(|(at, _)| {
            !text[..*at]
                .chars()
                .next_back()
                .is_some_and(|c| ident(c) || c == '.')
        })
        .count()
}

/// Whether a type is a list of places: `List<BlockPos>`, `Collection<BlockPos>`.
fn places(ty: &str) -> bool {
    let ty: String = ty.split_whitespace().collect();
    ["List", "Collection", "Set", "ArrayList"]
        .iter()
        .any(|c| ty == format!("{c}<BlockPos>"))
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
        "Vec<[i32; 3]>" => "Vec::new()",
        _ => "\"y\"",
    }
}

/// The methods `java` calls on something: each `name` of an `x.name(`.
pub fn called_methods(java: &str) -> HashSet<&str> {
    let mut out = HashSet::new();
    let bytes = java.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && ident(bytes[end]) {
                end += 1;
            }
            if end > start && bytes.get(end) == Some(&b'(') {
                out.insert(&java[start..end]);
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// The classes `java` calls a static function of: each `Name` of a `Name.function(`.
fn static_receivers(java: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    for (dot, _) in java.match_indices('.') {
        let after = &java[dot + 1..];
        let name_len = after.find(|c: char| !ident(c)).unwrap_or(after.len());
        if name_len == 0
            || !after.starts_with(|c: char| c.is_ascii_lowercase())
            || !after[name_len..].starts_with('(')
        {
            continue;
        }
        let before = &java[..dot];
        let start = before.rfind(|c: char| !ident(c)).map_or(0, |i| i + 1);
        let receiver = &before[start..];
        // A class by its capital, and not a constant (`AXIS.x(`).
        if receiver.starts_with(|c: char| c.is_ascii_uppercase())
            && receiver.chars().any(|c| c.is_ascii_lowercase())
        {
            out.insert(receiver.to_string());
        }
    }
    out
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

    fn first(source: &str, name: &str) -> Declaration {
        declarations(source, name)
            .into_iter()
            .next()
            .expect("declared")
    }

    fn entities() -> BTreeSet<String> {
        [
            "KineticBlockEntity".to_string(),
            "ClutchBlockEntity".to_string(),
        ]
        .into()
    }

    #[test]
    fn reads_a_declaration_and_gives_it_a_rust_shape() {
        let d = first(INTERFACE, "hasShaftTowards");
        assert_eq!(d.returns, "boolean");
        assert_eq!(
            d.params_text,
            "LevelReader world, BlockPos pos, BlockState state, Direction face"
        );
        let (params, returns) =
            signature(&d, Asks::Block, &entities(), &BTreeSet::new()).expect("a question");
        assert_eq!(returns, "bool");
        assert_eq!(
            params,
            [
                ("pos".to_string(), "[i32; 3]".to_string()),
                ("face".to_string(), "Side".to_string())
            ]
        );
        // An axis comes back as its letter.
        let d = first(INTERFACE, "getRotationAxis");
        assert_eq!(
            signature(&d, Asks::Block, &entities(), &BTreeSet::new()),
            Some((Vec::new(), "&'static str"))
        );
        // A default answer is kept; a type with no shape here is no question.
        let d = first(INTERFACE, "hideStressImpact");
        assert_eq!(d.body.as_deref(), Some("return false;"));
        let d = first(INTERFACE, "getMinimumRequiredSpeedLevel");
        assert_eq!(
            signature(&d, Asks::Block, &entities(), &BTreeSet::new()),
            None
        );
        // Nor is a static helper a question to a block.
        let d = first(INTERFACE, "isIt");
        assert_eq!(
            signature(&d, Asks::Block, &entities(), &BTreeSet::new()),
            None
        );
        // A static helper named like the question is passed over for the question.
        let cog = "interface ICogWheel {\n\tstatic boolean isLargeCog(BlockState state) {\n\t\treturn state.getBlock() instanceof ICogWheel c && c.isLargeCog();\n\t}\n\n\tdefault boolean isLargeCog() {\n\t\treturn false;\n\t}\n}";
        let found = declarations(cog, "isLargeCog")
            .into_iter()
            .find_map(|d| signature(&d, Asks::Block, &entities(), &BTreeSet::new()).map(|s| (d, s)))
            .expect("the question");
        assert!(!found.0.is_static);
        assert_eq!(found.1, (Vec::new(), "bool"));
        assert_eq!(found.0.body.as_deref(), Some("return false;"));
    }

    #[test]
    fn a_question_can_take_and_answer_with_places() {
        // Which blocks a cogwheel meshes with: asked of a block entity, answered by
        // each class. The block beside its state says nothing the state doesn't.
        let base = "public class KineticBlockEntity extends SmartBlockEntity {
	public List<BlockPos> addPropagationLocations(IRotate block, BlockState state, List<BlockPos> neighbours) {
		return neighbours;
	}
}";
        let d = first(base, "addPropagationLocations");
        let block_types: BTreeSet<String> = ["IRotate".to_string()].into();
        assert_eq!(
            signature(&d, Asks::Entity, &entities(), &block_types),
            Some((
                vec![
                    ("state__block".to_string(), "&str".to_string()),
                    ("state_".to_string(), "&[(String, String)]".to_string()),
                    ("neighbours".to_string(), "&[[i32; 3]]".to_string()),
                ],
                "Vec<[i32; 3]>"
            ))
        );
        assert_eq!(default_value("Vec<[i32; 3]>"), "Vec::new()");
        // A type of the mod's that isn't a block's stays out of reach.
        assert_eq!(
            signature(&d, Asks::Entity, &entities(), &BTreeSet::new()),
            None
        );
        assert!(places("List<BlockPos>") && places("Collection< BlockPos >"));
        assert!(!places("List<ItemStack>") && !places("BlockPos"));
    }

    #[test]
    fn a_block_entitys_method_and_a_helper_take_views() {
        let base = "public abstract class SplitShaftBlockEntity extends KineticBlockEntity {\n\tpublic abstract float getRotationSpeedModifier(Direction face);\n}";
        let d = first(base, "getRotationSpeedModifier");
        assert_eq!(
            signature(&d, Asks::Entity, &entities(), &BTreeSet::new()),
            Some((vec![("face".to_string(), "Side".to_string())], "f32"))
        );
        let helper = "public class RotationPropagator {\n\tprivate static float getRotationSpeedModifier(KineticBlockEntity from, KineticBlockEntity to) {\n\t\treturn 1;\n\t}\n\tpublic static void handleAdded(Level level, BlockPos pos, KineticBlockEntity be) {\n\t}\n\tstatic boolean isLargeToLargeGear(BlockState from, BlockState to, BlockPos diff) {\n\t\treturn false;\n\t}\n}";
        let d = first(helper, "getRotationSpeedModifier");
        assert_eq!(
            signature(&d, Asks::Helper, &entities(), &BTreeSet::new()),
            Some((
                vec![
                    ("from".to_string(), "&Neighbour".to_string()),
                    ("to".to_string(), "&Neighbour".to_string())
                ],
                "f32"
            ))
        );
        // A comment above it is not part of what it says.
        let documented = "class P {
	int x;

	/**
	 * Determines the change in rotation between two attached kinetics.
	 */
	private static float modifier(KineticBlockEntity from, KineticBlockEntity to) {
		return 1;
	}
}";
        assert_eq!(first(documented, "modifier").returns, "float");
        // A command isn't offered, nor a helper that takes no block entity.
        assert!(
            declarations(helper, "handleAdded")
                .iter()
                .all(|d| signature(d, Asks::Helper, &entities(), &BTreeSet::new()).is_none())
        );
        let d = first(helper, "isLargeToLargeGear");
        assert_eq!(
            signature(&d, Asks::Helper, &entities(), &BTreeSet::new()),
            None
        );
        // Called on the class: a helper the block entities call into.
        let calls = static_receivers(
            "RotationPropagator.handleAdded(level, pos, this); AXIS.get(); x.y();",
        );
        assert!(
            calls.contains("RotationPropagator") && calls.len() == 1,
            "{calls:?}"
        );
    }

    fn question(asks: Asks, owner: &str, method: &str, rust: &str) -> Question {
        Question {
            asks,
            owner: owner.into(),
            method: method.into(),
            java_params: "Direction face".into(),
            rust: rust.into(),
            params: vec![("face".into(), "Side".into())],
            returns: "bool",
            default: None,
            answers: vec![Answer {
                class: "com.spin.MotorBlock".into(),
                of: vec!["spin:motor".into()],
                body: Some("return face == state.getValue(FACING);".into()),
            }],
            asked: 3,
            others: vec!["spin:crank".into()],
        }
    }

    #[test]
    fn writes_a_function_to_port_and_finds_it_again() {
        let text = runtime(&[
            question(
                Asks::Block,
                "com.spin.IRotate",
                "hasShaftTowards",
                "i_rotate_has_shaft_towards",
            ),
            question(
                Asks::Entity,
                "com.spin.SplitShaft",
                "passes",
                "split_shaft_passes",
            ),
            Question {
                answers: Vec::new(),
                params: vec![("from".into(), "&Neighbour".into())],
                ..question(
                    Asks::Helper,
                    "com.spin.Propagator",
                    "modifier",
                    "propagator_modifier",
                )
            },
        ]);
        assert!(
            text.contains(
                "- `MotorBlock` (`spin:motor`): `return face == state.getValue(FACING);`"
            ),
            "{text}"
        );
        // The blocks of the interface no class answers for, and what a block outside it
        // answers: Java asks only blocks that are of the interface.
        assert!(
            text.contains(
                "/// These are `IRotate`s too, and answer as the interface does (it gives none, so the plainest answer): `spin:crank`."
            ),
            "{text}"
        );
        assert!(
            text.contains("/// A block that isn't a `IRotate` is never asked in Java"),
            "{text}"
        );
        assert!(
            text.contains("TODO: port from `com.spin.IRotate`."),
            "{text}"
        );
        assert!(
            text.contains("TODO: port `modifier` of `com.spin.Propagator`."),
            "{text}"
        );
        assert!(
            text.contains(
                "pub fn i_rotate_has_shaft_towards(block: &str, state: &[(String, String)], face: Side) -> bool {"
            ),
            "{text}"
        );
        assert!(
            text.contains("pub fn split_shaft_passes(be: &Neighbour, face: Side) -> bool {"),
            "{text}"
        );
        assert!(
            text.contains("pub fn propagator_modifier(from: &Neighbour) -> bool {"),
            "{text}"
        );
        assert_eq!(methods(&text), ["hasShaftTowards", "passes", "modifier"]);
        // A question with nothing more to ask than the block: its stub is still Rust.
        let bare = runtime(&[Question {
            params: Vec::new(),
            java_params: String::new(),
            ..question(Asks::Block, "com.spin.ICog", "isLarge", "i_cog_is_large")
        }]);
        assert!(
            bare.contains(
                "pub fn i_cog_is_large(block: &str, state: &[(String, String)]) -> bool {
    let _ = (block, state);
    false
}"
            ),
            "{bare}"
        );
        assert!(is_question(&text, "split_shaft_passes"));
        // A function whose Java asks is told; a question isn't told of itself.
        let told = ported(
            &text,
            "if (def.hasShaftTowards(face) && be.passes(face))",
            "tick_shaft",
        );
        assert!(
            told.contains("`i_rotate_has_shaft_towards`") && told.contains("`split_shaft_passes`"),
            "{told}"
        );
        assert!(!told.contains("propagator_modifier"), "{told}");
        let told = ported(&text, "Propagator.modifier(this)", "tick_shaft");
        assert!(told.contains("`propagator_modifier`"), "{told}");
        assert!(ported(&text, "x.hasShaftTowards(", "i_rotate_has_shaft_towards").is_empty());
        assert!(ported(&text, "nothing asked", "tick_shaft").is_empty());
    }

    #[test]
    fn lists_the_methods_called_on_something() {
        let asked = called_methods("if (def.hasShaftTowards(w, p, s, f) && own()) x.y.getAxis();");
        assert!(asked.contains("hasShaftTowards") && asked.contains("getAxis"));
        assert!(!asked.contains("own") && !asked.contains("y"));
    }

    #[test]
    fn reads_a_methods_body_on_one_line() {
        let source = "class Motor {\n    @Override\n    public boolean hasShaftTowards(LevelReader world, BlockPos pos, BlockState state, Direction face) {\n        return face == state.getValue(FACING);\n    }\n    void other() { hasShaftTowards(null, null, null, null); }\n}";
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
