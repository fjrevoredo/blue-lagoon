mod support;

use anyhow::Result;
use harness::{migration, schema};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn upgrade_path_assessment_reports_missing_schema_before_migration() -> Result<()> {
    support::with_clean_database(|ctx| async move {
        let migrations = migration::load_migrations()?;
        let expected_version = migration::latest_version(&migrations);
        let assessment = schema::assess_upgrade_path(
            &ctx.pool,
            schema::SchemaPolicy {
                minimum_supported_version: ctx.config.database.minimum_supported_schema_version,
                expected_version,
            },
        )
        .await?;

        assert_eq!(assessment.current_version, None);
        assert_eq!(assessment.expected_version, expected_version);
        assert_eq!(
            assessment.compatibility,
            schema::SchemaCompatibility::Missing
        );
        assert_eq!(assessment.pending_versions.len(), migrations.len());
        Ok(())
    })
    .await
}

#[tokio::test]
#[serial]
async fn upgrade_path_assessment_reports_supported_schema_after_migration() -> Result<()> {
    support::with_migrated_database(|ctx| async move {
        let migrations = migration::load_migrations()?;
        let expected_version = migration::latest_version(&migrations);
        let assessment = schema::assess_upgrade_path(
            &ctx.pool,
            schema::SchemaPolicy {
                minimum_supported_version: ctx.config.database.minimum_supported_schema_version,
                expected_version,
            },
        )
        .await?;

        assert_eq!(assessment.current_version, Some(expected_version));
        assert!(assessment.pending_versions.is_empty());
        assert_eq!(
            assessment.compatibility,
            schema::SchemaCompatibility::Supported {
                current: expected_version
            }
        );
        Ok(())
    })
    .await
}
