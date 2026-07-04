//! Clap argument definitions shared by the binary and unit tests.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use wax_contract::LanguageId;
use wax_lang_api::build_version;

use crate::commands::language::LanguageInstallSpec;

/// Root wax CLI parser.
#[derive(Debug, Parser)]
#[command(name = "wax")]
#[command(version = build_version(), about = "Design-system analysis engine")]
pub struct Cli {
    #[command(subcommand)]
    /// Selected command.
    pub command: Commands,
}

/// Top-level wax commands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Manage language pack lifecycle.
    Language(LanguageCli),
    /// Initialize wax repository configuration.
    Init(InitArgs),
    /// Discover and manage design-system registries.
    Registry(RegistryCli),
    /// Scan repository source with enabled language packs.
    Scan(ScanArgs),
    /// Refresh app registry inputs from remembered design systems.
    Sync(SyncArgs),
    /// Discover design-system registry components from source roots.
    Discover(DiscoverArgs),
    /// Validate repository wax inputs for CI usage.
    Validate(ValidateArgs),
    /// Uninstall wax global state and local binary paths.
    Uninstall(GlobalUninstallArgs),
}

/// Arguments for `wax init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Run scriptable onboarding without prompts.
    #[arg(long = "non-interactive")]
    pub non_interactive: bool,
    /// Language pack id to enable. Repeat for multiple languages.
    #[arg(long = "language", value_name = "ID")]
    pub languages: Vec<LanguageId>,
    /// Write config and lockfile without downloading language packs.
    #[arg(long)]
    pub no_install: bool,
    /// Pack index URL. Resolution precedence: --pack-index > WAX_PACK_INDEX > built-in default.
    #[arg(long = "pack-index", env = "WAX_PACK_INDEX")]
    pub pack_index: Option<String>,
    /// Repository root that will receive `.wax/wax.config.json` and `.wax/wax.lock.json`.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    pub target: Option<String>,
    /// Skip copying example design-system registry files.
    #[arg(long)]
    pub no_scaffold_registries: bool,
}

/// Arguments for `wax registry`.
#[derive(Debug, Args)]
pub struct RegistryCli {
    #[command(subcommand)]
    /// Registry subcommand.
    pub command: RegistrySubcommand,
}

/// Registry subcommands.
#[derive(Debug, Subcommand)]
pub enum RegistrySubcommand {
    /// Discover design-system registry components from source roots.
    Discover(DiscoverArgs),
    /// List remembered design systems.
    List(RegistryMemoryArgs),
    /// Show one remembered design system.
    Show(RegistryShowArgs),
    /// Update the remembered repository root for a design system.
    Update(RegistryUpdateArgs),
    /// Delete a remembered design system.
    Delete(RegistryDeleteArgs),
}

/// Arguments for `wax registry list`.
#[derive(Debug, Args)]
pub struct RegistryMemoryArgs {}

/// Arguments for `wax registry show`.
#[derive(Debug, Args)]
pub struct RegistryShowArgs {
    /// Design-system id to show.
    pub design_system_id: String,
}

/// Arguments for `wax registry update`.
#[derive(Debug, Args)]
pub struct RegistryUpdateArgs {
    /// Design-system id to update.
    pub design_system_id: String,
    /// New repository root for the remembered design system.
    #[arg(long)]
    pub repo_root: PathBuf,
}

/// Arguments for `wax registry delete`.
#[derive(Debug, Args)]
pub struct RegistryDeleteArgs {
    /// Design-system id to delete.
    pub design_system_id: String,
}

/// Arguments for registry discovery.
#[derive(Debug, Args)]
pub struct DiscoverArgs {
    /// Language pack id to discover registry components for.
    #[arg(long = "language", value_name = "ID")]
    pub language: LanguageId,
    /// Source root to inspect. Repeat for multiple roots.
    #[arg(long = "root", value_name = "PATH")]
    pub roots: Vec<PathBuf>,
    /// Print generated registry JSON to stdout without writing a file.
    #[arg(long)]
    pub dry_run: bool,
    /// Replace an existing registry file.
    #[arg(long)]
    pub force: bool,
    /// Design-system id to remember after discovery.
    #[arg(long = "design-system", value_name = "ID")]
    pub design_system: Option<String>,
    /// Display name for the remembered design system.
    #[arg(long)]
    pub name: Option<String>,
    /// Repository root where the registry should be written.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
}

/// Arguments for `wax language`.
#[derive(Debug, Args)]
pub struct LanguageCli {
    #[command(subcommand)]
    /// Language subcommand.
    pub command: LanguageSubcommand,
}

/// Language subcommands.
#[derive(Debug, Subcommand)]
pub enum LanguageSubcommand {
    /// List installed language packs.
    List(PackIndexArgs),
    /// Install the latest pack index version of a language pack.
    Install(InstallArgs),
    /// Uninstall one language pack version, or all versions when omitted.
    Uninstall(UninstallArgs),
    /// Install the latest pack index version and remove older local versions.
    Update(UpdateArgs),
    /// Check repository language configuration, lock pins, and installed binaries.
    Doctor(DoctorArgs),
}

/// Shared pack index flag for language list.
#[derive(Debug, Args)]
pub struct PackIndexArgs {
    /// Deprecated compatibility flag; installed-state listing ignores pack indexes.
    #[arg(long = "pack-index", env = "WAX_PACK_INDEX")]
    pub pack_index: Option<String>,
}

/// Arguments for `wax language install`.
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Language id to install, optionally pinned as <id>@<version>.
    pub language: LanguageInstallSpec,
    /// Pack index URL. Resolution precedence: --pack-index > WAX_PACK_INDEX > built-in default.
    #[arg(long = "pack-index", env = "WAX_PACK_INDEX")]
    pub pack_index: Option<String>,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    pub target: Option<String>,
}

/// Arguments for `wax language update`.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Language id to update. Omit only when using --all.
    #[arg(required_unless_present = "all")]
    pub language_id: Option<LanguageId>,
    /// Update every installed language.
    #[arg(long, conflicts_with = "language_id")]
    pub all: bool,
    /// Pack index URL. Resolution precedence: --pack-index > WAX_PACK_INDEX > built-in default.
    #[arg(long = "pack-index", env = "WAX_PACK_INDEX")]
    pub pack_index: Option<String>,
    /// Target triple override, primarily for tests and cross-install workflows.
    #[arg(long)]
    pub target: Option<String>,
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
}

/// Arguments for `wax language uninstall`.
#[derive(Debug, Args)]
pub struct UninstallArgs {
    /// Language id to uninstall.
    pub language_id: LanguageId,
    /// Specific version to uninstall. If omitted, all installed versions are removed.
    #[arg(long)]
    pub version: Option<String>,
}

/// Arguments for `wax language doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Repository root containing wax config and optionally lock files.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
}

/// Arguments for `wax scan`.
#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
    /// Disable automatic install of missing language packs before scan.
    #[arg(long)]
    pub no_auto_install: bool,
    /// Override scan worker concurrency.
    #[arg(long = "concurrency", value_parser = clap::value_parser!(u32).range(1..))]
    pub scan_concurrency: Option<u32>,
}

/// Arguments for `wax sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
}

/// Arguments for `wax validate`.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Repository root containing wax config and lock files.
    #[arg(long, default_value = ".")]
    pub repo_root: PathBuf,
}

/// Arguments for `wax uninstall`.
#[derive(Debug, Args)]
pub struct GlobalUninstallArgs {
    /// Remove global state (`~/.wax`) and best-effort binary install paths.
    #[arg(long)]
    pub full: bool,
}
