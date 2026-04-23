use std::fmt::Write as _;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use harness::{
    config::RuntimeConfig,
    management::{
        self, ApprovalRequestSummary, ApprovalResolutionSummary, BackgroundEnqueueOutcome,
        BackgroundJobSummary, BackgroundRunNextOutcome, EnqueueBackgroundJobRequest,
        GovernedActionSummary, OperationalDiagnosticSummary, OperationalHealthSummary,
        PendingForegroundConversationSummary, RecoveryCheckpointSummary, RecoverySupervisionReport,
        ResolveApprovalRequest, RuntimeStatusReport, SchemaStatusReport,
        SchemaUpgradeAssessmentReport, SuperviseWorkerLeasesRequest, WakeSignalSummary,
        WorkerLeaseInspectionSummary, WorkspaceScriptRunSummary,
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
    Health(HealthCommand),
    Diagnostics(DiagnosticsCommand),
    Recovery(RecoveryCommand),
    Schema(SchemaCommand),
    Foreground(ForegroundCommand),
    Background(BackgroundCommand),
    Approvals(ApprovalsCommand),
    Actions(ActionsCommand),
    Workspace(WorkspaceCommand),
    #[command(name = "wake-signals")]
    WakeSignals(WakeSignalsCommand),
}

#[derive(Debug, Args)]
pub struct StatusCommand {
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct HealthCommand {
    #[command(subcommand)]
    pub command: HealthSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum HealthSubcommand {
    Summary(StatusCommand),
}

#[derive(Debug, Parser)]
pub struct DiagnosticsCommand {
    #[command(subcommand)]
    pub command: DiagnosticsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum DiagnosticsSubcommand {
    List(ListCommand),
}

#[derive(Debug, Parser)]
pub struct RecoveryCommand {
    #[command(subcommand)]
    pub command: RecoverySubcommand,
}

#[derive(Debug, Subcommand)]
pub enum RecoverySubcommand {
    Checkpoints(RecoveryCheckpointsCommand),
    Leases(RecoveryLeasesCommand),
    Supervise(RecoverySuperviseCommand),
}

#[derive(Debug, Parser)]
pub struct RecoveryCheckpointsCommand {
    #[command(subcommand)]
    pub command: RecoveryCheckpointsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum RecoveryCheckpointsSubcommand {
    List(RecoveryCheckpointListCommand),
}

#[derive(Debug, Parser)]
pub struct RecoveryLeasesCommand {
    #[command(subcommand)]
    pub command: RecoveryLeasesSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum RecoveryLeasesSubcommand {
    List(RecoveryLeaseListCommand),
}

#[derive(Debug, Args)]
pub struct RecoveryCheckpointListCommand {
    #[arg(long, default_value_t = false)]
    pub open_only: bool,
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RecoveryLeaseListCommand {
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub soft_warning_threshold_percent: u8,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RecoverySuperviseCommand {
    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub soft_warning_threshold_percent: u8,
    #[arg(long)]
    pub actor_ref: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct SchemaCommand {
    #[command(subcommand)]
    pub command: SchemaSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SchemaSubcommand {
    Status(StatusCommand),
    #[command(name = "upgrade-path")]
    UpgradePath(StatusCommand),
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

#[derive(Debug, Parser)]
pub struct ApprovalsCommand {
    #[command(subcommand)]
    pub command: ApprovalsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ApprovalsSubcommand {
    List(ApprovalsListCommand),
    Resolve(ApprovalResolveCommand),
}

#[derive(Debug, Args)]
pub struct ApprovalsListCommand {
    #[arg(long, value_enum)]
    pub status: Option<ApprovalStatusArg>,
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ApprovalResolveCommand {
    #[arg(long)]
    pub approval_request_id: String,
    #[arg(long, value_enum)]
    pub decision: ApprovalDecisionArg,
    #[arg(long)]
    pub actor_ref: Option<String>,
    #[arg(long)]
    pub reason: Option<String>,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct ActionsCommand {
    #[command(subcommand)]
    pub command: ActionsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ActionsSubcommand {
    List(ActionsListCommand),
}

#[derive(Debug, Args)]
pub struct ActionsListCommand {
    #[arg(long, value_enum)]
    pub status: Option<GovernedActionStatusArg>,
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Parser)]
pub struct WorkspaceCommand {
    #[command(subcommand)]
    pub command: WorkspaceSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceSubcommand {
    Artifacts(WorkspaceArtifactsCommand),
    Scripts(WorkspaceScriptsCommand),
    Runs(WorkspaceRunsCommand),
}

#[derive(Debug, Parser)]
pub struct WorkspaceArtifactsCommand {
    #[command(subcommand)]
    pub command: WorkspaceArtifactsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceArtifactsSubcommand {
    List(ListCommand),
}

#[derive(Debug, Parser)]
pub struct WorkspaceScriptsCommand {
    #[command(subcommand)]
    pub command: WorkspaceScriptsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceScriptsSubcommand {
    List(ListCommand),
}

#[derive(Debug, Parser)]
pub struct WorkspaceRunsCommand {
    #[command(subcommand)]
    pub command: WorkspaceRunsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkspaceRunsSubcommand {
    List(WorkspaceRunsListCommand),
}

#[derive(Debug, Args)]
pub struct WorkspaceRunsListCommand {
    #[arg(long)]
    pub script_id: Option<String>,
    #[arg(long, default_value_t = management::default_list_limit())]
    pub limit: u32,
    #[arg(long, default_value_t = false)]
    pub json: bool,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ApprovalStatusArg {
    Pending,
    Approved,
    Rejected,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ApprovalDecisionArg {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GovernedActionStatusArg {
    Proposed,
    AwaitingApproval,
    Approved,
    Rejected,
    Expired,
    Invalidated,
    Blocked,
    Executed,
    Failed,
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

impl From<ApprovalStatusArg> for contracts::ApprovalRequestStatus {
    fn from(value: ApprovalStatusArg) -> Self {
        match value {
            ApprovalStatusArg::Pending => Self::Pending,
            ApprovalStatusArg::Approved => Self::Approved,
            ApprovalStatusArg::Rejected => Self::Rejected,
            ApprovalStatusArg::Expired => Self::Expired,
            ApprovalStatusArg::Invalidated => Self::Invalidated,
        }
    }
}

impl From<ApprovalDecisionArg> for contracts::ApprovalResolutionDecision {
    fn from(value: ApprovalDecisionArg) -> Self {
        match value {
            ApprovalDecisionArg::Approve => Self::Approved,
            ApprovalDecisionArg::Reject => Self::Rejected,
        }
    }
}

impl From<GovernedActionStatusArg> for contracts::GovernedActionStatus {
    fn from(value: GovernedActionStatusArg) -> Self {
        match value {
            GovernedActionStatusArg::Proposed => Self::Proposed,
            GovernedActionStatusArg::AwaitingApproval => Self::AwaitingApproval,
            GovernedActionStatusArg::Approved => Self::Approved,
            GovernedActionStatusArg::Rejected => Self::Rejected,
            GovernedActionStatusArg::Expired => Self::Expired,
            GovernedActionStatusArg::Invalidated => Self::Invalidated,
            GovernedActionStatusArg::Blocked => Self::Blocked,
            GovernedActionStatusArg::Executed => Self::Executed,
            GovernedActionStatusArg::Failed => Self::Failed,
        }
    }
}

pub async fn run_admin_command(config: &RuntimeConfig, command: AdminCommand) -> Result<()> {
    match command.command {
        AdminSubcommand::Status(command) => {
            let report = management::load_runtime_status(config).await?;
            print_status(report, command.json)?;
        }
        AdminSubcommand::Health(command) => match command.command {
            HealthSubcommand::Summary(command) => {
                let summary = management::load_operational_health_summary(config).await?;
                print_health_summary(summary, command.json)?;
            }
        },
        AdminSubcommand::Diagnostics(command) => match command.command {
            DiagnosticsSubcommand::List(command) => {
                let diagnostics =
                    management::list_recent_operational_diagnostics(config, command.limit).await?;
                print_diagnostics(diagnostics, command.json)?;
            }
        },
        AdminSubcommand::Recovery(command) => match command.command {
            RecoverySubcommand::Checkpoints(command) => match command.command {
                RecoveryCheckpointsSubcommand::List(command) => {
                    let checkpoints = management::list_recovery_checkpoints(
                        config,
                        command.open_only,
                        command.limit,
                    )
                    .await?;
                    print_recovery_checkpoints(checkpoints, command.json)?;
                }
            },
            RecoverySubcommand::Leases(command) => match command.command {
                RecoveryLeasesSubcommand::List(command) => {
                    let leases = management::list_active_worker_leases(
                        config,
                        command.limit,
                        command.soft_warning_threshold_percent,
                    )
                    .await?;
                    print_recovery_leases(leases, command.json)?;
                }
            },
            RecoverySubcommand::Supervise(command) => {
                let report = management::supervise_worker_leases(
                    config,
                    SuperviseWorkerLeasesRequest {
                        soft_warning_threshold_percent: command.soft_warning_threshold_percent,
                        actor_ref: command
                            .actor_ref
                            .unwrap_or_else(|| "cli:operator".to_string()),
                        reason: command.reason,
                    },
                )
                .await?;
                print_recovery_supervision(report, command.json)?;
            }
        },
        AdminSubcommand::Schema(command) => match command.command {
            SchemaSubcommand::Status(command) => {
                let report = management::load_schema_status(config).await?;
                print_schema_status(report, command.json)?;
            }
            SchemaSubcommand::UpgradePath(command) => {
                let report = management::load_schema_upgrade_assessment(config).await?;
                print_schema_upgrade_assessment(report, command.json)?;
            }
        },
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
        AdminSubcommand::Approvals(command) => match command.command {
            ApprovalsSubcommand::List(command) => {
                let approvals = management::list_approval_requests(
                    config,
                    command.status.map(Into::into),
                    command.limit,
                )
                .await?;
                print_approval_requests(approvals, command.json)?;
            }
            ApprovalsSubcommand::Resolve(command) => {
                let outcome = management::resolve_approval_request(
                    config,
                    ResolveApprovalRequest {
                        approval_request_id: command.approval_request_id.parse()?,
                        decision: command.decision.into(),
                        actor_ref: command.actor_ref,
                        reason: command.reason,
                    },
                )
                .await?;
                print_approval_resolution(outcome, command.json)?;
            }
        },
        AdminSubcommand::Actions(command) => match command.command {
            ActionsSubcommand::List(command) => {
                let actions = management::list_governed_actions(
                    config,
                    command.status.map(Into::into),
                    command.limit,
                )
                .await?;
                print_governed_actions(actions, command.json)?;
            }
        },
        AdminSubcommand::Workspace(command) => match command.command {
            WorkspaceSubcommand::Artifacts(command) => match command.command {
                WorkspaceArtifactsSubcommand::List(command) => {
                    let artifacts =
                        management::list_workspace_artifact_summaries(config, command.limit)
                            .await?;
                    print_workspace_artifacts(artifacts, command.json)?;
                }
            },
            WorkspaceSubcommand::Scripts(command) => match command.command {
                WorkspaceScriptsSubcommand::List(command) => {
                    let scripts = management::list_workspace_scripts(config, command.limit).await?;
                    print_workspace_scripts(scripts, command.json)?;
                }
            },
            WorkspaceSubcommand::Runs(command) => match command.command {
                WorkspaceRunsSubcommand::List(command) => {
                    let script_id = command.script_id.map(|value| value.parse()).transpose()?;
                    let runs =
                        management::list_workspace_script_runs(config, script_id, command.limit)
                            .await?;
                    print_workspace_runs(runs, command.json)?;
                }
            },
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

    print_schema_status_section(&report.schema);

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
    println!(
        "  pending approval requests: {}",
        report.pending_work.pending_approval_request_count
    );
    println!(
        "  awaiting approval governed actions: {}",
        report.pending_work.awaiting_approval_governed_action_count
    );
    println!(
        "  blocked governed actions: {}",
        report.pending_work.blocked_governed_action_count
    );

    Ok(())
}

fn print_health_summary(summary: OperationalHealthSummary, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    println!("{}", render_health_summary_text(&summary));
    Ok(())
}

fn print_diagnostics(diagnostics: Vec<OperationalDiagnosticSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&diagnostics)?);
        return Ok(());
    }

    println!("{}", render_diagnostics_text(&diagnostics));
    Ok(())
}

fn print_recovery_checkpoints(
    checkpoints: Vec<RecoveryCheckpointSummary>,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&checkpoints)?);
        return Ok(());
    }

    println!("{}", render_recovery_checkpoints_text(&checkpoints));
    Ok(())
}

fn print_recovery_leases(leases: Vec<WorkerLeaseInspectionSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&leases)?);
        return Ok(());
    }

    println!("{}", render_recovery_leases_text(&leases));
    Ok(())
}

fn print_recovery_supervision(report: RecoverySupervisionReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("{}", render_recovery_supervision_text(&report));
    Ok(())
}

fn print_schema_status(report: SchemaStatusReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_schema_status_section(&report);
    Ok(())
}

fn print_schema_upgrade_assessment(
    report: SchemaUpgradeAssessmentReport,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("{}", render_schema_upgrade_assessment_text(&report));
    Ok(())
}

fn print_schema_status_section(report: &SchemaStatusReport) {
    println!("Schema");
    println!("  compatibility: {}", report.compatibility);
    println!(
        "  current/expected/minimum: {}/{}/{}",
        report
            .current_version
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        report.expected_version,
        report.minimum_supported_version
    );
    println!("  applied migrations: {}", report.applied_migration_count);
    println!("  history valid: {}", yes_no(report.history_valid));
    if let Some(details) = &report.details {
        println!("  details: {details}");
    }
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

fn print_approval_requests(summaries: Vec<ApprovalRequestSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
        return Ok(());
    }

    println!("{}", render_approval_requests_text(&summaries));
    Ok(())
}

fn print_approval_resolution(summary: ApprovalResolutionSummary, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    println!("{}", render_approval_resolution_text(&summary));
    Ok(())
}

fn print_governed_actions(actions: Vec<GovernedActionSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&actions)?);
        return Ok(());
    }

    println!("{}", render_governed_actions_text(&actions));
    Ok(())
}

fn print_workspace_artifacts(
    artifacts: Vec<contracts::WorkspaceArtifactSummary>,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&artifacts)?);
        return Ok(());
    }

    println!("{}", render_workspace_artifacts_text(&artifacts));
    Ok(())
}

fn print_workspace_scripts(
    scripts: Vec<contracts::WorkspaceScriptSummary>,
    json: bool,
) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&scripts)?);
        return Ok(());
    }

    println!("{}", render_workspace_scripts_text(&scripts));
    Ok(())
}

fn print_workspace_runs(runs: Vec<WorkspaceScriptRunSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(());
    }

    println!("{}", render_workspace_runs_text(&runs));
    Ok(())
}

fn render_approval_requests_text(summaries: &[ApprovalRequestSummary]) -> String {
    if summaries.is_empty() {
        return "No approval requests.".to_string();
    }

    let mut output = String::new();
    for (index, summary) in summaries.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | status={} | risk={} | kind={} | requested_by={} | requested_at={} | expires_at={} | title={}",
            summary.approval_request_id,
            summary.status,
            summary.risk_tier,
            summary.action_kind,
            summary.requested_by,
            summary.requested_at,
            summary.expires_at,
            summary.title
        );
        if let Some(resolution_kind) = summary.resolution_kind.as_deref() {
            let _ = writeln!(
                output,
                "  resolved={} by={} at={} reason={}",
                resolution_kind,
                summary.resolved_by.as_deref().unwrap_or("unknown"),
                summary
                    .resolved_at
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                summary.resolution_reason.as_deref().unwrap_or("none")
            );
        }
    }
    output.trim_end().to_string()
}

fn render_approval_resolution_text(summary: &ApprovalResolutionSummary) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Approval {} resolved as {}",
        summary.approval_request.approval_request_id, summary.approval_request.status
    );
    let _ = writeln!(output, "  title: {}", summary.approval_request.title);
    let _ = writeln!(
        output,
        "  resolved_by: {}",
        summary
            .approval_request
            .resolved_by
            .as_deref()
            .unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  reason: {}",
        summary
            .approval_request
            .resolution_reason
            .as_deref()
            .unwrap_or("none")
    );
    if let Some(action) = &summary.governed_action {
        let _ = writeln!(
            output,
            "  governed action: {} status={} output_ref={}",
            action.governed_action_execution_id,
            action.status,
            action.output_ref.as_deref().unwrap_or("none")
        );
    }
    output.trim_end().to_string()
}

fn render_governed_actions_text(actions: &[GovernedActionSummary]) -> String {
    if actions.is_empty() {
        return "No governed actions.".to_string();
    }

    let mut output = String::new();
    for (index, action) in actions.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | status={} | risk={} | kind={} | approval_request_id={} | started={} | completed={}",
            action.governed_action_execution_id,
            action.status,
            action.risk_tier,
            action.action_kind,
            action
                .approval_request_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            action
                .started_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            action
                .completed_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        if let Some(blocked_reason) = action.blocked_reason.as_deref() {
            let _ = writeln!(output, "  blocked_reason: {blocked_reason}");
        }
        if let Some(output_ref) = action.output_ref.as_deref() {
            let _ = writeln!(output, "  output_ref: {output_ref}");
        }
    }
    output.trim_end().to_string()
}

fn render_workspace_artifacts_text(artifacts: &[contracts::WorkspaceArtifactSummary]) -> String {
    if artifacts.is_empty() {
        return "No workspace artifacts.".to_string();
    }

    let mut output = String::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | kind={:?} | latest_version={} | updated_at={} | title={}",
            artifact.artifact_id,
            artifact.artifact_kind,
            artifact.latest_version,
            artifact.updated_at,
            artifact.title
        );
    }
    output.trim_end().to_string()
}

fn render_workspace_scripts_text(scripts: &[contracts::WorkspaceScriptSummary]) -> String {
    if scripts.is_empty() {
        return "No workspace scripts.".to_string();
    }

    let mut output = String::new();
    for (index, script) in scripts.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | artifact_id={} | language={} | latest_version={} | updated_at={}",
            script.script_id,
            script.workspace_artifact_id,
            script.language,
            script.latest_version,
            script.updated_at
        );
    }
    output.trim_end().to_string()
}

fn render_workspace_runs_text(runs: &[WorkspaceScriptRunSummary]) -> String {
    if runs.is_empty() {
        return "No workspace script runs.".to_string();
    }

    let mut output = String::new();
    for (index, run) in runs.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | script_id={} | status={} | risk={} | started={} | completed={}",
            run.workspace_script_run_id,
            run.workspace_script_id,
            run.status,
            run.risk_tier,
            run.started_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            run.completed_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        if let Some(output_ref) = run.output_ref.as_deref() {
            let _ = writeln!(output, "  output_ref: {output_ref}");
        }
        if let Some(failure_summary) = run.failure_summary.as_deref() {
            let _ = writeln!(output, "  failure_summary: {failure_summary}");
        }
    }
    output.trim_end().to_string()
}

fn render_health_summary_text(summary: &OperationalHealthSummary) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Health | overall_status={} | evaluated_at={}",
        summary.overall_status, summary.evaluated_at
    );
    let _ = writeln!(
        output,
        "  pending_work: foreground={} background_pending={} background_due={} wake_signals={} approvals={} blocked_actions={}",
        summary.pending_work.pending_foreground_conversation_count,
        summary.pending_work.pending_background_job_count,
        summary.pending_work.due_background_job_count,
        summary.pending_work.pending_wake_signal_count,
        summary.pending_work.pending_approval_request_count,
        summary.pending_work.blocked_governed_action_count
    );
    let _ = writeln!(
        output,
        "  recovery: open_checkpoints={} foreground={} background={} governed_actions={} overdue_leases={} at_risk_leases={} recent_resolved={} recent_abandoned={}",
        summary.recovery.open_checkpoint_count,
        summary.recovery.open_foreground_checkpoint_count,
        summary.recovery.open_background_checkpoint_count,
        summary.recovery.open_governed_action_checkpoint_count,
        summary.recovery.overdue_active_worker_lease_count,
        summary.recovery.at_risk_active_worker_lease_count,
        summary.recovery.recent_resolved_checkpoint_count,
        summary.recovery.recent_abandoned_checkpoint_count
    );
    let _ = writeln!(
        output,
        "  diagnostics: observed={} info={} warn={} error={} critical={}",
        summary.diagnostics.observed_count,
        summary.diagnostics.info_count,
        summary.diagnostics.warn_count,
        summary.diagnostics.error_count,
        summary.diagnostics.critical_count
    );

    if summary.diagnostics.top_reason_codes.is_empty() {
        let _ = writeln!(output, "  top_reason_codes: none");
    } else {
        for reason in &summary.diagnostics.top_reason_codes {
            let _ = writeln!(
                output,
                "  top_reason_code: {} count={} latest_at={}",
                reason.reason_code, reason.count, reason.latest_at
            );
        }
    }

    if summary.anomalies.is_empty() {
        let _ = writeln!(output, "  anomalies: none");
    } else {
        for anomaly in &summary.anomalies {
            let _ = writeln!(
                output,
                "  anomaly: kind={} severity={} reason={} count={} first_seen={} last_seen={} summary={}",
                anomaly.anomaly_kind,
                anomaly.severity,
                anomaly.reason_code,
                anomaly.occurrence_count,
                anomaly.first_seen_at,
                anomaly.last_seen_at,
                anomaly.summary
            );
        }
    }

    output.trim_end().to_string()
}

fn render_diagnostics_text(diagnostics: &[OperationalDiagnosticSummary]) -> String {
    if diagnostics.is_empty() {
        return "No operational diagnostics.".to_string();
    }

    let mut output = String::new();
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | subsystem={} | severity={} | reason={} | created_at={} | summary={}",
            diagnostic.operational_diagnostic_id,
            diagnostic.subsystem,
            diagnostic.severity,
            diagnostic.reason_code,
            diagnostic.created_at,
            diagnostic.summary
        );
        let _ = writeln!(
            output,
            "  trace_id={} execution_id={}",
            diagnostic
                .trace_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            diagnostic
                .execution_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
    }
    output.trim_end().to_string()
}

fn render_recovery_checkpoints_text(checkpoints: &[RecoveryCheckpointSummary]) -> String {
    if checkpoints.is_empty() {
        return "No recovery checkpoints.".to_string();
    }

    let mut output = String::new();
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | kind={} | reason={} | status={} | created_at={} | resolved_at={}",
            checkpoint.recovery_checkpoint_id,
            checkpoint.checkpoint_kind,
            checkpoint.recovery_reason_code,
            checkpoint.status,
            checkpoint.created_at,
            checkpoint
                .resolved_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        let _ = writeln!(
            output,
            "  trace_id={} execution_id={} background_job_id={} governed_action_execution_id={} approval_request_id={} decision={}",
            checkpoint.trace_id,
            checkpoint
                .execution_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            checkpoint
                .background_job_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            checkpoint
                .governed_action_execution_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            checkpoint
                .approval_request_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            checkpoint.recovery_decision.as_deref().unwrap_or("none")
        );
    }
    output.trim_end().to_string()
}

fn render_recovery_leases_text(leases: &[WorkerLeaseInspectionSummary]) -> String {
    if leases.is_empty() {
        return "No active worker leases.".to_string();
    }

    let mut output = String::new();
    for (index, lease) in leases.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        let _ = writeln!(
            output,
            "{} | kind={} | lease_status={} | supervision_status={} | lease_expires_at={} | last_heartbeat_at={}",
            lease.worker_lease_id,
            lease.worker_kind,
            lease.lease_status,
            lease.supervision_status,
            lease.lease_expires_at,
            lease.last_heartbeat_at
        );
        let _ = writeln!(
            output,
            "  trace_id={} execution_id={} background_job_id={} background_job_run_id={} governed_action_execution_id={} lease_acquired_at={} released_at={}",
            lease.trace_id,
            lease
                .execution_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            lease
                .background_job_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            lease
                .background_job_run_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            lease
                .governed_action_execution_id
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
            lease.lease_acquired_at,
            lease
                .released_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
    }
    output.trim_end().to_string()
}

fn render_recovery_supervision_text(report: &RecoverySupervisionReport) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Recovery supervision completed at {} | trace_id={} | actor_ref={} | soft_warnings={} | recovered_expired_leases={}",
        report.supervised_at,
        report.trace_id,
        report.actor_ref,
        report.soft_warning_count,
        report.recovered_expired_lease_count
    );
    let _ = writeln!(
        output,
        "  reason={}",
        report.reason.as_deref().unwrap_or("none")
    );

    if report.soft_warning_diagnostics.is_empty() {
        let _ = writeln!(output, "  soft_warning_diagnostics: none");
    } else {
        for diagnostic in &report.soft_warning_diagnostics {
            let _ = writeln!(
                output,
                "  soft_warning_diagnostic: {} reason={} severity={} created_at={}",
                diagnostic.operational_diagnostic_id,
                diagnostic.reason_code,
                diagnostic.severity,
                diagnostic.created_at
            );
        }
    }

    if report.recovered_expired_leases.is_empty() {
        let _ = writeln!(output, "  recovered_expired_leases: none");
    } else {
        for lease in &report.recovered_expired_leases {
            let _ = writeln!(
                output,
                "  recovered_expired_lease: {} kind={} checkpoint={} status={} decision={} diagnostic_reason={} diagnostic_severity={}",
                lease.worker_lease_id,
                lease.worker_kind,
                lease.checkpoint_id,
                lease.checkpoint_status,
                lease.recovery_decision,
                lease.diagnostic_reason_code,
                lease.diagnostic_severity
            );
        }
    }

    output.trim_end().to_string()
}

fn render_schema_upgrade_assessment_text(report: &SchemaUpgradeAssessmentReport) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Schema upgrade path | compatibility={} | current={} | expected={} | minimum_supported={}",
        report.compatibility,
        report
            .current_version
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
        report.expected_version,
        report.minimum_supported_version
    );
    let _ = writeln!(
        output,
        "  discovered_versions={}",
        render_versions(&report.discovered_versions)
    );
    let _ = writeln!(
        output,
        "  applied_versions={}",
        render_versions(&report.applied_versions)
    );
    let _ = writeln!(
        output,
        "  pending_versions={}",
        render_versions(&report.pending_versions)
    );
    let _ = writeln!(output, "  history_valid={}", yes_no(report.history_valid));
    if let Some(details) = &report.details {
        let _ = writeln!(output, "  details={details}");
    }
    output.trim_end().to_string()
}

fn render_versions(versions: &[i64]) -> String {
    if versions.is_empty() {
        "none".to_string()
    } else {
        versions
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_approval_request_summary() -> ApprovalRequestSummary {
        serde_json::from_value(json!({
            "approval_request_id": "00000000-0000-0000-0000-000000000001",
            "trace_id": "00000000-0000-0000-0000-000000000002",
            "execution_id": null,
            "action_proposal_id": "00000000-0000-0000-0000-000000000003",
            "action_fingerprint": "sha256:test",
            "action_kind": "run_subprocess",
            "risk_tier": "tier_2",
            "capability_scope": {
                "filesystem": {
                    "read_roots": ["D:/Repos/blue-lagoon"],
                    "write_roots": ["D:/Repos/blue-lagoon/docs"]
                },
                "network": "disabled",
                "environment": {
                    "allow_variables": []
                },
                "execution": {
                    "timeout_ms": 30000,
                    "max_stdout_bytes": 65536,
                    "max_stderr_bytes": 32768
                }
            },
            "status": "approved",
            "title": "Run bounded subprocess",
            "consequence_summary": "Executes a scoped subprocess.",
            "requested_by": "telegram:primary-user",
            "requested_at": "2026-04-22T10:00:00Z",
            "expires_at": "2026-04-22T10:15:00Z",
            "resolved_at": "2026-04-22T10:05:00Z",
            "resolution_kind": "approved",
            "resolved_by": "cli:primary-user",
            "resolution_reason": "manual verification"
        }))
        .expect("sample approval request summary should deserialize")
    }

    fn sample_governed_action_summary() -> GovernedActionSummary {
        serde_json::from_value(json!({
            "governed_action_execution_id": "00000000-0000-0000-0000-000000000011",
            "trace_id": "00000000-0000-0000-0000-000000000012",
            "execution_id": null,
            "approval_request_id": "00000000-0000-0000-0000-000000000001",
            "action_proposal_id": "00000000-0000-0000-0000-000000000013",
            "action_fingerprint": "sha256:test-action",
            "action_kind": "run_subprocess",
            "risk_tier": "tier_2",
            "status": "blocked",
            "workspace_script_id": null,
            "workspace_script_version_id": null,
            "blocked_reason": "scope invalid",
            "output_ref": "execution_record:00000000-0000-0000-0000-000000000099",
            "started_at": null,
            "completed_at": "2026-04-22T10:06:00Z"
        }))
        .expect("sample governed action should deserialize")
    }

    fn sample_workspace_run_summary() -> WorkspaceScriptRunSummary {
        serde_json::from_value(json!({
            "workspace_script_run_id": "00000000-0000-0000-0000-000000000021",
            "workspace_script_id": "00000000-0000-0000-0000-000000000022",
            "workspace_script_version_id": "00000000-0000-0000-0000-000000000023",
            "trace_id": "00000000-0000-0000-0000-000000000024",
            "execution_id": null,
            "governed_action_execution_id": null,
            "approval_request_id": null,
            "status": "failed",
            "risk_tier": "tier_1",
            "args": ["--check"],
            "output_ref": null,
            "failure_summary": "script returned non-zero exit status",
            "started_at": "2026-04-22T10:07:00Z",
            "completed_at": "2026-04-22T10:08:00Z"
        }))
        .expect("sample workspace run should deserialize")
    }

    fn sample_workspace_artifact() -> contracts::WorkspaceArtifactSummary {
        serde_json::from_value(json!({
            "artifact_id": "00000000-0000-0000-0000-000000000031",
            "artifact_kind": "note",
            "title": "Operator note",
            "latest_version": 2,
            "updated_at": "2026-04-22T10:09:00Z"
        }))
        .expect("sample workspace artifact should deserialize")
    }

    fn sample_health_summary() -> OperationalHealthSummary {
        serde_json::from_value(json!({
            "evaluated_at": "2026-04-23T10:00:00Z",
            "overall_status": "degraded",
            "pending_work": {
                "pending_foreground_conversation_count": 1,
                "pending_background_job_count": 2,
                "due_background_job_count": 1,
                "pending_wake_signal_count": 0,
                "pending_approval_request_count": 1,
                "awaiting_approval_governed_action_count": 1,
                "blocked_governed_action_count": 0
            },
            "recovery": {
                "open_checkpoint_count": 1,
                "open_foreground_checkpoint_count": 1,
                "open_background_checkpoint_count": 0,
                "open_governed_action_checkpoint_count": 0,
                "recent_resolved_checkpoint_count": 3,
                "recent_abandoned_checkpoint_count": 1,
                "active_worker_lease_count": 2,
                "overdue_active_worker_lease_count": 0,
                "at_risk_active_worker_lease_count": 1
            },
            "diagnostics": {
                "recent_window_minutes": 60,
                "observed_count": 4,
                "info_count": 1,
                "warn_count": 2,
                "error_count": 1,
                "critical_count": 0,
                "top_reason_codes": [{
                    "reason_code": "worker_lease_soft_warning",
                    "count": 2,
                    "latest_at": "2026-04-23T09:58:00Z"
                }]
            },
            "anomalies": [{
                "anomaly_kind": "recovery_pressure",
                "severity": "warn",
                "reason_code": "operational_recovery_pressure_detected",
                "summary": "recovery pressure detected",
                "occurrence_count": 1,
                "latest_trace_id": "00000000-0000-0000-0000-000000000041",
                "latest_execution_id": "00000000-0000-0000-0000-000000000042",
                "first_seen_at": "2026-04-23T09:55:00Z",
                "last_seen_at": "2026-04-23T09:58:00Z"
            }]
        }))
        .expect("sample health summary should deserialize")
    }

    fn sample_recovery_supervision_report() -> RecoverySupervisionReport {
        serde_json::from_value(json!({
            "trace_id": "00000000-0000-0000-0000-000000000050",
            "supervised_at": "2026-04-23T10:05:00Z",
            "actor_ref": "cli:operator",
            "reason": "manual recovery verification",
            "soft_warning_count": 1,
            "recovered_expired_lease_count": 1,
            "soft_warning_diagnostics": [{
                "operational_diagnostic_id": "00000000-0000-0000-0000-000000000051",
                "trace_id": "00000000-0000-0000-0000-000000000052",
                "execution_id": null,
                "subsystem": "recovery",
                "severity": "warn",
                "reason_code": "worker_lease_soft_warning",
                "summary": "worker lease is nearing expiry",
                "created_at": "2026-04-23T10:05:00Z"
            }],
            "recovered_expired_leases": [{
                "worker_lease_id": "00000000-0000-0000-0000-000000000053",
                "worker_kind": "background",
                "checkpoint_id": "00000000-0000-0000-0000-000000000054",
                "checkpoint_status": "resolved",
                "recovery_decision": "retry",
                "diagnostic_reason_code": "worker_lease_expired",
                "diagnostic_severity": "warn",
                "trace_id": "00000000-0000-0000-0000-000000000055",
                "execution_id": "00000000-0000-0000-0000-000000000056"
            }]
        }))
        .expect("sample recovery supervision report should deserialize")
    }

    fn sample_worker_lease_inspection_summary() -> WorkerLeaseInspectionSummary {
        serde_json::from_value(json!({
            "worker_lease_id": "00000000-0000-0000-0000-000000000061",
            "trace_id": "00000000-0000-0000-0000-000000000062",
            "execution_id": "00000000-0000-0000-0000-000000000063",
            "background_job_id": "00000000-0000-0000-0000-000000000064",
            "background_job_run_id": "00000000-0000-0000-0000-000000000065",
            "governed_action_execution_id": null,
            "worker_kind": "background",
            "lease_status": "active",
            "supervision_status": "soft_warning",
            "lease_acquired_at": "2026-04-23T10:00:00Z",
            "lease_expires_at": "2026-04-23T10:10:00Z",
            "last_heartbeat_at": "2026-04-23T10:08:30Z",
            "released_at": null
        }))
        .expect("sample worker lease inspection summary should deserialize")
    }

    fn sample_schema_upgrade_assessment() -> SchemaUpgradeAssessmentReport {
        serde_json::from_value(json!({
            "compatibility": "pending_migrations",
            "current_version": 6,
            "expected_version": 7,
            "minimum_supported_version": 1,
            "discovered_versions": [1, 2, 3, 4, 5, 6, 7],
            "applied_versions": [1, 2, 3, 4, 5, 6],
            "pending_versions": [7],
            "history_valid": true,
            "details": "database schema version 6 is behind expected version 7"
        }))
        .expect("sample schema upgrade assessment should deserialize")
    }

    #[test]
    fn phase_five_admin_parser_accepts_approval_resolution_command() {
        let command = AdminCommand::try_parse_from([
            "runtime",
            "approvals",
            "resolve",
            "--approval-request-id",
            "00000000-0000-0000-0000-000000000001",
            "--decision",
            "approve",
            "--actor-ref",
            "cli:primary-user",
            "--reason",
            "manual verification",
        ])
        .expect("approval resolution command should parse");

        match command.command {
            AdminSubcommand::Approvals(ApprovalsCommand {
                command: ApprovalsSubcommand::Resolve(command),
            }) => {
                assert_eq!(
                    command.approval_request_id,
                    "00000000-0000-0000-0000-000000000001"
                );
                assert!(matches!(command.decision, ApprovalDecisionArg::Approve));
                assert_eq!(command.actor_ref.as_deref(), Some("cli:primary-user"));
            }
            other => panic!("expected approval resolution command, got {other:?}"),
        }
    }

    #[test]
    fn phase_five_admin_parser_accepts_workspace_run_filters() {
        let command = AdminCommand::try_parse_from([
            "runtime",
            "workspace",
            "runs",
            "list",
            "--script-id",
            "00000000-0000-0000-0000-000000000022",
            "--limit",
            "5",
        ])
        .expect("workspace runs command should parse");

        match command.command {
            AdminSubcommand::Workspace(WorkspaceCommand {
                command:
                    WorkspaceSubcommand::Runs(WorkspaceRunsCommand {
                        command: WorkspaceRunsSubcommand::List(command),
                    }),
            }) => {
                assert_eq!(
                    command.script_id.as_deref(),
                    Some("00000000-0000-0000-0000-000000000022")
                );
                assert_eq!(command.limit, 5);
            }
            other => panic!("expected workspace runs command, got {other:?}"),
        }
    }

    #[test]
    fn phase_five_admin_parser_accepts_action_status_filters() {
        let command = AdminCommand::try_parse_from([
            "runtime", "actions", "list", "--status", "blocked", "--limit", "3",
        ])
        .expect("actions list command should parse");

        match command.command {
            AdminSubcommand::Actions(ActionsCommand {
                command: ActionsSubcommand::List(command),
            }) => {
                assert!(matches!(
                    command.status,
                    Some(GovernedActionStatusArg::Blocked)
                ));
                assert_eq!(command.limit, 3);
            }
            other => panic!("expected actions list command, got {other:?}"),
        }
    }

    #[test]
    fn phase_six_admin_parser_accepts_recovery_checkpoint_filters() {
        let command = AdminCommand::try_parse_from([
            "runtime",
            "recovery",
            "checkpoints",
            "list",
            "--open-only",
            "--limit",
            "7",
        ])
        .expect("recovery checkpoints command should parse");

        match command.command {
            AdminSubcommand::Recovery(RecoveryCommand {
                command:
                    RecoverySubcommand::Checkpoints(RecoveryCheckpointsCommand {
                        command: RecoveryCheckpointsSubcommand::List(command),
                    }),
            }) => {
                assert!(command.open_only);
                assert_eq!(command.limit, 7);
            }
            other => panic!("expected recovery checkpoints command, got {other:?}"),
        }
    }

    #[test]
    fn phase_six_admin_parser_accepts_recovery_lease_list_command() {
        let command = AdminCommand::try_parse_from([
            "runtime",
            "recovery",
            "leases",
            "list",
            "--limit",
            "5",
            "--soft-warning-threshold-percent",
            "90",
            "--json",
        ])
        .expect("recovery leases list command should parse");

        match command.command {
            AdminSubcommand::Recovery(RecoveryCommand {
                command:
                    RecoverySubcommand::Leases(RecoveryLeasesCommand {
                        command: RecoveryLeasesSubcommand::List(command),
                    }),
            }) => {
                assert_eq!(command.limit, 5);
                assert_eq!(command.soft_warning_threshold_percent, 90);
                assert!(command.json);
            }
            other => panic!("expected recovery leases list command, got {other:?}"),
        }
    }

    #[test]
    fn phase_six_admin_parser_accepts_recovery_supervision_threshold() {
        let command = AdminCommand::try_parse_from([
            "runtime",
            "recovery",
            "supervise",
            "--soft-warning-threshold-percent",
            "90",
            "--actor-ref",
            "cli:primary-user",
            "--reason",
            "manual recovery verification",
        ])
        .expect("recovery supervise command should parse");

        match command.command {
            AdminSubcommand::Recovery(RecoveryCommand {
                command: RecoverySubcommand::Supervise(command),
            }) => {
                assert_eq!(command.soft_warning_threshold_percent, 90);
                assert_eq!(command.actor_ref.as_deref(), Some("cli:primary-user"));
                assert_eq!(
                    command.reason.as_deref(),
                    Some("manual recovery verification")
                );
            }
            other => panic!("expected recovery supervise command, got {other:?}"),
        }
    }

    #[test]
    fn phase_six_admin_parser_accepts_schema_upgrade_path_command() {
        let command = AdminCommand::try_parse_from(["runtime", "schema", "upgrade-path", "--json"])
            .expect("schema upgrade-path command should parse");

        match command.command {
            AdminSubcommand::Schema(SchemaCommand {
                command: SchemaSubcommand::UpgradePath(command),
            }) => {
                assert!(command.json);
            }
            other => panic!("expected schema upgrade-path command, got {other:?}"),
        }
    }

    #[test]
    fn phase_six_admin_parser_rejects_invalid_recovery_thresholds() {
        let lease_error = AdminCommand::try_parse_from([
            "runtime",
            "recovery",
            "leases",
            "list",
            "--soft-warning-threshold-percent",
            "0",
        ])
        .expect_err("zero recovery lease threshold should be rejected");
        assert!(lease_error.to_string().contains("1..=100"));

        let supervise_error = AdminCommand::try_parse_from([
            "runtime",
            "recovery",
            "supervise",
            "--soft-warning-threshold-percent",
            "101",
        ])
        .expect_err("out-of-range supervision threshold should be rejected");
        assert!(supervise_error.to_string().contains("1..=100"));
    }

    #[test]
    fn render_approval_requests_text_includes_resolution_metadata() {
        let rendered = render_approval_requests_text(&[sample_approval_request_summary()]);
        assert!(rendered.contains("status=approved"));
        assert!(rendered.contains("resolved=approved by=cli:primary-user"));
        assert!(rendered.contains("reason=manual verification"));
    }

    #[test]
    fn render_governed_actions_text_includes_blocked_reason_and_output_ref() {
        let rendered = render_governed_actions_text(&[sample_governed_action_summary()]);
        assert!(rendered.contains("status=blocked"));
        assert!(rendered.contains("blocked_reason: scope invalid"));
        assert!(rendered.contains("output_ref: execution_record:"));
    }

    #[test]
    fn render_workspace_artifacts_and_runs_text_include_phase_five_details() {
        let artifact_output = render_workspace_artifacts_text(&[sample_workspace_artifact()]);
        assert!(artifact_output.contains("kind=Note"));
        assert!(artifact_output.contains("latest_version=2"));

        let run_output = render_workspace_runs_text(&[sample_workspace_run_summary()]);
        assert!(run_output.contains("status=failed"));
        assert!(run_output.contains("failure_summary: script returned non-zero exit status"));
    }

    #[test]
    fn render_approval_resolution_text_includes_governed_action_summary() {
        let rendered = render_approval_resolution_text(&ApprovalResolutionSummary {
            approval_request: sample_approval_request_summary(),
            governed_action: Some(sample_governed_action_summary()),
        });
        assert!(
            rendered.contains("Approval 00000000-0000-0000-0000-000000000001 resolved as approved")
        );
        assert!(
            rendered
                .contains("governed action: 00000000-0000-0000-0000-000000000011 status=blocked")
        );
    }

    #[test]
    fn phase_six_render_health_summary_text_includes_recovery_and_anomalies() {
        let rendered = render_health_summary_text(&sample_health_summary());
        assert!(rendered.contains("overall_status=degraded"));
        assert!(rendered.contains("open_checkpoints=1"));
        assert!(rendered.contains("top_reason_code: worker_lease_soft_warning"));
        assert!(rendered.contains("anomaly: kind=recovery_pressure"));
    }

    #[test]
    fn phase_six_render_recovery_supervision_text_includes_counts() {
        let rendered = render_recovery_supervision_text(&sample_recovery_supervision_report());
        assert!(rendered.contains("soft_warnings=1"));
        assert!(rendered.contains("recovered_expired_leases=1"));
        assert!(rendered.contains("actor_ref=cli:operator"));
        assert!(rendered.contains("reason=manual recovery verification"));
        assert!(rendered.contains("diagnostic_reason=worker_lease_expired"));
    }

    #[test]
    fn phase_six_render_recovery_leases_text_includes_supervision_status() {
        let rendered = render_recovery_leases_text(&[sample_worker_lease_inspection_summary()]);
        assert!(rendered.contains("lease_status=active"));
        assert!(rendered.contains("supervision_status=soft_warning"));
        assert!(rendered.contains("background_job_run_id=00000000-0000-0000-0000-000000000065"));
    }

    #[test]
    fn phase_six_render_schema_upgrade_assessment_text_includes_versions() {
        let rendered = render_schema_upgrade_assessment_text(&sample_schema_upgrade_assessment());
        assert!(rendered.contains("compatibility=pending_migrations"));
        assert!(rendered.contains("pending_versions=7"));
        assert!(rendered.contains("history_valid=yes"));
    }

    #[test]
    fn phase_six_render_recovery_leases_text_reports_empty_state() {
        assert_eq!(
            render_recovery_leases_text(&[]),
            "No active worker leases.".to_string()
        );
    }
}
