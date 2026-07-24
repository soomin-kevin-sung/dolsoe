use std::{
    fs, io,
    path::{Path, PathBuf},
};

const LEGACY_IDENTIFIER: &str = "io.github.soomin-kevin-sung.local-llm-wiki";

pub(crate) fn migrate_legacy_app_data(app_data: &Path) -> io::Result<()> {
    let Some(data_root) = app_data.parent() else {
        return Ok(());
    };
    let legacy_root = data_root.join(LEGACY_IDENTIFIER);
    if !legacy_root.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(app_data)?;
    for (legacy_name, current_name) in [
        ("local-llm-wiki.db", "dolsoe.db"),
        ("local-llm-wiki.db-wal", "dolsoe.db-wal"),
        ("local-llm-wiki.db-shm", "dolsoe.db-shm"),
        ("runtime-packs", "runtime-packs"),
        ("runtime-selection.json", "runtime-selection.json"),
        (
            "EBWebView/Default/Local Storage",
            "EBWebView/Default/Local Storage",
        ),
    ] {
        move_if_destination_missing(legacy_root.join(legacy_name), app_data.join(current_name))?;
    }
    Ok(())
}

fn move_if_destination_missing(source: PathBuf, destination: PathBuf) -> io::Result<()> {
    if !source.exists() || destination.exists() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::migrate_legacy_app_data;
    use std::fs;

    #[test]
    fn migrates_database_runtime_and_selection_without_overwriting_new_data() {
        let root = tempfile::tempdir().expect("create data root");
        let legacy = root
            .path()
            .join("io.github.soomin-kevin-sung.local-llm-wiki");
        let current = root.path().join("ai.dolsoe.desktop");
        fs::create_dir_all(legacy.join("runtime-packs/cpu")).expect("create legacy runtime");
        fs::write(legacy.join("local-llm-wiki.db"), b"legacy-db").expect("write legacy db");
        fs::write(legacy.join("local-llm-wiki.db-wal"), b"legacy-wal").expect("write legacy wal");
        fs::write(legacy.join("runtime-packs/cpu/runtime-pack.json"), b"{}")
            .expect("write runtime");
        fs::write(legacy.join("runtime-selection.json"), b"legacy-selection")
            .expect("write selection");
        fs::create_dir_all(legacy.join("EBWebView/Default/Local Storage/leveldb"))
            .expect("create legacy web storage");
        fs::write(
            legacy.join("EBWebView/Default/Local Storage/leveldb/CURRENT"),
            b"MANIFEST-000001",
        )
        .expect("write legacy web storage");
        fs::create_dir_all(&current).expect("create current root");
        fs::write(current.join("runtime-selection.json"), b"current-selection")
            .expect("write current selection");

        migrate_legacy_app_data(&current).expect("migrate legacy data");

        assert_eq!(fs::read(current.join("dolsoe.db")).unwrap(), b"legacy-db");
        assert_eq!(
            fs::read(current.join("dolsoe.db-wal")).unwrap(),
            b"legacy-wal"
        );
        assert!(current
            .join("runtime-packs/cpu/runtime-pack.json")
            .is_file());
        assert_eq!(
            fs::read(current.join("runtime-selection.json")).unwrap(),
            b"current-selection"
        );
        assert_eq!(
            fs::read(current.join("EBWebView/Default/Local Storage/leveldb/CURRENT")).unwrap(),
            b"MANIFEST-000001"
        );
        assert!(legacy.join("runtime-selection.json").is_file());
    }

    #[test]
    fn does_nothing_when_legacy_data_is_absent() {
        let root = tempfile::tempdir().expect("create data root");
        let current = root.path().join("ai.dolsoe.desktop");

        migrate_legacy_app_data(&current).expect("skip migration");

        assert!(!current.exists());
    }
}
