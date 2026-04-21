use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use harness::{
    config::RuntimeConfig,
    management::{
        self, BackgroundEnqueueOutcome, BackgroundJobSummary, BackgroundRunNextOutcome,
        EnqueueBackgroundJobRequest, PendingForegroundConversationSummary, RuntimeStatusReport,
        WakeSignalSummary,
    },
};

#[derive(Debug, Parser)]
pub struct AdminCommand {
    #[command(subcommand)]
    pub command: AdminSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminSubcommand {
    Status(StatusCommand),
    Foreground(ForegroundCommand),
    Background(BackgroundCommand),
    #[command(name = "wake-signals")]
    WakeSignals(WakeSignalsCommand),
}

#[derive(Debug, Args)]
pub struct StatusCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct ForegroundCommand {
    #[command(subcommand)]
    pub command: ForegroundSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ForegroundSubcommand {
    Pending(ListCommand),
}

#[derive(Debug, Parser)]
pub struct BackgroundCommand {
    #[command(subcommand)]
    pub command: BackgroundSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum BackgroundSubcommand {
    List(ListCommand),
    Enqueue(BackgroundEnqueueCommand),
    #[command(name = "run-next")]
    RunNext(RunNextCommand),
}

#[derive(Debug, Args)]
pub struct ListCommand {
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct BackgroundEnqueueCommand {
    #[arg(long, value_enum)]
    pub job_kind: JobKindArg,
    #[arg(long, value_enum, default_value_t = TriggerKindArg::MaintenanceTrigger)]
    pub trigger_kind: TriggerKindArg,
    #[arg(long)]
    pub conversation_ref: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RunNextCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct WakeSignalsCommand {
    #[command(subcommand)]
    pub command: WakeSignalsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WakeSignalsSubcommand {
    List(ListCommand),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum JobKindArg {
    MemoryConsolidation,
    RetrievalMaintenance,
    ContradictionAndDriftScan,
    SelfModelReflection,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TriggerKindArg {
    TimeSchedule,
    VolumeThreshold,
    DriftOrAnomalySignal,
    ForegroundDelegation,
    ExternalPassiveEvent,
    MaintenanceTrigger,
}

impl From<JobKindArg> for contracts::UnconsciousJobKind {
    fn from(value: JobKindArg) -> Self {
        match value {
            JobKindArg::MemoryConsolidation => Self::MemoryConsolidation,
            JobKindArg::RetrievalMaintenance => Self::RetrievalMaintenance,
            JobKindArg::ContradictionAndDriftScan => Self::ContradictionAndDriftScan,
            JobKindArg::SelfModelReflection => Self::SelfModelReflection,
        }
    }
}

impl From<TriggerKindArg> for contracts::BackgroundTriggerKind {
    fn from(value: TriggerKindArg) -> Self {
        match value {
            TriggerKindArg::TimeSchedule => Self::TimeSchedule,
            TriggerKindArg::VolumeThreshold => Self::VolumeThreshold,
            TriggerKindArg::DriftOrAnomalySignal => Self::DriftOrAnomalySignal,
            TriggerKindArg::ForegroundDelegation => Self::ForegroundDelegation,
            TriggerKindArg::ExternalPassiveEvent => Self::ExternalPassiveEvent,
            TriggerKindArg::MaintenanceTrigger => Self::MaintenanceTrigger,
        }
    }
}

pub async fn run_admin_command(config: &RuntimeConfig, command: AdminCommand) -> Result<()> {
    match command.command {
        AdminSubcommand::Status(command) => {
            let report = management::load_runtime_status(config).await?;
            print_status(report, command.json)?;
        }
        AdminSubcommand::Foreground(command) => match command.command {
            ForegroundSubcommand::Pending(command) => {
                let summaries =
                    management::list_pending_foreground_conversations(config, command.limit)
                        .await?;
                print_pending_foreground(summaries, command.json)?;
            }
        },
        AdminSubcommand::Background(command) => match command.command {
            BackgroundSubcommand::List(command) => {
                let jobs = management::list_background_jobs(config, command.limit).await?;
                print_background_jobs(jobs, command.json)?;
            }
            BackgroundSubcommand::Enqueue(command) => {
                let outcome = management::enqueue_background_job(
                    config,
                    EnqueueBackgroundJobRequest {
                        job_kind: command.job_kind.into(),
                        trigger_kind: command.trigger_kind.into(),
                        internal_conversation_ref: command.conversation_ref,
                        reason: command.reason,
                    },
                )
                .await?;
                print_background_enqueue(outcome, command.json)?;
            }
            BackgroundSubcommand::RunNext(command) => {
                let outcome = management::run_next_background_job(config).await?;
                print_background_run_next(outcome, command.json)?;
            }
        },
        AdminSubcommand::WakeSignals(command) => match command.command {
            WakeSignalsSubcommand::List(command) => {
                let signals = management::list_wake_signals(config, command.limit).await?;
                print_wake_signals(signals, command.json)?;
            }
        },
    }
    Ok(())
}

fn print_status(report: RuntimeStatusReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Schema");
    println!("  compatibility: {}", report.schema.compatibility);
    println!(
        "  current/expected/minimum: {}/{}/{}",
        report
            .schema
            .current_version
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        report.schema.expected_version,
        report.schema.minimum_supported_version
    );
    println!(
        "  applied migrations: {}",
        report.schema.applied_migration_count
    );
    println!("  history valid: {}", yes_no(report.schema.history_valid));
    if let Some(details) = &report.schema.details {
        println!("  details: {details}");
    }

    println!("Worker");
    println!("  resolution: {}", report.worker.resolution_kind);
    println!(
        "  command: {}",
        report.worker.command.as_deref().unwrap_or("unresolved")
    );
    println!(
        "  args: {}",
        if report.worker.args.is_empty() {
            "none".to_string()
        } else {
            report.worker.args.join(", ")
        }
    );
    println!("  timeout_ms: {}", report.worker.timeout_ms);
    println!("  notes: {}", report.worker.notes);

    println!("Telegram");
    println!("  configured: {}", yes_no(report.telegram.configured));
    println!(
        "  binding present: {}",
        yes_no(report.telegram.binding_present)
    );
    if let Some(internal_conversation_ref) = &report.telegram.binding_internal_conversation_ref {
        println!("  binding conversation: {internal_conversation_ref}");
    }
    if let Some(internal_principal_ref) = &report.telegram.binding_internal_principal_ref {
        println!("  binding principal: {internal_principal_ref}");
    }
    if let Some(bot_token_env) = &report.telegram.bot_token_env {
        println!(
            "  bot token env: {} ({})",
            bot_token_env,
            presence_label(report.telegram.bot_token_present)
        );
    }
    if let Some(poll_limit) = report.telegram.poll_limit {
        println!("  poll limit: {poll_limit}");
    }

    println!("Model gateway");
    println!("  configured: {}", yes_no(report.model_gateway.configured));
    if let Some(provider) = &report.model_gateway.provider {
        println!("  provider: {provider}");
    }
    if let Some(model) = &report.model_gateway.model {
        println!("  model: {model}");
    }
    if let Some(api_base_url) = &report.model_gateway.api_base_url {
        println!("  api base url: {api_base_url}");
    }
    if let Some(api_key_env) = &report.model_gateway.api_key_env {
        println!(
            "  api key env: {} ({})",
            api_key_env,
            presence_label(report.model_gateway.api_key_present)
        );
    }
    if let Some(timeout_ms) = report.model_gateway.timeout_ms {
        println!("  timeout_ms: {timeout_ms}");
    }

    println!("Self model");
    println!("  configured: {}", yes_no(report.self_model.configured));
    if let Some(seed_path) = &report.self_model.seed_path {
        println!("  seed path: {seed_path}");
        println!("  seed exists: {}", yes_no(report.self_model.seed_exists));
    }

    println!("Pending work");
    println!(
        "  foreground conversations: {}",
        report.pending_work.pending_foreground_conversation_count
    );
    println!(
        "  planned background jobs: {}",
        report.pending_work.pending_background_job_count
    );
    println!(
        "  due background jobs: {}",
        report.pending_work.due_background_job_count
    );
    println!(
        "  pending wake signals: {}",
        report.pending_work.pending_wake_signal_count
    );

    Ok(())
}

fn print_pending_foreground(
    summaries: Vec<PendingForegroundConversationSummary>,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
        return Ok(());
    }

    if summaries.is_empty() {
        println!("No pending foreground conversations.");
        return Ok(());
    }

    for summary in summaries {
        println!(
            "{} | pending={} | mode={} | reason={} | stale_processing={} | oldest={} | newest={}",
            summary.internal_conversation_ref,
            summary.pending_count,
            summary.suggested_mode,
            summary.decision_reason,
            yes_no(summary.includes_stale_processing),
            summary.oldest_occurred_at,
            summary.newest_occurred_at
        );
    }
    Ok(())
}

fn print_background_jobs(jobs: Vec<BackgroundJobSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&jobs)?);
        return Ok(());
    }

    if jobs.is_empty() {
        println!("No background jobs found.");
        return Ok(());
    }

    for job in jobs {
        println!(
            "{} | {} | trigger={} | status={} | available_at={} | latest_run={} | conversation={} | scope={}",
            job.background_job_id,
            job.job_kind,
            job.trigger_kind,
            job.status,
            job.available_at,
            job.latest_run_status.unwrap_or_else(|| "none".to_string()),
            job.internal_conversation_ref
                .unwrap_or_else(|| "global".to_string()),
            job.scope_summary
        );
    }
    Ok(())
}

fn print_background_enqueue(outcome: BackgroundEnqueueOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }

    match outcome {
        BackgroundEnqueueOutcome::Planned {
            background_job_id,
            deduplication_key,
            scope_summary,
        } => {
            println!("Planned background job {background_job_id}");
            println!("  deduplication key: {deduplication_key}");
            println!("  scope: {scope_summary}");
        }
        BackgroundEnqueueOutcome::SuppressedDuplicate {
            existing_job_id,
            deduplication_key,
            reason,
        } => {
            println!("Background enqueue suppressed by duplicate detection.");
            println!("  existing job: {existing_job_id}");
            println!("  deduplication key: {deduplication_key}");
            println!("  reason: {reason}");
        }
        BackgroundEnqueueOutcome::Rejected { reason } => {
            println!("Background enqueue rejected.");
            println!("  reason: {reason}");
        }
    }

    Ok(())
}

fn print_background_run_next(outcome: BackgroundRunNextOutcome, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }

    match outcome {
        BackgroundRunNextOutcome::NoDueJob => println!("No due background job."),
        BackgroundRunNextOutcome::Completed {
            background_job_id,
            execution_id,
            trace_id,
            summary,
        } => {
            println!("Completed background job {background_job_id}");
            println!("  execution_id: {execution_id}");
            println!("  trace_id: {trace_id}");
            println!("  summary: {summary}");
        }
    }
    Ok(())
}

fn print_wake_signals(signals: Vec<WakeSignalSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&signals)?);
        return Ok(());
    }

    if signals.is_empty() {
        println!("No wake signals found.");
        return Ok(());
    }

    for signal in signals {
        println!(
            "{} | {} | priority={} | status={} | decision={} | requested_at={}",
            signal.wake_signal_id,
            signal.reason_code,
            signal.priority,
            signal.status,
            signal.decision_kind.unwrap_or_else(|| "none".to_string()),
            signal.requested_at
        );
    }
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn presence_label(present: bool) -> &'static str {
    if present { "present" } else { "missing" }
}
