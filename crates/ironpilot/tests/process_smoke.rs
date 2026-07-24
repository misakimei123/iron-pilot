use std::error::Error;
use std::process::Command;

#[test]
fn empty_application_exits_cleanly_without_output() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_ironpilot")).output()?;

    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    Ok(())
}
