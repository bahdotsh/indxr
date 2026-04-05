#![cfg(feature = "wiki")]

//! Integration tests for wiki generation via the `indxr wiki` CLI.
//!
//! These tests use the `--exec` flag with a mock script to avoid real LLM calls.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use tempfile::TempDir;

/// Creates a mock LLM script that returns canned responses based on
/// whether the stdin JSON contains planning keywords.
fn create_mock_llm_script(dir: &std::path::Path) -> std::path::PathBuf {
    let script_path = dir.join("mock-llm.sh");
    // The planning system prompt contains "wiki plan" and "page plans".
    // The index system prompt contains "index page for a codebase wiki".
    // We match on those to distinguish call types.
    let content = r#"#!/bin/bash
INPUT=$(cat)

if echo "$INPUT" | grep -q 'page plans'; then
    printf '[{"id":"architecture","page_type":"architecture","title":"Architecture Overview","source_files":["src/main.rs"]},{"id":"mod-core","page_type":"module","title":"Core Module","source_files":["src/main.rs"]},{"id":"index","page_type":"index","title":"Wiki Index","source_files":[]}]'
elif echo "$INPUT" | grep -q 'index page for a codebase wiki'; then
    printf '# Wiki Index\n\n## Architecture\n- [[architecture]] - Architecture Overview\n\n## Modules\n- [[mod-core]] - Core Module\n'
else
    printf '# Generated Page\n\nThis is a mock wiki page. See [[architecture]] for details.\n\n## Overview\nThe codebase follows a modular design pattern.\n'
fi
"#;

    fs::write(&script_path, content).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    script_path
}

/// Initialize a minimal git repo with a source file so indxr can index it.
fn create_test_project(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();

    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/main.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn indxr_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("indxr")
}

#[test]
fn test_wiki_generate_with_exec() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);
    let script = create_mock_llm_script(tmp.path());
    let wiki_dir = project_dir.join(".indxr/wiki");

    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "generate",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "wiki generate failed:\nstderr: {stderr}\nstdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // Verify wiki was created
    assert!(wiki_dir.join("manifest.yaml").exists(), "manifest missing");
    assert!(
        wiki_dir.join("architecture.md").exists(),
        "architecture page missing"
    );
    assert!(
        wiki_dir.join("modules/mod-core.md").exists(),
        "module page missing"
    );
    assert!(wiki_dir.join("index.md").exists(), "index page missing");

    // Verify manifest content
    let manifest_text = fs::read_to_string(wiki_dir.join("manifest.yaml")).unwrap();
    assert!(manifest_text.contains("architecture"));
    assert!(manifest_text.contains("mod-core"));

    // Verify page content was written correctly
    let arch_text = fs::read_to_string(wiki_dir.join("architecture.md")).unwrap();
    assert!(arch_text.contains("---")); // Has frontmatter
    assert!(arch_text.contains("Generated Page")); // Has content
}

#[test]
fn test_wiki_dry_run_does_not_write() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);
    let script = create_mock_llm_script(tmp.path());
    let wiki_dir = project_dir.join(".indxr/wiki");

    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "generate",
            "--dry-run",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "dry run failed: {stderr}\nstdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(stderr.contains("Dry run") || stderr.contains("dry run"));

    // No files should be written
    assert!(
        !wiki_dir.exists(),
        "wiki dir should not be created in dry run"
    );
}

#[test]
fn test_wiki_status_no_wiki() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);

    let output = Command::new(indxr_bin())
        .args(["wiki", "status"])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No wiki found"));
}

#[test]
fn test_wiki_status_after_generate() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);
    let script = create_mock_llm_script(tmp.path());
    let wiki_dir = project_dir.join(".indxr/wiki");

    // Generate first
    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "generate",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Now check status
    let output = Command::new(indxr_bin())
        .args(["wiki", "--wiki-dir", wiki_dir.to_str().unwrap(), "status"])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Pages:"));
}

#[test]
fn test_wiki_update_after_code_change() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);
    let script = create_mock_llm_script(tmp.path());
    let wiki_dir = project_dir.join(".indxr/wiki");

    // Step 1: Generate the wiki
    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "generate",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify wiki exists before update
    assert!(wiki_dir.join("manifest.yaml").exists());
    let manifest_before = fs::read_to_string(wiki_dir.join("manifest.yaml")).unwrap();
    assert!(manifest_before.contains("architecture"));

    // Step 2: Make a code change and commit
    fs::write(
        project_dir.join("src/main.rs"),
        "fn main() {\n    println!(\"updated\");\n}\n\npub fn new_function() -> bool {\n    true\n}\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add new_function"])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    // Step 3: Run wiki update against the first commit
    let first_commit = Command::new("git")
        .args(["rev-parse", "HEAD~1"])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    let since_ref = String::from_utf8(first_commit.stdout).unwrap();
    let since_ref = since_ref.trim();

    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "update",
            "--since",
            since_ref,
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "wiki update failed:\nstderr: {stderr}\nstdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // Step 4: Verify update results
    // stderr should mention finding changed files and updating pages
    assert!(
        stderr.contains("changed files")
            || stderr.contains("Updated")
            || stderr.contains("updated")
            || stderr.contains("No wiki pages"),
        "Expected update progress in stderr, got: {stderr}"
    );

    // Manifest should still be valid
    let manifest_after = fs::read_to_string(wiki_dir.join("manifest.yaml")).unwrap();
    assert!(manifest_after.contains("architecture"));

    // Wiki pages should still exist
    assert!(wiki_dir.join("architecture.md").exists());
}

#[test]
fn test_wiki_update_no_changes() {
    let tmp = TempDir::new().unwrap();
    let project_dir = tmp.path().join("project");
    fs::create_dir_all(&project_dir).unwrap();

    create_test_project(&project_dir);
    let script = create_mock_llm_script(tmp.path());
    let wiki_dir = project_dir.join(".indxr/wiki");

    // Generate the wiki
    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "generate",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Update with --since HEAD (no changes)
    let output = Command::new(indxr_bin())
        .args([
            "wiki",
            "--exec",
            script.to_str().unwrap(),
            "--wiki-dir",
            wiki_dir.to_str().unwrap(),
            "update",
            "--since",
            "HEAD",
        ])
        .current_dir(&project_dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "wiki update failed: {stderr}");
    assert!(
        stderr.contains("No file changes"),
        "Expected no-changes message, got: {stderr}"
    );
}

#[test]
fn test_mock_llm_script_returns_valid_plan() {
    let tmp = TempDir::new().unwrap();
    let script = create_mock_llm_script(tmp.path());

    let mut child = Command::new(&script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Simulate what the wiki generator sends — the system prompt contains "page plans"
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"system": "Your output must be a JSON array of wiki page plans.", "messages": []}"#)
        .unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}
