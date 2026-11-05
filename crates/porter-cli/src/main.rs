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
        /// The mod's source directory (a Gradle project)
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
            if !path.is_dir() {
                anyhow::bail!("port needs the mod's source folder, not a jar");
            }
            let scan = porter_java::scan(&path)?;
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
