use std::collections::HashSet;

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::runtime_path::validate_runtime_pack_id;

const SCHEMA_VERSION: u32 = 1;
const RELEASE_ASSET_PREFIX: &str =
    "https://github.com/soomin-sung-estsoft/local-llm-wiki/releases/download/";

#[derive(Debug, Clone)]
pub struct ManifestPolicy {
    pub app_version: String,
    pub abi_major: u32,
    pub platform: String,
    pub arch: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifestPack {
    pub id: String,
    pub backend: String,
    pub platform: String,
    pub arch: String,
    pub asset_url: String,
    pub size: u64,
    pub sha256: String,
    pub files: Vec<RuntimeManifestFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedRuntimeManifest {
    pub schema_version: u32,
    pub release_version: String,
    pub minimum_app_version: String,
    pub maximum_app_version: String,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub llama_cpp_commit: String,
    pub packs: Vec<RuntimeManifestPack>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("runtime manifest signature is invalid")]
    Signature,
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
    #[error("runtime manifest ABI major {actual} is incompatible with app ABI {expected}")]
    Abi { expected: u32, actual: u32 },
    #[error("invalid runtime pack ID {id}: {message}")]
    PackId { id: String, message: String },
    #[error("invalid runtime asset URL for {0}")]
    AssetUrl(String),
    #[error("invalid SHA-256 for {0}")]
    Sha256(String),
    #[error("duplicate runtime backend tuple {backend}/{platform}/{arch}")]
    DuplicateBackend {
        backend: String,
        platform: String,
        arch: String,
    },
    #[error("invalid runtime manifest field: {0}")]
    Field(String),
}

impl SignedRuntimeManifest {
    pub fn verify_and_parse(
        raw: &[u8],
        signature_base64: &[u8],
        public_key: &[u8],
        policy: &ManifestPolicy,
    ) -> Result<Self, ManifestError> {
        let key_bytes: [u8; 32] = public_key
            .try_into()
            .map_err(|_| ManifestError::Signature)?;
        let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| ManifestError::Signature)?;
        let encoded =
            std::str::from_utf8(signature_base64).map_err(|_| ManifestError::Signature)?;
        let decoded = STANDARD
            .decode(encoded.trim())
            .map_err(|_| ManifestError::Signature)?;
        let signature = Signature::from_slice(&decoded).map_err(|_| ManifestError::Signature)?;
        key.verify(raw, &signature)
            .map_err(|_| ManifestError::Signature)?;

        let manifest: Self = serde_json::from_slice(raw)?;
        manifest.validate(policy)?;
        Ok(manifest)
    }

    fn validate(&self, policy: &ManifestPolicy) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::Schema(self.schema_version));
        }
        if self.abi_major != policy.abi_major {
            return Err(ManifestError::Abi {
                expected: policy.abi_major,
                actual: self.abi_major,
            });
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
        if self.release_version.trim().is_empty() || self.llama_cpp_commit.len() != 40 {
            return Err(ManifestError::Field("release metadata".into()));
        }

        let mut tuples = HashSet::new();
        for pack in &self.packs {
            validate_runtime_pack_id(&pack.id).map_err(|message| ManifestError::PackId {
                id: pack.id.clone(),
                message,
            })?;
            if !matches!(pack.backend.as_str(), "cpu" | "cuda" | "vulkan") {
                return Err(ManifestError::Field(format!(
                    "unsupported backend {}",
                    pack.backend
                )));
            }
            validate_asset_url(&pack.asset_url)
                .map_err(|_| ManifestError::AssetUrl(pack.id.clone()))?;
            validate_sha256(&pack.sha256)
                .map_err(|_| ManifestError::Sha256(format!("{} archive", pack.id)))?;
            if pack.size == 0 || pack.files.is_empty() {
                return Err(ManifestError::Field(format!("empty pack {}", pack.id)));
            }
            for file in &pack.files {
                if file.path.is_empty() || file.size == 0 {
                    return Err(ManifestError::Field(format!("invalid file in {}", pack.id)));
                }
                validate_sha256(&file.sha256).map_err(|_| {
                    ManifestError::Sha256(format!("{} file {}", pack.id, file.path))
                })?;
            }
            let key = (&pack.backend, &pack.platform, &pack.arch);
            if !tuples.insert(key) {
                return Err(ManifestError::DuplicateBackend {
                    backend: pack.backend.clone(),
                    platform: pack.platform.clone(),
                    arch: pack.arch.clone(),
                });
            }
        }

        if !self
            .packs
            .iter()
            .any(|pack| pack.platform == policy.platform && pack.arch == policy.arch)
        {
            return Err(ManifestError::Field("no pack for this platform".into()));
        }
        Ok(())
    }
}

fn validate_asset_url(value: &str) -> Result<(), ()> {
    let parsed = Url::parse(value).map_err(|_| ())?;
    if parsed.scheme() != "https"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !value.starts_with(RELEASE_ASSET_PREFIX)
    {
        return Err(());
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), ()> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(())
    }
}

fn app_version_matches(actual: &str, minimum: &str, maximum: &str) -> bool {
    let Ok(actual) = Version::parse(actual) else {
        return false;
    };
    let Ok(minimum) = Version::parse(minimum) else {
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
    use base64::{engine::general_purpose::STANDARD, Engine};
    use ed25519_dalek::{Signer, SigningKey};

    use super::{ManifestPolicy, SignedRuntimeManifest};

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn manifest(pack_overrides: &str, root_overrides: &str) -> Vec<u8> {
        format!(
            r#"{{
  "schemaVersion": 1,
  "releaseVersion": "2026.07.1",
  "minimumAppVersion": "0.1.0",
  "maximumAppVersion": "0.1.x",
  "abiMajor": 1,
  "abiMinor": 1,
  "llamaCppCommit": "6bdd77f13cf11b264b4231d320afc404f48d576e"{root_overrides},
  "packs": [{{
    "id": "cuda-2026.07.1",
    "backend": "cuda",
    "platform": "windows",
    "arch": "x86_64",
    "assetUrl": "https://github.com/soomin-sung-estsoft/local-llm-wiki/releases/download/runtime-v2026.07.1/local-llm-wiki-runtime-2026.07.1-windows-x86_64-cuda.zip",
    "size": 123,
    "sha256": "{HASH}",
    "files": [{{"path":"local_llm_runtime.dll","size":12,"sha256":"{HASH}"}}]{pack_overrides}
  }}]
}}"#
        )
        .into_bytes()
    }

    fn verify(raw: &[u8], policy: ManifestPolicy) -> Result<SignedRuntimeManifest, String> {
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let signature = STANDARD.encode(signing.sign(raw).to_bytes());
        SignedRuntimeManifest::verify_and_parse(
            raw,
            signature.as_bytes(),
            &signing.verifying_key().to_bytes(),
            &policy,
        )
        .map_err(|error| error.to_string())
    }

    fn policy() -> ManifestPolicy {
        ManifestPolicy {
            app_version: "0.1.0".into(),
            abi_major: 1,
            platform: "windows".into(),
            arch: "x86_64".into(),
        }
    }

    #[test]
    fn verifies_signature_and_parses_compatible_manifest() {
        let parsed = verify(&manifest("", ""), policy()).expect("valid signed manifest");
        assert_eq!(parsed.release_version, "2026.07.1");
        assert_eq!(parsed.packs.len(), 1);
        assert_eq!(parsed.packs[0].id, "cuda-2026.07.1");
    }

    #[test]
    fn rejects_modified_manifest_and_unsupported_schema() {
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let raw = manifest("", "");
        let signature = STANDARD.encode(signing.sign(&raw).to_bytes());
        let mut modified = raw.clone();
        modified[20] ^= 1;
        let error = SignedRuntimeManifest::verify_and_parse(
            &modified,
            signature.as_bytes(),
            &signing.verifying_key().to_bytes(),
            &policy(),
        )
        .expect_err("modified bytes must fail");
        assert!(error.to_string().contains("signature"));

        let raw = String::from_utf8(manifest("", ""))
            .unwrap()
            .replace("\"schemaVersion\": 1", "\"schemaVersion\": 2")
            .into_bytes();
        assert!(verify(&raw, policy()).unwrap_err().contains("schema"));
    }

    #[test]
    fn rejects_incompatible_app_and_abi_versions() {
        let mut old_app = policy();
        old_app.app_version = "0.2.0".into();
        assert!(verify(&manifest("", ""), old_app)
            .unwrap_err()
            .contains("app version"));

        let mut wrong_abi = policy();
        wrong_abi.abi_major = 2;
        assert!(verify(&manifest("", ""), wrong_abi)
            .unwrap_err()
            .contains("ABI"));
    }

    #[test]
    fn rejects_duplicate_backend_and_untrusted_pack_fields() {
        let duplicate = r#",{
          "id":"cuda-other","backend":"cuda","platform":"windows","arch":"x86_64",
          "assetUrl":"https://github.com/soomin-sung-estsoft/local-llm-wiki/releases/download/runtime-v2026.07.1/other.zip",
          "size":1,"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
          "files":[{"path":"local_llm_runtime.dll","size":1,"sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}]
        }"#;
        let raw = String::from_utf8(manifest("", ""))
            .unwrap()
            .replace("  }]\n}", &format!("  }}{duplicate}]\n}}"))
            .into_bytes();
        assert!(verify(&raw, policy()).unwrap_err().contains("duplicate"));

        let invalid_id = String::from_utf8(manifest("", ""))
            .unwrap()
            .replace("\"id\": \"cuda-2026.07.1\"", "\"id\": \"../cuda\"")
            .into_bytes();
        assert!(verify(&invalid_id, policy())
            .unwrap_err()
            .contains("pack ID"));
        let invalid_url = String::from_utf8(manifest("", ""))
            .unwrap()
            .replace(
                "https://github.com/soomin-sung-estsoft/local-llm-wiki/releases/download/runtime-v2026.07.1/local-llm-wiki-runtime-2026.07.1-windows-x86_64-cuda.zip",
                "https://example.com/runtime.zip",
            )
            .into_bytes();
        assert!(verify(&invalid_url, policy())
            .unwrap_err()
            .contains("asset URL"));
        let invalid_hash = String::from_utf8(manifest("", ""))
            .unwrap()
            .replacen(HASH, "bad", 1)
            .into_bytes();
        assert!(verify(&invalid_hash, policy())
            .unwrap_err()
            .contains("SHA-256"));
    }
}
