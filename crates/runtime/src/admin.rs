use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use harness::{
    config::RuntimeConfig,
    management::{
        self, ApprovalRequestSummary, ApprovalResolutionSummary, BackgroundEnqueueOutcome,
        BackgroundJobSummary, BackgroundRunNextOutcome, EnqueueBackgroundJobRequest,
        GovernedActionSummary, PendingForegroundConversationSummary, ResolveApprovalRequest,
        RuntimeStatusReport, WakeSignalSummary, WorkspaceScriptRunSummary,
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

    if summaries.is_empty() {
        println!("No approval requests.");
        return Ok(());
    }

    for summary in summaries {
        println!(
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
            println!(
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

    Ok(())
}

fn print_approval_resolution(summary: ApprovalResolutionSummary, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    println!(
        "Approval {} resolved as {}",
        summary.approval_request.approval_request_id, summary.approval_request.status
    );
    println!("  title: {}", summary.approval_request.title);
    println!(
        "  resolved_by: {}",
        summary
            .approval_request
            .resolved_by
            .as_deref()
            .unwrap_or("unknown")
    );
    println!(
        "  reason: {}",
        summary
            .approval_request
            .resolution_reason
            .as_deref()
            .unwrap_or("none")
    );
    if let Some(action) = summary.governed_action {
        println!(
            "  governed action: {} status={} output_ref={}",
            action.governed_action_execution_id,
            action.status,
            action.output_ref.as_deref().unwrap_or("none")
        );
    }

    Ok(())
}

fn print_governed_actions(actions: Vec<GovernedActionSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&actions)?);
        return Ok(());
    }

    if actions.is_empty() {
        println!("No governed actions.");
        return Ok(());
    }

    for action in actions {
        println!(
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
            println!("  blocked_reason: {blocked_reason}");
        }
        if let Some(output_ref) = action.output_ref.as_deref() {
            println!("  output_ref: {output_ref}");
        }
    }

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

    if artifacts.is_empty() {
        println!("No workspace artifacts.");
        return Ok(());
    }

    for artifact in artifacts {
        println!(
            "{} | kind={:?} | latest_version={} | updated_at={} | title={}",
            artifact.artifact_id,
            artifact.artifact_kind,
            artifact.latest_version,
            artifact.updated_at,
            artifact.title
        );
    }

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

    if scripts.is_empty() {
        println!("No workspace scripts.");
        return Ok(());
    }

    for script in scripts {
        println!(
            "{} | artifact_id={} | language={} | latest_version={} | updated_at={}",
            script.script_id,
            script.workspace_artifact_id,
            script.language,
            script.latest_version,
            script.updated_at
        );
    }

    Ok(())
}

fn print_workspace_runs(runs: Vec<WorkspaceScriptRunSummary>, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(());
    }

    if runs.is_empty() {
        println!("No workspace script runs.");
        return Ok(());
    }

    for run in runs {
        println!(
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
            println!("  output_ref: {output_ref}");
        }
        if let Some(failure_summary) = run.failure_summary.as_deref() {
            println!("  failure_summary: {failure_summary}");
        }
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
