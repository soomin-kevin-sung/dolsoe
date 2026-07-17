# Native llama.cpp Scheduler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the ABI-only fake runtime with a Windows x64 native llama.cpp runtime that loads one GGUF model, schedules bounded concurrent generation across 1-4 sequence slots with shared batch decode, and streams generic callback events through the existing safe Rust boundary.

**Architecture:** Keep llama.cpp private behind the versioned `llw_` C ABI. A runtime owns one event dispatcher, one scheduler, and at most one model/context; the scheduler copies submitted inputs, assigns independent sequence IDs and sampler chains to active slots, combines work into one `llama_batch`, and guarantees one terminal event per accepted request. CPU, CUDA, and Vulkan are separate runtime-pack builds from the same pinned source commit; selecting another pack requires unloading/restarting the runtime, while future Metal can use reserved ABI fields without being implemented here.

**Tech Stack:** C++17, CMake 3.24 FetchContent, llama.cpp commit `6bdd77f13cf11b264b4231d320afc404f48d576e`, CTest, Rust 1.93, `libloading`, `crossbeam-channel`, Windows x64 CPU/CUDA/Vulkan

---

## Pinned llama.cpp Source

Pin: `6bdd77f13cf11b264b4231d320afc404f48d576e`

Resolved on 2026-07-18 with the official repository directly:

```powershell
git ls-remote https://github.com/ggml-org/llama.cpp.git refs/heads/master
```

Expected output:

```text
6bdd77f13cf11b264b4231d320afc404f48d576e refs/heads/master
```

The exact detached commit was inspected in the official repository: [`include/llama.h`](https://github.com/ggml-org/llama.cpp/blob/6bdd77f13cf11b264b4231d320afc404f48d576e/include/llama.h), [`ggml/include/ggml-backend.h`](https://github.com/ggml-org/llama.cpp/blob/6bdd77f13cf11b264b4231d320afc404f48d576e/ggml/include/ggml-backend.h), [`ggml/CMakeLists.txt`](https://github.com/ggml-org/llama.cpp/blob/6bdd77f13cf11b264b4231d320afc404f48d576e/ggml/CMakeLists.txt), and the [commit record](https://github.com/ggml-org/llama.cpp/commit/6bdd77f13cf11b264b4231d320afc404f48d576e). `include/llama.h` contains `llama_model_params.devices`, `llama_context_params.n_ctx`, `n_batch`, `n_ubatch`, `n_seq_max`, `n_threads`, and `n_threads_batch`; `llama_batch_init`, `llama_decode`, `llama_get_memory`, `llama_memory_seq_rm`, `llama_model_load_from_file`, `llama_init_from_model`, `llama_tokenize`, `llama_token_to_piece`, `llama_sampler_chain_init`, `llama_sampler_chain_add`, `llama_sampler_init_*`, `llama_sampler_sample`, `llama_sampler_accept`, and `llama_sampler_free` are present. `ggml/include/ggml-backend.h` contains `ggml_backend_load_all_from_path`, `ggml_backend_dev_count`, `ggml_backend_dev_get`, `ggml_backend_dev_name`, `ggml_backend_dev_description`, `ggml_backend_dev_memory`, and `ggml_backend_dev_type`. The pinned `ggml/CMakeLists.txt` defines `GGML_CPU`, `GGML_CUDA`, `GGML_VULKAN`, `GGML_BACKEND_DL`, and `GGML_NATIVE`; the root defines `BUILD_SHARED_LIBS`, `LLAMA_BUILD_TESTS`, `LLAMA_BUILD_EXAMPLES`, and `LLAMA_BUILD_SERVER`.

Do not update this pin while executing the plan. A pin update is a separate reviewed change that repeats header, build-option, license, CPU, CUDA, and Vulkan verification.

## Scope Boundaries

This plan implements only the native runtime/scheduler and the Rust wrapper needed to consume it. It does not implement product UI, SQLite, conversations, RAG, runtime downloading, installation/update/rollback, release packaging, or Claude design output.

The Tauri command continues to accept only a validated project-managed runtime pack ID and resolves `local_llm_runtime.dll` beneath the app-local `runtime-packs` root. Do not add a DLL path argument to `probe_runtime` or expose `RuntimeLibrary::load` directly to frontend input.

## File Responsibility Map

```text
native/llm-runtime/CMakeLists.txt                 Pin llama.cpp and select one CPU/CUDA/Vulkan pack
native/llm-runtime/include/llw_runtime.h          Append-only ABI 1.1 structs, constants, and exports
native/llm-runtime/src/event_dispatcher.h         Bounded event queue and callback-thread contract
native/llm-runtime/src/event_dispatcher.cpp       Payload-owning dispatcher implementation
native/llm-runtime/src/inference_engine.h         Scheduler-facing engine interface and batch result types
native/llm-runtime/src/llama_engine.h             llama.cpp model/context/sampler ownership
native/llm-runtime/src/llama_engine.cpp           Device selection, tokenization, shared decode, sampling
native/llm-runtime/src/scheduler.h                 Request state machine, handles, slots, snapshots
native/llm-runtime/src/scheduler.cpp               Bounded queue, cancellation, terminal-event uniqueness
native/llm-runtime/src/runtime.cpp                 C ABI validation, exception barrier, runtime/model lifecycle
native/llm-runtime/tests/abi_layout_test.cpp       C ABI type, size, offset, and legacy-prefix tests
native/llm-runtime/tests/fake_engine.h             Deterministic engine used only by scheduler tests
native/llm-runtime/tests/scheduler_test.cpp        Concurrency, queue-full, cancellation, terminal tests
native/llm-runtime/tests/runtime_backend_test.cpp Opt-in real GGUF backend end-to-end test
native/llm-runtime/tests/fixtures/model.json       Tiny GGUF URL, SHA-256, size, and provenance
scripts/acquire-test-model.ps1                     Explicit checksum-verified fixture acquisition
crates/llm-runtime-sys/src/lib.rs                  Exact repr(C) mirrors and fourteen dynamic exports
crates/llm-runtime/src/lib.rs                      Safe model/request API and callback payload copying
crates/llm-runtime/tests/native_runtime.rs         Rust-to-DLL load/generate/cancel integration tests
apps/desktop/src-tauri/src/runtime_probe.rs        Preserve managed-pack resolution; no inference UI
docs/native-runtime-validation.md                  CPU commands and hardware-gated CUDA/Vulkan commands
.github/workflows/ci.yml                           Required CPU job and GPU configuration smoke jobs
```

### Task 1: Pin llama.cpp And Define Backend Pack Builds

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Test: `native/llm-runtime/CMakeLists.txt`

- [ ] **Step 1: Prove the existing project has no llama.cpp target**

Run:

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu-plan -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu-plan --config Debug --target llama
```

Expected: configure either warns that `LLW_BACKEND_PACK` is unused or succeeds, then the build fails because target `llama` does not exist.

- [ ] **Step 2: Replace the CMake project with the pinned FetchContent build**

Replace `native/llm-runtime/CMakeLists.txt` with:

```cmake
cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime VERSION 0.2.0 LANGUAGES C CXX)

include(FetchContent)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(LLAMA_CPP_COMMIT "6bdd77f13cf11b264b4231d320afc404f48d576e")
set(LLW_BACKEND_PACK "CPU" CACHE STRING "Runtime backend pack: CPU, CUDA, or VULKAN")
set_property(CACHE LLW_BACKEND_PACK PROPERTY STRINGS CPU CUDA VULKAN)
string(TOUPPER "${LLW_BACKEND_PACK}" LLW_BACKEND_PACK)
if(NOT LLW_BACKEND_PACK MATCHES "^(CPU|CUDA|VULKAN)$")
  message(FATAL_ERROR "LLW_BACKEND_PACK must be CPU, CUDA, or VULKAN")
endif()

set(BUILD_SHARED_LIBS ON CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(GGML_BACKEND_DL ON CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_CPU ON CACHE BOOL "" FORCE)
set(GGML_CUDA OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN OFF CACHE BOOL "" FORCE)
set(GGML_METAL OFF CACHE BOOL "" FORCE)
if(LLW_BACKEND_PACK STREQUAL "CUDA")
  set(GGML_CUDA ON CACHE BOOL "" FORCE)
elseif(LLW_BACKEND_PACK STREQUAL "VULKAN")
  set(GGML_VULKAN ON CACHE BOOL "" FORCE)
endif()

FetchContent_Declare(
  llama_cpp
  GIT_REPOSITORY https://github.com/ggml-org/llama.cpp.git
  GIT_TAG ${LLAMA_CPP_COMMIT}
  GIT_SHALLOW FALSE
  GIT_PROGRESS TRUE
)
FetchContent_MakeAvailable(llama_cpp)

enable_testing()

add_library(local_llm_runtime SHARED src/fake_runtime.cpp)
target_compile_definitions(local_llm_runtime PRIVATE
  LLW_RUNTIME_BUILD
  LLW_BACKEND_PACK_NAME="${LLW_BACKEND_PACK}"
  LLW_LLAMA_CPP_COMMIT="${LLAMA_CPP_COMMIT}"
)
target_include_directories(local_llm_runtime PUBLIC include PRIVATE src)
target_link_libraries(local_llm_runtime PRIVATE llama ggml)

add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
target_link_libraries(llw_abi_layout_test PRIVATE local_llm_runtime)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test)
```

- [ ] **Step 3: Configure and inspect the CPU source pin**

Run:

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
git -C .cmake-build/llm-cpu/_deps/llama_cpp-src rev-parse HEAD
cmake --build .cmake-build/llm-cpu --config Debug --target llama
```

Expected: `rev-parse` prints exactly `6bdd77f13cf11b264b4231d320afc404f48d576e`; target `llama` builds and the existing fake runtime remains buildable.

- [ ] **Step 4: Commit the reproducible dependency boundary**

```powershell
git add native/llm-runtime/CMakeLists.txt
git commit -m "build: pin llama cpp backend packs"
```

### Task 2: Extend The C ABI Append-Only To Version 1.1

**Files:**
- Modify: `native/llm-runtime/include/llw_runtime.h`
- Modify: `native/llm-runtime/src/fake_runtime.cpp`
- Modify: `native/llm-runtime/tests/abi_layout_test.cpp`
- Modify: `crates/llm-runtime-sys/src/lib.rs`

- [ ] **Step 1: Add failing C and Rust assertions for ABI 1.1**

In `native/llm-runtime/tests/abi_layout_test.cpp`, change the minor assertion and add these assertions before runtime behavior checks:

```cpp
static_assert(LLW_ABI_MINOR == 1u);
static_assert(sizeof(llw_bytes_t) == 88u);
static_assert(sizeof(llw_buffer_t) == 96u);
static_assert(sizeof(llw_scheduler_config_t) == 88u);
static_assert(sizeof(llw_model_load_params_t) == 168u);
static_assert(sizeof(llw_request_params_t) == 192u);
static_assert(sizeof(llw_scheduler_snapshot_t) == 104u);
static_assert(sizeof(llw_metrics_t) == 128u);
static_assert(offsetof(llw_runtime_create_params_t, scheduler) == 160u);
static_assert(sizeof(llw_runtime_create_params_t) == 312u);
LLW_ASSERT_FIELD(llw_bytes_t, data, const std::uint8_t*, 8u);
LLW_ASSERT_FIELD(llw_bytes_t, len, std::uint64_t, 16u);
LLW_ASSERT_FIELD(llw_bytes_t, reserved, std::uint64_t[8], 24u);
LLW_ASSERT_FIELD(llw_buffer_t, data, std::uint8_t*, 8u);
LLW_ASSERT_FIELD(llw_buffer_t, capacity, std::uint64_t, 16u);
LLW_ASSERT_FIELD(llw_buffer_t, len, std::uint64_t, 24u);
LLW_ASSERT_FIELD(llw_scheduler_config_t, slot_count, std::uint32_t, 8u);
LLW_ASSERT_FIELD(llw_scheduler_config_t, request_queue_capacity, std::uint32_t, 12u);
LLW_ASSERT_FIELD(llw_scheduler_config_t, event_queue_capacity, std::uint32_t, 16u);
LLW_ASSERT_FIELD(llw_scheduler_config_t, reserved, std::uint64_t[8], 24u);
LLW_ASSERT_FIELD(llw_model_load_params_t, path_utf8, const std::uint8_t*, 8u);
LLW_ASSERT_FIELD(llw_model_load_params_t, path_len, std::uint64_t, 16u);
LLW_ASSERT_FIELD(llw_model_load_params_t, backend, std::int32_t, 24u);
LLW_ASSERT_FIELD(llw_model_load_params_t, context_tokens_per_slot, std::uint32_t, 32u);
LLW_ASSERT_FIELD(llw_model_load_params_t, n_gpu_layers, std::int32_t, 52u);
LLW_ASSERT_FIELD(llw_model_load_params_t, reserved, std::uint64_t[12], 72u);
LLW_ASSERT_FIELD(llw_request_params_t, model_handle, llw_handle_t, 8u);
LLW_ASSERT_FIELD(llw_request_params_t, prompt, const std::uint8_t*, 16u);
LLW_ASSERT_FIELD(llw_request_params_t, max_new_tokens, std::uint32_t, 32u);
LLW_ASSERT_FIELD(llw_request_params_t, temperature, float, 40u);
LLW_ASSERT_FIELD(llw_request_params_t, stop_sequences, const llw_bytes_t*, 80u);
LLW_ASSERT_FIELD(llw_request_params_t, request_user_data, void*, 88u);
LLW_ASSERT_FIELD(llw_request_params_t, reserved, std::uint64_t[12], 96u);
LLW_ASSERT_FIELD(llw_scheduler_snapshot_t, accepted_requests, std::uint64_t, 24u);
LLW_ASSERT_FIELD(llw_scheduler_snapshot_t, reserved, std::uint64_t[8], 40u);
LLW_ASSERT_FIELD(llw_metrics_t, prompt_tokens, std::uint64_t, 8u);
LLW_ASSERT_FIELD(llw_metrics_t, decode_ns, std::uint64_t, 56u);
LLW_ASSERT_FIELD(llw_metrics_t, reserved, std::uint64_t[8], 64u);
```

In the Rust `ffi_struct_layouts_match_x64_c_contract` test, add:

```rust
assert_eq!(ABI_MINOR, 1);
assert_layout!(Bytes, 88);
assert_layout!(Buffer, 96);
assert_layout!(SchedulerConfig, 88);
assert_layout!(ModelLoadParams, 168);
assert_layout!(RequestParams, 192);
assert_layout!(SchedulerSnapshot, 104);
assert_layout!(Metrics, 128);
assert_offset!(RuntimeCreateParams, scheduler, 160);
assert_offset!(RuntimeCreateParams, reserved_v1, 248);
assert_eq!(std::mem::size_of::<RuntimeCreateParams>(), 312);
assert_offset!(Bytes, data, 8);
assert_offset!(Bytes, len, 16);
assert_offset!(Bytes, reserved, 24);
assert_offset!(Buffer, data, 8);
assert_offset!(Buffer, capacity, 16);
assert_offset!(Buffer, len, 24);
assert_offset!(SchedulerConfig, slot_count, 8);
assert_offset!(SchedulerConfig, request_queue_capacity, 12);
assert_offset!(SchedulerConfig, event_queue_capacity, 16);
assert_offset!(SchedulerConfig, reserved, 24);
assert_offset!(ModelLoadParams, path_utf8, 8);
assert_offset!(ModelLoadParams, path_len, 16);
assert_offset!(ModelLoadParams, backend, 24);
assert_offset!(ModelLoadParams, context_tokens_per_slot, 32);
assert_offset!(ModelLoadParams, n_gpu_layers, 52);
assert_offset!(ModelLoadParams, reserved, 72);
assert_offset!(RequestParams, model_handle, 8);
assert_offset!(RequestParams, prompt, 16);
assert_offset!(RequestParams, max_new_tokens, 32);
assert_offset!(RequestParams, temperature, 40);
assert_offset!(RequestParams, stop_sequences, 80);
assert_offset!(RequestParams, request_user_data, 88);
assert_offset!(RequestParams, reserved, 96);
assert_offset!(SchedulerSnapshot, accepted_requests, 24);
assert_offset!(SchedulerSnapshot, reserved, 40);
assert_offset!(Metrics, prompt_tokens, 8);
assert_offset!(Metrics, decode_ns, 56);
assert_offset!(Metrics, reserved, 64);
```

Add a C++ behavior case that sets `create.struct_size = 160u`, calls `llw_runtime_create`, expects `LLW_OK`, and destroys the returned runtime. This proves an ABI 1.0 caller prefix remains accepted after the append-only extension.

Run:

```powershell
cargo test -p llm-runtime-sys ffi_struct_layouts_match_x64_c_contract
cmake --build .cmake-build/llm-runtime --config Debug
```

Expected: Rust fails because the new types do not exist; C++ fails because ABI minor is still 0 and the types are absent.

- [ ] **Step 2: Replace the public header with the exact ABI 1.1 contract**

Keep the existing platform/export macro preamble, opaque `llw_runtime_t`, all ABI 1.0 constants, comments, seven exports, and every ABI 1.0 structure/prefix byte-for-byte. Change `LLW_ABI_MINOR` to `1u`, add these constants after the existing result/event constants, define `llw_bytes_t`, `llw_buffer_t`, and `llw_scheduler_config_t` before `llw_runtime_create_params_t`, append `scheduler` and `reserved_v1` after its existing `reserved`, then add the remaining new structures and exports before `#endif`:

```c
#define LLW_ERR_BUSY ((llw_result_t)4)
#define LLW_ERR_QUEUE_FULL ((llw_result_t)5)
#define LLW_ERR_NOT_FOUND ((llw_result_t)6)
#define LLW_ERR_INVALID_STATE ((llw_result_t)7)
#define LLW_ERR_CANCELLED ((llw_result_t)8)
#define LLW_ERR_UNSUPPORTED ((llw_result_t)9)

#define LLW_EVENT_DATA_NONE ((uint32_t)0)
#define LLW_EVENT_DATA_BYTES ((uint32_t)1)
#define LLW_EVENT_DATA_UTF8 ((uint32_t)2)
#define LLW_EVENT_DATA_JSON_UTF8 ((uint32_t)3)

#define LLW_REQUEST_STATE_QUEUED ((int32_t)1)
#define LLW_REQUEST_STATE_PREPROCESSING ((int32_t)2)
#define LLW_REQUEST_STATE_RUNNING ((int32_t)3)
#define LLW_REQUEST_STATE_DONE ((int32_t)4)
#define LLW_REQUEST_STATE_CANCELLED ((int32_t)5)
#define LLW_REQUEST_STATE_ERROR ((int32_t)6)

#define LLW_MAX_SLOTS 4u
#define LLW_MAX_QUEUE_CAPACITY 1024u
#define LLW_MAX_EVENT_QUEUE_CAPACITY 65536u
#define LLW_MAX_PROMPT_BYTES (16u * 1024u * 1024u)
#define LLW_MAX_STOP_SEQUENCES 8u
#define LLW_MAX_STOP_BYTES 256u

#pragma pack(push, 8)

typedef struct llw_bytes_t {
    uint32_t struct_size;
    uint32_t flags;
    const uint8_t* data;
    uint64_t len;
    uint64_t reserved[8];
} llw_bytes_t;

typedef struct llw_buffer_t {
    uint32_t struct_size;
    uint32_t flags;
    uint8_t* data;
    uint64_t capacity;
    uint64_t len;
    uint64_t reserved[8];
} llw_buffer_t;

typedef struct llw_scheduler_config_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t slot_count;
    uint32_t request_queue_capacity;
    uint32_t event_queue_capacity;
    uint32_t reserved0;
    uint64_t reserved[8];
} llw_scheduler_config_t;

/* Append these fields after llw_runtime_create_params_t.reserved. */
/* llw_scheduler_config_t scheduler; */
/* uint64_t reserved_v1[8]; */

typedef struct llw_model_load_params_t {
    uint32_t struct_size;
    uint32_t flags;
    const uint8_t* path_utf8;
    uint64_t path_len;
    int32_t backend;
    uint32_t device_index;
    uint32_t context_tokens_per_slot;
    uint32_t logical_batch_tokens;
    uint32_t physical_batch_tokens;
    int32_t n_threads;
    int32_t n_threads_batch;
    int32_t n_gpu_layers;
    uint32_t use_mmap;
    uint32_t use_mlock;
    uint32_t check_tensors;
    uint32_t reserved0;
    uint64_t reserved[12];
} llw_model_load_params_t;

typedef struct llw_request_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_handle_t model_handle;
    const uint8_t* prompt;
    uint64_t prompt_len;
    uint32_t max_new_tokens;
    uint32_t seed;
    float temperature;
    int32_t top_k;
    float top_p;
    float min_p;
    int32_t repeat_last_n;
    float repeat_penalty;
    float frequency_penalty;
    float presence_penalty;
    uint32_t stop_count;
    uint32_t reserved0;
    const llw_bytes_t* stop_sequences;
    void* request_user_data;
    uint64_t reserved[12];
} llw_request_params_t;

typedef struct llw_scheduler_snapshot_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t slot_count;
    uint32_t active_count;
    uint32_t queued_count;
    uint32_t queue_capacity;
    uint64_t accepted_requests;
    uint64_t terminal_requests;
    uint64_t reserved[8];
} llw_scheduler_snapshot_t;

typedef struct llw_metrics_t {
    uint32_t struct_size;
    uint32_t flags;
    uint64_t prompt_tokens;
    uint64_t generated_tokens;
    uint64_t decode_calls;
    uint64_t cancelled_requests;
    uint64_t failed_requests;
    uint64_t queue_wait_ns;
    uint64_t decode_ns;
    uint64_t reserved[8];
} llw_metrics_t;

#pragma pack(pop)

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_option_schema(
    llw_runtime_t* runtime, llw_buffer_t* out_json, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_load(
    llw_runtime_t* runtime, const llw_model_load_params_t* params,
    llw_handle_t* out_model, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_unload(
    llw_runtime_t* runtime, llw_handle_t model, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_submit(
    llw_runtime_t* runtime, const llw_request_params_t* params,
    llw_handle_t* out_request, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_cancel(
    llw_runtime_t* runtime, llw_handle_t request, llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_scheduler_snapshot(
    llw_runtime_t* runtime, llw_scheduler_snapshot_t* out_snapshot,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_metrics(
    llw_runtime_t* runtime, llw_metrics_t* out_metrics, llw_error_t* out_error);
```

The actual `llw_runtime_create_params_t` definition must be exactly:

```c
typedef struct llw_runtime_create_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_callback_table_t callbacks;
    uint64_t reserved[8];
    llw_scheduler_config_t scheduler;
    uint64_t reserved_v1[8];
} llw_runtime_create_params_t;
```

In `native/llm-runtime/src/fake_runtime.cpp`, replace the create-size check with the legacy-prefix boundary so the incremental fake DLL remains compatible while later tasks replace it:

```cpp
constexpr size_t LLW_RUNTIME_CREATE_V1_0_SIZE = offsetof(llw_runtime_create_params_t, scheduler);
if (!params || !out_runtime || params->struct_size < LLW_RUNTIME_CREATE_V1_0_SIZE) {
    return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid runtime create parameters");
}
```

Add `#include <cstddef>` for `offsetof`. The fake runtime ignores the appended scheduler fields; the real runtime in Task 7 reads them only when `struct_size >= sizeof(llw_runtime_create_params_t)` and otherwise uses 1 slot, request queue 16, and event queue 1024.

ABI rules to retain in the header comments:

```text
All input byte pointers are borrowed only for the call. llw_request_submit copies the prompt,
stop arrays, stop bytes, and request_user_data value before returning. Event data uses event.flags:
TOKEN is BYTES; LOG is UTF8; QUEUED, MODEL_PROGRESS, METRICS, DONE, CANCELLED, and ERROR are
JSON_UTF8. event and data are valid only during the callback and must be copied before return.
Only the dispatcher thread invokes on_event; callbacks are serialized, may not call llw_* reentrantly,
and must not block indefinitely. Each accepted request emits increasing sequence_number values and
exactly one of DONE, CANCELLED, or ERROR.
```

Bounds to document beside fields: slots `1..4`; request queue `1..1024`; event queue `16..65536`; context per slot `512..262144`; logical batch `1..8192`; physical batch `1..logical_batch`; threads `1..256`; GPU layers `-1..65535`; prompt `1..LLW_MAX_PROMPT_BYTES`; new tokens `1..1048576`; top-k `0..100000`; probabilities `0.0..1.0`; temperature `0.0..10.0`; repeat-last-n `0..262144`; penalties finite with repeat penalty `0.0..10.0`; stop count `0..8`; each stop `1..256` bytes.

- [ ] **Step 3: Add exact Rust mirrors and function pointer types**

In `crates/llm-runtime-sys/src/lib.rs`, change `ABI_MINOR` to `1`, append the same integer constants, append `scheduler: SchedulerConfig` and `reserved_v1: [u64; 8]` to `RuntimeCreateParams`, and add `#[repr(C)]` Rust structures with fields in precisely the C order and these pointer translations:

```rust
pub struct Bytes { pub struct_size: u32, pub flags: u32, pub data: *const u8, pub len: u64, pub reserved: [u64; 8] }
pub struct Buffer { pub struct_size: u32, pub flags: u32, pub data: *mut u8, pub capacity: u64, pub len: u64, pub reserved: [u64; 8] }
pub struct SchedulerConfig { pub struct_size: u32, pub flags: u32, pub slot_count: u32, pub request_queue_capacity: u32, pub event_queue_capacity: u32, pub reserved0: u32, pub reserved: [u64; 8] }
pub struct ModelLoadParams { pub struct_size: u32, pub flags: u32, pub path_utf8: *const u8, pub path_len: u64, pub backend: i32, pub device_index: u32, pub context_tokens_per_slot: u32, pub logical_batch_tokens: u32, pub physical_batch_tokens: u32, pub n_threads: i32, pub n_threads_batch: i32, pub n_gpu_layers: i32, pub use_mmap: u32, pub use_mlock: u32, pub check_tensors: u32, pub reserved0: u32, pub reserved: [u64; 12] }
pub struct RequestParams { pub struct_size: u32, pub flags: u32, pub model_handle: Handle, pub prompt: *const u8, pub prompt_len: u64, pub max_new_tokens: u32, pub seed: u32, pub temperature: f32, pub top_k: i32, pub top_p: f32, pub min_p: f32, pub repeat_last_n: i32, pub repeat_penalty: f32, pub frequency_penalty: f32, pub presence_penalty: f32, pub stop_count: u32, pub reserved0: u32, pub stop_sequences: *const Bytes, pub request_user_data: *mut c_void, pub reserved: [u64; 12] }
pub struct SchedulerSnapshot { pub struct_size: u32, pub flags: u32, pub slot_count: u32, pub active_count: u32, pub queued_count: u32, pub queue_capacity: u32, pub accepted_requests: u64, pub terminal_requests: u64, pub reserved: [u64; 8] }
pub struct Metrics { pub struct_size: u32, pub flags: u32, pub prompt_tokens: u64, pub generated_tokens: u64, pub decode_calls: u64, pub cancelled_requests: u64, pub failed_requests: u64, pub queue_wait_ns: u64, pub decode_ns: u64, pub reserved: [u64; 8] }
```

Give each output/config structure a manual `Default` that sets its own `struct_size`, zeroes all fields, and defaults scheduler values to `slot_count=1`, `request_queue_capacity=16`, and `event_queue_capacity=1024`. Do not derive `Default` for pointer-containing structures.

Add exact function pointer aliases for all seven new signatures but do not load them yet; Task 9 performs the loader change after the DLL exports exist.

- [ ] **Step 4: Run layout tests**

```powershell
cargo test -p llm-runtime-sys ffi_struct_layouts_match_x64_c_contract
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug -R llw_abi_layout_test --output-on-failure
```

Expected: Rust and C++ layout tests pass. Existing ABI 1.0 offsets remain unchanged, and the scheduler field begins at byte 160.

- [ ] **Step 5: Commit the ABI contract**

```powershell
git add native/llm-runtime/include/llw_runtime.h native/llm-runtime/src/fake_runtime.cpp native/llm-runtime/tests/abi_layout_test.cpp crates/llm-runtime-sys/src/lib.rs
git commit -m "feat: define scheduler ABI contract"
```

### Task 3: Implement The Bounded Event Dispatcher

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Create: `native/llm-runtime/src/event_dispatcher.h`
- Create: `native/llm-runtime/src/event_dispatcher.cpp`
- Test: `native/llm-runtime/tests/scheduler_test.cpp`

- [ ] **Step 1: Write a failing dispatcher ownership test**

Create `native/llm-runtime/tests/scheduler_test.cpp` with a callback that copies `event->data` into a mutex-protected vector. Construct `EventDispatcher` with capacity 16, enqueue a TOKEN from a temporary `std::vector<uint8_t>{0xf0,0x9f,0x92,0xa1}`, mutate and destroy the source, call `drain_for_test()`, and assert the callback received the original four bytes on one non-test thread. Also assert two enqueue calls receive sequence numbers 1 and 2.

Run:

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_scheduler_test
```

Expected: FAIL because `event_dispatcher.h` does not exist.

- [ ] **Step 2: Create the dispatcher interface**

Create `native/llm-runtime/src/event_dispatcher.h`:

```cpp
#pragma once
#include "llw_runtime.h"
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <mutex>
#include <thread>
#include <vector>

struct OwnedEvent {
    int32_t type{};
    uint32_t data_format{};
    int32_t error_code{};
    llw_handle_t model{};
    llw_handle_t request{};
    uint32_t slot{UINT32_MAX};
    uint64_t sequence{};
    void* request_user_data{};
    std::vector<uint8_t> data;
};

class EventDispatcher {
public:
    EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity);
    ~EventDispatcher();
    EventDispatcher(const EventDispatcher&) = delete;
    EventDispatcher& operator=(const EventDispatcher&) = delete;
    bool publish(OwnedEvent event);
    void stop();
    void drain_for_test();
private:
    void run();
    llw_callback_table_t callbacks_{};
    const size_t capacity_;
    std::mutex mutex_;
    std::condition_variable readable_;
    std::condition_variable writable_;
    std::condition_variable drained_;
    std::deque<OwnedEvent> queue_;
    bool stopping_{};
    size_t in_callback_{};
    std::thread thread_;
};
```

Append this test target to `native/llm-runtime/CMakeLists.txt`:

```cmake
find_package(Threads REQUIRED)
add_executable(llw_scheduler_test tests/scheduler_test.cpp src/event_dispatcher.cpp)
target_include_directories(llw_scheduler_test PRIVATE include src tests)
target_link_libraries(llw_scheduler_test PRIVATE Threads::Threads)
add_test(NAME llw_scheduler_test COMMAND llw_scheduler_test)
```

- [ ] **Step 3: Implement payload ownership and serialized callbacks**

Create `native/llm-runtime/src/event_dispatcher.cpp`:

```cpp
#include "event_dispatcher.h"
#include <utility>

EventDispatcher::EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity)
    : callbacks_(callbacks), capacity_(capacity), thread_([this] { run(); }) {}

EventDispatcher::~EventDispatcher() { stop(); }

bool EventDispatcher::publish(OwnedEvent event) {
    std::unique_lock lock(mutex_);
    writable_.wait(lock, [this] { return stopping_ || queue_.size() < capacity_; });
    if (stopping_) return false;
    queue_.push_back(std::move(event));
    readable_.notify_one();
    return true;
}

void EventDispatcher::stop() {
    {
        std::lock_guard lock(mutex_);
        if (stopping_) return;
        stopping_ = true;
    }
    readable_.notify_all();
    writable_.notify_all();
    if (thread_.joinable()) thread_.join();
}

void EventDispatcher::drain_for_test() {
    std::unique_lock lock(mutex_);
    drained_.wait(lock, [this] { return queue_.empty() && in_callback_ == 0; });
}

void EventDispatcher::run() {
    for (;;) {
        OwnedEvent owned;
        {
            std::unique_lock lock(mutex_);
            readable_.wait(lock, [this] { return stopping_ || !queue_.empty(); });
            if (queue_.empty() && stopping_) break;
            owned = std::move(queue_.front());
            queue_.pop_front();
            ++in_callback_;
            writable_.notify_one();
        }
        if (callbacks_.on_event) {
            llw_event_t event{};
            event.struct_size = sizeof(event);
            event.flags = owned.data_format;
            event.event_type = owned.type;
            event.error_code = owned.error_code;
            event.model_handle = owned.model;
            event.request_handle = owned.request;
            event.slot_id = owned.slot;
            event.sequence_number = owned.sequence;
            event.data = owned.data.empty() ? nullptr : owned.data.data();
            event.data_len = owned.data.size();
            event.request_user_data = owned.request_user_data;
            callbacks_.on_event(&event, callbacks_.user_data);
        }
        {
            std::lock_guard lock(mutex_);
            --in_callback_;
            if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
        }
    }
    std::lock_guard lock(mutex_);
    if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
}
```

- [ ] **Step 4: Run the focused test**

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_scheduler_test
ctest --test-dir .cmake-build/llm-cpu -C Debug -R llw_scheduler_test --output-on-failure
```

Expected: dispatcher payload, sequence, and callback-thread assertions pass.

- [ ] **Step 5: Commit the dispatcher**

```powershell
git add native/llm-runtime/src/event_dispatcher.* native/llm-runtime/tests/scheduler_test.cpp
git commit -m "feat: dispatch owned runtime events"
```

### Task 4: Build The Scheduler Against A Fake Engine

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Create: `native/llm-runtime/src/inference_engine.h`
- Create: `native/llm-runtime/src/scheduler.h`
- Create: `native/llm-runtime/src/scheduler.cpp`
- Create: `native/llm-runtime/tests/fake_engine.h`
- Modify: `native/llm-runtime/tests/scheduler_test.cpp`

- [ ] **Step 1: Write failing lifecycle tests**

Add five tests to `scheduler_test.cpp`: two requests accepted together produce interleaved TOKEN events and distinct slot IDs; with one slot and queue capacity one, the third submit returns `LLW_ERR_QUEUE_FULL` without a handle; queued cancellation emits CANCELLED without engine decode; active cancellation becomes CANCELLED before the next batch and clears its sequence; repeated cancellation returns the same result and every accepted request has exactly one terminal event. Use a fake engine barrier so tests observe both requests active before releasing decode.

Run:

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_scheduler_test
```

Expected: FAIL because `Scheduler` and `FakeEngine` do not exist.

- [ ] **Step 2: Define the engine seam**

Create `native/llm-runtime/src/inference_engine.h`:

```cpp
#pragma once
#include "llw_runtime.h"
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

struct SamplingConfig {
    uint32_t seed{}; float temperature{}; int32_t top_k{}; float top_p{}; float min_p{};
    int32_t repeat_last_n{}; float repeat_penalty{}; float frequency_penalty{}; float presence_penalty{};
};
struct EngineRequest {
    llw_handle_t handle{}; uint32_t seq_id{}; std::vector<uint8_t> prompt;
    uint32_t max_new_tokens{}; SamplingConfig sampling; std::vector<std::vector<uint8_t>> stops;
};
struct EngineStep {
    llw_handle_t handle{}; std::vector<uint8_t> token_bytes; bool finished{}; bool failed{}; std::string error;
};
class InferenceEngine {
public:
    virtual ~InferenceEngine() = default;
    virtual void start(EngineRequest request) = 0;
    virtual std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) = 0;
    virtual void cancel(llw_handle_t handle, uint32_t seq_id) = 0;
};
```

Replace the scheduler test target declaration in `native/llm-runtime/CMakeLists.txt` with:

```cmake
add_executable(llw_scheduler_test
  tests/scheduler_test.cpp
  src/event_dispatcher.cpp
  src/scheduler.cpp
)
target_include_directories(llw_scheduler_test PRIVATE include src tests)
target_link_libraries(llw_scheduler_test PRIVATE Threads::Threads)
add_test(NAME llw_scheduler_test COMMAND llw_scheduler_test)
```

- [ ] **Step 3: Define scheduler ownership and states**

Create `native/llm-runtime/src/scheduler.h` with `RequestState {Queued, Preprocessing, Running, Done, Cancelled, Error}`, an owned `Request` containing copied prompt/stops/user-data and atomic cancellation, fixed `Slot` records with `seq_id == slot index`, and this public API:

```cpp
class Scheduler {
public:
    Scheduler(uint32_t slots, uint32_t queue_capacity, InferenceEngine& engine, EventDispatcher& events);
    ~Scheduler();
    llw_result_t submit(const llw_request_params_t& params, llw_handle_t& out, std::string& error);
    llw_result_t cancel(llw_handle_t handle, std::string& error);
    llw_scheduler_snapshot_t snapshot() const;
    llw_metrics_t metrics() const;
    void cancel_all_and_wait();
private:
    void run();
    void promote_locked();
    void finish_locked(llw_handle_t, RequestState, int32_t, std::string);
    void publish_locked(const Request&, int32_t, uint32_t, int32_t, std::vector<uint8_t>);
};
```

The private members must include one mutex/condition variable, a FIFO deque of handles, a map of owned requests, `1..4` slots, one worker thread, monotonic nonzero handle generation, and counters matching `llw_metrics_t`. No request may be erased until its terminal event is queued.

- [ ] **Step 4: Implement deterministic queueing and cancellation**

In `scheduler.cpp`, implement these exact state rules:

```text
submit validates/copies before locking; accepted -> QUEUED and one QUEUED JSON event.
If queued.size == capacity and no slot can accept immediately -> LLW_ERR_QUEUE_FULL, out=0, no event.
promote takes FIFO requests into free slots, assigns seq_id=slot index, calls engine.start, then RUNNING.
queued cancel removes from FIFO and finishes CANCELLED without calling engine.cancel.
active cancel marks cancellation; worker calls engine.cancel before constructing its next decode handle list.
decode receives all non-cancelled active handles in one call; each EngineStep emits TOKEN BYTES, then
DONE on finished, ERROR on failed, or remains RUNNING. Cancellation wins over a simultaneous normal step.
After each decode call, publish one METRICS JSON_UTF8 event with promptTokens, generatedTokens,
decodeCalls, and decodeNanoseconds. The event uses request_handle 0 because it is runtime-wide.
finish_locked uses a terminal_emitted boolean; a second finish attempt is a no-op; it releases the slot,
increments terminal counters, and queues exactly one terminal JSON payload.
cancel of a known terminal request returns LLW_OK; unknown handle returns LLW_ERR_NOT_FOUND.
cancel_all_and_wait cancels queued and active requests and waits until active_count=queued_count=0.
```

Use JSON payloads with fixed keys and integer values, for example `{"state":"queued","queuePosition":1}`, `{"state":"cancelled"}`, and `{"state":"done","generatedTokens":3}`. Build JSON from internal numeric/string values with a small `json_escape` function that escapes quote, backslash, control bytes as `\u00XX`, and never inserts prompt/token bytes.

- [ ] **Step 5: Create the deterministic fake engine**

Create `tests/fake_engine.h`. `start` stores the request by handle. Each `decode(active)` call returns one token byte equal to `'A' + seq_id` for every handle, sets `finished` after three calls for that handle, records each active vector for shared-batch assertions, and honors a condition-variable barrier. `cancel` records `(handle, seq_id)` and erases the request. Protect all fake state with one mutex.

- [ ] **Step 6: Run scheduler tests repeatedly**

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_scheduler_test
1..20 | ForEach-Object { ctest --test-dir .cmake-build/llm-cpu -C Debug -R llw_scheduler_test --output-on-failure }
```

Expected: all 20 runs pass; the two-request test proves at least one fake `decode` call contains both handles, queue-full is deterministic, active and queued cancellation pass, and terminal counts equal accepted counts.

- [ ] **Step 7: Commit the scheduler core**

```powershell
git add native/llm-runtime/src/inference_engine.h native/llm-runtime/src/scheduler.* native/llm-runtime/tests/fake_engine.h native/llm-runtime/tests/scheduler_test.cpp
git commit -m "feat: schedule bounded concurrent requests"
```

### Task 5: Load One Model And Select A Pack Device

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Create: `native/llm-runtime/src/llama_engine.h`
- Create: `native/llm-runtime/src/llama_engine.cpp`
- Test: `native/llm-runtime/tests/llama_engine_test.cpp`

- [ ] **Step 1: Write failing parameter and device-selection tests**

Create `native/llm-runtime/tests/llama_engine_test.cpp`. Test the pure helpers without a model: reject backend values outside AUTO/CPU/CUDA/VULKAN; reject slots outside `1..4`; reject every model bound documented in Task 2; choose the Nth matching device by backend type and return `LLW_ERR_NOT_FOUND` when the index is absent; reject CUDA when only CPU/Vulkan records exist and Vulkan when only CPU/CUDA records exist. Pass a synthetic vector of `{backend,index,name}` records into the selector so these tests do not require GPU hardware.

Run:

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_llama_engine_test
```

Expected: FAIL because the target and helper do not exist.

- [ ] **Step 2: Define llama.cpp ownership**

Create `native/llm-runtime/src/llama_engine.h`:

```cpp
#pragma once
#include "inference_engine.h"
#include "llama.h"
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>

struct DeviceRecord {
    int32_t backend{};
    uint32_t backend_index{};
    ggml_backend_dev_t device{};
    std::string id;
    std::string name;
    std::string vendor;
};

struct ModelConfig {
    std::string path;
    int32_t backend{};
    uint32_t device_index{};
    uint32_t slots{};
    uint32_t context_tokens_per_slot{};
    uint32_t logical_batch_tokens{};
    uint32_t physical_batch_tokens{};
    int32_t n_threads{};
    int32_t n_threads_batch{};
    int32_t n_gpu_layers{};
    bool use_mmap{};
    bool use_mlock{};
    bool check_tensors{};
};

llw_result_t validate_model_config(const ModelConfig&, std::string&);
std::optional<DeviceRecord> select_device(const std::vector<DeviceRecord>&, int32_t, uint32_t);
std::vector<DeviceRecord> enumerate_pack_devices(const std::string& backend_directory);

class LlamaEngine final : public InferenceEngine {
public:
    LlamaEngine(ModelConfig config, std::function<void(float)> progress);
    ~LlamaEngine() override;
    void start(EngineRequest request) override;
    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override;
    void cancel(llw_handle_t handle, uint32_t seq_id) override;
private:
    struct Sequence;
    ModelConfig config_;
    llama_model* model_{};
    llama_context* context_{};
    const llama_vocab* vocab_{};
    llama_batch batch_{};
    std::unordered_map<llw_handle_t, std::unique_ptr<Sequence>> sequences_;
    std::mutex mutex_;
};
```

- [ ] **Step 3: Implement pack device discovery and model construction**

In `llama_engine.cpp`, call `llama_backend_init()` once through a process-static reference-counted guard. Before enumeration, call `ggml_backend_load_all_from_path(backend_directory.c_str())`. Enumerate with `ggml_backend_dev_count/get`, map `GGML_BACKEND_DEVICE_TYPE_CPU` to CPU, and map GPU/IGPU devices to the compile-time pack backend. Set `id` to the pinned header's `ggml_backend_dev_props.device_id` when non-null and otherwise to `"<backend>:<index>"`; use `ggml_backend_dev_name`, `ggml_backend_dev_description`, and backend registry name as stable display/vendor data.

Construct model parameters from `llama_model_default_params()`:

```cpp
llama_model_params mp = llama_model_default_params();
ggml_backend_dev_t selected_devices[2] = {selected.device, nullptr};
mp.devices = selected_devices;
mp.n_gpu_layers = config.n_gpu_layers;
mp.main_gpu = 0;
mp.use_mmap = config.use_mmap;
mp.use_mlock = config.use_mlock;
mp.check_tensors = config.check_tensors;
mp.progress_callback = progress_bridge;
mp.progress_callback_user_data = &progress_state;
model_ = llama_model_load_from_file(config.path.c_str(), mp);
if (!model_) throw std::runtime_error("llama_model_load_from_file failed");

llama_context_params cp = llama_context_default_params();
const uint64_t total_context = uint64_t(config.context_tokens_per_slot) * config.slots;
if (total_context > UINT32_MAX) throw std::invalid_argument("total context exceeds uint32_t");
cp.n_ctx = static_cast<uint32_t>(total_context);
cp.n_batch = config.logical_batch_tokens;
cp.n_ubatch = config.physical_batch_tokens;
cp.n_seq_max = config.slots;
cp.n_threads = config.n_threads;
cp.n_threads_batch = config.n_threads_batch;
cp.embeddings = false;
cp.no_perf = false;
context_ = llama_init_from_model(model_, cp);
if (!context_) throw std::runtime_error("llama_init_from_model failed");
vocab_ = llama_model_get_vocab(model_);
batch_ = llama_batch_init(static_cast<int32_t>(config.logical_batch_tokens), 0, 1);
```

The progress bridge returns `true` and sends progress into the bounded dispatcher through the supplied function; it must not invoke the Rust callback directly. The destructor frees `batch_`, then context, then model, and finally releases the backend guard. On partial construction, use local `unique_ptr` guards so each acquired object is freed exactly once.

- [ ] **Step 4: Add the engine test target**

Append to CMake:

```cmake
add_executable(llw_llama_engine_test tests/llama_engine_test.cpp src/llama_engine.cpp)
target_include_directories(llw_llama_engine_test PRIVATE include src)
target_link_libraries(llw_llama_engine_test PRIVATE llama ggml Threads::Threads)
add_test(NAME llw_llama_engine_test COMMAND llw_llama_engine_test)
```

- [ ] **Step 5: Run pure CPU tests**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug --target llw_llama_engine_test
ctest --test-dir .cmake-build/llm-cpu -C Debug -R llw_llama_engine_test --output-on-failure
```

Expected: helper tests pass without a GGUF or GPU.

- [ ] **Step 6: Commit model and device ownership**

```powershell
git add native/llm-runtime/CMakeLists.txt native/llm-runtime/src/llama_engine.* native/llm-runtime/tests/llama_engine_test.cpp
git commit -m "feat: load llama model on selected device"
```

### Task 6: Implement Shared llama_batch Decode And Independent Samplers

**Files:**
- Modify: `native/llm-runtime/src/llama_engine.cpp`
- Modify: `native/llm-runtime/tests/llama_engine_test.cpp`

- [ ] **Step 1: Write failing batch-construction tests**

Extract a pure `BatchPlan plan_batch(vector<SequenceView>, capacity)` helper. Test that two active sequences produce one plan containing both sequence IDs, positions are monotonic per sequence, only the last prompt token requests logits, generation contributes one pending token per sequence, and capacity truncation resumes at the correct cursor. Test cancellation removes only the cancelled sequence and leaves the other sequence plan unchanged.

Run:

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_llama_engine_test
```

Expected: FAIL because `plan_batch` is absent.

- [ ] **Step 2: Implement sequence preprocessing and sampler chains**

Define `LlamaEngine::Sequence` with handle, seq ID, token vector, prompt cursor, next position, generated count, max tokens, pending sampled token, emitted byte buffer used for stop matching, stop byte vectors, and owning `llama_sampler*`.

In `start`, tokenize with the pinned API's negative-size retry convention:

```cpp
int32_t count = llama_tokenize(vocab_, reinterpret_cast<const char*>(request.prompt.data()),
    static_cast<int32_t>(request.prompt.size()), nullptr, 0, true, true);
if (count >= 0) throw std::runtime_error("token count query unexpectedly succeeded");
std::vector<llama_token> tokens(static_cast<size_t>(-count));
count = llama_tokenize(vocab_, reinterpret_cast<const char*>(request.prompt.data()),
    static_cast<int32_t>(request.prompt.size()), tokens.data(), static_cast<int32_t>(tokens.size()), true, true);
if (count < 0) throw std::runtime_error("prompt tokenization failed");
tokens.resize(static_cast<size_t>(count));
```

Create one sampler chain per request:

```cpp
llama_sampler_chain_params scp = llama_sampler_chain_default_params();
llama_sampler* chain = llama_sampler_chain_init(scp);
llama_sampler_chain_add(chain, llama_sampler_init_penalties(
    sampling.repeat_last_n, sampling.repeat_penalty,
    sampling.frequency_penalty, sampling.presence_penalty));
if (sampling.top_k > 0) llama_sampler_chain_add(chain, llama_sampler_init_top_k(sampling.top_k));
if (sampling.top_p < 1.0f) llama_sampler_chain_add(chain, llama_sampler_init_top_p(sampling.top_p, 1));
if (sampling.min_p > 0.0f) llama_sampler_chain_add(chain, llama_sampler_init_min_p(sampling.min_p, 1));
llama_sampler_chain_add(chain, llama_sampler_init_temp(sampling.temperature));
if (sampling.temperature == 0.0f) {
    llama_sampler_chain_add(chain, llama_sampler_init_greedy());
} else {
    llama_sampler_chain_add(chain, llama_sampler_init_dist(sampling.seed));
}
```

If any sampler allocation returns null, free the chain and fail the request. The chain owns all samplers added to it.

- [ ] **Step 3: Build and decode one shared batch per scheduler tick**

For all active handles under the engine mutex, clear `batch_.n_tokens`, append prompt chunks first and one pending generation token for every other sequence while respecting `logical_batch_tokens`, and set for each entry:

```cpp
batch_.token[i] = token;
batch_.pos[i] = sequence.next_position;
batch_.n_seq_id[i] = 1;
batch_.seq_id[i][0] = static_cast<llama_seq_id>(sequence.seq_id);
batch_.logits[i] = requests_logits ? 1 : 0;
```

Record an ordered `logit_owners` vector whenever `requests_logits` is true. Call `llama_decode(context_, batch_)` exactly once. Return ERROR steps for `-1` or values below `-1`; treat `1` as a context-capacity error; treat `2` as cooperative abort/cancellation; count positive warnings explicitly rather than silently succeeding.

For each logit owner at output index `i`, call:

```cpp
llama_token token = llama_sampler_sample(sequence.sampler, context_, static_cast<int32_t>(i));
llama_sampler_accept(sequence.sampler, token);
if (llama_vocab_is_eog(vocab_, token)) { step.finished = true; }
```

Convert non-EOG tokens with a stack buffer and negative-size retry using `llama_token_to_piece(vocab_, token, buffer, length, 0, true)`. Return the exact bytes as TOKEN data even when they are not independently valid UTF-8. Detect stop sequences against the cumulative emitted bytes before publishing: remove the matched stop suffix from the current token bytes when entirely contained there; otherwise retain at most `max_stop_length - 1` pending bytes per request so a stop spanning token boundaries is never emitted. Finish after `max_new_tokens` or EOG.

On cancellation and terminal completion call:

```cpp
llama_memory_seq_rm(llama_get_memory(context_), static_cast<llama_seq_id>(seq_id), -1, -1);
llama_sampler_free(sequence.sampler);
```

Require `llama_memory_seq_rm` to return true for whole-sequence removal; otherwise emit ERROR and do not reuse the slot until model teardown.

- [ ] **Step 4: Run engine and fake scheduler tests**

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_llama_engine_test llw_scheduler_test
ctest --test-dir .cmake-build/llm-cpu -C Debug -R "llw_(llama_engine|scheduler)_test" --output-on-failure
```

Expected: batch-plan, sampler configuration, stop-boundary, cancellation, and scheduler tests pass.

- [ ] **Step 5: Commit shared decode**

```powershell
git add native/llm-runtime/src/llama_engine.cpp native/llm-runtime/tests/llama_engine_test.cpp
git commit -m "feat: decode active slots in shared batches"
```

### Task 7: Implement The Complete C ABI Facade

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Delete: `native/llm-runtime/src/fake_runtime.cpp`
- Create: `native/llm-runtime/src/runtime.cpp`
- Modify: `native/llm-runtime/tests/abi_layout_test.cpp`

- [ ] **Step 1: Add failing export and lifecycle tests**

Extend `abi_layout_test.cpp` to create a runtime with 2 slots/queue 2/event queue 32, query option schema first with null storage and then an exact buffer, verify malformed/undersized structures return `LLW_ERR_INVALID_ARGUMENT`, verify model unload with handle 0 returns NOT_FOUND, request submit without a model returns INVALID_STATE, and destroy accepts null. Add a Windows export-table command:

```powershell
dumpbin /exports .cmake-build/llm-cpu/Debug/local_llm_runtime.dll | Select-String ' llw_'
```

Expected before implementation: the tests fail and only the original seven exports exist.

- [ ] **Step 2: Switch the DLL sources**

Replace the runtime target in CMake with:

```cmake
add_library(local_llm_runtime SHARED
  src/event_dispatcher.cpp
  src/llama_engine.cpp
  src/runtime.cpp
  src/scheduler.cpp
)
target_compile_definitions(local_llm_runtime PRIVATE
  LLW_RUNTIME_BUILD
  LLW_BACKEND_PACK_NAME="${LLW_BACKEND_PACK}"
  LLW_LLAMA_CPP_COMMIT="${LLAMA_CPP_COMMIT}"
)
target_include_directories(local_llm_runtime PUBLIC include PRIVATE src)
target_link_libraries(local_llm_runtime PRIVATE llama ggml Threads::Threads)
```

Delete `native/llm-runtime/src/fake_runtime.cpp` after the new target builds.

- [ ] **Step 3: Implement runtime ownership and validation**

In `runtime.cpp`, define `llw_runtime_t` with copied callback table, owned dispatcher, scheduler configuration, optional `unique_ptr<LlamaEngine>`, optional `unique_ptr<Scheduler>`, nonzero monotonic model handle, mutex, and canonical backend-directory path derived from the loaded DLL directory on Windows with `GetModuleHandleExW`/`GetModuleFileNameW`. Never accept a backend DLL directory or runtime DLL path through the C ABI.

Use one exception barrier for every result-returning export:

```cpp
template<class F>
llw_result_t guarded(llw_error_t* error, F&& body) noexcept {
    try { clear_error(error); return body(); }
    catch (const std::invalid_argument& e) { return fail(error, LLW_ERR_INVALID_ARGUMENT, e.what()); }
    catch (const std::bad_alloc&) { return fail(error, LLW_ERR_INTERNAL, "allocation failed"); }
    catch (const std::exception& e) { return fail(error, LLW_ERR_INTERNAL, e.what()); }
    catch (...) { return fail(error, LLW_ERR_INTERNAL, "unknown native exception"); }
}
```

Validation must check non-null pointers, minimum known prefixes with `offsetof(type, first_v1_field)` for old 1.0 create callers, `struct_size`, flags/reserved zero, numeric bounds, finite floats via `std::isfinite`, UTF-8 model path without embedded NUL, and prompt/stop lengths before dereference. Copy request bytes and pointer values before `Scheduler::submit` returns.

- [ ] **Step 4: Implement all fourteen exports**

Retain the seven existing functions. `llw_runtime_version` returns `"0.2.0"`; `llw_llama_cpp_commit` returns `LLW_LLAMA_CPP_COMMIT`; capabilities reflect the compile-time pack and max slots 4. Device listing comes from `enumerate_pack_devices` and retains the existing two-call buffer contract. Runtime creation publishes one LOG UTF8 event naming the pack, and model load publishes another LOG UTF8 event naming only the selected backend/device, never the model path or prompt.

Implement the seven new functions with these exact effects:

```text
get_option_schema: two-call UTF-8 JSON buffer contract; schema includes every bound and apply phase.
model_load: reject when a model exists; reserve a nonzero handle; publish MODEL_PROGRESS JSON through
dispatcher; create LlamaEngine and Scheduler; publish progress 1.0; set out_model only on success.
model_unload: require matching handle; scheduler.cancel_all_and_wait(); destroy scheduler then engine;
clear handle. It blocks until terminal events have entered the event queue, not until callbacks return.
request_submit: require current matching model; validate/copy; scheduler.submit; out_request is zero on failure.
request_cancel: delegate to scheduler with idempotent known-terminal semantics.
get_scheduler_snapshot/get_metrics: copy a zero-reserved snapshot under scheduler synchronization.
runtime_destroy: cancel/wait, destroy scheduler/engine, stop dispatcher after queued terminals drain, delete runtime.
```

The option schema is this exact minified JSON string so Rust and C++ can assert it:

```json
{"abiMinor":1,"backendPack":"CPU|CUDA|VULKAN","model":{"contextTokensPerSlot":{"min":512,"max":262144,"apply":"modelReload"},"logicalBatchTokens":{"min":1,"max":8192,"apply":"modelReload"},"physicalBatchTokens":{"min":1,"maxField":"logicalBatchTokens","apply":"modelReload"},"threads":{"min":1,"max":256,"apply":"modelReload"},"gpuLayers":{"min":-1,"max":65535,"apply":"modelReload"}},"scheduler":{"slots":{"min":1,"max":4,"apply":"runtimeRestart"},"queueCapacity":{"min":1,"max":1024,"apply":"runtimeRestart"}},"request":{"maxNewTokens":{"min":1,"max":1048576},"temperature":{"min":0.0,"max":10.0},"topK":{"min":0,"max":100000},"topP":{"min":0.0,"max":1.0},"minP":{"min":0.0,"max":1.0}}}
```

Replace `CPU|CUDA|VULKAN` at compile time with `LLW_BACKEND_PACK_NAME` before returning the bytes.

- [ ] **Step 5: Verify exports and native tests**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
$exports = dumpbin /exports .cmake-build/llm-cpu/Debug/local_llm_runtime.dll | Select-String ' llw_'
$exports.Count
$exports
```

Expected: all non-model native tests pass and export count is exactly 14, one occurrence for each declared function.

- [ ] **Step 6: Commit the ABI facade**

```powershell
git add native/llm-runtime
git commit -m "feat: expose native model and request lifecycle"
```

### Task 8: Load The New Exports And Copy Callback Data In Rust

**Files:**
- Modify: `crates/llm-runtime-sys/src/lib.rs`
- Modify: `crates/llm-runtime/Cargo.toml`
- Modify: `crates/llm-runtime/src/lib.rs`
- Modify: `crates/llm-runtime/tests/native_runtime.rs`

- [ ] **Step 1: Write failing raw-loader and callback-copy tests**

Add a `sys::Api` test helper assertion that its struct has fourteen function-pointer fields. Add safe-wrapper tests that invoke the callback trampoline with stack-backed TOKEN bytes, mutate the source after return, and assert the receiver retained the original bytes; invoke the consumer closure with a panic and assert `catch_unwind` prevents unwind across FFI; drop an active request and assert the injected cancel function is called once.

Run:

```powershell
cargo test -p llm-runtime --all-targets
```

Expected: FAIL because model/request/event APIs do not exist.

- [ ] **Step 2: Load each new export exactly once**

Add seven fields to `sys::Api` and seven `library.get` calls:

```rust
let runtime_get_option_schema = unsafe { *library.get::<RuntimeGetOptionSchemaFn>(b"llw_runtime_get_option_schema\0")? };
let model_load = unsafe { *library.get::<ModelLoadFn>(b"llw_model_load\0")? };
let model_unload = unsafe { *library.get::<ModelUnloadFn>(b"llw_model_unload\0")? };
let request_submit = unsafe { *library.get::<RequestSubmitFn>(b"llw_request_submit\0")? };
let request_cancel = unsafe { *library.get::<RequestCancelFn>(b"llw_request_cancel\0")? };
let get_scheduler_snapshot = unsafe { *library.get::<GetSchedulerSnapshotFn>(b"llw_get_scheduler_snapshot\0")? };
let get_metrics = unsafe { *library.get::<GetMetricsFn>(b"llw_get_metrics\0")? };
```

Initialize each corresponding `Api` field once. Add a source test that counts each NUL-terminated export byte string exactly once, including the original seven.

- [ ] **Step 3: Add safe owned types and callback delivery**

Add `crossbeam-channel = "0.5"` to workspace dependencies and `crates/llm-runtime/Cargo.toml`. Define `RuntimeOptions`, `ModelOptions`, `GenerationOptions`, `EventKind`, `EventPayload`, `RuntimeEvent`, `Model`, and `Request`. `RuntimeEvent` owns `Vec<u8>` payload and copies all scalar fields.

Use this trampoline shape:

```rust
unsafe extern "C" fn event_trampoline(event: *const sys::Event, user_data: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(raw) = event.as_ref() else { return };
        if raw.struct_size < std::mem::size_of::<sys::Event>() as u32 { return; }
        let payload = if raw.data.is_null() || raw.data_len == 0 {
            Vec::new()
        } else {
            let Ok(len) = usize::try_from(raw.data_len) else { return };
            unsafe { std::slice::from_raw_parts(raw.data, len) }.to_vec()
        };
        let state = unsafe { &*(user_data.cast::<CallbackState>()) };
        let _ = state.sender.send(RuntimeEvent::from_raw(raw, payload));
    }));
}
```

Keep `Box<CallbackState>` alive inside `RuntimeLibrary` until after native `runtime_destroy`. `Request::drop` calls cancel once unless a terminal event has marked its shared atomic terminal flag. `Model::drop` unloads only after all requests are dropped; encode this with `Arc<RuntimeInner>`, handle IDs, and a mutex rather than exposing raw pointers.

- [ ] **Step 4: Implement safe load, submit, cancel, snapshot, and metrics methods**

Convert paths with `path.as_os_str().encode_wide()` only for Rust filesystem checks; pass a canonical UTF-8 path to the C ABI and return an explicit error when a Windows path is not representable as UTF-8. Build stop `Vec<sys::Bytes>` only after owned stop byte vectors are finalized so pointers do not move. Keep all vectors alive through `request_submit`; native copies them before return.

Reject model APIs when runtime ABI minor is below 1. Preserve the existing unsafe `RuntimeLibrary::load(&Path)` contract; do not create a safe public arbitrary-DLL loader.

- [ ] **Step 5: Run Rust and DLL integration tests**

```powershell
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-cpu/Debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Expected: callback copying, panic containment, request-drop cancellation, existing probe, ABI layout, and runtime integration tests pass.

- [ ] **Step 6: Commit the Rust boundary**

```powershell
git add Cargo.toml Cargo.lock crates/llm-runtime-sys crates/llm-runtime
git commit -m "feat: wrap native inference callbacks safely"
```

### Task 9: Add A Reproducible Opt-In Tiny GGUF CPU Test

**Files:**
- Create: `native/llm-runtime/tests/fixtures/model.json`
- Create: `scripts/acquire-test-model.ps1`
- Create: `native/llm-runtime/tests/runtime_backend_test.cpp`
- Modify: `native/llm-runtime/CMakeLists.txt`
- Modify: `crates/llm-runtime/tests/native_runtime.rs`

- [ ] **Step 1: Record the non-redistributed fixture**

Create `native/llm-runtime/tests/fixtures/model.json`:

```json
{
  "name": "stories260K.gguf",
  "url": "https://huggingface.co/ggml-org/models/resolve/main/tinyllamas/stories260K.gguf",
  "sha256": "270cba1bd5109f42d03350f60406024560464db173c0e387d91f0426d3bd256d",
  "size": 1185376,
  "provenance": "ggml-org/models tinyllamas test artifact",
  "redistribution": "not committed; acquired only by explicit developer or CI opt-in"
}
```

The repository does not redistribute the GGUF. The acquisition URL is organization-maintained test data used for llama.cpp ecosystem validation; retaining only URL/checksum/provenance avoids silently relicensing model bytes. Before enabling a public CI download, repository maintainers must separately confirm the upstream artifact's license remains acceptable.

- [ ] **Step 2: Create checksum-verified acquisition**

Create `scripts/acquire-test-model.ps1`:

```powershell
param([string]$Destination = '.test-models/stories260K.gguf')
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -Raw 'native/llm-runtime/tests/fixtures/model.json' | ConvertFrom-Json
$destinationPath = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Destination))
$directory = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force $directory | Out-Null
$temporary = "$destinationPath.download"
Invoke-WebRequest -Uri $manifest.url -OutFile $temporary
$file = Get-Item $temporary
if ($file.Length -ne [int64]$manifest.size) { Remove-Item -LiteralPath $temporary; throw "fixture size mismatch" }
$actual = (Get-FileHash -Algorithm SHA256 $temporary).Hash.ToLowerInvariant()
if ($actual -ne $manifest.sha256) { Remove-Item -LiteralPath $temporary; throw "fixture SHA-256 mismatch: $actual" }
Move-Item -Force -LiteralPath $temporary -Destination $destinationPath
Write-Output $destinationPath
```

Add `.test-models/` to `.gitignore` in the implementation commit that creates this script.

- [ ] **Step 3: Write the CPU end-to-end test**

Create `runtime_backend_test.cpp`. Accept the GGUF path as `argv[1]`; read `LLW_TEST_BACKEND` as CPU/CUDA/VULKAN and default to CPU; create a 2-slot runtime; load the selected backend with context 512 per slot, batch 128, ubatch 64, hardware concurrency clamped to `1..8`, GPU layers 0 for CPU and `-1` for GPU; submit prompts `"Once"` and `"The"` with deterministic greedy temperature 0 and 8 max tokens; wait up to 60 seconds; assert both handles receive TOKEN bytes, exactly one DONE each, no ERROR, at least one scheduler snapshot reports active count 2, and model unload/runtime destroy succeed. On timeout cancel both and fail after terminal callbacks arrive.

Add the target and opt-in registration:

```cmake
add_executable(llw_runtime_backend_test tests/runtime_backend_test.cpp)
target_include_directories(llw_runtime_backend_test PRIVATE include)
target_link_libraries(llw_runtime_backend_test PRIVATE local_llm_runtime Threads::Threads)
if(DEFINED ENV{LLW_TEST_GGUF} AND EXISTS "$ENV{LLW_TEST_GGUF}")
  add_test(NAME llw_runtime_backend_test COMMAND llw_runtime_backend_test "$ENV{LLW_TEST_GGUF}")
  set_tests_properties(llw_runtime_backend_test PROPERTIES TIMEOUT 90 LABELS "model;opt-in")
endif()
```

- [ ] **Step 4: Acquire and run CPU E2E explicitly**

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-cpu/Debug/local_llm_runtime.dll')
cargo test --locked --workspace
```

Expected: native and Rust suites pass; CPU E2E proves two concurrent requests, streaming, terminal uniqueness, and unload. Standard developer/PR runs that do not set `LLW_TEST_GGUF` do not download or register the model test.

- [ ] **Step 5: Commit the opt-in CPU fixture strategy**

```powershell
git add .gitignore scripts/acquire-test-model.ps1 native/llm-runtime/tests crates/llm-runtime/tests/native_runtime.rs native/llm-runtime/CMakeLists.txt
git commit -m "test: validate concurrent CPU inference"
```

### Task 10: Add CUDA And Vulkan Pack Configuration Checks

**Files:**
- Create: `docs/native-runtime-validation.md`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Write backend validation documentation**

Create `docs/native-runtime-validation.md` with these exact commands and expectations:

```markdown
# Native Runtime Validation

All packs use llama.cpp `6bdd77f13cf11b264b4231d320afc404f48d576e`. A pack directory contains one
`local_llm_runtime.dll`, `llama.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`, and only its selected
GPU backend DLL/dependencies. Never combine DLLs from different build directories. Changing CPU/CUDA/Vulkan
packs requires model unload and process/runtime restart; in-process backend-core replacement is unsupported.

## CPU
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Release

## CUDA compile smoke
cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA
cmake --build .cmake-build/llm-cuda --config Release

## Vulkan compile smoke
$env:VULKAN_SDK must name an installed LunarG Vulkan SDK.
cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN
cmake --build .cmake-build/llm-vulkan --config Release

## Hardware-gated runtime checks
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
$env:LLW_TEST_BACKEND = 'CUDA' # use VULKAN on a Vulkan-capable host
ctest --test-dir .cmake-build/llm-cuda -C Release -R llw_runtime_backend_test --output-on-failure

The CUDA command requires a compatible NVIDIA driver/GPU and CUDA toolkit. The Vulkan command requires a
working Vulkan loader/device/driver. Standard CI performs configuration or compile smoke only when toolkits
are installed and does not claim runtime GPU validation. Metal is reserved for a future ABI-compatible macOS
plan and is not configured, compiled, or tested here.
```

- [ ] **Step 2: Add realistic CI jobs**

Keep the existing Windows CPU contract job and update it to configure `LLW_BACKEND_PACK=CPU`, acquire the tiny model only in a scheduled/manual job, and run locked Rust commands. Add `windows-cuda-configure` and `windows-vulkan-configure` jobs gated by repository variables `ENABLE_CUDA_SMOKE == 'true'` and `ENABLE_VULKAN_SMOKE == 'true'`. CUDA installs no driver and only configures/builds with the runner's available toolkit; Vulkan installs a pinned LunarG SDK version configured in the workflow and performs compile smoke. Neither job runs a model.

Use these job conditions exactly:

```yaml
if: vars.ENABLE_CUDA_SMOKE == 'true'
```

```yaml
if: vars.ENABLE_VULKAN_SMOKE == 'true'
```

Do not mark either optional job required until the repository has stable toolkit provisioning.

- [ ] **Step 3: Configure every available pack locally**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
if (Get-Command nvcc -ErrorAction SilentlyContinue) { cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA }
if ($env:VULKAN_SDK) { cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN }
```

Expected: CPU always configures. CUDA/Vulkan configure when their toolkits are present; absence is reported as skipped, not as runtime validation.

- [ ] **Step 4: Commit backend validation**

```powershell
git add docs/native-runtime-validation.md .github/workflows/ci.yml
git commit -m "ci: smoke test native backend packs"
```

### Task 11: Preserve The Managed Runtime-Pack Security Boundary

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime_probe.rs`
- Test: `apps/desktop/src-tauri/src/runtime_probe.rs`

- [ ] **Step 1: Add a regression assertion for command input**

Add a source-level test that reads `runtime_probe.rs`, finds `pub async fn probe_runtime`, and asserts the signature contains `runtime_pack_id: String` and does not contain `PathBuf` or `dll_path`. Keep the existing traversal, separator, junction escape, missing-pack, and canonical-root tests.

Run:

```powershell
cargo test -p local-llm-wiki-desktop runtime_probe
```

Expected: PASS without changing the production command. If it fails, restore the command to pack-ID-only resolution; do not add an arbitrary path compatibility overload.

- [ ] **Step 2: Verify runtime restart semantics remain outside the command**

Confirm `probe_runtime` loads one resolved pack for a bounded probe and drops it. Do not add model load, generation, backend switching, or frontend event wiring in this file; those belong to the later application plan.

- [ ] **Step 3: Commit only if the regression test changed the file**

```powershell
git add apps/desktop/src-tauri/src/runtime_probe.rs
git diff --cached --quiet; if ($LASTEXITCODE -ne 0) { git commit -m "test: preserve managed runtime pack boundary" }
```

### Task 12: Final Contract And Runtime Verification

**Files:**
- Read: `docs/superpowers/specs/2026-07-18-local-llm-desktop-mvp-design.md`
- Read: `native/llm-runtime/include/llw_runtime.h`
- Read: `crates/llm-runtime-sys/src/lib.rs`
- Read: `docs/native-runtime-validation.md`

- [ ] **Step 1: Audit ABI fields, offsets, and exports**

Compare every C structure name, field name, order, signedness, width, array length, pointer constness, and callback calling convention with its Rust `#[repr(C)]` mirror. Run both layout suites. Count all fourteen `library.get` byte strings and all fourteen DLL exports exactly once. Confirm ABI 1.0 field offsets are unchanged and all 1.1 additions are appended/new.

- [ ] **Step 2: Audit scheduler requirements**

Map evidence to: one loaded model; opaque nonzero numeric handles; explicit bounded model/request params; copied caller buffers; generic event encodings/lifetimes/thread rules; bounded request/event queues; configurable 1-4 slots; one shared batch decode call per tick; independent sequence IDs/KV/samplers; queued and active cancellation; exactly one terminal event; two concurrent requests; queue-full behavior; CPU E2E; hardware-gated CUDA/Vulkan.

- [ ] **Step 3: Audit scope and security**

Search implementation for SQLite/RAG/download/release/UI additions and remove them. Confirm no Tauri command accepts an arbitrary DLL path. Confirm Metal appears only as future ABI-extensibility documentation and `GGML_METAL OFF`.

- [ ] **Step 4: Run fresh full verification**

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run build
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-cpu/Debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
git diff --check
git status --short --branch
```

Expected: frontend and CPU native builds pass; all registered CTest and locked Rust tests pass; formatting/Clippy/diff checks are clean; status lists only intentional implementation changes/commits.

- [ ] **Step 5: Run plan-quality scans against the implementation**

```powershell
$redFlags = @('T' + 'BD', 'T' + 'ODO', 'implement later', 'similar to', 'place' + 'holder') -join '|'
rg -n $redFlags native/llm-runtime crates/llm-runtime crates/llm-runtime-sys scripts docs/native-runtime-validation.md
rg -n "PathBuf.*probe_runtime|probe_runtime.*PathBuf|dll_path" apps/desktop/src-tauri/src/runtime_probe.rs
git diff --stat main...HEAD
```

Expected: the first two searches return no matches; diff contains native runtime/scheduler, Rust ABI wrapper, tests, validation docs, and CI only.

- [ ] **Step 6: Commit final verification-only corrections**

If verification required corrections, stage only those exact files and commit:

```powershell
git add native/llm-runtime crates/llm-runtime-sys crates/llm-runtime scripts docs/native-runtime-validation.md .github/workflows/ci.yml apps/desktop/src-tauri/src/runtime_probe.rs
git commit -m "fix: complete native scheduler contract"
```

If there are no corrections, do not create an empty commit.
