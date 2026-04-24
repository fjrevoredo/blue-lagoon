mod admin;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use harness::{
    config::RuntimeConfig,
    runtime::{HarnessOptions, SyntheticTrigger, TelegramOptions},
    trace,
};
use std::path::PathBuf;

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
    Telegram(TelegramCommand),
    Admin(admin::AdminCommand),
}

#[derive(Debug, Parser)]
#[command(about = "Run the harness service by default, or execute one-shot harness actions")]
struct HarnessCommand {
    #[arg(
        long,
        help = "Run a one-shot harness action instead of the continuous service"
    )]
    once: bool,
    #[arg(
        long,
        default_value_t = false,
        conflicts_with_all = ["background_once", "synthetic_trigger"],
        help = "Verify schema and startup safety once, then return to idle"
    )]
    idle: bool,
    #[arg(
        long,
        default_value_t = false,
        conflicts_with_all = ["idle", "synthetic_trigger"],
        help = "Run one due background-maintenance job and exit"
    )]
    background_once: bool,
    #[arg(
        long,
        value_enum,
        help = "Run one synthetic foreground trigger and exit"
    )]
    synthetic_trigger: Option<SyntheticTriggerArg>,
}

#[derive(Debug, Parser)]
#[command(
    about = "Run the Telegram poller service by default, or execute one-shot Telegram actions"
)]
struct TelegramCommand {
    #[arg(
        long,
        conflicts_with = "poll_once",
        help = "Replay one stored Telegram fixture file"
    )]
    fixture: Option<PathBuf>,
    #[arg(
        long,
        default_value_t = false,
        conflicts_with = "fixture",
        help = "Fetch and process one live Telegram poll cycle"
    )]
    poll_once: bool,
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
            if command.once {
                let outcome = harness::runtime::run_harness_once(
                    &config,
                    HarnessOptions {
                        once: true,
                        idle: command.idle,
                        background_once: command.background_once,
                        synthetic_trigger: command.synthetic_trigger.map(|trigger| match trigger {
                            SyntheticTriggerArg::Smoke => SyntheticTrigger::Smoke,
                        }),
                    },
                )
                .await?;
                println!("{outcome:?}");
            } else {
                anyhow::ensure!(
                    !command.idle
                        && !command.background_once
                        && command.synthetic_trigger.is_none(),
                    "--idle, --background-once, and --synthetic-trigger require --once"
                );
                harness::runtime::run_harness_service(&config).await?;
            }
        }
        Command::Telegram(command) => {
            let config = RuntimeConfig::load()?;
            trace::init(&config.app.log_filter)?;
            if command.fixture.is_some() || command.poll_once {
                let outcome = harness::runtime::run_telegram_once(
                    &config,
                    TelegramOptions {
                        fixture_path: command.fixture,
                        poll_once: command.poll_once,
                    },
                )
                .await?;
                println!("{outcome:?}");
            } else {
                harness::runtime::run_telegram_service(&config).await?;
            }
        }
        Command::Admin(command) => {
            let config = RuntimeConfig::load()?;
            trace::init(&config.app.log_filter)?;
            admin::run_admin_command(&config, command).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_command_defaults_to_service_mode() {
        let cli = Cli::try_parse_from(["runtime", "harness"])
            .expect("harness command without flags should parse");

        match cli.command {
            Command::Harness(command) => {
                assert!(!command.once);
                assert!(!command.idle);
                assert!(!command.background_once);
                assert!(command.synthetic_trigger.is_none());
            }
            other => panic!("expected harness command, got {other:?}"),
        }
    }

    #[test]
    fn telegram_command_defaults_to_service_mode() {
        let cli = Cli::try_parse_from(["runtime", "telegram"])
            .expect("telegram command without flags should parse");

        match cli.command {
            Command::Telegram(command) => {
                assert!(command.fixture.is_none());
                assert!(!command.poll_once);
            }
            other => panic!("expected telegram command, got {other:?}"),
        }
    }
}
