//! `anki-workflow-mcp` CLI entrypoint.

use anki_workflow_mcp::{build_info, cli, mcp};
use std::process::ExitCode;

#[cfg(feature = "oracle")]
mod oracle;

#[derive(Debug, PartialEq, Eq)]
enum CliCommand {
    Version,
    BuildInfo,
    Health,
    Help,
    Serve,
    SyncMedia(Vec<String>),
    SyncDecks(Vec<String>),
    #[cfg(feature = "oracle")]
    Oracle,
}

fn print_usage(to_stdout: bool) {
    #[cfg(feature = "oracle")]
    let usage =
        "Usage: anki-workflow-mcp <version|build-info|health|serve|sync-media|sync-decks|oracle>";
    #[cfg(not(feature = "oracle"))]
    let usage = "Usage: anki-workflow-mcp <version|build-info|health|serve|sync-media|sync-decks>";

    if to_stdout {
        println!("{usage}");
    } else {
        eprintln!("{usage}");
    }
}

fn parse_cli_args(args: impl IntoIterator<Item = String>) -> Result<CliCommand, u8> {
    let mut args = args.into_iter();
    let Some(cmd) = args.next() else {
        print_usage(false);
        return Err(2);
    };

    match cmd.as_str() {
        "version" | "--version" | "-V" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::Version)
        }
        "build-info" | "--build-info" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::BuildInfo)
        }
        "health" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::Health)
        }
        "help" | "--help" | "-h" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::Help)
        }
        "serve" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::Serve)
        }
        "sync-media" => {
            let sub_args: Vec<String> = args.collect();
            Ok(CliCommand::SyncMedia(sub_args))
        }
        "sync-decks" => {
            let sub_args: Vec<String> = args.collect();
            Ok(CliCommand::SyncDecks(sub_args))
        }
        #[cfg(feature = "oracle")]
        "oracle" => {
            if args.next().is_some() {
                print_usage(false);
                return Err(2);
            }
            Ok(CliCommand::Oracle)
        }
        _ => {
            print_usage(false);
            Err(2)
        }
    }
}

fn main() -> ExitCode {
    let command = match parse_cli_args(std::env::args().skip(1)) {
        Ok(cmd) => cmd,
        Err(code) => return ExitCode::from(code),
    };

    match command {
        CliCommand::Version => {
            let info = build_info::BuildInfo::current();
            println!("{} {}", info.implementation, info.package_version);
            ExitCode::SUCCESS
        }
        CliCommand::BuildInfo => {
            let info = build_info::BuildInfo::current();
            match info.to_json() {
                Ok(json) => {
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("failed to serialize build-info: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        CliCommand::Health => {
            let info = build_info::BuildInfo::current();
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "implementation": info.implementation,
                    "version": info.package_version,
                })
            );
            ExitCode::SUCCESS
        }
        CliCommand::Help => {
            print_usage(true);
            ExitCode::SUCCESS
        }
        CliCommand::Serve => {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    eprintln!("failed to build Tokio runtime: {err}");
                    return ExitCode::FAILURE;
                }
            };
            match rt.block_on(mcp::run_stdio()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::FAILURE
                }
            }
        }
        CliCommand::SyncMedia(sub_args) => match cli::sync_media::run(&sub_args) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err((code, err)) => {
                eprintln!("{err}");
                ExitCode::from(code)
            }
        },
        CliCommand::SyncDecks(sub_args) => match cli::sync_decks::run(&sub_args) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err((code, err)) => {
                eprintln!("{err}");
                ExitCode::from(code)
            }
        },
        #[cfg(feature = "oracle")]
        CliCommand::Oracle => match oracle::run_oracle() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cli_args_valid() {
        assert_eq!(
            parse_cli_args(["version"].map(String::from)),
            Ok(CliCommand::Version)
        );
        assert_eq!(
            parse_cli_args(["--version"].map(String::from)),
            Ok(CliCommand::Version)
        );
        assert_eq!(
            parse_cli_args(["-V"].map(String::from)),
            Ok(CliCommand::Version)
        );
        assert_eq!(
            parse_cli_args(["build-info"].map(String::from)),
            Ok(CliCommand::BuildInfo)
        );
        assert_eq!(
            parse_cli_args(["--build-info"].map(String::from)),
            Ok(CliCommand::BuildInfo)
        );
        assert_eq!(
            parse_cli_args(["health"].map(String::from)),
            Ok(CliCommand::Health)
        );
        assert_eq!(
            parse_cli_args(["help"].map(String::from)),
            Ok(CliCommand::Help)
        );
        assert_eq!(
            parse_cli_args(["--help"].map(String::from)),
            Ok(CliCommand::Help)
        );
        assert_eq!(
            parse_cli_args(["-h"].map(String::from)),
            Ok(CliCommand::Help)
        );
        assert_eq!(
            parse_cli_args(["serve"].map(String::from)),
            Ok(CliCommand::Serve)
        );
        assert_eq!(
            parse_cli_args(["sync-media", "--catalog", "cat.json"].map(String::from)),
            Ok(CliCommand::SyncMedia(vec![
                "--catalog".to_string(),
                "cat.json".to_string()
            ]))
        );
        assert_eq!(
            parse_cli_args(["sync-decks", "--collection", "col.anki2"].map(String::from)),
            Ok(CliCommand::SyncDecks(vec![
                "--collection".to_string(),
                "col.anki2".to_string()
            ]))
        );
    }

    #[test]
    fn test_parse_cli_args_rejects_extras() {
        assert_eq!(
            parse_cli_args(["version", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(
            parse_cli_args(["--version", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(parse_cli_args(["-V", "extra"].map(String::from)), Err(2));
        assert_eq!(
            parse_cli_args(["build-info", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(
            parse_cli_args(["--build-info", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(
            parse_cli_args(["health", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(parse_cli_args(["help", "extra"].map(String::from)), Err(2));
        assert_eq!(
            parse_cli_args(["--help", "extra"].map(String::from)),
            Err(2)
        );
        assert_eq!(parse_cli_args(["-h", "extra"].map(String::from)), Err(2));
        assert_eq!(parse_cli_args(["serve", "extra"].map(String::from)), Err(2));
    }

    #[test]
    fn test_parse_cli_args_invalid_command() {
        assert_eq!(
            parse_cli_args(["--unknown-command"].map(String::from)),
            Err(2)
        );
        assert_eq!(parse_cli_args(Vec::<String>::new()), Err(2));
    }
}
