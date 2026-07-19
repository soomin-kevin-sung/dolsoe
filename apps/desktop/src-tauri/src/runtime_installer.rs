use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::Client;
use serde::Serialize;
use thiserror::Error;

use crate::{
    runtime_archive::install_verified_archive,
    runtime_download::{download_verified_archive, DownloadError},
    runtime_manifest::{ManifestPolicy, RuntimeManifestPack, SignedRuntimeManifest},
};

const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallPhase {
    Downloading,
    Verifying,
    Installing,
    Installed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInstallProgress {
    pub pack_id: String,
    pub phase: InstallPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeDistributionConfig {
    pub manifest_url: String,
    pub signature_url: String,
    pub public_key: [u8; 32],
    pub policy: ManifestPolicy,
}

impl RuntimeDistributionConfig {
    pub fn from_compile_time() -> Result<Self, String> {
        let manifest_url = option_env!("LLW_RUNTIME_MANIFEST_URL")
            .ok_or("runtime distribution manifest URL is not configured")?
            .to_owned();
        let signature_url = option_env!("LLW_RUNTIME_MANIFEST_SIGNATURE_URL")
            .ok_or("runtime distribution signature URL is not configured")?
            .to_owned();
        let encoded_key = option_env!("LLW_RUNTIME_MANIFEST_PUBLIC_KEY")
            .ok_or("runtime distribution public key is not configured")?;
        let decoded = STANDARD
            .decode(encoded_key)
            .map_err(|_| "runtime distribution public key is not valid base64")?;
        let public_key = decoded
            .try_into()
            .map_err(|_| "runtime distribution public key must contain 32 bytes")?;
        Ok(Self {
            manifest_url,
            signature_url,
            public_key,
            policy: ManifestPolicy {
                app_version: env!("CARGO_PKG_VERSION").into(),
                abi_major: 1,
                platform: "windows".into(),
                arch: "x86_64".into(),
            },
        })
    }
}

#[derive(Debug)]
pub struct ActiveInstall {
    pack_id: String,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
pub struct InstallCoordinator {
    active: Mutex<Option<ActiveInstall>>,
}

impl InstallCoordinator {
    pub fn begin(&self, pack_id: &str) -> Result<ActiveInstall, String> {
        let mut active = self.active.lock().map_err(|_| "installer lock poisoned")?;
        if active.is_some() {
            return Err("another runtime pack installation is already in progress".into());
        }
        let token = ActiveInstall {
            pack_id: pack_id.into(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        *active = Some(ActiveInstall {
            pack_id: token.pack_id.clone(),
            cancelled: token.cancelled.clone(),
        });
        Ok(token)
    }

    pub fn cancel(&self) -> Result<(), String> {
        let active = self.active.lock().map_err(|_| "installer lock poisoned")?;
        let active = active
            .as_ref()
            .ok_or("no runtime pack installation is in progress")?;
        active.cancelled.store(true, Ordering::Release);
        Ok(())
    }

    pub fn finish(&self, pack_id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if active
                .as_ref()
                .is_some_and(|active| active.pack_id == pack_id)
            {
                *active = None;
            }
        }
    }

    #[cfg(test)]
    pub fn active_pack_id(&self) -> Option<String> {
        self.active
            .lock()
            .ok()
            .and_then(|active| active.as_ref().map(|active| active.pack_id.clone()))
    }
}

#[derive(Debug, Error)]
pub enum RuntimeInstallerError {
    #[error("runtime distribution request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("runtime distribution response exceeded {0} bytes")]
    ResponseTooLarge(usize),
    #[error("runtime manifest failed validation: {0}")]
    Manifest(String),
    #[error("runtime pack {0} is not available")]
    UnknownPack(String),
    #[error("runtime pack download failed: {0}")]
    Download(#[from] DownloadError),
    #[error("runtime pack installation failed: {0}")]
    Install(String),
    #[error("runtime installer is busy: {0}")]
    Busy(String),
}

pub struct RuntimeInstaller {
    client: Client,
    runtime_root: PathBuf,
    config: RuntimeDistributionConfig,
    coordinator: Arc<InstallCoordinator>,
}

#[derive(Debug, Clone)]
pub struct AvailableRuntimeRelease {
    pub release_version: String,
    pub packs: Vec<RuntimeManifestPack>,
}

impl RuntimeInstaller {
    pub fn new(runtime_root: PathBuf, config: RuntimeDistributionConfig) -> Self {
        Self {
            client: Client::new(),
            runtime_root,
            config,
            coordinator: Arc::new(InstallCoordinator::default()),
        }
    }

    pub async fn available_packs(&self) -> Result<AvailableRuntimeRelease, RuntimeInstallerError> {
        let manifest = self.fetch_manifest().await?;
        let release_version = manifest.release_version.clone();
        let packs = manifest
            .packs
            .into_iter()
            .filter(|pack| {
                pack.platform == self.config.policy.platform && pack.arch == self.config.policy.arch
            })
            .collect();
        Ok(AvailableRuntimeRelease {
            release_version,
            packs,
        })
    }

    pub async fn install<F>(&self, pack_id: &str, emit: F) -> Result<(), RuntimeInstallerError>
    where
        F: Fn(RuntimeInstallProgress) + Send + Sync,
    {
        let token = self
            .coordinator
            .begin(pack_id)
            .map_err(RuntimeInstallerError::Busy)?;
        let result = self.install_inner(pack_id, &token, &emit).await;
        self.coordinator.finish(pack_id);
        match &result {
            Ok(()) => emit(progress(pack_id, InstallPhase::Installed, 0, 0, None)),
            Err(RuntimeInstallerError::Download(DownloadError::Cancelled)) => {
                emit(progress(pack_id, InstallPhase::Cancelled, 0, 0, None))
            }
            Err(error) => emit(progress(
                pack_id,
                InstallPhase::Failed,
                0,
                0,
                Some(error.to_string()),
            )),
        }
        result
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.coordinator.cancel()
    }

    async fn install_inner<F>(
        &self,
        pack_id: &str,
        token: &ActiveInstall,
        emit: &F,
    ) -> Result<(), RuntimeInstallerError>
    where
        F: Fn(RuntimeInstallProgress) + Send + Sync,
    {
        let pack = self
            .available_packs()
            .await?
            .packs
            .into_iter()
            .find(|pack| pack.id == pack_id)
            .ok_or_else(|| RuntimeInstallerError::UnknownPack(pack_id.into()))?;
        let download_directory = self.runtime_root.join(".downloads");
        let archive_path = download_directory.join(format!("{}.zip.part", pack.id));
        download_verified_archive(
            &self.client,
            &pack.asset_url,
            &archive_path,
            pack.size,
            &pack.sha256,
            token.cancelled.clone(),
            |downloaded, total| {
                emit(progress(
                    pack_id,
                    InstallPhase::Downloading,
                    downloaded,
                    total,
                    None,
                ));
            },
        )
        .await?;
        emit(progress(
            pack_id,
            InstallPhase::Verifying,
            pack.size,
            pack.size,
            None,
        ));
        if token.cancelled.load(Ordering::Acquire) {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(DownloadError::Cancelled.into());
        }
        emit(progress(
            pack_id,
            InstallPhase::Installing,
            pack.size,
            pack.size,
            None,
        ));
        let runtime_root = self.runtime_root.clone();
        let archive_for_install = archive_path.clone();
        tokio::task::spawn_blocking(move || {
            install_verified_archive(&archive_for_install, &runtime_root, &pack)
        })
        .await
        .map_err(|error| RuntimeInstallerError::Install(error.to_string()))?
        .map_err(|error| RuntimeInstallerError::Install(error.to_string()))?;
        let _ = tokio::fs::remove_file(archive_path).await;
        Ok(())
    }

    async fn fetch_manifest(&self) -> Result<SignedRuntimeManifest, RuntimeInstallerError> {
        let raw =
            fetch_bounded(&self.client, &self.config.manifest_url, MAX_MANIFEST_BYTES).await?;
        let signature = fetch_bounded(
            &self.client,
            &self.config.signature_url,
            MAX_SIGNATURE_BYTES,
        )
        .await?;
        SignedRuntimeManifest::verify_and_parse(
            &raw,
            &signature,
            &self.config.public_key,
            &self.config.policy,
        )
        .map_err(|error| RuntimeInstallerError::Manifest(error.to_string()))
    }
}

async fn fetch_bounded(
    client: &Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, RuntimeInstallerError> {
    let response = client.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(RuntimeInstallerError::ResponseTooLarge(limit));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > limit {
        return Err(RuntimeInstallerError::ResponseTooLarge(limit));
    }
    Ok(bytes.to_vec())
}

fn progress(
    pack_id: &str,
    phase: InstallPhase,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
) -> RuntimeInstallProgress {
    RuntimeInstallProgress {
        pack_id: pack_id.into(),
        phase,
        downloaded_bytes,
        total_bytes,
        error,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{InstallCoordinator, InstallPhase, RuntimeInstallProgress};

    #[test]
    fn coordinator_allows_one_install_and_cleans_up_terminal_state() {
        let coordinator = InstallCoordinator::default();
        let first = coordinator.begin("cuda-1").expect("begin first install");
        assert!(coordinator
            .begin("vulkan-1")
            .unwrap_err()
            .contains("progress"));
        assert_eq!(coordinator.active_pack_id().as_deref(), Some("cuda-1"));

        coordinator.cancel().expect("cancel active install");
        assert!(first.cancelled.load(Ordering::Acquire));
        coordinator.finish("cuda-1");
        assert_eq!(coordinator.active_pack_id(), None);
        assert!(coordinator.begin("vulkan-1").is_ok());
    }

    #[test]
    fn progress_serializes_with_camel_case_byte_counts() {
        let value = serde_json::to_value(RuntimeInstallProgress {
            pack_id: "cuda-1".into(),
            phase: InstallPhase::Downloading,
            downloaded_bytes: 12,
            total_bytes: 24,
            error: None,
        })
        .unwrap();
        assert_eq!(value["packId"], "cuda-1");
        assert_eq!(value["phase"], "downloading");
        assert_eq!(value["downloadedBytes"], 12);
        assert_eq!(value["totalBytes"], 24);
    }
}
