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
/// At most this many helpers that work on the level.
const WORLD_LIMIT: usize = 32;
/// At most this many methods asked of the behaviours of other block entities.
const BEHAVIOUR_LIMIT: usize = 20;

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
    /// No one: a static helper that works on the level at a place (`BeltBlock.initBelt(level,
    /// pos)`), called from a block entity's code: given `machine`.
    World,
    /// The behaviour of another block entity, the one the code finds at a place
    /// (`BlockEntityBehaviour.get(level, pos, DirectBeltInputBehaviour.TYPE)`): `(target,
    /// .., machine)`.
    Behaviour,
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
    /// The mod's methods that body calls on another class (`BeltBlockEntity::hasPulley`),
    /// each with its own body where that is short: what the answer is worked out from.
    pub refs: Vec<(String, String)>,
    /// The state structs of its block entities (`Belt`), where a behaviour's answer reads
    /// and writes them.
    pub states: Vec<String>,
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
                answers: answers(by_class, method, &source, &scan.classes),
                asked: asked[method],
                others,
            });
        }
    }
    // And a method a block class of the mod's introduces (not an override of the game's)
    // that a class below it overrides, asked of something: a chute's direction
    // (`AbstractChuteBlock.getFacing`: a chute's `facing`, a smart chute's `DOWN`), which
    // code holding only the state asks through a cast or a static helper.
    let mut bases: Vec<&ClassInfo> = Vec::new();
    for c in block_chains.iter().flat_map(|chain| chain.iter().skip(1)) {
        if c.kind != ClassKind::Interface && !bases.iter().any(|b| b.name == c.name) {
            bases.push(c);
        }
    }
    for base in bases {
        let Some(text) = source(base) else {
            continue;
        };
        for method in &base.method_names {
            if to_blocks.iter().any(|q| q.method == *method) || !asked.contains_key(method) {
                continue;
            }
            // Its own: no `@Override` just before it.
            let introduced = text.match_indices(&format!(" {method}(")).any(|(at, _)| {
                let before = &text[at.saturating_sub(160)..at];
                let stmt = &before[before.rfind([';', '}', '{']).map_or(0, |i| i + 1)..];
                !stmt.contains("@Override")
            });
            if !introduced {
                continue;
            }
            let mut by_class: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
            for (id, own) in &blocks {
                let chain = chain(&own.name);
                if !chain.iter().any(|c| c.name == base.name) {
                    continue;
                }
                let decides = chain
                    .into_iter()
                    .find(|c| c.method_names.iter().any(|n| n == method));
                if let Some(c) = decides {
                    match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                        Some((_, ids)) => ids.push(id.clone()),
                        None => by_class.push((c, vec![id.clone()])),
                    }
                }
            }
            // Asked of blocks whose classes answer differently: else it is no question.
            if by_class.len() < 2 {
                continue;
            }
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
                owner: base.name.clone(),
                method: method.clone(),
                java_params: decl.params_text.clone(),
                rust: format!("{}_{}", snake(&simple(&base.name)), snake(method)),
                params,
                returns,
                default: decl.body.filter(|b| b.len() <= BODY_LIMIT),
                answers: answers(by_class, method, &source, &scan.classes),
                asked: asked[method],
                others: Vec::new(),
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
    let mut entity_texts: Vec<&String> = Vec::new();
    for (_, classes) in &chains {
        for c in classes {
            if seen.insert(c.name.as_str())
                && let Some(text) = source(c)
            {
                entity_texts.push(text);
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
            answers: answers(by_class, method, &source, &scan.classes),
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
    // The parts block entities keep (a belt's `BeltInventory`): the static helpers they
    // call (`BeltFunnelInteractionHandler.checkForFunnels(beltInventory, item, offset)`)
    // work on the block entity too, given its state. And the elements of the lists they
    // keep (`TransportedItemStack`), each a struct of the port's.
    let mut part_texts: Vec<&String> = Vec::new();
    let mut part_called: BTreeSet<String> = BTreeSet::new();
    let mut holders: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut parts: HashMap<String, (String, bool)> = HashMap::new();
    for e in entities {
        for (_, held) in &e.held {
            holders
                .entry(simple(held))
                .or_default()
                .insert(e.rust.clone());
            if let Some(text) = class(held).and_then(source) {
                part_texts.push(text);
                part_called.extend(static_receivers(text));
            }
        }
        for l in &e.lists {
            parts.insert(simple(l), (format!("&mut {}", simple(l)), false));
        }
    }
    // (An element of a list is that struct, though a block entity keeps one of its own: an
    // item drain's `TransportedItemStack`.)
    for (part, of) in holders {
        if let (1, Some(rust)) = (of.len(), of.iter().next())
            && !parts.contains_key(&part)
        {
            parts.insert(part, (format!("&mut {rust}"), true));
        }
    }
    let mut helpers: Vec<Question> = Vec::new();
    let mut worlds: Vec<Question> = Vec::new();
    for c in scan.classes.iter().filter(|c| {
        !c.is_mixin
            && c.kind != ClassKind::Interface
            && (called_into.contains(&simple(&c.name)) || part_called.contains(&simple(&c.name)))
            && !entity_classes.contains(&simple(&c.name))
    }) {
        let by_entities = called_into.contains(&simple(&c.name));
        let Some(text) = source(c) else {
            continue;
        };
        let mut names: Vec<&String> = c.method_names.iter().collect();
        names.sort();
        names.dedup();
        for method in names {
            // Or one that works on the level at a place, where a block entity's own code
            // calls it (`BeltBlock.initBelt(level, worldPosition)`).
            let called = format!("{}.{method}(", simple(&c.name));
            let found = declarations(text, method)
                .into_iter()
                .filter(|_| by_entities)
                .find_map(|d| {
                    signature(&d, Asks::Helper, &entity_classes, &block_types)
                        .map(|(p, r)| (Asks::Helper, d, p, r))
                })
                .or_else(|| {
                    if !entity_texts
                        .iter()
                        .chain(&part_texts)
                        .any(|t| t.contains(&called))
                    {
                        return None;
                    }
                    declarations(text, method).into_iter().find_map(|d| {
                        signature_with(&d, Asks::World, &entity_classes, &block_types, &parts)
                            .map(|(p, r)| (Asks::World, d, p, r))
                    })
                });
            let Some((asks, decl, params, returns)) = found else {
                continue;
            };
            let into = if asks == Asks::World {
                &mut worlds
            } else {
                &mut helpers
            };
            into.push(Question {
                asks,
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
    worlds.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.rust.cmp(&b.rust)));
    worlds.truncate(WORLD_LIMIT);
    out.extend(worlds);

    // To the behaviour of another block entity: a method called on the behaviour the
    // code finds at a place (`BlockEntityBehaviour.get(level, next, DirectBeltInputBehaviour
    // .TYPE).handleInsertion(..)`, `funnelBE.getBehaviour(InvManipulationBehaviour.TYPE)`).
    // Each block entity that makes one sets it up in its own way: answered once for all.
    let mut behaviours: Vec<Question> = Vec::new();
    for b in scan
        .classes
        .iter()
        .filter(|c| !c.is_mixin && c.kind == ClassKind::Class)
    {
        let owner = simple(&b.name);
        let Some(text) = source(b) else {
            continue;
        };
        if !text.contains("BehaviourType<") {
            continue;
        }
        let reaching: Vec<&String> = sources
            .iter()
            .filter(|(n, t)| **n != b.name.as_str() && reaches_elsewhere(t, &owner))
            .map(|(_, t)| t)
            .collect();
        if reaching.is_empty() {
            continue;
        }
        // Who makes one: a block entity's class, or a part it keeps (a depot's
        // `DepotBehaviour`), with how it sets it up.
        let made = format!("new {owner}(");
        let mut by_class: Vec<(&ClassInfo, Vec<String>)> = Vec::new();
        for (e, classes) in &chains {
            let held: Vec<&ClassInfo> = e.held.iter().filter_map(|(_, held)| class(held)).collect();
            let maker = classes
                .iter()
                .copied()
                .chain(held)
                .find(|c| source(c).is_some_and(|t| t.contains(&made)));
            if let Some(c) = maker {
                match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                    Some((_, ids)) => ids.push(e.id.clone()),
                    None => by_class.push((c, vec![e.id.clone()])),
                }
            }
        }
        if by_class.is_empty() {
            continue;
        }
        let mut names: Vec<&String> = b.method_names.iter().collect();
        names.sort();
        names.dedup();
        for method in names {
            let call = format!(".{method}(");
            let asked = reaching.iter().filter(|t| t.contains(&call)).count();
            if asked == 0 {
                continue;
            }
            let Some((decl, params, returns)) =
                declarations(text, method).into_iter().find_map(|d| {
                    signature(&d, Asks::Behaviour, &entity_classes, &block_types)
                        .map(|(p, r)| (d, p, r))
                })
            else {
                continue;
            };
            behaviours.push(Question {
                asks: Asks::Behaviour,
                owner: b.name.clone(),
                method: method.clone(),
                java_params: decl.params_text.clone(),
                rust: format!("{}_{}", snake(&owner), snake(method)),
                params,
                returns,
                default: decl.body.filter(|b| b.len() <= BODY_LIMIT),
                answers: makers(&by_class, &made, &source)
                    .into_iter()
                    .map(|mut a| {
                        a.states =
                            a.of.iter()
                                .filter_map(|id| entities.iter().find(|e| e.id == *id))
                                .map(|e| e.rust.clone())
                                .collect();
                        a
                    })
                    .collect(),
                asked,
                others: Vec::new(),
            });
        }
    }
    behaviours.sort_by(|a, b| b.asked.cmp(&a.asked).then(a.rust.cmp(&b.rust)));
    behaviours.truncate(BEHAVIOUR_LIMIT);
    out.extend(behaviours);
    out
}

/// Whether `text` finds behaviour `owner` of another block entity, at a place:
/// `BlockEntityBehaviour.get(level, pos, Owner.TYPE)` or `other.getBehaviour(Owner.TYPE)`
/// (a block entity's own `getBehaviour(Owner.TYPE)` is not).
fn reaches_elsewhere(text: &str, owner: &str) -> bool {
    let ty = format!("{owner}.TYPE");
    text.match_indices(&ty).any(|(at, _)| {
        let before = &text[at.saturating_sub(160)..at];
        // The call nearest before it.
        let Some(call) = before.rfind("getBehaviour(").max(before.rfind(".get(")) else {
            return false;
        };
        // (Still in the same call.)
        if before[call..].contains(';') {
            return false;
        }
        if before[..call].ends_with("BlockEntityBehaviour") {
            return true;
        }
        let receiver: String = before[..call]
            .trim_end_matches('.')
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        before[..call].ends_with('.') && !receiver.is_empty() && receiver != "siht"
    })
}

/// How each block entity class in `by_class` makes the behaviour (`made` is `new Owner(`):
/// its statement on one line, with the methods of that class it hands the behaviour
/// (`this::tryInsertingFromSide`, `d -> canDirectlyInsertCached()`).
fn makers<'a>(
    by_class: &[(&ClassInfo, Vec<String>)],
    made: &str,
    source: &impl Fn(&ClassInfo) -> Option<&'a String>,
) -> Vec<Answer> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    by_class
        .iter()
        .map(|(c, of)| {
            let text = source(c).map(String::as_str).unwrap_or_default();
            let statement = text
                .find(made)
                .map(|at| {
                    let rest = &text[at..];
                    one_line(&rest[..rest.find(';').unwrap_or(rest.len())])
                })
                .filter(|s| s.len() <= BODY_LIMIT);
            let mut refs: Vec<(String, String)> = Vec::new();
            if let Some(st) = &statement {
                let mut handed: Vec<String> = Vec::new();
                for (at, _) in st.match_indices("::") {
                    let name: String = st[at + 2..].chars().take_while(|c| ident(*c)).collect();
                    handed.push(name);
                }
                // Called bare in a lambda it hands over (`d -> canDirectlyInsertCached()`).
                for (at, _) in st.match_indices('(') {
                    let name: String = st[..at]
                        .chars()
                        .rev()
                        .take_while(|c| ident(*c))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if !st[..at].trim_end_matches(&name).ends_with('.') {
                        handed.push(name);
                    }
                }
                for name in handed {
                    if name.is_empty()
                        || !c.method_names.contains(&name)
                        || refs
                            .iter()
                            .any(|(n, _)| *n == format!("{}.{name}()", simple(&c.name)))
                    {
                        continue;
                    }
                    let body = method_body(text, &name)
                        .filter(|b| b.len() <= BODY_LIMIT)
                        .unwrap_or_default();
                    refs.push((format!("{}.{name}()", simple(&c.name)), body));
                }
                // And the methods of that class those call bare (`insert(stack, ..)` from
                // a depot's `tryInsertingFromSide`): one step further.
                let quoted: Vec<String> = refs.iter().map(|(_, b)| b.clone()).collect();
                for body in quoted {
                    for (at, _) in body.match_indices('(') {
                        let name: String = body[..at]
                            .chars()
                            .rev()
                            .take_while(|c| ident(*c))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        let label = format!("{}.{name}()", simple(&c.name));
                        if name.is_empty()
                            || refs.len() >= 6
                            || body[..at].trim_end_matches(&name).ends_with('.')
                            || !c.method_names.contains(&name)
                            || refs.iter().any(|(n, _)| *n == label)
                        {
                            continue;
                        }
                        let called = method_body(text, &name)
                            .filter(|b| b.len() <= BODY_LIMIT)
                            .unwrap_or_default();
                        refs.push((label, called));
                    }
                }
            }
            Answer {
                class: c.name.clone(),
                of: of.clone(),
                body: statement,
                refs,
                states: Vec::new(),
            }
        })
        .collect()
}

/// The answers of `by_class`, each with its body where that is short enough to quote, and
/// the methods of the mod's other classes that body calls.
fn answers<'a>(
    by_class: Vec<(&ClassInfo, Vec<String>)>,
    method: &str,
    source: &impl Fn(&ClassInfo) -> Option<&'a String>,
    classes: &'a [ClassInfo],
) -> Vec<Answer> {
    by_class
        .into_iter()
        .map(|(c, of)| {
            let body = source(c)
                .and_then(|s| method_body(s, method))
                .filter(|b| b.len() <= BODY_LIMIT);
            let refs = body
                .as_deref()
                .map(|b| referenced(b, &simple(&c.name), source, classes))
                .unwrap_or_default();
            Answer {
                class: c.name.clone(),
                of,
                body,
                refs,
                states: Vec::new(),
            }
        })
        .collect()
}

/// The methods of the mod's other classes `body` calls (`BeltBlockEntity::hasPulley`,
/// `FluidPipeBlock.isPipe(..)`), with their bodies where those are short: at most three.
fn referenced<'a>(
    body: &str,
    own: &str,
    source: &impl Fn(&ClassInfo) -> Option<&'a String>,
    classes: &'a [ClassInfo],
) -> Vec<(String, String)> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out: Vec<(String, String)> = Vec::new();
    for (sep, at) in body
        .match_indices("::")
        .chain(body.match_indices('.'))
        .map(|(i, m)| (m.len(), i))
        .collect::<Vec<_>>()
    {
        let class: String = body[..at]
            .chars()
            .rev()
            .take_while(|c| ident(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let method: String = body[at + sep..].chars().take_while(|c| ident(*c)).collect();
        if !class.starts_with(|c: char| c.is_ascii_uppercase())
            || !method.starts_with(|c: char| c.is_ascii_lowercase())
            || class == own
        {
            continue;
        }
        let name = format!("{class}.{method}()");
        if out.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let mut found = classes.iter().filter(|k| simple(&k.name) == class);
        let (Some(k), None) = (found.next(), found.next()) else {
            continue;
        };
        let Some(b) = source(k)
            .and_then(|s| method_body(s, &method))
            .filter(|b| b.len() <= BODY_LIMIT)
        else {
            continue;
        };
        out.push((name, b));
        if out.len() == 3 {
            break;
        }
    }
    out
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
                    "\n/// `{owner}.{method}({java})`: a question the mod's code asks a block it meets,\n/// answered here once for every block of the mod.\n///\n/// `block` is the block asked\n/// (`{example}`) and `state` its state properties: `state.getValue(FACING)` is the value\n/// under `facing` (a `Direction` as `north`: `Side::from_name` makes it a `Side`),\n/// `getValue(AXIS)` the one under `axis` (`x`, `y` or `z`, as an `Axis` is here). The level\n/// isn't given.\n///\n/// What each block class answers, with its blocks and those of classes that inherit it:\n",
                    method = q.method,
                    java = q.java_params,
                    example = example(q),
                );
            }
            Asks::Entity => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a question the mod's code asks a block entity it meets,\n/// answered here once for every block entity of the mod.\n///\n/// `be` is a view of the one asked\n/// (`{example}`): {VIEW}. A neighbour is such a view already\n/// (`machine.neighbour(side)`), and `machine.me(pos, &state.save())` makes one of the block\n/// entity a tick is for. Its fields are those saved values; a method of its own that the\n/// Java calls (`hasSource()`, `getSpeed()`) is worked out from them as its class does.\n///\n/// What each block entity class answers, with its block entities and those that inherit it:\n",
                    method = q.method,
                    java = q.java_params,
                    example = example(q),
                );
            }
            Asks::Behaviour => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: asked of the `{owner}` of another block entity, the one\n/// the code finds at a place (`BlockEntityBehaviour.get(level, pos, {owner}.TYPE)`,\n/// `be.getBehaviour({owner}.TYPE)`), ported here once for every block entity that makes\n/// one.\n///\n/// `target` is a view of that block entity: {VIEW}. Each block entity sets the\n/// behaviour up in its own way, below with the methods it hands it: answer for `target` as\n/// its own does. What that changes goes through `machine.set_at(pos, key, value)` and\n/// `machine.set_item_at(..)` (the Java may change another block entity than `target`: a\n/// belt segment's items are its controller's, `machine.entity_at` finds it), and a\n/// container through `machine.give_at` and `machine.take_at`, all after the tick. A block\n/// entity none of these is has no `{owner}`: the Java's `get` gives null there, so the\n/// caller asks nothing; answer as the behaviour's own defaults do.\n///\n/// The block entities that make one, and how:\n",
                    method = q.method,
                    java = q.java_params,
                );
            }
            Asks::World => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a helper of the mod's code that works on the level,\n/// ported here once for every function that calls it.\n///\n/// The level is `machine`, the one of\n/// the tick that calls it: a block entity at a place it works out is `machine.entity_at(pos)`\n/// (a copy as it was before the tick: its block, state properties, saved values and item\n/// slots), and what it changes goes through `machine.set_at(pos, key, value)`,\n/// `machine.set_item_at(..)` and `machine.set_block_at(..)`, which land after that tick.\n/// A block entity it takes is a view: {VIEW}. A part a block entity keeps that it takes\n/// (a belt's `BeltInventory`) is that block entity's state, changed in place, and `pos`\n/// its place; an element of a list it keeps (`TransportedItemStack`) is that struct.\n",
                    method = q.method,
                    java = q.java_params,
                );
            }
            Asks::Helper => {
                let _ = write!(
                    r,
                    "\n/// `{owner}.{method}({java})`: a helper of the mod's code that takes block entities,\n/// ported here once for every function that calls it.\n///\n/// A block entity it takes is a view: {VIEW}. A neighbour is such a view already (`machine.neighbour(side)`), and\n/// `machine.me(pos, &state.save())` makes one of the block entity a tick is for. What the\n/// Java reads of them from the level (`getBlockState()`, `getBlockPos()`) is in the views;\n/// the level isn't given. `instanceof` a class is told by the view's block.\n",
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
                    if !a.states.is_empty() {
                        let states: Vec<String> =
                            a.states.iter().map(|t| format!("`{t}`")).collect();
                        let _ = writeln!(
                            r,
                            "///   (its state: {}, read with `{}::load(|k| target.get(k))`, written back key by key with `machine.set_at`)",
                            states.join(", "),
                            a.states[0]
                        );
                    }
                    for (name, called) in &a.refs {
                        if called.is_empty() {
                            let _ = writeln!(
                                r,
                                "///   (`{name}`: too long to quote, read it in `{}`)",
                                simple(&a.class)
                            );
                            if !long.contains(&a.class.as_str()) {
                                long.push(&a.class);
                            }
                        } else {
                            let _ = writeln!(r, "///   (`{name}` is `{}`)", one_line(called));
                        }
                    }
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
            Asks::Helper | Asks::World | Asks::Behaviour => {
                if let Some(body) = &q.default {
                    let _ = writeln!(r, "///\n/// In Java: `{}`", one_line(body));
                }
            }
        }
        // The classes whose answer wasn't quoted are read with the owner.
        let mut from = format!("`{}`", q.owner);
        let read = if q.asks == Asks::Behaviour { 8 } else { 4 };
        for class in long.iter().take(read) {
            let _ = write!(from, ", `{class}`");
        }
        let (given, names) = match q.asks {
            Asks::Block => ("block: &str, state: &[(String, String)]", "block, state"),
            Asks::Entity => ("be: &Neighbour", "be"),
            Asks::Helper | Asks::World => ("", ""),
            Asks::Behaviour => ("target: &Neighbour", "target"),
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
        if matches!(q.asks, Asks::World | Asks::Behaviour) {
            params.push("machine: &mut Machine<'_>".to_string());
            used.push("machine".to_string());
        }
        let todo = match q.asks {
            Asks::Helper | Asks::World | Asks::Behaviour => {
                format!("TODO: port `{}` of {from}.", q.method)
            }
            _ => format!("TODO: port from {from}."),
        };
        let _ = write!(
            r,
            "///\n/// {todo}\npub fn {rust}({params}){returns} {{\n    let _ = {used};\n{value}}}\n",
            rust = q.rust,
            params = params.join(", "),
            // (A command returns nothing.)
            returns = if q.returns == "()" {
                String::new()
            } else {
                format!(" -> {}", q.returns)
            },
            // (`block, state` is two names in one entry: always a tuple unless it is one.)
            used = match &used[..] {
                [one] if !one.contains(',') => one.clone(),
                many => format!("({})", many.join(", ")),
            },
            value = match q.returns {
                "()" => String::new(),
                returns => format!("    {}\n", default_value(returns)),
            },
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
            Asks::Behaviour
                if java.contains(&format!(".{method}(")) && java.contains(owner.as_str()) =>
            {
                let _ = writeln!(
                    out,
                    "- `x.{method}(..)` asked of the `{owner}` of a block entity at another place (`BlockEntityBehaviour.get(level, pos, {owner}.TYPE)`) is `{rust}`, ported once for every block entity that makes one: `{signature}`. Pass a view of that block entity (`machine.entity_at(pos)`; none there is the Java's null: ask nothing) and this tick's `machine`."
                );
            }
            Asks::World
                if java.contains(&format!("{method}(")) && java.contains(owner.as_str()) =>
            {
                let _ = writeln!(
                    out,
                    "- `{owner}.{method}(level, ..)` is `{rust}`, ported on its own: `{signature}`. Call it with the places and this tick's `machine`: what it changes lands after the tick."
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
            } else if line.contains("a helper of the mod's code that works on the level") {
                Some(Asks::World)
            } else if line.contains("of another block entity, the one") {
                Some(Asks::Behaviour)
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
    signature_with(decl, asks, entity_classes, block_types, &HashMap::new())
}

/// `signature`, where a helper that works on the level may take a part a block entity
/// keeps (`parts`: its simple name, the Rust it is here and whether it is a block
/// entity's part rather than a list's element): a belt's `BeltInventory` is the belt's
/// state (`&mut Belt`, with its place `pos` before it), a `TransportedItemStack` that
/// struct.
fn signature_with(
    decl: &Declaration,
    asks: Asks,
    entity_classes: &BTreeSet<String>,
    block_types: &BTreeSet<String>,
    parts: &HashMap<String, (String, bool)>,
) -> Option<(Vec<(String, String)>, &'static str)> {
    if decl.is_static != matches!(asks, Asks::Helper | Asks::World) {
        return None;
    }
    let acts = matches!(asks, Asks::World | Asks::Behaviour);
    let returns = match plain(&decl.returns) {
        // A helper that works on the level may be a command.
        "void" if acts => "()",
        // What is left of a stack (`handleInsertion` gives back what didn't go in).
        "ItemStack" if acts => "Option<Stack>",
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
    let mut level = false;
    let mut part = false;
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
                // (`state_` for a `state` that is the asked block's own name: one `_`.)
                params.push((
                    format!("{}_block", name.trim_end_matches('_')),
                    "&str".to_string(),
                ));
                "&[(String, String)]"
            }
            // The level isn't given (to a helper that works on it, it is `machine`).
            "Level" | "LevelReader" | "LevelAccessor" | "BlockGetter" | "ServerLevel"
            | "BlockAndTintGetter" | "CollisionGetter" => {
                level = true;
                continue;
            }
            "BlockPos" => "[i32; 3]",
            "ItemStack" if acts => "&Stack",
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
            // A part a block entity keeps, or an element of a list it keeps.
            _ if asks == Asks::World && parts.contains_key(ty) => {
                let (rust, holder) = &parts[ty];
                part |= *holder;
                rust.as_str()
            }
            _ => return None,
        };
        params.push((name, rust.to_string()));
    }
    // A helper is offered for the block entities it takes.
    if asks == Asks::Helper && views == 0 {
        return None;
    }
    // One that works on the level, for a place in it, or on a block entity's part.
    let place = params.iter().any(|(_, t)| t == "[i32; 3]");
    if asks == Asks::World && !(level && place) && !part {
        return None;
    }
    // (The part's block entity's place, which the Java reads from the part.)
    if part && !place {
        params.insert(0, ("pos".to_string(), "[i32; 3]".to_string()));
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
        "Option<Stack>" => "None",
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
                    ("state_block".to_string(), "&str".to_string()),
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
        // One that works on the level at a place, a command: given `machine`.
        let d = first(helper, "handleAdded");
        assert_eq!(
            signature(&d, Asks::World, &entities(), &BTreeSet::new()),
            Some((
                vec![
                    ("pos".to_string(), "[i32; 3]".to_string()),
                    ("be_".to_string(), "&Neighbour".to_string())
                ],
                "()"
            ))
        );
        // A belt's kept part is the belt's state, its place before it.
        let funnels = "public class BeltFunnelInteractionHandler {\n\tpublic static boolean checkForFunnels(BeltInventory beltInventory, TransportedItemStack currentItem, float nextOffset) {\n\t\treturn false;\n\t}\n}";
        let parts: HashMap<String, (String, bool)> = [
            ("BeltInventory".to_string(), ("&mut Belt".to_string(), true)),
            (
                "TransportedItemStack".to_string(),
                ("&mut TransportedItemStack".to_string(), false),
            ),
        ]
        .into();
        assert_eq!(
            signature_with(
                &first(funnels, "checkForFunnels"),
                Asks::World,
                &entities(),
                &BTreeSet::new(),
                &parts
            ),
            Some((
                vec![
                    ("pos".to_string(), "[i32; 3]".to_string()),
                    ("belt_inventory".to_string(), "&mut Belt".to_string()),
                    (
                        "current_item".to_string(),
                        "&mut TransportedItemStack".to_string()
                    ),
                    ("next_offset".to_string(), "f32".to_string())
                ],
                "bool"
            ))
        );
        // Not without the level, nor without a place.
        assert_eq!(
            signature(
                &first(helper, "isLargeToLargeGear"),
                Asks::World,
                &entities(),
                &BTreeSet::new()
            ),
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

    #[test]
    fn a_behaviour_found_at_another_place_is_asked_of_another_block_entity() {
        let belt = "DirectBeltInputBehaviour inputBehaviour =
		BlockEntityBehaviour.get(world, nextPosition, DirectBeltInputBehaviour.TYPE);";
        assert!(reaches_elsewhere(belt, "DirectBeltInputBehaviour"));
        let funnel = "InvManipulationBehaviour inserting = funnelBE.getBehaviour(InvManipulationBehaviour.TYPE);";
        assert!(reaches_elsewhere(funnel, "InvManipulationBehaviour"));
        // The call nearest it counts, not one of an earlier statement.
        let both = "x = getBehaviour(FilteringBehaviour.TYPE); y = BlockEntityBehaviour.get(level, below, DirectBeltInputBehaviour.TYPE);";
        assert!(reaches_elsewhere(both, "DirectBeltInputBehaviour"));
        // Its own is not another's.
        let own = "FilteringBehaviour f = getBehaviour(FilteringBehaviour.TYPE); this.getBehaviour(FilteringBehaviour.TYPE);";
        assert!(!reaches_elsewhere(own, "FilteringBehaviour"));
    }

    #[test]
    fn an_answer_brings_the_methods_it_calls() {
        let belt = "public class BeltBlockEntity {
	public boolean hasPulley() {
		return getBlockState().getValue(BeltBlock.PART) != BeltPart.MIDDLE;
	}
}"
        .to_string();
        let classes = vec![ClassInfo {
            name: "com.belt.BeltBlockEntity".into(),
            kind: ClassKind::Class,
            path: "BeltBlockEntity.java".into(),
            superclass: None,
            interfaces: Vec::new(),
            references: Vec::new(),
            side: porter_model::Side::Common,
            is_mixin: false,
            lines: 5,
            methods: 1,
            fields: Vec::new(),
            method_names: vec!["hasPulley".into()],
            saved: Vec::new(),
        }];
        let source = |c: &ClassInfo| (c.name == "com.belt.BeltBlockEntity").then_some(&belt);
        let refs = referenced(
            "return getBlockEntityOptional(world, pos).map(BeltBlockEntity::hasPulley).orElse(false);",
            "BeltBlock",
            &source,
            &classes,
        );
        assert_eq!(
            refs,
            vec![(
                "BeltBlockEntity.hasPulley()".to_string(),
                "return getBlockState().getValue(BeltBlock.PART) != BeltPart.MIDDLE;".to_string()
            )]
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
                refs: Vec::new(),
                states: Vec::new(),
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
