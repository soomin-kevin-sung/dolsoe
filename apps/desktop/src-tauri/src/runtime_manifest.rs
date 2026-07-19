use std::collections::HashSet;

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct ManifestPolicy {
    pub app_version: String,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub platform: String,
    pub arch: String,
    pub llama_cpp_release: String,
    pub llama_cpp_commit: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundledBaseline {
    release_tag: String,
    commit: String,
    abi_major: u32,
    abi_minor: u32,
    platform: String,
    arch: String,
}

pub fn bundled_manifest_policy() -> Result<ManifestPolicy, String> {
    let baseline: BundledBaseline = serde_json::from_str(include_str!(
        "../../../../native/llm-runtime/llama-baseline.json"
    ))
    .map_err(|error| format!("invalid bundled llama.cpp baseline: {error}"))?;
    Ok(ManifestPolicy {
        app_version: env!("CARGO_PKG_VERSION").into(),
        abi_major: baseline.abi_major,
        abi_minor: baseline.abi_minor,
        platform: baseline.platform,
        arch: baseline.arch,
        llama_cpp_release: baseline.release_tag,
        llama_cpp_commit: baseline.commit,
    })
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimePackManifest {
    pub schema_version: u32,
    pub id: String,
    pub backend: String,
    pub pack_version: String,
    pub platform: String,
    pub arch: String,
    pub llama_cpp_release: String,
    pub llama_cpp_commit: String,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub files: Vec<RuntimeManifestFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeManifestPack {
    pub id: String,
    pub backend: String,
    pub pack_version: String,
    pub platform: String,
    pub arch: String,
    pub llama_cpp_release: String,
    pub llama_cpp_commit: String,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub asset_name: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCatalog {
    pub schema_version: u32,
    pub release_version: String,
    pub minimum_app_version: String,
    pub maximum_app_version: String,
    pub packs: Vec<RuntimeManifestPack>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("runtime manifest JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported runtime manifest schema {0}")]
    Schema(u32),
    #[error("app version {actual} is outside runtime manifest range {minimum}..={maximum}")]
    AppVersion {
        actual: String,
        minimum: String,
        maximum: String,
    },
    #[error("runtime pack identity does not match the supported baseline: {0}")]
    Identity(String),
    #[error("invalid SHA-256 for {0}")]
    Sha256(String),
    #[error("duplicate runtime backend {0}")]
    DuplicateBackend(String),
    #[error("invalid runtime manifest field: {0}")]
    Field(String),
}

impl RuntimeCatalog {
    pub fn parse(raw: &[u8], policy: &ManifestPolicy) -> Result<Self, ManifestError> {
        let catalog: Self = serde_json::from_slice(raw)?;
        catalog.validate(policy)?;
        Ok(catalog)
    }

    fn validate(&self, policy: &ManifestPolicy) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::Schema(self.schema_version));
        }
        if !app_version_matches(
            &policy.app_version,
            &self.minimum_app_version,
            &self.maximum_app_version,
        ) {
            return Err(ManifestError::AppVersion {
                actual: policy.app_version.clone(),
                minimum: self.minimum_app_version.clone(),
                maximum: self.maximum_app_version.clone(),
            });
        }
        let mut backends = HashSet::new();
        for pack in &self.packs {
            pack.validate_identity(policy)?;
            if !backends.insert(pack.backend.as_str()) {
                return Err(ManifestError::DuplicateBackend(pack.backend.clone()));
            }
            validate_asset_name(&pack.asset_name)?;
            validate_sha256(&pack.sha256).map_err(|_| ManifestError::Sha256(pack.id.clone()))?;
            if pack.size == 0 {
                return Err(ManifestError::Field(format!("empty pack {}", pack.id)));
            }
        }
        Ok(())
    }
}

impl RuntimeManifestPack {
    pub fn validate_identity(&self, policy: &ManifestPolicy) -> Result<(), ManifestError> {
        if !matches!(self.backend.as_str(), "cpu" | "cuda" | "vulkan") || self.id != self.backend {
            return Err(ManifestError::Identity(self.id.clone()));
        }
        if self.platform != policy.platform
            || self.arch != policy.arch
            || self.llama_cpp_release != policy.llama_cpp_release
            || self.llama_cpp_commit != policy.llama_cpp_commit
            || self.abi_major != policy.abi_major
            || self.abi_minor != policy.abi_minor
            || self.pack_version.trim().is_empty()
        {
            return Err(ManifestError::Identity(self.id.clone()));
        }
        Ok(())
    }

    pub fn matches_internal(&self, internal: &RuntimePackManifest) -> bool {
        internal.schema_version == SCHEMA_VERSION
            && self.id == internal.id
            && self.backend == internal.backend
            && self.pack_version == internal.pack_version
            && self.platform == internal.platform
            && self.arch == internal.arch
            && self.llama_cpp_release == internal.llama_cpp_release
            && self.llama_cpp_commit == internal.llama_cpp_commit
            && self.abi_major == internal.abi_major
            && self.abi_minor == internal.abi_minor
    }
}

impl RuntimePackManifest {
    pub fn matches_policy(&self, policy: &ManifestPolicy) -> bool {
        self.schema_version == SCHEMA_VERSION
            && self.id == self.backend
            && matches!(self.backend.as_str(), "cpu" | "cuda" | "vulkan")
            && self.platform == policy.platform
            && self.arch == policy.arch
            && self.llama_cpp_release == policy.llama_cpp_release
            && self.llama_cpp_commit == policy.llama_cpp_commit
            && self.abi_major == policy.abi_major
            && self.abi_minor == policy.abi_minor
            && !self.pack_version.trim().is_empty()
    }
}

fn validate_asset_name(value: &str) -> Result<(), ManifestError> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ManifestError::Field("asset name".into()));
    }
    Ok(())
}

pub fn validate_sha256(value: &str) -> Result<(), ()> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(())
    }
}

fn app_version_matches(actual: &str, minimum: &str, maximum: &str) -> bool {
    let (Ok(actual), Ok(minimum)) = (Version::parse(actual), Version::parse(minimum)) else {
        return false;
    };
    if actual < minimum {
        return false;
    }
    if let Some(prefix) = maximum.strip_suffix(".x") {
        let mut parts = prefix.split('.');
        let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        return actual.major.to_string() == major && actual.minor.to_string() == minor;
    }
    Version::parse(maximum).is_ok_and(|maximum| actual <= maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn policy() -> ManifestPolicy {
        ManifestPolicy {
            app_version: "0.1.0".into(),
            abi_major: 1,
            abi_minor: 1,
            platform: "windows".into(),
            arch: "x86_64".into(),
            llama_cpp_release: "b10068".into(),
            llama_cpp_commit: "571d0d540df04f25298d0e159e520d9fc62ed121".into(),
        }
    }

    fn catalog(overrides: &str) -> Vec<u8> {
        format!(r#"{{"schemaVersion":1,"releaseVersion":"2026.07.1","minimumAppVersion":"0.1.0","maximumAppVersion":"0.1.x","packs":[{{"id":"cuda","backend":"cuda","packVersion":"2026.07.1","platform":"windows","arch":"x86_64","llamaCppRelease":"b10068","llamaCppCommit":"571d0d540df04f25298d0e159e520d9fc62ed121","abiMajor":1,"abiMinor":1,"assetName":"cuda.zip","size":123,"sha256":"{HASH}"{overrides}}}]}}"#).into_bytes()
    }

    #[test]
    fn parses_catalog_with_stable_exact_baseline_identity() {
        let parsed = RuntimeCatalog::parse(&catalog(""), &policy()).unwrap();
        assert_eq!(parsed.packs[0].id, "cuda");
    }

    #[test]
    fn rejects_identity_digest_asset_and_version_mismatches() {
        for (from, to) in [
            ("\"id\":\"cuda\"", "\"id\":\"cuda-v1\""),
            ("\"abiMinor\":1", "\"abiMinor\":2"),
            ("b10068", "b99999"),
            (HASH, "ABCDEF"),
            ("cuda.zip", "../cuda.zip"),
        ] {
            let value = String::from_utf8(catalog(""))
                .unwrap()
                .replacen(from, to, 1);
            assert!(
                RuntimeCatalog::parse(value.as_bytes(), &policy()).is_err(),
                "accepted {from} -> {to}"
            );
        }
        let mut incompatible = policy();
        incompatible.app_version = "0.2.0".into();
        assert!(RuntimeCatalog::parse(&catalog(""), &incompatible).is_err());
    }

    #[test]
    fn external_and_internal_identity_must_match() {
        let external = RuntimeCatalog::parse(&catalog(""), &policy())
            .unwrap()
            .packs
            .remove(0);
        let mut internal = RuntimePackManifest {
            schema_version: 1,
            id: "cuda".into(),
            backend: "cuda".into(),
            pack_version: "2026.07.1".into(),
            platform: "windows".into(),
            arch: "x86_64".into(),
            llama_cpp_release: "b10068".into(),
            llama_cpp_commit: policy().llama_cpp_commit,
            abi_major: 1,
            abi_minor: 1,
            files: vec![],
        };
        assert!(external.matches_internal(&internal));
        internal.abi_minor = 2;
        assert!(!external.matches_internal(&internal));
    }
}
