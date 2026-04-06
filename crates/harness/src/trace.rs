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
        fmt()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_env_filter(EnvFilter::new(log_filter))
            .try_init()
            .map_err(|error| error.to_string())
    });

    init_result
        .as_ref()
        .map_err(|error| anyhow!("failed to initialize tracing subscriber: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_trace_context_uses_non_nil_uuid() {
        let trace = TraceContext::root();
        assert_ne!(trace.trace_id, Uuid::nil());
    }
}
