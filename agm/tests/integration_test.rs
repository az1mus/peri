use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_init_creates_agm_json() {
    let tmp = TempDir::new().unwrap();
    let output = Command::new("cargo")
        .args(["run", "-p", "agm", "--", "init", "-C"])
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "init failed: {:?}", output);
    let manifest = tmp.path().join("agm.json");
    assert!(manifest.exists(), "agm.json not created");
}

#[test]
fn test_install_without_manifest_fails() {
    let tmp = TempDir::new().unwrap();
    let output = Command::new("cargo")
        .args([
            "run", "-p", "agm", "--", "install", "--tool", "claude", "-C",
        ])
        .arg(tmp.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "install should fail without agm.json"
    );
}

#[test]
fn test_init_then_list() {
    let tmp = TempDir::new().unwrap();

    let output = Command::new("cargo")
        .args(["run", "-p", "agm", "--", "init", "-C"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = Command::new("cargo")
        .args(["run", "-p", "agm", "--", "list", "-C"])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn test_help_output() {
    let output = Command::new("cargo")
        .args(["run", "-p", "agm", "--", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"), "help should mention init");
    assert!(stdout.contains("install"), "help should mention install");
    assert!(
        stdout.contains("uninstall"),
        "help should mention uninstall"
    );
    assert!(stdout.contains("list"), "help should mention list");
    assert!(stdout.contains("update"), "help should mention update");
    assert!(stdout.contains("publish"), "help should mention publish");
    assert!(stdout.contains("gc"), "help should mention gc");
}
