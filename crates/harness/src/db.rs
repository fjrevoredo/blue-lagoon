use anyhow::{Context, Result};
use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::config::RuntimeConfig;

pub async fn connect(config: &RuntimeConfig) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database.database_url)
        .await
        .with_context(|| "failed to connect to PostgreSQL")
}
