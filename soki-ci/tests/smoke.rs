use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_works() {
    Command::cargo_bin("soki-ci")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("soki-ci"));
}

#[test]
fn capabilities_json() {
    Command::cargo_bin("soki-ci")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"soki-ci\""));
}

#[test]
fn env_schema_json() {
    Command::cargo_bin("soki-ci")
        .unwrap()
        .args(["env", "schema", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SOKI_CI_CONFIG"));
}

#[test]
fn config_init_and_validate() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("projects.yaml");
    Command::cargo_bin("soki-ci")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "config",
            "init",
            "--json",
        ])
        .assert()
        .success();
    Command::cargo_bin("soki-ci")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "config",
            "validate",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("issue_count"));
}
