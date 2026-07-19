use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

const SOURCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSource {
    pub schema_version: u32,
    pub provider: String,
    pub repository: String,
    pub release_tag: String,
    pub manifest_asset: String,
    pub manifest_sha256: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeSourceError {
    #[error("runtime source is not valid JSON: {0}")]
    Json(String),
    #[error("runtime source contains an invalid {0}")]
    InvalidField(&'static str),
}

impl RuntimeSource {
    pub fn parse(bytes: &[u8]) -> Result<Self, RuntimeSourceError> {
        let source: Self = serde_json::from_slice(bytes)
            .map_err(|error| RuntimeSourceError::Json(error.to_string()))?;
        source.validate()?;
        Ok(source)
    }

    pub fn asset_url(&self, asset: &str) -> Result<String, RuntimeSourceError> {
        validate_token(asset, "asset name")?;
        Ok(format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.repository, self.release_tag, asset
        ))
    }

    pub fn manifest_url(&self) -> Result<String, RuntimeSourceError> {
        self.asset_url(&self.manifest_asset)
    }

    fn validate(&self) -> Result<(), RuntimeSourceError> {
        if self.schema_version != SOURCE_SCHEMA_VERSION {
            return Err(RuntimeSourceError::InvalidField("schema version"));
        }
        if self.provider != "github-release" {
            return Err(RuntimeSourceError::InvalidField("provider"));
        }
        let mut parts = self.repository.split('/');
        let owner = parts.next().unwrap_or_default();
        let repository = parts.next().unwrap_or_default();
        if parts.next().is_some() {
            return Err(RuntimeSourceError::InvalidField("repository"));
        }
        validate_token(owner, "repository")?;
        validate_token(repository, "repository")?;
        validate_token(&self.release_tag, "release tag")?;
        validate_token(&self.manifest_asset, "manifest asset")?;
        if self.manifest_sha256.len() != 64
            || !self
                .manifest_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RuntimeSourceError::InvalidField("manifest sha256"));
        }
        Ok(())
    }
}

fn validate_token(value: &str, field: &'static str) -> Result<(), RuntimeSourceError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(RuntimeSourceError::InvalidField(field));
    }
    Ok(())
}

pub fn load_runtime_source(
    app_data: &Path,
    bundled_default: &[u8],
) -> Result<RuntimeSource, RuntimeSourceError> {
    let override_path = app_data.join("runtime-source.json");
    match std::fs::read(override_path) {
        Ok(bytes) => RuntimeSource::parse(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            RuntimeSource::parse(bundled_default)
        }
        Err(error) => Err(RuntimeSourceError::Json(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn source_json(repository: &str) -> Vec<u8> {
        format!(
            r#"{{"schemaVersion":1,"provider":"github-release","repository":"{repository}","releaseTag":"runtime-v1.0.0","manifestAsset":"runtime-manifest.json","manifestSha256":"{HASH}"}}"#
        )
        .into_bytes()
    }

    #[test]
    fn parses_source_and_constructs_release_asset_urls() {
        let source = RuntimeSource::parse(&source_json("owner/repo")).unwrap();
        assert_eq!(
            source.asset_url("cuda.zip").unwrap(),
            "https://github.com/owner/repo/releases/download/runtime-v1.0.0/cuda.zip"
        );
    }

    #[test]
    fn rejects_untrusted_source_fields_and_assets() {
        for repository in ["../repo", "owner", "owner/repo/extra", "owner repo/name"] {
            assert!(RuntimeSource::parse(&source_json(repository)).is_err());
        }
        let source = RuntimeSource::parse(&source_json("owner/repo")).unwrap();
        for asset in ["../cuda.zip", "/cuda.zip", "folder/cuda.zip", "cuda zip"] {
            assert!(source.asset_url(asset).is_err(), "accepted asset: {asset}");
        }
    }

    #[test]
    fn rejects_non_lowercase_or_wrong_length_manifest_digest() {
        let uppercase = source_json("owner/repo").to_vec();
        let mut value = String::from_utf8(uppercase).unwrap();
        value = value.replace(HASH, &HASH.to_uppercase());
        assert!(RuntimeSource::parse(value.as_bytes()).is_err());

        value = value.replace(&HASH.to_uppercase(), "abcd");
        assert!(RuntimeSource::parse(value.as_bytes()).is_err());
    }

    #[test]
    fn local_override_replaces_the_default_as_a_whole() {
        let root = TempDir::new().unwrap();
        std::fs::write(
            root.path().join("runtime-source.json"),
            source_json("moved/repo"),
        )
        .unwrap();
        let source = load_runtime_source(root.path(), &source_json("default/repo")).unwrap();
        assert_eq!(source.repository, "moved/repo");

        std::fs::write(root.path().join("runtime-source.json"), b"{}").unwrap();
        assert!(load_runtime_source(root.path(), &source_json("default/repo")).is_err());
    }
}
