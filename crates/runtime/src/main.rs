use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use harness::{
    config::RuntimeConfig,
    runtime::{HarnessOptions, SyntheticTrigger},
    trace,
};

#[derive(Debug, Parser)]
#[command(name = "runtime", about = "Blue Lagoon runtime entrypoints")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Migrate,
    Harness(HarnessCommand),
}

#[derive(Debug, Parser)]
struct HarnessCommand {
    #[arg(long)]
    once: bool,
    #[arg(long, default_value_t = false, conflicts_with = "synthetic_trigger")]
    idle: bool,
    #[arg(long, value_enum)]
    synthetic_trigger: Option<SyntheticTriggerArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SyntheticTriggerArg {
    Smoke,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Migrate => {
            let config = RuntimeConfig::load()?;
            trace::init(&config.app.log_filter)?;
            let summary = harness::runtime::run_migrate(&config).await?;
            println!(
                "discovered migrations: {:?}; applied migrations: {:?}",
                summary.discovered_versions, summary.applied_versions
            );
        }
        Command::Harness(command) => {
            let config = RuntimeConfig::load()?;
            trace::init(&config.app.log_filter)?;
            let outcome = harness::runtime::run_harness_once(
                &config,
                HarnessOptions {
                    once: command.once,
                    idle: command.idle,
                    synthetic_trigger: command.synthetic_trigger.map(|trigger| match trigger {
                        SyntheticTriggerArg::Smoke => SyntheticTrigger::Smoke,
                    }),
                },
            )
            .await?;
            println!("{outcome:?}");
        }
    }
    Ok(())
}
