mod commands {
    pub mod init;
    pub mod language;
    pub mod scan;
    pub mod uninstall;
    pub mod validate;
}

#[cfg(test)]
mod testing;

use clap::{Args, Parser, Subcommand};
use commands::init::{InitOptions, run_init};
use commands::language::{
    DoctorOptions, InstallOptions, LanguageInstallSpec, ListOptions, UninstallOptions,
    UpdateOptions, run_doctor, run_install, run_list, run_uninstall, run_update,
};
use commands::scan::{ScanCommandOptions, run_scan};
use commands::uninstall::{UninstallCliOptions, run_uninstall_cli};
use commands::validate::{ValidateCommandOptions, run_validate};
use std::path::PathBuf;
use wax_contract::LanguageId;
use wax_lang_api::build_version;

#[derive(Debug, Parser)]
#[command(name = "wax")]
#[command(version = build_version(), about = "Design-system analysis engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Manage language pack lifecycle.
    Language(LanguageCli),
    /// Initialize wax repository configuration.
    Init(InitArgs),
    /// Scan repository source with enabled language packs.
    Scan(ScanArgs),
    /// Validate repository wax inputs for CI usage.
    Validate(ValidateArgs),
    /// Uninstall wax global state and local binary paths.
    Uninstall(GlobalUninstallArgs),
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Run scriptable onboarding without prompts.
    #[arg(long = "non-interactive")]
    non_interactive: bool,
    /// Language pack id to enable. Repeat for multiple languages.
    #[arg(long = "language", value_name = "ID")]
    languages: Vec<LanguageId>,
    /// Write config and lockfile without downloading language packs.
    #[arg(long)]
    no_install: bool,
    /// Pack index URL. Resolution precedence: --registry > WAX_LANG_INDEX > built-in default.
    #[arg(long, env = "WAX_LANG_INDEX")]
    registry: Option<String>,
    /// Repository root that will receive `.wax/wax.config.json` and `.wax/wax.lock.json`.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    target: Option<String>,
    /// Skip copying example design-system registry files.
    #[arg(long)]
    no_scaffold_registries: bool,
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
    /// Pack index URL. Resolution precedence: --registry > WAX_LANG_INDEX > built-in default.
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
    /// Pack index URL. Resolution precedence: --registry > WAX_LANG_INDEX > built-in default.
    #[arg(long, env = "WAX_LANG_INDEX")]
    registry: Option<String>,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    target: Option<String>,
    /// Repository root containing wax config and lock files.
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
    /// Repository root containing wax config and optionally lock files.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
}

#[derive(Debug, Args)]
struct ScanArgs {
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
    /// Disable automatic install of missing language packs before scan.
    #[arg(long)]
    no_auto_install: bool,
    /// Override scan worker concurrency.
    #[arg(long = "concurrency", value_parser = clap::value_parser!(u32).range(1..))]
    scan_concurrency: Option<u32>,
}

#[derive(Debug, Args)]
struct ValidateArgs {
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    repo_root: PathBuf,
}

#[derive(Debug, Args)]
struct GlobalUninstallArgs {
    /// Remove global state (`~/.wax`) and best-effort binary install paths.
    #[arg(long)]
    full: bool,
}

fn main() {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.command {
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
        }
        .map_err(Into::into),
        Commands::Init(args) => run_init(
            InitOptions {
                non_interactive: args.non_interactive,
                languages: args.languages,
                no_install: args.no_install,
                registry_url: args.registry,
                repo_root: args.repo_root,
                target_triple: args.target,
                state_path: None,
                scaffold_registries: !args.no_scaffold_registries,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Scan(args) => run_scan(
            ScanCommandOptions {
                repo_root: args.repo_root,
                allow_auto_install: !args.no_auto_install,
                scan_concurrency: args.scan_concurrency,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Validate(args) => run_validate(
            ValidateCommandOptions {
                repo_root: args.repo_root,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Uninstall(args) => {
            run_uninstall_cli(UninstallCliOptions { full: args.full }, &mut stdout)
                .map_err(Into::into)
        }
    };

    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
