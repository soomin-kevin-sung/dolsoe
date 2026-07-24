use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DEFAULT_MANIFEST: &str = include_str!("../resources/personas/dolsoe/manifest.json");
const DEFAULT_SOUL: &str = include_str!("../resources/personas/dolsoe/soul.md");
const DEFAULT_DOLSOE: &str = include_str!("../resources/personas/dolsoe/dolsoe.md");
const DEFAULT_SETTINGS: &str = r#"{"enabled":true}"#;
const MAX_DOCUMENT_BYTES: usize = 32 * 1024;
const MAX_COMPILED_BYTES: usize = 64 * 1024;
const MAX_DOCUMENTS: usize = 8;

pub type PersonaResult<T> = Result<T, String>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonaManifest {
    id: String,
    name: String,
    version: u32,
    files: Vec<PersonaManifestFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersonaManifestFile {
    path: String,
    label: String,
    description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersonaSettings {
    enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDocumentDraft {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaPromptDraft {
    pub enabled: bool,
    pub documents: Vec<PersonaDocumentDraft>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDocumentDto {
    pub path: String,
    pub label: String,
    pub description: String,
    pub content: String,
    pub character_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaPromptStateDto {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub enabled: bool,
    pub revision: String,
    pub compiled_prompt: String,
    pub character_count: usize,
    pub estimated_tokens: usize,
    pub directory_path: String,
    pub documents: Vec<PersonaDocumentDto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPersonaPrompt {
    pub persona_id: String,
    pub revision: String,
    pub content: String,
}

#[derive(Clone)]
pub struct PersonaPromptStore {
    profile_root: Arc<PathBuf>,
    settings_path: Arc<PathBuf>,
    gate: Arc<Mutex<()>>,
}

impl PersonaPromptStore {
    pub fn bootstrap(app_data: impl AsRef<Path>) -> PersonaResult<Self> {
        let personas_root = app_data.as_ref().join("personas");
        let profile_root = personas_root.join("dolsoe");
        fs::create_dir_all(&profile_root).map_err(file_error)?;
        seed_if_missing(&profile_root.join("manifest.json"), DEFAULT_MANIFEST)?;
        seed_if_missing(&profile_root.join("soul.md"), DEFAULT_SOUL)?;
        seed_if_missing(&profile_root.join("dolsoe.md"), DEFAULT_DOLSOE)?;
        let settings_path = personas_root.join("settings.json");
        seed_if_missing(&settings_path, DEFAULT_SETTINGS)?;
        let store = Self {
            profile_root: Arc::new(profile_root),
            settings_path: Arc::new(settings_path),
            gate: Arc::new(Mutex::new(())),
        };
        store.state()?;
        Ok(store)
    }

    pub fn state(&self) -> PersonaResult<PersonaPromptStateDto> {
        let _guard = self.lock()?;
        self.load_state()
    }

    pub fn preview(&self, draft: PersonaPromptDraft) -> PersonaResult<PersonaPromptStateDto> {
        let _guard = self.lock()?;
        let manifest = self.read_manifest()?;
        state_from_draft(&self.profile_root, manifest, draft)
    }

    pub fn save(&self, draft: PersonaPromptDraft) -> PersonaResult<PersonaPromptStateDto> {
        let _guard = self.lock()?;
        let manifest = self.read_manifest()?;
        validate_draft(&manifest, &draft)?;
        for document in &draft.documents {
            fs::write(
                self.profile_root.join(&document.path),
                document.content.as_bytes(),
            )
            .map_err(file_error)?;
        }
        let settings = serde_json::to_vec_pretty(&PersonaSettings {
            enabled: draft.enabled,
        })
        .map_err(|error| format!("페르소나 설정을 직렬화하지 못했습니다: {error}"))?;
        fs::write(self.settings_path.as_ref(), settings).map_err(file_error)?;
        self.load_state()
    }

    pub fn reset_defaults(&self) -> PersonaResult<PersonaPromptStateDto> {
        let _guard = self.lock()?;
        fs::write(self.profile_root.join("manifest.json"), DEFAULT_MANIFEST).map_err(file_error)?;
        fs::write(self.profile_root.join("soul.md"), DEFAULT_SOUL).map_err(file_error)?;
        fs::write(self.profile_root.join("dolsoe.md"), DEFAULT_DOLSOE).map_err(file_error)?;
        self.load_state()
    }

    pub fn compiled(&self) -> PersonaResult<CompiledPersonaPrompt> {
        let state = self.state()?;
        Ok(CompiledPersonaPrompt {
            persona_id: state.id,
            revision: state.revision,
            content: state.compiled_prompt,
        })
    }

    fn load_state(&self) -> PersonaResult<PersonaPromptStateDto> {
        let manifest = self.read_manifest()?;
        let settings = read_json::<PersonaSettings>(&self.settings_path, "페르소나 설정")?;
        let documents = manifest
            .files
            .iter()
            .map(|file| {
                let content = read_document(&self.profile_root.join(&file.path))?;
                Ok(PersonaDocumentDraft {
                    path: file.path.clone(),
                    content,
                })
            })
            .collect::<PersonaResult<Vec<_>>>()?;
        state_from_draft(
            &self.profile_root,
            manifest,
            PersonaPromptDraft {
                enabled: settings.enabled,
                documents,
            },
        )
    }

    fn read_manifest(&self) -> PersonaResult<PersonaManifest> {
        let manifest =
            read_json::<PersonaManifest>(&self.profile_root.join("manifest.json"), "manifest")?;
        validate_manifest(&manifest)?;
        Ok(manifest)
    }

    fn lock(&self) -> PersonaResult<MutexGuard<'_, ()>> {
        self.gate
            .lock()
            .map_err(|_| "페르소나 저장소 잠금이 손상되었습니다.".into())
    }
}

fn state_from_draft(
    profile_root: &Path,
    manifest: PersonaManifest,
    draft: PersonaPromptDraft,
) -> PersonaResult<PersonaPromptStateDto> {
    validate_draft(&manifest, &draft)?;
    let mut documents = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let draft_document = draft
            .documents
            .iter()
            .find(|document| document.path == file.path)
            .ok_or_else(|| format!("필수 프롬프트 파일이 없습니다: {}", file.path))?;
        documents.push(PersonaDocumentDto {
            path: file.path.clone(),
            label: file.label.clone(),
            description: file.description.clone(),
            character_count: draft_document.content.chars().count(),
            content: draft_document.content.clone(),
        });
    }
    let compiled_prompt = compile_prompt(&manifest, &documents, draft.enabled)?;
    let revision = prompt_revision(&manifest, &documents, draft.enabled);
    Ok(PersonaPromptStateDto {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        enabled: draft.enabled,
        revision,
        character_count: compiled_prompt.chars().count(),
        estimated_tokens: estimate_tokens(&compiled_prompt),
        compiled_prompt,
        directory_path: profile_root.to_string_lossy().into_owned(),
        documents,
    })
}

fn compile_prompt(
    manifest: &PersonaManifest,
    documents: &[PersonaDocumentDto],
    enabled: bool,
) -> PersonaResult<String> {
    if !enabled {
        return Ok(String::new());
    }
    let mut compiled = format!(
        "# {} system prompt\n\n\
         아래 문서는 시스템 지침입니다. 문서가 충돌하면 Soul 지침을 페르소나 표현보다 우선합니다.",
        manifest.name
    );
    for document in documents {
        compiled.push_str(&format!(
            "\n\n## {} (`{}`)\n\n{}",
            document.label,
            document.path,
            document.content.trim()
        ));
    }
    if compiled.len() > MAX_COMPILED_BYTES {
        return Err(format!(
            "컴파일된 시스템 프롬프트는 {}KB를 넘을 수 없습니다.",
            MAX_COMPILED_BYTES / 1024
        ));
    }
    Ok(compiled)
}

fn validate_manifest(manifest: &PersonaManifest) -> PersonaResult<()> {
    if manifest.id.trim().is_empty() || manifest.name.trim().is_empty() {
        return Err("manifest의 id와 name은 비어 있을 수 없습니다.".into());
    }
    if manifest.files.is_empty() || manifest.files.len() > MAX_DOCUMENTS {
        return Err(format!(
            "manifest에는 1개 이상 {}개 이하의 문서가 필요합니다.",
            MAX_DOCUMENTS
        ));
    }
    let mut paths = HashSet::new();
    for file in &manifest.files {
        let path = Path::new(&file.path);
        let valid_name = path.components().count() == 1
            && matches!(path.components().next(), Some(Component::Normal(_)))
            && path.extension().and_then(|value| value.to_str()) == Some("md");
        if !valid_name || !paths.insert(file.path.clone()) {
            return Err(format!(
                "허용되지 않는 프롬프트 파일 경로입니다: {}",
                file.path
            ));
        }
        if file.label.trim().is_empty() {
            return Err(format!("프롬프트 문서 라벨이 비어 있습니다: {}", file.path));
        }
    }
    Ok(())
}

fn validate_draft(manifest: &PersonaManifest, draft: &PersonaPromptDraft) -> PersonaResult<()> {
    if draft.documents.len() != manifest.files.len() {
        return Err("manifest와 프롬프트 문서 수가 일치하지 않습니다.".into());
    }
    let expected = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<HashSet<_>>();
    let actual = draft
        .documents
        .iter()
        .map(|document| document.path.as_str())
        .collect::<HashSet<_>>();
    if expected != actual || actual.len() != draft.documents.len() {
        return Err("manifest에 정의되지 않은 프롬프트 문서가 포함되어 있습니다.".into());
    }
    for document in &draft.documents {
        if document.content.trim().is_empty() {
            return Err(format!(
                "프롬프트 문서는 비어 있을 수 없습니다: {}",
                document.path
            ));
        }
        if document.content.len() > MAX_DOCUMENT_BYTES {
            return Err(format!(
                "{} 파일은 {}KB를 넘을 수 없습니다.",
                document.path,
                MAX_DOCUMENT_BYTES / 1024
            ));
        }
    }
    Ok(())
}

fn prompt_revision(
    manifest: &PersonaManifest,
    documents: &[PersonaDocumentDto],
    enabled: bool,
) -> String {
    let mut digest = Sha256::new();
    digest.update(manifest.id.as_bytes());
    digest.update(manifest.version.to_le_bytes());
    digest.update([u8::from(enabled)]);
    for document in documents {
        digest.update(document.path.as_bytes());
        digest.update([0]);
        digest.update(document.content.as_bytes());
        digest.update([0xff]);
    }
    format!("{:x}", digest.finalize())
}

fn estimate_tokens(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.len().div_ceil(4)
    }
}

fn seed_if_missing(path: &Path, content: &str) -> PersonaResult<()> {
    if !path.exists() {
        fs::write(path, content.as_bytes()).map_err(file_error)?;
    }
    Ok(())
}

fn read_document(path: &Path) -> PersonaResult<String> {
    let bytes = fs::read(path).map_err(file_error)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(format!(
            "{} 파일은 {}KB를 넘을 수 없습니다.",
            path.display(),
            MAX_DOCUMENT_BYTES / 1024
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| format!("프롬프트 파일은 UTF-8이어야 합니다: {}", path.display()))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> PersonaResult<T> {
    let bytes = fs::read(path).map_err(file_error)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("{label} 파일을 읽지 못했습니다: {error}"))
}

fn file_error(error: std::io::Error) -> String {
    format!("페르소나 파일 작업에 실패했습니다: {error}")
}

#[cfg(test)]
mod tests {
    use super::{
        PersonaDocumentDraft, PersonaPromptDraft, PersonaPromptStore, DEFAULT_DOLSOE, DEFAULT_SOUL,
    };

    #[test]
    fn bootstraps_and_compiles_documents_in_manifest_order() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonaPromptStore::bootstrap(directory.path()).unwrap();
        let state = store.state().unwrap();

        assert!(state.enabled);
        assert!(
            state.compiled_prompt.find("## Soul").unwrap()
                < state.compiled_prompt.find("## 돌쇠").unwrap()
        );
        assert!(state.compiled_prompt.contains(DEFAULT_SOUL.trim()));
        assert!(state.compiled_prompt.contains(DEFAULT_DOLSOE.trim()));
        assert_eq!(state.revision.len(), 64);
    }

    #[test]
    fn previews_without_saving_and_persists_explicit_save() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonaPromptStore::bootstrap(directory.path()).unwrap();
        let draft = PersonaPromptDraft {
            enabled: true,
            documents: vec![
                PersonaDocumentDraft {
                    path: "soul.md".into(),
                    content: "soul preview".into(),
                },
                PersonaDocumentDraft {
                    path: "dolsoe.md".into(),
                    content: "persona preview".into(),
                },
            ],
        };

        assert!(store
            .preview(draft.clone())
            .unwrap()
            .compiled_prompt
            .contains("soul preview"));
        assert!(!store
            .state()
            .unwrap()
            .compiled_prompt
            .contains("soul preview"));
        store.save(draft).unwrap();
        assert!(store
            .state()
            .unwrap()
            .compiled_prompt
            .contains("soul preview"));
    }

    #[test]
    fn disabled_persona_compiles_to_an_empty_system_prompt() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonaPromptStore::bootstrap(directory.path()).unwrap();
        let state = store.state().unwrap();
        let preview = store
            .preview(PersonaPromptDraft {
                enabled: false,
                documents: state
                    .documents
                    .into_iter()
                    .map(|document| PersonaDocumentDraft {
                        path: document.path,
                        content: document.content,
                    })
                    .collect(),
            })
            .unwrap();

        assert!(preview.compiled_prompt.is_empty());
        assert_eq!(preview.estimated_tokens, 0);
    }
}
