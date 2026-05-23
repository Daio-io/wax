use std::process::Command;

#[test]
fn wax_binary_exposes_cli_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("--help")
        .output()
        .expect("failed to spawn wax binary");

    assert!(
        output.status.success(),
        "wax --help exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    assert!(
        stdout.contains("Design-system analysis engine"),
        "expected wax help output, got: {stdout}"
    );
    assert!(
        stdout.contains("language"),
        "expected language subcommand in help output, got: {stdout}"
    );
    assert!(
        stdout.contains("init"),
        "expected init subcommand in help output, got: {stdout}"
    );
}
