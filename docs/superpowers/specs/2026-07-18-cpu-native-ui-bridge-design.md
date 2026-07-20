# CPU 네이티브 UI 브리지 설계

## 1. 목표

Windows 11 x64 Tauri 앱에서 사용자가 로컬 GGUF 파일을 선택하고, 관리형 CPU 런타임 팩으로 모델을 로드한 뒤 실제 토큰 스트리밍 대화와 생성 중지를 수행한다. 기존 14개 mock URL 상태와 Playwright 검증은 유지한다.

이 단계는 첫 번째 실제 사용 경로만 만든다. 대화 SQLite 저장, 런타임 팩 다운로드·업데이트·롤백, CUDA/Vulkan 실행, 동적 option schema UI는 후속 단계다.

## 2. 사용자 흐름

1. 앱을 일반 URL로 실행하면 실제 네이티브 서비스 모드가 열린다.
2. 모델이 없으면 중앙의 `GGUF 모델 선택…` 버튼이나 헤더 모델 칩으로 파일 선택기를 연다.
3. `.gguf` 파일을 선택하면 관리형 `cpu-dev` 팩을 확인하고 CPU 모델 로드를 시작한다.
4. 로딩 진행 이벤트를 UI에 표시하고 성공하면 입력창을 활성화한다.
5. 사용자가 메시지를 전송하면 사용자 메시지와 빈 어시스턴트 메시지를 추가하고 토큰 바이트 이벤트를 점진적으로 디코딩해 표시한다.
6. 완료·취소·오류 terminal 이벤트에서 메시지 상태와 텔레메트리를 확정한다.
7. 생성 중지 버튼이나 `Esc`는 현재 요청을 한 번만 취소한다.
8. 모델 변경은 활성 요청을 취소하고 기존 모델을 언로드한 뒤 새 모델을 로드한다.

`?state=<mock-state>`가 있는 URL은 기존 mock 서비스와 고정 fixture를 사용한다. Playwright는 네이티브 DLL 없이 이 모드를 계속 검증한다.

## 3. 프로세스와 소유권 구조

`llm-runtime`의 `InferenceRuntime`, `Model`, `RequestStream`은 Tauri managed state에 직접 저장하지 않는다. 이 객체들은 앱 시작 시 생성되는 전용 `llm-worker` 스레드 하나가 모두 소유한다.

```text
React NativeRuntimeService
  | invoke(command)
  | listen("llm://event")
  v
Tauri commands
  | bounded worker command channel
  v
llm-worker thread
  - Option<InferenceRuntime>
  - Option<Model>
  - Option<RequestStream>
  - optional request terminal receiver
  |
  +-- llm-event-relay thread
      - runtime regular event receiver
      - terminal forwarding control receiver
  |
  v
managed runtime DLL -> llama.cpp CPU backend
```

Tauri state에는 thread-safe command sender와 worker join handle만 둔다. DLL 호출, 모델 drop, 요청 drop은 worker 스레드에서만 일어난다. `InferenceRuntime::events()`로 복제한 channel receiver만 `llm-event-relay` 스레드로 이동한다. 이는 동기 `model_load`가 worker를 점유하는 동안에도 model-progress를 UI로 전달하기 위한 것으로, 네이티브 객체 소유권은 이동하지 않는다. 앱 종료 시 worker에 shutdown을 보내 활성 요청을 취소하고 relay, 요청, 모델, 런타임 순서로 정리한다.

명령 채널은 용량 32의 bounded channel로 고정한다. 각 명령은 결과를 돌려받을 일회성 응답 채널을 포함하며, Tauri command는 이 응답만 기다린다. worker는 명령 수신을 최대 5ms 기다린 뒤 terminal 이벤트와 relay 실패 신호를 drain한다. relay는 regular event receiver와 terminal 전달 control receiver를 blocking select한다. 이 방식으로 idle busy loop를 피하면서 토큰 전달 지연을 제한한다. 첫 구현에서 활성 요청은 최대 하나이므로 두 번째 submit은 큐에 넣지 않고 busy 오류를 반환한다.

## 4. 관리형 CPU 팩

Tauri 명령은 DLL 경로를 입력받지 않는다. `runtime_pack_id`만 받고 기존 `runtime_probe`와 동일한 검증을 거쳐 다음 trusted root 아래 DLL을 해석한다.

```text
<app-local-data>/runtime-packs/<runtime_pack_id>/local_llm_runtime.dll
```

개발과 수동 테스트를 위해 `scripts/prepare-dev-cpu-pack.ps1`을 추가한다. 기본 팩 ID는 `cpu-dev`다. 스크립트는 다음을 수행한다.

- 체크섬 고정 tiny GGUF acquisition은 기존 스크립트를 재사용한다.
- 고정 llama.cpp 커밋으로 CPU Debug 팩을 configure/build/install한다.
- 설치 결과의 필수 DLL 5개와 테스트 실행 파일을 확인한다.
- `%LOCALAPPDATA%/io.github.soomin-kevin-sung.local-llm-wiki/runtime-packs/cpu-dev`에 완전한 팩을 staging한 뒤 디렉터리 단위로 교체한다.

staging 디렉터리는 대상과 같은 부모 아래 `cpu-dev.staging-<pid>`로 만들고 검증이 끝난 뒤에만 활성화한다. 기존 팩은 `cpu-dev.backup-<pid>`로 이름을 바꾸고 staging을 `cpu-dev`로 바꾼 다음 백업을 삭제한다. 활성화가 실패하면 기존 팩 이름을 복원하고 staging을 남겨 진단할 수 있게 한다. `-DestinationRoot`를 선택 인자로 제공하되 기본값은 위 앱 식별자 경로다.

스크립트는 임의 DLL 한 개를 앱에 전달하지 않는다. 실제 배포용 다운로드·서명·롤백은 3단계 계획에서 이 개발 스크립트를 대체한다.

## 5. Tauri 명령 계약

명령 이름은 다음으로 고정한다.

```text
llm_get_status() -> LlmStatusDto
llm_load_model(request: LoadModelRequest) -> LlmStatusDto
llm_unload_model() -> LlmStatusDto
llm_submit(request: SubmitRequest) -> SubmitResponse
llm_cancel(request_handle: string) -> ()
llm_get_metrics() -> LlmMetricsDto
```

`LoadModelRequest`는 `runtime_pack_id`, 사용자 선택 `model_path`, CPU 모델 옵션을 포함한다. 첫 구현은 backend를 항상 CPU로 검증하며 GPU 값은 거부한다. 기본값은 슬롯 1, context 4096, logical batch 512, physical batch 128, CPU thread 수는 `available_parallelism`을 1~8로 제한한 값, GPU layer 0, mmap true다.

`SubmitRequest`는 prompt와 요청별 생성 옵션을 포함한다. 첫 UI는 max tokens, temperature, top-p, seed를 전달하고 나머지는 Rust 기본값을 사용한다. 프롬프트는 이번 단계에서 UTF-8 원문을 그대로 전달한다. chat template과 대화 이력 포맷은 option schema 단계에서 추가한다.

동시에 한 세션에서 요청 하나만 허용한다. 네이티브 스케줄러는 이후 여러 세션 연결을 위해 그대로 유지되지만 이 단계의 UI는 단일 활성 세션만 실제 요청에 연결한다.

## 6. 이벤트 계약

Tauri는 `llm://event` 하나를 emit한다.

```text
LlmEventDto {
  kind: "model-progress" | "queued" | "token" | "metrics" |
        "done" | "cancelled" | "error",
  request_handle: string | null,
  sequence_number: string,
  bytes: number[],
  error_code: number,
  metrics: LlmMetricsDto | null
}
```

64비트 handle과 sequence는 JavaScript 정밀도 손실을 막기 위해 문자열로 직렬화한다. 취소 명령도 같은 문자열 handle을 받고 Rust 경계에서 `u64`로 검증·변환한다. 토큰은 UTF-8 문자열이 아니라 바이트 배열로 보낸다. 프런트엔드는 요청별 `TextDecoder`를 `stream: true`로 유지하고 terminal에서 flush하여 분할된 멀티바이트 문자를 손상시키지 않는다.

event relay는 regular event receiver를 순서대로 emit한다. worker가 `RequestStream` terminal을 받으면 terminal을 직접 emit하지 않고 relay control channel로 보낸다. relay는 이미 큐에 들어온 regular 이벤트를 모두 drain한 뒤 terminal을 emit하여 마지막 token보다 terminal이 먼저 전달되지 않게 한다. terminal emit 이후 worker에 정리 완료를 알리고 worker가 해당 stream을 제거한다. 이벤트 emit 실패나 UI 구독 해제 시 relay가 worker에 실패 신호를 보내고 worker는 요청을 취소한 뒤 terminal 정리를 계속한다.

## 7. 프런트엔드 서비스와 상태

기존 `RuntimeService`는 mock 전용 snapshot API와 네이티브 command API를 혼합하지 않는다.

- `MockRuntimeService`: 기존 query fixture와 Playwright 전용
- `NativeRuntimeService`: `invoke`/`listen` 어댑터
- `useNativeRuntime`: 모델·메시지·현재 요청·텔레메트리 상태 reducer

`App`은 URL에 `state`가 있으면 `MockApp`, 없으면 `NativeApp`을 렌더한다. 공통 시각 컴포넌트는 그대로 재사용한다.

일반 URL을 Tauri가 아닌 브라우저에서 열어 `invoke` API를 찾을 수 없으면 화면을 비우지 않고 "데스크톱 앱에서 실행해야 합니다" 오류 상태와 `npm --prefix apps/desktop run tauri -- dev` 실행 방법을 표시한다.

네이티브 reducer 상태는 다음 전이만 가진다.

```text
no-model -> loading -> ready -> streaming -> done
                           \-> cancelled
                           \-> error
ready -> loading (model replacement)
any -> no-model (unload)
```

메시지와 대화는 메모리에만 저장한다. 새 대화와 초기화는 현재 메모리 메시지를 비운다. 앱 재시작 시 복구하지 않는다.

## 8. 파일 선택과 권한

`tauri-plugin-dialog`를 Rust와 TypeScript에 추가한다. 프런트엔드는 확장자 `gguf` 필터로 단일 파일을 선택하고 반환된 경로만 `llm_load_model`에 전달한다. 모델 경로는 worker 진입 전에 canonicalize하고 파일·확장자를 검증한다.

런타임 DLL은 사용자 선택 대상이 아니다. arbitrary DLL path를 받지 않는 기존 보안 회귀 테스트를 유지한다.

## 9. 오류와 취소

- 팩 없음: trusted runtime root와 `prepare-dev-cpu-pack.ps1` 실행 방법을 오류에 포함한다.
- 잘못된 GGUF: 현재 메시지를 유지하고 모델 상태만 오류로 표시한다.
- 중복 load/submit: 명시적 busy 오류를 반환한다.
- 취소: 같은 handle의 반복 취소는 성공으로 취급한다.
- UI 이벤트 수신 실패: worker가 요청을 취소하고 terminal 정리를 계속한다.
- 앱 종료: worker shutdown 완료를 제한 시간 없이 기다리되 native unload의 기존 quiescent 보장을 사용한다.

Rust panic과 C ABI 오류는 문자열 DTO로 변환하고 Tauri command 경계를 넘어 unwind하지 않는다.

## 10. 테스트 전략

### Rust

- trusted pack ID만 DLL로 해석되는 기존 회귀 테스트 유지
- worker 상태 전이: load 전 submit 거부, 중복 submit 거부, cancel 멱등, terminal 정리
- event DTO: u64 문자열 직렬화와 token bytes 보존
- 실제 tiny GGUF + CPU 팩이 주어진 명시적 환경에서 load, token, cancel, metrics smoke test

### 프런트엔드

- 기존 34개 mock Playwright 테스트 유지
- invoke/listen 어댑터를 주입한 native service contract 테스트
- token byte chunk가 한글 UTF-8 경계를 나눠도 올바르게 조합되는 테스트
- 일반 URL에서 no-model, 파일 선택 취소, load 오류, streaming, cancel reducer 상태 테스트

### 수동 Windows 검증

```powershell
& scripts/prepare-dev-cpu-pack.ps1
npm --prefix apps/desktop run tauri -- dev
```

앱에서 checksum-pinned tiny GGUF 또는 사용자 GGUF를 선택하고 모델 로드, 생성, 중지, 재로드를 확인한다.

## 11. 완료 기준

- 일반 Tauri 앱에서 로컬 GGUF 선택과 CPU 모델 로드가 성공한다.
- 실제 token bytes가 UI에 손상 없이 스트리밍된다.
- 버튼과 `Esc`로 생성 중지가 terminal까지 완료된다.
- 실제 metrics가 상태바에 표시된다.
- 모델 변경과 앱 종료 시 요청·모델·런타임이 순서대로 정리된다.
- `?state=` mock 모드와 Playwright 34개가 그대로 통과한다.
- Tauri command가 arbitrary runtime DLL 경로를 받지 않는다.
- 네트워크 없이 CPU 핵심 흐름이 동작한다.
