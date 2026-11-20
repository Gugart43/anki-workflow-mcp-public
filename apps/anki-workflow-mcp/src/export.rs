//! Atomic export implementation for Anki QA and Cloze reviewed batches.
//!
//! Enforces:
//! - Exact receipt hash integrity verification (`sha256(b"qa\0" + qa_csv + b"\0cloze\0" + cloze_csv)`);
//! - Exact evaluation verdict requirement (`PASS` exact);
//! - Fail-closed revalidation of CSV structure via `anki_validate::check_csv` and unsafe HTML via `anki_validate::prepare_quality`;
//! - Rejection of path traversal (`..`), invalid slug characters, Windows/POSIX reserved filenames, and symlink targets;
//! - Cross-process exclusive file locking (`fs2::FileExt::try_lock_exclusive` with bounded timeout) spanning ledger read, idempotency check, and write/swap;
//! - Same-filesystem staging and backup paths;
//! - RAII reverse rollback in `AtomicSwapGuard::drop` preserving recovery backups if restoration fails;
//! - Idempotent replay recording and verification of actual mirror configuration and existing on-disk files/hashes;
//! - Prevention of topic filename collisions across different batch IDs.
//!
//! Note on multi-file atomicity: Each individual file swap is atomic on POSIX/Windows filesystems,
//! but multi-file batches are coordinated via reverse RAII rollback in the event of an in-process error.
//! Cross-file crash atomicity (e.g. sudden power loss midway through swapping multiple files) is not
//! guaranteed by standard filesystem APIs without transaction support.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const EXPORT_STATE_FILE: &str = ".anki-workflow-export-state.json";
pub const EXPORT_LOCK_FILE: &str = ".anki_workflow_export.lock";
pub const EXPORT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const EXPORT_LOCK_POLL_INTERVAL: Duration = Duration::from_millis(25);

static NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn default_true() -> bool {
    true
}

/// Arguments for `export_reviewed_files`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportReviewedFilesArgs {
    pub deck_dir: String,
    pub batch_id: String,
    pub qa_csv: String,
    pub cloze_csv: String,
    pub receipt_hash: String,
    #[serde(default)]
    pub topic_slug: Option<String>,
    #[serde(default = "default_true")]
    pub write_to_deck_root: bool,
    pub evaluation_verdict: String,
}

/// Result of `export_reviewed_files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExportReviewedFilesResult {
    pub status: ExportStatus,
    pub batch_id: String,
    pub receipt_hash: String,
    pub qa_file: String,
    pub cloze_file: String,
    pub root_mirror_written: bool,
    pub state_file: String,
}

/// Status of the export operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExportStatus {
    Exported,
    AlreadyExported,
}

/// Domain error for export operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportError {
    pub code: &'static str,
    pub message: String,
}

impl ExportError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ExportError {}

/// Ledger recording all exported batches in the deck's exports directory.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ExportStateLedger {
    pub batches: BTreeMap<String, ExportBatchRecord>,
}

/// Record of a single exported batch in the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportBatchRecord {
    pub state: String,
    pub receipt_hash: String,
    pub qa_file: String,
    pub cloze_file: String,
    pub exported_at_unix: u64,
    #[serde(default = "default_true")]
    pub root_mirror_written: bool,
    #[serde(default)]
    pub topic_slug: Option<String>,
}

/// Computes the exact 64-hex SHA-256 receipt hash over QA and Cloze CSV contents.
#[must_use]
pub fn compute_receipt_hash(qa_csv: &str, cloze_csv: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"qa\0");
    hasher.update(qa_csv.as_bytes());
    hasher.update(b"\0cloze\0");
    hasher.update(cloze_csv.as_bytes());
    hex::encode(hasher.finalize())
}

/// Returns true if the string is a valid slug (alphanumeric, underscores, hyphens, non-empty).
pub fn is_valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Checks whether a name matches Windows/POSIX reserved device names.
pub fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();
    matches!(
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
    )
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn check_not_symlink(path: &Path) -> Result<(), ExportError> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(ExportError::new(
            "EXPORT.SYMLINK_TARGET",
            format!("Symlink target is rejected: '{}'", path.display()),
        ));
    }
    Ok(())
}

fn is_lock_contended(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock
        || (err.raw_os_error().is_some()
            && err.raw_os_error() == fs2::lock_contended_error().raw_os_error())
}

fn acquire_exclusive_lock_with_timeout(
    lock_file: &File,
    timeout: Duration,
) -> Result<(), ExportError> {
    let deadline = Instant::now() + timeout;
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(err) if is_lock_contended(&err) => {
                let now = Instant::now();
                if now >= deadline {
                    return Err(ExportError::new(
                        "EXPORT.LOCK_TIMEOUT",
                        format!(
                            "Timed out after {}ms waiting for exclusive export lock",
                            timeout.as_millis()
                        ),
                    ));
                }
                let remaining = deadline.saturating_duration_since(now);
                std::thread::sleep(EXPORT_LOCK_POLL_INTERVAL.min(remaining));
            }
            Err(err) => {
                return Err(ExportError::new(
                    "EXPORT.LOCK_ERROR",
                    format!("Failed to acquire exclusive cross-process lock: {err}"),
                ));
            }
        }
    }
}

fn cross_device_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    let dst_dir = dst
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let seq = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dst_name = dst.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp_path = dst_dir.join(format!(".tmp_xdev_{dst_name}_{}_{seq}", std::process::id()));

    let copy_res = (|| -> std::io::Result<()> {
        let mut reader = File::open(src)?;
        let mut writer = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        std::io::copy(&mut reader, &mut writer)?;
        writer.sync_all()?;
        Ok(())
    })();

    if let Err(err) = copy_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp_path, dst) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    let _ = fs::remove_file(src);
    Ok(())
}

fn replace_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {
            cross_device_replace(src, dst)
        }
        Err(err) => Err(err),
    }
}

struct SwappedEntry {
    target: PathBuf,
    backup: Option<PathBuf>,
}

/// RAII transaction guard for atomic staging, swapping, and reverse rollback.
pub struct AtomicSwapGuard {
    stage_dir: PathBuf,
    swapped: Vec<SwappedEntry>,
    committed: bool,
    nonce: u128,
    fail_after_swaps: Option<usize>,
}

impl AtomicSwapGuard {
    pub fn new(base_dir: &Path, batch_id: &str) -> std::io::Result<Self> {
        fs::create_dir_all(base_dir)?;
        let seq = NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let stage_dir = base_dir.join(format!(
            ".tmp_export_{batch_id}_{}_{nonce}_{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&stage_dir)?;
        Ok(Self {
            stage_dir,
            swapped: Vec::new(),
            committed: false,
            nonce,
            fail_after_swaps: None,
        })
    }

    #[cfg(test)]
    pub fn set_fail_after_swaps(&mut self, limit: usize) {
        self.fail_after_swaps = Some(limit);
    }

    pub fn stage_bytes(&self, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let staged_path = self.stage_dir.join(name);
        let mut file = File::create(&staged_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(staged_path)
    }

    pub fn swap_in(&mut self, staged: &Path, target: &Path) -> std::io::Result<()> {
        if let Some(limit) = self.fail_after_swaps
            && self.swapped.len() >= limit
        {
            return Err(std::io::Error::other("simulated swap failure for testing"));
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        let backup = if target.exists() || target.symlink_metadata().is_ok() {
            let name = target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            let backup_path = target.parent().unwrap_or(&self.stage_dir).join(format!(
                ".bak_{}_{}_{}_{}",
                self.swapped.len(),
                name,
                std::process::id(),
                self.nonce
            ));
            replace_file(target, &backup_path)?;
            Some(backup_path)
        } else {
            None
        };

        if let Err(err) = replace_file(staged, target) {
            if let Some(ref bak) = backup {
                let _ = replace_file(bak, target);
            }
            return Err(err);
        }

        self.swapped.push(SwappedEntry {
            target: target.to_path_buf(),
            backup,
        });
        Ok(())
    }

    pub fn commit(mut self) -> std::io::Result<()> {
        self.committed = true;
        for entry in &self.swapped {
            if let Some(ref bak) = entry.backup {
                let _ = fs::remove_file(bak);
            }
        }
        let _ = fs::remove_dir_all(&self.stage_dir);
        Ok(())
    }
}

impl Drop for AtomicSwapGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for entry in self.swapped.iter().rev() {
            match &entry.backup {
                Some(bak) => {
                    if let Err(err) = replace_file(bak, &entry.target) {
                        eprintln!(
                            "CRITICAL: Failed to restore backup '{}' to '{}': {err}. Recovery backup preserved on disk.",
                            bak.display(),
                            entry.target.display()
                        );
                    }
                }
                None => {
                    let _ = fs::remove_file(&entry.target);
                }
            }
        }
        let _ = fs::remove_dir_all(&self.stage_dir);
    }
}

/// Executes export of reviewed files with fail-closed security and cross-process serialization.
pub fn execute_export_reviewed_files(
    args: &ExportReviewedFilesArgs,
) -> Result<ExportReviewedFilesResult, ExportError> {
    // 1. Slug validation
    if !is_valid_slug(&args.batch_id) {
        return Err(ExportError::new(
            "EXPORT.INVALID_BATCH_ID",
            format!("batch_id inválido '{}'", args.batch_id),
        ));
    }
    if is_reserved_name(&args.batch_id) {
        return Err(ExportError::new(
            "EXPORT.INVALID_BATCH_ID",
            format!("batch_id cannot be a reserved filename '{}'", args.batch_id),
        ));
    }

    if let Some(ref slug) = args.topic_slug {
        if !is_valid_slug(slug) {
            return Err(ExportError::new(
                "EXPORT.INVALID_TOPIC_SLUG",
                format!("topic_slug inválido '{slug}'"),
            ));
        }
        if is_reserved_name(slug) {
            return Err(ExportError::new(
                "EXPORT.INVALID_TOPIC_SLUG",
                format!("topic_slug cannot be a reserved filename '{slug}'"),
            ));
        }
    }

    // 2. Verdict exactness
    if args.evaluation_verdict != "PASS" {
        return Err(ExportError::new(
            "EXPORT.VALIDATION_FAIL",
            "Apenas verdict 'PASS' autoriza exportação.",
        ));
    }

    // 3. Receipt hash format (64-char hex) and exact content match
    if args.receipt_hash.len() != 64 || !args.receipt_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ExportError::new(
            "EXPORT.NO_RECEIPT",
            "receipt_hash must be a valid 64-character hex digest.",
        ));
    }
    let expected_hash = compute_receipt_hash(&args.qa_csv, &args.cloze_csv);
    if !args.receipt_hash.eq_ignore_ascii_case(&expected_hash) {
        return Err(ExportError::new(
            "EXPORT.NO_RECEIPT",
            "receipt_hash divergente do conteúdo revisado.",
        ));
    }

    // 4. CSV structure revalidation
    let qa_diags = anki_validate::check_csv(&args.qa_csv, anki_domain::NoteType::Qa, true);
    if let Some(err) = qa_diags
        .iter()
        .find(|d| d.severity == anki_domain::Severity::Critical)
    {
        return Err(ExportError::new(
            "EXPORT.INVALID_CSV",
            format!("QA CSV structure validation failed: {}", err.message),
        ));
    }

    let cloze_diags = anki_validate::check_csv(&args.cloze_csv, anki_domain::NoteType::Cloze, true);
    if let Some(err) = cloze_diags
        .iter()
        .find(|d| d.severity == anki_domain::Severity::Critical)
    {
        return Err(ExportError::new(
            "EXPORT.INVALID_CSV",
            format!("Cloze CSV structure validation failed: {}", err.message),
        ));
    }

    // 5. Unsafe HTML revalidation via quality pipeline
    let quality_input = anki_validate::QualityInput {
        qa_csv: args.qa_csv.clone(),
        cloze_csv: args.cloze_csv.clone(),
        mode: anki_validate::quality::QualityMode::Quick,
        sources: Vec::new(),
        obligations: Vec::new(),
        bindings: Vec::new(),
        reviews: Vec::new(),
    };
    let quality_report = anki_validate::prepare_quality(&quality_input);
    if let Some(diag) = quality_report
        .diagnostics
        .iter()
        .find(|d| d.code == "QUALITY.UNSAFE_HTML")
    {
        return Err(ExportError::new(
            "EXPORT.UNSAFE_HTML",
            format!(
                "Active HTML or unsafe attributes rejected: {}",
                diag.message
            ),
        ));
    }

    // 6. Path validation & Traversal check
    if args.deck_dir.trim().is_empty() {
        return Err(ExportError::new(
            "EXPORT.INVALID_PATH",
            "deck_dir cannot be empty",
        ));
    }
    let deck_dir = PathBuf::from(&args.deck_dir);
    if deck_dir
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(ExportError::new(
            "EXPORT.PATH_TRAVERSAL",
            "deck_dir contains path traversal ('..')",
        ));
    }

    check_not_symlink(&deck_dir)?;

    let exports_dir = deck_dir.join("exports");
    check_not_symlink(&exports_dir)?;
    if !exports_dir.exists() {
        fs::create_dir_all(&exports_dir).map_err(|e| {
            ExportError::new(
                "EXPORT.IO_ERROR",
                format!("Failed to create exports dir: {e}"),
            )
        })?;
    }

    // 7. Cross-process exclusive lock spanning ledger read, idempotency, and writes
    let lock_path = exports_dir.join(EXPORT_LOCK_FILE);
    check_not_symlink(&lock_path)?;
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| {
            ExportError::new(
                "EXPORT.LOCK_ERROR",
                format!("Failed to open export lock file: {e}"),
            )
        })?;

    acquire_exclusive_lock_with_timeout(&lock_file, EXPORT_LOCK_TIMEOUT)?;

    // 8. Target filenames and symlink checks
    let prefix = args.topic_slug.as_deref().unwrap_or(args.batch_id.as_str());
    let qa_filename = format!("{prefix}_QA.csv");
    let cloze_filename = format!("{prefix}_Cloze.csv");
    let qa_target = exports_dir.join(&qa_filename);
    let cloze_target = exports_dir.join(&cloze_filename);
    check_not_symlink(&qa_target)?;
    check_not_symlink(&cloze_target)?;

    let root_qa_target = deck_dir.join(&qa_filename);
    let root_cloze_target = deck_dir.join(&cloze_filename);
    if args.write_to_deck_root {
        check_not_symlink(&root_qa_target)?;
        check_not_symlink(&root_cloze_target)?;
    }

    // 9. Read state ledger
    let state_path = exports_dir.join(EXPORT_STATE_FILE);
    check_not_symlink(&state_path)?;

    let mut ledger: ExportStateLedger = if state_path.exists() {
        let raw = fs::read_to_string(&state_path).map_err(|e| {
            ExportError::new(
                "EXPORT.STATE_READ_ERROR",
                format!("Failed to read state: {e}"),
            )
        })?;
        serde_json::from_str(&raw).map_err(|e| {
            ExportError::new(
                "EXPORT.STATE_CORRUPT",
                format!("Failed to parse state file: {e}"),
            )
        })?
    } else {
        ExportStateLedger::default()
    };

    // 10. Idempotent replay check with verification of files and content hashes
    if let Some(existing) = ledger.batches.get(&args.batch_id) {
        if !existing.receipt_hash.eq_ignore_ascii_case(&expected_hash) {
            return Err(ExportError::new(
                "EXPORT.BATCH_CONFLICT",
                format!("Batch '{}' já exportado com outro hash.", args.batch_id),
            ));
        }
        if existing.topic_slug != args.topic_slug {
            return Err(ExportError::new(
                "EXPORT.BATCH_CONFLICT",
                format!(
                    "Batch '{}' já exportado com topic_slug diferente ({:?} vs {:?}).",
                    args.batch_id, existing.topic_slug, args.topic_slug
                ),
            ));
        }
        if existing.root_mirror_written != args.write_to_deck_root {
            return Err(ExportError::new(
                "EXPORT.BATCH_CONFLICT",
                format!(
                    "Batch '{}' já exportado com write_to_deck_root={} (solicitado {}).",
                    args.batch_id, existing.root_mirror_written, args.write_to_deck_root
                ),
            ));
        }
        if existing.state != "exported" {
            return Err(ExportError::new(
                "EXPORT.STATE_INVALID",
                format!(
                    "Batch '{}' in invalid state '{}'",
                    args.batch_id, existing.state
                ),
            ));
        }

        let qa_p = Path::new(&existing.qa_file);
        let cloze_p = Path::new(&existing.cloze_file);
        if !qa_p.exists() || !cloze_p.exists() {
            return Err(ExportError::new(
                "EXPORT.FILE_MISSING",
                format!(
                    "Exported file for batch '{}' missing on disk during replay",
                    args.batch_id
                ),
            ));
        }

        let actual_qa = fs::read_to_string(qa_p).map_err(|e| {
            ExportError::new(
                "EXPORT.IO_ERROR",
                format!("Failed to read QA file on replay: {e}"),
            )
        })?;
        let actual_cloze = fs::read_to_string(cloze_p).map_err(|e| {
            ExportError::new(
                "EXPORT.IO_ERROR",
                format!("Failed to read Cloze file on replay: {e}"),
            )
        })?;
        if actual_qa != args.qa_csv || actual_cloze != args.cloze_csv {
            return Err(ExportError::new(
                "EXPORT.CONTENT_ALTERED",
                "Exported file content on disk has been altered from original export",
            ));
        }

        if existing.root_mirror_written {
            let r_qa_p = deck_dir.join(&qa_filename);
            let r_cloze_p = deck_dir.join(&cloze_filename);
            if !r_qa_p.exists() || !r_cloze_p.exists() {
                return Err(ExportError::new(
                    "EXPORT.FILE_MISSING",
                    "Root mirror export file missing on disk during replay",
                ));
            }
            let r_qa = fs::read_to_string(&r_qa_p).map_err(|e| {
                ExportError::new(
                    "EXPORT.IO_ERROR",
                    format!("Failed to read root QA file: {e}"),
                )
            })?;
            let r_cloze = fs::read_to_string(&r_cloze_p).map_err(|e| {
                ExportError::new(
                    "EXPORT.IO_ERROR",
                    format!("Failed to read root Cloze file: {e}"),
                )
            })?;
            if r_qa != args.qa_csv || r_cloze != args.cloze_csv {
                return Err(ExportError::new(
                    "EXPORT.CONTENT_ALTERED",
                    "Root mirror file content on disk has been altered from original export",
                ));
            }
        }

        return Ok(ExportReviewedFilesResult {
            status: ExportStatus::AlreadyExported,
            batch_id: args.batch_id.clone(),
            receipt_hash: expected_hash,
            qa_file: existing.qa_file.clone(),
            cloze_file: existing.cloze_file.clone(),
            root_mirror_written: existing.root_mirror_written,
            state_file: state_path.display().to_string(),
        });
    }

    // 11. Prevent topic filename collisions across different batch IDs
    for (other_batch, record) in &ledger.batches {
        if other_batch != &args.batch_id
            && (record.qa_file == qa_target.display().to_string()
                || record.cloze_file == cloze_target.display().to_string())
        {
            return Err(ExportError::new(
                "EXPORT.TOPIC_COLLISION",
                format!(
                    "Topic filename collision: batch '{}' already exported to '{}'",
                    other_batch, qa_filename
                ),
            ));
        }
    }

    // Prevent overwriting existing untracked files on disk
    if qa_target.exists() || cloze_target.exists() {
        return Err(ExportError::new(
            "EXPORT.FILE_COLLISION",
            format!(
                "Target export file already exists on disk: {}",
                if qa_target.exists() {
                    &qa_filename
                } else {
                    &cloze_filename
                }
            ),
        ));
    }
    if args.write_to_deck_root && (root_qa_target.exists() || root_cloze_target.exists()) {
        return Err(ExportError::new(
            "EXPORT.FILE_COLLISION",
            format!(
                "Root target export file already exists on disk: {}",
                if root_qa_target.exists() {
                    &qa_filename
                } else {
                    &cloze_filename
                }
            ),
        ));
    }

    // 12. Record in ledger and stage
    ledger.batches.insert(
        args.batch_id.clone(),
        ExportBatchRecord {
            state: "exported".to_string(),
            receipt_hash: expected_hash.clone(),
            qa_file: qa_target.display().to_string(),
            cloze_file: cloze_target.display().to_string(),
            exported_at_unix: unix_now(),
            root_mirror_written: args.write_to_deck_root,
            topic_slug: args.topic_slug.clone(),
        },
    );

    let state_bytes = serde_json::to_vec_pretty(&ledger).map_err(|e| {
        ExportError::new(
            "EXPORT.SERIALIZE_ERROR",
            format!("Serialization error: {e}"),
        )
    })?;

    // 13. Staging and atomic swaps via RAII guard
    let mut guard = AtomicSwapGuard::new(&exports_dir, &args.batch_id)
        .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;

    let staged_qa = guard
        .stage_bytes("qa.csv", args.qa_csv.as_bytes())
        .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;
    let staged_cloze = guard
        .stage_bytes("cloze.csv", args.cloze_csv.as_bytes())
        .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;
    let staged_state = guard
        .stage_bytes("state.json", &state_bytes)
        .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;

    guard
        .swap_in(&staged_qa, &qa_target)
        .map_err(|e| ExportError::new("EXPORT.SWAP_ERROR", e.to_string()))?;
    guard
        .swap_in(&staged_cloze, &cloze_target)
        .map_err(|e| ExportError::new("EXPORT.SWAP_ERROR", e.to_string()))?;

    if args.write_to_deck_root {
        let s_rqa = guard
            .stage_bytes("root_qa.csv", args.qa_csv.as_bytes())
            .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;
        let s_rcl = guard
            .stage_bytes("root_cloze.csv", args.cloze_csv.as_bytes())
            .map_err(|e| ExportError::new("EXPORT.IO_ERROR", e.to_string()))?;
        guard
            .swap_in(&s_rqa, &root_qa_target)
            .map_err(|e| ExportError::new("EXPORT.SWAP_ERROR", e.to_string()))?;
        guard
            .swap_in(&s_rcl, &root_cloze_target)
            .map_err(|e| ExportError::new("EXPORT.SWAP_ERROR", e.to_string()))?;
    }

    guard
        .swap_in(&staged_state, &state_path)
        .map_err(|e| ExportError::new("EXPORT.SWAP_ERROR", e.to_string()))?;

    guard
        .commit()
        .map_err(|e| ExportError::new("EXPORT.COMMIT_ERROR", e.to_string()))?;

    Ok(ExportReviewedFilesResult {
        status: ExportStatus::Exported,
        batch_id: args.batch_id.clone(),
        receipt_hash: expected_hash,
        qa_file: qa_target.display().to_string(),
        cloze_file: cloze_target.display().to_string(),
        root_mirror_written: args.write_to_deck_root,
        state_file: state_path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const VALID_QA: &str = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\nSynthetic Q?|Synthetic A|Cardiologia||||med::cardio\n";
    const VALID_CLOZE: &str = "Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n{{c1::Synthetic}} test sentence.||Cardiologia|||med::cardio\n";

    fn create_test_args(dir: &Path, batch_id: &str) -> ExportReviewedFilesArgs {
        let receipt = compute_receipt_hash(VALID_QA, VALID_CLOZE);
        ExportReviewedFilesArgs {
            deck_dir: dir.to_str().unwrap().to_string(),
            batch_id: batch_id.to_string(),
            qa_csv: VALID_QA.to_string(),
            cloze_csv: VALID_CLOZE.to_string(),
            receipt_hash: receipt,
            topic_slug: None,
            write_to_deck_root: true,
            evaluation_verdict: "PASS".to_string(),
        }
    }

    #[test]
    fn test_verdict_pass_exact() {
        let tmp = TempDir::new().unwrap();
        let mut args = create_test_args(tmp.path(), "batch_v1");

        for bad in ["pass", "Pass", "FAIL", "PASS ", " INCOMPLETE", "PASS\n"] {
            args.evaluation_verdict = bad.to_string();
            let err = execute_export_reviewed_files(&args).unwrap_err();
            assert_eq!(err.code, "EXPORT.VALIDATION_FAIL");
        }

        args.evaluation_verdict = "PASS".to_string();
        let res = execute_export_reviewed_files(&args).unwrap();
        assert_eq!(res.status, ExportStatus::Exported);
    }

    #[test]
    fn test_receipt_hash_64hex_exact_content() {
        let tmp = TempDir::new().unwrap();
        let mut args = create_test_args(tmp.path(), "batch_hash");

        // Length not 64
        args.receipt_hash = "abcdef".to_string();
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.NO_RECEIPT");

        // 64 chars but non-hex
        args.receipt_hash = "z".repeat(64);
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.NO_RECEIPT");

        // Valid 64-hex format but wrong hash for content
        args.receipt_hash = "0".repeat(64);
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.NO_RECEIPT");

        // Exact hash succeeds
        args.receipt_hash = compute_receipt_hash(VALID_QA, VALID_CLOZE);
        let res = execute_export_reviewed_files(&args).unwrap();
        assert_eq!(res.status, ExportStatus::Exported);
    }

    #[test]
    fn test_csv_structural_revalidation() {
        let tmp = TempDir::new().unwrap();
        let mut args = create_test_args(tmp.path(), "batch_csv_err");

        // Broken QA: wrong column count
        args.qa_csv = "Enunciado|Resposta\nOnly two columns\n".to_string();
        args.receipt_hash = compute_receipt_hash(&args.qa_csv, &args.cloze_csv);
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.INVALID_CSV");

        // Broken Cloze: missing headers
        args.qa_csv = VALID_QA.to_string();
        args.cloze_csv = "{{c1::Sentence}}|omit\n".to_string();
        args.receipt_hash = compute_receipt_hash(&args.qa_csv, &args.cloze_csv);
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.INVALID_CSV");
    }

    #[test]
    fn test_unsafe_html_revalidation() {
        let tmp = TempDir::new().unwrap();
        let mut args = create_test_args(tmp.path(), "batch_html_err");

        // Unsafe active content in QA field
        let malicious_qa = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\nSynthetic Q?|<b onclick=\"alert(1)\">Malicious</b>|Cardiologia||||med::cardio\n".to_string();
        args.qa_csv = malicious_qa;
        args.receipt_hash = compute_receipt_hash(&args.qa_csv, &args.cloze_csv);
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.UNSAFE_HTML");
    }

    #[test]
    fn test_slug_traversal_and_reserved_names() {
        let tmp = TempDir::new().unwrap();

        // Invalid batch_id slug characters
        let mut args = create_test_args(tmp.path(), "batch/slash");
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.INVALID_BATCH_ID");

        // Reserved batch_id
        args.batch_id = "CON".to_string();
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.INVALID_BATCH_ID");

        // Reserved topic_slug
        args.batch_id = "valid_batch".to_string();
        args.topic_slug = Some("NUL".to_string());
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.INVALID_TOPIC_SLUG");

        // Traversal in deck_dir
        args.topic_slug = None;
        args.deck_dir = tmp.path().join("..").to_str().unwrap().to_string();
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.PATH_TRAVERSAL");
    }

    #[test]
    fn test_idempotent_replay_and_mirror_verification() {
        let tmp = TempDir::new().unwrap();
        let args = create_test_args(tmp.path(), "batch_replay");

        // First run -> Exported
        let res1 = execute_export_reviewed_files(&args).unwrap();
        assert_eq!(res1.status, ExportStatus::Exported);
        assert!(res1.root_mirror_written);
        assert!(Path::new(&res1.qa_file).exists());
        assert!(Path::new(&res1.cloze_file).exists());

        // Second run with identical args -> AlreadyExported
        let res2 = execute_export_reviewed_files(&args).unwrap();
        assert_eq!(res2.status, ExportStatus::AlreadyExported);
        assert_eq!(res1.qa_file, res2.qa_file);
        assert_eq!(res1.cloze_file, res2.cloze_file);
        assert!(res2.root_mirror_written);
    }

    #[test]
    fn test_conflict_different_hash_same_batch() {
        let tmp = TempDir::new().unwrap();
        let args1 = create_test_args(tmp.path(), "batch_conflict");
        execute_export_reviewed_files(&args1).unwrap();

        // Second run with different CSV content for the same batch_id
        let mut args2 = args1.clone();
        args2.qa_csv = "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\nDifferent Q?|Different A|Cardiologia||||med::cardio\n".to_string();
        args2.receipt_hash = compute_receipt_hash(&args2.qa_csv, &args2.cloze_csv);

        let err = execute_export_reviewed_files(&args2).unwrap_err();
        assert_eq!(err.code, "EXPORT.BATCH_CONFLICT");
    }

    #[test]
    fn test_prevent_topic_filename_collision_across_batches() {
        let tmp = TempDir::new().unwrap();

        // Batch 1 with topic_slug "cardio"
        let mut args1 = create_test_args(tmp.path(), "batch_alpha");
        args1.topic_slug = Some("cardio".to_string());
        execute_export_reviewed_files(&args1).unwrap();

        // Batch 2 attempting to write to the same topic_slug "cardio"
        let mut args2 = create_test_args(tmp.path(), "batch_beta");
        args2.topic_slug = Some("cardio".to_string());
        let err = execute_export_reviewed_files(&args2).unwrap_err();
        assert_eq!(err.code, "EXPORT.TOPIC_COLLISION");
    }

    #[test]
    fn test_missing_and_altered_exports_on_replay() {
        let tmp = TempDir::new().unwrap();
        let args = create_test_args(tmp.path(), "batch_tamper");
        let res = execute_export_reviewed_files(&args).unwrap();

        // Tamper 1: Delete QA file
        fs::remove_file(&res.qa_file).unwrap();
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.FILE_MISSING");

        // Recreate QA file but alter content
        fs::write(&res.qa_file, "altered content").unwrap();
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.CONTENT_ALTERED");
    }

    #[test]
    fn test_induced_swap_failure_restores_originals() {
        let tmp = TempDir::new().unwrap();
        let exports_dir = tmp.path().join("exports");
        fs::create_dir_all(&exports_dir).unwrap();

        let original_file = exports_dir.join("existing.txt");
        fs::write(&original_file, "ORIGINAL_CONTENT").unwrap();

        let mut guard = AtomicSwapGuard::new(&exports_dir, "batch_fail").unwrap();
        let staged_1 = guard.stage_bytes("s1.txt", b"NEW_CONTENT").unwrap();
        let staged_2 = guard.stage_bytes("s2.txt", b"NEW_S2").unwrap();

        // First swap succeeds (original_file backed up, NEW_CONTENT swapped in)
        guard.swap_in(&staged_1, &original_file).unwrap();
        assert_eq!(fs::read_to_string(&original_file).unwrap(), "NEW_CONTENT");

        // Induce failure on second swap
        guard.set_fail_after_swaps(1);
        let target_2 = exports_dir.join("second.txt");
        let swap_err = guard.swap_in(&staged_2, &target_2);
        assert!(swap_err.is_err());

        // Drop guard without commit -> triggers reverse rollback
        drop(guard);

        // Assert original_file restored to ORIGINAL_CONTENT
        assert_eq!(
            fs::read_to_string(&original_file).unwrap(),
            "ORIGINAL_CONTENT"
        );
        // Assert second.txt does not exist
        assert!(!target_2.exists());
    }

    #[test]
    fn test_concurrent_access_serialization() {
        use std::sync::Arc;
        use std::thread;

        let tmp = Arc::new(TempDir::new().unwrap());
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let tmp_clone = Arc::clone(&tmp);
                thread::spawn(move || {
                    let batch_id = format!("batch_concurrent_{i}");
                    let args = create_test_args(tmp_clone.path(), &batch_id);
                    execute_export_reviewed_files(&args)
                })
            })
            .collect();

        for handle in handles {
            let res = handle.join().unwrap().unwrap();
            assert_eq!(res.status, ExportStatus::Exported);
        }

        // Verify ledger recorded all 5 batches cleanly
        let state_path = tmp.path().join("exports").join(EXPORT_STATE_FILE);
        let raw = fs::read_to_string(&state_path).unwrap();
        let ledger: ExportStateLedger = serde_json::from_str(&raw).unwrap();
        assert_eq!(ledger.batches.len(), 5);
    }

    #[test]
    #[cfg(unix)]
    fn test_symlink_lock_file_rejected() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let exports_dir = tmp.path().join("exports");
        fs::create_dir_all(&exports_dir).unwrap();
        let target = tmp.path().join("fake_lock_target");
        fs::write(&target, b"").unwrap();
        let lock_path = exports_dir.join(EXPORT_LOCK_FILE);
        symlink(&target, &lock_path).unwrap();

        let args = create_test_args(tmp.path(), "batch_symlink_lock");
        let err = execute_export_reviewed_files(&args).unwrap_err();
        assert_eq!(err.code, "EXPORT.SYMLINK_TARGET");
    }

    #[test]
    fn test_lock_timeout_returns_export_lock_timeout() {
        let tmp = TempDir::new().unwrap();
        let lock_path = tmp.path().join("test.lock");
        let handle1 = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        handle1.lock_exclusive().unwrap();

        let handle2 = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .unwrap();

        let start = Instant::now();
        let err =
            acquire_exclusive_lock_with_timeout(&handle2, Duration::from_millis(60)).unwrap_err();
        let elapsed = start.elapsed();

        assert_eq!(err.code, "EXPORT.LOCK_TIMEOUT");
        assert!(elapsed >= Duration::from_millis(50));
        drop(handle1);
    }

    #[test]
    fn test_replace_file_rejects_non_exdev_and_cross_device_replace_is_atomic() {
        let tmp = TempDir::new().unwrap();
        let non_existent = tmp.path().join("non_existent.txt");
        let dst = tmp.path().join("dst.txt");

        let res = replace_file(&non_existent, &dst);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::NotFound);
        assert!(!dst.exists());

        let src = tmp.path().join("src_file.txt");
        fs::write(&src, b"cross device payload").unwrap();
        let target = tmp.path().join("target_file.txt");
        fs::write(&target, b"old target payload").unwrap();

        cross_device_replace(&src, &target).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read(&target).unwrap(), b"cross device payload");
    }

    #[test]
    fn test_replay_divergent_topic_slug_or_root_mirror_returns_batch_conflict() {
        let tmp = TempDir::new().unwrap();
        let mut args1 = create_test_args(tmp.path(), "batch_divergent");
        args1.topic_slug = Some("original_slug".to_string());
        args1.write_to_deck_root = true;
        let res = execute_export_reviewed_files(&args1).unwrap();
        assert_eq!(res.status, ExportStatus::Exported);

        let mut args2 = args1.clone();
        args2.topic_slug = Some("other_slug".to_string());
        let err2 = execute_export_reviewed_files(&args2).unwrap_err();
        assert_eq!(err2.code, "EXPORT.BATCH_CONFLICT");
        assert!(err2.message.contains("topic_slug diferente"));

        let mut args3 = args1.clone();
        args3.write_to_deck_root = false;
        let err3 = execute_export_reviewed_files(&args3).unwrap_err();
        assert_eq!(err3.code, "EXPORT.BATCH_CONFLICT");
        assert!(err3.message.contains("write_to_deck_root"));
    }
}
