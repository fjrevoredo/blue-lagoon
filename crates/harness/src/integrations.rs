use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Duration, Utc};
use thiserror::Error;

use crate::config::{
    ResolvedCalendarIntegrationConfig, ResolvedEmailIntegrationConfig,
    ResolvedTaskSyncIntegrationConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarListEventsRequest {
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub max_results: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarUpsertEventRequest {
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub location: Option<String>,
    pub details: Option<String>,
    pub external_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEventSummary {
    pub external_event_id: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailListMessagesRequest {
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub mailbox: Option<String>,
    pub query: Option<String>,
    pub max_results: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendEmailMessageRequest {
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub reply_to_external_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailMessageSummary {
    pub external_message_id: String,
    pub mailbox: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSyncRequest {
    pub internal_principal_ref: String,
    pub internal_conversation_ref: String,
    pub task_list_title: String,
    pub items: Vec<String>,
    pub external_list_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSyncResult {
    pub external_list_id: String,
    pub task_list_title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarIntegrationErrorKind {
    Misconfigured,
    TemporaryFailure,
    PermanentFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct CalendarIntegrationError {
    pub kind: CalendarIntegrationErrorKind,
    pub message: String,
}

impl CalendarIntegrationError {
    pub fn misconfigured(message: impl Into<String>) -> Self {
        Self {
            kind: CalendarIntegrationErrorKind::Misconfigured,
            message: message.into(),
        }
    }

    pub fn temporary_failure(message: impl Into<String>) -> Self {
        Self {
            kind: CalendarIntegrationErrorKind::TemporaryFailure,
            message: message.into(),
        }
    }

    pub fn permanent_failure(message: impl Into<String>) -> Self {
        Self {
            kind: CalendarIntegrationErrorKind::PermanentFailure,
            message: message.into(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait CalendarIntegrationAdapter {
    async fn list_events(
        &self,
        request: &CalendarListEventsRequest,
    ) -> std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError>;

    async fn upsert_event(
        &self,
        request: &CalendarUpsertEventRequest,
    ) -> std::result::Result<CalendarEventSummary, CalendarIntegrationError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailIntegrationErrorKind {
    Misconfigured,
    TemporaryFailure,
    PermanentFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct EmailIntegrationError {
    pub kind: EmailIntegrationErrorKind,
    pub message: String,
}

impl EmailIntegrationError {
    pub fn misconfigured(message: impl Into<String>) -> Self {
        Self {
            kind: EmailIntegrationErrorKind::Misconfigured,
            message: message.into(),
        }
    }

    pub fn temporary_failure(message: impl Into<String>) -> Self {
        Self {
            kind: EmailIntegrationErrorKind::TemporaryFailure,
            message: message.into(),
        }
    }

    pub fn permanent_failure(message: impl Into<String>) -> Self {
        Self {
            kind: EmailIntegrationErrorKind::PermanentFailure,
            message: message.into(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait EmailIntegrationAdapter {
    async fn list_messages(
        &self,
        request: &EmailListMessagesRequest,
    ) -> std::result::Result<Vec<EmailMessageSummary>, EmailIntegrationError>;

    async fn send_message(
        &self,
        request: &SendEmailMessageRequest,
    ) -> std::result::Result<EmailMessageSummary, EmailIntegrationError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskSyncIntegrationErrorKind {
    Misconfigured,
    TemporaryFailure,
    PermanentFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct TaskSyncIntegrationError {
    pub kind: TaskSyncIntegrationErrorKind,
    pub message: String,
}

impl TaskSyncIntegrationError {
    pub fn misconfigured(message: impl Into<String>) -> Self {
        Self {
            kind: TaskSyncIntegrationErrorKind::Misconfigured,
            message: message.into(),
        }
    }

    pub fn temporary_failure(message: impl Into<String>) -> Self {
        Self {
            kind: TaskSyncIntegrationErrorKind::TemporaryFailure,
            message: message.into(),
        }
    }

    pub fn permanent_failure(message: impl Into<String>) -> Self {
        Self {
            kind: TaskSyncIntegrationErrorKind::PermanentFailure,
            message: message.into(),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait TaskSyncIntegrationAdapter {
    async fn sync_task_list(
        &self,
        request: &TaskSyncRequest,
    ) -> std::result::Result<TaskSyncResult, TaskSyncIntegrationError>;
}

pub fn is_supported_calendar_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "deterministic_fake" | "fake"
    )
}

pub fn is_supported_email_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "deterministic_fake" | "fake"
    )
}

pub fn is_supported_task_sync_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "deterministic_fake" | "fake"
    )
}

#[derive(Debug, Clone)]
pub struct DeterministicCalendarIntegrationAdapter {
    _provider: String,
    _api_base_url: Option<String>,
    _credential: String,
}

impl DeterministicCalendarIntegrationAdapter {
    pub fn from_resolved_config(config: &ResolvedCalendarIntegrationConfig) -> Self {
        Self {
            _provider: config.provider.clone(),
            _api_base_url: config.api_base_url.clone(),
            _credential: config.credential.clone(),
        }
    }
}

impl CalendarIntegrationAdapter for DeterministicCalendarIntegrationAdapter {
    async fn list_events(
        &self,
        request: &CalendarListEventsRequest,
    ) -> std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError> {
        validate_calendar_list_events_request(request)?;
        let event = CalendarEventSummary {
            external_event_id: format!(
                "deterministic:{}:{}:{}",
                request.internal_principal_ref.trim(),
                request.internal_conversation_ref.trim(),
                request.start_at.timestamp()
            ),
            title: "Deterministic calendar event".to_string(),
            starts_at: request.start_at,
            ends_at: (request.start_at + Duration::minutes(30)).min(request.end_at),
            location: None,
        };
        Ok(if request.max_results > 0 {
            vec![event]
        } else {
            Vec::new()
        })
    }

    async fn upsert_event(
        &self,
        request: &CalendarUpsertEventRequest,
    ) -> std::result::Result<CalendarEventSummary, CalendarIntegrationError> {
        validate_calendar_upsert_event_request(request)?;
        Ok(CalendarEventSummary {
            external_event_id: request.external_event_id.clone().unwrap_or_else(|| {
                format!(
                    "deterministic:{}:{}:{}",
                    request.internal_principal_ref.trim(),
                    request.internal_conversation_ref.trim(),
                    request.starts_at.timestamp()
                )
            }),
            title: request.title.trim().to_string(),
            starts_at: request.starts_at,
            ends_at: request.ends_at,
            location: request
                .location
                .as_ref()
                .map(|value| value.trim().to_string()),
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredCalendarIntegrationAdapter;

impl CalendarIntegrationAdapter for UnconfiguredCalendarIntegrationAdapter {
    async fn list_events(
        &self,
        _request: &CalendarListEventsRequest,
    ) -> std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError> {
        Err(CalendarIntegrationError::misconfigured(
            "calendar integration adapter is not configured",
        ))
    }

    async fn upsert_event(
        &self,
        _request: &CalendarUpsertEventRequest,
    ) -> std::result::Result<CalendarEventSummary, CalendarIntegrationError> {
        Err(CalendarIntegrationError::misconfigured(
            "calendar integration adapter is not configured",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct FakeCalendarIntegrationAdapter {
    state: Arc<Mutex<FakeCalendarIntegrationState>>,
}

#[derive(Debug, Default)]
struct FakeCalendarIntegrationState {
    queued_list_responses:
        VecDeque<std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError>>,
    queued_upsert_responses:
        VecDeque<std::result::Result<CalendarEventSummary, CalendarIntegrationError>>,
    seen_list_requests: Vec<CalendarListEventsRequest>,
    seen_upsert_requests: Vec<CalendarUpsertEventRequest>,
}

impl Default for FakeCalendarIntegrationAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeCalendarIntegrationAdapter {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeCalendarIntegrationState::default())),
        }
    }

    pub fn push_list_response(
        &self,
        response: std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError>,
    ) {
        self.state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned")
            .queued_list_responses
            .push_back(response);
    }

    pub fn push_upsert_response(
        &self,
        response: std::result::Result<CalendarEventSummary, CalendarIntegrationError>,
    ) {
        self.state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned")
            .queued_upsert_responses
            .push_back(response);
    }

    pub fn seen_list_requests(&self) -> Vec<CalendarListEventsRequest> {
        self.state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned")
            .seen_list_requests
            .clone()
    }

    pub fn seen_upsert_requests(&self) -> Vec<CalendarUpsertEventRequest> {
        self.state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned")
            .seen_upsert_requests
            .clone()
    }
}

impl CalendarIntegrationAdapter for FakeCalendarIntegrationAdapter {
    async fn list_events(
        &self,
        request: &CalendarListEventsRequest,
    ) -> std::result::Result<Vec<CalendarEventSummary>, CalendarIntegrationError> {
        validate_calendar_list_events_request(request)?;
        let mut state = self
            .state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned");
        state.seen_list_requests.push(request.clone());
        state.queued_list_responses.pop_front().unwrap_or_else(|| {
            Err(CalendarIntegrationError::temporary_failure(
                "no fake calendar list response queued",
            ))
        })
    }

    async fn upsert_event(
        &self,
        request: &CalendarUpsertEventRequest,
    ) -> std::result::Result<CalendarEventSummary, CalendarIntegrationError> {
        validate_calendar_upsert_event_request(request)?;
        let mut state = self
            .state
            .lock()
            .expect("fake calendar integration state mutex should not be poisoned");
        state.seen_upsert_requests.push(request.clone());
        state
            .queued_upsert_responses
            .pop_front()
            .unwrap_or_else(|| {
                Err(CalendarIntegrationError::temporary_failure(
                    "no fake calendar upsert response queued",
                ))
            })
    }
}

fn validate_calendar_list_events_request(
    request: &CalendarListEventsRequest,
) -> std::result::Result<(), CalendarIntegrationError> {
    if request.internal_principal_ref.trim().is_empty() {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar list_events request requires internal_principal_ref",
        ));
    }
    if request.internal_conversation_ref.trim().is_empty() {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar list_events request requires internal_conversation_ref",
        ));
    }
    if request.max_results == 0 {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar list_events request requires max_results greater than zero",
        ));
    }
    if request.start_at >= request.end_at {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar list_events request requires start_at before end_at",
        ));
    }
    Ok(())
}

fn validate_calendar_upsert_event_request(
    request: &CalendarUpsertEventRequest,
) -> std::result::Result<(), CalendarIntegrationError> {
    if request.internal_principal_ref.trim().is_empty() {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar upsert_event request requires internal_principal_ref",
        ));
    }
    if request.internal_conversation_ref.trim().is_empty() {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar upsert_event request requires internal_conversation_ref",
        ));
    }
    if request.title.trim().is_empty() {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar upsert_event request requires title",
        ));
    }
    if request.starts_at >= request.ends_at {
        return Err(CalendarIntegrationError::permanent_failure(
            "calendar upsert_event request requires starts_at before ends_at",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DeterministicEmailIntegrationAdapter {
    _provider: String,
    _api_base_url: Option<String>,
    _credential: String,
}

impl DeterministicEmailIntegrationAdapter {
    pub fn from_resolved_config(config: &ResolvedEmailIntegrationConfig) -> Self {
        Self {
            _provider: config.provider.clone(),
            _api_base_url: config.api_base_url.clone(),
            _credential: config.credential.clone(),
        }
    }
}

impl EmailIntegrationAdapter for DeterministicEmailIntegrationAdapter {
    async fn list_messages(
        &self,
        request: &EmailListMessagesRequest,
    ) -> std::result::Result<Vec<EmailMessageSummary>, EmailIntegrationError> {
        validate_email_list_messages_request(request)?;
        let message = EmailMessageSummary {
            external_message_id: format!(
                "deterministic-email:{}:{}:{}",
                request.internal_principal_ref.trim(),
                request.internal_conversation_ref.trim(),
                request.max_results
            ),
            mailbox: request
                .mailbox
                .clone()
                .unwrap_or_else(|| "inbox".to_string())
                .trim()
                .to_string(),
            from: "sender@example.com".to_string(),
            to: vec!["primary@example.com".to_string()],
            subject: request
                .query
                .clone()
                .unwrap_or_else(|| "Deterministic inbox message".to_string()),
            sent_at: Utc::now(),
        };
        Ok(vec![message])
    }

    async fn send_message(
        &self,
        request: &SendEmailMessageRequest,
    ) -> std::result::Result<EmailMessageSummary, EmailIntegrationError> {
        validate_send_email_message_request(request)?;
        Ok(EmailMessageSummary {
            external_message_id: format!(
                "deterministic-email:{}:{}:{}",
                request.internal_principal_ref.trim(),
                request.internal_conversation_ref.trim(),
                request.subject.trim().len()
            ),
            mailbox: "sent".to_string(),
            from: "primary@example.com".to_string(),
            to: request
                .to
                .iter()
                .map(|value| value.trim().to_string())
                .collect(),
            subject: request.subject.trim().to_string(),
            sent_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredEmailIntegrationAdapter;

impl EmailIntegrationAdapter for UnconfiguredEmailIntegrationAdapter {
    async fn list_messages(
        &self,
        _request: &EmailListMessagesRequest,
    ) -> std::result::Result<Vec<EmailMessageSummary>, EmailIntegrationError> {
        Err(EmailIntegrationError::misconfigured(
            "email integration adapter is not configured",
        ))
    }

    async fn send_message(
        &self,
        _request: &SendEmailMessageRequest,
    ) -> std::result::Result<EmailMessageSummary, EmailIntegrationError> {
        Err(EmailIntegrationError::misconfigured(
            "email integration adapter is not configured",
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FakeEmailIntegrationAdapter;

impl EmailIntegrationAdapter for FakeEmailIntegrationAdapter {
    async fn list_messages(
        &self,
        request: &EmailListMessagesRequest,
    ) -> std::result::Result<Vec<EmailMessageSummary>, EmailIntegrationError> {
        validate_email_list_messages_request(request)?;
        Err(EmailIntegrationError::temporary_failure(
            "no fake email list response queued",
        ))
    }

    async fn send_message(
        &self,
        request: &SendEmailMessageRequest,
    ) -> std::result::Result<EmailMessageSummary, EmailIntegrationError> {
        validate_send_email_message_request(request)?;
        Err(EmailIntegrationError::temporary_failure(
            "no fake email send response queued",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct DeterministicTaskSyncIntegrationAdapter {
    _provider: String,
    _api_base_url: Option<String>,
    _credential: String,
}

impl DeterministicTaskSyncIntegrationAdapter {
    pub fn from_resolved_config(config: &ResolvedTaskSyncIntegrationConfig) -> Self {
        Self {
            _provider: config.provider.clone(),
            _api_base_url: config.api_base_url.clone(),
            _credential: config.credential.clone(),
        }
    }
}

impl TaskSyncIntegrationAdapter for DeterministicTaskSyncIntegrationAdapter {
    async fn sync_task_list(
        &self,
        request: &TaskSyncRequest,
    ) -> std::result::Result<TaskSyncResult, TaskSyncIntegrationError> {
        validate_task_sync_request(request)?;
        let items = request
            .items
            .iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        Ok(TaskSyncResult {
            external_list_id: request.external_list_id.clone().unwrap_or_else(|| {
                format!(
                    "deterministic-task-list:{}:{}",
                    request.internal_principal_ref.trim(),
                    request.internal_conversation_ref.trim()
                )
            }),
            task_list_title: request.task_list_title.trim().to_string(),
            items,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredTaskSyncIntegrationAdapter;

impl TaskSyncIntegrationAdapter for UnconfiguredTaskSyncIntegrationAdapter {
    async fn sync_task_list(
        &self,
        _request: &TaskSyncRequest,
    ) -> std::result::Result<TaskSyncResult, TaskSyncIntegrationError> {
        Err(TaskSyncIntegrationError::misconfigured(
            "task sync integration adapter is not configured",
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FakeTaskSyncIntegrationAdapter;

impl TaskSyncIntegrationAdapter for FakeTaskSyncIntegrationAdapter {
    async fn sync_task_list(
        &self,
        request: &TaskSyncRequest,
    ) -> std::result::Result<TaskSyncResult, TaskSyncIntegrationError> {
        validate_task_sync_request(request)?;
        Err(TaskSyncIntegrationError::temporary_failure(
            "no fake task sync response queued",
        ))
    }
}

fn validate_email_list_messages_request(
    request: &EmailListMessagesRequest,
) -> std::result::Result<(), EmailIntegrationError> {
    if request.internal_principal_ref.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email list_messages request requires internal_principal_ref",
        ));
    }
    if request.internal_conversation_ref.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email list_messages request requires internal_conversation_ref",
        ));
    }
    if request.max_results == 0 {
        return Err(EmailIntegrationError::permanent_failure(
            "email list_messages request requires max_results greater than zero",
        ));
    }
    Ok(())
}

fn validate_send_email_message_request(
    request: &SendEmailMessageRequest,
) -> std::result::Result<(), EmailIntegrationError> {
    if request.internal_principal_ref.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email send_message request requires internal_principal_ref",
        ));
    }
    if request.internal_conversation_ref.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email send_message request requires internal_conversation_ref",
        ));
    }
    if request.to.iter().all(|value| value.trim().is_empty()) {
        return Err(EmailIntegrationError::permanent_failure(
            "email send_message request requires at least one recipient",
        ));
    }
    if request.subject.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email send_message request requires subject",
        ));
    }
    if request.body_text.trim().is_empty() {
        return Err(EmailIntegrationError::permanent_failure(
            "email send_message request requires body_text",
        ));
    }
    Ok(())
}

fn validate_task_sync_request(
    request: &TaskSyncRequest,
) -> std::result::Result<(), TaskSyncIntegrationError> {
    if request.internal_principal_ref.trim().is_empty() {
        return Err(TaskSyncIntegrationError::permanent_failure(
            "task sync request requires internal_principal_ref",
        ));
    }
    if request.internal_conversation_ref.trim().is_empty() {
        return Err(TaskSyncIntegrationError::permanent_failure(
            "task sync request requires internal_conversation_ref",
        ));
    }
    if request.task_list_title.trim().is_empty() {
        return Err(TaskSyncIntegrationError::permanent_failure(
            "task sync request requires task_list_title",
        ));
    }
    if request.items.is_empty() {
        return Err(TaskSyncIntegrationError::permanent_failure(
            "task sync request requires at least one task item",
        ));
    }
    if request.items.iter().all(|value| value.trim().is_empty()) {
        return Err(TaskSyncIntegrationError::permanent_failure(
            "task sync request requires at least one non-empty task item",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn sample_list_request() -> CalendarListEventsRequest {
        CalendarListEventsRequest {
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            start_at: Utc::now(),
            end_at: Utc::now() + Duration::hours(4),
            max_results: 5,
        }
    }

    fn sample_upsert_request() -> CalendarUpsertEventRequest {
        CalendarUpsertEventRequest {
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            title: "Ship Milestone 3".to_string(),
            starts_at: Utc::now() + Duration::hours(1),
            ends_at: Utc::now() + Duration::hours(2),
            location: Some("remote".to_string()),
            details: Some("integration alignment".to_string()),
            external_event_id: None,
        }
    }

    fn sample_email_list_request() -> EmailListMessagesRequest {
        EmailListMessagesRequest {
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            mailbox: Some("inbox".to_string()),
            query: Some("subject:milestone".to_string()),
            max_results: 5,
        }
    }

    fn sample_email_send_request() -> SendEmailMessageRequest {
        SendEmailMessageRequest {
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            to: vec!["owner@example.com".to_string()],
            cc: vec![],
            subject: "Milestone update".to_string(),
            body_text: "We shipped milestone 3.".to_string(),
            reply_to_external_message_id: None,
        }
    }

    fn sample_task_sync_request() -> TaskSyncRequest {
        TaskSyncRequest {
            internal_principal_ref: "primary-user".to_string(),
            internal_conversation_ref: "telegram-primary".to_string(),
            task_list_title: "Milestone Tasks".to_string(),
            items: vec![
                "Review PR #123".to_string(),
                "Write release notes".to_string(),
            ],
            external_list_id: None,
        }
    }

    #[tokio::test]
    async fn unconfigured_adapter_fails_closed() {
        let adapter = UnconfiguredCalendarIntegrationAdapter;
        let error = adapter
            .list_events(&sample_list_request())
            .await
            .expect_err("unconfigured adapter should fail closed");
        assert_eq!(error.kind, CalendarIntegrationErrorKind::Misconfigured);
    }

    #[tokio::test]
    async fn fake_adapter_records_requests_and_returns_queued_responses() {
        let adapter = FakeCalendarIntegrationAdapter::new();
        let list_request = sample_list_request();
        let upsert_request = sample_upsert_request();

        let expected_event = CalendarEventSummary {
            external_event_id: "evt-123".to_string(),
            title: upsert_request.title.clone(),
            starts_at: upsert_request.starts_at,
            ends_at: upsert_request.ends_at,
            location: upsert_request.location.clone(),
        };
        adapter.push_list_response(Ok(vec![expected_event.clone()]));
        adapter.push_upsert_response(Ok(expected_event.clone()));

        let listed = adapter
            .list_events(&list_request)
            .await
            .expect("list_events should use queued response");
        let upserted = adapter
            .upsert_event(&upsert_request)
            .await
            .expect("upsert_event should use queued response");

        assert_eq!(listed, vec![expected_event.clone()]);
        assert_eq!(upserted, expected_event);
        assert_eq!(adapter.seen_list_requests(), vec![list_request]);
        assert_eq!(adapter.seen_upsert_requests(), vec![upsert_request]);
    }

    #[tokio::test]
    async fn fake_adapter_rejects_invalid_request_shape() {
        let adapter = FakeCalendarIntegrationAdapter::new();
        let mut request = sample_list_request();
        request.max_results = 0;

        let error = adapter
            .list_events(&request)
            .await
            .expect_err("invalid request should fail before adapter call");
        assert_eq!(error.kind, CalendarIntegrationErrorKind::PermanentFailure);
        assert!(adapter.seen_list_requests().is_empty());
    }

    #[test]
    fn supported_provider_list_contains_deterministic_fake() {
        assert!(is_supported_calendar_provider("deterministic_fake"));
        assert!(is_supported_calendar_provider(" fake "));
        assert!(!is_supported_calendar_provider("google_calendar"));
        assert!(is_supported_email_provider("deterministic_fake"));
        assert!(is_supported_task_sync_provider("fake"));
        assert!(!is_supported_email_provider("gmail"));
        assert!(!is_supported_task_sync_provider("todoist"));
    }

    #[tokio::test]
    async fn email_adapters_validate_and_fail_closed_or_succeed_deterministically() {
        let unconfigured = UnconfiguredEmailIntegrationAdapter;
        let error = unconfigured
            .list_messages(&sample_email_list_request())
            .await
            .expect_err("unconfigured email adapter should fail closed");
        assert_eq!(error.kind, EmailIntegrationErrorKind::Misconfigured);

        let fake = FakeEmailIntegrationAdapter;
        let fake_error = fake
            .send_message(&sample_email_send_request())
            .await
            .expect_err("fake adapter should emit deterministic failure");
        assert_eq!(fake_error.kind, EmailIntegrationErrorKind::TemporaryFailure);

        let deterministic = DeterministicEmailIntegrationAdapter::from_resolved_config(
            &ResolvedEmailIntegrationConfig {
                provider: "deterministic_fake".to_string(),
                credential: "secret".to_string(),
                api_base_url: None,
            },
        );
        let listed = deterministic
            .list_messages(&sample_email_list_request())
            .await
            .expect("deterministic email list should succeed");
        assert_eq!(listed.len(), 1);
        let sent = deterministic
            .send_message(&sample_email_send_request())
            .await
            .expect("deterministic email send should succeed");
        assert_eq!(sent.mailbox, "sent");
    }

    #[tokio::test]
    async fn task_sync_adapters_validate_and_fail_closed_or_succeed_deterministically() {
        let unconfigured = UnconfiguredTaskSyncIntegrationAdapter;
        let error = unconfigured
            .sync_task_list(&sample_task_sync_request())
            .await
            .expect_err("unconfigured task sync adapter should fail closed");
        assert_eq!(error.kind, TaskSyncIntegrationErrorKind::Misconfigured);

        let fake = FakeTaskSyncIntegrationAdapter;
        let fake_error = fake
            .sync_task_list(&sample_task_sync_request())
            .await
            .expect_err("fake task sync adapter should emit deterministic failure");
        assert_eq!(
            fake_error.kind,
            TaskSyncIntegrationErrorKind::TemporaryFailure
        );

        let deterministic = DeterministicTaskSyncIntegrationAdapter::from_resolved_config(
            &ResolvedTaskSyncIntegrationConfig {
                provider: "deterministic_fake".to_string(),
                credential: "secret".to_string(),
                api_base_url: None,
            },
        );
        let synced = deterministic
            .sync_task_list(&sample_task_sync_request())
            .await
            .expect("deterministic task sync should succeed");
        assert_eq!(synced.task_list_title, "Milestone Tasks");
        assert_eq!(synced.items.len(), 2);
    }
}
