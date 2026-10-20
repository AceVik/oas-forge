#[cfg(feature = "cli")]
use colored::Colorize;
#[cfg(feature = "cli")]
use oas_forge::{Generator, config::Config};
#[cfg(feature = "cli")]
use std::path::PathBuf;

#[cfg(feature = "cli")]
fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Load configuration (CLI + TOML + Cargo.toml)
    let mut config = Config::load();
    apply_default_output(&mut config);
    let outputs = configured_outputs(&config);

    println!("{} Starting oas-forge...", "INFO:".blue().bold());

    // Set up Generator
    match Generator::new().with_config(config).generate() {
        Ok(_) => {
            println!(
                "{} Successfully generated OpenAPI definition(s) at {:?}",
                "SUCCESS:".green().bold(),
                outputs
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("{} {}", "ERROR:".red().bold(), e);
            Err(anyhow::anyhow!(e))
        }
    }
}

/// Falls back to `openapi.yaml` when no output of any kind is configured.
#[cfg(feature = "cli")]
fn apply_default_output(config: &mut Config) {
    if configured_outputs(config).is_empty() {
        config.output = Some(vec![PathBuf::from("openapi.yaml")]);
    }
}

/// Every file the generator will write, across all output strategies.
#[cfg(feature = "cli")]
fn configured_outputs(config: &Config) -> Vec<PathBuf> {
    [
        &config.output,
        &config.output_schemas,
        &config.output_paths,
        &config.output_fragments,
    ]
    .into_iter()
    .flatten()
    .flatten()
    .cloned()
    .collect()
}

#[cfg(not(feature = "cli"))]
fn main() {
    eprintln!("This binary requires the 'cli' feature to be enabled.");
    std::process::exit(1);
}
