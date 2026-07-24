use std::error::Error;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn valid_configuration_exits_cleanly_without_output() -> Result<(), Box<dyn Error>> {
    let output = configured_command().output()?;

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    Ok(())
}

#[test]
fn wrong_environment_fails_before_startup() -> Result<(), Box<dyn Error>> {
    let output = configured_command()
        .env("IRONPILOT_ENVIRONMENT", "paper")
        .output()?;

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("configured environment development"));

    Ok(())
}

fn configured_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ironpilot"));
    command
        .env_clear()
        .env("IRONPILOT_CONFIG_PATH", example_config_path())
        .env("IRONPILOT_ENVIRONMENT", "development")
        .env(
            "IRONPILOT_ENVIRONMENT_FINGERPRINT",
            "development-paper-local",
        );
    command
}

fn example_config_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("config")
        .join("ironpilot.example.yaml")
}
