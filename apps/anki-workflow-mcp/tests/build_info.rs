use serde_json::Value;
use std::borrow::Cow;
use std::process::Command;

#[allow(dead_code)]
#[path = "../src/build_info.rs"]
mod build_info;

#[test]
fn test_cli_version_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("--version")
        .output()
        .expect("failed to execute anki-workflow-mcp --version");

    assert!(output.status.success(), "--version must exit with 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        format!("anki-workflow-mcp {}", env!("CARGO_PKG_VERSION")),
        "--version stdout must match 'anki-workflow-mcp <CARGO_PKG_VERSION>'"
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on --version"
    );
}

#[test]
fn test_cli_version_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("version")
        .output()
        .expect("failed to execute anki-workflow-mcp version");

    assert!(
        output.status.success(),
        "version subcommand must exit with 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        format!("anki-workflow-mcp {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn test_cli_short_version_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("-V")
        .output()
        .expect("failed to execute anki-workflow-mcp -V");

    assert!(output.status.success(), "-V flag must exit with 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        format!("anki-workflow-mcp {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn test_cli_build_info_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("build-info")
        .output()
        .expect("failed to execute anki-workflow-mcp build-info");

    assert!(output.status.success(), "build-info must exit with 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value =
        serde_json::from_str(stdout.trim()).expect("build-info must output valid JSON");

    assert_eq!(json["implementation"], "anki-workflow-mcp");
    assert_eq!(json["package_version"], env!("CARGO_PKG_VERSION"));
    assert!(json["target"].is_string());
    assert!(!json["target"].as_str().unwrap().is_empty());

    // git_revision must be string (hex SHA) or null
    if json["git_revision"].is_null() {
        // Fallback without git
    } else {
        let rev = json["git_revision"]
            .as_str()
            .expect("git_revision must be string or null");
        assert!(
            rev.len() == 40 || rev.len() == 64,
            "git_revision must be hex sha"
        );
        assert!(rev.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // git_dirty must be boolean or null
    assert!(json["git_dirty"].is_boolean() || json["git_dirty"].is_null());

    // Strict schema check: exactly 5 known fields, no extra fields
    let obj = json.as_object().expect("build-info must be a JSON object");
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "git_dirty",
            "git_revision",
            "implementation",
            "package_version",
            "target"
        ],
        "build-info schema must strictly contain only declarative metadata"
    );

    // Verify absence of personal paths or timestamps
    let serialized = stdout.trim();
    assert!(
        !serialized.contains("C:"),
        "must not expose host drive paths"
    );
    assert!(
        !serialized.contains("/home/"),
        "must not expose host home paths"
    );
    assert!(
        !serialized.contains("/Users/"),
        "must not expose user paths"
    );
}

#[test]
fn test_cli_build_info_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("--build-info")
        .output()
        .expect("failed to execute anki-workflow-mcp --build-info");

    assert!(output.status.success(), "--build-info must exit with 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value =
        serde_json::from_str(stdout.trim()).expect("--build-info must output valid JSON");
    assert_eq!(json["implementation"], "anki-workflow-mcp");
    assert_eq!(json["package_version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn test_cli_invalid_arg_exits_2() {
    let output = Command::new(env!("CARGO_BIN_EXE_anki-workflow-mcp"))
        .arg("--invalid-flag-1234")
        .output()
        .expect("failed to execute anki-workflow-mcp");

    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid arg must exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage: anki-workflow-mcp"),
        "stderr must contain usage"
    );
}

#[test]
fn test_pure_fallback_resolution() {
    // 1. Missing git metadata: explicit nulls, never fabricated
    let info = build_info::resolve_build_info(
        Cow::Borrowed("anki-workflow-mcp"),
        Cow::Borrowed("0.1.0"),
        None,
        None,
        Cow::Borrowed("x86_64-pc-windows-msvc"),
    );
    assert_eq!(info.git_revision, None);
    assert_eq!(info.git_dirty, None);
    let json_str = info.to_json().unwrap();
    let json: Value = serde_json::from_str(&json_str).unwrap();
    assert!(json["git_revision"].is_null());
    assert!(json["git_dirty"].is_null());

    // 2. Known git metadata
    let info_known = build_info::resolve_build_info(
        Cow::Borrowed("anki-workflow-mcp"),
        Cow::Borrowed("0.1.0"),
        Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef"),
        Some("true"),
        Cow::Borrowed("x86_64-pc-windows-msvc"),
    );
    assert_eq!(
        info_known.git_revision.as_deref(),
        Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef")
    );
    assert_eq!(info_known.git_dirty, Some(true));
}
