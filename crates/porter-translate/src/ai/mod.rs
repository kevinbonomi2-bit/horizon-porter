//! The AI pass (milestone M4): translates the functions `port` left to port, one at
//! a time, from the Java code each names.
//!
//! For each function marked TODO, the translator sends the model the Java classes
//! it names, the Rust file it sits in and, for a hook helper, Horizon's hook and
//! the game functions hook code calls. The answer replaces the function. A
//! function in `runtime.rs` is then compiled and tested with `cargo test`; on an
//! error the model sees its attempt and the compiler's message and tries again.
//! Hook helpers compile only inside Horizon, so `build` checks them.
//!
//! It touches only functions that still carry their TODO, so it never overwrites
//! one you wrote or edited, and it can stop and resume at any point: each result
//! is saved to `.porter/translate.json` as it comes.

pub mod model;
pub mod units;

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use porter_model::ModScan;

use model::Model;
use units::{Kind, Unit};

/// What to translate.
#[derive(Debug, Clone)]
pub struct Options {
    /// Only functions whose name contains this.
    pub only: Option<String>,
    /// At most this many functions.
    pub limit: Option<usize>,
    /// Tries per function, the first included.
    pub attempts: usize,
    /// List the work and the prompt sizes, call no model.
    pub dry_run: bool,
    /// Also port again the functions `port --force` marked outdated, and those whose
    /// port said something was out of reach with an older horizon-porter.
    pub refresh: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            only: None,
            limit: None,
            attempts: 3,
            dry_run: false,
            refresh: false,
        }
    }
}

/// What happened to one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Translated, compiled and its module's tests pass.
    Ported,
    /// Translated; Horizon compiles it (`horizon-porter build`).
    Unchecked,
    /// Left as it was, with why.
    Failed(String),
    /// The model gave no answer (no key, no connection, Ollama not running): the
    /// translation stopped there, and this function and the rest are left as they were.
    NoAnswer(String),
    /// A dry run: what would be sent.
    Planned { prompt_chars: usize },
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub unit: Unit,
    pub status: Status,
    pub attempts: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What a ported function can reach, by version: each time runtime.rs gives ported code
/// more, a line here, with words a port uses for what it couldn't do then (none: any
/// port that says something was out of reach). `translate --refresh` ports a function
/// again when it was ported before a line whose words its notes use.
const REACHES: &[(u32, &[&str])] = &[
    // Tags, water, light, dropped items, redstone and a block's state in a tick, slots'
    // stack sizes: broad enough that every port with a gap is worth another go.
    (1, &[]),
    // Sturdy faces, the held item in a placement, the action bar, breaking a block,
    // and the mod's config where it is a class of plain fields.
    (
        2,
        &[
            "sturdy",
            "cansupport",
            "support",
            "shape query",
            "solid",
            "getiteminhand",
            "held item",
            "carry the held",
            "overlay",
            "displayclientmessage",
            "action bar",
            "destroyblock",
            "destroy the block",
            "break the block",
            // Plain-field config classes are read now (`config()`).
            "config",
        ],
    ),
    // Spawning an entity, the player's position, eyes and look in a use, vanilla's
    // level events and a block's break effect, game events, the sky.
    (
        3,
        &[
            "levelevent",
            "level event",
            "break effect",
            "block-break effect",
            "gameevent",
            "game event",
            "spawn",
            "summon",
            "addfreshentity",
            "player's position",
            "player position",
            "look vector",
            "look angle",
            "pov",
            "eye",
            "canseesky",
            "sky",
        ],
    ),
    // A shape update that removes the block or schedules a tick, spawning from a
    // block's tick and a block entity's, babies, thrown things, a block's own
    // particles, sounds at a point, the sky's and the blocks' light apart.
    (
        4,
        &[
            "blocks.air",
            "into air",
            "minecraft:air",
            "removal",
            "block change, not a property",
            "scheduledtickaccess",
            "schedule a tick",
            "scheduled-tick",
            "spawn",
            "summon",
            "addfreshentity",
            ".create(",
            "entity api",
            "entity creation",
            "setage",
            "baby",
            "velocity",
            "projectile",
            "thrown",
            "blockparticleoption",
            "block particle",
            "block-particle",
            "needs the block state",
            "takes a block position",
            "lightlayer",
        ],
    ),
    // A line in the player's chat; the ids of a long registry class's constants.
    (
        5,
        &[
            "chat",
            "sendsystemmessage",
            "system message",
            "sound id",
            "sound event id",
            "the id of",
            "registry id",
        ],
    ),
    // Whether a spot gives way to a placed block.
    (6, &["canbereplaced", "replaceable"]),
    // A placement on top of the template's own.
    (
        7,
        &[
            "super.getstateforplacement",
            "super method",
            "super placement",
            "multiface",
        ],
    ),
    // Stats, item cooldowns, effects on the player, whether they can eat, a ray, the
    // mobs near a spot and love mode.
    (
        8,
        &[
            "awardstat",
            "stats.",
            "stat award",
            "no stat",
            "cooldown",
            "addeffect",
            "mobeffectinstance",
            "caneat",
            "can eat",
            "povhit",
            "ray cast",
            "raycast",
            "no ray",
            "clip(",
            "getentitiesofclass",
            "entity query",
            "getentities",
            "setinlove",
            "love",
        ],
    ),
    // The prompt names the runtime's call beside the Java it stands for: ports that
    // overlooked one of reach 8's in the long trait.
    (
        9,
        &[
            "awardstat",
            "stats.",
            "stat award",
            "no stat",
            "cooldown",
            "addeffect",
            "mobeffectinstance",
            "caneat",
            "povhit",
            "getentitiesofclass",
            "setinlove",
        ],
    ),
    // A block entity keeps the items of a class it holds in its own slots.
    (10, &["no rust type"]),
    // A neighbour's state, a renderer's block state, and a network walk as each block
    // entity's own step.
    (
        11,
        &[
            "propagat",
            "torque network",
            "kinetic network",
            "neighbour's state",
            "neighbour's block state",
        ],
    ),
    // A tick does the work of the code it gave other block entities to call, on them;
    // and config values come with their declared defaults.
    (
        12,
        &[
            "config",
            "handler",
            "callback",
            "belt mode",
            "on a belt",
            "on the belt",
            "depot",
        ],
    ),
];

/// The newest line of `REACHES`.
pub const REACH: u32 = 12;

/// For a tick that reaches another block entity: who does what, where the Java has one
/// of them call code the other registered.
const CALLBACKS: &str = "Where the Java gives another block entity code to call (a behaviour built with handlers \
such as `whenItemEnters(..)` and `whileItemHeld(..)`, which the block entity an item rests on calls), nothing calls it \
here: each tick is ported alone. So the block entity that registers the handlers does their work itself, from its own \
tick, each tick, on the block entity they would be called for (`machine.at(..)`: its saved values and its item slots, \
changed with `machine.set_at` and `machine.set_item_at`). And the other one, whose Java only calls handlers it was \
given (`behaviour.handleReceivedItem(..)`, `handleHeldItem(..)`), leaves that out: say in a comment that the block \
entity which registers them does it, and write no `// Missing:` note for it.";

/// For a tick whose comment names the block entities that call code it gave them: how
/// to do that work from this tick.
const CALLED: &str = "This function's comment names block entities that call code this one gave them, and the Java \
classes that do (shown above). Nothing calls that code here, so write it into this tick as a part of its own; a tick \
that leaves it out does nothing for them. Each tick: (1) find the caller where its Java looks for this block entity \
from (`pos.above(2)` in the caller's code is `machine.at([x, y - 2, z])` here) and check its `entity` is one of the \
ids named; (2) apply the caller's own conditions for calling, then do what the code it would call does, with the \
caller's thing as its argument (the item it holds is `n.items[slot]`, by the slots the comment gives). Where the \
caller calls one handler first and another once the first answered that it holds the thing, tell the two apart by \
this block entity's own state (not started: the first; started: the second); (3) what that code does to the caller \
through what it is handed (`handler.handleProcessingOnItem(..)`: read the caller's Java for what it does with the \
result) is `machine.set_item_at(pos, slot, stack)` for its item slots and `machine.set_at(pos, key, value)` for its \
saved values. A flag the caller keeps on its own thing that this module has no place for (an item's `locked`) is left \
out with a `// Missing:` note; the work itself is not.";

/// For a tick that sees its neighbours: how to port a walk over a whole network.
const NETWORK_STEP: &str = "This tick sees the six blocks beside it (`machine.neighbour(side)`: each one's block, \
state properties and saved values) and what it sets in them (`machine.set_neighbour`) lands after the tick. Where the \
Java reaches further in one go (a helper that walks a network of connected block entities and sets values in each: \
rotation, energy, a signal), port it as this block entity's own step: every tick, work out from the neighbours what \
the walk would have given this one (a connected neighbour's saved speed or charge, and which neighbour it came from), \
keep it in `state`, drop it when that neighbour no longer gives it, and let the neighbours' own ticks carry it on. \
Keep which neighbour gave it too (in the field the Java keeps its source in, where `state` has one), and take nothing \
from a neighbour whose own came from this block: two blocks would keep each other going after their source is gone. A \
change then travels one block a tick instead of at once: say so in a comment, and write no `// Missing:` note for the \
walk.";

/// Whether a function ported at reach `at`, whose port says `notes` were out of reach,
/// is worth porting again now.
fn reach_grew_for(at: u32, notes: &str) -> bool {
    let notes = notes.to_ascii_lowercase();
    REACHES
        .iter()
        .filter(|(n, _)| *n > at)
        .any(|(_, words)| words.is_empty() || words.iter().any(|w| notes.contains(w)))
}

/// The ported functions `translate --refresh` ports again: those `port --force` marked
/// outdated, and those that said something was out of reach when ported with an older
/// horizon-porter.
pub fn outdated(dir: &Path) -> Result<Vec<Unit>> {
    let state = State::load(dir);
    let mut out = Vec::new();
    for file in rust_files(dir)? {
        let rel = file
            .strip_prefix(dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&file)?;
        for unit in units::find_again(&rel, &source, true) {
            let marked = units::function_comment(&source, &unit.function)
                .is_some_and(|c| c.contains(units::OUTDATED));
            let reach = state.units.get(&unit.key()).map_or(0, |r| r.reach);
            let notes = units::gap_notes(
                &units::function_source(&source, &unit.function).unwrap_or_default(),
            );
            // (One that names a state property no block has is wrong whatever it
            // reached: ported again whenever asked.)
            let misnamed = units::function_source(&source, &unit.function)
                .is_some_and(|f| units::misnames_a_property(&f));
            if marked || misnamed || reach_grew_for(reach, &notes) {
                out.push(unit);
            }
        }
    }
    Ok(out)
}

/// What the ported functions of a mod folder say they couldn't do: (file, function, its
/// notes), for each function whose port has a note.
pub fn notes(dir: &Path) -> Result<Vec<(String, String, String)>> {
    let mut out = Vec::new();
    for file in rust_files(dir)? {
        let rel = file
            .strip_prefix(dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&file)?;
        for name in units::ported_functions(&source) {
            let body = units::function_source(&source, &name).unwrap_or_default();
            let notes = units::gap_notes(&body);
            if !notes.is_empty() {
                out.push((rel.clone(), name, notes));
            }
        }
    }
    Ok(out)
}

/// Writes `TRANSLATION-NOTES.md` in a mod folder: what each ported function says it
/// couldn't do, so nobody has to read the code to learn what didn't come over. Removes
/// it when there is nothing to say. How many functions it lists.
pub fn write_notes(dir: &Path) -> Result<usize> {
    let notes = notes(dir)?;
    let path = dir.join("TRANSLATION-NOTES.md");
    if notes.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(0);
    }
    let mut text = String::from(
        "# What the translation left out\n\nEach function below was ported, with a note where it couldn't do what the Java does\n(the function wasn't given a way to). The rest of each function is ported. A newer\nhorizon-porter may reach more: `horizon-porter translate <this folder> --refresh`.\n",
    );
    let mut file = "";
    for (rel, function, notes) in &notes {
        if rel != file {
            let _ = write!(text, "\n## {rel}\n");
            file = rel;
        }
        let _ = write!(text, "\n### `{function}`\n\n");
        for line in notes.lines() {
            let line = line.trim_start_matches('/').trim();
            if !line.is_empty() {
                let _ = writeln!(text, "{line}");
            }
        }
    }
    fs::write(&path, text)?;
    Ok(notes.len())
}

/// The functions left to port in a mod folder `port` wrote.
pub fn pending(dir: &Path) -> Result<Vec<Unit>> {
    let mut out = Vec::new();
    for file in rust_files(dir)? {
        let rel = file
            .strip_prefix(dir)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&file)?;
        out.extend(units::find(&rel, &source));
    }
    Ok(out)
}

fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mixins = dir.join("mixins");
    let mut files = Vec::new();
    let Ok(bundles) = fs::read_dir(&mixins) else {
        return Ok(files);
    };
    for bundle in bundles.flatten() {
        if !bundle.path().is_dir() {
            continue;
        }
        for entry in fs::read_dir(bundle.path())?.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "rs") {
                files.push(p);
            }
        }
    }
    // runtime.rs first: helpers call it.
    files.sort_by_key(|p| (p.file_name().is_none_or(|n| n != "runtime.rs"), p.clone()));
    Ok(files)
}

/// What `translate` would do, calling no model: each function and its prompt's size.
pub fn plan(dir: &Path, scan: &ModScan, opts: &Options) -> Result<Vec<Outcome>> {
    let opts = Options {
        dry_run: true,
        ..opts.clone()
    };
    translate(
        dir,
        scan,
        &model::External {
            command: "none".into(),
        },
        &opts,
        |_| {},
    )
}

/// Input tokens for one try of each planned function (about four characters each).
pub fn planned_tokens(plan: &[Outcome]) -> usize {
    plan.iter()
        .map(|o| match o.status {
            Status::Planned { prompt_chars } => prompt_chars / 4,
            _ => 0,
        })
        .sum()
}

/// Dollars for one try of each planned function on `model`, when its price is known
/// (about a thousand tokens come back per function).
pub fn planned_dollars(plan: &[Outcome], model: &str) -> Option<f64> {
    let (input, output) = model::price(model)?;
    Some(planned_tokens(plan) as f64 / 1e6 * input + (plan.len() * 1000) as f64 / 1e6 * output)
}

/// Translates the functions left in `dir` (a folder `port` wrote from `scan`'s mod)
/// with `model`, reporting each step to `progress`.
pub fn translate(
    dir: &Path,
    scan: &ModScan,
    model: &dyn Model,
    opts: &Options,
    mut progress: impl FnMut(&str),
) -> Result<Vec<Outcome>> {
    let mut work = pending(dir)?;
    if opts.refresh {
        work.extend(outdated(dir)?);
    }
    if let Some(only) = &opts.only {
        work.retain(|u| u.function.contains(only.as_str()));
    }
    if let Some(limit) = opts.limit {
        work.truncate(limit);
    }
    let cargo = dir.join("Cargo.toml").is_file();
    if !opts.dry_run && cargo && work.iter().any(|u| u.kind == Kind::Runtime) {
        progress("Checking that the mod's tests pass before translating");
        if let Err(errors) = cargo_test(dir) {
            bail!(
                "`cargo test` fails in {} before anything is translated; fix that first:\n{}",
                dir.display(),
                errors
            );
        }
    }

    let mut state = State::load(dir);
    let mut out = Vec::new();
    for (n, unit) in work.iter().enumerate() {
        progress(&format!(
            "[{}/{}] {} ({})",
            n + 1,
            work.len(),
            unit.function,
            unit.file
        ));
        let prompt = prompt(dir, scan, unit)?;
        for m in &prompt.notes {
            progress(&format!("  {m}"));
        }
        if opts.dry_run {
            out.push(Outcome {
                unit: unit.clone(),
                status: Status::Planned {
                    prompt_chars: prompt.len(),
                },
                attempts: 0,
                input_tokens: 0,
                output_tokens: 0,
            });
            continue;
        }
        let outcome = translate_one(dir, unit, &prompt, model, opts, cargo, &mut progress);
        progress(&format!("  {}", describe(&outcome.status)));
        let stop = matches!(outcome.status, Status::NoAnswer(_));
        state.record(&outcome, &model.name());
        state.save(dir)?;
        out.push(outcome);
        // Every other function would get the same non-answer.
        if stop {
            break;
        }
    }
    Ok(out)
}

/// A status in a word or two, for progress and summaries.
pub fn describe(status: &Status) -> String {
    match status {
        Status::Ported => "ported: compiles and its tests pass".into(),
        Status::Unchecked => "ported: `horizon-porter build` checks it".into(),
        Status::Failed(why) => format!("left as it was: {why}"),
        Status::NoAnswer(why) => format!("stopped: the model gave no answer: {why}"),
        Status::Planned { prompt_chars } => format!("would send {prompt_chars} characters"),
    }
}

fn translate_one(
    dir: &Path,
    unit: &Unit,
    prompt: &Prompt,
    model: &dyn Model,
    opts: &Options,
    cargo: bool,
    progress: &mut impl FnMut(&str),
) -> Outcome {
    let path = dir.join(&unit.file);
    let mut outcome = Outcome {
        unit: unit.clone(),
        status: Status::Failed("no attempt".into()),
        attempts: 0,
        input_tokens: 0,
        output_tokens: 0,
    };
    let original = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            outcome.status = Status::Failed(format!("reading {}: {e}", unit.file));
            return outcome;
        }
    };
    let Some(stub) = units::function_source(&original, &unit.function) else {
        outcome.status = Status::Failed("its function is gone".into());
        return outcome;
    };
    let mut feedback = String::new();
    let mut asked_where = false;
    for attempt in 1..=opts.attempts.max(1) {
        outcome.attempts = attempt;
        let ask = if feedback.is_empty() {
            prompt.text.clone()
        } else {
            format!("{}\n\n{feedback}", prompt.text)
        };
        let reply = match model.complete(SYSTEM, &prompt.shared, &ask) {
            Ok(r) => r,
            Err(e) => {
                let why = format!("{e:#}");
                // One function's answer too long, or declined: that function's alone, the
                // others go on. Anything else (no key, no network) would be the same for all.
                outcome.status = if why.contains("cut off") || why.contains("declined") {
                    Status::Failed(why)
                } else {
                    Status::NoAnswer(why)
                };
                return outcome;
            }
        };
        outcome.input_tokens += reply.input_tokens;
        outcome.output_tokens += reply.output_tokens;
        let Some(function) = rust_function(&reply.text, &unit.function) else {
            feedback = format!(
                "Your last answer had no ```rust block with `fn {}`. Answer with the whole function in one ```rust block.",
                unit.function
            );
            continue;
        };
        if units::signature(&function) != units::signature(&stub) {
            feedback = format!(
                "Your last answer changed the function's signature. Keep it exactly:\n```rust\n{}\n```\nYour answer was:\n```rust\n{function}\n```",
                units::signature(&stub)
            );
            continue;
        }
        if prompt.placed_at
            && !asked_where
            && function.contains("p.clicked")
            && !function.contains("p.pos")
        {
            asked_where = true;
            feedback = format!(
                "Your last answer reads only `p.clicked`, the block that was clicked. The Java's                  `context.getClickedPos()` is where the block is being placed, which is `p.pos`: the blocks                  beside the new block are counted from `p.pos`. Use `p.pos` for it, and keep `p.clicked` only                  where the Java asks for the block placed against (`getHitResult().getBlockPos()`,                  `getClickedPos().relative(getClickedFace().getOpposite())`). Your answer was:
```rust
{function}
```"
            );
            continue;
        }
        if let Some((constant, name)) = prompt.properties.iter().find(|(constant, _)| {
            function.contains(&format!("\"{}\"", constant.to_ascii_lowercase()))
        }) {
            let wrong = constant.to_ascii_lowercase();
            feedback = format!(
                "Your last answer uses a state property `\"{wrong}\"`, which no block has: the Java constant \
                 `{constant}` is the property `\"{name}\"`. Use `\"{name}\"` wherever the Java has `{constant}`, \
                 and check the other properties you named the same way. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        let comment = ported_comment(&unit.comment, &model.name());
        let Some(updated) = units::replace_function(&original, &unit.function, &comment, &function)
        else {
            outcome.status = Status::Failed("could not place the answer in the file".into());
            return outcome;
        };
        if let Err(e) = fs::write(&path, &updated) {
            outcome.status = Status::Failed(format!("writing {}: {e}", unit.file));
            return outcome;
        }
        if unit.kind == Kind::Helper || !cargo {
            outcome.status = Status::Unchecked;
            return outcome;
        }
        match cargo_test(dir) {
            Ok(()) => {
                outcome.status = Status::Ported;
                return outcome;
            }
            Err(errors) => {
                let _ = fs::write(&path, &original);
                progress(&format!(
                    "  attempt {attempt} did not compile or pass its tests"
                ));
                feedback = format!(
                    "Your last answer was:\n```rust\n{function}\n```\n`cargo test` on the module with it said:\n```\n{}\n```\nFix it and answer with the whole function again.",
                    compiler_excerpt(&errors)
                );
            }
        }
    }
    let _ = fs::write(&path, &original);
    outcome.status = Status::Failed(format!(
        "no version compiled and passed its tests in {} tries",
        outcome.attempts
    ));
    outcome
}

/// The comment above a translated function: the TODO line becomes a note to review it.
fn ported_comment(comment: &str, model: &str) -> String {
    comment
        .split_inclusive('\n')
        .map(|line| match line.find(units::TODO) {
            Some(i) => {
                let rest = line[i + units::TODO.len()..].trim();
                let note = if rest == "into this function:" {
                    format!("Ported by horizon-porter translate ({model}) from these; review it:")
                } else if rest.starts_with("from ") {
                    format!("Ported by horizon-porter translate ({model}) {rest} Review it.")
                } else {
                    format!("Ported by horizon-porter translate ({model}): {rest} Review it.")
                };
                format!("{}{note}\n", &line[..i])
            }
            None => line.to_string(),
        })
        .collect()
}

/// The function named `name` in a ```rust block of the answer.
fn rust_function(text: &str, name: &str) -> Option<String> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let body_start = after.find('\n').map_or(after.len(), |i| i + 1);
        let Some(end) = after[body_start..].find("```") else {
            break;
        };
        blocks.push(after[body_start..body_start + end].to_string());
        rest = &after[body_start + end + 3..];
    }
    blocks
        .iter()
        .find_map(|b| units::function_source(b, name))
        .map(|f| f.trim_end().to_string() + "\n")
}

/// The first errors of `cargo test`'s output, as much as a model needs to fix them.
fn compiler_excerpt(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .filter(|l| {
            !l.trim_start().starts_with("Compiling") && !l.trim_start().starts_with("Running")
        })
        .collect();
    let text = lines.join("\n");
    if text.len() > 12_000 {
        format!("{}\n... (more errors after these)", &text[..12_000])
    } else {
        text
    }
}

/// `cargo test` in the mod folder, built in `.porter/target`.
fn cargo_test(dir: &Path) -> std::result::Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["test", "--quiet", "--offline"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join(".porter").join("target"))
        .env_remove("RUSTFLAGS")
        .output()
        .map_err(|e| format!("running cargo: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        ))
    }
}

/// How much Java one prompt carries: whole classes, up to about 100k tokens.
const JAVA_BUDGET: usize = 400_000;
/// Of it, the most for the mod's classes the named ones use (about 15 000 tokens).
const USED_BUDGET: usize = 60_000;

/// A prompt: a part shared by every prompt of its kind, which the API caches, and
/// the part for this function.
#[derive(Debug, Clone, Default)]
pub struct Prompt {
    pub shared: String,
    pub text: String,
    /// What it had to leave out, and why.
    pub notes: Vec<String>,
    /// State properties the Java names by a constant that isn't their name:
    /// (`HORIZONTAL_FACING`, `facing`). An answer that reads `"horizontal_facing"`
    /// reads a property no block has, and is sent back.
    pub properties: Vec<(String, String)>,
    /// Whether it is a placement whose Java asks where the block goes
    /// (`context.getClickedPos()`): an answer that only ever reads the block clicked
    /// is asked once whether it meant that.
    pub placed_at: bool,
}

impl Prompt {
    pub fn len(&self) -> usize {
        self.shared.len() + self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The prompt for one function.
fn prompt(dir: &Path, scan: &ModScan, unit: &Unit) -> Result<Prompt> {
    let mut out = Prompt::default();
    let file = fs::read_to_string(dir.join(&unit.file))
        .with_context(|| format!("reading {}", unit.file))?;
    let mut stub = units::function_source(&file, &unit.function).unwrap_or_default();
    if unit.again {
        // Ported again from the Java alone: its last port isn't shown.
        stub = units::stub_of(&stub);
    }

    // What every hook helper prompt shares: what a helper is, and the game's functions.
    if unit.kind == Kind::Helper {
        let s = &mut out.shared;
        let _ = writeln!(
            s,
            "The function to port is a horizon-mixins helper: it is pasted at the start of the Horizon function its comment \
             names, inside Horizon's game crate, and called with the parameters shown. It can't add `use` lines to the file: \
             put any `use` inside the function. It reaches the game through its parameters, and the mod's own state through \
             the mod's module (shown below), whose `pub mod`s hold the Java's static fields. These are the game's functions \
             hook code calls (`game.native_...(...)`), from Horizon {}:\n",
            &porter_map::hooks::catalog().horizon[..7]
        );
        for h in &porter_map::hooks::catalog().helpers {
            let _ = writeln!(s, "- `{}`: {}", h.signature(), h.doc);
        }
    }

    let p = &mut out.text;
    let _ = writeln!(
        p,
        "Port this function of a Minecraft mod's Horizon port. Its comment says which Java code it replaces:\n\n```rust\n{}{}```\n",
        unit.comment, stub
    );

    // The Java it names.
    let mut java_chars = 0;
    let mut sent = Vec::new();
    let mut java_text = String::new();
    for name in &unit.java {
        let Some(class) = find_class(scan, name) else {
            out.notes.push(format!("no Java source found for {name}"));
            continue;
        };
        if sent.contains(&class.name) {
            continue;
        }
        let path = Path::new(&scan.input).join(&class.path);
        let Ok(source) = fs::read_to_string(&path) else {
            out.notes
                .push(format!("{} is in a jar: no source to send", class.name));
            continue;
        };
        if java_chars + source.len() > JAVA_BUDGET {
            out.notes.push(format!(
                "{} left out: the Java for this function is past {JAVA_BUDGET} characters",
                class.name
            ));
            continue;
        }
        java_chars += source.len();
        let _ = writeln!(
            p,
            "The Java class `{}` ({}):\n\n```java\n{source}\n```\n",
            class.name, class.path
        );
        sent.push(class.name.clone());
        java_text.push_str(&source);
    }
    // The Java it is ported from, without the classes shown beside it.
    let own_java = java_text.clone();
    if sent.is_empty() {
        let _ = writeln!(
            p,
            "(No Java source could be found for it: port it from its comment, or write the most faithful stand-in and say what is missing in a comment.)\n"
        );
    }
    // The mod's own classes that code uses (a terminal's storage network, a
    // machine's recipe helper), smallest first, so the model ports what they do
    // instead of guessing.
    let mut used: Vec<(&porter_model::ClassInfo, String)> = Vec::new();
    for name in sent.clone() {
        let Some(class) = scan.classes.iter().find(|c| c.name == name) else {
            continue;
        };
        for r in &class.references {
            let Some(other) = scan.classes.iter().find(|c| c.name == *r) else {
                continue;
            };
            if other.is_mixin
                || sent.contains(&other.name)
                || used.iter().any(|(c, _)| c.name == other.name)
            {
                continue;
            }
            if let Ok(source) = fs::read_to_string(Path::new(&scan.input).join(&other.path)) {
                used.push((other, source));
            }
        }
        // A class of its own package needs no import, so the scan's references miss
        // it: a block's block entity beside it (`HandCrankBlockEntity`), by its name
        // in the code.
        let package = |n: &str| {
            n.rsplit_once('.')
                .map_or(String::new(), |(p, _)| p.to_string())
        };
        let here = package(&class.name);
        for other in scan.classes.iter().filter(|c| package(&c.name) == here) {
            let simple = other.name.rsplit('.').next().unwrap_or(&other.name);
            if other.is_mixin
                || sent.contains(&other.name)
                || used.iter().any(|(c, _)| c.name == other.name)
                || !names_word(&java_text, simple)
            {
                continue;
            }
            if let Ok(source) = fs::read_to_string(Path::new(&scan.input).join(&other.path)) {
                used.push((other, source));
            }
        }
    }
    // Those whose static functions the code calls come first (`RotationPropagator.
    // handleAdded(..)`: what the tick hands its work to), then the smallest.
    let calls_into = |class: &porter_model::ClassInfo| {
        let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
        java_text
            .match_indices(&format!("{simple}."))
            .any(|(i, m)| {
                let rest = &java_text[i + m.len()..];
                let name = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                rest.starts_with(|c: char| c.is_ascii_lowercase()) && rest[name..].starts_with('(')
            })
    };
    // A block's code reaches its block entity through an instance, not a static call
    // (`withBlockEntityDo(level, pos, be -> be.turn(sneaking))`): a block entity class
    // the code names comes before the rest, or a click that only tells its block
    // entity what to do is ported as doing nothing.
    let its_entity = |class: &porter_model::ClassInfo| {
        let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
        simple.ends_with("BlockEntity") || simple.ends_with("TileEntity")
    };
    // (The key reads all the Java: worked out once a class.)
    used.sort_by_cached_key(|(class, s)| (!its_entity(class), !calls_into(class), s.len()));
    let mut shown: Vec<String> = sent.clone();
    let mut used_chars = 0;
    // The Java of the classes too long to show.
    let mut unseen_java = String::new();
    for (class, source) in used {
        if used_chars + source.len() > USED_BUDGET || java_chars + source.len() > JAVA_BUDGET {
            // Too long to show (a registry class: `ModItems`, `ModSounds`): the ids of
            // its constants the code names, so `ModSounds.EGG_HATCH` isn't a guess.
            let simple = class.name.rsplit('.').next().unwrap_or(&class.name);
            unseen_java.push_str(&source);
            let named = named_constants(simple, &source, &java_text);
            if named.is_empty() {
                out.notes.push(format!(
                    "{} (a class it uses) left out: past the budget for them",
                    class.name
                ));
            } else {
                let _ = writeln!(
                    p,
                    "`{}` is a class of the mod too long to show. Its constants the code above names, with the id each is registered under (in the mod's namespace unless it has one):

{named}",
                    class.name
                );
                out.notes.push(format!(
                    "{} (a class it uses) is too long to show: the ids of its constants the code names are",
                    class.name
                ));
            }
            continue;
        }
        used_chars += source.len();
        java_chars += source.len();
        shown.push(class.name.clone());
        let _ = writeln!(
            p,
            "A class of the mod that code uses, `{}` ({}), for what its calls do:\n\n```java\n{source}\n```\n",
            class.name, class.path
        );
        java_text.push_str(&source);
    }
    // What the mod's blocks answer for its own interfaces (`IRotate.hasShaftTowards`),
    // where the code asks a block it meets: they are in classes this function isn't from.
    // Those the module already has a function for are named instead: ported once.
    let questions = crate::questions::ported(&file, &java_text, &unit.function);
    let answers = if crate::questions::is_question(&file, &unit.function) {
        // Its own comment lists them.
        String::new()
    } else {
        interface_answers(scan, &java_text, &shown, &crate::questions::methods(&file))
    };
    if !answers.is_empty() {
        let _ = writeln!(
            p,
            "The code above asks blocks it meets questions through the mod's own interfaces. What each block class of \
             the mod answers (the Java of its method, and the blocks registered with that class or one that inherits \
             it), so a neighbour of another kind can be judged from its block and state properties. Write each \
             question as one check over (block, state properties, face) with a case for every class listed, and use \
             it for both blocks of a pair: a block left out of it never connects.\n\n{answers}"
        );
    }
    // (Read from the classes left out for length too: a setting read two calls away.)
    let config = config_values(
        scan,
        &format!(
            "{java_text}
{unseen_java}"
        ),
        &shown,
    );
    if !config.is_empty() {
        let _ = writeln!(
            p,
            "Config values the code above reads, as the mod declares them. A port has no config file, so each \
             one is its default, always: write only the branch the default takes (a value that is `false` by \
             default never runs its `if` side), say in a comment which setting it was, and never guess a \
             default:\n{config}"
        );
    }
    let properties = property_names(scan, &format!("{}\n{java_text}", unit.comment));
    if !properties.is_empty() {
        let named: Vec<String> = properties
            .iter()
            .map(|(constant, name)| format!("`{constant}` is `\"{name}\"`"))
            .collect();
        let _ = writeln!(
            p,
            "A block's state has its properties by name, and the Java names them by constant. These are not their \
             constant in lower case: {}. Read and set each by its name (`\"{}\"`, never `\"{}\"`); every other \
             property the Java names is its constant in lower case.\n",
            named.join(", "),
            properties[0].1,
            properties[0].0.to_ascii_lowercase()
        );
    }
    for (mixin, member) in &unit.members {
        let _ = writeln!(
            p,
            "Port the Mixin member `{member}` of `{mixin}`: only its effect, through this hook's parameters."
        );
    }

    // The Rust around it: the parts of the mod's module it can use.
    match unit.kind {
        Kind::Runtime => {
            let _ = writeln!(
                p,
                "The module it is in, `{}` (the parts this function can use):\n\n```rust\n{}```\n",
                unit.file,
                module_context(&file, &stub, &unit.function, &java_text)
            );
            let _ = writeln!(
                p,
                "This module is plain Rust: it compiles on its own with only `std` (no crates), and `cargo test` runs its tests. \
                 Hook helpers in the game call it. The Java's static fields are already here, as Rust: a `pub mod` per \
                 Java class (`SteadyView.pickups` is `steady_view::PICKUPS`); use them for the state the Java keeps \
                 there, and the struct the function is given for the rest. A slot holds at most the item's own \
                 stack size: an `ItemStackHandler`'s `insertItem`/`extractItem` and a `Container` cap a slot at \
                 `min(getSlotLimit(slot), stack.getMaxStackSize())`, which is `world.max_stack(item)` (1 for a tool), \
                 not 64."
            );
            // A tick that reaches another block entity, where the Java has one of them
            // call what the other registered.
            let reached = reached_keys(&file, &own_java, &stub);
            if !reached.is_empty() {
                let _ = writeln!(
                    p,
                    "Other block entities of the mod this code names, and the key each of their values is \
                     saved under: what `n.get(key)` reads of one (`machine.neighbour(side)`, \
                     `machine.at(pos)`, a click's `world.entity_value`) and what `machine.set_at(pos, key, \
                     value)` or `machine.set_neighbour` changes in one (a `bool` is `Saved::Byte(1)` or \
                     `Saved::Byte(0)`, an `i32` `Saved::Int`, an `f32` `Saved::Float`, a `String` \
                     `Saved::Text`). A flag the Java sets on another block entity \
                     (`other.checker.scheduleUpdate()`) is set this way, by its key:\n{reached}"
                );
            }
            let called = unit.comment.contains("call code this one gave");
            if stub.contains("Machine<'_>") && unit.comment.contains("machine.at(pos)") && !called {
                let _ = writeln!(p, "{CALLBACKS}\n");
            }
            // A tick that looks at its neighbours, where the Java walks further.
            if stub.contains("Machine<'_>") && unit.comment.contains("machine.neighbour(") {
                let connect = if questions.is_empty() {
                    "Whether two blocks connect is in the Java of their block classes (which face has a \
                     shaft, which axis turns): read it from their state properties."
                } else {
                    "Whether two blocks connect, and what one passes on to the other, is what the mod's \
                     own questions and helpers answer. Those are functions of this module already \
                     (below): call them rather than working it out again."
                };
                let _ = writeln!(p, "{NETWORK_STEP} {connect}\n");
            }
            if stub.contains("Machine<'_>") && called {
                let _ = writeln!(p, "{CALLED}\n");
            }
            // The mod's own questions to a block, ported once: said last, where it is
            // read closest to the answer, or a tick writes out its own copy of them.
            if !questions.is_empty() {
                let _ = writeln!(
                    p,
                    "The Java asks the blocks and block entities it meets questions of the mod's own, and \
                     calls helpers of the mod's that take block entities. Each of these is a function of this \
                     module already, ported once. Call it, and do not write your own version of it or your \
                     own table of what each class answers:\n{questions}"
                );
            }
            let hints = equivalents(&java_text, &stub);
            if !hints.is_empty() {
                let _ = writeln!(
                    p,
                    "The Java calls things this function has a way to do. Use them, and write no `// Missing:` note for them:\n{hints}"
                );
            }
        }
        Kind::Helper => {
            let _ = writeln!(p, "Its file, `{}`:\n\n```rust\n{file}```\n", unit.file);
            let runtime = unit
                .file
                .rsplit_once('/')
                .map(|(d, _)| format!("{d}/runtime.rs"))
                .unwrap_or_default();
            if let Ok(rt) = fs::read_to_string(dir.join(&runtime)) {
                let module = rt_module(dir);
                let _ = writeln!(
                    p,
                    "The mod's own module, `{runtime}` (`crate::game::{module}` in the game), which the helper may call (the parts it can use):\n\n```rust\n{}```\n",
                    module_context(&rt, &stub, "", &java_text)
                );
            }
        }
    }
    out.properties = properties;
    out.placed_at = stub.contains("p: &Placement") && java_text.contains("getClickedPos()");
    Ok(out)
}

/// The parts of a mod's module a function can use: everything but the other block
/// entities' state structs and tick stubs (a mod like Create has a hundred), and
/// the module's tests.
fn module_context(module: &str, stub: &str, function: &str, java: &str) -> String {
    // The state structs: `pub struct Crank` under "The state of a ...".
    let mut states = Vec::new();
    let mut prev_state_doc = false;
    for line in module.lines() {
        if let Some(name) = line.strip_prefix("pub struct ")
            && prev_state_doc
        {
            states.push(
                name.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        if !line.starts_with("#[") {
            prev_state_doc = line.starts_with("/// The state of a `");
        }
    }
    let uses = |name: &str| {
        stub.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|w| w == name)
    };
    // Top-level chunks: a blank line followed by a line that is not indented
    // (blank lines inside an `impl` stay in its chunk).
    let mut chunks: Vec<String> = Vec::new();
    let mut after_blank = true;
    for line in module.split_inclusive('\n') {
        let starts_item = after_blank && !line.starts_with([' ', '\t', '\n', '}']);
        if starts_item || chunks.is_empty() {
            chunks.push(String::new());
        }
        if let Some(last) = chunks.last_mut() {
            last.push_str(line);
        }
        after_blank = line.trim().is_empty();
    }
    let mut out = String::new();
    for chunk in chunks.iter().map(|c| c.trim_end_matches('\n')) {
        if chunk.trim().is_empty() {
            continue;
        }
        let first_code = chunk
            .lines()
            .find(|l| !l.starts_with("//") && !l.starts_with("#["))
            .unwrap_or("");
        if chunk.contains("#[cfg(test)]") {
            continue;
        }
        let other_state = states.iter().any(|s| {
            !uses(s)
                && (first_code == format!("pub struct {s} {{")
                    || first_code == format!("impl {s} {{")
                    || chunk.starts_with(&format!("// Left out of {s},")))
        });
        let other_tick = first_code.starts_with("pub fn tick_")
            && !first_code.starts_with(&format!("pub fn {function}("))
            && first_code != "pub fn tick_block_entity(";
        // A Java class's statics, when the Java sent with this function never names it.
        let unused_statics = chunk
            .strip_prefix("/// `")
            .and_then(|r| r.split_once("`'s static fields."))
            .is_some_and(|(class, _)| {
                let simple = class.rsplit('.').next().unwrap_or(class);
                !java
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .any(|w| w == simple)
            });
        if other_state || other_tick || unused_statics {
            continue;
        }
        let is_fn = first_code.starts_with("fn ") || first_code.starts_with("pub fn ");
        let own = first_code.starts_with(&format!("fn {function}("))
            || first_code.starts_with(&format!("pub fn {function}("));
        // The function itself is shown above, as it is to be ported: its last port, where
        // it has one, isn't shown again here (it would be copied, mistakes and all).
        if own {
            continue;
        }
        let long = chunk.lines().count();
        // Another Java class's function (to port, or ported): this one can call it only
        // when its own Java names that class (a subclass calling `super`). A large mod has
        // hundreds; they would be most of the prompt.
        if is_fn && !own {
            let from = chunk
                .lines()
                .filter(|l| l.contains(units::TODO) || l.contains(units::PORTED))
                .flat_map(units::java_classes)
                .collect::<Vec<_>>();
            if !from.is_empty()
                && !from.iter().any(|class| {
                    let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
                    java.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .any(|w| w == simple)
                })
            {
                continue;
            }
            // A generated dispatch (a match over every block): its head says enough.
            if from.is_empty() && long > 60 {
                for line in chunk
                    .lines()
                    .take_while(|l| l.starts_with("//") || l.starts_with("#["))
                {
                    out.push_str(line);
                    out.push('\n');
                }
                let head = first_code.trim_end_matches('{').trim_end();
                let _ = writeln!(out, "{head} {{ /* {long} lines, generated */ }}\n");
                continue;
            }
        }
        // A long list of blocks or items for another function: its length says enough.
        if (first_code.starts_with("const ") || first_code.starts_with("pub const "))
            && first_code.ends_with("&[")
            && long > 14
            && !first_code.starts_with("pub const BLOCK_TAGS:")
            && !first_code.contains(&format!(" {}:", function.to_ascii_uppercase()))
        {
            for line in chunk.lines().take_while(|l| l.starts_with("//")) {
                out.push_str(line);
                out.push('\n');
            }
            let _ = writeln!(out, "{first_code}/* {} entries */];\n", long - 2);
            continue;
        }
        // The tag table: its tags' names say what `in_tag` knows; their blocks would
        // only fill the prompt.
        if first_code.starts_with("pub const BLOCK_TAGS:") {
            let names: Vec<&str> = chunk
                .lines()
                .filter_map(|l| l.trim().strip_prefix("(\"")?.split('"').next())
                .collect();
            for line in chunk.lines().take_while(|l| l.starts_with("///")) {
                out.push_str(line);
                out.push('\n');
            }
            let _ = writeln!(
                out,
                "pub const BLOCK_TAGS: &[(&str, &[&str])] = &[/* {} */];\n",
                names.join(", ")
            );
            continue;
        }
        out.push_str(chunk.trim_end_matches('\n'));
        out.push_str("\n\n");
    }
    out
}

/// The runtime module's name, from the bundle's manifest (`ported_<id>`).
fn rt_module(dir: &Path) -> String {
    rust_files(dir)
        .ok()
        .and_then(|files| {
            files
                .first()
                .and_then(|f| f.parent().map(Path::to_path_buf))
        })
        .and_then(|d| fs::read_to_string(d.join("horizon-mixin.toml")).ok())
        .and_then(|t| {
            t.lines()
                .find(|l| l.trim_start().starts_with("name = \"ported_"))
                .map(|l| l.split('"').nth(1).unwrap_or_default().to_string())
        })
        .unwrap_or_else(|| "ported_mod".into())
}

/// A class of the scan by fully qualified or simple name.
fn find_class<'a>(scan: &'a ModScan, name: &str) -> Option<&'a porter_model::ClassInfo> {
    scan.classes.iter().find(|c| c.name == name).or_else(|| {
        let mut found = scan
            .classes
            .iter()
            .filter(|c| c.name.rsplit('.').next() == Some(name));
        let first = found.next()?;
        found.next().is_none().then_some(first)
    })
}

/// What the Java calls, for each call the function's parameters can do: the call a model
/// overlooks in a long trait, named beside the Java it stands for. A line each.
fn equivalents(java: &str, stub: &str) -> String {
    // (what the stub is given, what the Java says, what to use)
    const TABLE: &[(&str, &str, &str)] = &[
        (
            "p: &Placement",
            "getClickedPos()",
            "`context.getClickedPos()` is `p.pos`: in a `BlockPlaceContext` it is where the block is being placed, whatever its name says, and the blocks beside the new block are counted from it. The block that was clicked is `p.clicked`",
        ),
        (
            "dyn World",
            "awardStat(",
            "`player.awardStat(Stats.ITEM_USED.get(item))` is `world.stat(\"used\", item)`; a custom stat is `world.stat(\"custom\", id)`",
        ),
        (
            "dyn World",
            "getCooldowns()",
            "`player.getCooldowns().addCooldown(stack, ticks)` is `world.cooldown(item, ticks)`",
        ),
        (
            "dyn World",
            ".addEffect(",
            "`player.addEffect(new MobEffectInstance(effect, ticks, amplifier))` is `world.effect(effect, ticks, amplifier)`",
        ),
        (
            "dyn World",
            ".canEat(",
            "`player.canEat(always)` is `world.can_eat(always)`",
        ),
        (
            "dyn World",
            "getPlayerPOVHitResult",
            "`getPlayerPOVHitResult(level, player, fluid)` is `world.ray(u.eye, to)`, `to` the eye plus the look times 5.0",
        ),
        (
            "dyn World",
            ".clip(",
            "`level.clip(new ClipContext(from, to, ..))` is `world.ray(from, to)`",
        ),
        (
            "dyn World",
            "getEntitiesOfClass(",
            "`level.getEntitiesOfClass(X.class, box)` is `world.mobs_in(min, max)`, picked by `kind`",
        ),
        (
            "dyn World",
            "setInLove(",
            "`animal.setInLove(player)` is `world.mob_love(mob.id)`",
        ),
        (
            "dyn World",
            "addFreshEntity(",
            "`level.addFreshEntity(entity)` is `world.summon(kind, at)`, `summon_baby` for a baby, `launch(kind, at, velocity)` for something thrown or shot",
        ),
        (
            "dyn World",
            "levelEvent(",
            "`level.levelEvent(2001, pos, Block.getId(state))` is `world.break_effect(pos, block)`; any other is `world.level_event(id, pos, data)`",
        ),
        (
            "dyn World",
            "gameEvent(",
            "`level.gameEvent(.., GameEvent.X, pos)` is `world.game_event(\"minecraft:x\", pos)`",
        ),
        (
            "dyn World",
            "displayClientMessage(",
            "`player.displayClientMessage(text, true)` is `world.overlay(text)`; with `false`, `world.chat(text)`",
        ),
        (
            "dyn World",
            "sendSystemMessage(",
            "`player.sendSystemMessage(text)` is `world.chat(text)`",
        ),
        (
            "dyn World",
            "destroyBlock(",
            "`level.destroyBlock(pos, drop)` is `world.destroy(pos, drop)`",
        ),
        (
            "dyn World",
            "BlockParticleOption",
            "particles of a block (`new BlockParticleOption(ParticleTypes.BLOCK, state)`) are `world.block_particles(block, at, spread, speed, count)`",
        ),
        (
            "Machine",
            "getEntitiesOfClass(",
            "`level.getEntitiesOfClass(ItemEntity.class, box)` is `machine.dropped_in(min, max)`; of animals or mobs, `machine.mobs_in(min, max)`",
        ),
        (
            "Machine",
            "setInLove(",
            "`animal.setInLove(null)` is `machine.love(mob.id)`",
        ),
        (
            "Machine",
            "addFreshEntity(",
            "`level.addFreshEntity(entity)` is `machine.summon(kind, at)`, `summon_baby` for a baby; an item is `machine.drop(..)`",
        ),
        (
            "Machine",
            "hasNeighborSignal(",
            "`level.hasNeighborSignal(pos)` is `machine.powered`",
        ),
        (
            "dyn World",
            "placeItemBackInInventory(",
            "`player.getInventory().placeItemBackInInventory(stack)` is `world.give(item, count)`, which answers how many fit; Java drops what doesn't at the player's feet, so `world.drop` the rest there",
        ),
        (
            "Machine",
            ".rollResults(",
            "`recipe.rollResults(random)` goes over `recipe.results`, each `(stack, chance)`: a stack with chance 1.0 is given whole; for a lower chance each item of its count is drawn on its own (`machine.random() <= chance`), as `ProcessingOutput.rollOutput` does",
        ),
        (
            "dyn World",
            "withBlockEntityDo(",
            "`withBlockEntityDo(level, pos, be -> be.method(..))` (and `level.getBlockEntity(pos)` then a call on it) can't run the block entity's code from here: do what that method does to its fields with `world.save_entity(pos, &[(key, value)])`, by the keys its state struct saves them under. Where the method goes on to call another of the entity's own (an update its tick also makes), set the field its tick watches for that, so the tick does it on its next turn",
        ),
        // A library's helper whose source the mod doesn't carry (Catnip, which Create
        // and its add-ons draw with): what it does, so the turn isn't a guess.
        (
            "&mut Draw",
            "partialFacing(",
            "`CachedBuffers.partialFacing(model, state)` (Catnip; the facing is the state's `FACING` unless one is passed) is `model`, which is built facing south, turned through the block's centre about Y by `AngleHelper.horizontalAngle(facing)` (south 0, west -90, north 180, east 90, up and down 0) and then about X by `AngleHelper.verticalAngle(facing)` (up -90, down 90, the rest 0): `draw.rotate_about([0.0, 1.0, 0.0], y, [0.5; 3]); draw.rotate_about([1.0, 0.0, 0.0], x, [0.5; 3]); draw.model(..)`. A spin Java gives that buffer afterwards is made before both",
        ),
        (
            "&mut Draw",
            "AngleHelper.horizontalAngle(",
            "`AngleHelper.horizontalAngle(facing)` (Catnip) is south 0, west -90, north 180, east 90, up and down 0, in degrees about Y; `AngleHelper.verticalAngle(facing)` is up -90, down 90, the rest 0, about X",
        ),
        (
            "",
            "canSeeSky(",
            "`level.canSeeSky(pos)` is `world.sees_sky(pos)`",
        ),
        (
            "",
            "isFaceSturdy(",
            "`state.isFaceSturdy(level, pos, face)` is `world.sturdy(pos, face)`",
        ),
        (
            "",
            "canSupportCenter(",
            "`canSupportCenter(level, pos, UP)` is `world.sturdy(pos, \"up\")`",
        ),
        (
            "",
            "canSupportRigidBlock(",
            "`canSupportRigidBlock(level, pos)` is `world.sturdy(pos, \"up\")`",
        ),
        (
            "",
            "canBeReplaced(",
            "`level.getBlockState(pos).canBeReplaced(context)` is `world.replaceable(pos)`",
        ),
        (
            "",
            ".is(BlockTags.",
            "`state.is(BlockTags.X)` is `world.in_tag(pos, \"minecraft:x\")`",
        ),
    ];
    let mut out = String::new();
    for (needs, call, hint) in TABLE {
        // The Blocks queries go by the parameter's name; the rest by its type.
        let has = if needs.is_empty() {
            stub.contains("world:")
        } else {
            stub.contains(needs)
        };
        if has && java.contains(call) {
            let _ = writeln!(out, "- {hint}");
        }
    }
    out
}

/// Whether `text` has `word` as a whole identifier.
fn names_word(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(word).any(|(i, m)| {
        !text[..i].chars().next_back().is_some_and(ident)
            && !text[i + m.len()..].chars().next().is_some_and(ident)
    })
}

/// The most the interface answers add to a prompt, in characters.
const ANSWERS_BUDGET: usize = 24_000;

/// For each method of the mod's own interfaces in `shown` that `java` calls on something
/// (`definition.hasShaftTowards(..)`): the body each of the mod's block classes gives it,
/// with the blocks registered with that class or with one that inherits the answer.
/// Shortest bodies first, within `ANSWERS_BUDGET`.
fn interface_answers(scan: &ModScan, java: &str, shown: &[String], ported: &[String]) -> String {
    use porter_model::ClassKind;
    let class = |name: &str| scan.classes.iter().find(|c| c.name == name);
    let simple = |name: &str| name.rsplit('.').next().unwrap_or(name).to_string();
    let ns = scan
        .main_metadata()
        .and_then(|m| m.id.clone())
        .unwrap_or_else(|| "minecraft".into());
    let mut methods: Vec<(String, String)> = Vec::new();
    // What the code calls on something (`x.name(`) and the words it has, read once: a
    // large mod has hundreds of interfaces to look up in a long text.
    let asked = crate::questions::called_methods(java);
    let words: std::collections::HashSet<&str> = java
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .collect();
    // The interfaces shown, and those the code names (a helper's `IRotate` it wasn't
    // shown for its size).
    for interface in scan.classes.iter().filter(|c| {
        c.kind == ClassKind::Interface
            && !c.is_mixin
            && (shown.contains(&c.name) || words.contains(simple(&c.name).as_str()))
    }) {
        for m in &interface.method_names {
            if ported.contains(m) {
                continue;
            }
            if asked.contains(m.as_str()) && !methods.iter().any(|(_, n)| n == m) {
                methods.push((simple(&interface.name), m.clone()));
            }
        }
    }
    if methods.is_empty() {
        return String::new();
    }
    // Each registered block's class, by simple name as registrations give it.
    let mut blocks: Vec<(String, &porter_model::ClassInfo)> = Vec::new();
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
    let mut entries: Vec<(String, String)> = Vec::new();
    for (interface, method) in &methods {
        // The class that decides it for each block: its own, or the nearest one above.
        let mut by_class: Vec<(&porter_model::ClassInfo, Vec<&str>)> = Vec::new();
        for (id, own) in &blocks {
            let mut next = Some(*own);
            let mut depth = 0;
            while let Some(c) = next {
                if c.kind != ClassKind::Interface && c.method_names.iter().any(|n| n == method) {
                    match by_class.iter_mut().find(|(k, _)| k.name == c.name) {
                        Some((_, ids)) => ids.push(id),
                        None => by_class.push((c, vec![id])),
                    }
                    break;
                }
                depth += 1;
                next = (depth < 12)
                    .then(|| c.superclass.as_deref().and_then(class))
                    .flatten();
            }
        }
        for (c, ids) in by_class {
            let Ok(source) = fs::read_to_string(Path::new(&scan.input).join(&c.path)) else {
                continue;
            };
            let Some(body) =
                crate::questions::method_body(&source, method).filter(|b| b.len() <= 600)
            else {
                continue;
            };
            let mut list = ids
                .iter()
                .take(8)
                .map(|i| format!("`{i}`"))
                .collect::<Vec<_>>()
                .join(", ");
            if ids.len() > 8 {
                let _ = write!(list, " and {} more", ids.len() - 8);
            }
            entries.push((
                format!("{interface}.{method}"),
                format!("- `{}.{method}` ({list}): `{}`", simple(&c.name), body),
            ));
        }
    }
    entries.sort_by_key(|(m, line)| (m.clone(), line.len()));
    let mut out = String::new();
    for (_, line) in entries {
        if out.len() + line.len() > ANSWERS_BUDGET {
            continue;
        }
        let _ = writeln!(out, "{line}");
    }
    out
}

/// Config values `java` reads through a getter (`AllConfigs.server().recipes.bulkPressing
/// .get()`) that a class not shown declares: a line each with the declaration, whose
/// default is the value a port runs with.
fn config_values(scan: &ModScan, java: &str, shown: &[String]) -> String {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut names: Vec<&str> = Vec::new();
    for (at, _) in java.match_indices(".get") {
        // `.name.get()`, `.name.getF()`, `.name.getAsBoolean()`: no arguments.
        let rest = &java[at + 4..];
        let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
        if !rest[end..].starts_with("()") {
            continue;
        }
        let start = java[..at].rfind(|c: char| !word(c)).map_or(0, |i| i + 1);
        let name = &java[start..at];
        if name.is_empty()
            || !name.starts_with(|c: char| c.is_ascii_alphabetic())
            || !java[..start].ends_with('.')
            || names.contains(&name)
        {
            continue;
        }
        names.push(name);
    }
    let mut out = String::new();
    let mut lines = 0;
    for name in names {
        let mut found = scan
            .classes
            .iter()
            .filter(|c| !shown.contains(&c.name))
            .filter_map(|c| {
                let field = c
                    .fields
                    .iter()
                    .find(|f| f.name == name && f.init.is_some())?;
                let config = c.path.to_ascii_lowercase().contains("config")
                    || field.ty.contains("Config")
                    || field.ty.contains("Value");
                config.then_some((c, field))
            });
        let (Some((class, field)), None) = (found.next(), found.next()) else {
            continue;
        };
        let simple = class.name.rsplit(['.', '$']).next().unwrap_or(&class.name);
        let init = field.init.as_deref().unwrap_or_default();
        let default = config_default(init)
            .map(|d| format!(", `{d}` by default"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- `{name}` is `{simple}.{name}`{default}: `{} {name} = {}`",
            field.ty,
            init.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        lines += 1;
        if lines >= 16 {
            break;
        }
    }
    out
}

/// The saved keys of the mod's other block entities whose classes `java` names (its
/// own, one it inherits, or one it keeps part of itself in): a line a block entity,
/// from the state structs of `module`. `stub` is the function, whose own state is left
/// out. So a tick that sets a flag in another block entity knows the key it is under.
fn reached_keys(module: &str, java: &str, stub: &str) -> String {
    let mut out = String::new();
    // Each block entity's id, struct, classes and keys.
    let mut found: Vec<(&str, &str, Vec<&str>, Vec<String>)> = Vec::new();
    let lines: Vec<&str> = module.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let Some(head) = lines[i].strip_prefix("/// The state of a `") else {
            i += 1;
            continue;
        };
        let id = head.split('`').next().unwrap_or_default();
        // Its classes: every dotted name in backticks on that line.
        // (Not the classes it keeps parts of itself in: many block entities share those.)
        let classes: Vec<&str> = head
            .split(" and what it keeps in ")
            .next()
            .unwrap_or(head)
            .split('`')
            .step_by(2)
            .skip(1)
            .filter(|c| c.contains('.'))
            .collect();
        // To its struct, and through its fields.
        let mut j = i + 1;
        while j < lines.len() && !lines[j].starts_with("pub struct ") {
            j += 1;
        }
        let Some(name) = lines
            .get(j)
            .and_then(|l| l.strip_prefix("pub struct "))
            .map(|l| l.trim_end_matches(['{', ' ']))
        else {
            break;
        };
        let mut keys: Vec<String> = Vec::new();
        let mut k = j + 1;
        while k + 1 < lines.len() && lines[k] != "}" {
            let doc = lines[k].trim();
            let field = lines[k + 1].trim();
            if let (Some(doc), Some(field)) = (doc.strip_prefix("/// "), field.strip_prefix("pub "))
            {
                let key = ["saved as `", "kept as `"]
                    .iter()
                    .find_map(|m| doc.split_once(m))
                    .and_then(|(_, rest)| rest.split('`').next());
                let ty = field
                    .split_once(": ")
                    .map_or("", |(_, t)| t.trim_end_matches(','));
                let java_field = doc.split('`').nth(1).unwrap_or_default();
                if let Some(key) = key {
                    keys.push(format!("`{key}` ({ty}, `{java_field}`)"));
                }
                k += 2;
            } else {
                k += 1;
            }
        }
        i = k;
        found.push((id, name, classes, keys));
    }
    // The function's own state, and the classes it is made of: a class it shares with
    // another block entity (a base of both) doesn't name that one.
    let is_own = |name: &str| {
        stub.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|w| w == name)
    };
    let own_classes: Vec<&str> = found
        .iter()
        .filter(|(_, name, _, _)| is_own(name))
        .flat_map(|(_, _, classes, _)| classes.iter().copied())
        .collect();
    // Those the Java names most come first.
    let mentions = |class: &str| {
        let simple = class.rsplit('.').next().unwrap_or(class);
        java.match_indices(simple)
            .filter(|(at, _)| {
                let before = java[..*at].chars().next_back();
                let after = java[at + simple.len()..].chars().next();
                let part =
                    |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                !part(before) && !part(after)
            })
            .count()
    };
    let mut named: Vec<(usize, &str, Vec<&str>, Vec<String>)> = found
        .iter()
        .filter(|(_, name, _, keys)| !is_own(name) && !keys.is_empty())
        .filter_map(|(id, _, classes, keys)| {
            let mut by: Vec<(usize, &str)> = classes
                .iter()
                .filter(|c| !own_classes.contains(c))
                .map(|c| (mentions(c), *c))
                .filter(|(n, _)| *n > 0)
                .collect();
            by.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
            let most = by.first()?.0;
            Some((
                most,
                *id,
                by.into_iter().map(|(_, c)| c).collect(),
                keys.clone(),
            ))
        })
        .collect();
    named.sort_by_key(|(n, _, _, _)| std::cmp::Reverse(*n));
    for (_, id, classes, mut keys) in named.into_iter().take(6) {
        let which: Vec<String> = classes
            .iter()
            .take(2)
            .map(|c| format!("`{}`", c.rsplit('.').next().unwrap_or(c)))
            .collect();
        keys.truncate(40);
        let _ = writeln!(out, "- `{id}` ({}): {}", which.join(", "), keys.join(", "));
    }
    out
}

/// State properties `java` reads or sets by a constant that isn't the property's
/// name in lower case: `state.getValue(HORIZONTAL_FACING)` reads `facing`. The name is
/// the mod's own where it declares the constant (`AXIS_ALONG_FIRST_COORDINATE =
/// BooleanProperty.create("axis_along_first")`), else vanilla's.
fn property_names(scan: &ModScan, java: &str) -> Vec<(String, String)> {
    let constant = |text: &str| -> Option<String> {
        let end = text
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(text.len());
        let name = text[..end].rsplit('.').next().unwrap_or_default();
        let upper = !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_uppercase())
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        upper.then(|| name.to_string())
    };
    let mut found: Vec<String> = Vec::new();
    for call in [
        "getValue(",
        "setValue(",
        "hasProperty(",
        "trySetValue(",
        "getOptionalValue(",
        "cycle(",
    ] {
        for (at, _) in java.match_indices(call) {
            if let Some(name) = constant(&java[at + call.len()..])
                && !found.contains(&name)
            {
                found.push(name);
            }
        }
    }
    // What the mod's own declaration says, followed through a renaming
    // (`FACING = BlockStateProperties.HORIZONTAL_FACING`).
    let declared = |name: &str| -> Option<String> {
        let mut names: Vec<String> = scan
            .classes
            .iter()
            .flat_map(|c| c.fields.iter())
            .filter(|f| f.name == name && f.is_static)
            .filter_map(|f| {
                let init = f.init.as_deref()?;
                if init.contains(".create(\"") {
                    return init.split('"').nth(1).map(str::to_string);
                }
                let other = constant(init.rsplit('.').next()?)?;
                init.contains("Properties.")
                    .then(|| porter_content::vanilla_property_name(&other))
            })
            .collect();
        names.dedup();
        match names.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        }
    };
    found
        .into_iter()
        .filter_map(|c| {
            let name = declared(&c).unwrap_or_else(|| porter_content::vanilla_property_name(&c));
            (name != c.to_ascii_lowercase()).then_some((c, name))
        })
        .collect()
}

/// The default a config value's declaration gives: the first argument that isn't its
/// name or comment (`b(false, "bulk", ..)`, `i(60, 5, "frequency")`, `BUILDER.define(
/// "bulk", true)`, `.defineInRange("reach", 2, 0, 8)`). None where that isn't a plain
/// value.
fn config_default(init: &str) -> Option<&str> {
    // The last call of the chain that has arguments is the one that defines it: of
    // the calls of the chain itself, not of those inside their arguments.
    let mut open = None;
    let mut nested = 0;
    let mut in_text = false;
    for (i, c) in init.char_indices() {
        match c {
            '"' => in_text = !in_text,
            '(' if !in_text => {
                if nested == 0 && !init[i + 1..].trim_start().starts_with(')') {
                    open = Some(i);
                }
                nested += 1;
            }
            ')' if !in_text => nested -= 1,
            _ => {}
        }
    }
    let open = open?;
    let args = &init[open + 1..];
    let mut depth = 0;
    let mut quoted = false;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in args.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => depth += 1,
            ')' | ']' | '}' if !quoted => {
                if depth == 0 {
                    parts.push(args[start..i].trim());
                    start = args.len();
                    break;
                }
                depth -= 1;
            }
            ',' if !quoted && depth == 0 => {
                parts.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        parts.push(args[start..].trim());
    }
    let value = parts
        .into_iter()
        .find(|a| !a.is_empty() && !a.starts_with('"'))?;
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    plain.then_some(value)
}

/// The constants of class `simple` that `code` names (`ModSounds.EGG_HATCH`), each with
/// the first string its declaration in `source` has, the id it is registered under:
/// a line each, `- `EGG_HATCH`: "entity.ostrich.egg_hatch"`.
fn named_constants(simple: &str, source: &str, code: &str) -> String {
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for statement in source.split(';') {
        let Some((head, value)) = statement.split_once(" = ") else {
            continue;
        };
        let name = head
            .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if name.is_empty()
            || !name.starts_with(|c: char| c.is_ascii_uppercase())
            || seen.contains(&name)
            || !code.contains(&format!("{simple}.{name}"))
        {
            continue;
        }
        let Some(id) = value.split('"').nth(1) else {
            continue;
        };
        let plain = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_./:-".contains(c));
        if !plain {
            continue;
        }
        seen.push(name);
        let _ = writeln!(out, "- `{name}`: \"{id}\"");
        if seen.len() >= 80 {
            break;
        }
    }
    out
}

/// The model's instructions.
const SYSTEM: &str = "You port Minecraft Java mods to Horizon, a Rust rewrite of Minecraft 26.3, one function at a time. \
A tool generated the Rust function you are given: its signature, its parameters and the Horizon hook it is wired to are \
right. Write its body so it does what the Java code it names does, through what the function is given.\n\n\
Answer with the whole function, its signature unchanged, in one ```rust block, and nothing else in code blocks. \
Keep the Java's behaviour: its numbers, its order, its edge cases. Where the Java uses something this function \
has no way to reach, do the part you can and say what is missing in a short `// Missing: ...` comment in the body; never invent \
APIs that are not in what you are shown. A working approximation beats doing nothing: when the main effect can be done \
without a detail (an entity thrown without its saved data, a sound at the block instead of the player), do it, note the \
detail, and return what the Java returns when it succeeds. No `unsafe`, no `unwrap` on values that can be absent, no new crates.";

/// The translation state: what happened to each function, for `status` and resuming.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct State {
    units: BTreeMap<String, Record>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Record {
    status: String,
    model: String,
    attempts: usize,
    input_tokens: u64,
    output_tokens: u64,
    at: u64,
    /// `REACH` when it was translated.
    #[serde(default)]
    reach: u32,
}

impl State {
    fn path(dir: &Path) -> PathBuf {
        dir.join(".porter").join("translate.json")
    }

    fn load(dir: &Path) -> State {
        fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: &Path) -> Result<()> {
        let path = Self::path(dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    fn record(&mut self, o: &Outcome, model: &str) {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.units.insert(
            o.unit.key(),
            Record {
                status: describe(&o.status),
                model: model.to_string(),
                attempts: o.attempts,
                input_tokens: o.input_tokens,
                output_tokens: o.output_tokens,
                at,
                reach: REACH,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_list_what_ports_left_out() {
        let dir = std::env::temp_dir().join(format!("porter-notes-{}", std::process::id()));
        let mixins = dir.join("mixins/m");
        fs::create_dir_all(&mixins).unwrap();
        fs::write(
            mixins.join("runtime.rs"),
            "/// Ported by horizon-porter translate (m) from a.B. Review it.\npub fn a(x: i32) -> i32 {\n    // Missing: the stat award.\n    x\n}\n\n/// Ported by horizon-porter translate (m) from a.C. Review it.\npub fn b() {}\n",
        )
        .unwrap();
        assert_eq!(write_notes(&dir).unwrap(), 1);
        let text = fs::read_to_string(dir.join("TRANSLATION-NOTES.md")).unwrap();
        assert!(
            text.contains("### `a`") && text.contains("Missing: the stat award."),
            "{text}"
        );
        assert!(!text.contains("### `b`"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lists_the_saved_keys_of_block_entities_the_java_names() {
        let module = "/// The state of a `demo:mixer` block entity: `net.demo.MixerBlockEntity` and what it inherits from `net.demo.OperatingBlockEntity` and what it keeps in `net.demo.Deferral`'s fields, saved with the block.
#[derive(Clone, Debug, PartialEq)]
pub struct Mixer {
    /// `float speed`, saved as `Speed`.
    pub speed: f32,
    /// `Deferral checker: boolean needsUpdate`, saved as `NeedsUpdate`.
    pub checker_needs_update: bool,
    /// `int lazy`, not saved by the Java mod: kept as `lazy` so it lasts between ticks.
    pub lazy: i32,
}

/// The state of a `demo:basin` block entity: `net.demo.BasinBlockEntity`'s fields, saved with the block.
#[derive(Clone, Debug, PartialEq)]
pub struct Basin {
    /// `boolean changed`, saved as `Changed`.
    pub changed: bool,
}

/// The state of a `demo:lamp` block entity: `net.demo.LampBlockEntity`'s fields, saved with the block.
pub struct Lamp {
    /// `boolean lit`, saved as `Lit`.
    pub lit: bool,
}
";
        let java = "class BasinBlockEntity { void changed() { getOperator().ifPresent(be -> be.checker.scheduleUpdate()); } Optional<OperatingBlockEntity> getOperator() { return null; } }";
        let stub =
            "pub fn tick_basin(pos: [i32; 3], state: &mut Basin, machine: &mut Machine<'_>) {}";
        let keys = reached_keys(module, java, stub);
        // The mixer inherits the class the basin's Java names; the basin's own state and
        // a block entity it doesn't name are left out.
        assert_eq!(
            keys,
            "- `demo:mixer` (`OperatingBlockEntity`): `Speed` (f32, `float speed`), `NeedsUpdate` (bool, `Deferral checker: boolean needsUpdate`), `lazy` (i32, `int lazy`)
"
        );
    }

    #[test]
    fn reads_a_config_values_default() {
        assert_eq!(
            config_default("b(false, \"bulkPressing\", Comments.bulkPressing)"),
            Some("false")
        );
        assert_eq!(
            config_default("i(60, 5, \"frequency\", Comments.f)"),
            Some("60")
        );
        assert_eq!(
            config_default("BUILDER.comment(\"Whole stacks\").define(\"bulk\", true)"),
            Some("true")
        );
        assert_eq!(
            config_default("BUILDER.defineInRange(\"reach\", 2, 0, 8)"),
            Some("2")
        );
        assert_eq!(
            config_default("e(Mode.FAST, \"mode\", \"How it runs\")"),
            Some("Mode.FAST")
        );
        // Not a plain value: nothing to say.
        assert_eq!(config_default("new ArrayList<>()"), None);
        assert_eq!(config_default("list(() -> defaults(), \"names\")"), None);
        // A call among its arguments isn't the one that defines it.
        assert_eq!(
            config_default("b(true, \"enable\", Comments.get(\"enable\"))"),
            Some("true")
        );
    }

    #[test]
    fn finds_a_name_as_a_whole_word() {
        assert!(names_word(
            "return HandCrankBlockEntity.class;",
            "HandCrankBlockEntity"
        ));
        assert!(!names_word(
            "new HandCrankBlockEntityRenderer()",
            "HandCrankBlockEntity"
        ));
        assert!(!names_word(
            "MyHandCrankBlockEntity x;",
            "HandCrankBlockEntity"
        ));
    }

    #[test]
    fn hints_name_what_the_java_calls() {
        let java = "player.awardStat(Stats.ITEM_USED.get(this)); level.canSeeSky(pos);";
        let click = "fn x_use(u: &ItemUse, world: &mut dyn World) -> bool {";
        let h = equivalents(java, click);
        assert!(
            h.contains("world.stat(") && h.contains("world.sees_sky("),
            "{h}"
        );
        // A tick isn't told of what only a click can do.
        let tick = "pub fn tick_x(pos: [i32; 3], state: &mut X, machine: &mut Machine<'_>) {";
        assert!(equivalents(java, tick).is_empty());
    }

    #[test]
    fn names_the_ids_of_a_long_classs_constants() {
        let source = "public class ModSounds {
   public static final Supplier<SoundEvent> EGG_HATCH = register(\"entity.ostrich.egg_hatch\");
   public static final Supplier<SoundEvent> EGG_CRACK = SOUNDS.register(
      \"entity.ostrich.egg_crack\", () -> make());
   public static final int COUNT = 3;
   static void f() { String s = \"x\"; }
}";
        let code = "level.playSound(null, pos, ModSounds.EGG_CRACK.get(), BLOCKS, 0.7F, 1.0F); int n = ModSounds.COUNT;";
        assert_eq!(
            named_constants("ModSounds", source, code),
            "- `EGG_CRACK`: \"entity.ostrich.egg_crack\"
"
        );
    }

    #[test]
    fn refresh_is_for_what_a_newer_reach_gives() {
        // Ported before any reach: every gap is worth another go.
        assert!(reach_grew_for(0, "// Missing: the advancement trigger."));
        // Ported at reach 1: only what reach 2 gives.
        assert!(!reach_grew_for(1, "// Missing: the advancement trigger."));
        assert!(reach_grew_for(
            1,
            "// Missing: `isFaceSturdy(level, below, UP)`."
        ));
        assert!(reach_grew_for(
            1,
            "// `Placement` does not carry the held item."
        ));
        assert!(!reach_grew_for(REACH, "// Missing: `isFaceSturdy`."));
        assert!(reach_grew_for(
            2,
            "// Missing: `level.levelEvent(2001, pos, id)`."
        ));
        assert!(!reach_grew_for(2, "// Missing: `isFaceSturdy`."));
        assert!(reach_grew_for(
            3,
            "// Missing: returning `Blocks.AIR` is a block removal."
        ));
        assert!(!reach_grew_for(
            3,
            "// Missing: `level.levelEvent(2001, pos, id)`."
        ));
        assert!(reach_grew_for(
            7,
            "// Missing: `player.awardStat(Stats.ITEM_USED)`."
        ));
        assert!(!reach_grew_for(
            REACH,
            "// Missing: `player.awardStat(Stats.ITEM_USED)`."
        ));
    }

    #[test]
    fn reads_a_function_from_an_answer() {
        let answer =
            "Here it is:\n```rust\npub fn tick(a: u8) {\n    let b = { a };\n}\n```\nDone.";
        assert_eq!(
            rust_function(answer, "tick").as_deref(),
            Some("pub fn tick(a: u8) {\n    let b = { a };\n}\n")
        );
        assert!(rust_function("no code", "tick").is_none());
    }

    #[test]
    fn marks_ported_functions_for_review() {
        assert_eq!(
            ported_comment("/// TODO: port from a.B.\n", "m"),
            "/// Ported by horizon-porter translate (m) from a.B. Review it.\n"
        );
        assert_eq!(
            ported_comment("/// TODO: port the tick of `a.B`.\n", "m"),
            "/// Ported by horizon-porter translate (m): the tick of `a.B`. Review it.\n"
        );
        assert_eq!(
            ported_comment("// TODO: port into this function:\n// - x\n", "m"),
            "// Ported by horizon-porter translate (m) from these; review it:\n// - x\n"
        );
    }
}
