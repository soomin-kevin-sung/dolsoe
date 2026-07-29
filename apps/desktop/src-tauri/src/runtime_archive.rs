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
    runtime_manifest::{
        bundled_manifest_policy, RuntimeManifestFile, RuntimeManifestPack, RuntimePackManifest,
    },
    runtime_path::validate_runtime_pack_id,
    runtime_transaction::{clear_repair_marker, replace_staged, ReplacementOutcome},
};

const MAX_FILES: usize = 4096;
const MAX_TOTAL_SIZE: u64 = 16 * 1024 * 1024 * 1024;
const PACK_MANIFEST_NAME: &str = "runtime-pack.json";
const COMMON_REQUIRED: &[&str] = &[
    "local_llm_runtime.dll",
    "llama.dll",
    "ggml.dll",
    "ggml-base.dll",
];

fn is_cpu_backend_file(path: &str) -> bool {
    path == "ggml-cpu.dll"
        || path
            .strip_prefix("ggml-cpu-")
            .is_some_and(|suffix| suffix.ends_with(".dll") && suffix.len() > 4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallArchiveResult {
    Installed,
    AlreadyInstalled,
    DeferredUntilRestart,
}

#[derive(Debug, Error)]
pub enum ArchiveInstallError {
    #[error("invalid runtime archive: {0}")]
    Invalid(String),
    #[error("runtime archive I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("runtime ZIP failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

#[cfg(test)]
pub fn install_verified_archive(
    archive_path: &Path,
    runtime_root: &Path,
    pack: &RuntimeManifestPack,
) -> Result<InstallArchiveResult, ArchiveInstallError> {
    install_verified_archive_with_mode(archive_path, runtime_root, pack, false)
}

#[cfg(test)]
pub fn install_verified_archive_with_mode(
    archive_path: &Path,
    runtime_root: &Path,
    pack: &RuntimeManifestPack,
    defer_replacement: bool,
) -> Result<InstallArchiveResult, ArchiveInstallError> {
    install_verified_archive_with_mode_and_probe(
        archive_path,
        runtime_root,
        pack,
        defer_replacement,
        |_, _| Ok(()),
    )
}

pub(crate) fn install_verified_archive_with_mode_and_probe<F>(
    archive_path: &Path,
    runtime_root: &Path,
    pack: &RuntimeManifestPack,
    defer_replacement: bool,
    live_probe: F,
) -> Result<InstallArchiveResult, ArchiveInstallError>
where
    F: Fn(&Path, &str) -> Result<(), String>,
{
    validate_runtime_pack_id(&pack.id).map_err(ArchiveInstallError::Invalid)?;
    let internal = read_pack_manifest(archive_path)?;
    if !pack.matches_internal(&internal) {
        return Err(ArchiveInstallError::Invalid(
            "catalog and runtime-pack identity mismatch".into(),
        ));
    }
    validate_declared_pack(&internal)?;
    fs::create_dir_all(runtime_root)?;

    let final_path = direct_child(runtime_root, &pack.id)?;
    if final_path.exists() && existing_matches(&final_path, &internal)? {
        live_probe(&final_path, &pack.id).map_err(ArchiveInstallError::Invalid)?;
        clear_repair_marker(runtime_root, &pack.id)
            .map_err(|error| ArchiveInstallError::Invalid(error.to_string()))?;
        return Ok(InstallArchiveResult::AlreadyInstalled);
    }

    let staging_name = format!(".staging-{}-{}", pack.id, Uuid::new_v4());
    let staging_path = direct_child(runtime_root, &staging_name)?;
    fs::create_dir(&staging_path)?;
    let result = extract_verified(archive_path, &staging_path, &internal.files)
        .and_then(|_| live_probe(&staging_path, &pack.id).map_err(ArchiveInstallError::Invalid))
        .and_then(|_| {
            replace_staged(
                runtime_root,
                &pack.id,
                &staging_path,
                &pack.sha256,
                defer_replacement,
                |candidate| validate_installed_pack(candidate, &pack.id),
            )
            .map_err(|error| ArchiveInstallError::Invalid(error.to_string()))
            .map(|outcome| match outcome {
                ReplacementOutcome::Installed => InstallArchiveResult::Installed,
                ReplacementOutcome::DeferredUntilRestart => {
                    InstallArchiveResult::DeferredUntilRestart
                }
            })
        });
    if result.is_err() && staging_path.exists() {
        let _ = fs::remove_dir_all(&staging_path);
    }
    result
}

pub fn validate_installed_pack(directory: &Path, expected_backend: &str) -> bool {
    let Ok(bytes) = fs::read(directory.join(PACK_MANIFEST_NAME)) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<RuntimePackManifest>(&bytes) else {
        return false;
    };
    let Ok(policy) = bundled_manifest_policy() else {
        return false;
    };
    manifest.id == expected_backend
        && manifest.backend == expected_backend
        && manifest.matches_policy(&policy)
        && validate_declared_pack(&manifest).is_ok()
        && existing_matches(directory, &manifest).unwrap_or(false)
}

pub fn validate_installed_pack_self(directory: &Path) -> bool {
    let Ok(bytes) = fs::read(directory.join(PACK_MANIFEST_NAME)) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<RuntimePackManifest>(&bytes) else {
        return false;
    };
    matches!(manifest.backend.as_str(), "cpu" | "cuda" | "vulkan")
        && manifest.id == manifest.backend
        && validate_installed_pack(directory, &manifest.backend)
}

fn validate_declared_pack(pack: &RuntimePackManifest) -> Result<(), ArchiveInstallError> {
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
    if !paths.iter().any(|path| is_cpu_backend_file(path)) {
        return Err(ArchiveInstallError::Invalid(
            "missing CPU backend DLL".into(),
        ));
    }
    let selected = match pack.backend.as_str() {
        "cpu" => None,
        "cuda" => Some("ggml-cuda.dll"),
        "vulkan" => Some("ggml-vulkan.dll"),
        _ => return Err(ArchiveInstallError::Invalid("unknown backend".into())),
    };
    if selected.is_some_and(|file| !paths.contains(file)) {
        return Err(ArchiveInstallError::Invalid(format!(
            "missing backend file {}",
            selected.unwrap()
        )));
    }
    for forbidden in ["ggml-cuda.dll", "ggml-vulkan.dll"] {
        if Some(forbidden) != selected && paths.contains(forbidden) {
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

fn read_pack_manifest(archive_path: &Path) -> Result<RuntimePackManifest, ArchiveInstallError> {
    let archive_file = fs::File::open(archive_path)?;
    let mut archive = ZipArchive::new(archive_file)?;
    let mut entry = archive
        .by_name(PACK_MANIFEST_NAME)
        .map_err(|_| ArchiveInstallError::Invalid("missing runtime-pack.json".into()))?;
    if entry.size() > 1024 * 1024 {
        return Err(ArchiveInstallError::Invalid(
            "runtime-pack.json is too large".into(),
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ArchiveInstallError::Invalid(format!("invalid runtime-pack.json: {error}"))
    })
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
        if name == PACK_MANIFEST_NAME {
            let target = staging_path.join(PACK_MANIFEST_NAME);
            let mut output = fs::File::create(target)?;
            std::io::copy(&mut entry, &mut output)?;
            continue;
        }
        let expected = declared
            .get(name.as_str())
            .ok_or_else(|| ArchiveInstallError::Invalid(format!("undeclared ZIP entry {name}")))?;
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
    if seen.len() != declared.len() + 1 || !seen.contains(PACK_MANIFEST_NAME) {
        return Err(ArchiveInstallError::Invalid(
            "archive is missing declared files".into(),
        ));
    }
    Ok(())
}

fn existing_matches(
    directory: &Path,
    expected_manifest: &RuntimePackManifest,
) -> Result<bool, ArchiveInstallError> {
    let manifest_path = directory.join(PACK_MANIFEST_NAME);
    let Ok(bytes) = fs::read(&manifest_path) else {
        return Ok(false);
    };
    let Ok(actual_manifest) = serde_json::from_slice::<RuntimePackManifest>(&bytes) else {
        return Ok(false);
    };
    if &actual_manifest != expected_manifest {
        return Ok(false);
    }
    let declared_files = &expected_manifest.files;
    let mut actual_files = Vec::new();
    collect_files(directory, directory, &mut actual_files)?;
    if actual_files.len() != declared_files.len() + 1 {
        return Ok(false);
    }
    let declared: HashMap<&str, &RuntimeManifestFile> = declared_files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    for (relative, path) in actual_files {
        if relative == PACK_MANIFEST_NAME {
            continue;
        }
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
        return Err(ArchiveInstallError::Invalid(format!("unsafe path {value}")));
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

    use crate::runtime_manifest::{RuntimeManifestFile, RuntimeManifestPack, RuntimePackManifest};

    use super::{
        install_verified_archive, install_verified_archive_with_mode_and_probe,
        validate_installed_pack, InstallArchiveResult,
    };

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

    fn pack(_files: &[(&str, &[u8])]) -> RuntimeManifestPack {
        RuntimeManifestPack {
            id: "cuda".into(),
            backend: "cuda".into(),
            pack_version: "2026.07.1".into(),
            platform: "windows".into(),
            arch: "x86_64".into(),
            llama_cpp_release: "b10068".into(),
            llama_cpp_commit: "571d0d540df04f25298d0e159e520d9fc62ed121".into(),
            abi_major: 1,
            abi_minor: 3,
            asset_name: "cuda.zip".into(),
            size: 1,
            sha256: "0".repeat(64),
        }
    }

    fn internal(files: &[(&str, &[u8])]) -> RuntimePackManifest {
        RuntimePackManifest {
            schema_version: 1,
            id: "cuda".into(),
            backend: "cuda".into(),
            pack_version: "2026.07.1".into(),
            platform: "windows".into(),
            arch: "x86_64".into(),
            llama_cpp_release: "b10068".into(),
            llama_cpp_commit: "571d0d540df04f25298d0e159e520d9fc62ed121".into(),
            abi_major: 1,
            abi_minor: 3,
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

    fn zip_with_manifest(path: &Path, files: &[(&str, &[u8])], manifest: &RuntimePackManifest) {
        let file = fs::File::create(path).expect("create zip");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("runtime-pack.json", SimpleFileOptions::default())
            .expect("start pack manifest");
        writer
            .write_all(&serde_json::to_vec(manifest).unwrap())
            .expect("write pack manifest");
        for (name, contents) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start zip file");
            writer.write_all(contents).expect("write zip file");
        }
        writer.finish().expect("finish zip");
    }

    fn zip(path: &Path, files: &[(&str, &[u8])]) {
        zip_with_manifest(path, files, &internal(files));
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
            fs::read(root.path().join("cuda/ggml-cuda.dll")).unwrap(),
            b"cuda"
        );
        assert!(!root.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("staging")));
    }

    #[test]
    fn accepts_official_cpu_variant_dll_names() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        let mut files = REQUIRED.to_vec();
        files[4] = ("ggml-cpu-x64.dll", b"cpu variant");
        zip(&archive, &files);

        install_verified_archive(&archive, root.path(), &pack(&files))
            .expect("install official llama.cpp layout");
    }

    #[test]
    fn rejects_traversal_duplicate_and_undeclared_entries() {
        for extra in ["../escape.dll", "C:/escape.dll", "surprise.dll"] {
            let root = TempDir::new().expect("runtime root");
            let archive = root.path().join("pack.zip");
            let mut files = REQUIRED.to_vec();
            files.push((extra, b"bad"));
            zip_with_manifest(&archive, &files, &internal(REQUIRED));
            assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());
            assert!(!root.path().join("cuda").exists());
        }

        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("duplicate.zip");
        zip(&archive, REQUIRED);
        let mut duplicate_manifest = internal(REQUIRED);
        duplicate_manifest
            .files
            .push(duplicate_manifest.files[0].clone());
        zip_with_manifest(&archive, REQUIRED, &duplicate_manifest);
        assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());
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
        let mut bad_manifest = internal(REQUIRED);
        bad_manifest.files[0].sha256 = "f".repeat(64);
        zip_with_manifest(&archive, REQUIRED, &bad_manifest);
        assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());

        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("mixed.zip");
        let mut mixed = REQUIRED.to_vec();
        mixed.push(("ggml-vulkan.dll", b"vulkan"));
        zip(&archive, &mixed);
        assert!(install_verified_archive(&archive, root.path(), &pack(&mixed)).is_err());
    }

    #[test]
    fn treats_identical_existing_pack_as_installed_and_repairs_conflict() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);
        let manifest = pack(REQUIRED);
        install_verified_archive(&archive, root.path(), &manifest).expect("first install");
        assert_eq!(
            install_verified_archive(&archive, root.path(), &manifest).unwrap(),
            InstallArchiveResult::AlreadyInstalled
        );

        fs::write(root.path().join("cuda/ggml-cuda.dll"), b"changed").unwrap();
        assert_eq!(
            install_verified_archive(&archive, root.path(), &manifest).unwrap(),
            InstallArchiveResult::Installed
        );
        assert_eq!(
            fs::read(root.path().join("cuda/ggml-cuda.dll")).unwrap(),
            b"cuda"
        );
    }

    #[test]
    fn rejects_catalog_and_internal_identity_mismatch() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        let mut manifest = internal(REQUIRED);
        manifest.abi_minor = 4;
        zip_with_manifest(&archive, REQUIRED, &manifest);
        assert!(install_verified_archive(&archive, root.path(), &pack(REQUIRED)).is_err());
    }

    #[test]
    fn installed_pack_must_match_the_current_bundled_baseline() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);
        install_verified_archive(&archive, root.path(), &pack(REQUIRED)).unwrap();
        let manifest_path = root.path().join("cuda/runtime-pack.json");
        let mut manifest: RuntimePackManifest =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest.llama_cpp_commit = "stale-commit".into();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert!(!validate_installed_pack(&root.path().join("cuda"), "cuda"));
    }

    #[test]
    fn verified_existing_pack_clears_a_stale_repair_marker() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);
        let manifest = pack(REQUIRED);
        install_verified_archive(&archive, root.path(), &manifest).unwrap();
        fs::write(root.path().join(".repair-required-cuda"), b"").unwrap();

        assert_eq!(
            install_verified_archive(&archive, root.path(), &manifest).unwrap(),
            InstallArchiveResult::AlreadyInstalled
        );
        assert!(!root.path().join(".repair-required-cuda").exists());
    }

    #[test]
    fn live_probe_must_succeed_before_staging_is_promoted() {
        let root = TempDir::new().expect("runtime root");
        let archive = root.path().join("pack.zip");
        zip(&archive, REQUIRED);

        let result = install_verified_archive_with_mode_and_probe(
            &archive,
            root.path(),
            &pack(REQUIRED),
            false,
            |directory, backend| {
                assert_eq!(backend, "cuda");
                assert!(directory.join("local_llm_runtime.dll").is_file());
                Err("DLL load failed".into())
            },
        );

        assert!(result.unwrap_err().to_string().contains("DLL load failed"));
        assert!(!root.path().join("cuda").exists());
    }
}
