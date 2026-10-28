mod debug;
mod eval;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use scorelift_model::{fixtures, validate};

#[derive(Debug, Parser)]
#[command(
    name = "scorelift",
    version,
    about = "Convert PDF sheet music to MusicXML"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Convert a PDF or image to MusicXML (.musicxml or .mxl).
    Convert {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write a built-in demo score, to try the MusicXML writer.
    Demo {
        /// Fixture name; omit to list them.
        name: Option<String>,
        #[arg(short, long, default_value = "demo.mxl")]
        output: PathBuf,
    },
    /// Analyze staves, barlines and measures, and write overlay images and layout.json.
    Debug {
        input: PathBuf,
        /// Directory for the overlay PNGs and layout.json.
        #[arg(short, long, default_value = "debug")]
        output: PathBuf,
        /// Also save each image unmodified, as `<name>.raw.png`.
        #[arg(long)]
        raw: bool,
    },
    /// Convert a PDF or image and compare it, bar by bar, with a correct
    /// MusicXML transcription of the same music.
    Eval {
        input: PathBuf,
        /// The correct transcription (.musicxml or .mxl).
        truth: PathBuf,
        /// Bars to compare, e.g. `1-40` (1-based, inclusive); default all.
        #[arg(long)]
        bars: Option<String>,
        /// Part to compare, counting from 1.
        #[arg(long, default_value_t = 1)]
        part: usize,
        /// Also list the bars that match exactly.
        #[arg(long)]
        all: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Debug { input, output, raw } => debug::run(&input, &output, raw)?,
        Command::Eval {
            input,
            truth,
            bars,
            part,
            all,
        } => eval::run(&input, &truth, bars.as_deref(), part, all)?,
        Command::Convert { input, output } => {
            let conversion = scorelift_core::convert(&input, &Default::default())?;
            for w in &conversion.warnings {
                eprintln!("warning: {w}");
            }
            for issue in validate::validate(&conversion.score) {
                eprintln!("warning: {issue}");
            }
            if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            scorelift_musicxml::write_file(&conversion.score, &output)?;
            println!("wrote {}", output.display());
        }
        Command::Demo { name: None, .. } => {
            for name in fixtures::NAMES {
                println!("{name}");
            }
        }
        Command::Demo {
            name: Some(name),
            output,
        } => {
            let score = fixtures::by_name(&name).ok_or_else(|| {
                format!(
                    "unknown demo {name:?}; choose one of: {}",
                    fixtures::NAMES.join(", ")
                )
            })?;
            if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            scorelift_musicxml::write_file(&score, &output)?;
            println!("wrote {}", output.display());
        }
    }
    Ok(())
}
