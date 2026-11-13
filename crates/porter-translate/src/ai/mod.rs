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

mod java;
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
    /// In a dry run: ask the model how many tokens each prompt is (the Claude API
    /// counts for free) rather than working it out from its length.
    pub count: bool,
    /// How many functions are asked at once. Answers are still put in the file and
    /// tested one at a time; the model's time to answer is what runs side by side.
    pub jobs: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            only: None,
            limit: None,
            attempts: 3,
            dry_run: false,
            refresh: false,
            count: false,
            jobs: 1,
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
    /// A dry run: what would be sent. Of its characters, `shared_chars` are the part
    /// functions of its kind share (the same text has the same `shared_key`), which is
    /// read from the cache after the first of them; `tokens` when the model counted.
    Planned {
        prompt_chars: usize,
        shared_chars: usize,
        shared_key: u64,
        tokens: Option<u64>,
    },
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub unit: Unit,
    pub status: Status,
    pub attempts: usize,
    /// Every input token of every try, the cached ones too.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Of the input, what was read from the cache (a tenth of the price).
    pub cached_tokens: u64,
    /// What the tries cost, where the model's prices are known.
    pub dollars: Option<f64>,
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
out with a `// Missing:` note; the work itself is not. This part runs every tick, whatever the rest of the tick did: \
write each Java method you port into this function (a behaviour's `tick()`, each handler) as a nested `fn` of its own \
and call it, so that a `return` of that method leaves the method and not the tick. A `return` in the tick's own body \
before this part skips it.";

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
walk. Where the Java's walk also takes places that aren't beside the block (the diagonals a cogwheel meshes over: \
`addPropagationLocations`, a `BlockPos.betweenClosed` over the cube around it) and this function's comment says \
`machine.at(pos)` reaches them, go over those places too, each `machine.at(pos)` taken as a neighbour is.";

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
            // (One that names a state property no block has, a model the port doesn't
            // bake or a recipe value no file carries is wrong whatever it reached:
            // ported again whenever asked.)
            let misnamed = units::function_source(&source, &unit.function)
                .is_some_and(|f| names_what_is_not_there(&source, &f));
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

/// Characters to a token, about, in a prompt: a mod's Java and the Rust around it are
/// short names and punctuation, and come to fewer characters a token than prose does
/// (measured on Claude Haiku 5.5 over Create's prompts).
pub const CHARS_PER_TOKEN: f64 = 2.3;

/// Tokens that come back for a function, about: its thinking and its answer (measured
/// over Create's first hundred).
pub const OUTPUT_PER_TRY: u64 = 4_000;

/// About how many tokens `chars` characters of a prompt are.
pub fn tokens_of(chars: usize) -> u64 {
    (chars as f64 / CHARS_PER_TOKEN).round() as u64
}

/// What one try of each planned function comes to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Estimate {
    pub functions: usize,
    /// Input tokens, all of them.
    pub tokens: u64,
    /// Of them, those read from the cache: the part functions of a kind share, after
    /// the first of the kind.
    pub cached: u64,
    /// Functions whose prompt is past `model::LONG_PROMPT`, where Claude Haiku 5.5
    /// costs five times as much a token.
    pub long: usize,
    /// Whether the model counted the tokens, or they are worked out from lengths.
    pub counted: bool,
    /// Dollars on the model asked about, when its prices are known.
    pub dollars: Option<f64>,
}

/// What `plan` would send, and what that costs on `model`: each prompt at the prices
/// its own length picks, its shared part read from the cache once a function before it
/// has written it.
pub fn estimate(plan: &[Outcome], model: &str) -> Estimate {
    let mut est = Estimate {
        counted: !plan.is_empty(),
        ..Estimate::default()
    };
    let mut written = std::collections::BTreeSet::new();
    let mut dollars = 0.0;
    let mut priced = true;
    for o in plan {
        let Status::Planned {
            prompt_chars,
            shared_chars,
            shared_key,
            tokens,
        } = o.status
        else {
            continue;
        };
        est.functions += 1;
        est.counted &= tokens.is_some();
        let prompt = tokens.unwrap_or_else(|| tokens_of(prompt_chars));
        let shared = if prompt_chars == 0 {
            0
        } else {
            (prompt as f64 * shared_chars as f64 / prompt_chars as f64) as u64
        };
        let mut usage = model::Usage {
            input: prompt - shared,
            output: OUTPUT_PER_TRY,
            ..model::Usage::default()
        };
        // (Too short a shared part isn't cached at all.)
        if shared < 512 {
            usage.input += shared;
        } else if written.insert(shared_key) {
            usage.cache_write = shared;
        } else {
            usage.cache_read = shared;
        }
        est.tokens += prompt;
        est.cached += usage.cache_read;
        est.long += usize::from(prompt > model::LONG_PROMPT);
        match usage.dollars(model) {
            Some(d) => dollars += d,
            None => priced = false,
        }
    }
    est.dollars = (priced && est.functions > 0).then_some(dollars);
    est
}

/// Input tokens for one try of each planned function.
pub fn planned_tokens(plan: &[Outcome]) -> usize {
    estimate(plan, "").tokens as usize
}

/// Dollars for one try of each planned function on `model`, when its price is known.
pub fn planned_dollars(plan: &[Outcome], model: &str) -> Option<f64> {
    estimate(plan, model).dollars
}

/// Translates the functions left in `dir` (a folder `port` wrote from `scan`'s mod)
/// with `model`, reporting each step to `progress`.
pub fn translate(
    dir: &Path,
    scan: &ModScan,
    model: &dyn Model,
    opts: &Options,
    progress: impl FnMut(&str) + Send,
) -> Result<Vec<Outcome>> {
    let progress = std::sync::Mutex::new(progress);
    let say = |line: &str| {
        if let Ok(mut p) = progress.lock() {
            p(line);
        }
    };
    let mut work = pending(dir)?;
    if opts.refresh {
        work.extend(outdated(dir)?);
    }
    // What `--only` leaves out, kept for the shared functions a ported one calls.
    let mut rest: Vec<Unit> = Vec::new();
    if let Some(only) = &opts.only {
        let (wanted, others) = work.into_iter().partition(|u| {
            // (Several, with commas between: `tick_motor,render_motor`.)
            only.split(',')
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .any(|o| u.function.contains(o))
        });
        work = wanted;
        rest = others;
    }
    if let Some(limit) = opts.limit {
        work.truncate(limit);
    }
    let cargo = dir.join("Cargo.toml").is_file();
    if !opts.dry_run && cargo && work.iter().any(|u| u.kind == Kind::Runtime) {
        say("Checking that the mod's tests pass before translating");
        if let Err(errors) = cargo_test(dir) {
            bail!(
                "`cargo test` fails in {} before anything is translated; fix that first:\n{}",
                dir.display(),
                errors
            );
        }
    }

    // A dry run: each prompt, one after the other.
    if opts.dry_run {
        let mut out = Vec::new();
        for (n, unit) in work.iter().enumerate() {
            say(&format!(
                "[{}/{}] {} ({})",
                n + 1,
                work.len(),
                unit.function,
                unit.file
            ));
            let prompt = prompt(dir, scan, unit)?;
            for m in &prompt.notes {
                say(&format!("  {m}"));
            }
            let tokens = if opts.count {
                model.count(SYSTEM, &prompt.shared, &prompt.text)?
            } else {
                None
            };
            let mut key = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&prompt.shared, &mut key);
            out.push(Outcome {
                unit: unit.clone(),
                status: Status::Planned {
                    prompt_chars: prompt.len(),
                    shared_chars: prompt.shared.len(),
                    shared_key: std::hash::Hasher::finish(&key),
                    tokens,
                },
                attempts: 0,
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                dollars: None,
            });
        }
        return Ok(out);
    }

    // The work, shared by `opts.jobs` workers. Each asks the model for one function at a
    // time; putting an answer in the file and testing it holds `file`, so the file is
    // always one that passes its tests between two of them.
    struct Shared {
        work: std::collections::VecDeque<Unit>,
        rest: Vec<Unit>,
        total: usize,
        started: usize,
        busy: usize,
        state: State,
        out: Vec<Outcome>,
        stop: bool,
        error: Option<anyhow::Error>,
    }
    let shared = std::sync::Mutex::new(Shared {
        total: work.len(),
        work: work.into(),
        rest,
        started: 0,
        busy: 0,
        state: State::load(dir),
        out: Vec::new(),
        stop: false,
        error: None,
    });
    let file = std::sync::Mutex::new(());
    let jobs = opts.jobs.max(1);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let (unit, n, total) = {
                        let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
                        if sh.stop {
                            return;
                        }
                        match sh.work.pop_front() {
                            Some(u) => {
                                sh.started += 1;
                                sh.busy += 1;
                                (u, sh.started, sh.total)
                            }
                            // Another worker may yet add what its function calls.
                            None if sh.busy > 0 => {
                                drop(sh);
                                std::thread::sleep(std::time::Duration::from_millis(200));
                                continue;
                            }
                            None => return,
                        }
                    };
                    // With several at once, each line says whose it is.
                    let tag = |line: &str| -> String {
                        if jobs > 1 {
                            format!("  {}: {}", unit.function, line.trim_start())
                        } else {
                            line.to_string()
                        }
                    };
                    say(&format!("[{n}/{total}] {} ({})", unit.function, unit.file));
                    let built = {
                        let _f = file.lock().unwrap_or_else(|e| e.into_inner());
                        prompt(dir, scan, &unit)
                    };
                    let prompt = match built {
                        Ok(p) => p,
                        Err(e) => {
                            let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
                            sh.error.get_or_insert(e);
                            sh.stop = true;
                            sh.busy -= 1;
                            return;
                        }
                    };
                    for m in &prompt.notes {
                        say(&tag(&format!("  {m}")));
                    }
                    let outcome = translate_one(
                        dir,
                        &unit,
                        &prompt,
                        model,
                        opts,
                        cargo,
                        &file,
                        &|line: &str| say(&tag(line)),
                    );
                    say(&tag(&format!("  {}", describe(&outcome.status))));
                    if let Some(cost) = spent(&outcome) {
                        say(&tag(&format!("  {cost}")));
                    }
                    let stop = matches!(outcome.status, Status::NoAnswer(_));
                    // A function of the mod it calls that is still to port (a question to a
                    // block, a helper over block entities, a tick it shares with others) does
                    // nothing until it is: ported too, though `--only` didn't name it.
                    let body = if matches!(outcome.status, Status::Ported | Status::Unchecked) {
                        let _f = file.lock().unwrap_or_else(|e| e.into_inner());
                        let text = fs::read_to_string(dir.join(&unit.file)).unwrap_or_default();
                        units::function_source(&text, &unit.function).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
                    if !body.is_empty() && !sh.rest.is_empty() {
                        let rest = std::mem::take(&mut sh.rest);
                        let (called, left): (Vec<Unit>, Vec<Unit>) = rest
                            .into_iter()
                            .partition(|u| u.file == unit.file && calls(&body, &u.function));
                        sh.rest = left;
                        for u in called {
                            say(&tag(&format!(
                                "  it calls `{}`, which isn't ported yet: porting that too",
                                u.function
                            )));
                            sh.total += 1;
                            sh.work.push_back(u);
                        }
                    }
                    sh.state.record(&outcome, &model.name());
                    if let Err(e) = sh.state.save(dir) {
                        sh.error.get_or_insert(e);
                        sh.stop = true;
                    }
                    sh.out.push(outcome);
                    sh.busy -= 1;
                    // Every other function would get the same non-answer.
                    if stop {
                        sh.stop = true;
                    }
                }
            });
        }
    });
    let sh = shared.into_inner().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = sh.error {
        return Err(e);
    }
    Ok(sh.out)
}

/// What a function's tries took, where the model says: `61,204 tokens in (23,000 of
/// them from the cache), 3,410 out: $0.008`.
fn spent(o: &Outcome) -> Option<String> {
    let thousands = |n: u64| {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    (o.input_tokens > 0).then(|| {
        format!(
            "{} tokens in{}, {} out{}",
            thousands(o.input_tokens),
            if o.cached_tokens > 0 {
                format!(" ({} of them from the cache)", thousands(o.cached_tokens))
            } else {
                String::new()
            },
            thousands(o.output_tokens),
            o.dollars.map_or(String::new(), |d| format!(": ${d:.3}"))
        )
    })
}

/// A status in a word or two, for progress and summaries.
pub fn describe(status: &Status) -> String {
    match status {
        Status::Ported => "ported: compiles and its tests pass".into(),
        Status::Unchecked => "ported: `horizon-porter build` checks it".into(),
        Status::Failed(why) => format!("left as it was: {why}"),
        Status::NoAnswer(why) => format!("stopped: the model gave no answer: {why}"),
        Status::Planned { prompt_chars, .. } => format!("would send {prompt_chars} characters"),
    }
}

#[allow(clippy::too_many_arguments)]
fn translate_one(
    dir: &Path,
    unit: &Unit,
    prompt: &Prompt,
    model: &dyn Model,
    opts: &Options,
    cargo: bool,
    file: &std::sync::Mutex<()>,
    progress: &dyn Fn(&str),
) -> Outcome {
    let path = dir.join(&unit.file);
    let mut outcome = Outcome {
        unit: unit.clone(),
        status: Status::Failed("no attempt".into()),
        attempts: 0,
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        dollars: None,
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
    let mut asked_which = false;
    let mut asked_format = false;
    let mut asked_returns = false;
    let mut asked_shared = false;
    let mut asked_field = false;
    let mut asked_handed = false;
    let mut asked_pivot = false;
    let mut asked_parts = false;
    let mut gave_up = false;
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
        outcome.cached_tokens += reply.cached_tokens;
        if let Some(d) = reply.dollars {
            outcome.dollars = Some(outcome.dollars.unwrap_or(0.0) + d);
        }
        // Every try at an answer ran out of room: this function's alone, the others go on.
        if let Some(why) = reply.cut_off {
            outcome.status = Status::Failed(why);
            return outcome;
        }
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
        if !gave_up && does_nothing(&function) {
            gave_up = true;
            feedback = format!(
                "Your last answer does nothing: its body is only a note that the Java isn't ported. Nothing else \
                 ports it; this function is where its Java goes. Write what the Java does through what the function \
                 is given, the main path first and in full, and keep `// Missing:` for the single things it has no \
                 way to reach. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if does_nothing(&function) {
            // Twice: left as it was (a port from before stays), not replaced by nothing.
            outcome.status = Status::Failed(
                "the model's answers did nothing but say the Java isn't ported".into(),
            );
            return outcome;
        }
        if !asked_which
            && let Some((entity, block)) = takes_an_entity_for_a_block(&function, &original)
        {
            asked_which = true;
            feedback = format!(
                "Your last answer compares a block's id (`.block`) with `\"{entity}\"`, which is a block entity's \
                 id: no block is called that (the block `{block}` has the block entity `{entity}`), so the \
                 comparison is never true. A view's `entity` is its block entity's id \
                 (`be.entity.as_deref() == Some(\"{entity}\")`), and a tick's own is the one its comment \
                 names. Tell the classes apart by that. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_format
            && original.contains("pub fn pos_text(pos: [i32; 3]) -> String {")
            && let Some(own) = own_position_text(&function)
        {
            asked_format = true;
            feedback = format!(
                "Your last answer writes a position as text its own way (`\"{own}\"`). Every function of this \
                 module keeps one as `x y z`: write it with `pos_text(pos)` and read it with \
                 `text_pos(&text)`, or a block entity's `source` saved here is read as none by the rest. If \
                 that text is not a position kept in a block entity, leave it. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_field && let Some(field) = leaves_out_for_a_field(&function, &original) {
            asked_field = true;
            feedback = format!(
                "Your last answer leaves work out because `{field}` is not kept in the state. That is no reason: \
                 the Java fills `{field}` by looking something up (the Java above shows where it is assigned), \
                 so look it up again at the place the Java reads it, from what the state, the blocks around \
                 (`machine.at(..)`) and the recipes (`machine.recipes(..)`) hold, and then do the work that uses \
                 it in full. Keep `// Missing:` for what truly has no way to be reached. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_handed
            && let Some((field, how, method)) = handed_methods(&unit.comment)
                .into_iter()
                .find(|(_, _, method)| !names_method(&function, method))
        {
            asked_handed = true;
            feedback = format!(
                "Your last answer never runs `{method}`. This block entity keeps `{field}`, made as `{how}`: \
                 each tick that part's own `tick()` runs (its Java is above) and calls `{method}`, a method of \
                 this block entity or a class above it (its Java is above too). No other function does this: \
                 without it, what `{method}` starts never starts. Port the kept part's tick and `{method}` into \
                 this tick, as nested functions, and call them each tick. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_parts && shares_turns_between_parts(&function, &prompt.java) {
            asked_parts = true;
            feedback = format!(
                "Your last answer turns the pose and draws several parts without `draw.push()` and                  `draw.pop()`. In the Java each part is its own buffer (`CachedBuffers.partial(..)`,                  `partialFacing(..)`: a `SuperByteBuffer`), and the turns made on one move only that part.                  Here they move the pose, so a part's turns move every part drawn after it. Draw each part as                  `draw.push()`, its own turns, its `draw.model(..)`, `draw.pop()`. Your answer was:
```rust
{function}
```"
            );
            continue;
        }
        if !asked_pivot && pivots_after_moving(&function, &prompt.java) {
            asked_pivot = true;
            feedback = format!(
                "Your last answer turns the model about a pivot (`rotate_about(.., [0.5, ..])`) after it has \
                 moved the origin with `draw.translate(..)`. The pivot is counted from the origin as it is then, \
                 so the turn throws the model aside (an item drawn far above a depot). Where the Java turns with \
                 `ms.mulPose(..)` after its `translate`, that is `draw.rotate(axis, degrees)`, about the moved \
                 origin; keep `rotate_about` for a turn Java makes centred from the block's corner \
                 (`rotateCentered`). Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_shared && let Some(shared) = skips_its_shared_tick(&unit.comment, &function) {
            asked_shared = true;
            feedback = format!(
                "Your last answer never calls `{shared}`. The comment names it: the tick of the classes above is \
                 ported once, as that function, and a tick that doesn't call it leaves all of their work out (the \
                 block entity takes no speed, counts nothing). Call it the way the comment shows, where the Java \
                 calls `super.tick()`, and port none of those classes' tick yourself. Your answer was:\n```rust\n{function}\n```"
            );
            continue;
        }
        if !asked_returns
            && unit.comment.contains("call code this one gave")
            && returns_early(&function)
        {
            asked_returns = true;
            feedback = format!(
                "Your last answer has a `return` in the tick's own body (outside any nested `fn`). In Java that \
                 `return` leaves one method (a behaviour's `tick()`); here it leaves the whole tick, and whatever \
                 comes after it never runs for that tick: the work other block entities call into this one for \
                 has to run every tick. Put each Java method in a nested `fn` (its `return`s stay its own) and \
                 call them one after the other from the tick's body, which then has no `return` before its last \
                 part. Your answer was:\n```rust\n{function}\n```"
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
        if !prompt.recipe_values.is_empty()
            && let Some(wrong) = read_recipe_values(&function)
                .into_iter()
                .find(|key| !prompt.recipe_values.contains(key))
        {
            let mut known = prompt.recipe_values.clone();
            known.sort();
            known.dedup();
            let known: Vec<String> = known.iter().map(|k| format!("`{k}`")).collect();
            feedback = format!(
                "Your last answer reads a recipe's value `\"{wrong}\"`, which no recipe file of this mod has: it \
                 would read nothing. The values they carry are {}. Use the name the files have, whatever the Java's \
                 getter is called. Your answer was:\n```rust\n{function}\n```",
                known.join(", ")
            );
            continue;
        }
        if !prompt.models.is_empty()
            && let Some(wrong) = drawn_models(&function)
                .into_iter()
                .find(|id| !prompt.models.contains(id))
        {
            // Those that share its last word, or its words, are what was meant.
            let last = wrong.rsplit(['/', ':']).next().unwrap_or(&wrong);
            let words: Vec<&str> = last.split('_').filter(|w| w.len() > 2).collect();
            let mut near: Vec<&String> = prompt
                .models
                .iter()
                .filter(|m| words.iter().all(|w| m.contains(w)))
                .collect();
            near.truncate(8);
            let near: Vec<String> = near.iter().map(|m| format!("`{m}`")).collect();
            feedback = format!(
                "Your last answer draws the model `\"{wrong}\"`, which this port doesn't have: it would draw \
                 nothing. A model is drawn by the id it is baked under{}. Use the ids the Java's constants stand \
                 for (named above), never one made up from a constant's name. Your answer was:\n```rust\n{function}\n```",
                if near.is_empty() {
                    String::new()
                } else {
                    format!("; those close to it are {}", near.join(", "))
                }
            );
            continue;
        }
        if let Some((wrong, right)) = prompt.lang.check(&function) {
            feedback = format!(
                "Your last answer names the lang key `\"{wrong}\"`, which this mod's lang files don't have: it \
                 would show as that text. {right} Your answer was:\n```rust\n{function}\n```"
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
        // Into the file as it is now (other functions may have been ported meanwhile),
        // tested, and taken out again if it fails: one answer at a time.
        let _held = file.lock().unwrap_or_else(|e| e.into_inner());
        let current = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                outcome.status = Status::Failed(format!("reading {}: {e}", unit.file));
                return outcome;
            }
        };
        let Some(updated) = units::replace_function(&current, &unit.function, &comment, &function)
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
                let _ = fs::write(&path, &current);
                drop(_held);
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
    outcome.status = Status::Failed(format!(
        "no version compiled and passed its tests in {} tries",
        outcome.attempts
    ));
    outcome
}

/// Whether an answer gave up: a body with no statement of its own (what the stub had:
/// `let _ = (..);`, a plain value handed back) under a note that the Java isn't
/// ported. It compiles and passes the tests the stub passed, and ports nothing. (A
/// function with nothing to do says no such thing: an empty Java method is ported as
/// an empty body without a `Missing` note.)
fn does_nothing(function: &str) -> bool {
    let Some((_, body)) = function.split_once('{') else {
        return false;
    };
    let plain = [
        "false",
        "true",
        "0",
        "0.0",
        "1",
        "None",
        "Vec::new()",
        "String::new()",
        "Default::default()",
        "todo!()",
        "unimplemented!()",
        "()",
    ];
    let mut noted = false;
    for line in body.lines().map(str::trim) {
        if line.starts_with("//") {
            noted |= line.contains("Missing")
                || line.to_ascii_lowercase().contains("not ported")
                || line.to_ascii_lowercase().contains("not been ported");
            continue;
        }
        let code = line
            .trim_end_matches(';')
            .trim_start_matches("return ")
            .trim();
        if line.is_empty() || line == "}" || line.starts_with("let _ =") || plain.contains(&code) {
            continue;
        }
        return false;
    }
    noted
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
    /// For a function that draws: the ids of the models the port bakes on their own
    /// (`STANDALONE_MODELS`). An answer that draws another (`draw.model("mod:block/
    /// crank_handle")`, guessed from a constant's name) draws nothing, and is sent back.
    pub models: Vec<String>,
    /// The plain values the mod's recipe files carry (`RECIPE_VALUES`), all types
    /// together: an answer that reads another (`recipe.number("processingTime")`
    /// where the files say `processing_time`) reads nothing, and is sent back.
    pub recipe_values: Vec<String>,
    /// The mod's own lang keys (`content/assets/<ns>/lang/en_us.json`), by namespace: an
    /// answer that names a key of the mod's it doesn't have (made up from a Java name,
    /// or `CreateLang`'s key without its `create.`) shows that key as text, and is sent
    /// back.
    pub lang: std::sync::Arc<Lang>,
    /// Whether it is a placement whose Java asks where the block goes
    /// (`context.getClickedPos()`): an answer that only ever reads the block clicked
    /// is asked once whether it meant that.
    pub placed_at: bool,
    /// The Java the function is ported from, with the classes shown beside it: what an
    /// answer's checks compare it with.
    pub java: String,
}

impl Prompt {
    pub fn len(&self) -> usize {
        self.shared.len() + self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The longest prompt it writes when it has a choice, in tokens as `tokens_of` works
/// them out: short of `model::LONG_PROMPT` by what that guess can be off.
const PROMPT_ROOM: u64 = 90_000;

/// The prompt for one function: with as much of the classes its Java uses as fits
/// under the length where a token starts to cost five times as much.
fn prompt(dir: &Path, scan: &ModScan, unit: &Unit) -> Result<Prompt> {
    let mut budget = USED_BUDGET;
    loop {
        let mut out = prompt_with(dir, scan, unit, budget)?;
        if tokens_of(out.len()) <= PROMPT_ROOM || budget == 0 {
            if budget < USED_BUDGET {
                out.notes.push(format!(
                    "the classes it uses are cut to {budget} characters: with more, the prompt is past {} tokens, where a token costs five times as much",
                    model::LONG_PROMPT
                ));
            }
            return Ok(out);
        }
        budget = if budget > 8_000 { budget / 2 } else { 0 };
    }
}

/// The prompt for one function, with up to `used_budget` characters of the mod's
/// classes its Java uses.
fn prompt_with(dir: &Path, scan: &ModScan, unit: &Unit, used_budget: usize) -> Result<Prompt> {
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
        // (Shown without its imports: nothing a port reads.)
        let shown = java::trimmed(&source);
        if java_chars + shown.len() > JAVA_BUDGET {
            out.notes.push(format!(
                "{} left out: the Java for this function is past {JAVA_BUDGET} characters",
                class.name
            ));
            continue;
        }
        java_chars += shown.len();
        let _ = writeln!(
            p,
            "The Java class `{}` ({}):\n\n```java\n{shown}\n```\n",
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
    // What is shown so far calls into the next class: the code itself, and the
    // methods of the classes before it that the code reaches.
    let mut calling = own_java.clone();
    // (What a kept part was made with, from the function's comment: `new
    // DeferralBehaviour(this, this::updateBasin)` reaches `updateBasin`.)
    for (_, how, _) in handed_methods(&unit.comment) {
        calling.push('\n');
        calling.push_str(&how);
    }
    let mut used: std::collections::VecDeque<(&porter_model::ClassInfo, String)> = used.into();
    // (Classes a shown class hands work to, found as they are shown.)
    let mut followed: Vec<String> = Vec::new();
    while let Some((class, source)) = used.pop_front() {
        // Of a class the code uses, the methods it reaches; the rest by their
        // signature alone.
        let cut = java::sliced(&java::trimmed(&source), &calling);
        if used_chars + cut.len() > used_budget || java_chars + cut.len() > JAVA_BUDGET {
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
        used_chars += cut.len();
        java_chars += cut.len();
        shown.push(class.name.clone());
        let _ = writeln!(
            p,
            "A class of the mod that code uses, `{}` ({}), for what its calls do{}:\n\n```java\n{cut}\n```\n",
            class.name,
            class.path,
            if cut.contains("{ /* not shown */ }") {
                " (the methods the code above doesn't reach are shown by their signature alone)"
            } else {
                ""
            }
        );
        // What the methods shown hand their work to (`BasinRecipe.apply(basin, recipe)`
        // from a base class's `applyBasinRecipe()`) is shown next: without it the port of
        // that call is a `Missing` note.
        for receiver in static_receivers(&cut) {
            let mut of_name = scan.classes.iter().filter(|c| {
                !c.is_mixin && c.name.rsplit(['.', '$']).next() == Some(receiver.as_str())
            });
            let (Some(next), None) = (of_name.next(), of_name.next()) else {
                continue;
            };
            if shown.contains(&next.name)
                || followed.contains(&next.name)
                || used.iter().any(|(c, _)| c.name == next.name)
            {
                continue;
            }
            if let Ok(text) = fs::read_to_string(Path::new(&scan.input).join(&next.path)) {
                followed.push(next.name.clone());
                used.push_front((next, text));
            }
        }
        calling.push_str(&cut);
        java_text.push_str(&cut);
    }
    // What the mod's blocks answer for its own interfaces (`IRotate.hasShaftTowards`),
    // where the code asks a block it meets: they are in classes this function isn't from.
    // Those the module already has a function for are named instead: ported once.
    // (Its own comment counts as its Java here: a question's quotes what each class
    // answers, and those answers call other questions: `clockwise()`.)
    let questions = crate::questions::ported(
        &file,
        &format!(
            "{java_text}
{}",
            unit.comment
        ),
        &unit.function,
    );
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
    let models = if stub.contains("draw: &mut Draw") {
        baked_models(&file)
    } else {
        Vec::new()
    };
    let named = model_constants(scan, &format!("{java_text}\n{unseen_java}"), &models);
    if !named.is_empty() {
        let list: Vec<String> = named
            .iter()
            .map(|(constant, id)| format!("`{constant}` is `\"{id}\"`"))
            .collect();
        let _ = writeln!(
            p,
            "The models the Java names by a constant, each by the id it is baked under here, which is what \
             `draw.model(id, tint)` takes: {}. A model id made up from a constant's name draws nothing.\n",
            list.join(", ")
        );
    }
    let recipe_values = recipe_values_of(&file);
    if !recipe_values.is_empty()
        && (stub.contains("machine: &mut Machine") || stub.contains("dyn World"))
    {
        // Those of the types the Java names (`MillingRecipe`, `AllRecipeTypes.MILLING`).
        let lower = java_text.to_ascii_lowercase();
        let named: Vec<String> = recipe_values
            .iter()
            .filter(|(kind, _)| {
                let name = kind
                    .rsplit([':', '/'])
                    .next()
                    .unwrap_or(kind)
                    .replace('_', "");
                !name.is_empty() && lower.contains(&name)
            })
            .map(|(kind, keys)| {
                let keys: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
                format!("`{kind}` has {}", keys.join(", "))
            })
            .collect();
        if !named.is_empty() {
            let _ = writeln!(
                p,
                "The plain values the mod's recipe files carry, under the names the files have, which is what \
                 `recipe.number(key)` reads (a Java getter may be named otherwise: `getProcessingDuration()` reads \
                 `processing_time`): {}.\n",
                named.join("; ")
            );
        }
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
            let (common, own) = module_context(&file, &stub, &unit.function, &java_text);
            if !common.is_empty() {
                // The same text for every function with this one's parameters: sent
                // first, and read from the cache after the first of them.
                let _ = writeln!(
                    out.shared,
                    "The functions to port are in `{}`, the module of a Minecraft mod's Horizon port. These parts of \
                     it are there for every function with the same parameters as the one to port: its types and \
                     what they can do, and the functions and tables the port generated. The function, its Java \
                     and the parts of the module that are its own come after.\n\n```rust\n{common}```\n",
                    unit.file
                );
            }
            let _ = writeln!(
                p,
                "The module it is in, `{}` ({}):\n\n```rust\n{own}```\n",
                unit.file,
                if common.is_empty() {
                    "the parts this function can use"
                } else {
                    "the parts for this function; its types and generated functions came first, before this"
                }
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
            // A question to a block entity: the saved keys of each class that answers.
            let asked = asked_keys(&file, &unit.comment);
            if !asked.is_empty() {
                let _ = writeln!(
                    p,
                    "The block entities whose classes answer, each by its id (which is `be.entity`, not \
                     `be.block`), with the key each of its values is saved under: what `be.get(key)` reads when \
                     the view is of that one. A field an answer's Java reads (`inUse`, `running`) is under its \
                     key here; write `// Missing:` only for what has no key:\n{asked}"
                );
            }
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
            // A click or a placement that tells a block entity to do what only its
            // tick can: the switch Java's own tick reads for it.
            if stub.contains("Machine<'_>") && unit.comment.contains("is ported once, as") {
                // A tick around a shared one: what its own class calls of the classes
                // above (`updateGeneratedRotation()` from `initialize`) is theirs to run.
                let switches = tick_switches(scan, &file, &own_java, true);
                if !switches.is_empty() {
                    let _ = writeln!(
                        p,
                        "A method of the classes above that this class's own code calls is not a function here: \
                         the tick ported once runs it. Java's own tick has a switch for these, so where this \
                         class calls one, set the switch on `state` and the tick ported once runs it on the next \
                         tick. Never leave such a call out as `Missing`:\n{switches}"
                    );
                }
            }
            if !stub.contains("Machine<'_>") {
                let switches = tick_switches(scan, &file, &java_text, false);
                if !switches.is_empty() {
                    let _ = writeln!(
                        p,
                        "This function can set a block entity's saved values, and can't run a method of it that \
                         needs its tick (the blocks around it, its network). Where the Java above calls one of \
                         these on a block entity, set the fields the method itself sets and the switch Java's \
                         own tick reads, which runs the rest on the entity's next tick. Use that key and no \
                         other in its place (a key the tick doesn't read does nothing):\n{switches}"
                    );
                }
            }
            let called = unit.comment.contains("call code this one gave");
            if stub.contains("Machine<'_>") && unit.comment.contains("machine.at(pos)") && !called {
                let _ = writeln!(p, "{CALLBACKS}\n");
            }
            // A tick that looks at its neighbours, where the Java walks further.
            // (Not one whose walk is the shared tick's above it.)
            if stub.contains("Machine<'_>")
                && unit.comment.contains("machine.neighbour(")
                && !unit.comment.contains("is ported once, as")
            {
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
            let mut hints = equivalents(&java_text, &stub);
            // A function called while drawing that isn't handed the frame has its clock.
            if file.contains("pub fn render_time() -> f32")
                && !stub.contains("frame: &Frame")
                && ["getRenderTime(", "getGameTime()", "getTicks("]
                    .iter()
                    .any(|c| java_text.contains(c))
            {
                hints.push_str(
                    "- the client's clock while drawing (`AnimationTickHolder.getRenderTime(level)`, \
                     `level.getGameTime() + partialTicks`) is `render_time()`, in ticks and smooth between \
                     them: this function isn't handed the frame, and reads its time there\n",
                );
            }
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
                    {
                        let (common, own) = module_context(&rt, &stub, "", &java_text);
                        format!("{common}{own}")
                    }
                );
            }
        }
    }
    out.properties = properties;
    out.lang = mod_lang(dir);
    out.recipe_values = recipe_values
        .into_iter()
        .flat_map(|(_, keys)| keys)
        .collect();
    out.models = models;
    out.placed_at = stub.contains("p: &Placement") && java_text.contains("getClickedPos()");
    out.java = java_text;
    Ok(out)
}

/// A module this long (Create's is a megabyte) is cut to what the function can reach:
/// below it, everything is shown, as it always was.
const LARGE_MODULE: usize = 150_000;

/// The parts of a mod's module a function can use: everything but the other block
/// entities' state structs and tick stubs (a mod like Create has a hundred), and
/// the module's tests. Of a large module, also only the types the function's own
/// reach (from its signature and comment, through what those types name), and other
/// functions by their comment and signature alone: a prompt over 100,000 tokens costs
/// five times as much a token on the model `translate` uses, and reads no better.
///
/// Two texts: of a large module, first the parts every function with the same
/// parameters is shown, word for word (the types those parameters reach, generated
/// functions and tables), which the model's cache holds between one function and the
/// next; then the parts for this function alone.
fn module_context(module: &str, stub: &str, function: &str, java: &str) -> (String, String) {
    let large = module.len() > LARGE_MODULE;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
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
    let first_code_of = |chunk: &str| -> String {
        chunk
            .lines()
            .find(|l| !l.starts_with("//") && !l.starts_with("#["))
            .unwrap_or("")
            .to_string()
    };
    // The type a chunk defines or gives methods to: `Machine` for `pub struct Machine<'a>`,
    // `impl<'a> Machine<'a>` and `impl Default for Machine`.
    let type_of = |first: &str| -> Option<String> {
        let first = first.trim_start_matches("pub ");
        let rest = ["struct ", "enum ", "trait ", "type "]
            .iter()
            .find_map(|k| first.strip_prefix(k))
            .or_else(|| {
                let after = first.strip_prefix("impl")?;
                // Past its own generics, and to the type a trait is implemented for.
                let after = match after.strip_prefix('<') {
                    Some(g) => g.split_once('>').map_or(g, |(_, r)| r),
                    None => after,
                };
                Some(after.split_once(" for ").map_or(after, |(_, t)| t))
            })?;
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| ident(*c))
            .collect();
        (!name.is_empty()).then_some(name)
    };
    // The state structs, and the parts of them shared ticks work on: `pub struct Crank`
    // under a comment that says so.
    let mut states = Vec::new();
    for chunk in &chunks {
        let doc: Vec<&str> = chunk.lines().take_while(|l| l.starts_with("///")).collect();
        let is_state = doc.iter().any(|l| {
            l.starts_with("/// The state of a `")
                || l.starts_with("/// The part of a block entity's state that")
        });
        if let (true, Some(name)) = (is_state, type_of(&first_code_of(chunk))) {
            states.push(name);
        }
    }
    // (The words of each, once: a large module asks about thousands of names.)
    let stub_words: std::collections::HashSet<&str> = stub.split(|c: char| !ident(c)).collect();
    let java_words: std::collections::HashSet<&str> = java.split(|c: char| !ident(c)).collect();
    let uses = |name: &str| stub_words.contains(name);
    let names_class = |class: &str| {
        let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
        java_words.contains(simple)
    };
    // Whether another function is one this one may call: generated, or from a class its
    // Java names (a subclass calling `super`). A large mod has hundreds of the rest.
    // (And not one that needs what this one isn't given: a block click's function takes
    // the `World`, which a tick has none of to hand it.)
    // (Its parameters, which is what it has to hand on: not what its body names, which
    // differs from one function to the next of the same kind.)
    let stub_head = stub
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ");
    let stub_head = stub_head.split('{').next().unwrap_or("").to_string();
    let callable = |chunk: &str| {
        let from = chunk
            .lines()
            .filter(|l| l.contains(units::TODO) || l.contains(units::PORTED))
            .flat_map(units::java_classes)
            .collect::<Vec<_>>();
        let head = first_code_of(chunk);
        let handed = ["dyn ", "&mut "].iter().all(|kind| {
            head.match_indices(kind).all(|(at, k)| {
                let name: String = head[at + k.len()..]
                    .chars()
                    .take_while(|c| ident(*c))
                    .collect();
                !name.starts_with(|c: char| c.is_ascii_uppercase())
                    || stub_head.split(|c: char| !ident(c)).any(|w| w == name)
            })
        });
        (
            from.is_empty(),
            handed && (from.is_empty() || from.iter().any(|c| names_class(c))),
        )
    };
    // In a large module, the types the function reaches: those its own text names, those
    // in the signatures of functions it may call, and those their definitions name.
    let reached: Option<std::collections::BTreeSet<String>> = large.then(|| {
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| !ident(c))
                .filter(|w| w.starts_with(|c: char| c.is_ascii_uppercase()))
                .map(str::to_string)
                .collect()
        };
        let code = |chunk: &str| -> String {
            chunk
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut reached: std::collections::BTreeSet<String> = words(stub).into_iter().collect();
        for chunk in &chunks {
            let first = first_code_of(chunk);
            if (first.starts_with("fn ") || first.starts_with("pub fn ")) && callable(chunk).1 {
                reached.extend(words(&first));
            }
        }
        loop {
            let before = reached.len();
            for chunk in &chunks {
                let first = first_code_of(chunk);
                if type_of(&first).is_some_and(|t| reached.contains(&t)) {
                    reached.extend(words(&code(chunk)));
                }
            }
            if reached.len() == before {
                break;
            }
        }
        reached
    });
    // Of them, the types every function with these parameters reaches: from the
    // parameters alone (not the state it is handed, which is its own), through the
    // generated functions it may call and what those types name.
    let common_reach: Option<std::collections::BTreeSet<String>> = large.then(|| {
        let words = |text: &str| -> Vec<String> {
            text.split(|c: char| !ident(c))
                .filter(|w| w.starts_with(|c: char| c.is_ascii_uppercase()))
                .filter(|w| !states.iter().any(|s| s == w))
                .map(str::to_string)
                .collect()
        };
        let mut reach: std::collections::BTreeSet<String> = words(&stub_head).into_iter().collect();
        for chunk in &chunks {
            let first = first_code_of(chunk);
            if (first.starts_with("fn ") || first.starts_with("pub fn "))
                && callable(chunk) == (true, true)
            {
                reach.extend(words(&first));
            }
        }
        loop {
            let before = reach.len();
            for chunk in &chunks {
                let first = first_code_of(chunk);
                if type_of(&first).is_some_and(|t| reach.contains(&t)) {
                    let code: Vec<&str> = chunk
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("//"))
                        .collect();
                    reach.extend(words(&code.join("\n")));
                }
            }
            if reach.len() == before {
                break;
            }
        }
        reach
    });
    let mut common_out = String::new();
    let mut own_out = String::new();
    for chunk in chunks.iter().map(|c| c.trim_end_matches('\n')) {
        if chunk.trim().is_empty() {
            continue;
        }
        let first_code = first_code_of(chunk);
        let first_code = first_code.as_str();
        if chunk.contains("#[cfg(test)]") {
            continue;
        }
        let defines = type_of(first_code);
        // Whether it is shown to every function of this kind the same: a type their
        // parameters reach, a generated function, a public table or value. What the
        // function's own name, state or Java picks is its own.
        let common = match (&common_reach, &defines) {
            (None, _) => false,
            (Some(reach), Some(name)) => reach.contains(name),
            (Some(_), None)
                if first_code.starts_with("fn ") || first_code.starts_with("pub fn ") =>
            {
                callable(chunk) == (true, true)
            }
            (Some(_), None) => {
                !(chunk.starts_with("/// `") && chunk.contains("`'s static fields."))
                    && !chunk.starts_with("// Left out of ")
                    && !first_code.starts_with("const ")
                    && !first_code.starts_with("pub const BLOCK_TAGS:")
                    && !(!function.is_empty()
                        && first_code.contains(&format!(" {}:", function.to_ascii_uppercase())))
            }
        };
        let out = if common {
            &mut common_out
        } else {
            &mut own_out
        };
        let other_state = states.iter().any(|s| {
            !uses(s)
                && (defines.as_deref() == Some(s.as_str())
                    || chunk.starts_with(&format!("// Left out of {s},")))
        });
        let other_tick = first_code.starts_with("pub fn tick_")
            && !first_code.starts_with(&format!("pub fn {function}("))
            && first_code != "pub fn tick_block_entity(";
        // A Java class's statics, when the Java sent with this function never names it.
        let unused_statics = chunk
            .strip_prefix("/// `")
            .and_then(|r| r.split_once("`'s static fields."))
            .is_some_and(|(class, _)| !names_class(class));
        // A type the function can't reach (a block click's `World` for a tick).
        let out_of_reach = match (&reached, &defines) {
            (Some(reached), Some(name)) => !reached.contains(name),
            _ => false,
        };
        if other_state || other_tick || unused_statics || out_of_reach {
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
        if is_fn {
            let (generated, may_call) = callable(chunk);
            if !may_call {
                continue;
            }
            // Its comment and signature say what a caller needs: a generated dispatch (a
            // match over every block) always, and in a large module any long function.
            let body = chunk
                .lines()
                .skip_while(|l| l.starts_with("//") || l.starts_with("#["))
                .count();
            // (Of a function ported from the mod's Java, in a large module, the first
            // paragraph of its comment: what it answers and what it is handed. What each
            // class answers, after it, is for porting that function, not for calling it.)
            let caller_reads = large && !generated;
            if (generated && long > 60) || (large && body > 12) || caller_reads {
                for line in chunk
                    .lines()
                    .take_while(|l| l.starts_with("//") || l.starts_with("#["))
                    .take_while(|l| !(caller_reads && l.trim_end() == "///"))
                {
                    out.push_str(line);
                    out.push('\n');
                }
                let head = first_code.trim_end_matches('{').trim_end();
                let what = if generated {
                    "generated"
                } else if chunk.contains(units::TODO) {
                    "ported with this one where it calls it"
                } else {
                    "ported"
                };
                let _ = writeln!(out, "{head} {{ /* {body} lines, {what} */ }}\n");
                continue;
            }
        }
        // A table a generated function of the module looks things up in (which blocks a
        // click's function is for): nothing this function names, in a large module.
        if large && first_code.starts_with("const ") {
            let name: String = first_code["const ".len()..]
                .chars()
                .take_while(|c| ident(*c))
                .collect();
            if !uses(&name) {
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
        // only fill the prompt. (In a large module, not even the names where the Java
        // asks about no tag.)
        if first_code.starts_with("pub const BLOCK_TAGS:") && large && !java.contains("Tags.") {
            let _ = writeln!(
                out,
                "pub const BLOCK_TAGS: &[(&str, &[&str])] = &[/* {} tags */];\n",
                long.saturating_sub(2)
            );
            continue;
        }
        if first_code.starts_with("pub const BLOCK_TAGS:") {
            let all: Vec<&str> = chunk
                .lines()
                .filter_map(|l| l.trim().strip_prefix("(\"")?.split('"').next())
                .collect();
            // Of a large module's, those the Java names by their constant
            // (`AllBlockTags.SAFE_NBT` is `create:safe_nbt`); all of them where it
            // names none that way (a tag built from a string).
            let lower = java.to_ascii_lowercase();
            let named: Vec<&str> = all
                .iter()
                .copied()
                .filter(|tag| {
                    let last = tag.rsplit([':', '/']).next().unwrap_or(tag);
                    last.len() > 2 && lower.contains(last)
                })
                .collect();
            let names = if large && !named.is_empty() && named.len() < all.len() {
                let _ = writeln!(
                    out,
                    "// ({} tags in all; these are the ones the Java's constants can mean.)",
                    all.len()
                );
                named
            } else {
                all
            };
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
        // The function's own state: what loads and saves it (a line a field, which the
        // struct's own comments already say) by its signature, in a large module.
        if large
            && first_code.starts_with("impl ")
            && defines.as_ref().is_some_and(|t| states.contains(t))
        {
            out.push_str(&short_methods(chunk));
            out.push_str("\n\n");
            continue;
        }
        out.push_str(chunk.trim_end_matches('\n'));
        out.push_str("\n\n");
    }
    (common_out, own_out)
}

/// An `impl` block with each method longer than a dozen lines cut to its signature.
fn short_methods(chunk: &str) -> String {
    let lines: Vec<&str> = chunk.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let starts = line.starts_with("    pub fn ") || line.starts_with("    fn ");
        // Where its signature ends (the line that opens the body) and where it closes.
        let open = starts
            .then(|| (i..lines.len()).find(|&j| lines[j].trim_end().ends_with('{')))
            .flatten();
        let close = open.and_then(|o| (o + 1..lines.len()).find(|&j| lines[j] == "    }"));
        match (open, close) {
            (Some(open), Some(close)) if close - open > 12 => {
                for l in &lines[i..open] {
                    out.push_str(l);
                    out.push('\n');
                }
                let head = lines[open].trim_end().trim_end_matches('{').trim_end();
                let _ = writeln!(out, "{head} {{ /* {} lines */ }}", close - open - 1);
                i = close + 1;
            }
            _ => {
                out.push_str(line);
                out.push('\n');
                i += 1;
            }
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// The switches a block entity's own `tick()` reads, for the methods `java` calls:
/// `if (reActivateSource) { updateGeneratedRotation(); reActivateSource = false; }` makes
/// `reActivateSource` the way to have `updateGeneratedRotation()` run on its next tick.
/// From the block entity classes `java` names and the classes above them; each with the
/// key `module` keeps the field under.
fn tick_switches(scan: &ModScan, module: &str, java: &str, on_state: bool) -> String {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let simple = |name: &str| name.rsplit(['.', '$']).next().unwrap_or(name).to_string();
    let mut out = String::new();
    let mut seen: Vec<String> = Vec::new();
    let mut said: Vec<String> = Vec::new();
    for named in scan.classes.iter().filter(|c| {
        let s = simple(&c.name);
        (s.ends_with("BlockEntity") || s.ends_with("TileEntity")) && names_word(java, &s)
    }) {
        // The class and those above it.
        let mut next = Some(named);
        let mut depth = 0;
        while let Some(class) = next {
            depth += 1;
            next = class
                .superclass
                .as_deref()
                .filter(|_| depth < 10)
                .and_then(|above| find_class(scan, above));
            if seen.contains(&class.name) {
                continue;
            }
            seen.push(class.name.clone());
            let Ok(source) = fs::read_to_string(Path::new(&scan.input).join(&class.path)) else {
                continue;
            };
            let Some(tick) = crate::questions::method_body(&source, "tick") else {
                continue;
            };
            for (at, _) in tick.match_indices("if (") {
                let rest = &tick[at + 4..];
                let flag: String = rest.chars().take_while(|c| ident(*c)).collect();
                let Some(block) = rest[flag.len()..].strip_prefix(") {") else {
                    continue;
                };
                // Its block, to the brace that closes it.
                let mut open = 1;
                let Some(end) = block.char_indices().find_map(|(i, c)| {
                    match c {
                        '{' => open += 1,
                        '}' => open -= 1,
                        _ => {}
                    }
                    (open == 0).then_some(i)
                }) else {
                    continue;
                };
                let block = block[..end].trim();
                if flag.is_empty() || !block.contains(&format!("{flag} = false;")) {
                    continue;
                }
                // The methods it runs then: `name();`.
                for call in block.split(';').map(str::trim) {
                    let Some(method) = call.strip_suffix("()") else {
                        continue;
                    };
                    if method.is_empty()
                        || !method.chars().all(ident)
                        || !java.contains(&format!("{method}("))
                        || said.contains(&method.to_string())
                    {
                        continue;
                    }
                    // The key the module keeps the field under.
                    let key = module
                        .lines()
                        .find(|l| l.contains(&format!("`boolean {flag}`")))
                        .and_then(|l| {
                            let after = l
                                .split_once("kept as `")
                                .or_else(|| l.split_once("saved as `"))?
                                .1;
                            after.split('`').next()
                        })
                        .unwrap_or(&flag);
                    said.push(method.to_string());
                    let how = if on_state {
                        // The state's field, in snake case as the struct has it.
                        let mut field = String::new();
                        for c in flag.chars() {
                            if c.is_ascii_uppercase() {
                                field.push('_');
                            }
                            field.push(c.to_ascii_lowercase());
                        }
                        format!("In this tick that is `state.{field} = true`.")
                    } else {
                        format!("Its key is `\"{key}\"`, set with `Saved::Byte(1)`.")
                    };
                    let _ = writeln!(
                        out,
                        "- `{method}()` runs on the block entity's next tick once `{flag}` is set: `{}`'s `tick()` has \
                         `if ({flag}) {{ {block} }}`. {how}",
                        simple(&class.name)
                    );
                }
            }
        }
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
            "partialFacingVertical(",
            "`CachedBuffers.partialFacingVertical(model, state, facing)` (Catnip) is `model`, which is built standing up (its axis along Y), turned through the block's centre about Y by `AngleHelper.horizontalAngle(facing)` (south 0, west -90, north 180, east 90, up and down 0) and then about X by `AngleHelper.verticalAngle(facing) + 90` (up 0, down 180, each of the four sides 90): `draw.rotate_about([0.0, 1.0, 0.0], y, [0.5; 3]); draw.rotate_about([1.0, 0.0, 0.0], x, [0.5; 3]); draw.model(..)`. So a wheel on the X axis (facing east) is turned 90 about Y and then 90 about X, one on the Z axis (facing south) 90 about X alone, and one on the Y axis not at all. A spin Java gives that buffer afterwards is made before both",
        ),
        (
            "&mut Draw",
            "mulPose(",
            "`ms.mulPose(Axis.YP.rotationDegrees(a))` turns about the pose's origin as it is then: `draw.rotate([0.0, 1.0, 0.0], a)`. After `ms.translate(.5, .., .5)` that origin is no longer the block's corner, so a pivot of `[0.5; 3]` there would be counted from the moved origin and throw the model aside. `draw.rotate_about(axis, a, [0.5; 3])` is only for TransformStack's `rotateCentered` and `center()`..`uncenter()`, made from the block's corner",
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
/// Each block entity of `module`: its id, its state struct, its classes (its own first)
/// and the key each of its values is saved under, with its type and Java field.
fn entity_keys(module: &str) -> Vec<(&str, &str, Vec<&str>, Vec<String>)> {
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
        // Its item slots, where its comment lays them out: what a tick that reaches it
        // reads and changes of them.
        const SLOTS: &str = "/// Its item handlers share its item slots, one after another: ";
        if let Some(layout) = lines[i..j]
            .iter()
            .find_map(|l| l.strip_prefix(SLOTS))
            .map(|l| l.split(" (`world.entity_item").next().unwrap_or(l))
        {
            keys.push(format!(
                "its item slots (`n.items[slot]` of the view; `machine.set_item_at(pos, slot, stack)` changes one): {layout}"
            ));
        }
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
    found
}

/// For a question to a block entity (its comment lists what each class answers): the
/// block entities of those classes, with the keys their values are saved under.
fn asked_keys(module: &str, comment: &str) -> String {
    let mut out = String::new();
    if !comment.contains("block entity class answers") {
        return out;
    }
    // The classes that answer: `/// - `HandCrankBlockEntity` (`create:hand_crank`): ..`.
    let classes: Vec<&str> = comment
        .lines()
        .filter_map(|l| l.strip_prefix("/// - `"))
        .filter_map(|l| l.split('`').next())
        .collect();
    let mut listed = 0;
    for (id, _, of, mut keys) in entity_keys(module) {
        // A block entity's own class is the first its state names. (The class all of
        // them inherit answers for the rest, from keys each of these has too.)
        let Some(own) = of.first().map(|c| c.rsplit('.').next().unwrap_or(c)) else {
            continue;
        };
        if !classes.contains(&own) || keys.is_empty() || listed >= 12 {
            continue;
        }
        listed += 1;
        keys.truncate(40);
        let _ = writeln!(out, "- `{id}` (`{own}`): {}", keys.join(", "));
    }
    out
}

/// What a tick's comment says its block entity keeps that was made with a method of
/// its own: (the field, how Java makes it, the method), from "`basinChecker` is `new
/// DeferralBehaviour(this, this::updateBasin)`".
fn handed_methods(comment: &str) -> Vec<(String, String, String)> {
    let Some((_, said)) = comment.split_once("was made with code of this block entity: ") else {
        return Vec::new();
    };
    let said = said.split(". A kept part").next().unwrap_or(said);
    let mut out = Vec::new();
    for part in said.split("; ") {
        let mut ticks = part.split('`');
        let (Some(field), Some(how)) = (ticks.nth(1), ticks.nth(1)) else {
            continue;
        };
        for (at, _) in how.match_indices("::") {
            let method: String = how[at + 2..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !method.is_empty() && method != "new" {
                out.push((field.to_string(), how.to_string(), method));
            }
        }
    }
    out
}

/// Whether `function` has code named after the Java method `method`: `update_basin`, or
/// the name as Java has it.
fn names_method(function: &str, method: &str) -> bool {
    let mut snake = String::new();
    for c in method.chars() {
        if c.is_ascii_uppercase() {
            snake.push('_');
        }
        snake.push(c.to_ascii_lowercase());
    }
    function
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .any(|l| l.contains(&snake) || l.contains(method))
}

/// A field the state has no place for (`// Left out of Mixer, to port by hand:` lists
/// `Recipe<?> currentRecipe`) that a `// Missing` note of the answer gives as its reason:
/// work left out for a value the Java only looked up.
fn leaves_out_for_a_field(function: &str, module: &str) -> Option<String> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    // The states the function is handed, by its signature.
    let head = function.split('{').next().unwrap_or("");
    let mut fields: Vec<&str> = Vec::new();
    for (at, _) in module.match_indices("\n// Left out of ") {
        let chunk = &module[at + 1..];
        let first = chunk.lines().next().unwrap_or("");
        let state = first
            .trim_start_matches("// Left out of ")
            .split(',')
            .next()
            .unwrap_or("");
        if state.is_empty() || !head.split(|c: char| !ident(c)).any(|w| w == state) {
            continue;
        }
        for line in chunk.lines().skip(1).take_while(|l| l.starts_with("// - ")) {
            if !line.contains("no Rust type for it yet") {
                continue;
            }
            // `Recipe<?> currentRecipe`: the name is its last word.
            if let Some(name) = line
                .split('`')
                .nth(1)
                .and_then(|decl| decl.rsplit(' ').next())
                .filter(|n| n.len() > 3)
            {
                fields.push(name);
            }
        }
    }
    function
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("//"))
        // A note and the lines it runs on to.
        .scan(false, |noting, l| {
            *noting =
                l.contains("Missing") || (*noting && !l.trim_start_matches('/').trim().is_empty());
            Some((*noting, l))
        })
        .filter(|(noting, _)| *noting)
        .find_map(|(_, l)| {
            fields
                .iter()
                .find(|f| l.split(|c: char| !ident(c)).any(|w| w == **f))
                .map(|f| f.to_string())
        })
}

/// The classes `java` calls a static function of: each `Name` of a `Name.function(`.
fn static_receivers(java: &str) -> Vec<String> {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    let mut out: Vec<String> = Vec::new();
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
            && !out.iter().any(|r| r == receiver)
        {
            out.push(receiver.to_string());
        }
    }
    out
}

/// Whether a drawing answer draws parts Java keeps in buffers of their own (two or more
/// `CachedBuffers.` parts, each a `SuperByteBuffer` with its own turns) through one pose,
/// turning it and never pushing: the first part's turns then move the rest.
fn shares_turns_between_parts(function: &str, java: &str) -> bool {
    if java.matches("CachedBuffers.").count() < 2 {
        return false;
    }
    let code: String = function
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let turns = [
        "draw.rotate_about(",
        "draw.rotate(",
        "draw.translate(",
        "draw.scale(",
    ]
    .iter()
    .any(|t| code.contains(t));
    let parts = code.matches("draw.model(").count() + code.matches("draw.block(").count();
    turns && parts >= 1 && !code.contains("draw.push()")
}

/// Whether a drawing answer turns about a pivot other than the origin after it moved
/// the origin, where the Java turns about the moved origin (`mulPose`) and never centred
/// (`rotateCentered`, `center()`): the pivot is then counted twice.
fn pivots_after_moving(function: &str, java: &str) -> bool {
    if !function.contains("draw: &mut Draw")
        || !java.contains("mulPose(")
        || java.contains("rotateCentered")
        || java.contains(".center()")
    {
        return false;
    }
    let Some(moved) = function.find("draw.translate(") else {
        return false;
    };
    function[moved..]
        .match_indices("rotate_about(")
        .any(|(at, _)| {
            // Its last argument, to the closing parenthesis.
            let rest = &function[moved + at..];
            let call = rest.split(';').next().unwrap_or(rest);
            let pivot = call.rsplit(',').next().unwrap_or("").trim();
            let pivot = pivot.trim_end_matches(')').trim();
            !(pivot.is_empty()
                || pivot == "[0.0; 3]"
                || pivot == "[0.0, 0.0, 0.0]"
                || pivot == "ORIGIN")
        })
}

/// The shared tick a function's comment tells it to call (`let called =
/// kinetic_block_entity_tick(pos, &mut base, ..)`, or `let mut called = ..` for the one
/// above a shared tick) that its answer never calls.
fn skips_its_shared_tick(comment: &str, function: &str) -> Option<String> {
    comment
        .split("called = ")
        .skip(1)
        .filter_map(|after| {
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (name.ends_with("_tick") && after[name.len()..].starts_with("(pos, &mut"))
                .then_some(name)
        })
        .find(|name| {
            // (In code: a `// Missing: ..` note that names it calls nothing.)
            !function
                .lines()
                .map(|l| l.split("//").next().unwrap_or(l))
                .any(|code| code.contains(&format!("{name}(")))
        })
}

/// Whether a function's own body has a `return` that isn't its last statement: one
/// outside every nested `fn` (whose returns are its own), with code after it.
fn returns_early(function: &str) -> bool {
    let Some((_, body)) = function.split_once('{') else {
        return false;
    };
    // Brace depth, and the depth each nested `fn` opened at.
    let mut depth = 1usize;
    let mut nested: Vec<usize> = Vec::new();
    let mut pending_fn = false;
    let lines: Vec<&str> = body.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let code = line.split("//").next().unwrap_or(line);
        let trimmed = code.trim_start();
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            pending_fn = true;
        }
        let is_return =
            (trimmed.starts_with("return;") || trimmed.starts_with("return ")) && nested.is_empty();
        if is_return {
            // The last statement of the function (only closing braces after it) is fine.
            let rest = lines[i + 1..]
                .iter()
                .map(|l| l.split("//").next().unwrap_or(l).trim())
                .any(|l| !l.is_empty() && l.chars().any(|c| c != '}'));
            if rest {
                return true;
            }
        }
        for c in code.chars() {
            match c {
                '{' => {
                    if pending_fn {
                        nested.push(depth);
                        pending_fn = false;
                    }
                    depth += 1;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if nested.last() == Some(&depth) {
                        nested.pop();
                    }
                }
                _ => {}
            }
        }
    }
    false
}

/// A format an answer gives three numbers that isn't `pos_text`'s: `{},{},{}`. A
/// position saved that way is read by no other function.
fn own_position_text(function: &str) -> Option<&'static str> {
    [
        "{},{},{}",
        "{}, {}, {}",
        "{}:{}:{}",
        "{}_{}_{}",
        "{}/{}/{}",
        "{};{};{}",
        "{}|{}|{}",
    ]
    .into_iter()
    .find(|f| function.contains(&format!("\"{f}\"")))
}

/// A block entity's id the answer holds against a block's id, where the two differ:
/// `be.block == "create:motor"` is never true of a `create:creative_motor` block, whose
/// block entity is `create:motor`. None where it names none, or reads `entity` too.
fn takes_an_entity_for_a_block(function: &str, module: &str) -> Option<(String, String)> {
    if !(function.contains(".block.as_str()") || function.contains(".block =="))
        || function.contains(".entity")
    {
        return None;
    }
    let table = module
        .split_once("pub const BLOCK_ENTITIES: &[(&str, &str, bool)] = &[")?
        .1;
    let table = &table[..table.find("\n];")?];
    let rows: Vec<(&str, &str)> = table
        .lines()
        .filter_map(|l| {
            let mut ids = l.split('"');
            Some((ids.nth(1)?, ids.nth(1)?))
        })
        .collect();
    rows.iter()
        .find(|(_, entity)| {
            !rows.iter().any(|(block, _)| block == entity)
                && function.contains(&format!("\"{entity}\""))
        })
        .map(|(block, entity)| (entity.to_string(), block.to_string()))
}

fn reached_keys(module: &str, java: &str, stub: &str) -> String {
    let mut out = String::new();
    let found = entity_keys(module);
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

/// The plain values a mod's recipe files carry, by type, as its module lists them
/// (`RECIPE_VALUES`).
fn recipe_values_of(module: &str) -> Vec<(String, Vec<String>)> {
    let Some(at) = module.find("pub const RECIPE_VALUES: &[(&str, &[&str])] = &[") else {
        return Vec::new();
    };
    let list = &module[at..];
    let end = list.find("\n];").unwrap_or(list.len());
    list[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut words = line.split('"').skip(1).step_by(2).map(str::to_string);
            let kind = words.next()?;
            Some((kind, words.collect()))
        })
        .collect()
}

/// The recipe values a function reads by a literal: each `.number("key")`.
/// A mod's own lang keys, by namespace (`create`), from the port's content.
#[derive(Debug, Default)]
pub struct Lang {
    pub keys: std::collections::HashMap<String, std::collections::HashSet<String>>,
}

impl Lang {
    /// The first string in `function` that looks like a lang key of the mod's and isn't
    /// one, with what to write instead: a key with the mod's namespace that it lacks, or a
    /// key that is the mod's once the namespace is put before it (`generic.unit.ticks`
    /// for `create.generic.unit.ticks`).
    pub fn check(&self, function: &str) -> Option<(String, String)> {
        if self.keys.is_empty() {
            return None;
        }
        for lit in function.split('"').skip(1).step_by(2) {
            let parts: Vec<&str> = lit.split('.').collect();
            if parts.len() < 3
                || !parts.iter().all(|p| {
                    !p.is_empty()
                        && p.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                })
            {
                continue;
            }
            if let Some(keys) = self.keys.get(parts[0]) {
                if keys.contains(lit) {
                    continue;
                }
                // The keys ending as it does are what it meant.
                let last = parts[parts.len() - 1];
                let mut near: Vec<&String> = keys
                    .iter()
                    .filter(|k| k.ends_with(&format!(".{last}")))
                    .collect();
                near.sort();
                near.truncate(8);
                let near: Vec<String> = near.iter().map(|k| format!("`{k}`")).collect();
                let right = if near.is_empty() {
                    "Use the key the Java names (the enum's or the class's own, as it builds it), never one made up from a name.".to_string()
                } else {
                    format!(
                        "Keys of the mod's ending the same way: {}.",
                        near.join(", ")
                    )
                };
                return Some((lit.to_string(), right));
            }
            for (ns, keys) in &self.keys {
                let full = format!("{ns}.{lit}");
                if keys.contains(&full) {
                    return Some((
                        lit.to_string(),
                        format!(
                            "The mod's key is `{full}`: its lang helper puts `{ns}.` before what the Java writes."
                        ),
                    ));
                }
            }
        }
        None
    }
}

/// The lang keys of the port in `dir`, read once a run.
fn mod_lang(dir: &Path) -> std::sync::Arc<Lang> {
    use std::sync::{Arc, Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<BTreeMap<PathBuf, Arc<Lang>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(l) = seen.lock().ok().and_then(|m| m.get(dir).cloned()) {
        return l;
    }
    let mut lang = Lang::default();
    if let Ok(namespaces) = fs::read_dir(dir.join("content/assets")) {
        for ns in namespaces.flatten() {
            let file = ns.path().join("lang/en_us.json");
            let Ok(text) = fs::read_to_string(&file) else {
                continue;
            };
            let Ok(serde_json::Value::Object(map)) =
                serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
            else {
                continue;
            };
            lang.keys.insert(
                ns.file_name().to_string_lossy().to_string(),
                map.keys().cloned().collect(),
            );
        }
    }
    let lang = Arc::new(lang);
    if let Ok(mut m) = seen.lock() {
        m.insert(dir.to_path_buf(), lang.clone());
    }
    lang
}

fn read_recipe_values(function: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, call) in function.match_indices(".number(") {
        let rest = function[at + call.len()..].trim_start();
        if let Some(key) = rest.strip_prefix('"').and_then(|r| r.split('"').next())
            && !out.iter().any(|o| o == key)
        {
            out.push(key.to_string());
        }
    }
    out
}

/// Whether `body` calls the function `function` of its module: not a longer name
/// ending in it, and not a method (`behaviour.tick()` is no call of the module's `tick`).
fn calls(body: &str, function: &str) -> bool {
    let call = format!("{function}(");
    body.match_indices(&call).any(|(at, _)| {
        !body[..at]
            .chars()
            .next_back()
            // (Nor a method of that name on something, or another module's.)
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':')
    })
}

/// Whether a ported function of `module` names something that isn't there: a state
/// property by a vanilla constant, a model the module doesn't bake, a recipe value its
/// recipe files don't carry. Each reads or draws nothing, and compiles all the same.
pub(crate) fn names_what_is_not_there(module: &str, function: &str) -> bool {
    if units::misnames_a_property(function) {
        return true;
    }
    let models = baked_models(module);
    if !models.is_empty() && drawn_models(function).iter().any(|m| !models.contains(m)) {
        return true;
    }
    let values: Vec<String> = recipe_values_of(module)
        .into_iter()
        .flat_map(|(_, keys)| keys)
        .collect();
    !values.is_empty()
        && read_recipe_values(function)
            .iter()
            .any(|k| !values.contains(k))
}

/// The ids of the models a mod's module bakes on their own (`STANDALONE_MODELS`).
fn baked_models(module: &str) -> Vec<String> {
    let Some(at) = module.find("pub const STANDALONE_MODELS: &[&str] = &[") else {
        return Vec::new();
    };
    let list = &module[at..];
    let end = list.find("];").unwrap_or(list.len());
    list[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The model ids a function draws by a literal: each `draw.model("id", ..)`.
fn drawn_models(function: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, call) in function.match_indices("draw.model(") {
        let rest = function[at + call.len()..].trim_start();
        if let Some(id) = rest.strip_prefix('"').and_then(|r| r.split('"').next())
            && !out.iter().any(|o| o == id)
        {
            out.push(id.to_string());
        }
    }
    out
}

/// The model constants of the mod `java` names (`AllPartialModels.HAND_CRANK_HANDLE`),
/// each with the id of `models` its declaration's path ends (`block("hand_crank/handle")`
/// is `create:block/hand_crank/handle`): what a renderer's `draw.model` takes.
fn model_constants(scan: &ModScan, java: &str, models: &[String]) -> Vec<(String, String)> {
    if models.is_empty() {
        return Vec::new();
    }
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out: Vec<(String, String)> = Vec::new();
    let mut sources: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (dot, _) in java.match_indices('.') {
        // `Class.CONSTANT`.
        let start = java[..dot].rfind(|c: char| !word(c)).map_or(0, |i| i + 1);
        let class = &java[start..dot];
        let rest = &java[dot + 1..];
        let end = rest.find(|c: char| !word(c)).unwrap_or(rest.len());
        let constant = &rest[..end];
        let shouting = constant.len() > 2
            && constant.starts_with(|c: char| c.is_ascii_uppercase())
            && constant
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !shouting
            || !class.starts_with(|c: char| c.is_ascii_uppercase())
            || out.iter().any(|(c, _)| c == constant)
        {
            continue;
        }
        let source = sources.entry(class.to_string()).or_insert_with(|| {
            let found = find_class(scan, class)?;
            fs::read_to_string(Path::new(&scan.input).join(&found.path)).ok()
        });
        let Some(source) = source else {
            continue;
        };
        // Its declaration's first string: the path it is made from.
        let Some(at) = source.find(&format!("{constant} =")) else {
            continue;
        };
        let decl = &source[at..];
        let decl = &decl[..decl.find(';').unwrap_or(decl.len())];
        let Some(path) = decl.split('"').nth(1) else {
            continue;
        };
        let mut matching = models.iter().filter(|m| {
            **m == path || m.ends_with(&format!("/{path}")) || m.ends_with(&format!(":{path}"))
        });
        if let (Some(id), None) = (matching.next(), matching.next()) {
            out.push((constant.to_string(), id.clone()));
        }
        if out.len() >= 24 {
            break;
        }
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
    /// What it cost, where the model's prices are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dollars: Option<f64>,
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
                dollars: o.dollars,
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
    fn parts_drawn_from_their_own_buffers_keep_their_own_turns() {
        let java = "SuperByteBuffer a = CachedBuffers.partial(SHAFT, s); SuperByteBuffer b = CachedBuffers.partialFacing(FAN, s, d);";
        let shared = "fn r(draw: &mut Draw) {
    draw.rotate_about([0.0, 1.0, 0.0], 30.0, [0.5; 3]);
    draw.model(\"a\", 0xFFFFFF);
    draw.rotate_about([0.0, 0.0, 1.0], 60.0, [0.5; 3]);
    draw.model(\"b\", 0xFFFFFF);
}";
        assert!(shares_turns_between_parts(shared, java));
        let own = shared.replace(
            "    draw.rotate_about([0.0, 1.0",
            "    draw.push();
    draw.rotate_about([0.0, 1.0",
        );
        assert!(!shares_turns_between_parts(&own, java));
        // One buffer only: nothing to keep apart.
        assert!(!shares_turns_between_parts(
            shared,
            "CachedBuffers.partial(SHAFT, s)"
        ));
    }

    #[test]
    fn answers_naming_lang_keys_the_mod_lacks_go_back() {
        let mut lang = Lang::default();
        lang.keys.insert(
            "create".into(),
            [
                "create.contraptions.movement_mode.rotate_place",
                "create.generic.unit.ticks",
            ]
            .iter()
            .map(|k| k.to_string())
            .collect(),
        );
        // Made up from the enum's name: the key ending the same way is offered.
        let (wrong, right) = lang
            .check(r#"vec!["create.gui.contraptions.rotation_mode.rotate_place".into()]"#)
            .unwrap();
        assert_eq!(wrong, "create.gui.contraptions.rotation_mode.rotate_place");
        assert!(right.contains("`create.contraptions.movement_mode.rotate_place`"));
        // `CreateLang`'s key without its namespace.
        let (wrong, right) = lang
            .check(r#"rows: vec!["generic.unit.ticks".into()]"#)
            .unwrap();
        assert_eq!(wrong, "generic.unit.ticks");
        assert!(right.contains("`create.generic.unit.ticks`"));
        // Its own keys, other mods' and plain text pass.
        assert!(
            lang.check(
                r#""create.generic.unit.ticks", "key.mouse.right", "Value", "minecraft:stone""#
            )
            .is_none()
        );
    }

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
    fn names_the_models_a_renderers_java_refers_to() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../porter-java/tests/fixtures/neoforge-machine-demo");
        let scan = porter_java::scan(&path).unwrap();
        let models = baked_models(
            "pub const STANDALONE_MODELS: &[&str] = &[\n    \"grind:block/crusher_wheel\",\n];\n",
        );
        assert_eq!(models, ["grind:block/crusher_wheel"]);
        // A constant declared over two lines, named by the code that draws it: the id
        // its path ends, not one made from its name.
        let named = model_constants(
            &scan,
            "nodes.submitModel(GrindClient.WHEEL, pose);",
            &models,
        );
        assert_eq!(
            named,
            [("WHEEL".to_string(), "grind:block/crusher_wheel".to_string())]
        );
        assert_eq!(
            drawn_models(
                "draw.model(\"grind:block/wheel\", 0xFFFFFF);\ndraw.model(\n    \"grind:block/crusher_wheel\",\n    1,\n);"
            ),
            ["grind:block/wheel", "grind:block/crusher_wheel"]
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
    fn names_the_switch_a_tick_reads_for_a_method_a_click_calls() {
        let dir = std::env::temp_dir().join(format!("porter-switch-{}", std::process::id()));
        fs::create_dir_all(dir.join("net/demo")).unwrap();
        fs::write(
            dir.join("net/demo/Generator.java"),
            "package net.demo;\npublic class Generator extends BlockEntity {\n\tboolean reActivateSource;\n\tboolean dirty;\n\tpublic void tick() {\n\t\tsuper.tick();\n\t\tif (reActivateSource) {\n\t\t\tupdateGeneratedRotation();\n\t\t\treActivateSource = false;\n\t\t}\n\t\tif (dirty) {\n\t\t\tspeed = 0;\n\t\t}\n\t}\n\tpublic void updateGeneratedRotation() {}\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("net/demo/CrankBlockEntity.java"),
            "package net.demo;\npublic class CrankBlockEntity extends Generator {\n\tpublic void turn(boolean back) {\n\t\tinUse = 10;\n\t\tupdateGeneratedRotation();\n\t}\n}\n",
        )
        .unwrap();
        let class = |name: &str, above: Option<&str>| porter_model::ClassInfo {
            name: format!("net.demo.{name}"),
            kind: porter_model::ClassKind::Class,
            path: format!("net/demo/{name}.java"),
            superclass: above.map(|a| format!("net.demo.{a}")),
            interfaces: Vec::new(),
            references: Vec::new(),
            side: porter_model::Side::Common,
            is_mixin: false,
            lines: 10,
            methods: 1,
            fields: Vec::new(),
            method_names: vec!["tick".into()],
            saved: Vec::new(),
        };
        let scan = ModScan {
            input: dir.to_string_lossy().to_string(),
            classes: vec![
                class("Generator", None),
                class("CrankBlockEntity", Some("Generator")),
            ],
            ..ModScan::default()
        };
        let module = "pub struct Crank {\n    /// `boolean reActivateSource`, not saved by the Java mod: kept as `reActivateSource` so it lasts between ticks.\n    pub re_activate_source: bool,\n}\n";
        // A click whose Java has the block entity turn, which updates its rotation.
        let java = "withBlockEntityDo(level, pos, be -> be.turn(sneaking)); class CrankBlockEntity { void turn(boolean back) { inUse = 10; updateGeneratedRotation(); } }";
        let said = tick_switches(&scan, module, java, false);
        assert!(
            said.contains("- `updateGeneratedRotation()` runs on the block entity's next tick once `reActivateSource` is set: `Generator`'s `tick()` has `if (reActivateSource) { updateGeneratedRotation(); reActivateSource = false; }`. Its key is `\"reActivateSource\"`, set with `Saved::Byte(1)`."),
            "{said}"
        );
        assert_eq!(said.lines().count(), 1);
        // Java that calls no such method is told nothing.
        assert_eq!(
            tick_switches(&scan, module, "class CrankBlockEntity { int x; }", false),
            ""
        );
        // A tick is told the field of its state.
        assert!(
            tick_switches(&scan, module, java, true)
                .ends_with("In this tick that is `state.re_activate_source = true`.\n")
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_question_is_given_its_classes_keys_and_an_entitys_id_is_not_a_blocks() {
        let module = "pub const BLOCK_ENTITIES: &[(&str, &str, bool)] = &[
    (\"demo:creative_motor\", \"demo:motor\", true),
    (\"demo:gearbox\", \"demo:gearbox\", true),
];

/// The state of a `demo:motor` block entity: `net.demo.MotorBlockEntity` and what it inherits from `net.demo.KineticBlockEntity`, saved with the block.
pub struct Motor {
    /// `float speed`, saved as `Speed`.
    pub speed: f32,
    /// `int inUse`, not saved by the Java mod: kept as `inUse` so it lasts between ticks.
    pub in_use: i32,
}

/// The state of a `demo:gearbox` block entity: `net.demo.GearboxBlockEntity` and what it inherits from `net.demo.KineticBlockEntity`, saved with the block.
pub struct Gearbox {
    /// `float speed`, saved as `Speed`.
    pub speed: f32,
}
";
        let comment = "/// `KineticBlockEntity.getGeneratedSpeed()`: a question the mod's code asks a block entity it meets,\n///\n/// What each block entity class answers, with its block entities and those that inherit it:\n/// - `KineticBlockEntity` (`demo:gearbox`): `return 0;`\n/// - `MotorBlockEntity` (`demo:motor`): `return inUse == 0 ? 0 : 16;`\n///\n/// TODO: port from `net.demo.KineticBlockEntity`.\n";
        // The motor's keys, under its block entity's id; not the gearbox's, whose own
        // class answers nothing of its own.
        assert_eq!(
            asked_keys(module, comment),
            "- `demo:motor` (`MotorBlockEntity`): `Speed` (f32, `float speed`), `inUse` (i32, `int inUse`)\n"
        );
        assert_eq!(
            asked_keys(
                module,
                "/// TODO: port the tick of `net.demo.MotorBlockEntity`.\n"
            ),
            ""
        );

        // `demo:motor` is no block: an answer that takes it for one is told so.
        let wrong = "pub fn speed(be: &Neighbour) -> f32 {\n    match be.block.as_str() {\n        \"demo:motor\" => 16.0,\n        _ => 0.0,\n    }\n}\n";
        assert_eq!(
            takes_an_entity_for_a_block(wrong, module),
            Some(("demo:motor".to_string(), "demo:creative_motor".to_string()))
        );
        // Told apart by `entity`, or naming an id that is both, it is fine.
        let right = wrong.replace("be.block.as_str()", "be.entity.as_deref().unwrap_or(\"\")");
        assert_eq!(takes_an_entity_for_a_block(&right, module), None);
        let both = wrong.replace("demo:motor", "demo:gearbox");
        assert_eq!(takes_an_entity_for_a_block(&both, module), None);
    }

    #[test]
    fn a_return_in_a_ticks_own_body_is_told_from_one_in_a_nested_function() {
        // The behaviour's `return` copied into the tick: the depot's part never runs.
        assert!(returns_early(
            "pub fn tick_press(state: &mut Press, machine: &mut Machine<'_>) {
    if !state.running {
        if state.speed == 0.0 {
            return;
        }
        return;
    }
    hand_over(state, machine);
}
"
        ));
        // In a nested function it leaves that function, and the tick goes on.
        assert!(!returns_early(
            "pub fn tick_press(state: &mut Press, machine: &mut Machine<'_>) {
    fn behaviour(state: &mut Press) {
        if !state.running {
            return;
        }
        state.ticks += 1;
    }
    behaviour(state);
    hand_over(state, machine);
}
"
        ));
        // As the last thing the tick does, it skips nothing.
        assert!(!returns_early(
            "pub fn tick_press(state: &mut Press) {
    state.ticks += 1;
    if state.ticks > 3 {
        return;
    }
}
"
        ));
    }

    #[test]
    fn a_tick_that_leaves_out_the_shared_tick_is_noticed() {
        let comment = "/// The tick of `Basin` and the classes above it is ported once, as `basin_tick`. Where\n/// this block entity's Java calls `super.tick()` into them, do:\n/// `let called = basin_tick(pos, &mut base, &me, machine);`\n";
        assert_eq!(
            skips_its_shared_tick(comment, "pub fn tick_press() {\n    pressing_tick();\n}\n"),
            Some("basin_tick".to_string())
        );
        assert_eq!(
            skips_its_shared_tick(
                comment,
                "pub fn tick_press() {\n    let called = basin_tick(pos, &mut base, &me, machine);\n}\n"
            ),
            None
        );
        // Named in a note is not called.
        assert_eq!(
            skips_its_shared_tick(
                comment,
                "pub fn tick_press() {
    // Missing: basin_tick(pos, &mut base, &me, machine).
}
"
            ),
            Some("basin_tick".to_string())
        );
        // A tick with none above it is asked for none.
        assert_eq!(
            skips_its_shared_tick("/// TODO: port the tick of `a.B`.\n", "pub fn t() {}\n"),
            None
        );
        // The classes code hands its work to, by the static calls it makes.
        assert_eq!(
            static_receivers(
                "if (!BasinRecipe.apply(basin, recipe)) return; AXIS.choose(1); level.getBlockState(pos); Mth.clamp(a, b, c);"
            ),
            vec!["BasinRecipe".to_string(), "Mth".to_string()]
        );
    }

    #[test]
    fn work_left_out_for_a_field_the_java_only_looked_up_is_noticed() {
        let module = "pub struct Mixer {\n    pub speed: f32,\n}\n\n// Left out of Mixer, to port by hand:\n// - `static Object key`: a constant; copy its value into the tick\n// - `Recipe<?> currentRecipe`: no Rust type for it yet; where the Java fills it by looking something up (a recipe it found), look it up again where the Java reads it\n\n// Left out of Press, to port by hand:\n// - `Thing otherField`: no Rust type for it yet\n";
        let gave_up = "pub fn tick_mixer(pos: [i32; 3], state: &mut Mixer, machine: &mut Machine<'_>) {\n    state.speed += 1.0;\n    // Missing: applyBasinRecipe(): it applies\n    // currentRecipe, which is not kept.\n}\n";
        assert_eq!(
            leaves_out_for_a_field(gave_up, module),
            Some("currentRecipe".to_string())
        );
        // Named where it is looked up again, it is no reason given.
        let did = "pub fn tick_mixer(pos: [i32; 3], state: &mut Mixer, machine: &mut Machine<'_>) {\n    // currentRecipe: found again from the basin.\n    let recipe = machine.recipes(\"demo:mixing\");\n    // Missing: the sound.\n}\n";
        assert_eq!(leaves_out_for_a_field(did, module), None);
        // Another state's field is not this function's.
        let other = gave_up.replace("currentRecipe", "otherField");
        assert_eq!(leaves_out_for_a_field(&other, module), None);
    }

    #[test]
    fn a_reached_block_entitys_item_slots_are_laid_out() {
        let module = "/// The state of a `demo:basin` block entity: `net.demo.BasinBlockEntity`, saved with the block.
/// Its item handlers share its item slots, one after another: `inputInventory` slots 0 to 8, `outputInventory` slots 9 to 17 (`world.entity_item(pos, slot)`
/// in a click, `machine.slots[slot]` in its tick).
pub struct Basin {
    /// `boolean contentsChanged`, saved as `Changed`.
    pub contents_changed: bool,
}
";
        let said = reached_keys(
            module,
            "class Mixer { void apply() { BasinBlockEntity basin = getBasin(); } }",
            "pub fn tick_mixer(state: &mut Mixer) {}",
        );
        assert!(
            said.starts_with("- `demo:basin` (`BasinBlockEntity`): its item slots (`n.items[slot]` of the view; `machine.set_item_at(pos, slot, stack)` changes one): `inputInventory` slots 0 to 8, `outputInventory` slots 9 to 17, `Changed` (bool, `boolean contentsChanged`)"),
            "{said}"
        );
    }

    #[test]
    fn a_method_handed_to_a_kept_part_is_read_from_the_comment() {
        let comment = "/// What it keeps was made with code of this block entity: `basinChecker` is `new DeferralBehaviour(this, this::updateBasin)`; `filter` is `new Filter(this, slot -> true)`. A kept part's `tick()` calls\n/// the code it was made with.\n";
        assert_eq!(
            handed_methods(comment),
            vec![(
                "basinChecker".to_string(),
                "new DeferralBehaviour(this, this::updateBasin)".to_string(),
                "updateBasin".to_string()
            )]
        );
        assert!(handed_methods("/// TODO: port the tick of `a.B`.\n").is_empty());
        // Run under its Rust name, or not at all (a comment that names it runs nothing).
        assert!(names_method(
            "fn t() {\n    fn update_basin() {}\n    update_basin();\n}\n",
            "updateBasin"
        ));
        assert!(!names_method(
            "fn t() {\n    // updateBasin is the basin tick's.\n}\n",
            "updateBasin"
        ));
    }

    #[test]
    fn a_turn_about_a_pivot_after_moving_the_origin_is_noticed() {
        let java = "ms.translate(.5f, 15 / 16f, .5f); ms.mulPose(Axis.XP.rotationDegrees(90));";
        let wrong = "pub fn render_depot(state: &Depot, frame: &Frame, draw: &mut Draw) {\n    draw.translate([0.5, 15.0 / 16.0, 0.5]);\n    draw.rotate_about([1.0, 0.0, 0.0], 90.0, PIVOT);\n}\n";
        assert!(pivots_after_moving(wrong, java));
        let right = wrong.replace(
            "rotate_about([1.0, 0.0, 0.0], 90.0, PIVOT)",
            "rotate([1.0, 0.0, 0.0], 90.0)",
        );
        assert!(!pivots_after_moving(&right, java));
        // Centred in the Java: a pivot is what it means.
        assert!(!pivots_after_moving(
            wrong,
            "buffer.rotateCentered(a, Direction.UP);"
        ));
        // A pivot before any move is the block's centre, as it should be.
        let before = "pub fn render_fan(state: &Fan, frame: &Frame, draw: &mut Draw) {\n    draw.rotate_about([0.0, 1.0, 0.0], 90.0, [0.5; 3]);\n    draw.translate([0.0, 0.1, 0.0]);\n}\n";
        assert!(!pivots_after_moving(before, java));
    }

    #[test]
    fn a_position_written_another_way_is_noticed() {
        assert_eq!(
            own_position_text(
                "fn key_of(p: [i32; 3]) -> String {\n    format!(\"{},{},{}\", p[0], p[1], p[2])\n}\n"
            ),
            Some("{},{},{}")
        );
        assert_eq!(own_position_text("let source = pos_text(from);"), None);
        assert_eq!(own_position_text("format!(\"{} {} {}\", a, b, c)"), None);
    }

    #[test]
    fn an_answer_that_only_says_the_java_is_not_ported_does_nothing() {
        assert!(does_nothing(
            "pub fn tick_press(pos: [i32; 3], state: &mut Press, machine: &mut Machine<'_>) {
    let _ = (pos, state, machine);
    // Missing: the press's pressing behaviour, the basin tick and the belt hand-off.
    // None of that has been ported yet, so this tick does nothing.
}
"
        ));
        assert!(does_nothing(
            "pub fn can_survive(block: &str) -> bool {\n    let _ = block;\n    // Missing: the support check.\n    false\n}\n"
        ));
        // A function whose Java does nothing, or hands back a plain value, is a port.
        assert!(!does_nothing(
            "pub fn tick_idle(pos: [i32; 3]) {\n    let _ = pos;\n}\n"
        ));
        assert!(!does_nothing(
            "pub fn is_small(block: &str) -> bool {\n    let _ = block;\n    // `ICog.isSmallCog()` is `return false;`.\n    false\n}\n"
        ));
        // And one that does something with a note on what it couldn't.
        assert!(!does_nothing(
            "pub fn tick_press(state: &mut Press) {\n    // Missing: the sound.\n    state.speed += 1.0;\n}\n"
        ));
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

    /// A module past `LARGE_MODULE`: what a tick and a click are handed, two block
    /// entities, a question ported once, a generated function, and a long private table.
    fn large_module() -> String {
        let mut m = String::from(
            "use std::collections::BTreeMap;

/// What a tick is handed.
pub struct Machine<'a> {
    pub level: &'a str,
    pub facing: Side,
}

impl<'a> Machine<'a> {
    /// The block entity beside this one.
    pub fn neighbour(&self, side: Side) -> Option<Neighbour> {
        let _ = side;
        None
    }
}

pub enum Side {
    Up,
    Down,
}

pub struct Neighbour {
    pub block: String,
}

/// What a click is handed.
pub trait World {
    fn block(&self, pos: [i32; 3]) -> Clicked;
}

pub struct Clicked {
    pub id: String,
}

/// The state of a `demo:press` block entity: `net.demo.PressBlockEntity`, saved with the block.
pub struct Press {
    /// `float speed`, saved as `Speed`.
    pub speed: f32,
}

impl Press {
    pub fn load(get: impl Fn(&str) -> Option<f32>) -> Self {
        let mut state = Press { speed: 0.0 };
        if let Some(v) = get(\"Speed\") {
            state.speed = v;
        }
        if let Some(v) = get(\"A\") {
            state.speed += v;
        }
        if let Some(v) = get(\"B\") {
            state.speed += v;
        }
        if let Some(v) = get(\"C\") {
            state.speed += v;
        }
        state
    }

    pub fn fast(&self) -> bool {
        self.speed > 8.0
    }
}

/// The state of a `demo:tray` block entity: `net.demo.TrayBlockEntity`, saved with the block.
pub struct Tray {
    pub held: i32,
}

/// `ICog.isSmall()`: a question the mod's code asks a block it meets,
/// answered here once for every block of the mod.
///
/// What each block class answers, with its blocks:
/// - `SmallCog` (`demo:cog`): `return true;`
///
/// TODO: port from `net.demo.ICog`.
pub fn i_cog_is_small(block: &str) -> bool {
    let _ = block;
    false
}

/// How many of an item a slot holds.
pub fn stack_size(item: &str) -> u32 {
    if item.is_empty() { 0 } else { 64 }
}

/// TODO: port the tick of `net.demo.PressBlockEntity`.
pub fn tick_press(pos: [i32; 3], state: &mut Press, machine: &mut Machine<'_>) {
    let _ = (pos, state, machine);
}

/// TODO: port the tick of `net.demo.TrayBlockEntity`.
pub fn tick_tray(pos: [i32; 3], state: &mut Tray, machine: &mut Machine<'_>) {
    let _ = (pos, state, machine);
}

/// TODO: port from `net.demo.PressBlock`.
pub fn press_block_use(world: &mut dyn World, pos: [i32; 3]) -> bool {
    let _ = (world, pos);
    false
}

const FILLER: &[&str] = &[
",
        );
        while m.len() <= LARGE_MODULE {
            m.push_str("    \"demo:a_block_among_very_many\",\n");
        }
        m.push_str("];\n");
        m
    }

    #[test]
    fn a_large_modules_shared_part_is_the_same_for_every_function_of_a_kind() {
        let module = large_module();
        let stub = |name: &str| units::function_source(&module, name).unwrap();
        let (common, own) = module_context(
            &module,
            &stub("tick_press"),
            "tick_press",
            "class PressBlockEntity { boolean small(Block b) { return ((ICog) b).isSmall(); } }",
        );
        let (tray_common, tray_own) = module_context(
            &module,
            &stub("tick_tray"),
            "tick_tray",
            "class TrayBlockEntity { }",
        );
        // What a tick's parameters reach and what the port generated, word for word
        // the same for both: the part the model's cache holds between them.
        assert!(!common.is_empty());
        assert_eq!(common, tray_common);
        for shared in [
            "pub struct Machine<'a>",
            "pub fn neighbour(&self, side: Side)",
            "pub enum Side",
            "pub struct Neighbour",
            "pub fn stack_size(item: &str) -> u32",
        ] {
            assert!(
                common.contains(shared),
                "{shared} is missing from:\n{common}"
            );
            assert!(!own.contains(shared), "{shared} is shown twice");
        }
        // Not a click's world, which a tick can't reach; nothing of either state.
        assert!(!common.contains("World") && !common.contains("Clicked"));
        assert!(!common.contains("Press") && !common.contains("Tray"));
        assert!(!common.contains("FILLER") && !own.contains("FILLER"));
        // Its own: its state (what loads it by its signature), and of a question its
        // Java asks, what a caller reads.
        assert!(own.contains("pub struct Press") && own.contains("saved as `Speed`"));
        assert!(
            own.contains(
                "pub fn load(get: impl Fn(&str) -> Option<f32>) -> Self { /* 14 lines */ }"
            ),
            "{own}"
        );
        assert!(
            own.contains("pub fn fast(&self) -> bool {\n        self.speed > 8.0"),
            "{own}"
        );
        assert!(!own.contains("pub struct Tray") && !own.contains("tick_tray"));
        assert!(
            own.contains("/// `ICog.isSmall()`: a question the mod's code asks a block it meets,")
        );
        assert!(own.contains("pub fn i_cog_is_small(block: &str) -> bool { /* 4 lines, ported with this one where it calls it */ }"), "{own}");
        assert!(!own.contains("What each block class answers"));
        // The other tick has its own state, and no question its Java never asks.
        assert!(tray_own.contains("pub struct Tray") && !tray_own.contains("i_cog_is_small"));

        // A click reaches other types: another shared part.
        let (click_common, _) = module_context(
            &module,
            &stub("press_block_use"),
            "press_block_use",
            "class PressBlock { }",
        );
        assert!(
            click_common.contains("pub trait World") && click_common.contains("pub struct Clicked")
        );
        assert!(!click_common.contains("pub struct Machine"));
        assert_ne!(click_common, common);
    }

    #[test]
    fn a_small_module_is_shown_whole_with_nothing_shared() {
        let module = large_module();
        let small = &module[..module.find("const FILLER").unwrap()];
        let stub = units::function_source(small, "tick_press").unwrap();
        let (common, own) = module_context(
            small,
            &stub,
            "tick_press",
            "class PressBlockEntity { boolean small(Block b) { return ((ICog) b).isSmall(); } }",
        );
        assert_eq!(common, "");
        for part in [
            "pub struct Machine<'a>",
            "pub trait World",
            "pub struct Press",
            "if let Some(v) = get(\"C\")",
            "What each block class answers",
            "pub fn i_cog_is_small(block: &str) -> bool {\n    let _ = block;",
        ] {
            assert!(own.contains(part), "{part} is missing");
        }
        assert!(!own.contains("pub struct Tray"));
    }

    #[test]
    fn an_estimate_prices_each_prompt_by_its_own_length_and_reads_the_shared_part_from_the_cache() {
        let planned = |tokens: u64, shared_chars: usize, shared_key: u64| Outcome {
            unit: Unit {
                file: "mixins/m/runtime.rs".into(),
                function: "f".into(),
                kind: Kind::Runtime,
                comment: String::new(),
                java: Vec::new(),
                members: Vec::new(),
                again: false,
            },
            status: Status::Planned {
                prompt_chars: 100,
                shared_chars,
                shared_key,
                tokens: Some(tokens),
            },
            attempts: 0,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            dollars: None,
        };
        // Two of a kind (two fifths of each is the shared part) and one past the line.
        let plan = [
            planned(50_000, 40, 7),
            planned(50_000, 40, 7),
            planned(120_000, 0, 0),
        ];
        let est = estimate(&plan, "claude-haiku-5-5");
        assert_eq!(
            (est.functions, est.tokens, est.cached, est.long),
            (3, 220_000, 20_000, 1)
        );
        assert!(est.counted);
        let out = OUTPUT_PER_TRY as f64;
        let first = (30_000.0 + 20_000.0 * 1.25) * 0.1 / 1e6 + out * 0.5 / 1e6;
        let second = (30_000.0 + 20_000.0 * 0.1) * 0.1 / 1e6 + out * 0.5 / 1e6;
        let long = 120_000.0 * 0.5 / 1e6 + out * 2.5 / 1e6;
        assert!(
            (est.dollars.unwrap() - (first + second + long)).abs() < 1e-9,
            "{est:?}"
        );
        // A model whose prices aren't known has tokens and no dollars.
        let other = estimate(&plan, "someone-elses");
        assert_eq!((other.tokens, other.dollars), (220_000, None));
        assert_eq!(planned_tokens(&plan), 220_000);
        // Worked out from the length where no model counted.
        let mut guess = planned(0, 0, 0);
        guess.status = Status::Planned {
            prompt_chars: 230_000,
            shared_chars: 0,
            shared_key: 0,
            tokens: None,
        };
        let est = estimate(&[guess], "claude-haiku-5-5");
        assert!(!est.counted);
        assert_eq!(est.tokens, 100_000);
    }
}
