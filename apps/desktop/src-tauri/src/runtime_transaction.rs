use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};
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
    archive_sha256: String,
    phase: String,
}

pub fn replace_staged<F>(
    root: &Path,
    backend: &str,
    staging: &Path,
    archive_sha256: &str,
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

    let suffix = Uuid::new_v4();
    let mut journal = TransactionJournal {
        schema_version: 1,
        backend: backend.into(),
        staging: file_name(staging)?,
        backup: format!(".backup-{backend}-{suffix}"),
        quarantine: format!(".quarantine-{backend}-{suffix}"),
        archive_sha256: archive_sha256.into(),
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

pub fn recover_transactions<F>(root: &Path, validate: F) -> Result<(), TransactionError>
where
    F: Fn(&Path) -> bool + Copy,
{
    let directory = root.join(".transactions");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|value| value == "json")
        {
            let mut journal: TransactionJournal = serde_json::from_slice(&fs::read(entry.path())?)?;
            validate_journal(root, &journal)?;
            recover_journal(root, &mut journal, validate)?;
        }
    }
    Ok(())
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
    if stable.exists() {
        fs::rename(&stable, &backup)?;
        journal.phase = "backed-up".into();
        write_journal(root, journal)?;
    }
    fs::rename(&staging, &stable)?;
    journal.phase = "promoted".into();
    write_journal(root, journal)?;
    if !validate(&stable) {
        quarantine(root, journal, &stable)?;
        if backup.exists() {
            fs::rename(&backup, &stable)?;
        }
        remove_journal(root, &journal.backend)?;
        return Err(TransactionError::Invalid("final validation failed".into()));
    }
    remove_if_exists(&backup)?;
    remove_journal(root, &journal.backend)?;
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
    if journal.phase == "deferred" {
        return apply_journal(root, journal, validate);
    }
    let stable = root.join(&journal.backend);
    let staging = root.join(&journal.staging);
    let backup = root.join(&journal.backup);
    if validate(&stable) {
        remove_if_exists(&staging)?;
        remove_if_exists(&backup)?;
        remove_journal(root, &journal.backend)?;
        return Ok(());
    }
    if validate(&staging) {
        quarantine(root, journal, &stable)?;
        fs::rename(&staging, &stable)?;
        if !validate(&stable) {
            quarantine(root, journal, &stable)?;
        } else {
            remove_if_exists(&backup)?;
            remove_journal(root, &journal.backend)?;
            return Ok(());
        }
    }
    if validate(&backup) {
        quarantine(root, journal, &stable)?;
        fs::rename(&backup, &stable)?;
        remove_if_exists(&staging)?;
        remove_journal(root, &journal.backend)?;
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
    if journal.schema_version != 1 {
        return Err(TransactionError::Invalid("journal schema".into()));
    }
    validate_backend(&journal.backend)?;
    for name in [&journal.staging, &journal.backup, &journal.quarantine] {
        ensure_direct_child(root, &root.join(name))?;
        if !name.starts_with('.') {
            return Err(TransactionError::Invalid("unsafe transaction path".into()));
        }
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
    let journal = TransactionJournal {
        schema_version: 1,
        backend: backend.into(),
        staging: staging.into(),
        backup: backup.into(),
        quarantine: format!(".quarantine-{backend}-test"),
        archive_sha256: "a".into(),
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
        path
    }

    #[test]
    fn replacement_rolls_back_when_final_validation_fails() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(root, ".staging-cuda-new", false);

        assert!(replace_staged(root, "cuda", &staging, "a", false, valid).is_err());
        assert!(valid(&root.join("cuda")));
        assert!(!root.join(".transactions/cuda.json").exists());
    }

    #[test]
    fn deferred_replacement_is_applied_by_startup_recovery() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        candidate(root, "cuda", true);
        let staging = candidate(root, ".staging-cuda-new", true);

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
        candidate(root, ".backup-cuda-test", true);
        write_test_journal(
            root,
            "cuda",
            ".staging-cuda-test",
            ".backup-cuda-test",
            "promoted",
        );

        recover_transactions(root, valid).unwrap();
        assert!(valid(&root.join("cuda")));
    }

    #[test]
    fn rejects_backend_and_paths_that_escape_runtime_root() {
        let temp = TempDir::new().unwrap();
        let staging = candidate(temp.path(), ".staging-cuda", true);
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
}
