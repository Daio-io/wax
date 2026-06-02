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

    let commands_section = stdout
        .split("Commands:")
        .nth(1)
        .unwrap_or("missing Commands section in help output");
    assert!(
        commands_section.contains("language"),
        "expected language subcommand in Commands section, got: {stdout}"
    );
    assert!(
        commands_section.contains("init"),
        "expected init subcommand in Commands section, got: {stdout}"
    );
    assert!(
        commands_section.contains("scan"),
        "expected scan subcommand in Commands section, got: {stdout}"
    );
    assert!(
        commands_section.contains("validate"),
        "expected validate subcommand in Commands section, got: {stdout}"
    );
}

#[test]
fn wax_binary_exposes_cli_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("--version")
        .output()
        .expect("failed to spawn wax binary");

    assert!(
        output.status.success(),
        "wax --version exited with {:?}; stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    let expected_version = option_env!("WAX_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    assert!(
        stdout.contains(expected_version),
        "expected wax version output to contain build version {expected_version}, got: {stdout}"
    );
}
