//! The `oas-forge` binary.

#![forbid(unsafe_code)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::dbg_macro,
    clippy::todo,
    clippy::unimplemented
)]

use clap::Parser;
use colored::Colorize;
use oas_forge::Error;
use oas_forge::diagnostics::Diagnostic;
use oas_forge_cli::{Cli, MessageFormat, run};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.message_format == MessageFormat::Json;
    if !json {
        println!("{} Starting oas-forge...", "INFO:".blue().bold());
    }
    match run(&cli, std::path::Path::new(".")) {
        Ok(outcome) => {
            for diagnostic in &outcome.diagnostics {
                if json {
                    println!("{}", diagnostic.to_json());
                } else {
                    eprintln!("{}", human(diagnostic));
                }
            }
            if cli.check {
                if outcome.outdated.is_empty() {
                    if !json {
                        println!("{} Every output is up to date", "SUCCESS:".green().bold());
                    }
                    return ExitCode::SUCCESS;
                }
                for path in &outcome.outdated {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({ "severity": "error", "message": "output is not up to date", "file": path.display().to_string() })
                        );
                    } else {
                        eprintln!(
                            "{} {} is not up to date",
                            "ERROR:".red().bold(),
                            path.display()
                        );
                    }
                }
                return ExitCode::FAILURE;
            }
            if !json {
                println!(
                    "{} Successfully generated OpenAPI definition(s) at {:?}",
                    "SUCCESS:".green().bold(),
                    outcome.outputs
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if json {
                match &error {
                    Error::Diagnostics(findings) => {
                        for finding in findings {
                            println!("{}", finding.to_json());
                        }
                    }
                    other => println!(
                        "{}",
                        serde_json::json!({ "severity": "error", "message": other.to_string() })
                    ),
                }
            } else {
                eprintln!("{} {}", "ERROR:".red().bold(), error);
            }
            ExitCode::FAILURE
        }
    }
}

/// `warning[OF5001]: message (file:line)` with a colored label. A
/// successful run reports warnings only: errors fail it, notes are dropped.
fn human(diagnostic: &Diagnostic) -> String {
    let label = format!("{}[{}]", diagnostic.severity(), diagnostic.code());
    let location = diagnostic
        .location()
        .map(|location| format!(" ({location})"))
        .unwrap_or_default();
    format!(
        "{}: {}{location}",
        label.yellow().bold(),
        diagnostic.message()
    )
}
