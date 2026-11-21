//! Terminal eligibility and cosmetic human output.

use crate::cli::{Cli, Commands, LookupStdoutFormat, RegistrySubcommand, ScanStdoutFormat};
use std::io::{self, IsTerminal};

/// Output decoration selected once at command entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Presentation {
    /// Preserve lean, scriptable output.
    #[default]
    Plain,
    /// Show human terminal decoration.
    Human,
}

/// Captured terminal facts; callers may supply values without terminal access.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalContext {
    /// Standard input is a terminal.
    pub stdin_tty: bool,
    /// Standard output is a terminal.
    pub stdout_tty: bool,
    /// Standard error is a terminal.
    pub stderr_tty: bool,
    /// CI exists in the environment, even with an empty value.
    pub ci_present: bool,
    /// TERM is exactly `dumb`.
    pub term_dumb: bool,
}

impl TerminalContext {
    /// Captures stream and environment eligibility once.
    pub fn detect() -> Self {
        Self {
            stdin_tty: io::stdin().is_terminal(),
            stdout_tty: io::stdout().is_terminal(),
            stderr_tty: io::stderr().is_terminal(),
            ci_present: std::env::var_os("CI").is_some(),
            term_dumb: std::env::var_os("TERM").is_some_and(|term| term == "dumb"),
        }
    }

    /// Whether all streams and environment support human decoration.
    pub fn is_human(&self) -> bool {
        self.stdin_tty && self.stdout_tty && self.stderr_tty && !self.ci_present && !self.term_dumb
    }
}

/// Selects decoration for successfully parsed command arguments.
pub fn select_presentation(cli: &Cli, terminal: &TerminalContext) -> Presentation {
    if !terminal.is_human() {
        return Presentation::Plain;
    }
    let human = match &cli.command {
        Commands::Scan(args) => !args.compact && args.format == ScanStdoutFormat::Summary,
        Commands::Search(args) => !args.compact && args.common.format == LookupStdoutFormat::Text,
        Commands::Component(args) | Commands::Token(args) => {
            args.common.format == LookupStdoutFormat::Text
        }
        Commands::Init(args) => !args.non_interactive,
        Commands::Discover(args) => !args.dry_run,
        Commands::Registry(args) => match &args.command {
            RegistrySubcommand::Discover(args) => !args.dry_run,
            RegistrySubcommand::Find(args) => args.format == LookupStdoutFormat::Text,
            _ => true,
        },
        _ => true,
    };
    if human {
        Presentation::Human
    } else {
        Presentation::Plain
    }
}

const LAUNCH: &str = r"__      __  _   __  __
\ \ /\ / / /_\  \ \/ /
 \ V  V / / _ \  >  <
  \_/\_/ /_/ \_\/_/\_\
Design-system analysis

";

/// Writes the fixed ASCII launch asset with one separating blank line.
///
/// # Errors
///
/// Returns the writer's I/O error.
pub fn write_launch(writer: &mut impl io::Write) -> io::Result<()> {
    writer.write_all(LAUNCH.as_bytes())
}

/// Writes an ASCII section title and matching underline without truncation.
///
/// # Errors
///
/// Returns the writer's I/O error.
pub fn write_scan_heading(writer: &mut impl io::Write, title: &str) -> io::Result<()> {
    let underline = if title == "Scan results" { '=' } else { '-' };
    writeln!(writer, "{title}")?;
    writeln!(writer, "{}", underline.to_string().repeat(title.len()))
}
