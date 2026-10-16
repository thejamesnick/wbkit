//! wbkit: the WebBox packaging tool. Turns a web game folder plus manifest.json into a .wbx package.
//! All format rules live in the library (`wbkit_core::wbx`); this file only handles the command line.

mod files;
mod manifest;
mod report;
mod warnings;

use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf, process::ExitCode};
use wbkit_core::wbx::{build_package, read_package, Limits};

/// What publish.webbox.fun accepts, so problems show up here instead of at upload.
pub const UPLOAD_LIMITS: Limits = Limits { max_package_bytes: 50 * 1024 * 1024, max_files: 512, max_file_bytes: 50 * 1024 * 1024, max_total_bytes: 200 * 1024 * 1024 };

#[derive(Parser)]
#[command(name = "wbkit", version, about = "Package web games and apps for WebBox", after_help = "Docs: https://publish.webbox.fun/docs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write a starter manifest.json into a game folder
    Init {
        /// The folder holding your built game
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
    /// Package a game folder into a .wbx
    Bundle {
        /// The folder holding your built game (the one with index.html)
        dir: PathBuf,
        /// Where to write the package (default: <id>-<version>.wbx)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// The manifest to use (default: manifest.json in the folder)
        #[arg(short, long)]
        manifest: Option<PathBuf>,
        /// Leave out the build time, so the same files always give the same bytes
        #[arg(long, conflicts_with = "timestamp")]
        reproducible: bool,
        /// Stamp the package with this ISO time (default: SOURCE_DATE_EPOCH if set, otherwise now)
        #[arg(long)]
        timestamp: Option<String>,
        /// Compression level, 0 to 9
        #[arg(long, default_value_t = 6, value_parser = clap::value_parser!(u32).range(0..=9))]
        compress: u32,
        /// List every file packed
        #[arg(short, long)]
        verbose: bool,
    },
    /// Check a .wbx completely: format, manifest, every file and checksum, and upload limits
    Validate { file: PathBuf },
    /// Show a package's listing details and files
    Info { file: PathBuf },
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Init { dir } => {
            let path = dir.join("manifest.json");
            if path.exists() { return Err(format!("{} already exists; edit it instead", path.display())); }
            if !dir.join("index.html").exists() { eprintln!("note: no index.html in {}; set \"entry_point\" to your start page", dir.display()); }
            let text = serde_json::to_string_pretty(&manifest::starter(&dir)).map_err(|e| e.to_string())?;
            fs::write(&path, text + "\n").map_err(|e| format!("can't write {}: {}", path.display(), e))?;
            println!("Wrote {}\nSet your author, rating, category and permissions, then run: wbkit bundle {}", path.display(), dir.display());
            Ok(())
        }
        Command::Bundle { dir, output, manifest: manifest_path, reproducible, timestamp, compress, verbose } => {
            if !dir.is_dir() { return Err(format!("{} is not a folder", dir.display())); }
            let manifest_path = manifest_path.unwrap_or_else(|| dir.join("manifest.json"));
            if !manifest_path.exists() { return Err(format!("no manifest at {}; run \"wbkit init {}\" to create one", manifest_path.display(), dir.display())); }
            let manifest = manifest::load(&dir, &manifest_path)?;
            let files = files::collect(&dir)?;
            if verbose { for (path, data) in &files { println!("  {:>10}  {}", report::size(data.len()), path); } }
            let created = if reproducible { None } else { Some(timestamp.unwrap_or_else(report::now_or_source_date)) };
            if manifest.game_type == wbkit_core::wbx::GameType::Bundled {
                for (page, url) in warnings::external_scripts(&files) {
                    eprintln!("warning: {} loads a script from another website ({}). Bundled titles can only load their own files on WebBox, so it won't load for players. Copy it into your game folder and link it locally (for the SDK: <script src=\"webbox.js\">).", page, url);
                }
            }
            let bytes = build_package(&manifest, &files, compress, created.as_deref(), &UPLOAD_LIMITS).map_err(report::explain)?;
            read_package(&bytes, &UPLOAD_LIMITS).map_err(report::explain)?;
            let output = output.unwrap_or_else(|| PathBuf::from(format!("{}-{}.wbx", manifest.id, manifest.version)));
            fs::write(&output, &bytes).map_err(|e| format!("can't write {}: {}", output.display(), e))?;
            println!("Created {} ({}, {} files)\n{} {} is ready to upload at https://publish.webbox.fun", output.display(), report::size(bytes.len()), files.len(), manifest.name, manifest.version);
            Ok(())
        }
        Command::Validate { file } => {
            let bytes = fs::read(&file).map_err(|e| format!("can't read {}: {}", file.display(), e))?;
            let package = read_package(&bytes, &UPLOAD_LIMITS).map_err(report::explain)?;
            println!("Valid: {} {} ({}, {} files, format {})", package.header.manifest.name, package.header.manifest.version, report::size(bytes.len()), package.files.len(), package.header.version);
            Ok(())
        }
        Command::Info { file } => {
            let bytes = fs::read(&file).map_err(|e| format!("can't read {}: {}", file.display(), e))?;
            let package = read_package(&bytes, &Limits::default()).map_err(report::explain)?;
            report::info(&package, bytes.len());
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => { eprintln!("error: {}", message); ExitCode::FAILURE }
    }
}
