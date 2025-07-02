use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_version_command() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("helium"))
        .stdout(predicate::str::contains("build:"));
}

#[test]
fn test_init_command_missing_chain_id() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "required arguments were not provided",
        ));
}

#[test]
fn test_init_command_with_chain_id() {
    let temp_dir = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("helium").unwrap();

    cmd.arg("init")
        .arg("--chain-id")
        .arg("test-chain-1")
        .arg("--home")
        .arg(temp_dir.path())
        .assert()
        .success();

    // Verify directories were created
    assert!(temp_dir.path().join("config").exists());
    assert!(temp_dir.path().join("data").exists());
    assert!(temp_dir.path().join("wasm_modules").exists());

    // Verify config files were created
    assert!(temp_dir.path().join("config").join("app.toml").exists());
    assert!(temp_dir.path().join("config").join("config.toml").exists());
    assert!(temp_dir
        .path()
        .join("config")
        .join("node_key.json")
        .exists());
}

#[test]
fn test_genesis_validate_missing_file() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("genesis")
        .arg("validate")
        .arg("non_existent_file.json")
        .assert()
        .failure();
}

#[test]
fn test_config_validate_missing_file() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("config")
        .arg("validate")
        .arg("non_existent_config.toml")
        .assert()
        .failure();
}

#[test]
fn test_config_show_with_home() {
    let temp_dir = TempDir::new().unwrap();

    // First initialize the node
    let mut init_cmd = Command::cargo_bin("helium").unwrap();
    init_cmd
        .arg("init")
        .arg("--chain-id")
        .arg("test-chain-1")
        .arg("--home")
        .arg(temp_dir.path())
        .assert()
        .success();

    // Then show the config
    let mut show_cmd = Command::cargo_bin("helium").unwrap();
    show_cmd
        .arg("config")
        .arg("show")
        .arg("--home")
        .arg(temp_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("chain_id = \"test-chain-1\""));
}

#[test]
fn test_keys_list_command() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("keys")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No keys found").or(predicate::str::contains("NAME")));
}

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Helium blockchain node"))
        .stdout(predicate::str::contains("Commands:"));
}

#[test]
fn test_subcommand_help() {
    let mut cmd = Command::cargo_bin("helium").unwrap();
    cmd.arg("init")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialize node configuration"));
}
