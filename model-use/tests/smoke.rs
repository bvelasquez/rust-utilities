use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_works() {
    Command::cargo_bin("model-use")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("model-use"));
}

#[test]
fn capabilities_json() {
    Command::cargo_bin("model-use")
        .unwrap()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"model-use\""));
}

#[test]
fn env_schema_json() {
    Command::cargo_bin("model-use")
        .unwrap()
        .args(["env", "schema", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MODEL_USE_OPENROUTER_KEY"));
}

#[test]
fn providers_list_json() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    std::fs::write(&config, "").unwrap();
    Command::cargo_bin("model-use")
        .unwrap()
        .args([
            "--config",
            config.to_str().unwrap(),
            "providers",
            "list",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("openrouter"));
}
