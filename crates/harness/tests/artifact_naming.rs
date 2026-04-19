use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn implementation_artifacts_do_not_use_planning_labels() -> Result<()> {
    let root = workspace_root();
    let mut violations = Vec::new();
    for relative_root in implementation_roots() {
        collect_violations(&root, &root.join(relative_root), &mut violations)?;
    }

    if !violations.is_empty() {
        anyhow::bail!(
            "planning labels leaked into implementation artifacts:\n{}",
            violations.join("\n")
        );
    }

    Ok(())
}

fn collect_violations(root: &Path, current: &Path, violations: &mut Vec<String>) -> Result<()> {
    if !current.exists() {
        return Ok(());
    }

    if current.is_file() {
        collect_file_violations(root, current, violations)?;
        return Ok(());
    }

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

        collect_file_violations(root, &path, violations)?;
    }

    Ok(())
}

fn collect_file_violations(root: &Path, path: &Path, violations: &mut Vec<String>) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .expect("walked path should remain under workspace root");

    if should_skip_path(relative) {
        return Ok(());
    }

    let relative_text = slash_path(relative);
    if contains_forbidden_label(&relative_text) {
        violations.push(format!("filename: {relative_text}"));
    }

    let contents = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(()),
    };
    let contents = match String::from_utf8(contents) {
        Ok(text) => text,
        Err(_) => return Ok(()),
    };
    let lowered_contents = contents.to_ascii_lowercase();
    for label in forbidden_labels(&lowered_contents) {
        violations.push(format!("content: {relative_text} contains '{label}'"));
    }

    Ok(())
}

fn implementation_roots() -> &'static [&'static str] {
    &[
        ".github/workflows",
        "config",
        "crates",
        "migrations",
        "scripts",
        ".env.example",
        "compose.yaml",
    ]
}

fn should_skip_path(relative: &Path) -> bool {
    let relative_text = slash_path(relative);
    relative_text.starts_with("target/")
        || relative_text.starts_with(".git/")
        || relative_text.starts_with("docs/archive/")
        || relative_text == "crates/harness/tests/artifact_naming.rs"
        || relative_text == "docs/HIGH_LEVEL_IMPLEMENTATION_PLAN.md"
        || (relative_text.starts_with("docs/PHASE_")
            && relative_text.ends_with("_DETAILED_IMPLEMENTATION_PLAN.md"))
        || relative_text.starts_with("migrations/0001__")
        || relative_text.starts_with("migrations/0002__")
}

fn contains_forbidden_label(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    !forbidden_labels(&lowered).is_empty()
}

fn forbidden_labels(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for prefix in ["phase", "phase_", "phase-", "phase "] {
        for number in ['1', '2', '3', '4', '5', '6', '7', '8', '9'] {
            tokens.push(format!("{prefix}{number}"));
        }
    }
    for label in [
        "phase one",
        "phase two",
        "phase three",
        "phase four",
        "phase five",
        "phase six",
        "milestone a",
        "milestone b",
        "milestone c",
        "milestone d",
        "implementation plan",
        "implementation plans",
        "detailed implementation plan",
        "detailed implementation plans",
        "high-level implementation plan",
        "execution ledger",
        "implementation ledger",
    ] {
        tokens.push(label.to_string());
    }

    let mut matches = tokens
        .into_iter()
        .filter(|token| text.contains(token))
        .collect::<Vec<_>>();

    for token in task_id_tokens() {
        if text.contains(&token) {
            matches.push(token);
        }
    }

    matches
}

fn task_id_tokens() -> Vec<String> {
    let mut tokens = Vec::new();
    for phase in 1..=9 {
        for task in 1..=99 {
            tokens.push(format!("p{phase}-{task:02}"));
            tokens.push(format!("task p{phase}-{task:02}"));
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
