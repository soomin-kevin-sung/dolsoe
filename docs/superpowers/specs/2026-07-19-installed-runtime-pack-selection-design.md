# 설치된 런타임 팩 선택 설계

## 목적

Windows 11 x64 데스크톱 앱이 앱 로컬 데이터 디렉터리에 이미 설치된 CPU, CUDA, Vulkan 런타임 팩을 탐지하고 검증한 뒤, 사용자가 설정 패널에서 선택하여 현재 GGUF 모델을 선택한 팩으로 다시 로드할 수 있게 한다.

이번 마일스톤은 설치된 팩의 사용 경로만 완성한다. GitHub Releases 다운로드, 매니페스트 서명 검증, 업데이트, 롤백, 오프라인 ZIP 가져오기는 후속 배포 마일스톤으로 분리한다.

## 범위

### 포함

- `app_local_data_dir/runtime-packs/<pack-id>` 아래의 설치된 팩 탐지
- CPU, CUDA, Vulkan 팩별 파일 구조와 C ABI 검증
- 런타임 버전, llama.cpp 커밋, 지원 백엔드, 장치 정보 조회
- 설치된 팩 선택과 선택값의 WebView 로컬 저장
- 생성 중 요청 취소, 모델 언로드, 선택 팩 전환, 기존 모델 재로드
- 손상되거나 사라진 저장 팩에 대한 CPU fallback과 오류 표시
- 기존 Claude 설정 패널의 런타임 세그먼트 연결

### 제외

- 네트워크 다운로드와 설치 진행률
- 릴리스 및 자산 선택
- 공개키 서명과 SHA-256 매니페스트 검증
- 업데이트, 롤백, staging 활성화
- macOS Metal 팩
- 여러 팩의 동시 로드

## 런타임 팩 규칙

팩 디렉터리 이름이 팩 ID다. 팩 ID는 기존 ASCII 검증 규칙을 그대로 사용한다. Windows 팩은 최소한 다음 파일을 포함해야 한다.

```text
runtime-packs/<pack-id>/
  local_llm_runtime.dll
  llama.dll
  ggml.dll
  ggml-base.dll
  ggml-cpu.dll
```

CUDA 팩은 `ggml-cuda.dll`과 해당 종속 DLL을 추가로 포함하고, Vulkan 팩은 `ggml-vulkan.dll`을 추가로 포함한다. 탐지는 디렉터리 이름이나 파일명만 믿지 않고 `local_llm_runtime.dll`을 로드하여 ABI와 capability를 조회한다.

하나의 팩이 여러 backend capability를 보고하면 각 backend 후보로 표시할 수 있지만, MVP 빌드 팩은 CPU, CUDA, Vulkan 중 하나의 주 backend를 갖는 것을 기본으로 한다. 앱은 capability와 실제 장치 목록이 모두 존재하는 backend만 선택 가능 상태로 노출한다.

## 백엔드 모델

프론트엔드와 Tauri 명령은 다음의 정규화된 값을 사용한다.

```text
cpu | cuda | vulkan
```

각 설치 팩은 다음 정보를 제공한다.

- 팩 ID
- backend
- 상태: `ready | invalid`
- runtime 버전
- llama.cpp 커밋
- ABI major/minor
- 장치 이름 목록
- 선택 여부
- 오류 메시지(검증 실패 시)

동일 backend의 팩이 여러 개이면 저장된 팩을 우선하고, 없으면 팩 ID 오름차순의 첫 번째 `ready` 팩을 후보로 사용한다. UI 세그먼트는 backend를 선택하고 실제 팩 ID는 이 규칙으로 결정한다. 버전 직접 선택과 롤백은 후속 범위다.

## Tauri 경계

새 팩 서비스는 파일 탐지와 동적 라이브러리 검증을 담당하며 LLM worker의 모델 상태를 직접 소유하지 않는다.

```text
list_runtime_packs() -> RuntimePackInventory
```

`list_runtime_packs`는 blocking 작업으로 실행하고, 개별 팩 오류 때문에 전체 목록을 실패시키지 않는다. 신뢰 루트를 벗어나는 junction, 잘못된 팩 ID, DLL 로드 실패는 해당 팩의 `invalid` 상태로 반환한다.

선택값은 프론트엔드의 WebView `localStorage`에 팩 ID와 backend만 저장한다. 시작 시 inventory에 존재하는 `ready` 팩인지 다시 검증하며, 저장값을 신뢰하여 임의 경로를 열지 않는다. 이 단계에서 단일 선호값을 위해 SQLite migration이나 별도 JSON 저장소를 추가하지 않는다.

`LoadModelRequest`에는 기존 `runtimePackId`와 함께 정규화된 `backend`, `deviceIndex`를 추가한다. worker는 문자열 backend를 Rust `Backend` enum으로 검증하여 변환한다. CPU는 `n_gpu_layers=0`, CUDA/Vulkan은 우선 전체 offload를 뜻하는 `n_gpu_layers=-1`을 사용한다. GPU layer 수 직접 설정은 후속 고급 옵션 범위다. 장치 선택 UI가 없는 이번 단계에서는 inventory가 반환한 해당 backend의 첫 번째 장치 index를 사용한다.

## 전환 흐름

설정 패널은 현재 적용된 backend와 pending backend를 분리한다.

1. 사용자가 설치된 backend 세그먼트를 선택한다.
2. 선택값이 현재 값과 다르면 `재로드` 배지와 적용 버튼을 표시한다.
3. 적용 시 활성 생성 요청이 있으면 취소하고 terminal 메시지 저장까지 기다린다.
4. 현재 모델 경로와 로드 옵션을 보존한다.
5. 모델을 언로드한다.
6. 선택한 팩 ID와 backend를 WebView 설정에 저장한다.
7. 보존한 모델 경로가 있으면 선택 팩, backend, 첫 장치 index로 다시 로드한다.
8. 성공하면 적용 backend를 갱신한다.

재로드 실패 시 선택값과 오류를 유지하여 사용자가 다른 backend를 선택할 수 있게 한다. 이전 DLL을 프로세스 안에서 강제로 교체하지 않는다. 현재 worker가 런타임과 모델을 완전히 해제한 뒤 새 팩을 로드하는 기존 명령 직렬화 경계를 사용한다.

모델이 선택되지 않은 상태에서는 팩 선택값만 저장하고 재로드 없이 완료한다.

## 시작 시 fallback

앱 시작 시 저장된 팩이 `ready`면 이를 적용값으로 사용한다. 저장 팩이 없거나 손상됐으면 다음 순서로 fallback한다.

1. `cpu-dev`가 `ready`면 선택
2. 다른 `ready` CPU 팩 중 팩 ID 오름차순의 첫 항목 선택
3. CPU가 없으면 전체 `ready` 팩 중 첫 항목 선택
4. 사용 가능한 팩이 없으면 기존 `no-model` 상태와 설치 안내 오류 표시

fallback 결과는 자동 저장하지 않는다. 사용자가 적용을 확정할 때만 WebView 선호값을 변경하여 손상된 설정의 진단 가능성을 보존한다.

## UI

기존 `NativeSettingsPanel`의 런타임 섹션을 사용한다.

- CPU/CUDA/Vulkan 세그먼트는 inventory 기반으로 활성화한다.
- 미설치 backend는 disabled 상태와 `설치되지 않음` 설명을 표시한다.
- invalid 팩만 있는 backend는 disabled 상태와 첫 검증 오류를 표시한다.
- 선택 backend 아래에 팩 ID와 첫 장치 이름을 표시한다.
- pending 선택이 적용값과 다르면 기존 `재로드` 배지와 푸터를 표시한다.
- 다운로드 팩 행과 설치 버튼은 표시하지 않는다.

진단 화면에는 선택된 팩 ID, runtime 버전, llama.cpp 커밋, ABI를 표시할 수 있도록 inventory 데이터를 전달한다. 새로운 화면이나 마법사는 만들지 않는다.

## 오류 처리

- 팩 디렉터리 읽기 실패: inventory 명령 오류
- 개별 팩 DLL/ABI/capability 오류: 해당 팩 `invalid`
- 선택할 수 없는 팩 요청: Tauri 명령에서 거부
- WebView 선호값 저장 실패: 적용 중단, 현재 팩 유지
- 취소 또는 언로드 실패: 새 팩 로드 금지, 오류 표시
- 새 팩 모델 재로드 실패: 모델 없음/오류 상태로 전환, 저장된 선택값은 유지

오류는 원인과 팩 ID를 포함하고 DLL 절대 경로는 진단 정보에서만 노출한다.

## 테스트

### Rust

- 신뢰 루트의 유효 팩 탐지
- invalid DLL이 전체 inventory를 실패시키지 않음
- junction 경로 탈출 거부
- capability와 장치가 없는 backend 비활성화
- 저장 팩 누락 시 fallback 순서
- load 요청의 backend 문자열과 device index 검증
- CPU와 GPU backend별 `n_gpu_layers` 기본값

### TypeScript

- inventory 명령 DTO와 camelCase 계약
- WebView preference 읽기·쓰기와 inventory 재검증
- applied/pending backend 상태 전이
- 미설치 backend 선택 거부
- 적용 시 취소 -> 언로드 -> preference 저장 -> 모델 재로드 순서
- 모델 미선택 시 preference만 저장

### 통합

- 관리된 `cpu-dev` 팩 탐지와 ABI probe
- 실제 GGUF를 CPU 팩으로 로드
- 설정에서 CPU 선택 상태와 팩/장치 표시
- 앱 재시작 후 WebView preference 복원
- CUDA/Vulkan은 해당 팩과 하드웨어가 준비된 환경에서만 실행

## 완료 기준

- 앱이 하드코딩된 `cpu-dev` 대신 inventory와 preference에서 런타임 팩 ID를 결정한다.
- 설치된 backend만 설정 패널에서 선택할 수 있다.
- 모델이 로드된 상태에서 적용하면 안전한 취소·언로드·재로드 순서를 거친다.
- 선택값이 재시작 후 복원된다.
- 설치/배포 기능을 구현하지 않고도 수동 또는 개발 스크립트로 배치된 CPU/CUDA/Vulkan 팩을 사용할 수 있다.
