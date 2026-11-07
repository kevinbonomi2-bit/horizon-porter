//! The `horizon-porter` command line.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "horizon-porter",
    version,
    about = "Port Minecraft Java mods to Horizon native mods"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read a mod and summarize its classes, Mixins, registries and resources
    Scan {
        /// The mod's source directory (a Gradle project) or a built jar
        path: PathBuf,
        /// Also write everything found, as JSON, to this file
        #[arg(long, value_name = "FILE")]
        json: Option<PathBuf>,
    },
    /// Say what can be ported, what is blocked and why
    Report {
        /// The mod's source directory (a Gradle project) or a built jar
        path: PathBuf,
        /// List every Mixin member, not only the partial and blocked ones
        #[arg(long)]
        all: bool,
        /// How many entries to show in each ranked list
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Write the report as a self-contained HTML page to this file instead
        #[arg(long, value_name = "FILE")]
        html: Option<PathBuf>,
        /// With --html: the page the desktop app shows, without header and footer
        #[arg(long, hide = true, requires = "html")]
        embedded: bool,
    },
    /// Generate a Horizon native mod bundle for a client mod, with a porting checklist
    Port {
        /// The mod's source directory (a Gradle project), or its .jar (decompiled first)
        path: PathBuf,
        /// The folder to write the bundle into (empty or new)
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// Replace the generated files in a folder that already has files
        #[arg(long)]
        force: bool,
    },
    /// Build a ported mod into a Horizon instance with your Horizon launcher
    Build {
        /// The folder `port` wrote
        path: PathBuf,
        /// The instance to build into, created when missing (default: "<mod id> port")
        #[arg(long)]
        instance: Option<String>,
        /// The Horizon launcher (default: found on PATH and in its usual places)
        #[arg(long, value_name = "PATH")]
        launcher: Option<PathBuf>,
        /// You read the mod's Rust code and trust it with full access to this computer (not needed for a content mod alone)
        #[arg(long)]
        trust_native_code: bool,
        /// Start the instance when the build is done
        #[arg(long)]
        play: bool,
    },
    /// Translate the functions left to port with an AI model, compiling and testing each
    Translate {
        /// The folder `port` wrote
        path: PathBuf,
        /// Who translates: `anthropic` (Claude through the API: ANTHROPIC_API_KEY or `ant auth login`),
        /// `ollama` (a model on this computer, free: needs Ollama and --model), `claude-code` (your
        /// `claude` command), or a command that reads the prompt on stdin
        #[arg(long, default_value = "anthropic")]
        ai: String,
        /// The model (`anthropic`: claude-haiku-5-5 by default, the cheapest; claude-sonnet-5-5 or
        /// claude-opus-5-5 for harder mods. `ollama`: an installed model, such as a coding model)
        #[arg(long)]
        model: Option<String>,
        /// How hard the model thinks: low, medium, high, xhigh or max (`anthropic`; default: the model's)
        #[arg(long)]
        effort: Option<String>,
        /// Only functions whose name contains this
        #[arg(long)]
        only: Option<String>,
        /// At most this many functions
        #[arg(long)]
        limit: Option<usize>,
        /// Tries per function
        #[arg(long, default_value_t = 3)]
        attempts: usize,
        /// List what would be translated and how big each prompt is; call no model
        #[arg(long)]
        dry_run: bool,
        /// Go ahead without the estimate (it asks when there are more than 5 functions)
        #[arg(long)]
        yes: bool,
    },
    /// Add a ported mod to one of your Horizon instances, beside its other mods
    Install {
        /// The folder `port` wrote
        path: PathBuf,
        /// The instance (default: the one the launcher's Play starts)
        #[arg(long)]
        instance: Option<String>,
        /// The Horizon launcher (default: found on PATH and in its usual places)
        #[arg(long, value_name = "PATH")]
        launcher: Option<PathBuf>,
        /// Build the instance's native mods afterwards (needs --trust-native-code)
        #[arg(long)]
        build: bool,
        /// You read the code of the instance's native mods and trust it with full access to this computer
        #[arg(long)]
        trust_native_code: bool,
    },
    /// Play a ported mod in the real game and say what doesn't work there: it opens a
    /// test world in the instance it is installed in, places each of its blocks and
    /// right-clicks those that act
    Check {
        /// The folder `port` wrote (installed and built in the instance)
        path: PathBuf,
        /// The instance (default: the one the launcher's Play starts)
        #[arg(long)]
        instance: Option<String>,
        /// The Horizon launcher (default: found on PATH and in its usual places)
        #[arg(long, value_name = "PATH")]
        launcher: Option<PathBuf>,
        /// At most this many blocks
        #[arg(long)]
        limit: Option<usize>,
        /// Leave the game running afterwards
        #[arg(long)]
        keep: bool,
    },
    /// Play a ported mod in its instance, with its content loaded, without building it again
    Play {
        /// The folder `port` wrote
        path: PathBuf,
        /// The instance to play (default: "<mod id> port")
        #[arg(long)]
        instance: Option<String>,
        /// The Horizon launcher (default: found on PATH and in its usual places)
        #[arg(long, value_name = "PATH")]
        launcher: Option<PathBuf>,
    },
    /// Pack a ported mod into a .hnmod (its native half) and a .hmod (its content) to share or publish
    Package {
        /// The folder `port` wrote
        path: PathBuf,
        /// Where to write them (default: the folder itself)
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Show the port's progress, class by class (not implemented yet)
    Status { path: PathBuf },
}

/// 1234567 → "1,234,567".
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Scan { path, json } => {
            let scan = porter_java::scan(&path)?;
            print!("{}", porter_report::text_summary(&scan));
            if let Some(out) = json {
                let text = serde_json::to_string_pretty(&scan)?;
                fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
                println!("\nFull scan written to {}", out.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Report {
            path,
            all,
            top,
            html,
            embedded,
        } => {
            let scan = porter_java::scan(&path)?;
            match html {
                Some(out) => {
                    let page = if embedded {
                        porter_report::html_embedded(&scan)
                    } else {
                        porter_report::html(&scan)
                    };
                    fs::write(&out, page).with_context(|| format!("writing {}", out.display()))?;
                    println!("Report written to {}", out.display());
                }
                None => print!("{}", porter_report::text_full(&scan, all, top)),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Port { path, out, force } => {
            // A jar is decompiled into a source tree first (once; it is kept).
            let source = if path.is_dir() {
                path.clone()
            } else {
                let dir = porter_java::decompile::decompile(
                    &path,
                    &porter_java::decompile::cache_dir(),
                    |line| eprintln!("{line}"),
                )?;
                println!(
                    "Porting the source decompiled from {} ({})",
                    path.display(),
                    dir.display()
                );
                dir
            };
            let scan = porter_java::scan(&source)?;
            let lang = porter_translate::bundle::load_lang(&scan);
            let bundle = porter_translate::bundle::generate_with(
                &scan,
                &lang,
                porter_map::ledger::horizon_source().as_deref(),
            );
            porter_translate::bundle::write(&bundle, &out, force)?;
            let what = match (bundle.native, bundle.content.is_empty()) {
                (true, true) => "a Horizon native mod",
                (true, false) => "a Horizon native mod and content mod",
                _ => "a Horizon content mod",
            };
            println!("Wrote {what} for `{}` to {}\n", bundle.id, out.display());
            let c = &bundle.content;
            if !c.is_empty() {
                println!(
                    "Content: {} blocks, {} items, {} resource files (content/)",
                    c.blocks.len(),
                    c.items.len(),
                    c.copies.len()
                );
            }
            if !bundle.block_entities.is_empty() {
                println!(
                    "Block entities: {}",
                    bundle
                        .block_entities
                        .iter()
                        .map(|b| format!("{} ({} saved fields)", b.id, b.fields.len()))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !bundle.native {
                println!(
                    "Next: play it with horizon-porter build --play; PORTING.md lists what was carried."
                );
                return Ok(ExitCode::SUCCESS);
            }
            println!(
                "Wired hooks: {}",
                bundle
                    .wired
                    .keys()
                    .map(|h| format!("native_{h}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !bundle.stubbed.is_empty() {
                println!(
                    "Hooks to port into (helpers ready): {}",
                    bundle
                        .stubbed
                        .keys()
                        .map(|h| format!("native_{h}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !bundle.unwired.is_empty() {
                println!(
                    "To wire by hand: {}",
                    bundle
                        .unwired
                        .keys()
                        .map(|h| format!("native_{h}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            println!(
                "Next: port the Java logic into mixins/{}/ (runtime.rs and the hook helpers); PORTING.md lists every class and Mixin.",
                bundle.id
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Build {
            path,
            instance,
            launcher,
            trust_native_code,
            play,
        } => {
            let port = porter_verify::Port::read(&path)?;
            let id = port.id.clone();
            if port.native.is_some() && !trust_native_code {
                eprintln!(
                    "Native mods are not sandboxed: this compiles the Rust in {} into Horizon, and it runs with your rights.\n\
                     Read it, and if you trust it, run this again with --trust-native-code.",
                    path.join("mixins").join(&id).display()
                );
                return Ok(ExitCode::FAILURE);
            }
            let launcher = porter_verify::Launcher::find(launcher.as_deref())?;
            let name = instance.unwrap_or_else(|| format!("{id} port"));
            let instance = launcher.ensure_instance(&name)?;
            if let Some(folder) = &port.native {
                println!(
                    "Building {} into the instance \"{}\" with {}. The first build downloads Rust and takes a while.",
                    id, instance.name, launcher.version
                );
                // The launcher's own lines, its result last, already show as progress.
                launcher.native_build(
                    folder,
                    &instance,
                    porter_verify::Trust::given_by_player(),
                    |line| eprintln!("{line}"),
                )?;
                println!();
            } else {
                println!(
                    "{id} is a content mod: nothing to build. It plays in the instance \"{}\".",
                    instance.name
                );
            }
            let dev_mods: Vec<&std::path::Path> = port.content.as_deref().into_iter().collect();
            if play {
                launcher.play(&instance, &dev_mods)?;
                println!("Starting \"{}\".", instance.name);
                if port.content.is_some() {
                    println!(
                        "Its blocks and items load from content/: restart the game after editing it."
                    );
                }
            } else if port.content.is_some() {
                println!(
                    "Play it: horizon-porter play {}, which loads content/ too.",
                    path.display()
                );
            } else {
                println!(
                    "Play it: horizon-porter play {}, or pick \"{}\" in the launcher.",
                    path.display(),
                    instance.name
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Translate {
            path,
            ai,
            model,
            effort,
            only,
            limit,
            attempts,
            dry_run,
            yes,
        } => {
            let info: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(path.join(".porter").join("port.json")).with_context(|| {
                    format!(
                        "{} has no .porter/port.json: port the mod again with this version of horizon-porter",
                        path.display()
                    )
                })?,
            )?;
            let input = PathBuf::from(info["input"].as_str().unwrap_or_default());
            if !input.is_dir() {
                anyhow::bail!(
                    "the Java source this was ported from, {}, is not there any more",
                    input.display()
                );
            }
            let scan = porter_java::scan(&input)?;
            let mut opts = porter_translate::ai::Options {
                only,
                limit,
                attempts,
                dry_run: true,
            };
            // What it would send, first.
            let plan = porter_translate::ai::translate(
                &path,
                &scan,
                &porter_translate::ai::model::External {
                    command: "none".into(),
                },
                &opts,
                |_| {},
            )?;
            if plan.is_empty() {
                println!(
                    "Nothing left to translate in {}: every function is ported.",
                    path.display()
                );
                return Ok(ExitCode::SUCCESS);
            }
            let chars: usize = plan
                .iter()
                .map(|o| match o.status {
                    porter_translate::ai::Status::Planned { prompt_chars } => prompt_chars,
                    _ => 0,
                })
                .sum();
            let tokens = chars / 4;
            println!(
                "{} functions to translate, about {} input tokens for one try each (fixes add more).",
                plan.len(),
                thousands(tokens)
            );
            // About a thousand tokens come back per function.
            let output = plan.len() * 1000;
            match ai.as_str() {
                "anthropic" | "claude" | "api" => {
                    let name = model
                        .clone()
                        .unwrap_or_else(|| porter_translate::ai::model::DEFAULT_MODEL.into());
                    if let Some((input_price, output_price)) =
                        porter_translate::ai::model::price(&name)
                    {
                        let dollars =
                            tokens as f64 / 1e6 * input_price + output as f64 / 1e6 * output_price;
                        println!(
                            "With {name}: about ${dollars:.2} (${input_price:.2} and ${output_price:.2} per million tokens in and out). Free on your own computer with --ai ollama."
                        );
                    }
                }
                "ollama" | "local" => println!("On your computer with Ollama: free."),
                _ => {}
            }
            if dry_run {
                for o in &plan {
                    if let porter_translate::ai::Status::Planned { prompt_chars } = o.status {
                        println!(
                            "  {:<48} {:>8} tokens  {}",
                            o.unit.function,
                            thousands(prompt_chars / 4),
                            o.unit.file
                        );
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
            if plan.len() > 5 && !yes {
                println!(
                    "That calls the model at least {} times. Run it again with --yes to go ahead, or narrow it with --only or --limit.",
                    plan.len()
                );
                return Ok(ExitCode::FAILURE);
            }
            opts.dry_run = false;
            let model =
                porter_translate::ai::model::from_choice(&ai, model.as_deref(), effort.as_deref())?;
            println!("Translating with {}.", model.name());
            let out =
                porter_translate::ai::translate(&path, &scan, model.as_ref(), &opts, |line| {
                    eprintln!("{line}")
                })?;
            let count = |f: &dyn Fn(&porter_translate::ai::Status) -> bool| {
                out.iter().filter(|o| f(&o.status)).count()
            };
            use porter_translate::ai::Status;
            let (input_tokens, output_tokens) = out.iter().fold((0, 0), |(i, o), x| {
                (i + x.input_tokens, o + x.output_tokens)
            });
            println!(
                "\nPorted and tested: {}. Ported, checked when you build: {}. Left as they were: {}.",
                count(&|s| *s == Status::Ported),
                count(&|s| *s == Status::Unchecked),
                count(&|s| matches!(s, Status::Failed(_)))
            );
            if let Some(why) = out.iter().find_map(|o| match &o.status {
                Status::NoAnswer(why) => Some(why.clone()),
                _ => None,
            }) {
                println!(
                    "It stopped because the model gave no answer: {why}\nThe functions after it are untouched: run translate again once that is fixed."
                );
            }
            if input_tokens > 0 {
                println!(
                    "Tokens: {} in, {} out.",
                    thousands(input_tokens as usize),
                    thousands(output_tokens as usize)
                );
            }
            println!(
                "Next: read what changed (each ported function says so above it), then build: horizon-porter build {} --trust-native-code",
                path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Check {
            path,
            instance,
            launcher,
            limit,
            keep,
        } => {
            let launcher = porter_verify::Launcher::find(launcher.as_deref())?;
            let list = launcher.instances()?;
            let target = match &instance {
                Some(name) => list
                    .into_iter()
                    .find(|i| i.name.eq_ignore_ascii_case(name) || i.id.eq_ignore_ascii_case(name)),
                None => list.into_iter().find(|i| i.selected),
            }
            .with_context(|| match &instance {
                Some(name) => {
                    format!("no instance named {name}: `horizon-launcher instance list` lists them")
                }
                None => "no instance is selected in the launcher".to_string(),
            })?;
            let options = porter_verify::play::PlayOptions {
                limit: limit.unwrap_or(0),
                keep,
            };
            let report = porter_verify::play::play_check(&target, &path, &options, |line| {
                eprintln!("{line}")
            })?;
            println!(
                "
{}.",
                report.summary()
            );
            for f in &report.faults {
                println!("FAULT {f}");
            }
            for n in &report.notes {
                println!("note  {n}");
            }
            Ok(if report.faults.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Install {
            path,
            instance,
            launcher,
            build,
            trust_native_code,
        } => {
            let port = porter_verify::Port::read(&path)?;
            if build && !trust_native_code {
                eprintln!(
                    "Building compiles the Rust of every native mod in the instance into Horizon, and it runs with your rights.\n\
                     Read it, and if you trust it, run this again with --trust-native-code."
                );
                return Ok(ExitCode::FAILURE);
            }
            let launcher = porter_verify::Launcher::find(launcher.as_deref())?;
            let list = launcher.instances()?;
            let target = match &instance {
                Some(name) => list
                    .into_iter()
                    .find(|i| i.name.eq_ignore_ascii_case(name) || i.id.eq_ignore_ascii_case(name)),
                None => list.into_iter().find(|i| i.selected),
            }
            .with_context(|| match &instance {
                Some(name) => {
                    format!("no instance named {name}: `horizon-launcher instance list` lists them")
                }
                None => "no instance is selected in the launcher".to_string(),
            })?;
            if !target.plays_horizon() {
                anyhow::bail!(
                    "\"{}\" plays Java Edition: Horizon mods go in a Horizon instance",
                    target.name
                );
            }
            let home = target
                .home
                .clone()
                .context("the launcher gave no game folder for that instance")?;
            let files =
                porter_translate::bundle::package(&path, &path.join(".porter").join("dist"))?;
            let pick = |ext: &str| {
                files
                    .iter()
                    .find(|f| f.extension().is_some_and(|e| e == ext))
                    .cloned()
            };
            let installed = porter_verify::install(
                &home,
                &port.id,
                pick("hmod").as_deref(),
                pick("hnmod").as_deref(),
            )?;
            for f in installed.content.iter().chain(installed.native.iter()) {
                println!("Added {}", f.display());
            }
            println!("Turned on {} in \"{}\".", port.id, target.name);
            if installed.native.is_none() {
                println!("It loads the next time \"{}\" starts.", target.name);
            } else if build {
                println!("Building \"{}\" with {}.", target.name, launcher.version);
                launcher.build_instance(
                    &target,
                    porter_verify::Trust::given_by_player(),
                    |line| eprintln!("{line}"),
                )?;
                println!("\nDone: play \"{}\" from the launcher.", target.name);
            } else {
                println!(
                    "Its code is built with the instance's other native mods: the launcher's Mods page shows \"Needs rebuild\", \
                     or run this again with --build --trust-native-code."
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Play {
            path,
            instance,
            launcher,
        } => {
            let port = porter_verify::Port::read(&path)?;
            let launcher = porter_verify::Launcher::find(launcher.as_deref())?;
            let name = instance.unwrap_or_else(|| format!("{} port", port.id));
            let instance = launcher.ensure_instance(&name)?;
            let dev_mods: Vec<&std::path::Path> = port.content.as_deref().into_iter().collect();
            launcher.play(&instance, &dev_mods)?;
            println!("Starting \"{}\".", instance.name);
            if port.native.is_some() {
                println!(
                    "It plays the last build: run horizon-porter build after changing the Rust."
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Package { path, out } => {
            let out_dir = out.unwrap_or_else(|| path.clone());
            for file in porter_translate::bundle::package(&path, &out_dir)? {
                println!("Packed {}", file.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Status { .. } => {
            eprintln!("not implemented yet: see docs/ROADMAP.md");
            Ok(ExitCode::FAILURE)
        }
    }
}
