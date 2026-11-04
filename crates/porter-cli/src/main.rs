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
        /// You read the mod's Rust code and trust it with full access to this computer
        #[arg(long)]
        trust_native_code: bool,
        /// Start the instance when the build is done
        #[arg(long)]
        play: bool,
    },
    /// Pack a ported mod into a .hnmod to share or publish
    Package {
        /// The folder `port` wrote
        path: PathBuf,
        /// Where to write the .hnmod (default: the folder itself)
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Show the port's progress, class by class (not implemented yet)
    Status { path: PathBuf },
}

/// The mod id of a folder `port` wrote: the name of its single `mixins/<id>/`.
fn mod_id(dir: &std::path::Path) -> Result<String> {
    let folder = porter_verify::build_folder(dir)?;
    let bundle = if folder == dir {
        fs::read_dir(dir.join("mixins"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.join("horizon-mixin.toml").is_file())
            .with_context(|| format!("{} has no bundle under mixins/", dir.display()))?
    } else {
        folder
    };
    Ok(bundle
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default())
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
            let bundle = porter_translate::bundle::generate(&scan, &lang);
            porter_translate::bundle::write(&bundle, &out, force)?;
            println!(
                "Wrote a Horizon native mod bundle for `{}` to {}\n",
                bundle.id,
                out.display()
            );
            println!(
                "Wired hooks: {}",
                bundle
                    .wired
                    .keys()
                    .map(|h| format!("native_{h}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
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
                "Next: port the Java logic into mixins/{}/runtime.rs; PORTING.md lists every class.",
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
            let folder = porter_verify::build_folder(&path)?;
            let id = mod_id(&path)?;
            if !trust_native_code {
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
            println!(
                "Building {} into the instance \"{}\" with {}. The first build downloads Rust and takes a while.",
                id, instance.name, launcher.version
            );
            let done = launcher.native_build(
                &folder,
                &instance,
                porter_verify::Trust::given_by_player(),
                |line| eprintln!("{line}"),
            )?;
            println!("\n{done}");
            if play {
                launcher.play(&instance)?;
                println!("Starting \"{}\".", instance.name);
            } else {
                println!(
                    "Play it: pick \"{}\" in the launcher, or run horizon-porter build again with --play.",
                    instance.name
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Package { path, out } => {
            let out_dir = out.unwrap_or_else(|| path.clone());
            let file = porter_translate::bundle::package(&path, &out_dir)?;
            println!("Packed {}", file.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Status { .. } => {
            eprintln!("not implemented yet: see docs/ROADMAP.md");
            Ok(ExitCode::FAILURE)
        }
    }
}
