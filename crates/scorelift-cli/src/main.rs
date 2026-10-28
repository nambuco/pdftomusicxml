mod debug;

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
