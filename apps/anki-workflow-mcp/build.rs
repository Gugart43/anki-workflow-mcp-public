use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    // 1. Export target triple
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=ANKI_BUILD_TARGET={target}");

    // 2. Track source files and build script without invalidating on target/cache
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../crates/anki-domain/src");
    println!("cargo:rerun-if-changed=../../crates/anki-parser/src");
    println!("cargo:rerun-if-changed=../../crates/anki-validate/src");

    // 3. Locate workspace root
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.clone());

    // 4. Safely detect git metadata.
    // If workspace root does not contain .git, do NOT run git to avoid collecting an ancestor repo.
    let git_marker = workspace_root.join(".git");
    if !git_marker.exists() {
        return;
    }

    if git_marker.is_file() {
        println!("cargo:rerun-if-changed={}", git_marker.display());
    }

    // Try collecting via git command first
    if try_git_command(&workspace_root) {
        return;
    }

    // Fallback: pure filesystem inspection if git CLI is absent or failed
    try_filesystem_git(&workspace_root, &git_marker);
}

fn try_git_command(workspace_root: &Path) -> bool {
    let show_toplevel = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output();

    let toplevel_out = match show_toplevel {
        Ok(out) if out.status.success() => out.stdout,
        _ => return false,
    };

    let toplevel_str = String::from_utf8_lossy(&toplevel_out).trim().to_string();
    if toplevel_str.is_empty() {
        return false;
    }

    let toplevel_path = PathBuf::from(toplevel_str);
    if !paths_match(workspace_root, &toplevel_path) {
        // Discovered repo is outside workspace root (e.g. ancestor repo); reject it.
        return false;
    }

    // Revision
    if let Ok(out) = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(workspace_root)
        .args(["rev-parse", "HEAD"])
        .output()
        && out.status.success()
    {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if is_valid_hex_sha(&sha) {
            println!("cargo:rustc-env=ANKI_BUILD_GIT_REVISION={sha}");
        }
    }

    // Dirty state
    if let Ok(out) = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(workspace_root)
        .args(["status", "--porcelain"])
        .output()
        && out.status.success()
    {
        let is_dirty =
            !out.stdout.is_empty() && out.stdout.iter().any(|b| !b.is_ascii_whitespace());
        println!("cargo:rustc-env=ANKI_BUILD_GIT_DIRTY={is_dirty}");
    }

    // Git ref/index triggers
    watch_git_path(workspace_root, "HEAD");
    watch_git_path(workspace_root, "index");
    watch_git_path(workspace_root, "packed-refs");

    if let Ok(out) = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(workspace_root)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        && out.status.success()
    {
        let ref_name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !ref_name.is_empty() {
            watch_git_path(workspace_root, &ref_name);
        }
    }

    true
}

fn watch_git_path(workspace_root: &Path, rel_or_key: &str) {
    if let Ok(out) = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(workspace_root)
        .args(["rev-parse", "--git-path", rel_or_key])
        .output()
        && out.status.success()
    {
        let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path_str.is_empty() {
            let p = Path::new(&path_str);
            let resolved = if p.is_absolute() {
                p.to_path_buf()
            } else {
                workspace_root.join(p)
            };
            if resolved.exists() {
                println!("cargo:rerun-if-changed={}", resolved.display());
            }
        }
    }
}

fn try_filesystem_git(workspace_root: &Path, git_marker: &Path) {
    let (admin_dir, common_dir) = if git_marker.is_file() {
        let content = match fs::read_to_string(git_marker) {
            Ok(c) => c,
            Err(_) => return,
        };
        let line = content.lines().next().unwrap_or("").trim();
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let p = PathBuf::from(rest.trim());
            let resolved = if p.is_absolute() {
                p
            } else {
                workspace_root.join(p)
            };
            let commondir = match fs::read_to_string(resolved.join("commondir")) {
                Ok(c) => {
                    let cd = PathBuf::from(c.trim());
                    if cd.is_absolute() {
                        cd
                    } else {
                        resolved.join(cd)
                    }
                }
                Err(_) => resolved.clone(),
            };
            (resolved, commondir)
        } else {
            return;
        }
    } else if git_marker.is_dir() {
        (git_marker.to_path_buf(), git_marker.to_path_buf())
    } else {
        return;
    };

    let head_file = admin_dir.join("HEAD");
    if head_file.exists() {
        println!("cargo:rerun-if-changed={}", head_file.display());
        if let Ok(content) = fs::read_to_string(&head_file) {
            let trimmed = content.trim();
            if is_valid_hex_sha(trimmed) {
                println!("cargo:rustc-env=ANKI_BUILD_GIT_REVISION={trimmed}");
            } else if let Some(ref_name) = trimmed.strip_prefix("ref:") {
                let ref_rel = ref_name.trim();
                let ref_path_admin = admin_dir.join(ref_rel);
                let ref_path_common = common_dir.join(ref_rel);
                let actual_ref = if ref_path_admin.exists() {
                    Some(ref_path_admin)
                } else if ref_path_common.exists() {
                    Some(ref_path_common)
                } else {
                    None
                };

                if let Some(rf) = actual_ref {
                    println!("cargo:rerun-if-changed={}", rf.display());
                    if let Ok(ref_content) = fs::read_to_string(&rf) {
                        let sha = ref_content.trim();
                        if is_valid_hex_sha(sha) {
                            println!("cargo:rustc-env=ANKI_BUILD_GIT_REVISION={sha}");
                        }
                    }
                } else {
                    // Check packed-refs in common_dir
                    let packed_refs = common_dir.join("packed-refs");
                    if packed_refs.exists()
                        && let Ok(packed) = fs::read_to_string(&packed_refs)
                    {
                        for line in packed.lines() {
                            let line = line.trim();
                            if line.starts_with('#') || line.starts_with('^') {
                                continue;
                            }
                            let mut parts = line.split_whitespace();
                            if let (Some(sha), Some(name)) = (parts.next(), parts.next())
                                && name == ref_rel
                                && is_valid_hex_sha(sha)
                            {
                                println!("cargo:rustc-env=ANKI_BUILD_GIT_REVISION={sha}");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    let index_file = admin_dir.join("index");
    if index_file.exists() {
        println!("cargo:rerun-if-changed={}", index_file.display());
    }

    let packed_refs = common_dir.join("packed-refs");
    if packed_refs.exists() {
        println!("cargo:rerun-if-changed={}", packed_refs.display());
    }
}

fn paths_match(p1: &Path, p2: &Path) -> bool {
    let c1 = p1.canonicalize().unwrap_or_else(|_| p1.to_path_buf());
    let c2 = p2.canonicalize().unwrap_or_else(|_| p2.to_path_buf());
    normalize_path(&c1) == normalize_path(&c2)
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let s_ref: &str = &s;
    let stripped = s_ref.strip_prefix(r"\\?\").unwrap_or(s_ref);
    PathBuf::from(stripped.replace('/', "\\").to_lowercase())
}

#[cfg(not(windows))]
fn normalize_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn is_valid_hex_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.chars().all(|c| c.is_ascii_hexdigit())
}
