use clap::Parser;
use colored::Colorize;
use oas_forge_cli::{Cli, run};
use std::process::ExitCode;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    println!("{} Starting oas-forge...", "INFO:".blue().bold());
    match run(&cli, std::path::Path::new(".")) {
        Ok(outputs) => {
            println!(
                "{} Successfully generated OpenAPI definition(s) at {:?}",
                "SUCCESS:".green().bold(),
                outputs
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{} {}", "ERROR:".red().bold(), e);
            ExitCode::FAILURE
        }
    }
}
