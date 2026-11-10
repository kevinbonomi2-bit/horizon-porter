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
];

/// The newest line of `REACHES`.
pub const REACH: u32 = 3;

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
            if marked || reach_grew_for(reach, &notes) {
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
    }
    used.sort_by_key(|(_, s)| s.len());
    let mut used_chars = 0;
    for (class, source) in used {
        if used_chars + source.len() > USED_BUDGET || java_chars + source.len() > JAVA_BUDGET {
            out.notes.push(format!(
                "{} (a class it uses) left out: past the budget for them",
                class.name
            ));
            continue;
        }
        used_chars += source.len();
        java_chars += source.len();
        let _ = writeln!(
            p,
            "A class of the mod that code uses, `{}` ({}), for what its calls do:\n\n```java\n{source}\n```\n",
            class.name, class.path
        );
        java_text.push_str(&source);
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

/// The model's instructions.
const SYSTEM: &str = "You port Minecraft Java mods to Horizon, a Rust rewrite of Minecraft 26.3, one function at a time. \
A tool generated the Rust function you are given: its signature, its parameters and the Horizon hook it is wired to are \
right. Write its body so it does what the Java code it names does, through what the function is given.\n\n\
Answer with the whole function, its signature unchanged, in one ```rust block, and nothing else in code blocks. \
Keep the Java's behaviour: its numbers, its order, its edge cases. Where the Java uses something this function \
has no way to reach, do the part you can and say what is missing in a short `// Missing: ...` comment in the body; never invent \
APIs that are not in what you are shown. No `unsafe`, no `unwrap` on values that can be absent, no new crates.";

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
    fn refresh_is_for_what_a_newer_reach_gives() {
        // Ported before any reach: every gap is worth another go.
        assert!(reach_grew_for(0, "// Missing: the stat award."));
        // Ported at reach 1: only what reach 2 gives.
        assert!(!reach_grew_for(1, "// Missing: the stat award."));
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
