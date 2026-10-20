use clap::Parser;
use colored::Colorize;
use oas_forge_cli::{Cli, run};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    println!("{} Starting oas-forge...", "INFO:".blue().bold());
    match run(&cli, std::path::Path::new(".")) {
        Ok(outcome) => {
            for diagnostic in &outcome.diagnostics {
                eprintln!("{} {diagnostic}", "WARNING:".yellow().bold());
            }
            println!(
                "{} Successfully generated OpenAPI definition(s) at {:?}",
                "SUCCESS:".green().bold(),
                outcome.outputs
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{} {}", "ERROR:".red().bold(), e);
            ExitCode::FAILURE
        }
    }
}
