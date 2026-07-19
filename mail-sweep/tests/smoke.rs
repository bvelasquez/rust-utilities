use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_shows_agent_hints() {
    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("capabilities --json"));
}

#[test]
fn capabilities_json() {
    Command::cargo_bin("mail-sweep")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"success\": true"))
        .stdout(predicate::str::contains("mail-sweep"));
}

#[test]
fn config_schema_includes_secrets() {
    Command::cargo_bin("mail-sweep")
        .unwrap()
        .args(["config", "schema", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("secrets.toml"))
        .stdout(predicate::str::contains("openrouter_api_key"));
}

#[test]
fn stats_json_empty_cache() {
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["stats", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\": 0"));
}

#[test]
fn list_json_empty() {
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"success\": true"));
}

#[test]
fn capabilities_includes_sync() {
    Command::cargo_bin("mail-sweep")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"sync\""));
}

#[test]
fn rules_list_empty() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[llm]\n").unwrap();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--config")
        .arg(&config)
        .args(["rules", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"success\": true"));
}

#[test]
fn rules_audit_json_empty() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        "[[rules]]\nmatch=\"from:test@example.com\"\naction=\"archive\"\n",
    )
    .unwrap();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--config")
        .arg(&config)
        .arg("--data-dir")
        .arg(dir.path())
        .args(["rules", "audit", "--json"])
        .assert()
        .failure();
}

#[test]
fn secrets_set_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[[accounts]]\nid=\"personal\"\nemail=\"a@b.com\"\nimap_host=\"imap.gmail.com\"\nsmtp_host=\"smtp.gmail.com\"\n").unwrap();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--config")
        .arg(&config)
        .args([
            "secrets",
            "set-openrouter-key",
            "--key",
            "sk-test",
        ])
        .assert()
        .success();

    Command::cargo_bin("mail-sweep")
        .unwrap()
        .arg("--config")
        .arg(&config)
        .args(["secrets", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"openrouter_key_set\": true"));
}
