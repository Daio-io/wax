use clap::Parser;
use wax_cli::cli::{Cli, Commands, LanguageSubcommand, RegistrySubcommand};
use wax_cli::commands::init::{InitOptions, run_init_cli};
use wax_cli::commands::language::{
    DoctorOptions, InstallOptions, ListOptions, UninstallOptions, UpdateOptions, run_doctor,
    run_install, run_list, run_uninstall, run_update,
};
use wax_cli::commands::registry::{
    RegistryDiscoverCommandOptions, RegistryMemoryCommandOptions, RegistryUpdateCommandOptions,
    run_registry_delete, run_registry_discover, run_registry_list, run_registry_show,
    run_registry_update,
};
use wax_cli::commands::scan::{ScanCommandOptions, run_scan_cli};
use wax_cli::commands::uninstall::{UninstallCliOptions, run_uninstall_cli};
use wax_cli::commands::validate::{ValidateCommandOptions, run_validate};

fn main() {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.command {
        Commands::Language(language) => match language.command {
            LanguageSubcommand::List(args) => run_list(
                ListOptions {
                    registry_url: args.pack_index,
                    state_path: None,
                },
                &mut stdout,
            ),
            LanguageSubcommand::Install(args) => run_install(
                InstallOptions {
                    language_id: args.language.language_id,
                    version: args.language.version,
                    registry_url: args.pack_index,
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
                    registry_url: args.pack_index,
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
        Commands::Init(args) => run_init_cli(
            InitOptions {
                non_interactive: args.non_interactive,
                languages: args.languages,
                no_install: args.no_install,
                registry_url: args.pack_index,
                repo_root: args.repo_root,
                target_triple: args.target,
                state_path: None,
                scaffold_registries: !args.no_scaffold_registries,
                interactive: None,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Discover(args)
        | Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::Discover(args),
        }) => run_registry_discover(
            RegistryDiscoverCommandOptions {
                repo_root: args.repo_root,
                language_id: args.language.as_str().to_owned(),
                roots: args.roots,
                dry_run: args.dry_run,
                force: args.force,
                design_system_id: args.design_system,
                design_system_name: args.name,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::List(_),
        }) => run_registry_list(
            RegistryMemoryCommandOptions { state_path: None },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::Show(args),
        }) => run_registry_show(
            &args.design_system_id,
            RegistryMemoryCommandOptions { state_path: None },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::Update(args),
        }) => run_registry_update(
            RegistryUpdateCommandOptions {
                design_system_id: args.design_system_id,
                repo_root: args.repo_root,
                state_path: None,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::Delete(args),
        }) => run_registry_delete(
            &args.design_system_id,
            RegistryMemoryCommandOptions { state_path: None },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Scan(args) => run_scan_cli(
            ScanCommandOptions {
                repo_root: args.repo_root,
                allow_auto_install: !args.no_auto_install,
                scan_concurrency: args.scan_concurrency,
                state_path: None,
                pack_index_url: None,
                target_triple: None,
                ephemeral: None,
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
