use anyhow::{Result, bail};
use sqlx::PgPool;

use crate::migration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaPolicy {
    pub minimum_supported_version: i64,
    pub expected_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaCompatibility {
    Supported {
        current: i64,
    },
    Missing,
    TooOld {
        current: i64,
        minimum_supported: i64,
    },
    PendingMigrations {
        current: i64,
        expected: i64,
    },
    TooNew {
        current: i64,
        expected: i64,
    },
    IncompatibleHistory {
        details: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaUpgradeAssessment {
    pub policy: SchemaPolicy,
    pub current_version: Option<i64>,
    pub expected_version: i64,
    pub discovered_versions: Vec<i64>,
    pub applied_versions: Vec<i64>,
    pub pending_versions: Vec<i64>,
    pub compatibility: SchemaCompatibility,
}

impl SchemaCompatibility {
    pub fn ensure_supported(self) -> Result<i64> {
        match self {
            SchemaCompatibility::Supported { current } => Ok(current),
            SchemaCompatibility::Missing => bail!("database schema is missing required migrations"),
            SchemaCompatibility::TooOld {
                current,
                minimum_supported,
            } => bail!(
                "database schema version {current} is below minimum supported version {minimum_supported}"
            ),
            SchemaCompatibility::PendingMigrations { current, expected } => bail!(
                "database schema version {current} is behind expected version {expected}; run the migrate command first"
            ),
            SchemaCompatibility::TooNew { current, expected } => bail!(
                "database schema version {current} is newer than runtime-supported version {expected}"
            ),
            SchemaCompatibility::IncompatibleHistory { details } => bail!(
                "database schema history is incompatible with the reviewed migration set: {details}"
            ),
        }
    }
}

pub fn evaluate(current: Option<i64>, policy: SchemaPolicy) -> SchemaCompatibility {
    match current {
        None => SchemaCompatibility::Missing,
        Some(current) if current < policy.minimum_supported_version => {
            SchemaCompatibility::TooOld {
                current,
                minimum_supported: policy.minimum_supported_version,
            }
        }
        Some(current) if current < policy.expected_version => {
            SchemaCompatibility::PendingMigrations {
                current,
                expected: policy.expected_version,
            }
        }
        Some(current) if current > policy.expected_version => SchemaCompatibility::TooNew {
            current,
            expected: policy.expected_version,
        },
        Some(current) => SchemaCompatibility::Supported { current },
    }
}

pub async fn verify(pool: &PgPool, policy: SchemaPolicy) -> Result<i64> {
    assess_upgrade_path(pool, policy)
        .await?
        .compatibility
        .ensure_supported()
}

pub async fn assess_upgrade_path(
    pool: &PgPool,
    policy: SchemaPolicy,
) -> Result<SchemaUpgradeAssessment> {
    let discovered = migration::load_migrations()?;
    migration::normalize_applied_migration_names(pool, &discovered).await?;
    let applied = migration::load_applied_migrations(pool).await?;
    let discovered_versions = discovered
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let applied_versions = applied
        .iter()
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    let current_version = applied.last().map(|migration| migration.version);
    let pending_versions = discovered_versions
        .iter()
        .copied()
        .filter(|version| current_version.is_none_or(|current| *version > current))
        .collect::<Vec<_>>();
    let compatibility = match migration::validate_applied_history(&discovered, &applied) {
        Ok(()) => evaluate(current_version, policy),
        Err(error) => SchemaCompatibility::IncompatibleHistory {
            details: error.to_string(),
        },
    };

    Ok(SchemaUpgradeAssessment {
        policy,
        current_version,
        expected_version: policy.expected_version,
        discovered_versions,
        applied_versions,
        pending_versions,
        compatibility,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_reports_missing_schema() {
        let status = evaluate(
            None,
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 1,
            },
        );
        assert_eq!(status, SchemaCompatibility::Missing);
    }

    #[test]
    fn evaluate_reports_pending_migrations() {
        let status = evaluate(
            Some(1),
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 2,
            },
        );
        assert_eq!(
            status,
            SchemaCompatibility::PendingMigrations {
                current: 1,
                expected: 2,
            }
        );
    }

    #[test]
    fn evaluate_reports_supported_schema() {
        let status = evaluate(
            Some(1),
            SchemaPolicy {
                minimum_supported_version: 1,
                expected_version: 1,
            },
        );
        assert_eq!(status, SchemaCompatibility::Supported { current: 1 });
    }

    #[test]
    fn incompatible_history_fails_closed() {
        let error = SchemaCompatibility::IncompatibleHistory {
            details: "gap".to_string(),
        }
        .ensure_supported()
        .expect_err("history should fail closed");
        assert!(error.to_string().contains("incompatible"));
    }
}
