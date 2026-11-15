//! Runtime identity and build metadata.
//!
//! Provides declarative, unauthenticated build identification for `anki-workflow-mcp`.
//! Contains no timestamps or host filesystem paths.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Runtime build information.
///
/// All metadata is purely declarative and does not constitute a cryptographic release signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Implementation identifier matching MCP serverInfo name.
    pub implementation: Cow<'static, str>,
    /// Package version matching Cargo package version and MCP serverInfo version.
    pub package_version: Cow<'static, str>,
    /// Git revision commit SHA if built from a verifiable git worktree, or None if unknown.
    pub git_revision: Option<Cow<'static, str>>,
    /// Whether local uncommitted changes were detected at build time, or None if unknown.
    pub git_dirty: Option<bool>,
    /// Compilation target architecture and OS triple.
    pub target: Cow<'static, str>,
}

impl BuildInfo {
    /// Return the runtime identity of the current build.
    pub fn current() -> Self {
        let raw_rev = option_env!("ANKI_BUILD_GIT_REVISION");
        let raw_dirty = option_env!("ANKI_BUILD_GIT_DIRTY");
        let target = option_env!("ANKI_BUILD_TARGET").unwrap_or("unknown");

        resolve_build_info(
            Cow::Borrowed(env!("CARGO_PKG_NAME")),
            Cow::Borrowed(env!("CARGO_PKG_VERSION")),
            raw_rev,
            raw_dirty,
            Cow::Borrowed(target),
        )
    }

    /// Serialize build info to compact JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Parse raw git revision string into an optional sanitized SHA.
pub fn parse_git_revision(raw: Option<&str>) -> Option<Cow<'static, str>> {
    match raw {
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
                None
            } else {
                Some(Cow::Owned(trimmed.to_owned()))
            }
        }
        None => None,
    }
}

/// Parse raw git dirty string ("true"/"false") into Option<bool>.
pub fn parse_git_dirty(raw: Option<&str>) -> Option<bool> {
    match raw.map(|s| s.trim()) {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

/// Pure helper to resolve BuildInfo from raw environment inputs.
pub fn resolve_build_info(
    implementation: Cow<'static, str>,
    package_version: Cow<'static, str>,
    raw_git_revision: Option<&str>,
    raw_git_dirty: Option<&str>,
    target: Cow<'static, str>,
) -> BuildInfo {
    BuildInfo {
        implementation,
        package_version,
        git_revision: parse_git_revision(raw_git_revision),
        git_dirty: parse_git_dirty(raw_git_dirty),
        target,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_revision() {
        assert_eq!(
            parse_git_revision(Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef")).as_deref(),
            Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef")
        );
        assert_eq!(
            parse_git_revision(Some("  abcdef1234567890abcdef1234567890abcdef12  \n")).as_deref(),
            Some("abcdef1234567890abcdef1234567890abcdef12")
        );
        assert_eq!(parse_git_revision(Some("")), None);
        assert_eq!(parse_git_revision(Some("   ")), None);
        assert_eq!(parse_git_revision(Some("unknown")), None);
        assert_eq!(parse_git_revision(Some("UNKNOWN")), None);
        assert_eq!(parse_git_revision(None), None);
    }

    #[test]
    fn test_parse_git_dirty() {
        assert_eq!(parse_git_dirty(Some("true")), Some(true));
        assert_eq!(parse_git_dirty(Some("false")), Some(false));
        assert_eq!(parse_git_dirty(Some("  true  ")), Some(true));
        assert_eq!(parse_git_dirty(Some("  false  ")), Some(false));
        assert_eq!(parse_git_dirty(Some("unknown")), None);
        assert_eq!(parse_git_dirty(Some("")), None);
        assert_eq!(parse_git_dirty(None), None);
    }

    #[test]
    fn test_resolve_build_info_clean() {
        let info = resolve_build_info(
            Cow::Borrowed("anki-workflow-mcp"),
            Cow::Borrowed("0.1.0"),
            Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef"),
            Some("false"),
            Cow::Borrowed("x86_64-pc-windows-msvc"),
        );
        assert_eq!(info.implementation, "anki-workflow-mcp");
        assert_eq!(info.package_version, "0.1.0");
        assert_eq!(
            info.git_revision.as_deref(),
            Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef")
        );
        assert_eq!(info.git_dirty, Some(false));
        assert_eq!(info.target, "x86_64-pc-windows-msvc");

        let json = info.to_json().expect("to_json succeeds");
        assert!(json.contains(r#""implementation":"anki-workflow-mcp""#));
        assert!(json.contains(r#""package_version":"0.1.0""#));
        assert!(json.contains(r#""git_revision":"73e3fd03d2d5f3285f9c7090d8e185c703e979ef""#));
        assert!(json.contains(r#""git_dirty":false"#));
        assert!(json.contains(r#""target":"x86_64-pc-windows-msvc""#));
    }

    #[test]
    fn test_resolve_build_info_fallback_unknown() {
        let info = resolve_build_info(
            Cow::Borrowed("anki-workflow-mcp"),
            Cow::Borrowed("0.1.0"),
            None,
            None,
            Cow::Borrowed("x86_64-unknown-linux-gnu"),
        );
        assert_eq!(info.implementation, "anki-workflow-mcp");
        assert_eq!(info.package_version, "0.1.0");
        assert_eq!(info.git_revision, None);
        assert_eq!(info.git_dirty, None);
        assert_eq!(info.target, "x86_64-unknown-linux-gnu");

        let json = info.to_json().expect("to_json succeeds");
        // Verify explicit nulls for missing provenance without fabricating SHA or clean state
        assert!(json.contains(r#""git_revision":null"#));
        assert!(json.contains(r#""git_dirty":null"#));
    }

    #[test]
    fn test_no_timestamps_or_personal_paths() {
        let info = resolve_build_info(
            Cow::Borrowed("anki-workflow-mcp"),
            Cow::Borrowed("0.1.0"),
            Some("73e3fd03d2d5f3285f9c7090d8e185c703e979ef"),
            Some("true"),
            Cow::Borrowed("x86_64-pc-windows-msvc"),
        );
        let json = info.to_json().expect("to_json succeeds");
        assert!(!json.contains("C:"));
        assert!(!json.contains("/home/"));
        assert!(!json.contains("/Users/"));
        assert!(!json.contains('\\'));
    }

    #[test]
    fn test_json_roundtrip() {
        let original = resolve_build_info(
            Cow::Borrowed("anki-workflow-mcp"),
            Cow::Borrowed("0.1.0"),
            Some("abcdef1234567890abcdef1234567890abcdef12"),
            Some("true"),
            Cow::Borrowed("aarch64-apple-darwin"),
        );
        let serialized = serde_json::to_string(&original).expect("serialize");
        let deserialized: BuildInfo = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(original, deserialized);
    }
}
