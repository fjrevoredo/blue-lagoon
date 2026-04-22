use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use contracts::{
    ApprovalRequest, ApprovalRequestStatus, ApprovalResolutionDecision, ApprovalResolutionEvent,
    CapabilityScope, GovernedActionFingerprint, GovernedActionKind, GovernedActionRiskTier,
};
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    audit::{self, NewAuditEvent},
    config::RuntimeConfig,
};

const APPROVAL_EXPIRY_ACTOR: &str = "system:approval-expiry";
const APPROVAL_INVALIDATION_ACTOR: &str = "system:approval-invalidation";

#[derive(Debug, Clone)]
pub struct NewApprovalRequestRecord {
    pub approval_request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: GovernedActionFingerprint,
    pub action_kind: GovernedActionKind,
    pub risk_tier: GovernedActionRiskTier,
    pub title: String,
    pub consequence_summary: String,
    pub capability_scope: CapabilityScope,
    pub requested_by: String,
    pub token: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApprovalRequestRecord {
    pub approval_request_id: Uuid,
    pub trace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub action_proposal_id: Uuid,
    pub action_fingerprint: GovernedActionFingerprint,
    pub action_kind: GovernedActionKind,
    pub risk_tier: GovernedActionRiskTier,
    pub title: String,
    pub consequence_summary: String,
    pub capability_scope: CapabilityScope,
    pub status: ApprovalRequestStatus,
    pub requested_by: String,
    pub token: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_kind: Option<ApprovalResolutionDecision>,
    pub resolved_by: Option<String>,
    pub resolution_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ApprovalRequestRecord {
    pub fn to_contract(&self) -> ApprovalRequest {
        ApprovalRequest {
            approval_request_id: self.approval_request_id,
            action_proposal_id: self.action_proposal_id,
            action_fingerprint: self.action_fingerprint.clone(),
            status: self.status,
            risk_tier: self.risk_tier,
            title: self.title.clone(),
            consequence_summary: self.consequence_summary.clone(),
            capability_scope: self.capability_scope.clone(),
            requested_at: self.requested_at,
            expires_at: self.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalResolutionAttempt {
    pub token: String,
    pub actor_ref: String,
    pub expected_action_fingerprint: GovernedActionFingerprint,
    pub decision: ApprovalResolutionDecision,
    pub reason: Option<String>,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApprovalResolutionResult {
    pub request: ApprovalRequestRecord,
    pub event: ApprovalResolutionEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApprovalActorRef<'a> {
    surface: &'a str,
    principal: &'a str,
}

pub async fn create_approval_request(
    config: &RuntimeConfig,
    pool: &PgPool,
    request: &NewApprovalRequestRecord,
) -> Result<ApprovalRequestRecord> {
    validate_new_approval_request(request)?;
    enforce_pending_request_limit(config, pool).await?;

    let capability_scope_json = serde_json::to_value(&request.capability_scope)
        .context("failed to encode approval capability scope")?;

    let mut tx = pool
        .begin()
        .await
        .context("failed to start approval request transaction")?;

    sqlx::query(
        r#"
        INSERT INTO approval_requests (
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        ) VALUES (
            $1,
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            $8,
            $9,
            $10,
            'pending',
            $11,
            $12,
            $13,
            $14,
            NULL,
            NULL,
            NULL,
            NULL,
            NOW(),
            NOW()
        )
        "#,
    )
    .bind(request.approval_request_id)
    .bind(request.trace_id)
    .bind(request.execution_id)
    .bind(request.action_proposal_id)
    .bind(&request.action_fingerprint.value)
    .bind(governed_action_kind_as_str(request.action_kind))
    .bind(governed_action_risk_tier_as_str(request.risk_tier))
    .bind(&request.title)
    .bind(&request.consequence_summary)
    .bind(capability_scope_json)
    .bind(&request.requested_by)
    .bind(&request.token)
    .bind(request.requested_at)
    .bind(request.expires_at)
    .execute(&mut *tx)
    .await
    .context("failed to insert approval request")?;

    write_approval_audit_event(
        &mut tx,
        request.trace_id,
        request.execution_id,
        "approval_request_created",
        "info",
        json!({
            "approval_request_id": request.approval_request_id,
            "action_proposal_id": request.action_proposal_id,
            "action_fingerprint": request.action_fingerprint.value,
            "action_kind": governed_action_kind_as_str(request.action_kind),
            "risk_tier": governed_action_risk_tier_as_str(request.risk_tier),
            "requested_by": request.requested_by,
            "expires_at": request.expires_at,
        }),
    )
    .await?;

    tx.commit()
        .await
        .context("failed to commit approval request transaction")?;

    get_approval_request(pool, request.approval_request_id).await
}

pub async fn get_approval_request(
    pool: &PgPool,
    approval_request_id: Uuid,
) -> Result<ApprovalRequestRecord> {
    let row = sqlx::query(
        r#"
        SELECT
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        FROM approval_requests
        WHERE approval_request_id = $1
        "#,
    )
    .bind(approval_request_id)
    .fetch_one(pool)
    .await
    .context("failed to fetch approval request")?;

    decode_approval_request_row(row)
}

pub async fn get_approval_request_by_token(
    pool: &PgPool,
    token: &str,
) -> Result<Option<ApprovalRequestRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        FROM approval_requests
        WHERE token = $1
        "#,
    )
    .bind(token)
    .fetch_optional(pool)
    .await
    .context("failed to fetch approval request by token")?;

    row.map(decode_approval_request_row).transpose()
}

pub async fn get_pending_approval_request_by_fingerprint(
    pool: &PgPool,
    action_fingerprint: &GovernedActionFingerprint,
) -> Result<Option<ApprovalRequestRecord>> {
    let row = sqlx::query(
        r#"
        SELECT
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        FROM approval_requests
        WHERE action_fingerprint = $1
          AND status = 'pending'
        "#,
    )
    .bind(&action_fingerprint.value)
    .fetch_optional(pool)
    .await
    .context("failed to fetch pending approval request by fingerprint")?;

    row.map(decode_approval_request_row).transpose()
}

pub async fn list_approval_requests(
    pool: &PgPool,
    status: Option<ApprovalRequestStatus>,
    limit: i64,
) -> Result<Vec<ApprovalRequestRecord>> {
    let rows = if let Some(status) = status {
        sqlx::query(
            r#"
            SELECT
                approval_request_id,
                trace_id,
                execution_id,
                action_proposal_id,
                action_fingerprint,
                action_kind,
                risk_tier,
                title,
                consequence_summary,
                capability_scope_json,
                status,
                requested_by,
                token,
                requested_at,
                expires_at,
                resolved_at,
                resolution_kind,
                resolved_by,
                resolution_reason,
                created_at,
                updated_at
            FROM approval_requests
            WHERE status = $1
            ORDER BY requested_at DESC, approval_request_id DESC
            LIMIT $2
            "#,
        )
        .bind(approval_request_status_as_str(status))
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("failed to list approval requests by status")?
    } else {
        sqlx::query(
            r#"
            SELECT
                approval_request_id,
                trace_id,
                execution_id,
                action_proposal_id,
                action_fingerprint,
                action_kind,
                risk_tier,
                title,
                consequence_summary,
                capability_scope_json,
                status,
                requested_by,
                token,
                requested_at,
                expires_at,
                resolved_at,
                resolution_kind,
                resolved_by,
                resolution_reason,
                created_at,
                updated_at
            FROM approval_requests
            ORDER BY requested_at DESC, approval_request_id DESC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(pool)
        .await
        .context("failed to list approval requests")?
    };

    rows.into_iter().map(decode_approval_request_row).collect()
}

pub async fn resolve_approval_request(
    pool: &PgPool,
    attempt: &ApprovalResolutionAttempt,
) -> Result<ApprovalResolutionResult> {
    validate_resolution_attempt(attempt)?;

    let mut tx = pool
        .begin()
        .await
        .context("failed to start approval resolution transaction")?;

    let row = sqlx::query(
        r#"
        SELECT
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        FROM approval_requests
        WHERE token = $1
        FOR UPDATE
        "#,
    )
    .bind(&attempt.token)
    .fetch_one(&mut *tx)
    .await
    .context("failed to lock approval request for resolution")?;

    let record = decode_approval_request_row(row)?;
    if record.status != ApprovalRequestStatus::Pending {
        bail!(
            "approval request '{}' is no longer pending",
            record.approval_request_id
        );
    }

    let transition = if attempt.resolved_at >= record.expires_at {
        ResolutionTransition {
            decision: ApprovalResolutionDecision::Expired,
            actor_ref: APPROVAL_EXPIRY_ACTOR.to_string(),
            reason: Some("approval request expired before resolution".to_string()),
            event_kind: "approval_request_expired",
            severity: "warn",
        }
    } else if record.action_fingerprint != attempt.expected_action_fingerprint {
        ResolutionTransition {
            decision: ApprovalResolutionDecision::Invalidated,
            actor_ref: APPROVAL_INVALIDATION_ACTOR.to_string(),
            reason: Some(
                "approval request invalidated because the action fingerprint changed".to_string(),
            ),
            event_kind: "approval_request_invalidated",
            severity: "warn",
        }
    } else {
        validate_resolution_actor(&record, attempt)?;
        ResolutionTransition {
            decision: attempt.decision,
            actor_ref: attempt.actor_ref.clone(),
            reason: attempt.reason.clone(),
            event_kind: approval_event_kind(attempt.decision),
            severity: approval_event_severity(attempt.decision),
        }
    };

    let Some(event) =
        apply_resolution_transition(&mut tx, &record, transition, attempt.resolved_at).await?
    else {
        bail!(
            "approval request '{}' is no longer pending",
            record.approval_request_id
        );
    };

    tx.commit()
        .await
        .context("failed to commit approval resolution transaction")?;

    Ok(ApprovalResolutionResult {
        request: get_approval_request(pool, record.approval_request_id).await?,
        event,
    })
}

pub async fn expire_due_approval_requests(
    pool: &PgPool,
    now: DateTime<Utc>,
) -> Result<Vec<ApprovalResolutionResult>> {
    let pending_rows = sqlx::query(
        r#"
        SELECT
            approval_request_id,
            trace_id,
            execution_id,
            action_proposal_id,
            action_fingerprint,
            action_kind,
            risk_tier,
            title,
            consequence_summary,
            capability_scope_json,
            status,
            requested_by,
            token,
            requested_at,
            expires_at,
            resolved_at,
            resolution_kind,
            resolved_by,
            resolution_reason,
            created_at,
            updated_at
        FROM approval_requests
        WHERE status = 'pending'
          AND expires_at <= $1
        ORDER BY expires_at ASC, approval_request_id ASC
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await
    .context("failed to fetch due approval requests for expiry")?;

    let mut results = Vec::with_capacity(pending_rows.len());
    for row in pending_rows {
        let record = decode_approval_request_row(row)?;
        let mut tx = pool
            .begin()
            .await
            .context("failed to start approval expiry transaction")?;

        let maybe_event = apply_resolution_transition(
            &mut tx,
            &record,
            ResolutionTransition {
                decision: ApprovalResolutionDecision::Expired,
                actor_ref: APPROVAL_EXPIRY_ACTOR.to_string(),
                reason: Some("approval request expired before resolution".to_string()),
                event_kind: "approval_request_expired",
                severity: "warn",
            },
            now,
        )
        .await?;
        let Some(event) = maybe_event else {
            tx.rollback()
                .await
                .context("failed to rollback skipped approval expiry transaction")?;
            continue;
        };

        tx.commit()
            .await
            .context("failed to commit approval expiry transaction")?;

        results.push(ApprovalResolutionResult {
            request: get_approval_request(pool, record.approval_request_id).await?,
            event,
        });
    }

    Ok(results)
}

async fn apply_resolution_transition(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    record: &ApprovalRequestRecord,
    transition: ResolutionTransition,
    resolved_at: DateTime<Utc>,
) -> Result<Option<ApprovalResolutionEvent>> {
    let status = approval_resolution_to_status(transition.decision);
    let update_result = sqlx::query(
        r#"
        UPDATE approval_requests
        SET
            status = $2,
            resolved_at = $3,
            resolution_kind = $4,
            resolved_by = $5,
            resolution_reason = $6,
            updated_at = NOW()
        WHERE approval_request_id = $1
          AND status = 'pending'
        "#,
    )
    .bind(record.approval_request_id)
    .bind(approval_request_status_as_str(status))
    .bind(resolved_at)
    .bind(approval_resolution_decision_as_str(transition.decision))
    .bind(&transition.actor_ref)
    .bind(&transition.reason)
    .execute(&mut **tx)
    .await
    .context("failed to update approval request resolution")?;
    if update_result.rows_affected() == 0 {
        return Ok(None);
    }

    let event = ApprovalResolutionEvent {
        resolution_id: Uuid::now_v7(),
        approval_request_id: record.approval_request_id,
        decision: transition.decision,
        resolved_by: transition.actor_ref.clone(),
        resolved_at,
        reason: transition.reason.clone(),
    };

    write_approval_audit_event(
        tx,
        record.trace_id,
        record.execution_id,
        transition.event_kind,
        transition.severity,
        json!({
            "approval_request_id": record.approval_request_id,
            "action_proposal_id": record.action_proposal_id,
            "action_fingerprint": record.action_fingerprint.value,
            "decision": approval_resolution_decision_as_str(transition.decision),
            "resolved_by": event.resolved_by,
            "resolved_at": event.resolved_at,
            "reason": event.reason,
        }),
    )
    .await?;

    Ok(Some(event))
}

async fn enforce_pending_request_limit(config: &RuntimeConfig, pool: &PgPool) -> Result<()> {
    let pending_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM approval_requests
        WHERE status = 'pending'
        "#,
    )
    .fetch_one(pool)
    .await
    .context("failed to count pending approval requests")?;

    if pending_count >= i64::from(config.approvals.max_pending_requests) {
        bail!(
            "pending approval request count is at or above the configured limit ({})",
            config.approvals.max_pending_requests
        );
    }
    Ok(())
}

async fn write_approval_audit_event(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    trace_id: Uuid,
    execution_id: Option<Uuid>,
    event_kind: &str,
    severity: &str,
    payload: serde_json::Value,
) -> Result<()> {
    audit::insert(
        &mut **executor,
        &NewAuditEvent {
            loop_kind: "conscious".to_string(),
            subsystem: "approval".to_string(),
            event_kind: event_kind.to_string(),
            severity: severity.to_string(),
            trace_id,
            execution_id,
            worker_pid: None,
            payload,
        },
    )
    .await?;
    Ok(())
}

fn decode_approval_request_row(row: sqlx::postgres::PgRow) -> Result<ApprovalRequestRecord> {
    Ok(ApprovalRequestRecord {
        approval_request_id: row.get("approval_request_id"),
        trace_id: row.get("trace_id"),
        execution_id: row.get("execution_id"),
        action_proposal_id: row.get("action_proposal_id"),
        action_fingerprint: GovernedActionFingerprint {
            value: row.get("action_fingerprint"),
        },
        action_kind: parse_governed_action_kind(row.get("action_kind"))?,
        risk_tier: parse_governed_action_risk_tier(row.get("risk_tier"))?,
        title: row.get("title"),
        consequence_summary: row.get("consequence_summary"),
        capability_scope: serde_json::from_value(row.get("capability_scope_json"))
            .context("failed to decode approval capability scope")?,
        status: parse_approval_request_status(row.get("status"))?,
        requested_by: row.get("requested_by"),
        token: row.get("token"),
        requested_at: row.get("requested_at"),
        expires_at: row.get("expires_at"),
        resolved_at: row.get("resolved_at"),
        resolution_kind: row
            .get::<Option<String>, _>("resolution_kind")
            .map(|value| parse_approval_resolution_decision(&value))
            .transpose()?,
        resolved_by: row.get("resolved_by"),
        resolution_reason: row.get("resolution_reason"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn validate_new_approval_request(request: &NewApprovalRequestRecord) -> Result<()> {
    if request.requested_by.trim().is_empty() {
        bail!("approval request requested_by must not be empty");
    }
    if request.title.trim().is_empty() {
        bail!("approval request title must not be empty");
    }
    if request.consequence_summary.trim().is_empty() {
        bail!("approval request consequence summary must not be empty");
    }
    if request.token.trim().is_empty() {
        bail!("approval request token must not be empty");
    }
    if request.expires_at <= request.requested_at {
        bail!("approval request expires_at must be greater than requested_at");
    }
    Ok(())
}

fn validate_resolution_attempt(attempt: &ApprovalResolutionAttempt) -> Result<()> {
    if attempt.token.trim().is_empty() {
        bail!("approval resolution token must not be empty");
    }
    if attempt.actor_ref.trim().is_empty() {
        bail!("approval resolution actor_ref must not be empty");
    }
    parse_actor_ref(&attempt.actor_ref)?;
    match attempt.decision {
        ApprovalResolutionDecision::Approved | ApprovalResolutionDecision::Rejected => {}
        ApprovalResolutionDecision::Expired | ApprovalResolutionDecision::Invalidated => {
            bail!("approval resolution decision must come from an external approve/reject action");
        }
    }
    Ok(())
}

fn validate_resolution_actor(
    record: &ApprovalRequestRecord,
    attempt: &ApprovalResolutionAttempt,
) -> Result<()> {
    let requested_by = parse_actor_ref(&record.requested_by)?;
    let actor = parse_actor_ref(&attempt.actor_ref)?;

    if actor.principal != requested_by.principal {
        bail!(
            "approval resolution actor '{}' does not match the requested principal '{}'",
            attempt.actor_ref,
            record.requested_by
        );
    }

    Ok(())
}

fn parse_actor_ref(actor_ref: &str) -> Result<ApprovalActorRef<'_>> {
    let trimmed = actor_ref.trim();
    let Some((surface, principal)) = trimmed.split_once(':') else {
        bail!("approval resolution actor_ref must use '<surface>:<principal>' format");
    };
    if surface.is_empty() || principal.is_empty() {
        bail!("approval resolution actor_ref must use '<surface>:<principal>' format");
    }
    if surface == "system" {
        bail!("approval resolution actor_ref must not use the reserved system surface");
    }

    Ok(ApprovalActorRef { surface, principal })
}

fn approval_event_kind(decision: ApprovalResolutionDecision) -> &'static str {
    match decision {
        ApprovalResolutionDecision::Approved => "approval_request_approved",
        ApprovalResolutionDecision::Rejected => "approval_request_rejected",
        ApprovalResolutionDecision::Expired => "approval_request_expired",
        ApprovalResolutionDecision::Invalidated => "approval_request_invalidated",
    }
}

fn approval_event_severity(decision: ApprovalResolutionDecision) -> &'static str {
    match decision {
        ApprovalResolutionDecision::Approved => "info",
        ApprovalResolutionDecision::Rejected => "info",
        ApprovalResolutionDecision::Expired => "warn",
        ApprovalResolutionDecision::Invalidated => "warn",
    }
}

fn approval_request_status_as_str(status: ApprovalRequestStatus) -> &'static str {
    match status {
        ApprovalRequestStatus::Pending => "pending",
        ApprovalRequestStatus::Approved => "approved",
        ApprovalRequestStatus::Rejected => "rejected",
        ApprovalRequestStatus::Expired => "expired",
        ApprovalRequestStatus::Invalidated => "invalidated",
    }
}

fn parse_approval_request_status(value: &str) -> Result<ApprovalRequestStatus> {
    match value {
        "pending" => Ok(ApprovalRequestStatus::Pending),
        "approved" => Ok(ApprovalRequestStatus::Approved),
        "rejected" => Ok(ApprovalRequestStatus::Rejected),
        "expired" => Ok(ApprovalRequestStatus::Expired),
        "invalidated" => Ok(ApprovalRequestStatus::Invalidated),
        other => bail!("unrecognized approval request status '{other}'"),
    }
}

fn approval_resolution_decision_as_str(decision: ApprovalResolutionDecision) -> &'static str {
    match decision {
        ApprovalResolutionDecision::Approved => "approved",
        ApprovalResolutionDecision::Rejected => "rejected",
        ApprovalResolutionDecision::Expired => "expired",
        ApprovalResolutionDecision::Invalidated => "invalidated",
    }
}

fn parse_approval_resolution_decision(value: &str) -> Result<ApprovalResolutionDecision> {
    match value {
        "approved" => Ok(ApprovalResolutionDecision::Approved),
        "rejected" => Ok(ApprovalResolutionDecision::Rejected),
        "expired" => Ok(ApprovalResolutionDecision::Expired),
        "invalidated" => Ok(ApprovalResolutionDecision::Invalidated),
        other => bail!("unrecognized approval resolution decision '{other}'"),
    }
}

fn approval_resolution_to_status(decision: ApprovalResolutionDecision) -> ApprovalRequestStatus {
    match decision {
        ApprovalResolutionDecision::Approved => ApprovalRequestStatus::Approved,
        ApprovalResolutionDecision::Rejected => ApprovalRequestStatus::Rejected,
        ApprovalResolutionDecision::Expired => ApprovalRequestStatus::Expired,
        ApprovalResolutionDecision::Invalidated => ApprovalRequestStatus::Invalidated,
    }
}

fn governed_action_kind_as_str(kind: GovernedActionKind) -> &'static str {
    match kind {
        GovernedActionKind::InspectWorkspaceArtifact => "inspect_workspace_artifact",
        GovernedActionKind::RunSubprocess => "run_subprocess",
        GovernedActionKind::RunWorkspaceScript => "run_workspace_script",
    }
}

fn parse_governed_action_kind(value: &str) -> Result<GovernedActionKind> {
    match value {
        "inspect_workspace_artifact" => Ok(GovernedActionKind::InspectWorkspaceArtifact),
        "run_subprocess" => Ok(GovernedActionKind::RunSubprocess),
        "run_workspace_script" => Ok(GovernedActionKind::RunWorkspaceScript),
        other => bail!("unrecognized governed action kind '{other}'"),
    }
}

fn governed_action_risk_tier_as_str(risk_tier: GovernedActionRiskTier) -> &'static str {
    match risk_tier {
        GovernedActionRiskTier::Tier0 => "tier_0",
        GovernedActionRiskTier::Tier1 => "tier_1",
        GovernedActionRiskTier::Tier2 => "tier_2",
        GovernedActionRiskTier::Tier3 => "tier_3",
    }
}

fn parse_governed_action_risk_tier(value: &str) -> Result<GovernedActionRiskTier> {
    match value {
        "tier_0" => Ok(GovernedActionRiskTier::Tier0),
        "tier_1" => Ok(GovernedActionRiskTier::Tier1),
        "tier_2" => Ok(GovernedActionRiskTier::Tier2),
        "tier_3" => Ok(GovernedActionRiskTier::Tier3),
        other => bail!("unrecognized governed action risk tier '{other}'"),
    }
}

#[derive(Debug, Clone)]
struct ResolutionTransition {
    decision: ApprovalResolutionDecision,
    actor_ref: String,
    reason: Option<String>,
    event_kind: &'static str,
    severity: &'static str,
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use contracts::{
        EnvironmentCapabilityScope, ExecutionCapabilityBudget, FilesystemCapabilityScope,
        NetworkAccessPosture,
    };

    use super::*;

    fn sample_scope() -> CapabilityScope {
        CapabilityScope {
            filesystem: FilesystemCapabilityScope {
                read_roots: vec!["D:/Repos/blue-lagoon".to_string()],
                write_roots: vec!["D:/Repos/blue-lagoon/docs".to_string()],
            },
            network: NetworkAccessPosture::Disabled,
            environment: EnvironmentCapabilityScope {
                allow_variables: Vec::new(),
            },
            execution: ExecutionCapabilityBudget {
                timeout_ms: 30_000,
                max_stdout_bytes: 65_536,
                max_stderr_bytes: 32_768,
            },
        }
    }

    fn sample_new_request() -> NewApprovalRequestRecord {
        let requested_at = Utc::now();
        NewApprovalRequestRecord {
            approval_request_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            action_proposal_id: Uuid::now_v7(),
            action_fingerprint: GovernedActionFingerprint {
                value: "sha256:test".to_string(),
            },
            action_kind: GovernedActionKind::RunSubprocess,
            risk_tier: GovernedActionRiskTier::Tier2,
            title: "Run bounded subprocess".to_string(),
            consequence_summary: "Executes a scoped subprocess.".to_string(),
            capability_scope: sample_scope(),
            requested_by: "telegram:primary-user".to_string(),
            token: "approval-token".to_string(),
            requested_at,
            expires_at: requested_at + Duration::minutes(15),
        }
    }

    fn sample_resolution_attempt() -> ApprovalResolutionAttempt {
        ApprovalResolutionAttempt {
            token: "approval-token".to_string(),
            actor_ref: "cli:primary-user".to_string(),
            expected_action_fingerprint: GovernedActionFingerprint {
                value: "sha256:test".to_string(),
            },
            decision: ApprovalResolutionDecision::Approved,
            reason: Some("manual verification".to_string()),
            resolved_at: Utc::now(),
        }
    }

    fn sample_pending_record() -> ApprovalRequestRecord {
        let requested_at = Utc::now();
        ApprovalRequestRecord {
            approval_request_id: Uuid::now_v7(),
            trace_id: Uuid::now_v7(),
            execution_id: None,
            action_proposal_id: Uuid::now_v7(),
            action_fingerprint: GovernedActionFingerprint {
                value: "sha256:test".to_string(),
            },
            action_kind: GovernedActionKind::RunSubprocess,
            risk_tier: GovernedActionRiskTier::Tier2,
            title: "Run bounded subprocess".to_string(),
            consequence_summary: "Executes a scoped subprocess.".to_string(),
            capability_scope: sample_scope(),
            status: ApprovalRequestStatus::Pending,
            requested_by: "telegram:primary-user".to_string(),
            token: "approval-token".to_string(),
            requested_at,
            expires_at: requested_at + Duration::minutes(15),
            resolved_at: None,
            resolution_kind: None,
            resolved_by: None,
            resolution_reason: None,
            created_at: requested_at,
            updated_at: requested_at,
        }
    }

    #[test]
    fn validate_new_approval_request_rejects_empty_requested_by() {
        let mut request = sample_new_request();
        request.requested_by = "   ".to_string();

        let error = validate_new_approval_request(&request)
            .expect_err("blank requested_by should be rejected");
        assert!(error.to_string().contains("requested_by must not be empty"));
    }

    #[test]
    fn validate_new_approval_request_rejects_non_increasing_expiry() {
        let mut request = sample_new_request();
        request.expires_at = request.requested_at;

        let error = validate_new_approval_request(&request)
            .expect_err("non-increasing expiry should be rejected");
        assert!(
            error
                .to_string()
                .contains("expires_at must be greater than requested_at")
        );
    }

    #[test]
    fn validate_resolution_attempt_rejects_reserved_system_surface() {
        let mut attempt = sample_resolution_attempt();
        attempt.actor_ref = "system:primary-user".to_string();

        let error = validate_resolution_attempt(&attempt)
            .expect_err("system actor surface should be rejected");
        assert!(error.to_string().contains("reserved system surface"));
    }

    #[test]
    fn validate_resolution_attempt_rejects_internal_transition_decisions() {
        let mut attempt = sample_resolution_attempt();
        attempt.decision = ApprovalResolutionDecision::Expired;

        let error = validate_resolution_attempt(&attempt)
            .expect_err("expired decision should be rejected for external resolution");
        assert!(
            error
                .to_string()
                .contains("must come from an external approve/reject action")
        );
    }

    #[test]
    fn validate_resolution_actor_accepts_matching_principal_across_surfaces() {
        let record = sample_pending_record();
        let attempt = sample_resolution_attempt();
        validate_resolution_actor(&record, &attempt)
            .expect("matching principal across surfaces should be accepted");
    }

    #[test]
    fn validate_resolution_actor_rejects_mismatched_principal() {
        let record = sample_pending_record();
        let mut attempt = sample_resolution_attempt();
        attempt.actor_ref = "cli:someone-else".to_string();

        let error = validate_resolution_actor(&record, &attempt)
            .expect_err("mismatched principal should be rejected");
        assert!(
            error
                .to_string()
                .contains("does not match the requested principal")
        );
    }
}
