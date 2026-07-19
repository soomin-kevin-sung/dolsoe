use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::runtime_path::validate_runtime_pack_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementOutcome {
    Installed,
    DeferredUntilRestart,
}

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("invalid runtime transaction: {0}")]
    Invalid(String),
    #[error("runtime transaction I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime transaction JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransactionJournal {
    schema_version: u32,
    backend: String,
    staging: String,
    backup: String,
    quarantine: String,
    pack_manifest_sha256: String,
    phase: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFailure {
    pub backend: Option<String>,
    pub error: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub failures: Vec<RecoveryFailure>,
}

pub fn replace_staged<F>(
    root: &Path,
    backend: &str,
    staging: &Path,
    _archive_sha256: &str,
    defer: bool,
    validate: F,
) -> Result<ReplacementOutcome, TransactionError>
where
    F: Fn(&Path) -> bool + Copy,
{
    validate_backend(backend)?;
    fs::create_dir_all(root)?;
    ensure_direct_child(root, staging)?;
    if !validate(staging) {
        return Err(TransactionError::Invalid(
            "staging validation failed".into(),
        ));
    }

    let pack_manifest_sha256 = manifest_digest(staging)?;
    let suffix = Uuid::new_v4();
    let mut journal = TransactionJournal {
        schema_version: 2,
        backend: backend.into(),
        staging: file_name(staging)?,
        backup: format!(".backup-{backend}-{suffix}"),
        quarantine: format!(".quarantine-{backend}-{suffix}"),
        pack_manifest_sha256,
        phase: "prepared".into(),
    };
    write_journal(root, &journal)?;
    if defer {
        journal.phase = "deferred".into();
        write_journal(root, &journal)?;
        return Ok(ReplacementOutcome::DeferredUntilRestart);
    }
    apply_journal(root, &mut journal, validate)?;
    Ok(ReplacementOutcome::Installed)
}

pub fn recover_transactions<F>(root: &Path, validate: F) -> Result<RecoveryReport, TransactionError>
where
    F: Fn(&Path) -> bool + Copy,
{
    let directory = root.join(".transactions");
    let mut report = RecoveryReport::default();
    for backend in ["cpu", "cuda", "vulkan"] {
        let path = directory.join(format!("{backend}.json"));
        if !path.is_file() {
            continue;
        }
        let result = (|| {
            let mut journal: TransactionJournal = serde_json::from_slice(&fs::read(&path)?)?;
            validate_journal(root, &journal)?;
            if journal.backend != backend {
                return Err(TransactionError::Invalid(
                    "journal filename and backend do not match".into(),
                ));
            }
            recover_journal(root, &mut journal, validate)
        })();
        if let Err(error) = result {
            let isolation_error = isolate_failed_journal(root, &path, Some(backend)).err();
            let message = match isolation_error {
                Some(isolation) => format!("{error}; journal isolation failed: {isolation}"),
                None => error.to_string(),
            };
            report.failures.push(RecoveryFailure {
                backend: Some(backend.into()),
                error: message,
            });
        }
    }
    Ok(report)
}

fn apply_journal<F>(
    root: &Path,
    journal: &mut TransactionJournal,
    validate: F,
) -> Result<(), TransactionError>
where
    F: Fn(&Path) -> bool + Copy,
{
    validate_journal(root, journal)?;
    let stable = root.join(&journal.backend);
    let staging = root.join(&journal.staging);
    let backup = root.join(&journal.backup);
    if !validate_new_candidate(&staging, journal, validate) {
        return Err(TransactionError::Invalid(
            "staging manifest changed after verification".into(),
        ));
    }
    if stable.exists() {
        fs::rename(&stable, &backup)?;
        journal.phase = "backed-up".into();
        write_journal(root, journal)?;
    }
    fs::rename(&staging, &stable)?;
    journal.phase = "promoted".into();
    write_journal(root, journal)?;
    if !validate_new_candidate(&stable, journal, validate) {
        quarantine(root, journal, &stable)?;
        if backup.exists() {
            fs::rename(&backup, &stable)?;
        }
        remove_journal(root, &journal.backend)?;
        return Err(TransactionError::Invalid("final validation failed".into()));
    }
    remove_if_exists(&backup)?;
    remove_journal(root, &journal.backend)?;
    clear_repair_marker(root, &journal.backend)?;
    Ok(())
}

fn recover_journal<F>(
    root: &Path,
    journal: &mut TransactionJournal,
    validate: F,
) -> Result<(), TransactionError>
where
    F: Fn(&Path) -> bool + Copy,
{
    let stable = root.join(&journal.backend);
    let staging = root.join(&journal.staging);
    let backup = root.join(&journal.backup);

    if journal.phase == "deferred" {
        return apply_journal(root, journal, validate);
    }
    if journal.phase == "prepared" && validate(&stable) {
        remove_if_exists(&staging)?;
        remove_if_exists(&backup)?;
        remove_journal(root, &journal.backend)?;
        return Ok(());
    }
    if journal.phase == "promoted" && validate_new_candidate(&stable, journal, validate) {
        remove_if_exists(&staging)?;
        remove_if_exists(&backup)?;
        remove_journal(root, &journal.backend)?;
        clear_repair_marker(root, &journal.backend)?;
        return Ok(());
    }
    if matches!(journal.phase.as_str(), "prepared" | "backed-up")
        && validate_new_candidate(&staging, journal, validate)
    {
        quarantine(root, journal, &stable)?;
        fs::rename(&staging, &stable)?;
        if !validate_new_candidate(&stable, journal, validate) {
            quarantine(root, journal, &stable)?;
        } else {
            remove_if_exists(&backup)?;
            remove_journal(root, &journal.backend)?;
            clear_repair_marker(root, &journal.backend)?;
            return Ok(());
        }
    }
    if validate(&backup) {
        quarantine(root, journal, &stable)?;
        fs::rename(&backup, &stable)?;
        remove_if_exists(&staging)?;
        remove_journal(root, &journal.backend)?;
        clear_repair_marker(root, &journal.backend)?;
        return Ok(());
    }
    Err(TransactionError::Invalid(format!(
        "no valid candidate for {}",
        journal.backend
    )))
}

fn quarantine(
    root: &Path,
    journal: &TransactionJournal,
    path: &Path,
) -> Result<(), TransactionError> {
    if path.exists() {
        let quarantine = root.join(&journal.quarantine);
        remove_if_exists(&quarantine)?;
        fs::rename(path, &quarantine)?;
        remove_if_exists(&quarantine)?;
    }
    Ok(())
}

fn write_journal(root: &Path, journal: &TransactionJournal) -> Result<(), TransactionError> {
    validate_journal(root, journal)?;
    let directory = root.join(".transactions");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.json", journal.backend));
    let temporary = directory.join(format!("{}.json.tmp", journal.backend));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec(journal)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, path)?;
    Ok(())
}

fn remove_journal(root: &Path, backend: &str) -> Result<(), TransactionError> {
    let path = root.join(".transactions").join(format!("{backend}.json"));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), TransactionError> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn validate_backend(backend: &str) -> Result<(), TransactionError> {
    validate_runtime_pack_id(backend).map_err(TransactionError::Invalid)?;
    if !matches!(backend, "cpu" | "cuda" | "vulkan") {
        return Err(TransactionError::Invalid("unsupported backend".into()));
    }
    Ok(())
}

fn validate_journal(root: &Path, journal: &TransactionJournal) -> Result<(), TransactionError> {
    if journal.schema_version != 2 {
        return Err(TransactionError::Invalid("journal schema".into()));
    }
    validate_backend(&journal.backend)?;
    for (kind, name) in [
        ("staging", &journal.staging),
        ("backup", &journal.backup),
        ("quarantine", &journal.quarantine),
    ] {
        ensure_direct_child(root, &root.join(name))?;
        validate_transaction_name(kind, &journal.backend, name)?;
        reject_reparse_target(root, name)?;
    }
    if journal.staging == journal.backup
        || journal.staging == journal.quarantine
        || journal.backup == journal.quarantine
    {
        return Err(TransactionError::Invalid(
            "transaction paths must be distinct".into(),
        ));
    }
    if journal.pack_manifest_sha256.len() != 64
        || !journal
            .pack_manifest_sha256
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    {
        return Err(TransactionError::Invalid(
            "invalid pack manifest digest".into(),
        ));
    }
    Ok(())
}

fn validate_transaction_name(
    kind: &str,
    backend: &str,
    name: &str,
) -> Result<(), TransactionError> {
    let prefix = format!(".{kind}-{backend}-");
    let suffix = name
        .strip_prefix(&prefix)
        .ok_or_else(|| TransactionError::Invalid("unsafe transaction path".into()))?;
    let uuid = Uuid::parse_str(suffix)
        .map_err(|_| TransactionError::Invalid("unsafe transaction path".into()))?;
    if suffix != uuid.to_string() {
        return Err(TransactionError::Invalid("unsafe transaction path".into()));
    }
    Ok(())
}

fn reject_reparse_target(root: &Path, name: &str) -> Result<(), TransactionError> {
    let path = root.join(name);
    if !path.exists() {
        return Ok(());
    }
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(TransactionError::Invalid(
            "transaction path is a link".into(),
        ));
    }
    if path.canonicalize()?.parent() != Some(root.canonicalize()?.as_path()) {
        return Err(TransactionError::Invalid(
            "transaction path leaves runtime root".into(),
        ));
    }
    Ok(())
}

fn manifest_digest(directory: &Path) -> Result<String, TransactionError> {
    let bytes = fs::read(directory.join("runtime-pack.json"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_new_candidate<F>(path: &Path, journal: &TransactionJournal, validate: F) -> bool
where
    F: Fn(&Path) -> bool + Copy,
{
    validate(path)
        && manifest_digest(path).is_ok_and(|digest| digest == journal.pack_manifest_sha256)
}

fn isolate_failed_journal(
    root: &Path,
    journal_path: &Path,
    backend: Option<&str>,
) -> Result<(), TransactionError> {
    if let Some(backend) = backend {
        fs::write(root.join(format!(".repair-required-{backend}")), b"")?;
    }
    let invalid = root
        .join(".transactions")
        .join(format!(".invalid-{}.json", Uuid::new_v4()));
    fs::rename(journal_path, invalid)?;
    Ok(())
}

pub(crate) fn clear_repair_marker(root: &Path, backend: &str) -> Result<(), TransactionError> {
    let marker = root.join(format!(".repair-required-{backend}"));
    if marker.exists() {
        fs::remove_file(marker)?;
    }
    Ok(())
}

fn ensure_direct_child(root: &Path, path: &Path) -> Result<(), TransactionError> {
    if path.parent() != Some(root) {
        return Err(TransactionError::Invalid(
            "path escapes runtime root".into(),
        ));
    }
    Ok(())
}

fn file_name(path: &Path) -> Result<String, TransactionError> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| TransactionError::Invalid("invalid transaction path".into()))
}

#[cfg(test)]
fn write_test_journal(root: &Path, backend: &str, staging: &str, backup: &str, phase: &str) {
    let pack_manifest_sha256 =
        manifest_digest(&root.join(staging)).unwrap_or_else(|_| "a".repeat(64));
    let journal = TransactionJournal {
        schema_version: 2,
        backend: backend.into(),
        staging: staging.into(),
        backup: backup.into(),
        quarantine: format!(".quarantine-{backend}-00000000-0000-4000-8000-000000000001"),
        pack_manifest_sha256,
        phase: phase.into(),
    };
    write_journal(root, &journal).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn valid(path: &std::path::Path) -> bool {
        std::fs::read(path.join("valid")).is_ok_and(|bytes| bytes == b"yes")
    }

    fn candidate(root: &std::path::Path, name: &str, is_valid: bool) -> std::path::PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("valid"),
            if is_valid { &b"yes"[..] } else { &b"no"[..] },
        )
        .unwrap();
        std::fs::write(path.join("runtime-pack.json"), format!("{name}-manifest")).unwrap();
        std::fs::write(path.join("version"), name).unwrap();
        path
    }

    #[test]
    fn replacement_rolls_back_when_final_validation_fails() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(
            root,
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            false,
        );

        assert!(replace_staged(root, "cuda", &staging, "a", false, valid).is_err());
        assert!(valid(&root.join("cuda")));
        assert!(!root.join(".transactions/cuda.json").exists());
    }

    #[test]
    fn deferred_replacement_is_applied_by_startup_recovery() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(
            root,
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            true,
        );

        assert_eq!(
            replace_staged(root, "cuda", &staging, "a", true, valid).unwrap(),
            ReplacementOutcome::DeferredUntilRestart
        );
        recover_transactions(root, valid).unwrap();
        assert!(valid(&root.join("cuda")));
        assert!(!root.join(".transactions/cuda.json").exists());
    }

    #[test]
    fn recovery_restores_valid_backup_when_promoted_stable_is_invalid() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", false);
        candidate(
            root,
            ".backup-cuda-00000000-0000-4000-8000-000000000001",
            true,
        );
        write_test_journal(
            root,
            "cuda",
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            ".backup-cuda-00000000-0000-4000-8000-000000000001",
            "promoted",
        );

        recover_transactions(root, valid).unwrap();
        assert!(valid(&root.join("cuda")));
    }

    #[test]
    fn rejects_backend_and_paths_that_escape_runtime_root() {
        let temp = TempDir::new().unwrap();
        let staging = candidate(
            temp.path(),
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            true,
        );
        assert!(replace_staged(temp.path(), "../cuda", &staging, "a", false, valid).is_err());
        assert!(replace_staged(
            temp.path(),
            "cuda",
            &temp.path().join("../escape"),
            "a",
            false,
            valid
        )
        .is_err());
    }

    #[test]
    fn corrupt_gpu_journal_is_isolated_from_other_backends() {
        let temp = TempDir::new().unwrap();
        let transactions = temp.path().join(".transactions");
        std::fs::create_dir_all(&transactions).unwrap();
        std::fs::write(transactions.join("cuda.json"), b"not-json").unwrap();

        let report = recover_transactions(temp.path(), valid).unwrap();

        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].backend.as_deref(), Some("cuda"));
        assert!(temp.path().join(".repair-required-cuda").is_file());
        assert!(!transactions.join("cuda.json").exists());

        let second = recover_transactions(temp.path(), valid).unwrap();
        assert!(second.failures.is_empty());
    }

    #[test]
    fn journal_cannot_target_reserved_runtime_directories() {
        let temp = TempDir::new().unwrap();
        let transactions = temp.path().join(".transactions");
        std::fs::create_dir_all(&transactions).unwrap();
        let downloads = temp.path().join(".downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::write(downloads.join("sentinel"), b"keep").unwrap();
        let journal = serde_json::json!({
            "schemaVersion": 2,
            "backend": "cuda",
            "staging": ".downloads",
            "backup": ".backup-cuda-00000000-0000-4000-8000-000000000001",
            "quarantine": ".quarantine-cuda-00000000-0000-4000-8000-000000000001",
            "packManifestSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "phase": "deferred"
        });
        std::fs::write(
            transactions.join("cuda.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();

        let report = recover_transactions(temp.path(), valid).unwrap();

        assert_eq!(report.failures.len(), 1);
        assert!(downloads.join("sentinel").is_file());
    }

    #[test]
    fn recovery_rejects_staging_whose_manifest_changed_after_install() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(
            root,
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            true,
        );
        replace_staged(root, "cuda", &staging, "archive", true, valid).unwrap();
        std::fs::write(staging.join("runtime-pack.json"), b"changed").unwrap();

        let report = recover_transactions(root, valid).unwrap();

        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("cuda/version")).unwrap(),
            "cuda"
        );
    }

    #[test]
    fn journal_filename_must_match_its_backend() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(
            root,
            ".staging-cuda-00000000-0000-4000-8000-000000000001",
            true,
        );
        replace_staged(root, "cuda", &staging, "archive", true, valid).unwrap();
        std::fs::rename(
            root.join(".transactions/cuda.json"),
            root.join(".transactions/vulkan.json"),
        )
        .unwrap();

        let report = recover_transactions(root, valid).unwrap();

        assert_eq!(report.failures[0].backend.as_deref(), Some("vulkan"));
        assert_eq!(
            std::fs::read_to_string(root.join("cuda/version")).unwrap(),
            "cuda"
        );
    }

    #[test]
    fn journal_isolation_failure_does_not_abort_recovery() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join(".transactions")).unwrap();
        std::fs::write(root.join(".transactions/cuda.json"), b"bad").unwrap();
        std::fs::create_dir(root.join(".repair-required-cuda")).unwrap();

        let report = recover_transactions(root, valid).unwrap();

        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].error.contains("JSON"));
    }
}
