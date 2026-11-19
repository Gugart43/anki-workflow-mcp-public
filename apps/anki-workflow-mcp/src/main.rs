//! `anki-workflow-mcp` CLI entrypoint.

mod build_info;
mod mcp;
#[cfg(feature = "oracle")]
mod oracle;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("version") | Some("--version") | Some("-V") => {
            let info = build_info::BuildInfo::current();
            println!("{} {}", info.implementation, info.package_version);
            ExitCode::SUCCESS
        }
        Some("build-info") | Some("--build-info") => {
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
        Some("health") => {
            let info = build_info::BuildInfo::current();
            println!(
                "{}",
                serde_json::json!({
                    "status": "ok",
                    "version": info.package_version,
                })
            );
            ExitCode::SUCCESS
        }
        Some("serve") => match mcp::run_stdio().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        #[cfg(feature = "oracle")]
        Some("oracle") => match oracle::run_oracle() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        _ => {
            #[cfg(feature = "oracle")]
            eprintln!("Usage: anki-workflow-mcp <version|build-info|health|oracle|serve>");
            #[cfg(not(feature = "oracle"))]
            eprintln!("Usage: anki-workflow-mcp <version|build-info|health|serve>");
            ExitCode::from(2)
        }
    }
}
