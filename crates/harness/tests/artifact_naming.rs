use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn implementation_artifacts_do_not_use_planning_labels() -> Result<()> {
    let root = workspace_root();
    let mut violations = Vec::new();
    collect_violations(&root, &root, &mut violations)?;

    if !violations.is_empty() {
        anyhow::bail!(
            "planning labels leaked into implementation artifacts:\n{}",
            violations.join("\n")
        );
    }

    Ok(())
}

fn collect_violations(root: &Path, current: &Path, violations: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("failed to read directory {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("walked path should remain under workspace root");

        if should_skip_path(relative) {
            continue;
        }

        if path.is_dir() {
            collect_violations(root, &path, violations)?;
            continue;
        }

        let relative_text = slash_path(relative);
        if contains_forbidden_label(&relative_text) {
            violations.push(format!("filename: {relative_text}"));
        }

        let contents = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let contents = match String::from_utf8(contents) {
            Ok(text) => text,
            Err(_) => continue,
        };
        let lowered_contents = contents.to_ascii_lowercase();

        for token in forbidden_tokens() {
            if lowered_contents.contains(&token) {
                violations.push(format!("content: {relative_text} contains '{token}'"));
            }
        }
    }

    Ok(())
}

fn should_skip_path(relative: &Path) -> bool {
    let relative_text = slash_path(relative);
    relative_text.starts_with("target/")
        || relative_text.starts_with(".git/")
        || relative_text.starts_with("docs/archive/")
        || relative_text == "docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md"
        || (relative_text.starts_with("docs/PHASE_")
            && relative_text.ends_with("_DETAILED_IMPLEMENTATION_PLAN.md"))
        || relative_text.starts_with("migrations/0001__")
        || relative_text.starts_with("migrations/0002__")
}

fn contains_forbidden_label(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    forbidden_tokens()
        .into_iter()
        .any(|token| lowered.contains(&token))
}

fn forbidden_tokens() -> Vec<String> {
    let mut tokens = Vec::new();
    for prefix in ["phase", "phase_", "phase-", "phase "] {
        for number in ['1', '2', '3', '4'] {
            tokens.push(format!("{prefix}{number}"));
        }
    }
    tokens
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root should exist")
        .to_path_buf()
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
