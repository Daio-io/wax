use clap::Parser;
use wax_cli::cli::Cli;
use wax_cli::presentation::{Presentation, TerminalContext, select_presentation};

#[test]
fn decoration_requires_interactive_streams_and_a_capable_non_ci_terminal() {
    let cli = Cli::try_parse_from(["wax", "scan"]).unwrap();
    let terminal = TerminalContext {
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
        ci_present: false,
        term_dumb: false,
    };
    assert_eq!(select_presentation(&cli, &terminal), Presentation::Human);
    for plain in [
        TerminalContext {
            stdin_tty: false,
            ..terminal
        },
        TerminalContext {
            stdout_tty: false,
            ..terminal
        },
        TerminalContext {
            stderr_tty: false,
            ..terminal
        },
        TerminalContext {
            ci_present: true,
            ..terminal
        },
        TerminalContext {
            term_dumb: true,
            ..terminal
        },
    ] {
        assert_eq!(select_presentation(&cli, &plain), Presentation::Plain);
    }
}

#[test]
fn machine_command_forms_stay_plain_even_in_a_terminal() {
    let terminal = TerminalContext {
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
        ..TerminalContext::default()
    };
    for args in [
        vec!["scan", "--compact"],
        vec!["scan", "--format", "quiet"],
        vec!["scan", "--format", "json-summary"],
        vec!["scan", "--format", "markdown"],
        vec!["search", "button", "--compact"],
        vec!["search", "button", "--format", "json"],
        vec!["component", "button", "--format", "json"],
        vec!["token", "space", "--format", "json"],
        vec!["init", "--non-interactive"],
        vec!["discover", "--language", "compose", "--dry-run"],
        vec!["registry", "discover", "--language", "compose", "--dry-run"],
        vec!["registry", "find", "--format", "json"],
    ] {
        let cli = Cli::try_parse_from(std::iter::once("wax").chain(args.iter().copied())).unwrap();
        assert_eq!(
            select_presentation(&cli, &terminal),
            Presentation::Plain,
            "{args:?}"
        );
    }
    for args in [
        vec!["scan"],
        vec!["search", "button"],
        vec!["component", "button"],
        vec!["token", "space"],
        vec!["init"],
        vec!["discover", "--language", "compose"],
        vec!["registry", "discover", "--language", "compose"],
        vec!["registry", "list"],
        vec!["registry", "find"],
        vec!["language", "list"],
        vec!["sync"],
        vec!["validate"],
        vec!["uninstall"],
    ] {
        let cli = Cli::try_parse_from(std::iter::once("wax").chain(args.iter().copied())).unwrap();
        assert_eq!(
            select_presentation(&cli, &terminal),
            Presentation::Human,
            "{args:?}"
        );
    }
}

#[test]
fn launch_is_the_exact_ascii_asset_and_propagates_write_errors() {
    use std::io::{self, Write};
    use wax_cli::presentation::write_launch;
    let mut bytes = Vec::new();
    write_launch(&mut bytes).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        concat!(
            "__      __  _   __  __\n",
            "\\ \\ /\\ / / /_\\  \\ \\/ /\n",
            " \\ V  V / / _ \\  >  <\n",
            "  \\_/\\_/ /_/ \\_\\/_/\\_\\\n",
            "Design-system analysis\n\n",
        )
    );
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        write_launch(&mut Broken).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}
