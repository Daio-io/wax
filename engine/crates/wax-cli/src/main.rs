use clap::{Parser, error::ErrorKind};
use wax_cli::cli::{
    Cli, Commands, LanguageSubcommand, LookupStdoutFormat, RegistrySubcommand, SearchEntryKind,
};
use wax_cli::commands::init::{InitOptions, run_init_cli};
use wax_cli::commands::language::{
    DoctorOptions, InstallOptions, ListOptions, UninstallOptions, UpdateOptions, run_doctor,
    run_install, run_list, run_uninstall, run_update,
};
use wax_cli::commands::registry::{
    RegistryAddCommandOptions, RegistryDiscoverCommandOptions, RegistryMemoryCommandOptions,
    RegistryUpdateCommandOptions, run_registry_add, run_registry_delete, run_registry_discover,
    run_registry_list, run_registry_refresh, run_registry_show, run_registry_update,
};
use wax_cli::commands::scan::parse_output_flag;
use wax_cli::commands::scan::{ScanCommandOptions, run_scan_cli};
use wax_cli::commands::search::{
    DetailCommandOptions, LookupCommandOptions, SearchCommandError, SearchCommandOptions,
    run_detail, run_search, write_lookup_error_json,
};
use wax_cli::commands::sync::{SyncCommandOptions, run_sync_cli};
use wax_cli::commands::uninstall::{UninstallCliOptions, run_uninstall_cli};
use wax_cli::commands::validate::{ValidateCommandOptions, run_validate};
use wax_contract::{RegistryEntryKind, RegistryLookupError, RegistryLookupErrorCode};

fn main() {
    let argv = std::env::args_os().collect::<Vec<_>>();
    let wants_json = args_request_json_format(&argv);
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if wants_json && is_lookup_command(&argv) && is_argument_failure(&error) {
                let lookup_error = RegistryLookupError::new(
                    RegistryLookupErrorCode::InvalidArgument,
                    error.to_string(),
                );
                let mut stdout = std::io::stdout().lock();
                if write_lookup_error_json(&lookup_error, &mut stdout).is_err() {
                    eprintln!("error: {error}");
                }
                std::process::exit(2);
            }
            error.exit();
        }
    };

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
            command: RegistrySubcommand::Add(args),
        }) => run_registry_add(
            RegistryAddCommandOptions {
                name: args.name.unwrap_or_else(|| args.design_system_id.clone()),
                design_system_id: args.design_system_id,
                source: args.source,
                language: args.language,
                tag: args.tag,
                state_path: None,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Registry(wax_cli::cli::RegistryCli {
            command: RegistrySubcommand::Refresh(args),
        }) => run_registry_refresh(
            &args.design_system_id,
            RegistryMemoryCommandOptions { state_path: None },
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
        Commands::Search(args) => run_search(
            SearchCommandOptions {
                lookup: lookup_options_from_common(args.common),
                query: args.query,
                kind: args.kind.map(entry_kind_from_cli),
                limit: args.limit,
                compact: args.compact,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Component(args) => run_detail(
            DetailCommandOptions {
                lookup: lookup_options_from_common(args.common),
                kind: RegistryEntryKind::Component,
                id: args.id,
                snapshot_digest: args.snapshot,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Token(args) => run_detail(
            DetailCommandOptions {
                lookup: lookup_options_from_common(args.common),
                kind: RegistryEntryKind::Token,
                id: args.id,
                snapshot_digest: args.snapshot,
            },
            &mut stdout,
        )
        .map_err(Into::into),
        Commands::Scan(args) => (|| {
            let cli_outputs = args
                .outputs
                .iter()
                .map(|raw| parse_output_flag(raw))
                .collect::<Result<Vec<_>, _>>()?;
            run_scan_cli(
                ScanCommandOptions {
                    repo_root: args.repo_root,
                    strict: args.strict,
                    allow_auto_install: !args.no_auto_install,
                    scan_concurrency: args.scan_concurrency,
                    root_group: args.root_group,
                    format: args.format,
                    compact: args.compact,
                    cli_outputs,
                    allow_absolute_output: args.allow_absolute_output,
                    baseline: args.baseline,
                    state_path: None,
                    pack_index_url: None,
                    target_triple: None,
                    ephemeral: None,
                },
                &mut stdout,
            )
        })()
        .map_err(Into::into),
        Commands::Sync(args) => run_sync_cli(
            SyncCommandOptions {
                repo_root: args.repo_root,
                state_path: None,
                upgrade: args.upgrade,
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
        if let Some(search_error) = err.downcast_ref::<SearchCommandError>() {
            let format = lookup_format_from_argv(&argv);
            if format == LookupStdoutFormat::Json
                && let Some(lookup_error) = search_error.to_lookup_error()
            {
                let mut stdout = std::io::stdout().lock();
                if write_lookup_error_json(&lookup_error, &mut stdout).is_ok() {
                    std::process::exit(1);
                }
            }
        }
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn lookup_options_from_common(common: wax_cli::cli::LookupCommonArgs) -> LookupCommandOptions {
    LookupCommandOptions {
        repo_root: common.repo_root,
        design_system: common.design_system,
        language: common.language,
        tag: common.tag,
        offline: common.offline,
        format: common.format,
        state_path: None,
    }
}

fn entry_kind_from_cli(kind: SearchEntryKind) -> RegistryEntryKind {
    match kind {
        SearchEntryKind::Component => RegistryEntryKind::Component,
        SearchEntryKind::Token => RegistryEntryKind::Token,
    }
}

fn args_request_json_format(argv: &[std::ffi::OsString]) -> bool {
    lookup_format_from_argv(argv) == LookupStdoutFormat::Json
}

fn is_lookup_command(argv: &[std::ffi::OsString]) -> bool {
    argv.get(1)
        .and_then(|arg| arg.to_str())
        .is_some_and(|command| matches!(command, "search" | "component" | "token"))
}

fn lookup_format_from_argv(argv: &[std::ffi::OsString]) -> LookupStdoutFormat {
    let mut format = LookupStdoutFormat::Text;
    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].to_string_lossy();
        if arg == "--format" {
            if let Some(value) = argv.get(index + 1) {
                if value == "json" {
                    format = LookupStdoutFormat::Json;
                } else if value == "text" {
                    format = LookupStdoutFormat::Text;
                }
                index += 2;
                continue;
            }
        } else if let Some(value) = arg.strip_prefix("--format=") {
            if value == "json" {
                format = LookupStdoutFormat::Json;
            } else if value == "text" {
                format = LookupStdoutFormat::Text;
            }
        }
        index += 1;
    }
    format
}

fn is_argument_failure(error: &clap::Error) -> bool {
    !matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    )
}
