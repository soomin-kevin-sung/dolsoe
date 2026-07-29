use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

const DATA_DIRECTORY_NAME: &str = "data";
const DATA_LAYOUT_MARKER: &str = ".dolsoe-data-v1";
const APP_DIRECTORY_NAME: &str = "Dolsoe";

pub fn resolve(tauri_app_data: &Path) -> io::Result<PathBuf> {
    let target_root = installed_velopack_root()
        .map(|install_root| install_root.join(DATA_DIRECTORY_NAME))
        .unwrap_or_else(|| development_data_root(tauri_app_data));
    prepare_data_root(tauri_app_data, &target_root)
}

fn development_data_root(tauri_app_data: &Path) -> PathBuf {
    tauri_app_data
        .parent()
        .map(|parent| parent.join(APP_DIRECTORY_NAME).join(DATA_DIRECTORY_NAME))
        .unwrap_or_else(|| tauri_app_data.join(DATA_DIRECTORY_NAME))
}

#[cfg(target_os = "windows")]
fn installed_velopack_root() -> Option<PathBuf> {
    use velopack::locator::{auto_locate_app_manifest, LocationContext};

    auto_locate_app_manifest(LocationContext::FromCurrentExe)
        .ok()
        .filter(|locator| !locator.get_is_portable())
        .map(|locator| locator.get_root_dir())
}

#[cfg(not(target_os = "windows"))]
fn installed_velopack_root() -> Option<PathBuf> {
    None
}

fn prepare_data_root(legacy_root: &Path, target_root: &Path) -> io::Result<PathBuf> {
    if legacy_root == target_root {
        fs::create_dir_all(target_root)?;
        write_layout_marker(target_root)?;
        return Ok(target_root.to_path_buf());
    }

    validate_directory_if_present(legacy_root, "legacy app data")?;
    validate_directory_if_present(target_root, "Velopack app data")?;

    if target_root.exists() {
        if legacy_root.exists()
            && !target_root.join(DATA_LAYOUT_MARKER).is_file()
            && !directory_is_empty(target_root)?
        {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!(
                    "both legacy and Velopack app data contain files: {} and {}",
                    legacy_root.display(),
                    target_root.display()
                ),
            ));
        }
        if legacy_root.exists() && directory_is_empty(target_root)? {
            fs::remove_dir(target_root)?;
            migrate_directory(legacy_root, target_root)?;
        }
    } else if legacy_root.exists() {
        migrate_directory(legacy_root, target_root)?;
    } else {
        fs::create_dir_all(target_root)?;
    }

    write_layout_marker(target_root)?;
    Ok(target_root.to_path_buf())
}

fn migrate_directory(source: &Path, target: &Path) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device_error(&error) => copy_across_volumes(source, target),
        Err(error) => Err(error),
    }
}

fn copy_across_volumes(source: &Path, target: &Path) -> io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "Velopack data directory must have a parent",
        )
    })?;
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "Velopack data directory must have a UTF-8 file name",
            )
        })?;
    let staging = parent.join(format!("{file_name}.migrating-{}", std::process::id()));
    if staging.exists() {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "stale app data migration directory exists: {}",
                staging.display()
            ),
        ));
    }

    if let Err(error) = copy_directory(source, &staging).and_then(|()| fs::rename(&staging, target))
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = fs::remove_dir_all(source) {
        eprintln!(
            "Dolsoe app data migrated, but the legacy directory could not be removed: {error}"
        );
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> io::Result<()> {
    validate_directory_if_present(source, "migration source")?;
    fs::create_dir(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "app data migration refuses symbolic links: {}",
                    entry.path().display()
                ),
            ));
        }
        if file_type.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), destination)?;
        } else {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "app data migration found an unsupported entry: {}",
                    entry.path().display()
                ),
            ));
        }
    }
    Ok(())
}

fn validate_directory_if_present(path: &Path, label: &str) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("{label} is not a regular directory: {}", path.display()),
        ));
    }
    Ok(())
}

fn directory_is_empty(path: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(path)?.next().transpose()?.is_none())
}

fn write_layout_marker(target_root: &Path) -> io::Result<()> {
    fs::write(
        target_root.join(DATA_LAYOUT_MARKER),
        b"Dolsoe Velopack data layout v1\n",
    )
}

fn is_cross_device_error(error: &io::Error) -> bool {
    #[cfg(target_os = "windows")]
    {
        error.raw_os_error() == Some(17)
    }
    #[cfg(not(target_os = "windows"))]
    {
        error.raw_os_error() == Some(18)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::{
        copy_across_volumes, development_data_root, prepare_data_root, DATA_LAYOUT_MARKER,
    };

    #[test]
    fn development_uses_the_same_dolsoe_data_layout_as_installed_copies() {
        let app_local_root = Path::new(r"C:\Users\tester\AppData\Local");
        let legacy = app_local_root.join("ai.dolsoe.desktop");

        assert_eq!(
            development_data_root(&legacy),
            app_local_root.join("Dolsoe").join("data")
        );
    }

    #[test]
    fn migrates_the_legacy_directory_into_the_velopack_root() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("ai.dolsoe.desktop");
        let target = root.path().join("Dolsoe").join("data");
        fs::create_dir_all(legacy.join("personas")).unwrap();
        fs::write(legacy.join("dolsoe.db"), b"database").unwrap();
        fs::write(legacy.join("personas").join("settings.json"), b"{}").unwrap();

        assert_eq!(prepare_data_root(&legacy, &target).unwrap(), target);
        assert!(!legacy.exists());
        assert_eq!(fs::read(target.join("dolsoe.db")).unwrap(), b"database");
        assert!(target.join(DATA_LAYOUT_MARKER).is_file());
    }

    #[test]
    fn rejects_ambiguous_nonempty_data_roots() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("legacy");
        let target = root.path().join("Dolsoe").join("data");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(legacy.join("dolsoe.db"), b"old").unwrap();
        fs::write(target.join("dolsoe.db"), b"new").unwrap();

        let error = prepare_data_root(&legacy, &target).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(legacy.join("dolsoe.db")).unwrap(), b"old");
        assert_eq!(fs::read(target.join("dolsoe.db")).unwrap(), b"new");
    }

    #[test]
    fn reuses_an_initialized_velopack_data_root() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("legacy");
        let target = root.path().join("Dolsoe").join("data");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("dolsoe.db"), b"database").unwrap();

        assert_eq!(prepare_data_root(&legacy, &target).unwrap(), target);
        assert_eq!(fs::read(target.join("dolsoe.db")).unwrap(), b"database");
        assert!(target.join(DATA_LAYOUT_MARKER).is_file());
    }

    #[test]
    fn cross_volume_copy_promotes_a_complete_tree_before_removing_the_source() {
        let root = TempDir::new().unwrap();
        let source = root.path().join("legacy");
        let target = root.path().join("Dolsoe").join("data");
        fs::create_dir_all(source.join("runtime-packs").join("cpu")).unwrap();
        fs::write(source.join("dolsoe.db"), b"database").unwrap();
        fs::write(
            source
                .join("runtime-packs")
                .join("cpu")
                .join("manifest.json"),
            b"{}",
        )
        .unwrap();
        fs::create_dir_all(target.parent().unwrap()).unwrap();

        copy_across_volumes(&source, &target).unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(target.join("dolsoe.db")).unwrap(), b"database");
        assert_eq!(
            fs::read(
                target
                    .join("runtime-packs")
                    .join("cpu")
                    .join("manifest.json")
            )
            .unwrap(),
            b"{}"
        );
    }
}
