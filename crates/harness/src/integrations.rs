use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Duration, Utc};
use thiserror::Error;

use crate::config::ResolvedCalendarIntegrationConfig;

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

pub fn is_supported_calendar_provider(provider: &str) -> bool {
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
    }
}
