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
    /// Build the port against Horizon and collect errors (not implemented yet)
    Check { path: PathBuf },
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
        } => {
            let scan = porter_java::scan(&path)?;
            match html {
                Some(out) => {
                    fs::write(&out, porter_report::html(&scan))
                        .with_context(|| format!("writing {}", out.display()))?;
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
        Command::Check { .. } | Command::Status { .. } => {
            eprintln!("not implemented yet: see docs/ROADMAP.md");
            Ok(ExitCode::FAILURE)
        }
    }
}
