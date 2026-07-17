# Local LLM Wiki 데스크톱 MVP 설계

## 1. 목적

Windows 11 x64에서 로컬 GGUF 모델을 불러와 CPU, CUDA, Vulkan 장치로 대화할 수 있는 Tauri 데스크톱 앱을 만든다. 앱은 대화와 설정을 로컬에 저장하고, 추론 성능과 런타임 상태를 외부 전송 없이 표시한다.

이번 설계는 사용자와 LLM 사이의 첫 번째 창구와 네이티브 추론 기반을 다룬다. 문서 수집, 검색, 임베딩, RAG, 위키 편집은 후속 설계로 분리한다.

## 2. 제품 원칙

- YAGNI를 적용해 현재 필요한 기능만 구현한다.
- 앱은 인터넷 연결 없이 핵심 기능을 사용할 수 있어야 한다.
- 모델, 프롬프트, 대화, 텔레메트리를 외부로 전송하지 않는다.
- llama.cpp 변경은 안정적인 자체 C ABI 뒤에 격리한다.
- 한 번에 모델 하나만 메모리에 올리되, 같은 모델에 대한 요청은 병렬 처리할 수 있다.
- Windows 11 x64를 먼저 지원하고 플랫폼 경계는 향후 macOS Apple Silicon과 Metal을 수용할 수 있게 둔다.

## 3. MVP 범위

### 포함

- `create-tauri-app`의 React + TypeScript 템플릿으로 생성한 Tauri 2 앱
- 사용자가 선택한 로컬 `.gguf` 모델 로드와 언로드
- CPU, CUDA, Vulkan 백엔드 탐지와 선택
- 토큰 스트리밍, 생성 중지, 오류 표시
- 대화 생성, 이름 변경, 삭제, 메시지 초기화
- SQLite 기반 대화, 설정, 추론 기록 저장
- llama.cpp의 모델 로딩, 컨텍스트, 성능, 샘플링 옵션 중 C API에 대응되는 설정
- 기본 설정과 검색 가능한 고급 설정
- 로컬 텔레메트리와 런타임 진단 정보
- 기본 1개, 설정 가능 범위 1~4개의 병렬 요청 슬롯
- GitHub Releases 기반 런타임 팩 설치, 검증, 업데이트, 롤백
- 오프라인 ZIP 런타임 팩 가져오기

### 제외

- 모델 다운로드와 기본 모델 번들
- 여러 모델 동시 로드
- 문서 수집, 임베딩, 검색, RAG
- 외부 텔레메트리와 자동 오류 보고
- 진단 ZIP 내보내기
- 계정, 동기화, 클라우드 기능
- Windows 이외 플랫폼의 실제 빌드와 배포

## 4. 시스템 아키텍처

```text
React + TypeScript UI
        |
        | Tauri commands / events
        v
Tauri Rust app core
  - conversation service
  - SQLite repository
  - settings service
  - runtime pack manager
  - safe LLM wrapper
        |
        | stable C ABI
        v
local_llm_runtime.dll
  - ABI facade
  - model runtime
  - request scheduler
  - shared batch decoder
  - event dispatcher
  - metrics collector
        |
        v
llama.dll + ggml core + CPU/CUDA/Vulkan backend modules
```

Tauri 프로세스가 런타임 DLL을 직접 로드한다. 별도 사이드카와 로컬 서버는 사용하지 않는다. 네이티브 충돌 시 앱 전체가 종료될 수 있다는 제한을 받아들이고, 다음 실행에서 미완료 메시지를 `중단됨`으로 복구한다.

## 5. 저장소와 빌드 경계

예상 최상위 구조는 다음과 같다.

```text
apps/desktop/                 Tauri React 앱
crates/llm-runtime-sys/       raw FFI 선언과 동적 심볼 로더
crates/llm-runtime/           안전한 Rust 래퍼
native/llm-runtime/           C++ 런타임 DLL과 C 공개 헤더
packages/runtime-manifests/   런타임 팩 매니페스트와 스키마
docs/                         설계와 운영 문서
```

`create-tauri-app`으로 먼저 앱을 생성한 뒤 필요한 워크스페이스 경계를 추가한다. 프런트엔드 구현은 Claude 디자인 산출물이 승인된 후 시작한다.

## 6. C ABI 계약

llama.cpp의 공개 함수를 Rust에 그대로 노출하지 않는다. 런타임 DLL 내부에서 고정된 llama.cpp 커밋의 함수만 사용하고 Rust에는 최소한의 `llw_` API를 제공한다.

### ABI 원칙

- ABI는 major/minor 버전을 제공한다.
- 공개 구조체는 고정 폭 정수, `struct_size`, `flags`, `reserved` 필드를 사용한다.
- C++ 객체는 opaque pointer 또는 64비트 handle로만 노출한다.
- C++ 예외와 Rust panic은 ABI 경계를 넘지 않는다.
- 문자열과 바이트의 소유권 및 유효 기간을 함수별로 명시한다.
- 오류는 정수 코드와 호출자가 제공한 오류 버퍼로 반환한다.
- major 불일치는 거부하고 minor 차이는 `struct_size`와 capability로 협상한다.

### 최소 함수 집합

```text
llw_get_abi_info
llw_runtime_create
llw_runtime_destroy
llw_runtime_get_capabilities
llw_runtime_list_devices
llw_runtime_get_option_schema
llw_model_load
llw_model_unload
llw_request_submit
llw_request_cancel
llw_get_scheduler_snapshot
llw_get_metrics
```

### 이벤트 콜백

런타임은 임의 훅 여러 개 대신 범용 이벤트 콜백 하나를 제공한다.

```text
MODEL_PROGRESS
QUEUED
TOKEN
METRICS
DONE
CANCELLED
ERROR
LOG
```

이벤트에는 runtime, model, request handle, slot ID, 요청별 sequence number, 오류 코드, payload가 포함된다. payload는 콜백 호출 중에만 유효하며 Rust가 즉시 복사한다. 같은 요청의 이벤트는 순서대로 전달하고 `DONE`, `CANCELLED`, `ERROR` 중 하나를 정확히 한 번 보낸다.

스케줄러 스레드는 Rust 콜백을 직접 호출하지 않는다. bounded response queue에 이벤트를 넣고 전용 dispatcher thread가 콜백을 호출한다. 콜백은 블로킹 API를 재진입하지 않으며 Rust 래퍼는 `catch_unwind`로 panic 전파를 차단한다.

## 7. 병렬 스케줄러

런타임은 모델 하나와 공유 llama context를 소유한다. 각 활성 요청은 별도 slot과 sequence ID를 가진다.

- 기본 슬롯 수는 1이다.
- 고급 설정에서 1~4로 변경할 수 있다.
- MVP 합격 기준에는 동시 2요청이 포함된다.
- 요청 큐와 이벤트 큐는 크기가 제한된다.
- 활성 slot을 순환하며 각 요청의 다음 토큰을 공용 batch에 포함한다.
- 한 대화에서는 동시에 하나의 생성만 허용한다.
- 서로 다른 대화의 생성은 전역 슬롯 한도 안에서 병렬 처리한다.
- 사용자가 다른 대화로 이동해도 기존 생성은 계속되고 사이드바에 상태를 표시한다.

요청 상태는 `queued -> preprocessing -> running -> terminal` 순서로 진행한다. 취소는 비차단이며 여러 번 호출해도 같은 결과를 내는 멱등 연산이다. 대기 요청은 slot 할당 전에 제거하고, 실행 요청은 다음 batch 구성 전에 제외한 뒤 해당 sequence의 KV 상태를 정리한다. 진행 중인 단일 `llama_decode` 호출은 안전하게 강제 종료하지 않는다.

모델이나 백엔드를 변경할 때는 활성 요청을 모두 취소하고 종료 이벤트를 확인한 다음 모델을 언로드한다.

## 8. 런타임과 백엔드 팩

런타임 버전 디렉터리는 동일한 llama.cpp 커밋과 도구 체인으로 빌드한 파일만 포함한다.

```text
runtimes/<runtime-version>/
  runtime.json
  local_llm_runtime.dll
  llama.dll
  ggml-base.dll
  ggml.dll
  backends/
    ggml-cpu.dll
    ggml-cuda.dll
    ggml-vulkan.dll
```

CPU 기반 런타임은 앱 설치에 포함한다. CUDA와 Vulkan 모듈 및 필요한 종속 파일은 선택형 팩으로 제공한다. 설치된 모듈은 앱 시작 시 등록하며 CPU, CUDA, Vulkan 전환은 활성 모델을 언로드하고 선택 장치로 다시 로드한다. 일반 전환에는 앱 재시작이 필요하지 않다.

새 모듈 설치, 런타임 업데이트, 롤백은 다음 앱 시작부터 적용한다. 프로세스 안에서 서로 다른 llama.cpp 코어 버전을 교체하지 않는다.

### 배포와 검증

- 프로젝트 GitHub Releases에 앱 릴리스와 호환되는 런타임 팩을 게시한다.
- 앱 버전은 기본 런타임 릴리스, bridge ABI, llama.cpp 커밋 범위를 지정한다.
- 앱은 검증된 최신 호환 릴리스를 자동 선택한다.
- 고급 설정에서 현재 버전과 이전 호환 버전으로의 롤백을 제공한다.
- 매니페스트는 앱에 내장한 공개 키로 서명을 검증한다.
- 각 파일은 SHA-256, 크기, 안전한 상대 경로를 검증한다.
- 다운로드는 staging 디렉터리에 완료한 뒤 원자적으로 활성화한다.
- ZIP 경로 탈출과 DLL basename 충돌을 거부한다.
- 오프라인 가져오기도 동일한 서명과 체크섬 검증을 거친다.
- 릴리스에는 llama.cpp와 CUDA 등 재배포 대상의 라이선스 고지를 포함한다.

## 9. 모델과 설정

사용자는 로컬 `.gguf` 파일을 선택한다. 앱은 모델 메타데이터, 아키텍처, 양자화, 컨텍스트 한도와 예상 메모리를 표시한다. 4B를 기본 예상 크기로 삼되 더 큰 모델을 제한하지 않는다.

설정은 적용 시점에 따라 구분한다.

- 런타임 재시작 필요: 설치된 런타임 버전 변경
- 모델 재로드 필요: 백엔드, 장치, GPU layer, context, KV cache, mmap, batch 계열
- 다음 요청부터 적용: sampling, seed, 최대 생성 토큰, stop sequence

자주 쓰는 설정은 버전형 C 구조체로 전달한다. 런타임별 고급 설정은 capability와 option schema JSON으로 설명하고 앱과 DLL이 각각 검증한다. 지원하지 않는 옵션은 무시하지 않고 명시적 오류로 반환한다.

`llama-server`의 HTTP host, port, TLS, API key, HTTP thread, endpoint 설정은 제외한다. 모델 로딩, 장치, 컨텍스트, KV cache, batch, sampling, chat template, 성능 관련 C API 대응 옵션만 제공한다.

## 10. 대화와 데이터 저장

SQLite는 Rust 계층만 접근하며 migration으로 버전을 관리한다.

### 핵심 테이블

- `conversations`: 제목, 생성·수정 시각, 마지막 사용 모델 정보
- `messages`: 역할, 내용, 생성 상태, 오류, 생성·수정 시각
- `settings_profiles`: 모델별 로드·생성 설정과 스키마 버전
- `inference_runs`: 요청 상태, 토큰 수, 시간, 속도, 중지 사유
- `runtime_packs`: 설치 버전, backend, checksum, 검증·활성 상태

스트리밍 응답은 메모리에서 갱신하고 짧은 주기로 부분 저장한 뒤 terminal 이벤트에서 확정한다. 앱 시작 시 `streaming` 상태로 남은 메시지와 실행 기록은 `interrupted`로 변경한다.

새 대화는 빈 세션을 만들고, 대화 초기화는 세션을 유지한 채 메시지만 삭제한다. 대화 삭제는 확인 후 관련 메시지와 실행 기록을 트랜잭션으로 삭제한다. 첫 사용자 메시지를 기반으로 로컬에서 짧은 제목을 만들며 별도 LLM 호출은 하지 않는다.

## 11. 사용자 경험

첫 화면은 랜딩 페이지가 아니라 실제 채팅 작업 공간이다.

- 왼쪽: 새 대화, 검색 가능한 대화 목록, 생성 상태
- 중앙: 메시지 목록, 스트리밍 응답, 입력창, 중지 버튼
- 오른쪽 또는 오버레이: 모델, backend, 장치, 기본·고급 설정
- 하단 상태 영역: 모델 상태, backend, tokens/s, context 사용량
- 별도 진단 화면: 앱·ABI·llama.cpp·팩·드라이버 버전과 상세 메트릭

모델이 없으면 중앙 영역에서 로컬 GGUF 선택을 유도한다. GPU 팩이 없거나 장치가 호환되지 않으면 이유와 CPU fallback을 제시한다. 설정 변경으로 모델 재로드가 필요할 때는 예상 영향과 활성 요청 취소를 확인한다.

Claude가 시각 디자인과 아이콘을 담당한다. 기술 구조를 바꾸지 않고 이 설계의 상태와 흐름을 모두 포함하는 디자인 산출물을 먼저 만든다. 전달 지시서는 `docs/design/claude-design-brief.md`에 둔다.

## 12. 로컬 텔레메트리

채팅 화면에는 backend, 장치, 생성 속도, context 사용량만 간결하게 표시한다. 상세 화면에는 다음을 표시한다.

- 앱 버전, bridge ABI, llama.cpp 커밋과 빌드 번호
- 런타임 팩 버전, checksum 검증 상태, 업데이트·롤백 상태
- backend, 장치, 드라이버, CUDA/Vulkan 런타임 정보
- 모델 아키텍처, 양자화, 로딩 시간
- 입력·출력 토큰, prompt 처리 속도, 생성 속도
- 첫 토큰 시간, 전체 추론 시간, 종료 사유
- RAM·VRAM 사용량, context 사용량
- queue 대기 시간, 활성 slot, 취소 지연

데이터는 SQLite와 로컬 로그에만 저장한다. 외부 전송과 진단 ZIP 내보내기는 제공하지 않는다.

## 13. 오류와 복구

- 잘못되거나 지원하지 않는 GGUF는 로드 실패 원인을 표시하고 현재 대화를 유지한다.
- 메모리 부족은 필요한 설정 조정과 CPU 또는 낮은 GPU offload 선택을 안내한다.
- backend 초기화 실패 시 해당 backend를 비활성화하고 CPU fallback을 제안한다.
- 런타임 팩 검증 실패 시 활성화하지 않고 마지막 정상 버전을 유지한다.
- 이벤트 큐 포화는 해당 요청을 오류 또는 취소로 종료하고 scheduler를 막지 않는다.
- 콜백 수신자가 사라지면 요청을 취소한다.
- DLL 함수가 예상하지 못한 예외를 던지면 ABI에서 오류 코드로 변환한다.
- 네이티브 access violation과 GPU 드라이버 충돌은 in-process 구조상 앱을 종료할 수 있다. 다음 실행에서 미완료 상태를 복구하고 최근 로드 설정을 진단 화면에 표시한다.

## 14. 테스트 전략

### C/C++ 런타임

- ABI 구조체 크기, 정렬, 이전 minor 버전 호환성
- fake model 기반 queue, slot, callback, terminal 이벤트 테스트
- 동시 2요청 공정성 및 단일 terminal 이벤트 보장
- queued, preprocessing, running 단계별 취소
- cancel과 unload/destroy 경쟁 조건
- KV sequence 정리와 slot 재사용
- 느린 콜백과 이벤트 큐 포화

### Rust

- DLL 심볼과 ABI 협상 실패 처리
- 콜백 payload 즉시 복사와 panic 차단
- stream drop 시 취소
- SQLite migration과 crash recovery
- 런타임 매니페스트 서명, checksum, 경로 검증
- staging 설치, 원자적 활성화, 롤백

### 프런트엔드

- 대화 생성·이름 변경·초기화·삭제
- 모델 없음, 로딩, 준비, 생성, 취소, 오류 상태
- 설정 적용 시점과 재로드 확인
- 1~4 slot 설정과 여러 대화의 동시 생성 표시
- 로컬 텔레메트리 렌더링
- 긴 토큰, 긴 파일명, 작은 창에서 레이아웃 안정성

### 통합 검증

- 작은 테스트 GGUF를 사용하는 CPU 자동 smoke test
- Windows x64에서 모델 로드, 생성, 취소, 언로드 반복
- CUDA와 Vulkan은 해당 장치가 있는 검증 환경에서 backend별 smoke test
- 동일 빌드의 CPU/CUDA/Vulkan 모듈 경로와 커밋 일치 검사
- React UI는 mock Tauri API로 Playwright 검증하고 네이티브 흐름은 Rust/C++ harness로 분리 검증

## 15. 향후 macOS 경계

Rust 안전 래퍼와 C ABI는 플랫폼 중립형 이름과 고정 폭 타입을 사용한다. macOS 구현은 `.dylib`, Apple Silicon, Metal 장치 탐지를 추가하되 React UI, SQLite schema, 요청·이벤트 계약은 유지한다. Windows의 `LoadLibraryExW` 정책만 플랫폼 loader 모듈로 격리한다.

## 16. 완료 기준

- Windows 11 x64에서 CPU, CUDA, Vulkan 장치를 탐지하고 선택할 수 있다.
- 로컬 GGUF를 불러와 스트리밍 대화와 중지가 동작한다.
- 두 대화의 동시 요청이 shared scheduler에서 완료된다.
- backend 변경 시 모델만 안전하게 재로드된다.
- 앱 재시작 후 대화, 설정, 로컬 텔레메트리가 복구된다.
- 런타임 팩 설치와 rollback이 ABI, 서명, checksum 검증을 통과한다.
- 지원하지 않는 llama.cpp 옵션과 오류가 사용자에게 명시적으로 표시된다.
- 외부 네트워크 없이 핵심 기능과 CPU 추론을 사용할 수 있다.
