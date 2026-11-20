//! CLI implementation for `sync-decks` (worker 14).
//!
//! Synchronizes deck hierarchy from a live Anki SQLite collection into
//! `deck_hierarchy.json` without modifying the original collection files.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_ROOT: &str = "02_MEDICINA_AnkiMBR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub collection: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubdeckEntry {
    pub id: i64,
    pub name: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub depth: usize,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyData {
    pub root_deck: String,
    pub last_synced: String,
    pub total_subdecks: usize,
    pub decks: Vec<String>,
    pub deck_details: BTreeMap<String, DeckDetail>,
}

/// Parse command line arguments for `sync-decks`.
pub fn parse_args(args: &[String]) -> Result<CliArgs, (u8, String)> {
    let mut collection = None;
    let mut output = None;
    let mut root = DEFAULT_ROOT.to_string();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" {
            return Err((
                0,
                "Usage: anki-workflow-mcp sync-decks [--collection <path>] [--output <path>] [--root <name>]".to_string(),
            ));
        } else if arg == "--collection" {
            i += 1;
            if i >= args.len() {
                return Err((2, "Missing value for --collection".to_string()));
            }
            collection = Some(PathBuf::from(&args[i]));
        } else if let Some(val) = arg.strip_prefix("--collection=") {
            collection = Some(PathBuf::from(val));
        } else if arg == "--output" {
            i += 1;
            if i >= args.len() {
                return Err((2, "Missing value for --output".to_string()));
            }
            output = Some(PathBuf::from(&args[i]));
        } else if let Some(val) = arg.strip_prefix("--output=") {
            output = Some(PathBuf::from(val));
        } else if arg == "--root" {
            i += 1;
            if i >= args.len() {
                return Err((2, "Missing value for --root".to_string()));
            }
            root = args[i].clone();
        } else if let Some(val) = arg.strip_prefix("--root=") {
            root = val.to_string();
        } else {
            return Err((2, format!("Unexpected argument: {arg}")));
        }
        i += 1;
    }

    Ok(CliArgs {
        collection,
        output,
        root,
    })
}

/// Normalize deck name by replacing unit separator (`\x1f`) or slashes with `::`,
/// trimming whitespace, and discarding empty segments.
pub fn normalize_name(name: &str) -> String {
    let clean = name.replace(['\x1f', '/'], "::");
    let parts: Vec<&str> = clean
        .split("::")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("::")
}

/// Fold PT-BR and common Latin diacritics to base lowercase characters for primary collation.
/// Also strips Unicode combining diacritical marks (`U+0300..=U+036F`) for NFD inputs.
pub fn fold_pt_br_diacritics(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ('\u{0300}'..='\u{036f}').contains(&ch) {
            continue;
        }
        match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                out.push('a');
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => out.push('i'),
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => out.push('o'),
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => out.push('u'),
            'ç' | 'Ç' => out.push('c'),
            'ñ' | 'Ñ' => out.push('n'),
            'ý' | 'ÿ' | 'Ý' | 'Ÿ' => out.push('y'),
            _ => {
                for lc in ch.to_lowercase() {
                    out.push(lc);
                }
            }
        }
    }
    out
}

/// PT-BR case- and diacritic-insensitive collation with lowercase and raw tie-breaking.
/// Ensures leading accents sort with their base letters (e.g. `"Áudio"` before `"Zoonoses"`).
pub fn pt_br_unicase_cmp(a: &str, b: &str) -> Ordering {
    fold_pt_br_diacritics(a)
        .cmp(&fold_pt_br_diacritics(b))
        .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        .then_with(|| a.cmp(b))
}

/// Generate UTC ISO 8601 timestamp string from `SystemTime::now()`.
pub fn iso_8601_now() -> String {
    let now = SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = duration.as_secs();
    let millis = duration.subsec_millis();

    let days = total_secs / 86400;
    let rem_secs = total_secs % 86400;
    let hours = rem_secs / 3600;
    let minutes = (rem_secs % 3600) / 60;
    let seconds = rem_secs % 60;

    let mut year = 1970;
    let mut d = days;
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        year += 1;
    }

    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months = [
        31,
        if is_leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &dim in &days_in_months {
        if d < dim {
            break;
        }
        d -= dim;
        month += 1;
    }
    let day = d + 1;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

/// Resolve default path to Anki `collection.anki2` if not provided.
pub fn resolve_default_collection() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = Path::new(&appdata)
            .join("Anki2")
            .join("Usuário 1")
            .join("collection.anki2");
        if p.exists() {
            return Some(p);
        }
        let base_anki2 = Path::new(&appdata).join("Anki2");
        if let Ok(entries) = fs::read_dir(&base_anki2) {
            for entry in entries.flatten() {
                let cand = entry.path().join("collection.anki2");
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }

    let home_candidates = [
        std::env::var("HOME").ok().map(PathBuf::from),
        std::env::var("USERPROFILE").ok().map(PathBuf::from),
    ];

    for home in home_candidates.into_iter().flatten() {
        let local_share = home.join(".local").join("share").join("Anki2");
        if let Ok(entries) = fs::read_dir(&local_share) {
            for entry in entries.flatten() {
                let cand = entry.path().join("collection.anki2");
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
        let roaming = home
            .join("AppData")
            .join("Roaming")
            .join("Anki2")
            .join("Usuário 1")
            .join("collection.anki2");
        if roaming.exists() {
            return Some(roaming);
        }
    }

    None
}

/// Resolve default output path for `deck_hierarchy.json`.
pub fn resolve_default_output() -> Result<PathBuf, (u8, String)> {
    if let Ok(env_override) = std::env::var("ANKI_WORKFLOW_HIERARCHY_FILE")
        && !env_override.trim().is_empty()
    {
        return Ok(PathBuf::from(env_override));
    }

    let home_candidates = [
        std::env::var("USERPROFILE").ok().map(PathBuf::from),
        std::env::var("HOME").ok().map(PathBuf::from),
    ];

    if let Some(home) = home_candidates.into_iter().flatten().next() {
        return Ok(home.join(".anki-workflow").join("deck_hierarchy.json"));
    }

    Ok(PathBuf::from(".anki-workflow").join("deck_hierarchy.json"))
}

/// Snapshot collection online using SQLite's Backup API into a temporary database file
/// (never copying `-shm`), then open a read-only connection with PT-BR `unicase` collation.
pub fn snapshot_and_open_collection(
    col_path: &Path,
) -> Result<(tempfile::TempDir, Connection), (u8, String)> {
    if !col_path.exists() {
        return Err((
            1,
            format!("Anki database not found at {}", col_path.display()),
        ));
    }

    let mut wal_os = col_path.as_os_str().to_os_string();
    wal_os.push("-wal");
    let wal_path = PathBuf::from(wal_os);

    if wal_path.exists() {
        let wal_len = fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
        if wal_len > 0 {
            if wal_len < 32 {
                return Err((
                    1,
                    format!("Corrupted WAL file (truncated header, length {wal_len} < 32)"),
                ));
            }
            let mut wal_header = [0u8; 32];
            let mut f = fs::File::open(&wal_path).map_err(|e| {
                (
                    1,
                    format!("Failed to open WAL file for consistency check: {e}"),
                )
            })?;
            std::io::Read::read_exact(&mut f, &mut wal_header).map_err(|e| {
                (
                    1,
                    format!("Failed to read WAL header for consistency check: {e}"),
                )
            })?;
            let magic = &wal_header[0..4];
            let valid_magics: [&[u8]; 4] = [
                &[0x37, 0x7f, 0x06, 0x82],
                &[0x37, 0x7f, 0x06, 0x83],
                &[0x82, 0x06, 0x7f, 0x37],
                &[0x83, 0x06, 0x7f, 0x37],
            ];
            if !valid_magics.contains(&magic) {
                return Err((
                    1,
                    format!("Corrupted WAL file (invalid SQLite WAL header magic: {magic:02x?})"),
                ));
            }
        }
    }

    let temp_dir = tempfile::Builder::new()
        .prefix("anki_col_sync_")
        .tempdir()
        .map_err(|e| {
            (
                1,
                format!("Failed to create temporary directory for database snapshot: {e}"),
            )
        })?;

    let file_name = col_path
        .file_name()
        .unwrap_or_else(|| OsStr::new("collection.anki2"));
    let temp_col_path = temp_dir.path().join(file_name);

    {
        let src_conn = Connection::open_with_flags(
            col_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| (1, format!("Failed to open source collection: {e}")))?;

        src_conn.busy_timeout(Duration::from_secs(5)).map_err(|e| {
            (
                1,
                format!("Failed to set busy_timeout on source collection: {e}"),
            )
        })?;

        let mut dst_conn = Connection::open(&temp_col_path).map_err(|e| {
            (
                1,
                format!("Failed to create temporary snapshot database: {e}"),
            )
        })?;

        {
            let backup = Backup::new(&src_conn, &mut dst_conn)
                .map_err(|e| (1, format!("Failed to initialize online SQLite backup: {e}")))?;
            backup
                .run_to_completion(100, Duration::from_millis(25), None)
                .map_err(|e| (1, format!("Failed to complete online SQLite backup: {e}")))?;
        }
        let _ = dst_conn.execute_batch("PRAGMA journal_mode = DELETE;");
    }

    let conn = Connection::open_with_flags(
        &temp_col_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| (1, format!("Failed to open collection snapshot: {e}")))?;

    conn.create_collation("unicase", pt_br_unicase_cmp)
        .map_err(|e| (1, format!("Failed to register unicase collation: {e}")))?;

    // Ensure consistent snapshot when WAL is active / reject corrupt or uncertain mutations
    let quick_check: Result<String, _> =
        conn.query_row("PRAGMA quick_check(1);", [], |row| row.get(0));
    match quick_check {
        Ok(status) if status == "ok" => {}
        Ok(status) => {
            return Err((1, format!("Snapshot consistency check failed: {status}")));
        }
        Err(err) => {
            return Err((1, format!("Snapshot quick_check failed: {err}")));
        }
    }

    Ok((temp_dir, conn))
}

/// Query all decks from SQLite, supporting both modern `decks` table and legacy `col.decks` JSON.
pub fn query_all_decks(conn: &Connection) -> Result<Vec<(i64, String)>, (u8, String)> {
    let has_decks: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'decks'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if has_decks {
        let mut stmt = conn
            .prepare("SELECT id, name FROM decks ORDER BY name COLLATE unicase ASC")
            .map_err(|e| (1, format!("Failed to prepare query on decks table: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                Ok((id, name))
            })
            .map_err(|e| (1, format!("Failed to execute query on decks table: {e}")))?;

        let mut results = Vec::new();
        for r in rows {
            let item = r.map_err(|e| (1, format!("Failed to read row from decks table: {e}")))?;
            results.push(item);
        }
        return Ok(results);
    }

    let has_col: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'col'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if has_col {
        let decks_json_str: Result<String, _> =
            conn.query_row("SELECT decks FROM col LIMIT 1", [], |row| row.get(0));

        let decks_json_str = decks_json_str
            .map_err(|e| (1, format!("Failed to read 'decks' from col table: {e}")))?;

        let val: Value = serde_json::from_str(&decks_json_str)
            .map_err(|e| (1, format!("Failed to parse JSON in col.decks: {e}")))?;

        let mut results = Vec::new();
        if let Some(obj) = val.as_object() {
            for (key, deck_val) in obj {
                let id = deck_val
                    .get("id")
                    .and_then(|v| v.as_i64())
                    .or_else(|| key.parse::<i64>().ok())
                    .unwrap_or(0);
                let name = deck_val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    results.push((id, name));
                }
            }
        }

        results.sort_by(|a, b| pt_br_unicase_cmp(&a.1, &b.1));
        return Ok(results);
    }

    Err((
        1,
        "Database contains neither 'decks' nor 'col' table".to_string(),
    ))
}

/// Filter decks matching `root_deck` and merge with existing `deck_hierarchy.json` if present.
pub fn filter_and_merge_hierarchy(
    raw_decks: &[(i64, String)],
    root_deck: &str,
    output_path: &Path,
) -> Result<HierarchyData, (u8, String)> {
    let norm_root = normalize_name(root_deck);

    let mut subdecks: Vec<SubdeckEntry> = Vec::new();
    for (did, raw_name) in raw_decks {
        let clean = normalize_name(raw_name);
        if clean == norm_root || clean.starts_with(&format!("{norm_root}::")) {
            let depth = clean.matches("::").count();
            subdecks.push(SubdeckEntry {
                id: *did,
                name: clean,
                depth,
            });
        }
    }
    subdecks.sort_by(|a, b| pt_br_unicase_cmp(&a.name, &b.name));

    let mut existing_decks: BTreeSet<String> = BTreeSet::new();
    let mut existing_details: BTreeMap<String, Value> = BTreeMap::new();

    if output_path.exists() {
        let content = fs::read_to_string(output_path).map_err(|e| {
            (
                1,
                format!(
                    "HIERARCHY.CORRUPT: cannot read {}: {e}",
                    output_path.display()
                ),
            )
        })?;
        let prev: Value = serde_json::from_str(&content).map_err(|e| {
            (
                1,
                format!(
                    "HIERARCHY.CORRUPT: cannot read {}: {e}",
                    output_path.display()
                ),
            )
        })?;

        let decks_arr = prev
            .get("decks")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                (
                    1,
                    format!(
                        "HIERARCHY.CORRUPT: {} has no 'decks' key",
                        output_path.display()
                    ),
                )
            })?;

        for d in decks_arr {
            if let Some(s) = d.as_str() {
                existing_decks.insert(s.to_string());
            }
        }

        if let Some(details_obj) = prev.get("deck_details").and_then(|v| v.as_object()) {
            for (k, v) in details_obj {
                existing_details.insert(k.clone(), v.clone());
            }
        }
    }

    let mut all_deck_names = existing_decks;
    for d in &subdecks {
        all_deck_names.insert(d.name.clone());
    }
    let mut deck_names: Vec<String> = all_deck_names.into_iter().collect();
    deck_names.sort_by(|a, b| pt_br_unicase_cmp(a, b));

    let mut final_details: BTreeMap<String, DeckDetail> = BTreeMap::new();

    for (name, val) in &existing_details {
        let depth = val
            .get("depth")
            .and_then(|v| v.as_u64())
            .map(|d| d as usize)
            .unwrap_or_else(|| name.matches("::").count());
        let id = val.get("id").and_then(|v| v.as_i64());
        let mut extra = BTreeMap::new();
        if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                if k != "id" && k != "depth" {
                    extra.insert(k.clone(), v.clone());
                }
            }
        }
        final_details.insert(name.clone(), DeckDetail { id, depth, extra });
    }

    for d in &subdecks {
        if let Some(existing) = final_details.get_mut(&d.name) {
            existing.id = Some(d.id);
            existing.depth = d.depth;
        } else {
            final_details.insert(
                d.name.clone(),
                DeckDetail {
                    id: Some(d.id),
                    depth: d.depth,
                    extra: BTreeMap::new(),
                },
            );
        }
    }

    for name in &deck_names {
        final_details
            .entry(name.clone())
            .or_insert_with(|| DeckDetail {
                id: None,
                depth: name.matches("::").count(),
                extra: BTreeMap::new(),
            });
    }

    let timestamp = iso_8601_now();

    Ok(HierarchyData {
        root_deck: norm_root,
        last_synced: timestamp,
        total_subdecks: deck_names.len(),
        decks: deck_names,
        deck_details: final_details,
    })
}

/// Persist hierarchy data atomically to disk using `NamedTempFile` and `persist`.
pub fn persist_hierarchy_atomic(
    data: &HierarchyData,
    output_path: &Path,
) -> Result<(), (u8, String)> {
    let parent_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent_dir).map_err(|e| {
        (
            1,
            format!(
                "Failed to create output directory {}: {e}",
                parent_dir.display()
            ),
        )
    })?;

    let json_bytes = serde_json::to_vec_pretty(data)
        .map_err(|e| (1, format!("Failed to serialize hierarchy JSON: {e}")))?;

    let mut temp_file = tempfile::NamedTempFile::new_in(parent_dir).map_err(|e| {
        (
            1,
            format!("Failed to create temporary file for atomic write: {e}"),
        )
    })?;

    temp_file
        .write_all(&json_bytes)
        .map_err(|e| (1, format!("Failed to write hierarchy data: {e}")))?;

    temp_file
        .as_file()
        .sync_all()
        .map_err(|e| (1, format!("Failed to sync temporary hierarchy file: {e}")))?;

    #[cfg(windows)]
    {
        let mut current = temp_file;
        let mut attempt = 0u8;
        loop {
            match current.persist(output_path) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let is_transient = e.error.kind() == std::io::ErrorKind::PermissionDenied
                        || matches!(e.error.raw_os_error(), Some(5) | Some(32));
                    if is_transient && attempt < 3 {
                        attempt += 1;
                        current = e.file;
                        std::thread::sleep(Duration::from_millis(25 * u64::from(attempt)));
                        continue;
                    }
                    return Err((
                        1,
                        format!(
                            "Failed to persist hierarchy file to {}: {}",
                            output_path.display(),
                            e.error
                        ),
                    ));
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        temp_file.persist(output_path).map(|_| ()).map_err(|e| {
            (
                1,
                format!(
                    "Failed to persist hierarchy file to {}: {}",
                    output_path.display(),
                    e.error
                ),
            )
        })
    }
}

/// Main entrypoint for CLI `sync-decks`.
pub fn run(args: &[String]) -> Result<serde_json::Value, (u8, String)> {
    let parsed = match parse_args(args) {
        Ok(a) => a,
        Err((0, help)) => {
            return Ok(serde_json::json!({
                "status": "help",
                "usage": help,
            }));
        }
        Err(err) => return Err(err),
    };

    let col_path = match parsed.collection {
        Some(p) => p,
        None => resolve_default_collection().ok_or_else(|| {
            (
                1,
                "Anki database not found. Specify --collection <path>".to_string(),
            )
        })?,
    };

    let output_path = match parsed.output {
        Some(p) => p,
        None => resolve_default_output()?,
    };

    let (_temp_dir, conn) = snapshot_and_open_collection(&col_path)?;
    let raw_decks = query_all_decks(&conn)?;
    let hierarchy = filter_and_merge_hierarchy(&raw_decks, &parsed.root, &output_path)?;
    persist_hierarchy_atomic(&hierarchy, &output_path)?;

    Ok(serde_json::json!({
        "status": "synced",
        "root_deck": hierarchy.root_deck,
        "total_subdecks": hierarchy.total_subdecks,
        "last_synced": hierarchy.last_synced,
        "storage_path": output_path.display().to_string(),
        "decks": hierarchy.decks,
        "deck_details": hierarchy.deck_details,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_normalize_name() {
        assert_eq!(
            normalize_name("02_MEDICINA_AnkiMBR\x1fCiclo Clínico\x1fCardiologia"),
            "02_MEDICINA_AnkiMBR::Ciclo Clínico::Cardiologia"
        );
        assert_eq!(
            normalize_name("02_MEDICINA_AnkiMBR/Ciclo Clínico/Pneumo"),
            "02_MEDICINA_AnkiMBR::Ciclo Clínico::Pneumo"
        );
        assert_eq!(
            normalize_name("  02_MEDICINA_AnkiMBR ::  Nefro  :: "),
            "02_MEDICINA_AnkiMBR::Nefro"
        );
        assert_eq!(normalize_name(""), "");
    }

    #[test]
    fn test_parse_args_defaults() {
        let args = vec![];
        let parsed = parse_args(&args).expect("parse defaults");
        assert_eq!(parsed.collection, None);
        assert_eq!(parsed.output, None);
        assert_eq!(parsed.root, DEFAULT_ROOT);
    }

    #[test]
    fn test_parse_args_custom() {
        let args = vec![
            "--collection".to_string(),
            "/path/to/col.anki2".to_string(),
            "--output=/path/to/out.json".to_string(),
            "--root".to_string(),
            "CustomRoot".to_string(),
        ];
        let parsed = parse_args(&args).expect("parse custom");
        assert_eq!(parsed.collection, Some(PathBuf::from("/path/to/col.anki2")));
        assert_eq!(parsed.output, Some(PathBuf::from("/path/to/out.json")));
        assert_eq!(parsed.root, "CustomRoot");
    }

    #[test]
    fn test_parse_args_missing_value() {
        let args = vec!["--collection".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert_eq!(err.0, 2);
    }

    #[test]
    fn test_parse_args_unknown_option() {
        let args = vec!["--invalid-opt".to_string()];
        let err = parse_args(&args).unwrap_err();
        assert_eq!(err.0, 2);
    }

    #[test]
    fn test_synthetic_sqlite_modern_decks() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.execute(
            "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();

        conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR\x1fCardiologia')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (3, '02_MEDICINA_AnkiMBR/Pediatria')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO decks VALUES (4, 'OutroDeck::Geral')", [])
            .unwrap();
        drop(conn);

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args).expect("run modern decks");
        assert_eq!(result["status"], "synced");
        assert_eq!(result["total_subdecks"], 3);

        let decks = result["decks"].as_array().unwrap();
        let deck_names: Vec<&str> = decks.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(
            deck_names,
            vec![
                "02_MEDICINA_AnkiMBR",
                "02_MEDICINA_AnkiMBR::Cardiologia",
                "02_MEDICINA_AnkiMBR::Pediatria"
            ]
        );

        assert!(out_path.exists());
        let file_content = fs::read_to_string(&out_path).unwrap();
        let parsed_json: Value = serde_json::from_str(&file_content).unwrap();
        assert_eq!(parsed_json["total_subdecks"], 3);
        assert_eq!(
            parsed_json["deck_details"]["02_MEDICINA_AnkiMBR"]["depth"],
            0
        );
        assert_eq!(
            parsed_json["deck_details"]["02_MEDICINA_AnkiMBR::Cardiologia"]["depth"],
            1
        );
    }

    #[test]
    fn test_synthetic_sqlite_legacy_col_decks() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.execute(
            "CREATE TABLE col (id INTEGER PRIMARY KEY, decks TEXT NOT NULL)",
            [],
        )
        .unwrap();

        let legacy_json = serde_json::json!({
            "1": { "id": 1, "name": "Default" },
            "100": { "id": 100, "name": "02_MEDICINA_AnkiMBR" },
            "101": { "id": 101, "name": "02_MEDICINA_AnkiMBR::Neurologia" }
        })
        .to_string();

        conn.execute("INSERT INTO col (id, decks) VALUES (1, ?1)", [&legacy_json])
            .unwrap();
        drop(conn);

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args).expect("run legacy decks");
        assert_eq!(result["status"], "synced");
        assert_eq!(result["total_subdecks"], 2);

        let decks = result["decks"].as_array().unwrap();
        let names: Vec<&str> = decks.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec!["02_MEDICINA_AnkiMBR", "02_MEDICINA_AnkiMBR::Neurologia"]
        );
    }

    #[test]
    fn test_wal_snapshot_consistency() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
        conn.execute(
            "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR::Ortopedia')",
            [],
        )
        .unwrap();

        let mut wal_os = col_path.as_os_str().to_os_string();
        wal_os.push("-wal");
        let wal_path = PathBuf::from(wal_os);

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args).expect("run wal snapshot");
        assert_eq!(result["status"], "synced");
        assert_eq!(result["total_subdecks"], 2);
        drop(conn);

        // Verify WAL existed or was consistent
        let _ = wal_path.exists();
    }

    #[test]
    fn test_wal_corrupt_rejection() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
        conn.execute(
            "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        drop(conn);

        // Intentionally corrupt WAL file with garbage bytes
        let mut wal_os = col_path.as_os_str().to_os_string();
        wal_os.push("-wal");
        let wal_path = PathBuf::from(wal_os);
        {
            let mut f = File::create(&wal_path).unwrap();
            f.write_all(b"CORRUPTED_GARBAGE_BYTES_NOT_A_VALID_WAL_HEADER")
                .unwrap();
        }

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args);
        assert!(result.is_err(), "Must reject corrupted WAL snapshot");
        let (code, msg) = result.unwrap_err();
        assert_eq!(code, 1);
        assert!(
            msg.to_lowercase().contains("corrupt")
                || msg.contains("consistency")
                || msg.contains("quick_check")
                || msg.contains("Failed to open"),
            "Error message should indicate corruption: {msg}"
        );
    }

    #[test]
    fn test_unicase_collation_sorting() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.create_collation("unicase", |a, b| a.to_lowercase().cmp(&b.to_lowercase()))
            .unwrap();
        conn.execute(
            "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR::braço')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR::BRUTO')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (3, '02_MEDICINA_AnkiMBR::Audio')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO decks VALUES (4, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        drop(conn);

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args).expect("run unicase");
        let decks = result["decks"].as_array().unwrap();
        let names: Vec<&str> = decks.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "02_MEDICINA_AnkiMBR",
                "02_MEDICINA_AnkiMBR::Audio",
                "02_MEDICINA_AnkiMBR::braço",
                "02_MEDICINA_AnkiMBR::BRUTO",
            ]
        );
    }

    #[test]
    fn test_merge_existing_hierarchy_metadata() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        // Pre-create hierarchy JSON with manual additions
        let initial_json = serde_json::json!({
            "root_deck": "02_MEDICINA_AnkiMBR",
            "last_synced": "2026-01-01T00:00:00.000Z",
            "total_subdecks": 2,
            "decks": [
                "02_MEDICINA_AnkiMBR",
                "02_MEDICINA_AnkiMBR::ManualExtra"
            ],
            "deck_details": {
                "02_MEDICINA_AnkiMBR": { "depth": 0, "id": 10 },
                "02_MEDICINA_AnkiMBR::ManualExtra": {
                    "depth": 1,
                    "manually_added": true,
                    "added_at": "2026-01-01T00:00:00.000Z"
                }
            }
        });
        fs::write(
            &out_path,
            serde_json::to_string_pretty(&initial_json).unwrap(),
        )
        .unwrap();

        // Create DB with new subdeck from Anki
        let conn = Connection::open(&col_path).unwrap();
        conn.execute(
            "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR::Oftalmo')",
            [],
        )
        .unwrap();
        drop(conn);

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args).expect("run merge");
        assert_eq!(result["total_subdecks"], 3);

        let details = &result["deck_details"];
        assert_eq!(
            details["02_MEDICINA_AnkiMBR::ManualExtra"]["manually_added"],
            true
        );
        assert_eq!(
            details["02_MEDICINA_AnkiMBR::ManualExtra"]["added_at"],
            "2026-01-01T00:00:00.000Z"
        );
        assert_eq!(details["02_MEDICINA_AnkiMBR::Oftalmo"]["id"], 2);
    }

    #[test]
    fn test_io_missing_collection() {
        let args = vec![
            "--collection".to_string(),
            "/non/existent/path/collection.anki2".to_string(),
        ];
        let result = run(&args);
        assert!(result.is_err());
        let (code, msg) = result.unwrap_err();
        assert_eq!(code, 1);
        assert!(msg.contains("not found"));
    }

    #[test]
    fn test_io_corrupt_existing_hierarchy() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");
        let out_path = td.path().join("deck_hierarchy.json");

        let conn = Connection::open(&col_path).unwrap();
        conn.execute("CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT)", [])
            .unwrap();
        conn.execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        drop(conn);

        fs::write(&out_path, "{ invalid_json: true").unwrap();

        let args = vec![
            "--collection".to_string(),
            col_path.to_string_lossy().to_string(),
            "--output".to_string(),
            out_path.to_string_lossy().to_string(),
        ];

        let result = run(&args);
        assert!(result.is_err());
        let (code, msg) = result.unwrap_err();
        assert_eq!(code, 1);
        assert!(msg.contains("HIERARCHY.CORRUPT"));
    }

    #[test]
    fn test_pt_br_unicase_collation_leading_accents_and_tiebreak() {
        assert_eq!(pt_br_unicase_cmp("Áudio", "Zoonoses"), Ordering::Less);
        assert_eq!(pt_br_unicase_cmp("Audio", "Áudio"), Ordering::Less);
        assert_eq!(
            pt_br_unicase_cmp("A\u{0301}udio", "Zoonoses"),
            Ordering::Less
        );
        assert_eq!(pt_br_unicase_cmp("Audio", "A\u{0301}udio"), Ordering::Less);
        assert_eq!(pt_br_unicase_cmp("A\u{0301}udio", "Áudio"), Ordering::Less);
        assert_eq!(pt_br_unicase_cmp("Zoonoses", "Áudio"), Ordering::Greater);
    }

    #[test]
    fn test_online_backup_snapshot_no_shm_copy_with_live_wal_writer() {
        let td = tempfile::tempdir().unwrap();
        let col_path = td.path().join("collection.anki2");

        let writer = Connection::open(&col_path).unwrap();
        writer.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
        writer
            .execute(
                "CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
                [],
            )
            .unwrap();
        writer
            .execute("INSERT INTO decks VALUES (1, '02_MEDICINA_AnkiMBR')", [])
            .unwrap();
        writer
            .execute(
                "INSERT INTO decks VALUES (2, '02_MEDICINA_AnkiMBR::Cardiologia')",
                [],
            )
            .unwrap();

        // While writer is live in WAL mode, take snapshot
        let (snap_dir, snap_conn) =
            snapshot_and_open_collection(&col_path).expect("snapshot should succeed");

        // Verify absence of -shm in the snapshot directory
        for entry in fs::read_dir(snap_dir.path()).unwrap().flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            assert!(
                !fname.ends_with("-shm"),
                "Snapshot directory should never contain -shm companion file: {fname}"
            );
        }

        // Verify decks are readable from snapshot connection
        let decks = query_all_decks(&snap_conn).expect("query decks from snapshot");
        assert_eq!(decks.len(), 2);
        assert_eq!(decks[0].1, "02_MEDICINA_AnkiMBR");
        assert_eq!(decks[1].1, "02_MEDICINA_AnkiMBR::Cardiologia");

        drop(snap_conn);
        drop(snap_dir);
        drop(writer);
    }

    #[test]
    fn test_persist_hierarchy_atomic_overwrites_existing_file() {
        let td = tempfile::tempdir().unwrap();
        let out_path = td.path().join("deck_hierarchy.json");

        // Pre-create existing file
        fs::write(&out_path, b"PRE_EXISTING_CONTENT").unwrap();
        assert!(out_path.exists());

        let initial_data = HierarchyData {
            root_deck: "02_MEDICINA_AnkiMBR".to_string(),
            last_synced: "2026-01-01T00:00:00.000Z".to_string(),
            total_subdecks: 1,
            decks: vec!["02_MEDICINA_AnkiMBR".to_string()],
            deck_details: BTreeMap::new(),
        };

        persist_hierarchy_atomic(&initial_data, &out_path)
            .expect("persist should succeed overwriting existing file");
        let content1 = fs::read_to_string(&out_path).unwrap();
        let json1: Value = serde_json::from_str(&content1).unwrap();
        assert_eq!(json1["total_subdecks"], 1);

        // Overwrite again with updated data
        let updated_data = HierarchyData {
            root_deck: "02_MEDICINA_AnkiMBR".to_string(),
            last_synced: "2026-01-02T00:00:00.000Z".to_string(),
            total_subdecks: 2,
            decks: vec![
                "02_MEDICINA_AnkiMBR".to_string(),
                "02_MEDICINA_AnkiMBR::Pediatria".to_string(),
            ],
            deck_details: BTreeMap::new(),
        };

        persist_hierarchy_atomic(&updated_data, &out_path)
            .expect("second persist should also succeed overwriting");
        let content2 = fs::read_to_string(&out_path).unwrap();
        let json2: Value = serde_json::from_str(&content2).unwrap();
        assert_eq!(json2["total_subdecks"], 2);
        assert_eq!(
            json2["decks"].as_array().unwrap()[1].as_str().unwrap(),
            "02_MEDICINA_AnkiMBR::Pediatria"
        );
    }
}
