use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use tracing_subscriber::{EnvFilter, fmt};
use uuid::Uuid;

static TRACING_READY: OnceLock<std::result::Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: Uuid,
}

impl TraceContext {
    pub fn root() -> Self {
        Self {
            trace_id: Uuid::now_v7(),
        }
    }
}

pub fn init(log_filter: &str) -> Result<()> {
    let init_result = TRACING_READY.get_or_init(|| {
        let filter = runtime_env_filter(log_filter);
        fmt()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_env_filter(filter)
            .try_init()
            .map_err(|error| error.to_string())
    });

    init_result
        .as_ref()
        .map_err(|error| anyhow!("failed to initialize tracing subscriber: {error}"))?;
    Ok(())
}

fn runtime_env_filter(log_filter: &str) -> EnvFilter {
    if mentions_sqlx_directive(log_filter) {
        EnvFilter::new(log_filter)
    } else {
        EnvFilter::new(format!(
            "{log_filter},sqlx=warn,sqlx::query=warn,sqlx::postgres::notice=warn"
        ))
    }
}

fn mentions_sqlx_directive(log_filter: &str) -> bool {
    log_filter
        .split(',')
        .map(str::trim)
        .any(|directive| directive.starts_with("sqlx"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_trace_context_uses_non_nil_uuid() {
        let trace = TraceContext::root();
        assert_ne!(trace.trace_id, Uuid::nil());
    }

    #[test]
    fn runtime_env_filter_suppresses_sqlx_noise_by_default() {
        let filter = runtime_env_filter("debug").to_string();
        assert!(filter.contains("debug"));
        assert!(filter.contains("sqlx=warn"));
        assert!(filter.contains("sqlx::query=warn"));
    }

    #[test]
    fn runtime_env_filter_preserves_explicit_sqlx_directive() {
        let filter = runtime_env_filter("debug,sqlx=debug").to_string();
        assert!(filter.contains("sqlx=debug"));
        assert!(filter.contains("debug"));
        assert!(!filter.contains("sqlx=warn"));
    }
}
