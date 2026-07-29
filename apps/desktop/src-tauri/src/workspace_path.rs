use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

pub fn default_for_app(app: &AppHandle) -> Result<String, String> {
    let candidates = [app.path().document_dir(), app.path().home_dir()];
    for candidate in candidates {
        let Ok(path) = candidate else {
            continue;
        };
        if path.is_dir() {
            return directory(&path.to_string_lossy());
        }
    }
    Err("a default workspace directory could not be resolved".into())
}

pub fn directory(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("workspace path must not be empty".into());
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("workspace path must be absolute".into());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("workspace directory is unavailable: {error}"))?;
    if !metadata.is_dir() {
        return Err("workspace path must point to a directory".into());
    }
    Ok(path
        .components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::directory;

    #[test]
    fn accepts_only_existing_absolute_directories() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            directory(&root.path().to_string_lossy()).unwrap(),
            root.path().to_string_lossy()
        );
        assert!(directory("relative/path").is_err());
        assert!(directory(&root.path().join("missing").to_string_lossy()).is_err());
    }
}
