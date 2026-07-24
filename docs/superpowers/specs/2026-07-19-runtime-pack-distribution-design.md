# 런타임 팩 배포 및 앱 내 설치 설계

## 1. 목표

Windows 11 x64 앱이 프로젝트 GitHub Releases에서 현재 앱과 호환되는 CPU, CUDA, Vulkan 런타임 팩을 조회하고, 사용자가 설정 화면에서 선택한 팩을 다운로드·검증·설치할 수 있게 한다. 설치 실패나 앱 종료가 기존 정상 팩을 손상시키지 않아야 한다.

이번 마일스톤은 최신 호환 팩의 신규 설치까지 완성한다. 업데이트, 이전 버전 롤백, 오프라인 ZIP 가져오기는 동일한 매니페스트와 설치 엔진을 재사용하는 후속 마일스톤으로 남긴다.

## 2. 사용자 흐름

1. 앱은 설치된 팩 inventory를 표시한 뒤 고정된 HTTPS URL에서 릴리스 매니페스트와 분리 서명을 조회한다.
2. 앱은 운영체제 `windows`, 아키텍처 `x86_64`, bridge ABI major, 앱 버전과 호환되는 팩만 노출한다.
3. 설치되지 않은 backend에는 팩 버전과 다운로드 크기를 표시하고 `설치` 버튼을 제공한다.
4. 사용자가 설치를 누르면 다운로드·검증·설치 단계를 진행률과 함께 표시한다.
5. 성공한 팩은 앱 로컬 데이터의 `runtime-packs/<pack-id>`에 배치한다.
6. 새 DLL은 현재 프로세스에 주입하지 않는다. UI는 설치 완료와 재시작 필요 상태를 표시한다.
7. 앱 재시작 후 기존 inventory 경로가 ABI와 장치를 검사하고 backend 선택을 활성화한다.

네트워크 오류나 설치 실패 시 CPU와 기존 설치 팩은 계속 사용할 수 있다. 사용자가 설치를 명시적으로 시작하기 전에는 대용량 팩을 자동 다운로드하지 않는다.

## 3. 릴리스 계약

릴리스에는 다음 자산을 게시한다.

```text
runtime-manifest.json
runtime-manifest.sig
dolsoe-runtime-<version>-windows-x86_64-cpu.zip
dolsoe-runtime-<version>-windows-x86_64-cuda.zip
dolsoe-runtime-<version>-windows-x86_64-vulkan.zip
THIRD_PARTY_NOTICES.txt
```

매니페스트는 UTF-8 JSON 원문 바이트에 Ed25519 분리 서명을 적용한다. 앱에는 공개키만 내장하며 GitHub Actions는 저장소 secret의 PKCS#8 개인키로 서명한다. 서명 검증이 성공하기 전에는 매니페스트 필드를 신뢰하지 않는다.

```json
{
  "schemaVersion": 1,
  "releaseVersion": "2026.07.1",
  "minimumAppVersion": "0.1.0",
  "maximumAppVersion": "0.1.x",
  "abiMajor": 1,
  "abiMinor": 0,
  "llamaCppCommit": "6bdd77f13cf11b264b4231d320afc404f48d576e",
  "packs": [
    {
      "id": "cuda-2026.07.1",
      "backend": "cuda",
      "platform": "windows",
      "arch": "x86_64",
      "assetUrl": "https://github.com/.../cuda.zip",
      "size": 123,
      "sha256": "...",
      "files": [{ "path": "local_llm_runtime.dll", "size": 123, "sha256": "..." }]
    }
  ]
}
```

`assetUrl`은 HTTPS이고 프로젝트 GitHub Release 경로여야 한다. 팩 ID는 기존 `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` 규칙을 따른다. 동일 backend·platform·arch 조합은 한 릴리스에 하나만 허용한다.

## 4. 설치 엔진

Rust의 `runtime_installer` 모듈이 네트워크와 파일시스템을 소유한다.

- 매니페스트 원문과 서명을 내려받아 내장 공개키로 검증한다.
- 앱 버전과 ABI major가 맞지 않으면 전체 릴리스를 거부한다.
- 다운로드는 `<runtime-root>/.downloads/<pack-id>.zip.part`에 저장하며 기존 partial 길이부터 HTTP Range로 이어받는다.
- 수신 바이트가 선언 크기를 넘으면 즉시 실패한다.
- 완료 ZIP의 크기와 SHA-256을 검증한다.
- `<pack-id>.staging-<uuid>`에 압축을 해제한다.
- 절대 경로, `..`, symlink, 중복 경로, 선언되지 않은 파일을 거부한다.
- 각 파일의 크기와 SHA-256을 검증하고 필수 DLL 구성을 확인한다.
- 같은 pack ID가 이미 있으면 설치 완료로 처리하되, 내용이 다르면 충돌 오류를 반환한다. 이번 마일스톤은 설치된 팩을 덮어쓰지 않는다.
- staging 디렉터리를 최종 pack ID 디렉터리로 rename한다. 실패하면 staging만 정리하고 기존 팩은 유지한다.

설치 작업은 프로세스당 하나만 실행한다. 취소는 다운로드 중 cooperative cancellation이며 검증 또는 최종 rename 단계에 들어간 뒤에는 해당 짧은 단계를 끝낸다.

## 5. Tauri 및 프런트엔드 계약

Tauri 명령은 다음 세 개다.

- `list_available_runtime_packs`: 검증된 원격 팩 목록과 현재 설치 여부를 반환한다.
- `install_runtime_pack`: pack ID를 받아 백그라운드 설치를 시작한다.
- `cancel_runtime_pack_install`: 현재 다운로드 취소를 요청한다.

`runtime-pack-install-progress` 이벤트는 `packId`, `phase`, `downloadedBytes`, `totalBytes`, `error`를 전달한다. phase는 `downloading`, `verifying`, `installing`, `installed`, `cancelled`, `failed` 중 하나다.

프런트엔드는 기존 `RuntimePackService`를 확장하고 설치 상태를 `NativeApp`에서 소유한다. 설정 패널은 미설치 팩마다 크기와 설치 버튼을 표시하고, 실행 중에는 한 개의 진행률과 취소 버튼을 표시한다. 설치 완료 후에는 `앱을 재시작하면 사용할 수 있습니다`를 보여준다.

매니페스트를 조회하지 못해도 설정 패널과 채팅은 계속 동작한다. 원격 오류는 설치 영역에만 표시한다.

## 6. 빌드와 배포

별도 GitHub Actions workflow가 수동 실행 또는 `runtime-v*` 태그에서 동작한다.

- 같은 pinned llama.cpp 커밋과 MSVC 도구 체인으로 backend별 Release 팩을 빌드한다.
- CPU와 Vulkan은 GitHub-hosted Windows runner를 사용한다.
- CUDA는 CUDA Toolkit이 준비된 self-hosted Windows runner를 사용한다.
- CTest와 pack file 검증을 통과한 출력만 ZIP으로 만든다.
- manifest 생성기는 ZIP 및 내부 파일의 크기와 SHA-256을 기록한다.
- 서명 secret이 없으면 게시 단계는 명시적으로 실패한다.
- GitHub Release 자산은 같은 tag의 기존 자산을 덮어쓰지 않는다.

## 7. 보안 및 복구

- 네트워크 입력은 서명 검증 전 신뢰하지 않는다.
- 다운로드 URL, pack ID, archive path를 각각 검증한다.
- runtime root 밖의 삭제·이동을 금지한다.
- 설치 실패는 마지막 정상 팩과 사용자 선택을 변경하지 않는다.
- 로그와 UI 오류에는 URL을 표시할 수 있지만 서명이나 내부 시스템 경로 외의 민감정보는 기록하지 않는다.

## 8. 테스트

- Rust 단위 테스트: 서명, 앱/ABI 호환성, URL 제한, 크기·checksum, ZIP traversal·symlink·중복·미선언 파일, staging 활성화, 기존 팩 충돌, 취소.
- Rust 통합 테스트: 로컬 HTTP 서버의 Range 다운로드와 재시도 가능한 partial 파일.
- 프런트엔드 단위 테스트: 원격 목록 매핑, 설치 이벤트 상태 전이.
- Playwright: 미설치 팩, 설치 진행, 실패, 설치 완료·재시작 안내.
- workflow 정적 검증: CPU/CUDA/Vulkan 자산명과 manifest 생성 명령.
- 실기 검증: RTX 3070에서 CUDA 팩 설치, 앱 재시작, CUDA 선택, GGUF 생성.

## 9. 완료 기준

- 설치되지 않은 최신 호환 팩을 앱 설정에서 조회할 수 있다.
- 설치 버튼 한 번으로 다운로드·서명·checksum·archive 검증과 원자적 설치가 완료된다.
- 실패 또는 취소 후 기존 CPU 팩과 대화 기능이 정상이다.
- 앱 재시작 후 설치 팩이 기존 inventory 검증을 통과하고 backend 선택이 활성화된다.
- Release workflow가 동일 소스에서 CPU/CUDA/Vulkan 팩과 서명된 manifest를 생성할 수 있다.
