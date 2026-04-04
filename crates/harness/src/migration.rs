use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub name: String,
    pub checksum: String,
    pub sql: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationSummary {
    pub discovered_versions: Vec<i64>,
    pub applied_versions: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
    pub checksum: String,
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist")
        .to_path_buf()
}

pub fn migrations_dir() -> PathBuf {
    workspace_root().join("migrations")
}

pub fn load_migrations() -> Result<Vec<Migration>> {
    let mut entries =
        fs::read_dir(migrations_dir()).context("failed to read migrations directory")?;
    let mut migrations = Vec::new();

    while let Some(entry) = entries
        .next()
        .transpose()
        .context("failed to read migration entry")?
    {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("sql") {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("invalid migration filename"))?;
        let (version, name) = parse_migration_name(file_name)?;
        let sql = fs::read_to_string(&path)
            .with_context(|| format!("failed to read migration {}", path.display()))?;
        let checksum = hex::encode(Sha256::digest(sql.as_bytes()));
        migrations.push(Migration {
            version,
            name,
            checksum,
            sql,
        });
    }

    migrations.sort_by_key(|migration| migration.version);
    Ok(migrations)
}

pub fn latest_version(migrations: &[Migration]) -> i64 {
    migrations
        .last()
        .map(|migration| migration.version)
        .unwrap_or(0)
}

pub async fn ensure_schema_migrations_table(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version BIGINT PRIMARY KEY,
            name TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            app_version TEXT NOT NULL,
            applied_by TEXT NOT NULL,
            execution_ms BIGINT NOT NULL CHECK (execution_ms >= 0)
        )
        "#,
    )
    .execute(pool)
    .await
    .context("failed to ensure schema_migrations table exists")?;
    Ok(())
}

pub async fn apply_pending_migrations(
    pool: &PgPool,
    app_version: &str,
) -> Result<MigrationSummary> {
    ensure_schema_migrations_table(pool).await?;
    let migrations = load_migrations()?;
    let applied = applied_versions(pool).await?;
    validate_applied_history(&migrations, &load_applied_migrations(pool).await?)?;
    let applied_set = applied
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut newly_applied = Vec::new();

    for migration in &migrations {
        if applied_set.contains(&migration.version) {
            continue;
        }

        let start = Instant::now();
        let mut transaction = pool
            .begin()
            .await
            .context("failed to begin migration transaction")?;
        sqlx::raw_sql(&migration.sql)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("failed to execute migration {}", migration.version))?;
        sqlx::query(
            r#"
            INSERT INTO schema_migrations
                (version, name, checksum, applied_at, app_version, applied_by, execution_ms)
            VALUES
                ($1, $2, $3, NOW(), $4, $5, $6)
            "#,
        )
        .bind(migration.version)
        .bind(&migration.name)
        .bind(&migration.checksum)
        .bind(app_version)
        .bind(applied_by())
        .bind(start.elapsed().as_millis() as i64)
        .execute(&mut *transaction)
        .await
        .with_context(|| format!("failed to record migration {}", migration.version))?;
        transaction
            .commit()
            .await
            .with_context(|| format!("failed to commit migration {}", migration.version))?;
        newly_applied.push(migration.version);
    }

    Ok(MigrationSummary {
        discovered_versions: migrations
            .iter()
            .map(|migration| migration.version)
            .collect(),
        applied_versions: newly_applied,
    })
}

pub async fn applied_versions(pool: &PgPool) -> Result<Vec<i64>> {
    ensure_schema_migrations_table(pool).await?;
    let rows = sqlx::query("SELECT version FROM schema_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .context("failed to fetch applied schema versions")?;
    Ok(rows
        .into_iter()
        .map(|row| row.get::<i64, _>("version"))
        .collect())
}

pub async fn load_applied_migrations(pool: &PgPool) -> Result<Vec<AppliedMigration>> {
    ensure_schema_migrations_table(pool).await?;
    let rows = sqlx::query(
        r#"
        SELECT version, name, checksum
        FROM schema_migrations
        ORDER BY version
        "#,
    )
    .fetch_all(pool)
    .await
    .context("failed to fetch applied migration metadata")?;

    Ok(rows
        .into_iter()
        .map(|row| AppliedMigration {
            version: row.get("version"),
            name: row.get("name"),
            checksum: row.get("checksum"),
        })
        .collect())
}

pub fn validate_applied_history(
    discovered: &[Migration],
    applied: &[AppliedMigration],
) -> Result<()> {
    for (index, applied_migration) in applied.iter().enumerate() {
        let Some(expected) = discovered.get(index) else {
            bail!(
                "applied migration history contains unexpected version {}; runtime expected at most {} reviewed migrations",
                applied_migration.version,
                discovered.len()
            );
        };

        if applied_migration.version != expected.version {
            bail!(
                "applied migration history is incomplete or out of order at position {}: expected version {}, found {}",
                index + 1,
                expected.version,
                applied_migration.version
            );
        }

        if applied_migration.name != expected.name {
            bail!(
                "applied migration {} has name '{}', but reviewed migration name is '{}'",
                applied_migration.version,
                applied_migration.name,
                expected.name
            );
        }

        if applied_migration.checksum != expected.checksum {
            bail!(
                "applied migration {} checksum does not match the reviewed migration file",
                applied_migration.version
            );
        }
    }

    Ok(())
}

fn parse_migration_name(file_name: &str) -> Result<(i64, String)> {
    let stem = file_name
        .strip_suffix(".sql")
        .ok_or_else(|| anyhow!("migration filename must end with .sql"))?;
    let (version, name) = stem
        .split_once("__")
        .ok_or_else(|| anyhow!("migration filename must follow NNNN__short_snake_case.sql"))?;
    if version.len() != 4 || !version.chars().all(|character| character.is_ascii_digit()) {
        bail!("migration version must be four digits");
    }
    if !name.chars().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    }) {
        bail!("migration name must be snake_case");
    }
    Ok((version.parse::<i64>()?, name.to_string()))
}

fn applied_by() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_migration_name_accepts_expected_format() {
        let (version, name) = parse_migration_name("0001__phase_1_foundation.sql")
            .expect("migration name should parse");
        assert_eq!(version, 1);
        assert_eq!(name, "phase_1_foundation");
    }

    #[test]
    fn load_migrations_discovers_phase_1_files() {
        let versions = load_migrations()
            .expect("migrations should load")
            .into_iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();
        assert_eq!(versions, vec![1]);
    }

    #[test]
    fn validate_applied_history_rejects_out_of_order_versions() {
        let discovered = vec![Migration {
            version: 1,
            name: "phase_1_foundation".to_string(),
            checksum: "abc".to_string(),
            sql: "SELECT 1".to_string(),
        }];
        let applied = vec![AppliedMigration {
            version: 2,
            name: "phase_2".to_string(),
            checksum: "def".to_string(),
        }];

        let error = validate_applied_history(&discovered, &applied)
            .expect_err("history should be rejected");
        assert!(error.to_string().contains("incomplete or out of order"));
    }
}
