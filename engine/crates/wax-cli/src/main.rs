mod commands {
    pub mod init;
    pub mod language;
}

use clap::{Args, Parser, Subcommand};
use commands::language::{
    DoctorOptions, InstallOptions, LanguageInstallSpec, ListOptions, UninstallOptions,
    UpdateOptions, run_doctor, run_install, run_list, run_uninstall, run_update,
};
use std::path::PathBuf;
use wax_contract::LanguageId;

#[derive(Debug, Parser)]
#[command(name = "wax")]
#[command(about = "Design-system analysis engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Manage language pack lifecycle.
    Language(LanguageCli),
    /// Initialize wax repository configuration.
    Init,
}

#[derive(Debug, Args)]
struct LanguageCli {
    #[command(subcommand)]
    command: LanguageSubcommand,
}

#[derive(Debug, Subcommand)]
enum LanguageSubcommand {
    /// List installed language packs.
    List(RegistryArgs),
    /// Install the latest registry version of a language pack.
    Install(InstallArgs),
    /// Uninstall one language pack version, or all versions when omitted.
    Uninstall(UninstallArgs),
    /// Install the latest registry version and remove older local versions.
    Update(UpdateArgs),
    /// Check repository language configuration, lock pins, and installed binaries.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
struct RegistryArgs {
    /// Deprecated compatibility flag; installed-state listing ignores registry indexes.
    #[arg(long, env = "WAX_LANG_INDEX")]
    registry: Option<String>,
}

#[derive(Debug, Args)]
struct InstallArgs {
    /// Language id to install, optionally pinned as <id>@<version>.
    language: LanguageInstallSpec,
    /// Pack index URL. Defaults to WAX_LANG_INDEX when unset.
    #[arg(long, env = "WAX_LANG_INDEX")]
    registry: Option<String>,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    target: Option<String>,
}

#[derive(Debug, Args)]
struct UpdateArgs {
    /// Language id to update. Omit only when using --all.
    #[arg(required_unless_present = "all")]
    language_id: Option<LanguageId>,
    /// Update every installed language.
    #[arg(long, conflicts_with = "language_id")]
    all: bool,
    /// Pack index URL. Defaults to WAX_LANG_INDEX when unset.
    #[arg(long, env = "WAX_LANG_INDEX")]
    registry: Option<String>,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    target: Option<String>,
    /// Repository root containing wax.lock.json.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
}

#[derive(Debug, Args)]
struct UninstallArgs {
    /// Language id to uninstall.
    language_id: LanguageId,
    /// Specific version to uninstall. If omitted, all installed versions are removed.
    #[arg(long)]
    version: Option<String>,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Repository root containing .waxrc and optionally wax.lock.json.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    let result = match cli.command {
        Commands::Language(language) => match language.command {
            LanguageSubcommand::List(args) => run_list(
                ListOptions {
                    registry_url: args.registry,
                    state_path: None,
                },
                &mut stdout,
            ),
            LanguageSubcommand::Install(args) => run_install(
                InstallOptions {
                    language_id: args.language.language_id,
                    version: args.language.version,
                    registry_url: args.registry,
                    target_triple: args.target,
                    state_path: None,
                },
                &mut stdout,
            ),
            LanguageSubcommand::Uninstall(args) => run_uninstall(
                UninstallOptions {
                    language_id: args.language_id,
                    version: args.version,
                    state_path: None,
                },
                &mut stdout,
            ),
            LanguageSubcommand::Update(args) => run_update(
                UpdateOptions {
                    language_id: args.language_id,
                    all: args.all,
                    registry_url: args.registry,
                    target_triple: args.target,
                    state_path: None,
                    repo_root: args.repo_root,
                },
                &mut stdout,
            ),
            LanguageSubcommand::Doctor(args) => run_doctor(
                DoctorOptions {
                    repo_root: args.repo_root,
                    state_path: None,
                },
                &mut stdout,
            ),
        },
        Commands::Init => {
            eprintln!("wax init is implemented in Task 10");
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
