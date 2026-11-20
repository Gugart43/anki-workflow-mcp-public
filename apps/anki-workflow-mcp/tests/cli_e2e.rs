//! End-to-end integration tests for `anki-workflow-mcp` CLI.

use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
}

#[test]
fn cli_help_exits_zero_and_prints_usage() {
    for flag in ["--help", "-h", "help"] {
        let out = bin().arg(flag).output().expect("spawn binary");
        assert!(out.status.success(), "{flag} should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Usage: anki-workflow-mcp"));
    }
}

#[test]
fn cli_health_returns_status_implementation_and_version() {
    let out = bin().arg("health").output().expect("spawn binary");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(json["status"], "ok");
    assert!(
        json["implementation"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "implementation field must be a non-empty string"
    );
    assert!(
        json["version"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "version field must be a non-empty string"
    );
}

#[test]
fn cli_rejects_extra_arguments_with_exit_code_2() {
    for args in [
        &["health", "extra"][..],
        &["version", "--extra"][..],
        &["--help", "extra"][..],
        &["serve", "extra"][..],
        &["build-info", "extra"][..],
    ] {
        let out = bin().args(args).output().expect("spawn binary");
        assert_eq!(out.status.code(), Some(2), "expected exit 2 for {args:?}");
    }
}

#[test]
fn cli_serve_boots_tokio_runtime_and_exits_cleanly_on_eof() {
    let out = bin()
        .arg("serve")
        .stdin(Stdio::null())
        .output()
        .expect("spawn serve");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_version_options_exit_zero_and_report_version() {
    for flag in ["version", "--version", "-V"] {
        let out = bin().arg(flag).output().expect("spawn binary");
        assert!(out.status.success(), "{flag} should exit 0");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("anki-workflow-mcp"),
            "expected implementation name in {flag} stdout"
        );
    }
}

#[test]
fn cli_build_info_options_exit_zero_and_output_json() {
    for flag in ["build-info", "--build-info"] {
        let out = bin().arg(flag).output().expect("spawn binary");
        assert!(out.status.success(), "{flag} should exit 0");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
        assert_eq!(json["implementation"], "anki-workflow-mcp");
    }
}

#[test]
fn cli_documented_options_listed_in_help() {
    let out = bin().arg("--help").output().expect("spawn binary");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for opt in [
        "version",
        "build-info",
        "health",
        "serve",
        "sync-media",
        "sync-decks",
    ] {
        assert!(stdout.contains(opt), "usage missing option: {opt}");
    }
}

#[test]
fn cli_unknown_subcommand_exits_2() {
    let out = bin()
        .arg("--nonexistent-command")
        .output()
        .expect("spawn binary");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Usage: anki-workflow-mcp"));
}
