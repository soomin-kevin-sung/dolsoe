use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGuard {
    root: PathBuf,
}

impl WorkspaceGuard {
    pub fn new(workspace_path: &str) -> Result<Self, String> {
        let workspace_path = workspace_path.trim();
        if workspace_path.is_empty() {
            return Err("workspace path is empty".into());
        }
        let root = fs::canonicalize(workspace_path)
            .map_err(|error| format!("failed to resolve workspace path: {error}"))?;
        if !root.is_dir() {
            return Err("workspace path is not a directory".into());
        }
        Ok(Self { root })
    }

    pub fn resolve_existing(&self, requested_path: &str) -> Result<PathBuf, String> {
        let requested_path = requested_path.trim();
        if requested_path.is_empty() {
            return Err("the `path` argument is required".into());
        }
        self.resolve_existing_path(Path::new(requested_path))
    }

    pub fn resolve_existing_path(&self, requested: &Path) -> Result<PathBuf, String> {
        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.root.join(requested)
        };
        let display = requested.to_string_lossy();
        let resolved = fs::canonicalize(&candidate)
            .map_err(|error| format!("failed to resolve `{display}`: {error}"))?;
        if !resolved.starts_with(&self.root) {
            return Err(format!("path `{display}` is outside the current workspace"));
        }
        Ok(resolved)
    }

    pub fn relative_display(&self, path: &Path) -> Result<String, String> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "resolved path is outside the current workspace".to_string())?;
        if relative.as_os_str().is_empty() {
            return Ok(".".into());
        }
        Ok(relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::WorkspaceGuard;

    #[test]
    fn resolves_only_existing_paths_inside_the_workspace() {
        let workspace = tempdir().unwrap();
        let nested = workspace.path().join("src");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("lib.rs"), "fn main() {}\n").unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        let guard = WorkspaceGuard::new(workspace.path().to_str().unwrap()).unwrap();
        let file = guard.resolve_existing("src/lib.rs").unwrap();
        assert_eq!(guard.relative_display(&file).unwrap(), "src/lib.rs");
        assert!(guard
            .resolve_existing(outside.path().join("secret.txt").to_str().unwrap())
            .unwrap_err()
            .contains("outside the current workspace"));
        assert!(guard.resolve_existing("../missing").is_err());
    }

    #[test]
    fn workspace_root_must_be_an_existing_directory() {
        let workspace = tempdir().unwrap();
        let file = workspace.path().join("file.txt");
        fs::write(&file, "content").unwrap();

        assert!(WorkspaceGuard::new(file.to_str().unwrap()).is_err());
        assert!(WorkspaceGuard::new("").is_err());
    }
}
