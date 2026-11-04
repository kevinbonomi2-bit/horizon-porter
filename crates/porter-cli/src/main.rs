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
    /// Generate the Horizon bundle and content mod (not implemented yet)
    Port { path: PathBuf },
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
        Command::Port { .. } | Command::Check { .. } | Command::Status { .. } => {
            eprintln!("not implemented yet: see docs/ROADMAP.md");
            Ok(ExitCode::FAILURE)
        }
    }
}
