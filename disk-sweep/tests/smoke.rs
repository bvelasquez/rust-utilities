use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_shows_agent_hints() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("capabilities --json"));
}

#[test]
fn capabilities_json() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"success\": true"))
        .stdout(predicate::str::contains("disk-sweep"));
}

#[test]
fn scan_json() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["scan", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("total_bytes"));
}

#[test]
fn targets_list() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["targets", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("xcode-derived-data"));
}

#[test]
fn watch_json_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_string_lossy();

    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["watch", "--json", "--top", "3", "--path", &path])
        .assert()
        .success()
        .stdout(predicate::str::contains("volume"))
        .stdout(predicate::str::contains("folders"));
}

#[test]
fn analyze_json() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("tiny-crate");
    let incremental = project
        .join("target")
        .join("debug")
        .join("incremental");
    std::fs::create_dir_all(&incremental).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"tiny\"\n").unwrap();
    std::fs::write(incremental.join("blob"), vec![0u8; 2 * 1024 * 1024]).unwrap();

    let projects = root.path().to_string_lossy();

    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args([
            "analyze",
            "--json",
            "--skip-dot",
            "--skip-library",
            "--projects-root",
            &projects,
            "--project-build-min-mb",
            "1",
            "--stale-days",
            "99999",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("project_build_count"))
        .stdout(predicate::str::contains("Rust build artifacts"));
}

#[test]
fn capabilities_includes_analyze() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"analyze\""));
}

#[test]
fn capabilities_includes_watch() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"watch\""));
}

#[test]
fn env_schema_includes_openrouter() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["env", "schema", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DISK_SWEEP_OPENROUTER_KEY"))
        .stdout(predicate::str::contains("OPENROUTER_API_KEY"));
}

#[test]
fn clean_requires_selection_or_yes() {
    Command::cargo_bin("disk-sweep")
        .unwrap()
        .args(["clean", "--targets", "nonexistent-id", "--json"])
        .assert()
        .failure();
}
