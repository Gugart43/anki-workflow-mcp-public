//! `sync-media` CLI implementation: atomic, transactional synchronization
//! of media assets into Anki Desktop's `collection.media` directory.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

// Cleans extractor wrappers, HTML tags, and whitespace from caption strings.
pub fn clean_extractor_caption(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut clean = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        if c == '<' {
            in_tag = true;
            clean.push(' ');
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            clean.push(c);
        }
    }

    let mut out = String::with_capacity(clean.len());
    let mut last_was_ws = false;
    for c in clean.chars() {
        if c.is_whitespace() {
            if !last_was_ws {
                out.push(' ');
                last_was_ws = true;
            }
        } else {
            out.push(c);
            last_was_ws = false;
        }
    }
    out.trim().to_string()
}
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const PRIMARY_LOCK_FILE: &str = ".anki_workflow_media.lock";
pub const LEGACY_LOCK_FILE: &str = ".anki-media-sync.lock";

/// Parsed CLI arguments for `sync-media`.
#[derive(Debug, Clone, Default)]
pub struct SyncMediaArgs {
    pub catalog: PathBuf,
    pub collection_media_dir: Option<PathBuf>,
    pub apply: bool,
    pub copy_noise: bool,
    pub only: Vec<String>,
}

/// Catalog entry loaded from JSON or CSV.
#[derive(Debug, Clone, Default)]
pub struct CatalogEntry {
    pub filename: String,
    pub namespaced: String,
    pub caption: String,
    pub noise: bool,
    pub noise_reason: String,
    pub slug: String,
    pub page: Option<u32>,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
    pub item_type: String,
}

/// Parse command-line flags.
pub fn parse_args(args: &[String]) -> Result<SyncMediaArgs, (u8, String)> {
    let mut parsed = SyncMediaArgs::default();
    let mut catalog_set = false;
    let mut apply_flag = false;
    let mut dry_run_flag = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            return Err((
                0,
                "Usage: anki-workflow-mcp sync-media --catalog <PATH> [--collection-media-dir <DIR>] [--apply | --dry-run] [--copy-noise] [--only <NAMES>]"
                    .to_string(),
            ));
        } else if arg == "--apply" {
            parsed.apply = true;
            apply_flag = true;
            i += 1;
        } else if arg == "--dry-run" {
            parsed.apply = false;
            dry_run_flag = true;
            i += 1;
        } else if arg == "--copy-noise" {
            parsed.copy_noise = true;
            i += 1;
        } else if arg == "--catalog" {
            if i + 1 >= args.len() {
                return Err((2, "Missing value for --catalog".to_string()));
            }
            parsed.catalog = PathBuf::from(&args[i + 1]);
            catalog_set = true;
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--catalog=") {
            parsed.catalog = PathBuf::from(val);
            catalog_set = true;
            i += 1;
        } else if arg == "--collection-media-dir" {
            if i + 1 >= args.len() {
                return Err((2, "Missing value for --collection-media-dir".to_string()));
            }
            parsed.collection_media_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--collection-media-dir=") {
            parsed.collection_media_dir = Some(PathBuf::from(val));
            i += 1;
        } else if arg == "--only" {
            if i + 1 >= args.len() {
                return Err((2, "Missing value for --only".to_string()));
            }
            parsed.only = args[i + 1]
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--only=") {
            parsed.only = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            i += 1;
        } else {
            return Err((2, format!("Unknown option: {arg}")));
        }
    }

    if apply_flag && dry_run_flag {
        return Err((2, "Cannot specify both --apply and --dry-run".to_string()));
    }

    if !catalog_set {
        return Err((2, "Missing required option: --catalog".to_string()));
    }

    Ok(parsed)
}

/// Strips common latin diacritics / accents.
pub fn strip_accents(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'á' | 'à' | 'ã' | 'â' | 'ä' | 'Á' | 'À' | 'Ã' | 'Â' | 'Ä' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => out.push('i'),
            'ó' | 'ò' | 'õ' | 'ô' | 'ö' | 'Ó' | 'Ò' | 'Õ' | 'Ô' | 'Ö' => out.push('o'),
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => out.push('u'),
            'ç' | 'Ç' => out.push('c'),
            'ñ' | 'Ñ' => out.push('n'),
            _ => out.push(c),
        }
    }
    out
}

/// Normalizes a document slug to lowercase ASCII alphanumeric and underscores.
pub fn normalize_slug(text: &str) -> String {
    let stripped = strip_accents(text.trim()).to_lowercase();
    let mut out = String::with_capacity(stripped.len());
    let mut last_was_underscore = false;
    for c in stripped.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
            last_was_underscore = c == '_';
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    let s = out.trim_matches('_');
    if s.is_empty() {
        return "deck".to_string();
    }
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("doc_{s}")
    } else {
        s.to_string()
    }
}

/// Builds a 100% lowercase ASCII namespaced filename without redundant double-prefixing.
pub fn build_namespaced(slug: &str, filename: &str) -> String {
    let clean_slug = if slug.is_empty() {
        String::new()
    } else {
        normalize_slug(slug)
    };
    let fn_norm = strip_accents(filename.trim());
    let path = Path::new(&fn_norm);

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let mut s = String::from(".");
            for c in e.chars() {
                if c.is_ascii_alphanumeric() {
                    s.push(c.to_ascii_lowercase());
                }
            }
            s
        })
        .unwrap_or_default();

    let stem_raw = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("img")
        .to_ascii_lowercase();

    let mut stem_buf = String::with_capacity(stem_raw.len());
    let mut last_was_underscore = false;
    for c in stem_raw.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            stem_buf.push(c);
            last_was_underscore = c == '_';
        } else if !last_was_underscore {
            stem_buf.push('_');
            last_was_underscore = true;
        }
    }
    let mut stem = stem_buf.trim_matches('_').to_string();
    if stem.is_empty() {
        stem = "img".to_string();
    }

    let final_stem = if !clean_slug.is_empty() {
        let slug_parts: Vec<&str> = clean_slug.split('_').collect();
        let short_prefix = if slug_parts.len() >= 2 {
            format!("{}_{}", slug_parts[0], slug_parts[1])
        } else {
            clean_slug.clone()
        };

        if stem.starts_with(&format!("{clean_slug}_"))
            || stem == clean_slug
            || (short_prefix.len() >= 5 && stem.starts_with(&format!("{short_prefix}_")))
        {
            stem
        } else {
            format!("{clean_slug}_{stem}")
        }
    } else {
        stem
    };

    let mut res = final_stem;
    if res.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        res = format!("doc_{res}");
    }
    format!("{res}{ext}")
}

/// Classifies extraction noise based on caption, size, and bibliographic markers.
pub fn is_noise_caption(clean_caption: &str) -> (bool, &'static str) {
    if clean_caption.trim().is_empty() {
        return (true, "empty_caption");
    }

    let lower = strip_accents(clean_caption).to_lowercase();
    let trimmed = lower.trim();

    const BIBLIO_KEYWORDS: &[&str] = &[
        "isbn",
        "copyright",
        "todos os direitos reservados",
        "all rights reserved",
        "reproducao proibida",
        "publicado por",
        "editora",
        "printed in",
    ];
    for kw in BIBLIO_KEYWORDS {
        if trimmed.contains(kw) {
            return (true, "bibliographic_or_copyright_noise");
        }
    }

    const AUTO_PREFIXES: &[&str] = &[
        "figura", "imagem", "foto", "esquema", "grafico", "tabela", "quadro", "prancha", "image",
        "figure",
    ];
    for prefix in AUTO_PREFIXES {
        if let Some(stripped) = trimmed.strip_prefix(prefix) {
            let remainder = stripped.trim();
            if remainder.is_empty() {
                return (true, "auto_generated_caption");
            }
            let tokens: Vec<&str> = remainder.split_whitespace().collect();
            if tokens.len() == 1 && tokens[0].chars().all(|c| c.is_ascii_digit()) {
                return (true, "auto_generated_caption");
            }
            let is_page_pattern = match tokens.as_slice() {
                [num, "da", "pagina", page_num]
                    if num.chars().all(|c| c.is_ascii_digit())
                        && page_num.chars().all(|c| c.is_ascii_digit()) =>
                {
                    true
                }
                [num, "of", "page", page_num]
                    if num.chars().all(|c| c.is_ascii_digit())
                        && page_num.chars().all(|c| c.is_ascii_digit()) =>
                {
                    true
                }
                ["da", "pagina", page_num] if page_num.chars().all(|c| c.is_ascii_digit()) => true,
                ["of", "page", page_num] if page_num.chars().all(|c| c.is_ascii_digit()) => true,
                _ => false,
            };
            if is_page_pattern {
                return (true, "auto_generated_caption");
            }
        }
    }

    (false, "")
}

/// Validates that a filename does not contain path traversal or reserved device names.
pub fn validate_filename_security(name: &str) -> Result<(), (u8, String)> {
    if name.is_empty() || name.trim().is_empty() || name == "." || name == ".." {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Invalid or empty filename rejected: '{name}'"),
            "suggestion": "Filename must not be empty."
        });
        return Err((1, err.to_string()));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Path traversal rejected: '{name}'"),
            "suggestion": "Filename must not contain path separators or traversal segments."
        });
        return Err((1, err.to_string()));
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Drive-relative filename rejected: '{name}'"),
            "suggestion": "Filename must not contain a Windows drive prefix."
        });
        return Err((1, err.to_string()));
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Invalid Windows filename character rejected: '{name}'"),
            "suggestion": "Remove control characters and Windows-forbidden characters (< > : \" | ? *)."
        });
        return Err((1, err.to_string()));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Trailing space or dot rejected: '{name}'"),
            "suggestion": "Filename must not end with a space or dot."
        });
        return Err((1, err.to_string()));
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Reserved device name rejected: '{name}'"),
            "suggestion": "Use a safe filename."
        });
        return Err((1, err.to_string()));
    }
    Ok(())
}

/// Checks that a path is not a symlink.
pub fn check_not_symlink(path: &Path) -> Result<(), (u8, String)> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SECURITY_ERROR",
            "message": format!("Symlink forbidden: '{}'", path.display()),
            "suggestion": "Symlinks are forbidden in media operations."
        });
        return Err((1, err.to_string()));
    }
    Ok(())
}

/// Computes hex-encoded SHA-256 digest of bytes.
pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Parses a JSON catalog (either `catalog.json`, `central_images.json`, or array).
pub fn parse_json_catalog(
    raw: &str,
    default_slug: &str,
) -> Result<Vec<CatalogEntry>, (u8, String)> {
    let parsed: Value = serde_json::from_str(raw).map_err(|e| {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.CATALOG_ERROR",
            "message": format!("Failed to parse JSON catalog: {e}"),
            "suggestion": "Check JSON syntax."
        });
        (1, err.to_string())
    })?;

    let mut entries = Vec::new();

    if let Some(images) = parsed.get("images").and_then(|v| v.as_array()) {
        for item in images {
            let fn_str = item
                .get("filename")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if fn_str.is_empty() {
                continue;
            }
            let doc_title = item
                .get("doc_title")
                .or_else(|| item.get("folder"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let slug = if !doc_title.is_empty() {
                normalize_slug(doc_title)
            } else if let Some(first_part) = fn_str.split("_p").next() {
                normalize_slug(first_part)
            } else {
                default_slug.to_string()
            };

            let raw_cap = item
                .get("caption")
                .or_else(|| item.get("caption_html"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let clean_cap = clean_extractor_caption(raw_cap);

            let explicit_ns = item
                .get("namespaced")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let namespaced = if !explicit_ns.is_empty() {
                explicit_ns.to_lowercase()
            } else {
                build_namespaced(&slug, fn_str)
            };

            let explicit_noise = item.get("noise").and_then(|v| v.as_bool());
            let w_px = item
                .get("width_px")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let h_px = item
                .get("height_px")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let (is_noise, n_reason) = if let Some(n) = explicit_noise {
                (n, if n { "explicit_noise" } else { "" })
            } else {
                let (cap_noise, reason) = is_noise_caption(&clean_cap);
                if cap_noise {
                    (true, reason)
                } else if w_px.unwrap_or(u32::MAX) < 180 && h_px.unwrap_or(u32::MAX) < 180 {
                    (true, "tiny_icon_or_logo")
                } else {
                    (false, "")
                }
            };

            entries.push(CatalogEntry {
                filename: fn_str.to_string(),
                namespaced,
                caption: clean_cap,
                noise: is_noise,
                noise_reason: n_reason.to_string(),
                slug,
                page: item.get("page").and_then(|v| v.as_u64()).map(|v| v as u32),
                width_px: w_px,
                height_px: h_px,
                item_type: item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("figure")
                    .to_string(),
            });
        }
        return Ok(entries);
    }

    let items = if let Some(items) = parsed.get("items").and_then(|v| v.as_array()) {
        items.clone()
    } else if let Some(arr) = parsed.as_array() {
        arr.clone()
    } else {
        Vec::new()
    };

    let obj_slug = parsed
        .get("slug")
        .or_else(|| parsed.get("pdf_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let doc_slug = if !obj_slug.is_empty() {
        normalize_slug(obj_slug)
    } else {
        default_slug.to_string()
    };

    for item in &items {
        let fn_str = item
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if fn_str.is_empty() {
            continue;
        }
        let raw_cap = item
            .get("caption")
            .or_else(|| item.get("caption_html"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let clean_cap = clean_extractor_caption(raw_cap);

        let explicit_ns = item
            .get("namespaced")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let namespaced = if !explicit_ns.is_empty() {
            explicit_ns.to_lowercase()
        } else {
            build_namespaced(&doc_slug, fn_str)
        };

        let explicit_noise = item.get("noise").and_then(|v| v.as_bool());
        let w_px = item
            .get("width_px")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let h_px = item
            .get("height_px")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let (is_noise, n_reason) = if let Some(n) = explicit_noise {
            (n, if n { "explicit_noise" } else { "" })
        } else {
            let (cap_noise, reason) = is_noise_caption(&clean_cap);
            if cap_noise {
                (true, reason)
            } else if w_px.unwrap_or(u32::MAX) < 180 && h_px.unwrap_or(u32::MAX) < 180 {
                (true, "tiny_icon_or_logo")
            } else {
                (false, "")
            }
        };

        entries.push(CatalogEntry {
            filename: fn_str.to_string(),
            namespaced,
            caption: clean_cap,
            noise: is_noise,
            noise_reason: n_reason.to_string(),
            slug: doc_slug.clone(),
            page: item.get("page").and_then(|v| v.as_u64()).map(|v| v as u32),
            width_px: w_px,
            height_px: h_px,
            item_type: item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("figure")
                .to_string(),
        });
    }

    Ok(entries)
}

/// Parses a pipe-delimited CSV catalog (with headers or headerless).
pub fn parse_csv_catalog(raw: &str, default_slug: &str) -> Result<Vec<CatalogEntry>, (u8, String)> {
    let text = raw.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let first_line = lines[0];
    let has_header = first_line.to_lowercase().contains("filename")
        || first_line.to_lowercase().contains("slug")
        || first_line.to_lowercase().contains("namespaced");

    let mut entries = Vec::new();

    if has_header {
        let headers: Vec<String> = first_line
            .split('|')
            .map(|h| h.trim().to_lowercase())
            .collect();

        let col_filename = headers.iter().position(|h| h == "filename");
        let col_namespaced = headers.iter().position(|h| h == "namespaced");
        let col_caption = headers.iter().position(|h| h == "caption");
        let col_slug = headers.iter().position(|h| h == "slug");
        let col_noise = headers.iter().position(|h| h == "noise");
        let col_page = headers.iter().position(|h| h == "page");
        let col_type = headers.iter().position(|h| h == "type");

        for line in &lines[1..] {
            let parts: Vec<&str> = line.split('|').map(|p| p.trim()).collect();
            let fn_str = col_filename
                .and_then(|idx| parts.get(idx))
                .unwrap_or(&"")
                .trim();
            if fn_str.is_empty() {
                continue;
            }

            let raw_slug = col_slug
                .and_then(|idx| parts.get(idx))
                .unwrap_or(&"")
                .trim();
            let slug = if !raw_slug.is_empty() {
                normalize_slug(raw_slug)
            } else {
                default_slug.to_string()
            };

            let raw_cap = col_caption
                .and_then(|idx| parts.get(idx))
                .unwrap_or(&"")
                .trim();
            let clean_cap = clean_extractor_caption(raw_cap);

            let explicit_ns = col_namespaced
                .and_then(|idx| parts.get(idx))
                .unwrap_or(&"")
                .trim();
            let namespaced = if !explicit_ns.is_empty() {
                explicit_ns.to_lowercase()
            } else {
                build_namespaced(&slug, fn_str)
            };

            let explicit_noise = col_noise.and_then(|idx| parts.get(idx)).and_then(|v| {
                let v_lower = v.to_lowercase();
                if v_lower == "true" || v_lower == "1" || v_lower == "yes" {
                    Some(true)
                } else if v_lower == "false" || v_lower == "0" || v_lower == "no" {
                    Some(false)
                } else {
                    None
                }
            });

            let (is_noise, n_reason) = if let Some(n) = explicit_noise {
                (n, if n { "explicit_noise" } else { "" })
            } else {
                is_noise_caption(&clean_cap)
            };

            let page = col_page
                .and_then(|idx| parts.get(idx))
                .and_then(|s| s.parse::<u32>().ok());
            let item_type = col_type
                .and_then(|idx| parts.get(idx))
                .copied()
                .unwrap_or("figure")
                .to_string();

            entries.push(CatalogEntry {
                filename: fn_str.to_string(),
                namespaced,
                caption: clean_cap,
                noise: is_noise,
                noise_reason: n_reason.to_string(),
                slug,
                page,
                width_px: None,
                height_px: None,
                item_type,
            });
        }
        return Ok(entries);
    }

    // Headerless format
    for line in lines {
        if line.starts_with('|') && (line.contains("---") || line.contains("Tag Anki")) {
            continue;
        }
        let parts: Vec<&str> = line.split('|').map(|p| p.trim()).collect();
        if parts.is_empty() {
            continue;
        }

        // Format 1: filename|namespaced|noise
        if parts.len() == 3
            && matches!(
                parts[2].to_lowercase().as_str(),
                "true" | "false" | "1" | "0" | "yes" | "no"
            )
        {
            let fn_str = parts[0];
            let ns_str = parts[1];
            let n_bool = matches!(parts[2].to_lowercase().as_str(), "true" | "1" | "yes");
            entries.push(CatalogEntry {
                filename: fn_str.to_string(),
                namespaced: ns_str.to_lowercase(),
                caption: String::new(),
                noise: n_bool,
                noise_reason: if n_bool {
                    "explicit_noise".to_string()
                } else {
                    String::new()
                },
                slug: default_slug.to_string(),
                page: None,
                width_px: None,
                height_px: None,
                item_type: "figure".to_string(),
            });
            continue;
        }

        // Format 2: filename|caption|slug
        if parts.len() >= 3 && !parts[0].is_empty() {
            let fn_str = parts[0];
            let cap_str = parts[1];
            let raw_slug = parts[2];
            let slug = if !raw_slug.is_empty() {
                normalize_slug(raw_slug)
            } else {
                default_slug.to_string()
            };
            let clean_cap = clean_extractor_caption(cap_str);
            let (is_noise, n_reason) = is_noise_caption(&clean_cap);
            let namespaced = build_namespaced(&slug, fn_str);
            entries.push(CatalogEntry {
                filename: fn_str.to_string(),
                namespaced,
                caption: clean_cap,
                noise: is_noise,
                noise_reason: n_reason.to_string(),
                slug,
                page: None,
                width_px: None,
                height_px: None,
                item_type: "figure".to_string(),
            });
            continue;
        }

        // Format 3: 2-column "<img src=...>" | caption or filename | caption
        if parts.len() >= 2 {
            let col0 = parts[0];
            let fn_str = if let Some(start) = col0.find("src=\"") {
                let sub = &col0[start + 5..];
                sub.split('"').next().unwrap_or(col0).trim()
            } else if let Some(start) = col0.find("src='") {
                let sub = &col0[start + 5..];
                sub.split('\'').next().unwrap_or(col0).trim()
            } else {
                col0.trim_matches('"').trim_matches('\'').trim()
            };

            if fn_str.is_empty() {
                continue;
            }

            let clean_cap = clean_extractor_caption(parts[1]);
            let (is_noise, n_reason) = is_noise_caption(&clean_cap);
            let namespaced = build_namespaced(default_slug, fn_str);
            entries.push(CatalogEntry {
                filename: fn_str.to_string(),
                namespaced,
                caption: clean_cap,
                noise: is_noise,
                noise_reason: n_reason.to_string(),
                slug: default_slug.to_string(),
                page: None,
                width_px: None,
                height_px: None,
                item_type: "figure".to_string(),
            });
        }
    }

    Ok(entries)
}

fn is_md_separator_row(cells: &[&str]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let t = cell.trim();
            !t.is_empty()
                && t.contains('-')
                && t.chars().all(|c| c == '-' || c == ':' || c.is_whitespace())
        })
}

fn extract_markdown_image_target(cell: &str) -> Option<String> {
    if let Some(bang_idx) = cell.find("![") {
        let after_bang = &cell[bang_idx + 2..];
        if let Some(close_bracket) = after_bang.find("](") {
            let after_open = &after_bang[close_bracket + 2..];
            if let Some(close_paren) = after_open.find(')') {
                let raw_target = after_open[..close_paren].trim();
                let target = if let Some(inner) = raw_target
                    .strip_prefix('<')
                    .and_then(|s| s.strip_suffix('>'))
                {
                    inner.trim()
                } else {
                    raw_target.split_whitespace().next().unwrap_or("")
                };
                if !target.is_empty() {
                    return Some(target.to_string());
                }
            }
        }
    }
    None
}

/// Parses a Markdown table catalog (`catalog.md`), extracting `![](file)` image targets
/// and ignoring Markdown table separator rows (`---`).
pub fn parse_markdown_catalog(
    raw: &str,
    default_slug: &str,
) -> Result<Vec<CatalogEntry>, (u8, String)> {
    let text = raw.trim_start_matches('\u{feff}');
    let mut entries = Vec::new();

    let mut col_filename: Option<usize> = None;
    let mut col_namespaced: Option<usize> = None;
    let mut col_caption: Option<usize> = None;
    let mut col_slug: Option<usize> = None;
    let mut col_page: Option<usize> = None;
    let mut col_noise: Option<usize> = None;
    let mut header_seen = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains('|') {
            continue;
        }

        let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
        let inner = inner.strip_suffix('|').unwrap_or(inner);
        let cells: Vec<&str> = inner.split('|').map(|c| c.trim()).collect();

        if cells.is_empty() || cells.iter().all(|c| c.is_empty()) || is_md_separator_row(&cells) {
            continue;
        }

        let has_md_image = cells
            .iter()
            .any(|c| extract_markdown_image_target(c).is_some());

        if !header_seen && !has_md_image {
            let lower_cells: Vec<String> = cells.iter().map(|c| c.to_lowercase()).collect();
            let looks_like_header = lower_cells.iter().any(|h| {
                matches!(
                    h.as_str(),
                    "arquivo"
                        | "filename"
                        | "file"
                        | "imagem"
                        | "image"
                        | "preview"
                        | "legenda"
                        | "caption"
                        | "namespaced"
                        | "tag anki"
                        | "slug"
                        | "page"
                        | "página"
                        | "pagina"
                        | "noise"
                )
            });
            if looks_like_header {
                col_filename = lower_cells
                    .iter()
                    .position(|h| matches!(h.as_str(), "arquivo" | "filename" | "file"));
                col_namespaced = lower_cells
                    .iter()
                    .position(|h| matches!(h.as_str(), "namespaced" | "tag anki"));
                col_caption = lower_cells
                    .iter()
                    .position(|h| matches!(h.as_str(), "legenda" | "caption"));
                col_slug = lower_cells.iter().position(|h| h == "slug");
                col_page = lower_cells
                    .iter()
                    .position(|h| matches!(h.as_str(), "page" | "página" | "pagina"));
                col_noise = lower_cells.iter().position(|h| h == "noise");
                header_seen = true;
                continue;
            }
        }

        let img_match = cells
            .iter()
            .enumerate()
            .find_map(|(idx, c)| extract_markdown_image_target(c).map(|target| (idx, target)));

        let (img_col, filename) = if let Some((idx, target)) = img_match {
            (Some(idx), target)
        } else if let Some(fn_idx) = col_filename {
            let candidate = cells.get(fn_idx).copied().unwrap_or("").trim();
            if candidate.is_empty() {
                continue;
            }
            (Some(fn_idx), candidate.trim_matches('`').to_string())
        } else {
            continue;
        };

        let raw_slug = col_slug
            .and_then(|idx| cells.get(idx))
            .copied()
            .unwrap_or("")
            .trim();
        let slug = if !raw_slug.is_empty() {
            normalize_slug(raw_slug)
        } else {
            default_slug.to_string()
        };

        let raw_cap = if let Some(cap_idx) = col_caption {
            cells.get(cap_idx).copied().unwrap_or("")
        } else {
            cells
                .iter()
                .enumerate()
                .find(|(idx, c)| {
                    Some(*idx) != img_col
                        && Some(*idx) != col_filename
                        && Some(*idx) != col_namespaced
                        && !c.is_empty()
                        && !c.chars().all(|ch| ch.is_ascii_digit())
                })
                .map(|(_, c)| *c)
                .unwrap_or("")
        };
        let clean_cap = clean_extractor_caption(raw_cap);

        let explicit_ns = col_namespaced
            .and_then(|idx| cells.get(idx))
            .copied()
            .unwrap_or("")
            .trim()
            .trim_matches('`');
        let namespaced = if !explicit_ns.is_empty() {
            if let Some(start) = explicit_ns.find("src=\"") {
                explicit_ns[start + 5..]
                    .split('"')
                    .next()
                    .unwrap_or(explicit_ns)
                    .trim()
                    .to_lowercase()
            } else {
                explicit_ns.to_lowercase()
            }
        } else {
            build_namespaced(&slug, &filename)
        };

        let explicit_noise = col_noise.and_then(|idx| cells.get(idx)).and_then(|v| {
            match v.to_lowercase().as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => None,
            }
        });
        let (is_noise, n_reason) = if let Some(n) = explicit_noise {
            (n, if n { "explicit_noise" } else { "" })
        } else {
            is_noise_caption(&clean_cap)
        };

        let page = col_page
            .and_then(|idx| cells.get(idx))
            .and_then(|s| s.parse::<u32>().ok());

        entries.push(CatalogEntry {
            filename,
            namespaced,
            caption: clean_cap,
            noise: is_noise,
            noise_reason: n_reason.to_string(),
            slug,
            page,
            width_px: None,
            height_px: None,
            item_type: "figure".to_string(),
        });
    }

    Ok(entries)
}

/// Loads catalog entries from a file or directory.
pub fn load_catalog(catalog_path: &Path) -> Result<Vec<CatalogEntry>, (u8, String)> {
    let target_file = if catalog_path.is_dir() {
        let candidates = [
            "catalog.json",
            "anki_catalog.csv",
            "anki_deck.csv",
            "catalog.md",
            "central_images.json",
        ];
        candidates
            .iter()
            .map(|c| catalog_path.join(c))
            .find(|p| p.is_file())
            .unwrap_or_else(|| catalog_path.to_path_buf())
    } else {
        catalog_path.to_path_buf()
    };

    // Promote sibling `catalog.json` automatically when `catalog.md` is provided.
    let target_file = if target_file
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("catalog.md"))
    {
        let sibling_json = target_file.with_file_name("catalog.json");
        if sibling_json.is_file() {
            sibling_json
        } else {
            target_file
        }
    } else {
        target_file
    };

    if target_file.is_dir() || !target_file.exists() {
        return Ok(Vec::new());
    }

    check_not_symlink(&target_file)?;

    let raw = fs::read_to_string(&target_file).map_err(|e| {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.CATALOG_ERROR",
            "message": format!("Failed to read catalog file: {e}"),
            "suggestion": "Check file permissions."
        });
        (1, err.to_string())
    })?;

    let ext = target_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let parent_name = target_file
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let default_slug = normalize_slug(parent_name.trim_start_matches("images_"));

    if ext == "json" {
        parse_json_catalog(&raw, &default_slug)
    } else if ext == "md" {
        parse_markdown_catalog(&raw, &default_slug)
    } else {
        parse_csv_catalog(&raw, &default_slug)
    }
}

/// Helper to acquire an exclusive lock with a timeout.
fn acquire_exclusive_lock_with_timeout(file: &File, timeout: Duration) -> std::io::Result<bool> {
    let start = std::time::Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(true),
            Err(e) => {
                #[cfg(windows)]
                let is_contention =
                    e.raw_os_error() == Some(33) || e.kind() == std::io::ErrorKind::WouldBlock;
                #[cfg(not(windows))]
                let is_contention = e.kind() == std::io::ErrorKind::WouldBlock;

                if is_contention {
                    if start.elapsed() >= timeout {
                        return Ok(false);
                    }
                    thread::sleep(Duration::from_millis(50));
                } else {
                    return Err(e);
                }
            }
        }
    }
}

/// Acquires both primary and legacy exclusive media locks, propagating `GOV.LOCK` on contention.
fn acquire_media_locks(
    collection_media_dir: &Path,
    timeout: Duration,
) -> Result<(File, File), (u8, String)> {
    let primary_lock_path = collection_media_dir.join(PRIMARY_LOCK_FILE);
    let primary_lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&primary_lock_path)
        .map_err(|e| {
            let err = json!({
                "status": "ERROR",
                "code": "MEDIA.IO_ERROR",
                "message": format!("Failed to open lock file: {e}"),
                "suggestion": "Check directory permissions in collection.media."
            });
            (1, err.to_string())
        })?;

    if !acquire_exclusive_lock_with_timeout(&primary_lock_file, timeout).map_err(|e| {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.IO_ERROR",
            "message": format!("Failed while acquiring primary lock: {e}"),
            "suggestion": "Check permissions."
        });
        (1, err.to_string())
    })? {
        let err = json!({
            "status": "ERROR",
            "code": "GOV.LOCK",
            "message": "Media collection is busy.",
            "suggestion": "Retry after the current synchronization finishes."
        });
        return Err((1, err.to_string()));
    }

    let legacy_lock_path = collection_media_dir.join(LEGACY_LOCK_FILE);
    let legacy_lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&legacy_lock_path)
        .map_err(|e| {
            let err = json!({
                "status": "ERROR",
                "code": "MEDIA.IO_ERROR",
                "message": format!("Failed to open legacy lock file: {e}"),
                "suggestion": "Check directory permissions in collection.media."
            });
            (1, err.to_string())
        })?;

    if !acquire_exclusive_lock_with_timeout(&legacy_lock_file, timeout).map_err(|e| {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.IO_ERROR",
            "message": format!("Failed while acquiring legacy lock: {e}"),
            "suggestion": "Check permissions."
        });
        (1, err.to_string())
    })? {
        let err = json!({
            "status": "ERROR",
            "code": "GOV.LOCK",
            "message": "Media collection is busy (legacy lock active).",
            "suggestion": "Retry after the current synchronization finishes."
        });
        return Err((1, err.to_string()));
    }

    Ok((primary_lock_file, legacy_lock_file))
}

fn collect_rollback_missing(
    to_copy: &[(PathBuf, PathBuf, String)],
    entries: &[CatalogEntry],
    existing_missing: &[Value],
) -> Vec<Value> {
    let mut remaining_missing = existing_missing.to_vec();
    for (_, _, ns) in to_copy {
        if !remaining_missing
            .iter()
            .any(|m| m.get("namespaced").and_then(|v| v.as_str()) == Some(ns.as_str()))
        {
            let cap = entries
                .iter()
                .find(|e| e.namespaced == *ns)
                .map(|e| e.caption.clone())
                .unwrap_or_default();
            remaining_missing.push(json!({
                "namespaced": ns,
                "caption": cap,
            }));
        }
    }
    remaining_missing
}

/// Main entrypoint for `sync-media` CLI subcommand.
pub fn run(args: &[String]) -> Result<Value, (u8, String)> {
    let parsed_args = match parse_args(args) {
        Ok(a) => a,
        Err((0, help)) => {
            return Ok(json!({
                "status": "help",
                "usage": help,
            }));
        }
        Err(err) => return Err(err),
    };

    let collection_media_dir = match parsed_args.collection_media_dir {
        Some(dir) => dir,
        None => {
            if let Ok(env_path) = std::env::var("ANKI_COLLECTION_MEDIA") {
                if !env_path.trim().is_empty() {
                    PathBuf::from(env_path.trim())
                } else {
                    let err = json!({
                        "status": "ERROR",
                        "code": "MEDIA.CATALOG_ERROR",
                        "message": "Missing collection_media_dir: neither --collection-media-dir nor ANKI_COLLECTION_MEDIA is set",
                        "suggestion": "Check Anki collection.media directory."
                    });
                    return Err((1, err.to_string()));
                }
            } else {
                let err = json!({
                    "status": "ERROR",
                    "code": "MEDIA.CATALOG_ERROR",
                    "message": "Missing collection_media_dir: neither --collection-media-dir nor ANKI_COLLECTION_MEDIA is set",
                    "suggestion": "Check Anki collection.media directory."
                });
                return Err((1, err.to_string()));
            }
        }
    };

    if !collection_media_dir.exists() || !collection_media_dir.is_dir() {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.CATALOG_ERROR",
            "message": format!("collection_media_dir not found: {}", collection_media_dir.display()),
            "suggestion": "Check Anki collection.media directory."
        });
        return Err((1, err.to_string()));
    }

    check_not_symlink(&collection_media_dir)?;

    if !parsed_args.catalog.exists() {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.CATALOG_ERROR",
            "message": format!("File not found: {}", parsed_args.catalog.display()),
            "suggestion": "Check the path and the CSV/JSON format."
        });
        return Err((1, err.to_string()));
    }

    check_not_symlink(&parsed_args.catalog)?;

    // Only create/acquire lock files when mutating (`--apply`).
    let _locks = if parsed_args.apply {
        Some(acquire_media_locks(
            &collection_media_dir,
            Duration::from_secs(5),
        )?)
    } else {
        None
    };

    let source_dir = if parsed_args.catalog.is_file() {
        parsed_args
            .catalog
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        parsed_args.catalog.clone()
    };

    let entries = load_catalog(&parsed_args.catalog)?;
    let filter_set: HashSet<String> = parsed_args
        .only
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut skipped_noise = 0usize;
    let mut planned: HashMap<String, String> = HashMap::new();
    let mut collisions: Vec<Value> = Vec::new();
    let mut synced: Vec<String> = Vec::new();
    let mut missing: Vec<Value> = Vec::new();
    let mut to_copy: Vec<(PathBuf, PathBuf, String)> = Vec::new();

    for e in &entries {
        if e.namespaced.is_empty() {
            continue;
        }
        if !parsed_args.copy_noise && e.noise && filter_set.is_empty() {
            skipped_noise += 1;
            continue;
        }
        if !filter_set.is_empty()
            && !filter_set.contains(&e.namespaced.to_lowercase())
            && !filter_set.contains(&e.filename.to_lowercase())
        {
            continue;
        }

        validate_filename_security(&e.filename)?;
        validate_filename_security(&e.namespaced)?;

        let target = collection_media_dir.join(&e.namespaced);
        check_not_symlink(&target)?;

        let mut src_file = source_dir.join(&e.filename);
        if !src_file.exists() {
            src_file = source_dir.join(&e.namespaced);
        }
        if src_file.exists() {
            check_not_symlink(&src_file)?;
        }

        let key = e.namespaced.to_lowercase();

        if src_file.is_file() {
            let src_bytes = fs::read(&src_file).map_err(|e| {
                let err = json!({
                    "status": "ERROR",
                    "code": "MEDIA.IO_ERROR",
                    "message": format!("Failed to read source file '{}': {e}", src_file.display()),
                    "suggestion": "Check file permissions."
                });
                (1, err.to_string())
            })?;
            let digest = compute_sha256(&src_bytes);

            if let Some(existing_digest) = planned.get(&key) {
                if existing_digest != &digest {
                    collisions.push(json!({
                        "namespaced": e.namespaced,
                        "source": src_file.display().to_string(),
                        "target": target.display().to_string(),
                        "source_sha256": digest,
                        "target_sha256": existing_digest,
                    }));
                }
                continue;
            }
            planned.insert(key, digest.clone());

            if target.exists() {
                let target_bytes = fs::read(&target).map_err(|e| {
                    let err = json!({
                        "status": "ERROR",
                        "code": "MEDIA.IO_ERROR",
                        "message": format!("Failed to read target file '{}': {e}", target.display()),
                        "suggestion": "Check file permissions."
                    });
                    (1, err.to_string())
                })?;
                let dst_sha = compute_sha256(&target_bytes);
                if digest != dst_sha {
                    collisions.push(json!({
                        "namespaced": e.namespaced,
                        "source": src_file.display().to_string(),
                        "target": target.display().to_string(),
                        "source_sha256": digest,
                        "target_sha256": dst_sha,
                    }));
                    continue;
                }
                synced.push(e.namespaced.clone());
            } else {
                to_copy.push((src_file, target, e.namespaced.clone()));
            }
        } else if target.exists() {
            synced.push(e.namespaced.clone());
        } else {
            missing.push(json!({
                "namespaced": e.namespaced,
                "caption": e.caption,
            }));
        }
    }

    if !parsed_args.apply || !collisions.is_empty() {
        for (_, _, ns) in &to_copy {
            let cap = entries
                .iter()
                .find(|e| &e.namespaced == ns)
                .map(|e| e.caption.clone())
                .unwrap_or_default();
            missing.push(json!({
                "namespaced": ns,
                "caption": cap,
            }));
        }

        let active_total = synced.len() + missing.len() + collisions.len();
        let ratio = if active_total > 0 {
            ((synced.len() as f64 / active_total as f64) * 10000.0).round() / 10000.0
        } else {
            1.0
        };

        if !collisions.is_empty() {
            let err = json!({
                "status": "ERROR",
                "code": "MEDIA.COLLISION",
                "message": format!("Found {} collision(s) between catalog and collection.media", collisions.len()),
                "suggestion": "Resolve filename collisions or update catalog before syncing.",
                "catalog": parsed_args.catalog.display().to_string(),
                "collection": collection_media_dir.display().to_string(),
                "total_catalog": entries.len(),
                "active_evaluated": active_total,
                "skipped_noise": skipped_noise,
                "synced": synced.len(),
                "copied": 0,
                "missing": missing.len(),
                "collisions": collisions,
                "sync_ratio": ratio,
                "missing_list": missing.iter().take(50).cloned().collect::<Vec<_>>(),
            });
            return Err((1, err.to_string()));
        }

        return Ok(json!({
            "status": "OK",
            "catalog": parsed_args.catalog.display().to_string(),
            "collection": collection_media_dir.display().to_string(),
            "total_catalog": entries.len(),
            "active_evaluated": active_total,
            "skipped_noise": skipped_noise,
            "synced": synced.len(),
            "copied": 0,
            "missing": missing.len(),
            "collisions": collisions,
            "sync_ratio": ratio,
            "missing_list": missing.iter().take(50).cloned().collect::<Vec<_>>(),
        }));
    }

    if !missing.is_empty() {
        let err = json!({
            "status": "ERROR",
            "code": "MEDIA.SOURCE_MISSING",
            "message": "Source files are missing; no files were copied.",
            "suggestion": "Restore all source files before applying the batch.",
            "missing": missing.len(),
            "missing_list": missing,
            "copied": 0,
        });
        return Err((1, err.to_string()));
    }

    let initial_synced = synced.len();
    let mut batch_copied: Vec<PathBuf> = Vec::new();
    let mut copied_names: Vec<String> = Vec::new();

    for (src_file, target, namespaced) in &to_copy {
        let copy_res: Result<(), std::io::Error> = (|| {
            let mut tmp = tempfile::NamedTempFile::new_in(&collection_media_dir)?;
            let mut src = File::open(src_file)?;
            std::io::copy(&mut src, &mut tmp)?;
            tmp.as_file().sync_all()?;
            tmp.persist(target).map_err(|e| e.error)?;
            Ok(())
        })();

        match copy_res {
            Ok(()) => {
                batch_copied.push(target.clone());
                copied_names.push(namespaced.clone());
                synced.push(namespaced.clone());
            }
            Err(exc) => {
                let mut rollback_errors = Vec::new();
                let mut rolled_back = 0usize;
                for rollback_file in batch_copied.iter().rev() {
                    if rollback_file.exists() {
                        match fs::remove_file(rollback_file) {
                            Ok(()) => {
                                rolled_back += 1;
                            }
                            Err(cleanup_exc) => {
                                rollback_errors.push(json!({
                                    "path": rollback_file.display().to_string(),
                                    "error": cleanup_exc.to_string(),
                                }));
                            }
                        }
                    }
                }

                let remaining_missing = collect_rollback_missing(&to_copy, &entries, &missing);

                let effective_synced = initial_synced + batch_copied.len() - rolled_back;
                let active_total = initial_synced + remaining_missing.len() + collisions.len();
                let ratio = if active_total > 0 {
                    ((initial_synced as f64 / active_total as f64) * 10000.0).round() / 10000.0
                } else {
                    1.0
                };

                let err = json!({
                    "status": "ERROR",
                    "code": "MEDIA.IO_ERROR",
                    "message": format!(
                        "I/O error copying {} to collection.media ({}); rolled back {} file(s)",
                        src_file.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
                        exc,
                        rolled_back
                    ),
                    "suggestion": "Check write permissions and free disk space in collection.media.",
                    "catalog": parsed_args.catalog.display().to_string(),
                    "collection": collection_media_dir.display().to_string(),
                    "failed_file": target.display().to_string(),
                    "rolled_back_count": rolled_back,
                    "rollback_errors": rollback_errors,
                    "rollback_complete": rollback_errors.is_empty(),
                    "total_catalog": entries.len(),
                    "active_evaluated": active_total,
                    "skipped_noise": skipped_noise,
                    "synced": effective_synced,
                    "copied": batch_copied.len() - rolled_back,
                    "missing": remaining_missing.len(),
                    "collisions": collisions,
                    "sync_ratio": ratio,
                    "missing_list": remaining_missing.iter().take(50).cloned().collect::<Vec<_>>(),
                });
                return Err((1, err.to_string()));
            }
        }
    }

    let active_total = synced.len() + missing.len() + collisions.len();
    let ratio = if active_total > 0 {
        ((synced.len() as f64 / active_total as f64) * 10000.0).round() / 10000.0
    } else {
        1.0
    };

    Ok(json!({
        "status": "OK",
        "catalog": parsed_args.catalog.display().to_string(),
        "collection": collection_media_dir.display().to_string(),
        "total_catalog": entries.len(),
        "active_evaluated": active_total,
        "skipped_noise": skipped_noise,
        "synced": synced.len(),
        "copied": copied_names.len(),
        "missing": missing.len(),
        "collisions": collisions,
        "sync_ratio": ratio,
        "missing_list": missing.iter().take(50).cloned().collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_args_parsing_variants() {
        let args = vec![
            "--catalog".to_string(),
            "my_cat.json".to_string(),
            "--collection-media-dir".to_string(),
            "/path/media".to_string(),
            "--apply".to_string(),
            "--copy-noise".to_string(),
            "--only".to_string(),
            "a.png, b.png".to_string(),
        ];
        let p = parse_args(&args).unwrap();
        assert_eq!(p.catalog, PathBuf::from("my_cat.json"));
        assert_eq!(p.collection_media_dir, Some(PathBuf::from("/path/media")));
        assert!(p.apply);
        assert!(p.copy_noise);
        assert_eq!(p.only, vec!["a.png", "b.png"]);

        let p_eq = parse_args(&[
            "--catalog=cat2.csv".to_string(),
            "--collection-media-dir=/media2".to_string(),
            "--only=c.png".to_string(),
        ])
        .unwrap();
        assert_eq!(p_eq.catalog, PathBuf::from("cat2.csv"));
        assert_eq!(p_eq.collection_media_dir, Some(PathBuf::from("/media2")));
        assert_eq!(p_eq.only, vec!["c.png"]);
        assert!(!p_eq.apply);
        assert!(!p_eq.copy_noise);

        // Missing catalog -> exit code 2
        let err_missing = parse_args(&["--apply".to_string()]).unwrap_err();
        assert_eq!(err_missing.0, 2);

        // Unknown option -> exit code 2
        let err_unknown = parse_args(&[
            "--catalog".to_string(),
            "c".to_string(),
            "--foo".to_string(),
        ])
        .unwrap_err();
        assert_eq!(err_unknown.0, 2);

        // Help -> exit code 0
        let err_help = parse_args(&["-h".to_string()]).unwrap_err();
        assert_eq!(err_help.0, 0);
    }

    #[test]
    fn test_slug_and_namespacing() {
        assert_eq!(
            normalize_slug("Tratado de Neurologia 2"),
            "tratado_de_neurologia_2"
        );
        assert_eq!(normalize_slug("123"), "doc_123");
        assert_eq!(normalize_slug(""), "deck");

        assert_eq!(build_namespaced("audit", "a.png"), "audit_a.png");
        assert_eq!(build_namespaced("audit", "diag 1.png"), "audit_diag_1.png");
        assert_eq!(build_namespaced("neuro", "neuro_ct.png"), "neuro_ct.png");
        assert_eq!(build_namespaced("", "ecg.png"), "ecg.png");
    }

    #[test]
    fn test_noise_detection() {
        assert!(is_noise_caption("Figura 2 da página 15").0);
        assert!(is_noise_caption("figura 1").0);
        assert!(is_noise_caption("Tabela 3 da página 40").0);
        assert!(is_noise_caption("Copyright 2024").0);
        assert!(is_noise_caption("").0);

        assert!(!is_noise_caption("Ressonância magnética mostrando lesão em substância branca").0);
    }

    #[test]
    fn test_security_validation() {
        assert!(validate_filename_security("../etc/passwd").is_err());
        assert!(validate_filename_security("/root/file.png").is_err());
        assert!(validate_filename_security("\\windows\\file.png").is_err());
        assert!(validate_filename_security("NUL").is_err());
        assert!(validate_filename_security("CON.png").is_err());
        assert!(validate_filename_security("safe_image.png").is_ok());
    }

    #[test]
    fn test_pipe_csv_filename_namespaced_noise() {
        let csv_data = "filename|namespaced|noise\nraw1.png|custom_ns1.png|false\nraw2.png|custom_ns2.png|true\n";
        let entries = parse_csv_catalog(csv_data, "fallback").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "raw1.png");
        assert_eq!(entries[0].namespaced, "custom_ns1.png");
        assert!(!entries[0].noise);
        assert_eq!(entries[1].filename, "raw2.png");
        assert_eq!(entries[1].namespaced, "custom_ns2.png");
        assert!(entries[1].noise);
    }

    #[test]
    fn test_json_catalog_parsing() {
        let json_data = r#"{
            "slug": "cardio",
            "items": [
                { "filename": "ecg.png", "caption": "Eletrocardiograma de repouso" },
                { "filename": "echo.png", "caption": "Figura 1 da página 2", "noise": true }
            ]
        }"#;
        let entries = parse_json_catalog(json_data, "default").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "ecg.png");
        assert_eq!(entries[0].namespaced, "cardio_ecg.png");
        assert!(!entries[0].noise);
        assert_eq!(entries[1].namespaced, "cardio_echo.png");
        assert!(entries[1].noise);
    }

    #[test]
    fn test_dry_run_no_copy_and_lock_side_effect() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        let img_path = src.join("brain.png");
        fs::write(&img_path, b"brain_data").unwrap();

        let cat_path = src.join("catalog.csv");
        fs::write(
            &cat_path,
            "filename|caption|slug\nbrain.png|Ressonância|neuro\n",
        )
        .unwrap();

        let res = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
        ])
        .unwrap();

        assert_eq!(res["status"], "OK");
        assert_eq!(res["copied"], 0);
        assert_eq!(res["missing"], 1);
        assert_eq!(res["synced"], 0);

        // Lock files are NOT created in dry-run
        assert!(!dst.join(PRIMARY_LOCK_FILE).exists());
        assert!(!dst.join(LEGACY_LOCK_FILE).exists());
        // Destination image was NOT copied
        assert!(!dst.join("neuro_brain.png").exists());
    }

    #[test]
    fn test_apply_successful_transactional_copy() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        fs::write(src.join("ecg.png"), b"ecg_data").unwrap();
        fs::write(src.join("eeg.png"), b"eeg_data").unwrap();

        let cat_path = src.join("catalog.csv");
        fs::write(
            &cat_path,
            "filename|caption|slug\necg.png|Eletrocardiograma|cardio\neeg.png|Eletroencefalograma|neuro\n",
        )
        .unwrap();

        let res = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--apply".to_string(),
        ])
        .unwrap();

        assert_eq!(res["status"], "OK");
        assert_eq!(res["copied"], 2);
        assert_eq!(res["synced"], 2);
        assert_eq!(res["missing"], 0);
        assert_eq!(res["sync_ratio"], 1.0);

        assert_eq!(fs::read(dst.join("cardio_ecg.png")).unwrap(), b"ecg_data");
        assert_eq!(fs::read(dst.join("neuro_eeg.png")).unwrap(), b"eeg_data");
    }

    #[test]
    fn test_collision_detection_blocks_copy() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        // Destination has old content
        fs::write(dst.join("neuro_fig1.png"), b"old_content").unwrap();
        // Source has new conflicting content
        fs::write(src.join("fig1.png"), b"new_conflicting_content").unwrap();
        // Source has fig2 (missing in dst)
        fs::write(src.join("fig2.png"), b"fig2_content").unwrap();

        let cat_path = src.join("catalog.csv");
        fs::write(
            &cat_path,
            "filename|caption|slug\nfig1.png|Fig 1|neuro\nfig2.png|Fig 2|neuro\n",
        )
        .unwrap();

        let err = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--apply".to_string(),
        ])
        .unwrap_err();

        assert_eq!(err.0, 1);
        let err_val: Value = serde_json::from_str(&err.1).unwrap();
        assert_eq!(err_val["code"], "MEDIA.COLLISION");
        assert_eq!(err_val["copied"], 0);

        // fig2 was NOT copied
        assert!(!dst.join("neuro_fig2.png").exists());
        // fig1 was NOT overwritten
        assert_eq!(
            fs::read(dst.join("neuro_fig1.png")).unwrap(),
            b"old_content"
        );
    }

    #[test]
    fn test_missing_source_blocks_apply() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        fs::write(src.join("a.png"), b"content_a").unwrap();
        // missing.png does not exist in src

        let cat_path = src.join("catalog.csv");
        fs::write(
            &cat_path,
            "filename|caption|slug\na.png|Valida|audit\nmissing.png|Ausente|audit\n",
        )
        .unwrap();

        let err = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--apply".to_string(),
        ])
        .unwrap_err();

        assert_eq!(err.0, 1);
        let err_val: Value = serde_json::from_str(&err.1).unwrap();
        assert_eq!(err_val["code"], "MEDIA.SOURCE_MISSING");
        assert_eq!(err_val["copied"], 0);

        // a.png was NOT copied
        assert!(!dst.join("audit_a.png").exists());
    }

    #[test]
    fn test_same_byte_skip() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        fs::write(src.join("a.png"), b"identical").unwrap();
        fs::write(dst.join("audit_a.png"), b"identical").unwrap();

        let cat_path = src.join("catalog.csv");
        fs::write(&cat_path, "filename|caption|slug\na.png|Valida|audit\n").unwrap();

        let res = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--apply".to_string(),
        ])
        .unwrap();

        assert_eq!(res["status"], "OK");
        assert_eq!(res["synced"], 1);
        assert_eq!(res["copied"], 0);
        assert_eq!(res["missing"], 0);
        assert_eq!(res["sync_ratio"], 1.0);
    }

    #[test]
    fn test_help_exit_zero_and_dry_run_flag() {
        let err_short = parse_args(&["-h".to_string()]).unwrap_err();
        assert_eq!(err_short.0, 0);
        assert!(err_short.1.contains("Usage:"));
        assert!(err_short.1.contains("--apply | --dry-run"));

        let err_long = parse_args(&["--help".to_string()]).unwrap_err();
        assert_eq!(err_long.0, 0);

        let run_help = run(&["--help".to_string()]).unwrap();
        assert_eq!(run_help["status"], "help");
        assert!(run_help["usage"].as_str().unwrap().contains("Usage:"));

        let dry_args = parse_args(&[
            "--catalog".to_string(),
            "cat.json".to_string(),
            "--dry-run".to_string(),
        ])
        .unwrap();
        assert!(!dry_args.apply);

        let conflict = parse_args(&[
            "--catalog".to_string(),
            "cat.json".to_string(),
            "--apply".to_string(),
            "--dry-run".to_string(),
        ])
        .unwrap_err();
        assert_eq!(conflict.0, 2);
        assert_eq!(conflict.1, "Cannot specify both --apply and --dry-run");
    }

    #[test]
    fn test_dry_run_creates_no_lock_files() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        let img_path = src.join("heart.png");
        fs::write(&img_path, b"heart_bytes").unwrap();

        let cat_path = src.join("catalog.json");
        fs::write(
            &cat_path,
            r#"{"slug":"cardio","items":[{"filename":"heart.png","caption":"Coração"}]}"#,
        )
        .unwrap();

        let res = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--dry-run".to_string(),
        ])
        .unwrap();

        assert_eq!(res["status"], "OK");
        assert_eq!(res["copied"], 0);
        assert!(!dst.join(PRIMARY_LOCK_FILE).exists());
        assert!(!dst.join(LEGACY_LOCK_FILE).exists());
    }

    #[test]
    fn test_legacy_lock_contention_propagates_gov_lock() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        fs::create_dir(&src).unwrap();
        fs::create_dir(&dst).unwrap();

        let img_path = src.join("echo.png");
        fs::write(&img_path, b"echo_bytes").unwrap();

        let cat_path = src.join("catalog.json");
        fs::write(
            &cat_path,
            r#"{"slug":"cardio","items":[{"filename":"echo.png","caption":"Ecocardiograma"}]}"#,
        )
        .unwrap();

        let legacy_lock_path = dst.join(LEGACY_LOCK_FILE);
        let legacy_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&legacy_lock_path)
            .unwrap();
        legacy_file.lock_exclusive().unwrap();

        let err = run(&[
            "--catalog".to_string(),
            cat_path.to_str().unwrap().to_string(),
            "--collection-media-dir".to_string(),
            dst.to_str().unwrap().to_string(),
            "--apply".to_string(),
        ])
        .unwrap_err();

        assert_eq!(err.0, 1);
        let err_val: Value = serde_json::from_str(&err.1).unwrap();
        assert_eq!(err_val["code"], "GOV.LOCK");
        assert!(
            err_val["message"]
                .as_str()
                .unwrap()
                .contains("legacy lock active")
        );
    }

    #[test]
    fn test_rollback_missing_uses_to_copy_not_all_entries() {
        let entries = vec![
            CatalogEntry {
                filename: "f1.png".to_string(),
                namespaced: "cardio_f1.png".to_string(),
                caption: "Fig 1".to_string(),
                ..Default::default()
            },
            CatalogEntry {
                filename: "f2.png".to_string(),
                namespaced: "cardio_f2.png".to_string(),
                caption: "Fig 2".to_string(),
                ..Default::default()
            },
            CatalogEntry {
                filename: "f3.png".to_string(),
                namespaced: "cardio_f3.png".to_string(),
                caption: "Fig 3".to_string(),
                ..Default::default()
            },
        ];

        let to_copy = vec![(
            PathBuf::from("/src/f1.png"),
            PathBuf::from("/dst/cardio_f1.png"),
            "cardio_f1.png".to_string(),
        )];

        let existing_missing = vec![json!({
            "namespaced": "cardio_f3.png",
            "caption": "Fig 3",
        })];

        let rollback_missing = collect_rollback_missing(&to_copy, &entries, &existing_missing);

        let names: Vec<&str> = rollback_missing
            .iter()
            .filter_map(|m| m.get("namespaced").and_then(|v| v.as_str()))
            .collect();

        assert_eq!(names.len(), 2);
        assert!(names.contains(&"cardio_f1.png"));
        assert!(names.contains(&"cardio_f3.png"));
        assert!(!names.contains(&"cardio_f2.png"));
    }

    #[test]
    fn test_markdown_catalog_parser_and_sibling_json_promotion() {
        let md_content = r#"
| Arquivo | Legenda | Slug | Noise |
| :--- | :--- | :--- | :--- |
| ![img](ct_brain.png) | Tomografia de crânio | neuro | false |
| `angio.png` | Figura 1 da página 5 | vascular | |
| ![img](<xray.png>) | Radiografia de tórax | pneumo | true |
"#;
        let entries = parse_markdown_catalog(md_content, "default").unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].filename, "ct_brain.png");
        assert_eq!(entries[0].namespaced, "neuro_ct_brain.png");
        assert_eq!(entries[0].caption, "Tomografia de crânio");
        assert!(!entries[0].noise);

        assert_eq!(entries[1].filename, "angio.png");
        assert_eq!(entries[1].namespaced, "vascular_angio.png");
        assert!(entries[1].noise); // Detected via auto_generated_caption

        assert_eq!(entries[2].filename, "xray.png");
        assert_eq!(entries[2].namespaced, "pneumo_xray.png");
        assert!(entries[2].noise); // Explicit noise column

        // Sibling promotion
        let dir = tempdir().unwrap();
        let md_path = dir.path().join("catalog.md");
        let json_path = dir.path().join("catalog.json");

        fs::write(
            &md_path,
            "| Arquivo | Legenda |\n| --- | --- |\n| md_img.png | Da MD |\n",
        )
        .unwrap();
        fs::write(
            &json_path,
            r#"{"slug":"promoted","items":[{"filename":"json_img.png","caption":"Do JSON"}]}"#,
        )
        .unwrap();

        let loaded_promoted = load_catalog(&md_path).unwrap();
        assert_eq!(loaded_promoted.len(), 1);
        assert_eq!(loaded_promoted[0].filename, "json_img.png");

        // When sibling json does not exist, md is parsed
        fs::remove_file(&json_path).unwrap();
        let loaded_md = load_catalog(&md_path).unwrap();
        assert_eq!(loaded_md.len(), 1);
        assert_eq!(loaded_md[0].filename, "md_img.png");
    }

    #[test]
    fn test_security_validation_hardened() {
        // Empty / whitespace / dots
        assert!(validate_filename_security("").is_err());
        assert!(validate_filename_security("   ").is_err());
        assert!(validate_filename_security(".").is_err());
        assert!(validate_filename_security("..").is_err());

        // Path separators / traversal
        assert!(validate_filename_security("foo/bar.png").is_err());
        assert!(validate_filename_security("foo\\bar.png").is_err());
        assert!(validate_filename_security("dir/..").is_err());

        // Windows drive prefix
        assert!(validate_filename_security("C:image.png").is_err());
        assert!(validate_filename_security("d:file.jpg").is_err());

        // Windows forbidden chars
        for ch in ['<', '>', ':', '"', '|', '?', '*'] {
            let bad = format!("file{ch}name.png");
            assert!(
                validate_filename_security(&bad).is_err(),
                "char '{ch}' should be rejected"
            );
        }
        assert!(validate_filename_security("file\u{0007}.png").is_err());

        // Trailing space or dot
        assert!(validate_filename_security("image.png ").is_err());
        assert!(validate_filename_security("image.png.").is_err());

        // Reserved DOS device names
        assert!(validate_filename_security("aux").is_err());
        assert!(validate_filename_security("prn.txt").is_err());
        assert!(validate_filename_security("com1.png").is_err());
        assert!(validate_filename_security("lpt3.jpg").is_err());
        assert!(validate_filename_security("NUL.dat").is_err());
        assert!(validate_filename_security("con .txt").is_err());

        // Valid
        assert!(validate_filename_security("normal_image-01.png").is_ok());
        assert!(validate_filename_security("ECG_12_leads.jpeg").is_ok());
    }
}
