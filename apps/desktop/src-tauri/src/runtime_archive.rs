use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    runtime_manifest::{RuntimeManifestFile, RuntimeManifestPack},
    runtime_path::validate_runtime_pack_id,
};

const MAX_FILES: usize = 4096;
const MAX_TOTAL_SIZE: u64 = 16 * 1024 * 1024 * 1024;
const COMMON_REQUIRED: &[&str] = &[
    "local_llm_runtime.dll",
    "llama.dll",
    "ggml.dll",
    "ggml-base.dll",
    "ggml-cpu.dll",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallArchiveResult {
    Installed,
    AlreadyInstalled,
}

#[derive(Debug, Error)]
pub enum ArchiveInstallError {
    #[error("invalid runtime archive: {0}")]
    Invalid(String),
    #[error("runtime pack {0} already exists with different contents")]
    Conflict(String),
    #[error("runtime archive I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime ZIP failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub fn install_verified_archive(
    archive_path: &Path,
    runtime_root: &Path,
    pack: &RuntimeManifestPack,
) -> Result<InstallArchiveResult, ArchiveInstallError> {
    validate_runtime_pack_id(&pack.id).map_err(ArchiveInstallError::Invalid)?;
    validate_declared_pack(pack)?;
    fs::create_dir_all(runtime_root)?;

    let final_path = direct_child(runtime_root, &pack.id)?;
    if final_path.exists() {
        return if existing_matches(&final_path, &pack.files)? {
            Ok(InstallArchiveResult::AlreadyInstalled)
        } else {
            Err(ArchiveInstallError::Conflict(pack.id.clone()))
        };
    }

    let staging_name = format!("{}.staging-{}", pack.id, Uuid::new_v4());
    let staging_path = direct_child(runtime_root, &staging_name)?;
    fs::create_dir(&staging_path)?;
    let result = extract_verified(archive_path, &staging_path, &pack.files)
        .and_then(|_| fs::rename(&staging_path, &final_path).map_err(ArchiveInstallError::Io));
    if result.is_err() && staging_path.exists() {
        let _ = fs::remove_dir_all(&staging_path);
    }
    result.map(|_| InstallArchiveResult::Installed)
}

fn validate_declared_pack(pack: &RuntimeManifestPack) -> Result<(), ArchiveInstallError> {
    if pack.files.len() > MAX_FILES {
        return Err(ArchiveInstallError::Invalid("too many files".into()));
    }
    let total = pack.files.iter().try_fold(0_u64, |sum, file| {
        sum.checked_add(file.size)
            .ok_or_else(|| ArchiveInstallError::Invalid("file size overflow".into()))
    })?;
    if total > MAX_TOTAL_SIZE {
        return Err(ArchiveInstallError::Invalid("pack is too large".into()));
    }
    let paths: HashSet<&str> = pack.files.iter().map(|file| file.path.as_str()).collect();
    if paths.len() != pack.files.len() {
        return Err(ArchiveInstallError::Invalid(
            "duplicate declared file".into(),
        ));
    }
    for required in COMMON_REQUIRED {
        if !paths.contains(required) {
            return Err(ArchiveInstallError::Invalid(format!(
                "missing required file {required}"
            )));
        }
    }
    let selected = match pack.backend.as_str() {
        "cpu" => "ggml-cpu.dll",
        "cuda" => "ggml-cuda.dll",
        "vulkan" => "ggml-vulkan.dll",
        _ => return Err(ArchiveInstallError::Invalid("unknown backend".into())),
    };
    if !paths.contains(selected) {
        return Err(ArchiveInstallError::Invalid(format!(
            "missing backend file {selected}"
        )));
    }
    for forbidden in ["ggml-cuda.dll", "ggml-vulkan.dll"] {
        if forbidden != selected && paths.contains(forbidden) {
            return Err(ArchiveInstallError::Invalid(format!(
                "mixed backend file {forbidden}"
            )));
        }
    }
    for file in &pack.files {
        safe_relative_path(&file.path)?;
    }
    Ok(())
}

fn extract_verified(
    archive_path: &Path,
    staging_path: &Path,
    declared_files: &[RuntimeManifestFile],
) -> Result<(), ArchiveInstallError> {
    let archive_file = fs::File::open(archive_path)?;
    let mut archive = ZipArchive::new(archive_file)?;
    if archive.len() > MAX_FILES {
        return Err(ArchiveInstallError::Invalid("too many ZIP entries".into()));
    }
    let declared: HashMap<&str, &RuntimeManifestFile> = declared_files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut seen = HashSet::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().replace('\\', "/");
        if entry.is_dir() || entry.enclosed_name().is_none() || is_symlink(entry.unix_mode()) {
            return Err(ArchiveInstallError::Invalid(format!(
                "unsafe ZIP entry {name}"
            )));
        }
        safe_relative_path(&name)?;
        if !seen.insert(name.clone()) {
            return Err(ArchiveInstallError::Invalid(format!(
                "duplicate ZIP entry {name}"
            )));
        }
        let expected = declared.get(name.as_str()).ok_or_else(|| {
            ArchiveInstallError::Invalid(format!("undeclared ZIP entry {name}"))
        })?;
        if entry.size() != expected.size {
            return Err(ArchiveInstallError::Invalid(format!(
                "size mismatch for {name}"
            )));
        }
        let target = staging_path.join(Path::new(&name));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&target)?;
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > expected.size {
                return Err(ArchiveInstallError::Invalid(format!(
                    "size overflow for {name}"
                )));
            }
            hasher.update(&buffer[..count]);
            output.write_all(&buffer[..count])?;
        }
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected.sha256.to_ascii_lowercase() {
            return Err(ArchiveInstallError::Invalid(format!(
                "SHA-256 mismatch for {name}"
            )));
        }
    }
    if seen.len() != declared.len() {
        return Err(ArchiveInstallError::Invalid(
            "archive is missing declared files".into(),
        ));
    }
    Ok(())
}

fn existing_matches(
    directory: &Path,
    declared_files: &[RuntimeManifestFile],
) -> Result<bool, ArchiveInstallError> {
    let mut actual_files = Vec::new();
    collect_files(directory, directory, &mut actual_files)?;
    if actual_files.len() != declared_files.len() {
        return Ok(false);
    }
    let declared: HashMap<&str, &RuntimeManifestFile> = declared_files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for (relative, path) in actual_files {
        let Some(expected) = declared.get(relative.as_str()) else {
            return Ok(false);
        };
        let bytes = fs::read(path)?;
        if bytes.len() as u64 != expected.size
            || format!("{:x}", Sha256::digest(&bytes)) != expected.sha256.to_ascii_lowercase()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), ArchiveInstallError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(ArchiveInstallError::Invalid(
                "installed pack contains symlink".into(),
            ));
        }
        if kind.is_dir() {
            collect_files(root, &path, output)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ArchiveInstallError::Invalid("invalid installed path".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, path));
        }
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, ArchiveInstallError> {
    if value.contains('\\') {
        return Err(ArchiveInstallError::Invalid(format!(
            "non-normalized path {value}"
        )));
    }
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArchiveInstallError::Invalid(format!(
            "unsafe path {value}"
        )));
    }
    Ok(path.to_path_buf())
}

fn direct_child(root: &Path, name: &str) -> Result<PathBuf, ArchiveInstallError> {
    let candidate = root.join(name);
    if candidate.parent() != Some(root) {
        return Err(ArchiveInstallError::Invalid(
            "path escapes runtime root".into(),
        ));
    }
    Ok(candidate)
}

fn is_symlink(mode: Option<u32>) -> bool {
    mode.is_some_and(|mode| mode & 0o170000 == 0o120000)
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path};

    use sha2::{Digest, Sha256};
    use tempfile::TempDir;
    use zip::{write::SimpleFileOptions, ZipWriter};

    use crate::runtime_manifest::{RuntimeManifestFile, RuntimeManifestPack};

    use super::{install_verified_archive, InstallArchiveResult};

    const REQUIRED: &[(&str, &[u8])] = &[
        ("local_llm_runtime.dll", b"bridge"),
        ("llama.dll", b"llama"),
        ("ggml.dll", b"ggml"),
        ("ggml-base.dll", b"base"),
        ("ggml-cpu.dll", b"cpu"),
        ("ggml-cuda.dll", b"cuda"),
    ];

    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn pack(files: &[(&str, &[u8])]) -> RuntimeManifestPack {
        RuntimeManifestPack {
            id: "cuda-2026.07.1".into(),
            backend: "cuda".into(),
            platform: "windows".into(),
            arch: "x86_64".into(),
            asset_url: "https://github.com/soomin-sung-estsoft/local-llm-wiki/releases/download/runtime-v2026.07.1/cuda.zip".into(),
            size: 1,
            sha256: "0".repeat(64),
            files: files
                .iter()
                .map(|(path, bytes)| RuntimeManifestFile {
                    path: (*path).into(),
                    size: bytes.len() as u64,
                    sha256: hash(bytes),
                })
                .collect(),
        }
    }

    fn zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        for (name, contents) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start zip file");
            writer.write_all(contents).expect("write zip file");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn installs_declared_pack_with_atomic_directory_promotion() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);

        let result = install_verified_archive(&archive, root.path(), &pack(REQUIRED))
            .expect("install valid archive");

        assert_eq!(result, InstallArchiveResult::Installed);
        assert_eq!(
            fs::read(root.path().join("cuda-2026.07.1/ggml-cuda.dll")).unwrap(),
            b"cuda"
        );
        assert!(!root
            .path()
            .read_dir()
            .unwrap()
            .any(|entry| entry.unwrap().file_name().to_string_lossy().contains("staging")));
    }

    #[test]
    fn rejects_traversal_duplicate_and_undeclared_entries() {
        for extra in ["../escape.dll", "C:/escape.dll", "surprise.dll"] {
            let root = TempDir::new().expect("runtime root");
            let archive = root.path().join("pack.zip");
            let mut files = REQUIRED.to_vec();
            files.push((extra, b"bad"));
            zip(&archive, &files);
            assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());
            assert!(!root.path().join("cuda-2026.07.1").exists());
        }

        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("duplicate.zip");
        zip(&archive, REQUIRED);
        let mut duplicate_manifest = pack(REQUIRED);
        duplicate_manifest.files.push(duplicate_manifest.files[0].clone());
        assert!(install_verified_archive(&archive, root.path(), &duplicate_manifest).is_err());
    }

    #[test]
    fn rejects_missing_hash_mismatch_and_mixed_backend_files() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("missing.zip");
        zip(&archive, &REQUIRED[..5]);
        assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());

        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("hash.zip");
        zip(&archive, REQUIRED);
        let mut bad_pack = pack(REQUIRED);
        bad_pack.files[0].sha256 = "f".repeat(64);
        assert!(install_verified_archive(&archive, root.path(), &bad_pack).is_err());

        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("mixed.zip");
        let mut mixed = REQUIRED.to_vec();
        mixed.push(("ggml-vulkan.dll", b"vulkan"));
        zip(&archive, &mixed);
        assert!(install_verified_archive(&archive, root.path(), &pack(&mixed)).is_err());
    }

    #[test]
    fn treats_identical_existing_pack_as_installed_and_rejects_conflict() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);
        let manifest = pack(REQUIRED);
        install_verified_archive(&archive, root.path(), &manifest).expect("first install");
        assert_eq!(
            install_verified_archive(&archive, root.path(), &manifest).unwrap(),
            InstallArchiveResult::AlreadyInstalled
        );

        fs::write(
            root.path().join("cuda-2026.07.1/ggml-cuda.dll"),
            b"changed",
        )
        .unwrap();
        assert!(install_verified_archive(&archive, root.path(), &manifest).is_err());
    }
}
