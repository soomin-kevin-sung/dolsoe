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
native/llm-runtime/tests/runtime_backend_test.cpp Required checksum-pinned GGUF CPU end-to-end test
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
  $<$<CONFIG:Debug>:LLW_RUNTIME_TESTING>
)
target_include_directories(local_llm_runtime PUBLIC include PRIVATE src)
target_link_libraries(local_llm_runtime PRIVATE llama ggml)

add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
target_link_libraries(llw_abi_layout_test PRIVATE local_llm_runtime)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test)
set_tests_properties(llw_abi_layout_test PROPERTIES TIMEOUT 30)
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
- Modify: `crates/llm-runtime/src/lib.rs`

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

Add this C++ behavior case immediately after the existing successful create/destroy case. It proves an ABI 1.0 caller prefix remains accepted after the append-only extension:

```cpp
llw_runtime_create_params_t legacy_create{};
legacy_create.struct_size = offsetof(llw_runtime_create_params_t, scheduler);
legacy_create.callbacks.struct_size = sizeof(llw_callback_table_t);
llw_runtime_t* legacy_runtime = nullptr;
reset_error();
CHECK(llw_runtime_create(&legacy_create, &legacy_runtime, &error) == LLW_OK);
CHECK(legacy_runtime != nullptr);
llw_runtime_destroy(legacy_runtime);
```

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
#define LLW_MAX_MODEL_PATH_BYTES 32768u
#define LLW_MAX_DEVICE_INDEX 255u
#define LLW_MAX_PROMPT_BYTES (16u * 1024u * 1024u)
#define LLW_MAX_STOP_SEQUENCES 8u
#define LLW_MAX_STOP_BYTES 256u
#define LLW_MAX_STOP_TOTAL_BYTES 2048u

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

The complete `fake_runtime.cpp` include block after this edit is:

```cpp
#include "llw_runtime.h"

#include <algorithm>
#include <cstddef>
#include <cstring>
#include <new>
```

The fake runtime ignores the appended scheduler fields. The real runtime in Task 7 reads them only when `struct_size >= sizeof(llw_runtime_create_params_t)` and otherwise uses 1 slot, request queue 16, and event queue 1024.

ABI rules to retain in the header comments:

```text
All input byte pointers are borrowed only for the call. llw_request_submit copies the prompt,
stop arrays, stop bytes, and request_user_data value before returning. Event data uses event.flags:
TOKEN is BYTES; LOG is UTF8; QUEUED, MODEL_PROGRESS, METRICS, DONE, CANCELLED, and ERROR are
JSON_UTF8. event and data are valid only during the callback and must be copied before return.
Only the dispatcher thread invokes on_event; callbacks are serialized, may not call llw_* reentrantly,
and must not block indefinitely. Each accepted request emits increasing sequence_number values and
exactly one of DONE, CANCELLED, or ERROR. After that terminal event is copied into the bounded event
queue and sequence cleanup completes, the scheduler erases the request and later
llw_request_cancel calls for that handle deterministically return LLW_ERR_NOT_FOUND.
DONE JSON uses `reason:"stop"` for EOS/configured-stop completion and `reason:"length"` when the
effective per-slot generation budget is exhausted; per-slot length completion is not an ERROR.
The caller must externally exclude llw_runtime_destroy from every other llw_* call and callback;
no thread may retain or use the raw llw_runtime_t pointer once destruction begins. Load, unload,
submit, and cancel are internally serialized while the runtime remains alive. Under this precondition,
model-progress callbacks finish before unload/destroy returns and cannot outlive the runtime.
```

Bounds to document beside fields: slots `1..4`; request queue `1..1024`; event queue `16..65536`; model path `1..32768` UTF-8 bytes with no NUL; backend AUTO/CPU/CUDA/VULKAN; device index `0..255`; context per slot `512..262144`; logical batch `1..8192`; physical batch `1..logical_batch`; threads and batch threads `1..256`; GPU layers `-1..65535`; prompt `1..LLW_MAX_PROMPT_BYTES`; new tokens `1..1048576`; top-k `0..100000`; probabilities `0.0..1.0`; temperature `0.0..10.0`; repeat-last-n `0..262144`; repeat penalty `0.0..10.0`; frequency/presence penalties `-2.0..2.0`; stop count `0..8`; each stop `1..256` bytes; all stops combined `0..2048` bytes.

- [ ] **Step 3: Add exact Rust mirrors and function pointer types**

In `crates/llm-runtime-sys/src/lib.rs`, change `ABI_MINOR` and insert this complete ABI 1.1 constant/type block after the existing ABI 1.0 constants and `DeviceList`. Every pointer translation and default is explicit:

```rust
pub const ABI_MINOR: u32 = 1;
pub const ERR_INVALID_ARGUMENT: i32 = 1;
pub const ERR_INTERNAL: i32 = 2;
pub const ERR_BUSY: i32 = 4;
pub const ERR_QUEUE_FULL: i32 = 5;
pub const ERR_NOT_FOUND: i32 = 6;
pub const ERR_INVALID_STATE: i32 = 7;
pub const ERR_CANCELLED: i32 = 8;
pub const ERR_UNSUPPORTED: i32 = 9;
pub const EVENT_DATA_NONE: u32 = 0;
pub const EVENT_DATA_BYTES: u32 = 1;
pub const EVENT_DATA_UTF8: u32 = 2;
pub const EVENT_DATA_JSON_UTF8: u32 = 3;
pub const EVENT_MODEL_PROGRESS: i32 = 1;
pub const EVENT_QUEUED: i32 = 2;
pub const EVENT_TOKEN: i32 = 3;
pub const EVENT_METRICS: i32 = 4;
pub const EVENT_DONE: i32 = 5;
pub const EVENT_CANCELLED: i32 = 6;
pub const EVENT_ERROR: i32 = 7;
pub const EVENT_LOG: i32 = 8;
pub const REQUEST_STATE_QUEUED: i32 = 1;
pub const REQUEST_STATE_PREPROCESSING: i32 = 2;
pub const REQUEST_STATE_RUNNING: i32 = 3;
pub const REQUEST_STATE_DONE: i32 = 4;
pub const REQUEST_STATE_CANCELLED: i32 = 5;
pub const REQUEST_STATE_ERROR: i32 = 6;
pub const MAX_SLOTS: u32 = 4;
pub const MAX_QUEUE_CAPACITY: u32 = 1024;
pub const MAX_EVENT_QUEUE_CAPACITY: u32 = 65_536;
pub const MAX_MODEL_PATH_BYTES: u32 = 32_768;
pub const MAX_DEVICE_INDEX: u32 = 255;
pub const MAX_PROMPT_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_STOP_SEQUENCES: u32 = 8;
pub const MAX_STOP_BYTES: u32 = 256;
pub const MAX_STOP_TOTAL_BYTES: u32 = 2048;

#[repr(C)]
pub struct Bytes {
    pub struct_size: u32, pub flags: u32, pub data: *const u8, pub len: u64,
    pub reserved: [u64; 8],
}
#[repr(C)]
pub struct Buffer {
    pub struct_size: u32, pub flags: u32, pub data: *mut u8, pub capacity: u64,
    pub len: u64, pub reserved: [u64; 8],
}
#[repr(C)]
pub struct SchedulerConfig {
    pub struct_size: u32, pub flags: u32, pub slot_count: u32,
    pub request_queue_capacity: u32, pub event_queue_capacity: u32,
    pub reserved0: u32, pub reserved: [u64; 8],
}
#[repr(C)]
pub struct ModelLoadParams {
    pub struct_size: u32, pub flags: u32, pub path_utf8: *const u8, pub path_len: u64,
    pub backend: i32, pub device_index: u32, pub context_tokens_per_slot: u32,
    pub logical_batch_tokens: u32, pub physical_batch_tokens: u32, pub n_threads: i32,
    pub n_threads_batch: i32, pub n_gpu_layers: i32, pub use_mmap: u32,
    pub use_mlock: u32, pub check_tensors: u32, pub reserved0: u32,
    pub reserved: [u64; 12],
}
#[repr(C)]
pub struct RequestParams {
    pub struct_size: u32, pub flags: u32, pub model_handle: Handle,
    pub prompt: *const u8, pub prompt_len: u64, pub max_new_tokens: u32, pub seed: u32,
    pub temperature: f32, pub top_k: i32, pub top_p: f32, pub min_p: f32,
    pub repeat_last_n: i32, pub repeat_penalty: f32, pub frequency_penalty: f32,
    pub presence_penalty: f32, pub stop_count: u32, pub reserved0: u32,
    pub stop_sequences: *const Bytes, pub request_user_data: *mut c_void,
    pub reserved: [u64; 12],
}
#[repr(C)]
pub struct SchedulerSnapshot {
    pub struct_size: u32, pub flags: u32, pub slot_count: u32, pub active_count: u32,
    pub queued_count: u32, pub queue_capacity: u32, pub accepted_requests: u64,
    pub terminal_requests: u64, pub reserved: [u64; 8],
}
#[repr(C)]
pub struct Metrics {
    pub struct_size: u32, pub flags: u32, pub prompt_tokens: u64,
    pub generated_tokens: u64, pub decode_calls: u64, pub cancelled_requests: u64,
    pub failed_requests: u64, pub queue_wait_ns: u64, pub decode_ns: u64,
    pub reserved: [u64; 8],
}

macro_rules! zero_default {
    ($type:ty, $value:expr) => {
        impl Default for $type {
            fn default() -> Self { $value }
        }
    };
}
zero_default!(Bytes, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, data: std::ptr::null(), len: 0, reserved: [0; 8] });
zero_default!(Buffer, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, data: std::ptr::null_mut(), capacity: 0, len: 0, reserved: [0; 8] });
zero_default!(SchedulerConfig, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, slot_count: 1, request_queue_capacity: 16, event_queue_capacity: 1024,
    reserved0: 0, reserved: [0; 8] });
zero_default!(ModelLoadParams, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, path_utf8: std::ptr::null(), path_len: 0, backend: BACKEND_AUTO,
    device_index: 0, context_tokens_per_slot: 4096, logical_batch_tokens: 512,
    physical_batch_tokens: 128, n_threads: 8, n_threads_batch: 8, n_gpu_layers: 0,
    use_mmap: 1, use_mlock: 0, check_tensors: 0, reserved0: 0, reserved: [0; 12] });
zero_default!(RequestParams, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, model_handle: 0, prompt: std::ptr::null(), prompt_len: 0,
    max_new_tokens: 256, seed: u32::MAX, temperature: 0.8, top_k: 40, top_p: 0.95,
    min_p: 0.05, repeat_last_n: 64, repeat_penalty: 1.1, frequency_penalty: 0.0,
    presence_penalty: 0.0, stop_count: 0, reserved0: 0,
    stop_sequences: std::ptr::null(), request_user_data: std::ptr::null_mut(),
    reserved: [0; 12] });
zero_default!(SchedulerSnapshot, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, slot_count: 0, active_count: 0, queued_count: 0, queue_capacity: 0,
    accepted_requests: 0, terminal_requests: 0, reserved: [0; 8] });
zero_default!(Metrics, Self { struct_size: std::mem::size_of::<Self>() as u32,
    flags: 0, prompt_tokens: 0, generated_tokens: 0, decode_calls: 0,
    cancelled_requests: 0, failed_requests: 0, queue_wait_ns: 0, decode_ns: 0,
    reserved: [0; 8] });

pub type RuntimeGetOptionSchemaFn =
    unsafe extern "C" fn(*mut Runtime, *mut Buffer, *mut Error) -> i32;
pub type ModelLoadFn = unsafe extern "C" fn(
    *mut Runtime, *const ModelLoadParams, *mut Handle, *mut Error) -> i32;
pub type ModelUnloadFn =
    unsafe extern "C" fn(*mut Runtime, Handle, *mut Error) -> i32;
pub type RequestSubmitFn = unsafe extern "C" fn(
    *mut Runtime, *const RequestParams, *mut Handle, *mut Error) -> i32;
pub type RequestCancelFn =
    unsafe extern "C" fn(*mut Runtime, Handle, *mut Error) -> i32;
pub type GetSchedulerSnapshotFn =
    unsafe extern "C" fn(*mut Runtime, *mut SchedulerSnapshot, *mut Error) -> i32;
pub type GetMetricsFn =
    unsafe extern "C" fn(*mut Runtime, *mut Metrics, *mut Error) -> i32;
```

Replace `RuntimeCreateParams` and its `Default` with this append-only mirror:

```rust
#[repr(C)]
pub struct RuntimeCreateParams {
    pub struct_size: u32,
    pub flags: u32,
    pub callbacks: CallbackTable,
    pub reserved: [u64; 8],
    pub scheduler: SchedulerConfig,
    pub reserved_v1: [u64; 8],
}

impl Default for RuntimeCreateParams {
    fn default() -> Self {
        Self { struct_size: std::mem::size_of::<Self>() as u32, flags: 0,
            callbacks: CallbackTable::default(), reserved: [0; 8],
            scheduler: SchedulerConfig::default(), reserved_v1: [0; 8] }
    }
}
```

Do not add these seven aliases to `Api` in this task; Task 8 performs the loader change after the DLL exports exist.

In the existing `RuntimeLibrary::load` initializer in `crates/llm-runtime/src/lib.rs`, add the append-only fields so Task 2 remains compilable before the new safe API is introduced:

```rust
let create = sys::RuntimeCreateParams {
    struct_size: std::mem::size_of::<sys::RuntimeCreateParams>() as u32,
    flags: 0,
    callbacks: sys::CallbackTable::default(),
    reserved: [0; 8],
    scheduler: sys::SchedulerConfig::default(),
    reserved_v1: [0; 8],
};
```

- [ ] **Step 4: Run layout tests**

```powershell
cargo test -p llm-runtime-sys ffi_struct_layouts_match_x64_c_contract
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug -R llw_abi_layout_test --output-on-failure
```

Expected: Rust and C++ layout tests pass. Existing ABI 1.0 offsets remain unchanged, and the scheduler field begins at byte 160.

- [ ] **Step 5: Commit the ABI contract**

```powershell
git add native/llm-runtime/include/llw_runtime.h native/llm-runtime/src/fake_runtime.cpp native/llm-runtime/tests/abi_layout_test.cpp crates/llm-runtime-sys/src/lib.rs crates/llm-runtime/src/lib.rs
git commit -m "feat: define scheduler ABI contract"
```

### Task 3: Implement The Bounded Event Dispatcher

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Create: `native/llm-runtime/src/event_dispatcher.h`
- Create: `native/llm-runtime/src/event_dispatcher.cpp`
- Test: `native/llm-runtime/tests/scheduler_test.cpp`

- [ ] **Step 1: Write a failing dispatcher ownership test**

Create `native/llm-runtime/tests/scheduler_test.cpp`:

```cpp
#include "event_dispatcher.h"
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

struct Received {
    uint64_t sequence{};
    std::vector<uint8_t> data;
    std::thread::id thread;
};
struct Collector {
    std::mutex mutex;
    std::condition_variable changed;
    std::vector<Received> events;
    bool block{};
    bool entered{};
    bool release{};
};

void LLW_CALL collect(const llw_event_t* event, void* user_data) {
    auto& collector = *static_cast<Collector*>(user_data);
    Received received;
    received.sequence = event->sequence_number;
    received.thread = std::this_thread::get_id();
    if (event->data && event->data_len)
        received.data.assign(event->data, event->data + event->data_len);
    std::unique_lock lock(collector.mutex);
    collector.events.push_back(std::move(received));
    if (collector.block) {
        collector.entered = true;
        collector.changed.notify_all();
        collector.changed.wait(lock, [&collector] { return collector.release; });
    }
}

int main() {
    const std::thread::id test_thread = std::this_thread::get_id();
    Collector collector;
    llw_callback_table_t callbacks{};
    callbacks.struct_size = sizeof(callbacks);
    callbacks.on_event = collect;
    callbacks.user_data = &collector;
    EventDispatcher dispatcher(callbacks, 16);
    std::vector<uint8_t> source = {0xf0, 0x9f, 0x92, 0xa1};
    OwnedEvent first;
    first.type = LLW_EVENT_TOKEN;
    first.data_format = LLW_EVENT_DATA_BYTES;
    first.sequence = 41;
    first.data = source;
    if (!dispatcher.publish(std::move(first))) return 1;
    source.assign(4, 0);
    OwnedEvent second;
    second.type = LLW_EVENT_DONE;
    second.data_format = LLW_EVENT_DATA_JSON_UTF8;
    second.sequence = 42;
    second.data = {'{', '}'};
    if (!dispatcher.publish(std::move(second))) return 1;
    dispatcher.drain_for_test();
    {
        std::lock_guard lock(collector.mutex);
        if (collector.events.size() != 2) return 1;
        if (collector.events[0].sequence != 41 || collector.events[1].sequence != 42) return 1;
        if (collector.events[0].data != std::vector<uint8_t>({0xf0, 0x9f, 0x92, 0xa1})) return 1;
        if (collector.events[0].thread == test_thread ||
            collector.events[1].thread != collector.events[0].thread) return 1;
        collector.block = true;
    }
    OwnedEvent slow;
    slow.type = LLW_EVENT_LOG;
    slow.data_format = LLW_EVENT_DATA_UTF8;
    slow.data = {'s', 'l', 'o', 'w'};
    if (!dispatcher.publish(std::move(slow))) return 1;
    {
        std::unique_lock lock(collector.mutex);
        if (!collector.changed.wait_for(lock, std::chrono::seconds(5),
                                        [&collector] { return collector.entered; })) return 1;
    }
    std::atomic<bool> flushed{false};
    std::thread flusher([&] { dispatcher.flush(); flushed.store(true); });
    std::this_thread::sleep_for(std::chrono::milliseconds(50));
    const bool returned_early = flushed.load();
    {
        std::lock_guard lock(collector.mutex);
        collector.release = true;
    }
    collector.changed.notify_all();
    flusher.join();
    if (returned_early || !flushed.load()) return 1;
    return 0;
}
```

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
#include <cstddef>
#include <cstdint>
#include <deque>
#include <future>
#include <memory>
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
    uint64_t sequence{}; // Assigned by Scheduler::publish_locked for request events.
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
    void flush();
    void stop();
    void drain_for_test();
private:
    struct DispatchItem {
        OwnedEvent event;
        std::shared_ptr<std::promise<void>> barrier;
    };
    void run();
    llw_callback_table_t callbacks_{};
    const size_t capacity_;
    std::mutex mutex_;
    std::condition_variable readable_;
    std::condition_variable writable_;
    std::condition_variable drained_;
    std::deque<DispatchItem> queue_;
    bool stopping_{};
    size_t in_callback_{};
    std::thread::id callback_thread_{};
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
set_tests_properties(llw_scheduler_test PROPERTIES TIMEOUT 30)
```

- [ ] **Step 3: Implement payload ownership and serialized callbacks**

Create `native/llm-runtime/src/event_dispatcher.cpp`:

```cpp
#include "event_dispatcher.h"
#include <chrono>
#include <stdexcept>
#include <utility>

EventDispatcher::EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity)
    : callbacks_(callbacks), capacity_(capacity), thread_([this] { run(); }) {}

EventDispatcher::~EventDispatcher() { stop(); }

bool EventDispatcher::publish(OwnedEvent event) {
    std::unique_lock lock(mutex_);
    writable_.wait(lock, [this] { return stopping_ || queue_.size() < capacity_; });
    if (stopping_) return false;
    queue_.push_back(DispatchItem{std::move(event), {}});
    readable_.notify_one();
    return true;
}

void EventDispatcher::flush() {
    auto barrier = std::make_shared<std::promise<void>>();
    std::future<void> completed = barrier->get_future();
    {
        std::unique_lock lock(mutex_);
        if (std::this_thread::get_id() == callback_thread_)
            throw std::logic_error("event dispatcher flush is not callback-reentrant");
        writable_.wait(lock, [this] { return stopping_ || queue_.size() < capacity_; });
        if (stopping_) throw std::runtime_error("event dispatcher is stopping");
        queue_.push_back(DispatchItem{{}, std::move(barrier)});
        readable_.notify_one();
    }
    completed.get();
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
    if (!drained_.wait_for(lock, std::chrono::seconds(5),
                           [this] { return queue_.empty() && in_callback_ == 0; }))
        throw std::runtime_error("event dispatcher drain timeout");
}

void EventDispatcher::run() {
    {
        std::lock_guard lock(mutex_);
        callback_thread_ = std::this_thread::get_id();
    }
    for (;;) {
        DispatchItem item;
        {
            std::unique_lock lock(mutex_);
            readable_.wait(lock, [this] { return stopping_ || !queue_.empty(); });
            if (queue_.empty() && stopping_) break;
            item = std::move(queue_.front());
            queue_.pop_front();
            if (!item.barrier) ++in_callback_;
            writable_.notify_one();
        }
        if (item.barrier) {
            item.barrier->set_value();
            continue;
        }
        OwnedEvent& owned = item.event;
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

`flush()` is a production FIFO barrier: completion means every event published before the barrier
has returned from `on_event`. Runtime lifecycle code calls it only after scheduler and engine
shutdown. Calling it from `on_event` violates the ABI callback non-reentrancy rule; the explicit
callback-thread guard throws instead of deadlocking.

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

Replace `native/llm-runtime/tests/scheduler_test.cpp` with this complete test file:

```cpp
#include "event_dispatcher.h"
#include "fake_engine.h"
#include "scheduler.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <map>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

struct SeenEvent { int32_t type{}; llw_handle_t request{}; uint32_t slot{}; uint64_t sequence{}; };
struct Collector {
    std::mutex mutex;
    std::condition_variable changed;
    std::vector<SeenEvent> events;
};

void LLW_CALL collect_scheduler_event(const llw_event_t* event, void* user_data) {
    auto& collector = *static_cast<Collector*>(user_data);
    {
        std::lock_guard lock(collector.mutex);
        collector.events.push_back(SeenEvent{event->event_type, event->request_handle,
                                              event->slot_id, event->sequence_number});
    }
    collector.changed.notify_all();
}

llw_callback_table_t callbacks(Collector& collector) {
    llw_callback_table_t result{};
    result.struct_size = sizeof(result);
    result.on_event = collect_scheduler_event;
    result.user_data = &collector;
    return result;
}

llw_request_params_t request_params(const std::string& prompt) {
    llw_request_params_t result{};
    result.struct_size = sizeof(result);
    result.model_handle = 1;
    result.prompt = reinterpret_cast<const uint8_t*>(prompt.data());
    result.prompt_len = prompt.size();
    result.max_new_tokens = 3;
    result.seed = 7;
    result.temperature = 0;
    result.top_k = 40;
    result.top_p = 0.95f;
    result.min_p = 0.05f;
    result.repeat_last_n = 64;
    result.repeat_penalty = 1.1f;
    return result;
}

void wait_for_terminals(Collector& collector, size_t count) {
    std::unique_lock lock(collector.mutex);
    const bool ready = collector.changed.wait_for(lock, std::chrono::seconds(5), [&] {
        return std::count_if(collector.events.begin(), collector.events.end(), [](const SeenEvent& event) {
            return event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_CANCELLED ||
                   event.type == LLW_EVENT_ERROR;
        }) >= static_cast<std::ptrdiff_t>(count);
    });
    if (!ready) throw std::runtime_error("terminal event timeout");
}

void assert_sequences(const Collector& collector, llw_handle_t request) {
    uint64_t expected = 1;
    size_t terminals = 0;
    for (const SeenEvent& event : collector.events) {
        if (event.request != request) continue;
        if (event.sequence != expected++) throw std::runtime_error("non-monotonic request sequence");
        if (event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_CANCELLED ||
            event.type == LLW_EVENT_ERROR) ++terminals;
    }
    if (terminals != 1) throw std::runtime_error("request did not have exactly one terminal");
}

void concurrent_requests_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(2, 4, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    llw_handle_t first{};
    llw_handle_t second{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_batch_size(2);
    engine.release();
    wait_for_terminals(collector, 2);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, first);
    assert_sequences(collector, second);
    if (engine.cleanup_count(first) != 1 || engine.cleanup_count(second) != 1)
        throw std::runtime_error("completed sequences were not cleaned exactly once");
    std::map<llw_handle_t, uint32_t> slots;
    for (const SeenEvent& event : collector.events)
        if (event.request != 0 && event.slot != UINT32_MAX) slots[event.request] = event.slot;
    if (slots[first] == slots[second]) throw std::runtime_error("requests shared a slot");
}

void queue_full_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(1, 1, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    const std::string third_prompt = "third";
    llw_handle_t first{}, second{}, third{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_started(1);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.submit(request_params(third_prompt), third, error) != LLW_ERR_QUEUE_FULL || third != 0)
        throw std::runtime_error("queue-full contract failed");
    engine.release();
    wait_for_terminals(collector, 2);
}

void per_slot_failure_isolation_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    const std::string oversized_prompt = "oversized";
    const std::string healthy_prompt = "healthy";
    engine.reject_prompt(std::vector<uint8_t>(oversized_prompt.begin(), oversized_prompt.end()));
    Scheduler scheduler(2, 2, engine, dispatcher);
    llw_handle_t oversized{}, healthy{};
    std::string error;
    if (scheduler.submit(request_params(oversized_prompt), oversized, error) != LLW_OK)
        throw std::runtime_error(error);
    if (scheduler.submit(request_params(healthy_prompt), healthy, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_started(1);
    engine.release();
    wait_for_terminals(collector, 2);
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, oversized);
    assert_sequences(collector, healthy);
    const auto terminal_type = [&collector](llw_handle_t handle) {
        for (const SeenEvent& event : collector.events) {
            if (event.request == handle && (event.type == LLW_EVENT_DONE ||
                event.type == LLW_EVENT_CANCELLED || event.type == LLW_EVENT_ERROR))
                return event.type;
        }
        return int32_t{0};
    };
    if (terminal_type(oversized) != LLW_EVENT_ERROR || terminal_type(healthy) != LLW_EVENT_DONE)
        throw std::runtime_error("per-slot failure affected a healthy peer");
    if (engine.cleanup_count(oversized) != 0 || engine.cleanup_count(healthy) != 1)
        throw std::runtime_error("per-slot cleanup counts are incorrect");
}

void cancellation_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    Scheduler scheduler(1, 2, engine, dispatcher);
    const std::string active_prompt = "active";
    const std::string queued_prompt = "queued";
    llw_handle_t active{}, queued{};
    std::string error;
    if (scheduler.submit(request_params(active_prompt), active, error) != LLW_OK) throw std::runtime_error(error);
    engine.wait_for_started(1);
    if (scheduler.submit(request_params(queued_prompt), queued, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.cancel(queued, error) != LLW_OK) throw std::runtime_error(error);
    if (scheduler.cancel(active, error) != LLW_OK) throw std::runtime_error(error);
    engine.release();
    wait_for_terminals(collector, 2);
    if (scheduler.cancel(active, error) != LLW_ERR_NOT_FOUND ||
        scheduler.cancel(queued, error) != LLW_ERR_NOT_FOUND)
        throw std::runtime_error("erased terminal handles must return not-found");
    dispatcher.drain_for_test();
    std::lock_guard lock(collector.mutex);
    assert_sequences(collector, active);
    assert_sequences(collector, queued);
    if (engine.cleanup_count(active) != 1 || engine.cleanup_count(queued) != 0)
        throw std::runtime_error("active and queued cancellation cleanup counts differ");
    if (scheduler.tracked_request_count_for_test() != 0)
        throw std::runtime_error("cancelled requests remained tracked");
}

void decode_failure_cleanup_precedes_slot_reuse_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    engine.set_decode_failure(true);
    Scheduler scheduler(2, 2, engine, dispatcher);
    const std::string first_prompt = "first";
    const std::string second_prompt = "second";
    const std::string reuse_prompt = "reuse";
    llw_handle_t first{}, second{}, reuse{};
    std::string error;
    if (scheduler.submit(request_params(first_prompt), first, error) != LLW_OK)
        throw std::runtime_error(error);
    if (scheduler.submit(request_params(second_prompt), second, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_batch_size(2);
    engine.release();
    wait_for_terminals(collector, 2);
    if (engine.cleanup_count(first) != 1 || engine.cleanup_count(second) != 1 ||
        scheduler.tracked_request_count_for_test() != 0)
        throw std::runtime_error("failed shared decode did not clean and erase every request");
    engine.set_decode_failure(false);
    if (scheduler.submit(request_params(reuse_prompt), reuse, error) != LLW_OK)
        throw std::runtime_error(error);
    wait_for_terminals(collector, 3);
    const auto operations = engine.operation_log();
    const auto first_cleanup = std::find(operations.begin(), operations.end(),
                                         "cleanup:" + std::to_string(first));
    const auto second_cleanup = std::find(operations.begin(), operations.end(),
                                          "cleanup:" + std::to_string(second));
    const auto reused = std::find(operations.begin(), operations.end(),
                                  "start:" + std::to_string(reuse));
    if (first_cleanup == operations.end() || second_cleanup == operations.end() ||
        reused == operations.end() || first_cleanup > reused || second_cleanup > reused)
        throw std::runtime_error("slot was reused before all failed-sequence cleanup");
    if (engine.cleanup_count(reuse) != 1)
        throw std::runtime_error("reused slot completion was not cleaned exactly once");
}

void bounded_terminal_storage_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 256);
    FakeEngine engine;
    Scheduler scheduler(1, 2, engine, dispatcher);
    engine.release();
    std::string error;
    for (size_t index = 0; index < 100; ++index) {
        const std::string prompt = "request-" + std::to_string(index);
        llw_handle_t handle{};
        if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
            throw std::runtime_error(error);
        wait_for_terminals(collector, index + 1);
        if (scheduler.tracked_request_count_for_test() != 0 || engine.cleanup_count(handle) != 1)
            throw std::runtime_error("terminal request storage was retained");
        if (scheduler.cancel(handle, error) != LLW_ERR_NOT_FOUND)
            throw std::runtime_error("terminal request cancel must return not-found");
    }
}

void deterministic_metrics_test() {
    Collector collector;
    EventDispatcher dispatcher(callbacks(collector), 64);
    FakeEngine engine;
    std::atomic<uint64_t> ticks{0};
    Scheduler scheduler(1, 2, engine, dispatcher, [&ticks] {
        return Scheduler::TimePoint(std::chrono::nanoseconds(ticks.fetch_add(10)));
    });
    const std::string prompt = "four";
    llw_handle_t handle{};
    std::string error;
    if (scheduler.submit(request_params(prompt), handle, error) != LLW_OK)
        throw std::runtime_error(error);
    engine.wait_for_started(1);
    engine.release();
    wait_for_terminals(collector, 1);
    const llw_metrics_t metrics = scheduler.metrics();
    if (metrics.prompt_tokens != prompt.size() || metrics.queue_wait_ns != 10)
        throw std::runtime_error("prompt-token or queue-wait metrics are not deterministic");
}

int main() {
    try {
        concurrent_requests_test();
        queue_full_test();
        per_slot_failure_isolation_test();
        cancellation_test();
        decode_failure_cleanup_precedes_slot_reuse_test();
        bounded_terminal_storage_test();
        deterministic_metrics_test();
        return 0;
    } catch (const std::exception& exception) {
        std::fprintf(stderr, "%s\n", exception.what());
        return 1;
    }
}
```

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
    llw_handle_t handle{}; std::vector<uint8_t> token_bytes; bool finished{}; bool failed{};
    std::string error; std::string finish_reason;
};
class InferenceEngine {
public:
    virtual ~InferenceEngine() = default;
    virtual uint64_t start(EngineRequest request) = 0;
    virtual std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) = 0;
    virtual void cleanup(llw_handle_t handle, uint32_t seq_id) = 0;
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
set_tests_properties(llw_scheduler_test PROPERTIES TIMEOUT 30)
```

- [ ] **Step 3: Define scheduler ownership and states**

Create `native/llm-runtime/src/scheduler.h` with `RequestState {Queued, Preprocessing, Running, Done, Cancelled, Error}`, an owned `Request` containing copied prompt/stops/user-data, cancellation state, and `uint64_t next_sequence{1}` as the only per-request event sequence owner. Fixed `Slot` records use `seq_id == slot index`. Use this complete header:

```cpp
#pragma once
#include "event_dispatcher.h"
#include "inference_engine.h"
#include "llw_runtime.h"
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

enum class RequestState { Queued, Preprocessing, Running, Done, Cancelled, Error };

class Scheduler {
public:
    using TimePoint = std::chrono::steady_clock::time_point;
    using NowFn = std::function<TimePoint()>;
    Scheduler(uint32_t slots, uint32_t queue_capacity, InferenceEngine& engine,
              EventDispatcher& events,
              NowFn now = [] { return std::chrono::steady_clock::now(); });
    ~Scheduler();
    llw_result_t submit(const llw_request_params_t& params, llw_handle_t& out, std::string& error);
    llw_result_t cancel(llw_handle_t handle, std::string& error);
    llw_scheduler_snapshot_t snapshot() const;
    llw_metrics_t metrics() const;
    size_t tracked_request_count_for_test() const;
    void cancel_all_and_wait();
private:
    struct Request {
        llw_handle_t handle{};
        llw_handle_t model{};
        RequestState state{RequestState::Queued};
        std::vector<uint8_t> prompt;
        std::vector<std::vector<uint8_t>> stops;
        SamplingConfig sampling{};
        uint32_t max_new_tokens{};
        uint32_t generated_tokens{};
        uint64_t prompt_tokens{};
        uint32_t slot_id{UINT32_MAX};
        void* user_data{};
        bool cancel_requested{};
        bool terminal_emitted{};
        bool engine_started{};
        bool cleanup_attempted{};
        uint64_t next_sequence{1};
        TimePoint enqueued_at{};
        TimePoint started_at{};
    };
    struct Slot { uint32_t id{}; llw_handle_t request{}; };
    void run();
    void promote_locked();
    void finish_locked(llw_handle_t, RequestState, int32_t, std::string);
    void publish_locked(Request&, int32_t, uint32_t, int32_t, std::vector<uint8_t>, uint32_t);
    bool has_work_locked() const;
    uint32_t slots_count_{};
    uint32_t queue_capacity_{};
    InferenceEngine& engine_;
    EventDispatcher& events_;
    NowFn now_;
    mutable std::mutex mutex_;
    std::condition_variable wake_;
    std::condition_variable idle_;
    std::deque<llw_handle_t> queued_;
    std::map<llw_handle_t, Request> requests_;
    std::vector<Slot> slots_;
    llw_handle_t next_handle_{1};
    bool stopping_{};
    std::thread worker_;
    llw_metrics_t metrics_{};
    uint64_t accepted_{};
    uint64_t terminal_{};
};
```

- [ ] **Step 4: Implement deterministic queueing, sequence ownership, and cancellation**

Create `native/llm-runtime/src/scheduler.cpp` with this complete implementation. `Request::next_sequence` is initialized to 1 in the header and incremented only in `publish_locked`; the dispatcher receives the assigned value unchanged.

```cpp
#include "scheduler.h"
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <sstream>
#include <stdexcept>
#include <utility>

namespace {
std::vector<uint8_t> bytes(std::string value) {
    return {value.begin(), value.end()};
}

std::string json_escape(const std::string& value) {
    std::ostringstream out;
    for (const unsigned char ch : value) {
        switch (ch) {
        case '"': out << "\\\""; break;
        case '\\': out << "\\\\"; break;
        case '\b': out << "\\b"; break;
        case '\f': out << "\\f"; break;
        case '\n': out << "\\n"; break;
        case '\r': out << "\\r"; break;
        case '\t': out << "\\t"; break;
        default:
            if (ch < 0x20) {
                char escaped[7]{};
                std::snprintf(escaped, sizeof(escaped), "\\u%04x", ch);
                out << escaped;
            } else {
                out << static_cast<char>(ch);
            }
        }
    }
    return out.str();
}

bool terminal(RequestState state) {
    return state == RequestState::Done || state == RequestState::Cancelled ||
           state == RequestState::Error;
}
} // namespace

Scheduler::Scheduler(uint32_t slots, uint32_t queue_capacity, InferenceEngine& engine,
                     EventDispatcher& events, NowFn now)
    : slots_count_(slots), queue_capacity_(queue_capacity), engine_(engine), events_(events),
      now_(std::move(now)) {
    if (slots < 1 || slots > LLW_MAX_SLOTS || queue_capacity < 1 ||
        queue_capacity > LLW_MAX_QUEUE_CAPACITY) {
        throw std::invalid_argument("invalid scheduler bounds");
    }
    metrics_ = {};
    metrics_.struct_size = sizeof(metrics_);
    for (uint32_t index = 0; index < slots; ++index) slots_.push_back(Slot{index, 0});
    worker_ = std::thread([this] { run(); });
}

Scheduler::~Scheduler() {
    cancel_all_and_wait();
    {
        std::lock_guard lock(mutex_);
        stopping_ = true;
    }
    wake_.notify_all();
    if (worker_.joinable()) worker_.join();
}

llw_result_t Scheduler::submit(const llw_request_params_t& params, llw_handle_t& out,
                               std::string& error) {
    out = 0;
    Request request;
    request.model = params.model_handle;
    request.prompt.assign(params.prompt, params.prompt + params.prompt_len);
    request.max_new_tokens = params.max_new_tokens;
    request.sampling = SamplingConfig{params.seed, params.temperature, params.top_k, params.top_p,
        params.min_p, params.repeat_last_n, params.repeat_penalty, params.frequency_penalty,
        params.presence_penalty};
    request.user_data = params.request_user_data;
    request.enqueued_at = now_();
    request.stops.reserve(params.stop_count);
    for (uint32_t index = 0; index < params.stop_count; ++index) {
        const llw_bytes_t& stop = params.stop_sequences[index];
        request.stops.emplace_back(stop.data, stop.data + stop.len);
    }

    std::lock_guard lock(mutex_);
    if (queued_.size() >= queue_capacity_) {
        error = "request queue is full";
        return LLW_ERR_QUEUE_FULL;
    }
    request.handle = next_handle_++;
    if (request.handle == 0) request.handle = next_handle_++;
    const llw_handle_t handle = request.handle;
    auto [it, inserted] = requests_.emplace(handle, std::move(request));
    if (!inserted) throw std::runtime_error("request handle collision");
    queued_.push_back(handle);
    ++accepted_;
    out = handle;
    const auto payload = bytes("{\"state\":\"queued\",\"queuePosition\":" +
                               std::to_string(queued_.size()) + "}");
    publish_locked(it->second, LLW_EVENT_QUEUED, UINT32_MAX, 0, payload,
                   LLW_EVENT_DATA_JSON_UTF8);
    wake_.notify_one();
    return LLW_OK;
}

llw_result_t Scheduler::cancel(llw_handle_t handle, std::string& error) {
    std::lock_guard lock(mutex_);
    const auto found = requests_.find(handle);
    if (found == requests_.end()) {
        error = "request handle was not found";
        return LLW_ERR_NOT_FOUND;
    }
    Request& request = found->second;
    if (terminal(request.state) || request.cancel_requested) return LLW_OK;
    request.cancel_requested = true;
    if (request.state == RequestState::Queued) {
        queued_.erase(std::remove(queued_.begin(), queued_.end(), handle), queued_.end());
        finish_locked(handle, RequestState::Cancelled, 0, "");
    }
    wake_.notify_one();
    return LLW_OK;
}

llw_scheduler_snapshot_t Scheduler::snapshot() const {
    std::lock_guard lock(mutex_);
    llw_scheduler_snapshot_t result{};
    result.struct_size = sizeof(result);
    result.slot_count = slots_count_;
    result.queue_capacity = queue_capacity_;
    result.queued_count = static_cast<uint32_t>(queued_.size());
    result.active_count = static_cast<uint32_t>(std::count_if(
        slots_.begin(), slots_.end(), [](const Slot& slot) { return slot.request != 0; }));
    result.accepted_requests = accepted_;
    result.terminal_requests = terminal_;
    return result;
}

llw_metrics_t Scheduler::metrics() const {
    std::lock_guard lock(mutex_);
    return metrics_;
}

size_t Scheduler::tracked_request_count_for_test() const {
    std::lock_guard lock(mutex_);
    return requests_.size();
}

void Scheduler::cancel_all_and_wait() {
    std::unique_lock lock(mutex_);
    for (auto& [handle, request] : requests_) {
        (void)handle;
        if (!terminal(request.state)) request.cancel_requested = true;
    }
    while (!queued_.empty()) {
        const llw_handle_t handle = queued_.front();
        queued_.pop_front();
        finish_locked(handle, RequestState::Cancelled, 0, "");
    }
    wake_.notify_one();
    idle_.wait(lock, [this] { return terminal_ == accepted_; });
}

bool Scheduler::has_work_locked() const {
    if (!queued_.empty()) return true;
    return std::any_of(slots_.begin(), slots_.end(), [this](const Slot& slot) {
        if (slot.request == 0) return false;
        const auto found = requests_.find(slot.request);
        return found != requests_.end() && !terminal(found->second.state);
    });
}

void Scheduler::promote_locked() {
    for (Slot& slot : slots_) {
        if (slot.request != 0 || queued_.empty()) continue;
        const llw_handle_t handle = queued_.front();
        queued_.pop_front();
        Request& request = requests_.at(handle);
        if (request.cancel_requested) {
            finish_locked(handle, RequestState::Cancelled, 0, "");
            continue;
        }
        request.slot_id = slot.id;
        request.state = RequestState::Preprocessing;
        slot.request = handle;
        try {
            request.started_at = now_();
            metrics_.queue_wait_ns += static_cast<uint64_t>(
                std::chrono::duration_cast<std::chrono::nanoseconds>(
                    request.started_at - request.enqueued_at).count());
            request.prompt_tokens = engine_.start(EngineRequest{request.handle, slot.id,
                request.prompt, request.max_new_tokens, request.sampling, request.stops});
            request.engine_started = true;
            metrics_.prompt_tokens += request.prompt_tokens;
            request.state = RequestState::Running;
        } catch (const std::exception& exception) {
            finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL, exception.what());
        } catch (...) {
            finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL,
                          "unknown preprocessing failure");
        }
    }
}

void Scheduler::publish_locked(Request& request, int32_t type, uint32_t slot,
                               int32_t error_code, std::vector<uint8_t> payload,
                               uint32_t data_format) {
    OwnedEvent event;
    event.type = type;
    event.data_format = data_format;
    event.error_code = error_code;
    event.model = request.model;
    event.request = request.handle;
    event.slot = slot;
    event.sequence = request.next_sequence++;
    event.request_user_data = request.user_data;
    event.data = std::move(payload);
    if (!events_.publish(std::move(event))) throw std::runtime_error("event dispatcher stopped");
}

void Scheduler::finish_locked(llw_handle_t handle, RequestState state, int32_t error_code,
                               std::string message) {
    Request& request = requests_.at(handle);
    if (request.terminal_emitted) return;
    request.terminal_emitted = true;
    request.state = state;
    if (request.engine_started && !request.cleanup_attempted) {
        request.cleanup_attempted = true;
        try {
            engine_.cleanup(request.handle, request.slot_id);
        } catch (const std::exception& exception) {
            state = RequestState::Error;
            error_code = LLW_ERR_INTERNAL;
            message = std::string("sequence cleanup failed: ") + exception.what();
            request.state = state;
        } catch (...) {
            state = RequestState::Error;
            error_code = LLW_ERR_INTERNAL;
            message = "sequence cleanup failed";
            request.state = state;
        }
    }
    int32_t event_type = LLW_EVENT_DONE;
    const std::string done_reason = message.empty() ? "stop" : message;
    std::string payload = "{\"state\":\"done\",\"reason\":\"" +
                          json_escape(done_reason) + "\",\"generatedTokens\":" +
                          std::to_string(request.generated_tokens) + "}";
    if (state == RequestState::Cancelled) {
        event_type = LLW_EVENT_CANCELLED;
        payload = "{\"state\":\"cancelled\"}";
        ++metrics_.cancelled_requests;
    } else if (state == RequestState::Error) {
        event_type = LLW_EVENT_ERROR;
        payload = "{\"state\":\"error\",\"message\":\"" + json_escape(message) + "\"}";
        ++metrics_.failed_requests;
    }
    publish_locked(request, event_type, request.slot_id, error_code, bytes(payload),
                   LLW_EVENT_DATA_JSON_UTF8);
    for (Slot& slot : slots_) {
        if (slot.request == handle) slot.request = 0;
    }
    ++terminal_;
    requests_.erase(handle);
    if (terminal_ == accepted_) idle_.notify_all();
}

void Scheduler::run() {
    std::unique_lock lock(mutex_);
    for (;;) {
        wake_.wait(lock, [this] { return stopping_ || has_work_locked(); });
        if (stopping_ && !has_work_locked()) break;

        for (Slot& slot : slots_) {
            if (slot.request == 0) continue;
            const llw_handle_t handle = slot.request;
            Request& request = requests_.at(handle);
            if (!request.cancel_requested || terminal(request.state)) continue;
            finish_locked(handle, RequestState::Cancelled, 0, "");
        }
        promote_locked();

        std::vector<llw_handle_t> active;
        for (const Slot& slot : slots_) if (slot.request != 0) active.push_back(slot.request);
        if (active.empty()) continue;

        lock.unlock();
        std::vector<EngineStep> steps;
        std::string decode_error;
        const auto started = std::chrono::steady_clock::now();
        try { steps = engine_.decode(active); }
        catch (const std::exception& exception) { decode_error = exception.what(); }
        catch (...) { decode_error = "unknown decode failure"; }
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - started).count();
        lock.lock();

        ++metrics_.decode_calls;
        metrics_.decode_ns += static_cast<uint64_t>(elapsed);
        if (!decode_error.empty()) {
            for (const llw_handle_t handle : active) {
                const auto found = requests_.find(handle);
                if (found != requests_.end())
                    finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL, decode_error);
            }
            continue;
        }
        for (EngineStep& step : steps) {
            auto found = requests_.find(step.handle);
            if (found == requests_.end() || found->second.terminal_emitted) continue;
            Request& request = found->second;
            if (request.cancel_requested) {
                finish_locked(request.handle, RequestState::Cancelled, 0, "");
                continue;
            }
            if (!step.token_bytes.empty()) {
                ++request.generated_tokens;
                ++metrics_.generated_tokens;
                publish_locked(request, LLW_EVENT_TOKEN, request.slot_id, 0,
                               std::move(step.token_bytes), LLW_EVENT_DATA_BYTES);
            }
            if (step.failed) finish_locked(request.handle, RequestState::Error,
                                           LLW_ERR_INTERNAL, step.error);
            else if (step.finished) finish_locked(request.handle, RequestState::Done, 0,
                                                  step.finish_reason);
        }

        OwnedEvent metrics_event;
        metrics_event.type = LLW_EVENT_METRICS;
        metrics_event.data_format = LLW_EVENT_DATA_JSON_UTF8;
        metrics_event.sequence = 0;
        metrics_event.data = bytes("{\"promptTokens\":" + std::to_string(metrics_.prompt_tokens) +
            ",\"generatedTokens\":" + std::to_string(metrics_.generated_tokens) +
            ",\"decodeCalls\":" + std::to_string(metrics_.decode_calls) +
            ",\"queueWaitNanoseconds\":" + std::to_string(metrics_.queue_wait_ns) +
            ",\"decodeNanoseconds\":" + std::to_string(metrics_.decode_ns) + "}");
        if (!events_.publish(std::move(metrics_event))) stopping_ = true;
    }
}
```

- [ ] **Step 5: Create the deterministic fake engine**

Create `native/llm-runtime/tests/fake_engine.h`:

```cpp
#pragma once
#include "inference_engine.h"
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

class FakeEngine final : public InferenceEngine {
public:
    uint64_t start(EngineRequest request) override {
        std::lock_guard lock(mutex_);
        if (request.prompt == rejected_prompt_)
            throw std::invalid_argument("prompt leaves no generation token in its slot context");
        const uint64_t prompt_tokens = request.prompt.size();
        operation_log_.push_back("start:" + std::to_string(request.handle));
        requests_.emplace(request.handle, Stored{std::move(request), 0});
        changed_.notify_all();
        return prompt_tokens;
    }

    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override {
        std::unique_lock lock(mutex_);
        batches_.push_back(active);
        changed_.notify_all();
        if (!gate_.wait_for(lock, std::chrono::seconds(5), [this] { return released_; }))
            throw std::runtime_error("fake engine release timeout");
        if (decode_failure_) throw std::runtime_error("injected decode failure");
        std::vector<EngineStep> result;
        for (const llw_handle_t handle : active) {
            auto found = requests_.find(handle);
            if (found == requests_.end()) continue;
            Stored& stored = found->second;
            ++stored.steps;
            EngineStep step;
            step.handle = handle;
            step.token_bytes = {static_cast<uint8_t>('A' + stored.request.seq_id)};
            step.finished = stored.steps == 3;
            result.push_back(std::move(step));
        }
        return result;
    }

    void cleanup(llw_handle_t handle, uint32_t seq_id) override {
        std::lock_guard lock(mutex_);
        const auto found = requests_.find(handle);
        if (found == requests_.end() || found->second.request.seq_id != seq_id)
            throw std::runtime_error("cleanup called for unknown sequence");
        cleanup_calls_[handle] += 1;
        operation_log_.push_back("cleanup:" + std::to_string(handle));
        requests_.erase(handle);
        changed_.notify_all();
    }

    void set_decode_failure(bool value) {
        std::lock_guard lock(mutex_);
        decode_failure_ = value;
    }

    void reject_prompt(std::vector<uint8_t> prompt) {
        std::lock_guard lock(mutex_);
        rejected_prompt_ = std::move(prompt);
    }

    void release() {
        std::lock_guard lock(mutex_);
        released_ = true;
        gate_.notify_all();
    }

    void wait_for_started(size_t count) {
        std::unique_lock lock(mutex_);
        if (!changed_.wait_for(lock, std::chrono::seconds(5),
                               [this, count] { return requests_.size() >= count; }))
            throw std::runtime_error("fake engine start timeout");
    }

    void wait_for_batch_size(size_t count) {
        std::unique_lock lock(mutex_);
        if (!changed_.wait_for(lock, std::chrono::seconds(5), [this, count] {
            for (const auto& batch : batches_) if (batch.size() >= count) return true;
            return false;
        })) throw std::runtime_error("fake engine batch timeout");
    }

    std::vector<std::vector<llw_handle_t>> batches() const {
        std::lock_guard lock(mutex_);
        return batches_;
    }

    uint32_t cleanup_count(llw_handle_t handle) const {
        std::lock_guard lock(mutex_);
        const auto found = cleanup_calls_.find(handle);
        return found == cleanup_calls_.end() ? 0 : found->second;
    }

    std::vector<std::string> operation_log() const {
        std::lock_guard lock(mutex_);
        return operation_log_;
    }

private:
    struct Stored { EngineRequest request; uint32_t steps{}; };
    mutable std::mutex mutex_;
    std::condition_variable gate_;
    std::condition_variable changed_;
    std::map<llw_handle_t, Stored> requests_;
    std::vector<std::vector<llw_handle_t>> batches_;
    std::map<llw_handle_t, uint32_t> cleanup_calls_;
    std::vector<std::string> operation_log_;
    bool released_{};
    bool decode_failure_{};
    std::vector<uint8_t> rejected_prompt_;
};
```

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

Create `native/llm-runtime/tests/llama_engine_test.cpp`:

```cpp
#include "llama_engine.h"
#include <cstdio>
#include <string>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "%s:%d failed: %s\n", __FILE__, __LINE__, #condition); return 1; \
} } while (false)

ModelConfig valid_config() {
    ModelConfig config;
    config.path = "model.gguf";
    config.backend_directory = ".";
    config.backend = LLW_BACKEND_CPU;
    config.device_index = 0;
    config.slots = 2;
    config.context_tokens_per_slot = 4096;
    config.logical_batch_tokens = 512;
    config.physical_batch_tokens = 128;
    config.n_threads = 4;
    config.n_threads_batch = 4;
    config.n_gpu_layers = 0;
    config.use_mmap = true;
    return config;
}

int main() {
    std::string error;
    ModelConfig config = valid_config();
    CHECK(validate_model_config(config, error) == LLW_OK);
    config.backend = 99;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 0;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 5;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.context_tokens_per_slot = 511;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.physical_batch_tokens = 513;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.n_threads_batch = 257;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.device_index = LLW_MAX_DEVICE_INDEX + 1;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);

    const std::vector<DeviceRecord> devices = {
        {LLW_BACKEND_CPU, 0, nullptr, "cpu:0", "CPU", "ggml-cpu"},
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:0", "CUDA 0", "ggml-cuda"},
        {LLW_BACKEND_CUDA, 1, nullptr, "cuda:1", "CUDA 1", "ggml-cuda"},
    };
    const auto cuda_one = select_device(devices, LLW_BACKEND_CUDA, 1, LLW_BACKEND_CUDA);
    CHECK(cuda_one.has_value());
    CHECK(cuda_one->id == "cuda:1");
    CHECK(!select_device(devices, LLW_BACKEND_VULKAN, 0, LLW_BACKEND_CUDA).has_value());
    CHECK(!select_device(devices, LLW_BACKEND_CUDA, 2, LLW_BACKEND_CUDA).has_value());
    return 0;
}
```

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
#include <cstddef>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

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
    std::string backend_directory;
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

struct SequenceView {
    llw_handle_t handle{};
    uint32_t seq_id{};
    const std::vector<llama_token>* prompt_tokens{};
    size_t prompt_cursor{};
    uint32_t next_position{};
    bool has_pending_token{};
    llama_token pending_token{};
};
struct BatchItem {
    llw_handle_t handle{};
    uint32_t seq_id{};
    llama_token token{};
    llama_pos position{};
    bool logits{};
};
struct BatchPlan {
    std::vector<BatchItem> items;
    size_t next_start{};
};
struct LogitOwner {
    llw_handle_t handle{};
    int32_t batch_index{};
};
struct StopMatch {
    size_t position{};
    size_t stop_index{};
    size_t length{};
};

llw_result_t validate_model_config(const ModelConfig&, std::string&);
std::vector<DeviceRecord> assign_device_indices(std::vector<DeviceRecord>);
std::optional<DeviceRecord> select_device(
    const std::vector<DeviceRecord>&, int32_t, uint32_t, int32_t);
std::vector<DeviceRecord> enumerate_pack_devices(const std::string& backend_directory);
BatchPlan plan_batch(
    const std::vector<SequenceView>& sequences, size_t capacity, size_t start_index);
std::vector<LogitOwner> collect_logit_owners(const BatchPlan& plan);
uint32_t effective_generation_budget(
    size_t prompt_tokens, uint32_t requested_tokens, uint32_t context_tokens_per_slot);
std::optional<StopMatch> find_stop_match(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops);
size_t safe_output_prefix(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops);
void accept_history_tokens(const std::vector<llama_token>& tokens,
                           const std::function<void(llama_token)>& accept);
void accept_history_token(llama_token token,
                          const std::function<void(llama_token)>& accept);

class LlamaEngine final : public InferenceEngine {
public:
    LlamaEngine(ModelConfig config, std::function<void(float)> progress);
    ~LlamaEngine() override;
    uint64_t start(EngineRequest request) override;
    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override;
    void cleanup(llw_handle_t handle, uint32_t seq_id) override;
private:
    struct Sequence;
    ModelConfig config_;
    llama_model* model_{};
    llama_context* context_{};
    const llama_vocab* vocab_{};
    llama_batch batch_{};
    std::unordered_map<llw_handle_t, std::unique_ptr<Sequence>> sequences_;
    size_t batch_start_{};
    std::mutex mutex_;
    bool backend_acquired_{};
};
```

- [ ] **Step 3: Implement pack discovery, model ownership, shared decode, and samplers**

Create `native/llm-runtime/src/llama_engine.cpp` with this complete implementation:

Stop matching chooses the lowest output byte position across all configured stops. At the same
position, the longest stop wins; equal-length ties preserve configuration order. Only bytes before
the selected stop are emitted, and suffixes that are prefixes of any stop remain buffered.

```cpp
#include "llama_engine.h"
#include "ggml-backend.h"
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <stdexcept>
#include <string>
#include <utility>

namespace {
std::mutex backend_mutex;
uint32_t backend_users{};

void acquire_backend() {
    std::lock_guard lock(backend_mutex);
    if (backend_users++ == 0) llama_backend_init();
}

void release_backend() {
    std::lock_guard lock(backend_mutex);
    if (backend_users != 0 && --backend_users == 0) llama_backend_free();
}

int32_t compiled_gpu_backend() {
    const std::string pack = LLW_BACKEND_PACK_NAME;
    if (pack == "CUDA") return LLW_BACKEND_CUDA;
    if (pack == "VULKAN") return LLW_BACKEND_VULKAN;
    return LLW_BACKEND_CPU;
}

llama_sampler* make_sampler(const SamplingConfig& config) {
    llama_sampler* chain = llama_sampler_chain_init(llama_sampler_chain_default_params());
    if (!chain) throw std::bad_alloc();
    const auto add = [chain](llama_sampler* sampler) {
        if (!sampler) { llama_sampler_free(chain); throw std::bad_alloc(); }
        llama_sampler_chain_add(chain, sampler);
    };
    add(llama_sampler_init_penalties(config.repeat_last_n, config.repeat_penalty,
                                     config.frequency_penalty, config.presence_penalty));
    if (config.top_k > 0) add(llama_sampler_init_top_k(config.top_k));
    if (config.top_p < 1.0f) add(llama_sampler_init_top_p(config.top_p, 1));
    if (config.min_p > 0.0f) add(llama_sampler_init_min_p(config.min_p, 1));
    add(llama_sampler_init_temp(config.temperature));
    add(config.temperature == 0.0f ? llama_sampler_init_greedy()
                                   : llama_sampler_init_dist(config.seed));
    return chain;
}

std::vector<uint8_t> token_piece(const llama_vocab* vocab, llama_token token) {
    char local[256];
    int32_t count = llama_token_to_piece(vocab, token, local, sizeof(local), 0, true);
    if (count >= 0) return {reinterpret_cast<uint8_t*>(local),
                            reinterpret_cast<uint8_t*>(local) + count};
    if (count == std::numeric_limits<int32_t>::min())
        throw std::runtime_error("token piece length overflow");
    std::vector<char> storage(static_cast<size_t>(-count));
    count = llama_token_to_piece(vocab, token, storage.data(),
                                 static_cast<int32_t>(storage.size()), 0, true);
    if (count < 0) throw std::runtime_error("llama_token_to_piece failed");
    return {reinterpret_cast<uint8_t*>(storage.data()),
            reinterpret_cast<uint8_t*>(storage.data()) + count};
}

} // namespace

struct LlamaEngine::Sequence {
    llw_handle_t handle{};
    uint32_t seq_id{};
    std::vector<llama_token> prompt_tokens;
    uint32_t prompt_token_count{};
    size_t prompt_cursor{};
    uint32_t next_position{};
    uint32_t generated{};
    uint32_t max_new_tokens{};
    uint32_t effective_generation_budget{};
    std::optional<llama_token> pending_token;
    std::vector<std::vector<uint8_t>> stops;
    std::vector<uint8_t> pending_output;
    llama_sampler* sampler{};
    ~Sequence() { if (sampler) llama_sampler_free(sampler); }
};

llw_result_t validate_model_config(const ModelConfig& config, std::string& error) {
    if (config.path.empty() || config.path.size() > LLW_MAX_MODEL_PATH_BYTES ||
        config.backend < LLW_BACKEND_AUTO || config.backend > LLW_BACKEND_VULKAN ||
        config.device_index > LLW_MAX_DEVICE_INDEX || config.slots < 1 ||
        config.slots > LLW_MAX_SLOTS || config.context_tokens_per_slot < 512 ||
        config.context_tokens_per_slot > 262144 || config.logical_batch_tokens < 1 ||
        config.logical_batch_tokens > 8192 || config.physical_batch_tokens < 1 ||
        config.physical_batch_tokens > config.logical_batch_tokens || config.n_threads < 1 ||
        config.n_threads > 256 || config.n_threads_batch < 1 ||
        config.n_threads_batch > 256 || config.n_gpu_layers < -1 ||
        config.n_gpu_layers > 65535) {
        error = "model configuration is outside declared bounds";
        return LLW_ERR_INVALID_ARGUMENT;
    }
    return LLW_OK;
}

std::vector<DeviceRecord> assign_device_indices(std::vector<DeviceRecord> devices) {
    uint32_t cpu_index = 0;
    uint32_t gpu_index = 0;
    for (DeviceRecord& device : devices) {
        if (device.backend == LLW_BACKEND_CPU) {
            device.backend_index = cpu_index++;
        } else if (device.backend == LLW_BACKEND_CUDA ||
                   device.backend == LLW_BACKEND_VULKAN) {
            device.backend_index = gpu_index++;
        }
    }
    return devices;
}

std::optional<DeviceRecord> select_device(const std::vector<DeviceRecord>& devices,
                                          int32_t backend, uint32_t index,
                                          int32_t pack_backend) {
    int32_t selected_backend = backend;
    if (backend == LLW_BACKEND_AUTO) {
        selected_backend = pack_backend;
        if (std::none_of(devices.begin(), devices.end(), [selected_backend](const DeviceRecord& d) {
                return d.backend == selected_backend;
            })) selected_backend = LLW_BACKEND_CPU;
    }
    for (const DeviceRecord& device : devices) {
        if (device.backend == selected_backend && device.backend_index == index) return device;
    }
    return std::nullopt;
}

std::vector<DeviceRecord> enumerate_pack_devices(const std::string& directory) {
    ggml_backend_load_all_from_path(directory.c_str());
    std::vector<DeviceRecord> result;
    for (size_t index = 0; index < ggml_backend_dev_count(); ++index) {
        ggml_backend_dev_t device = ggml_backend_dev_get(index);
        if (!device) continue;
        const auto type = ggml_backend_dev_type(device);
        int32_t backend = LLW_BACKEND_CPU;
        if (type == GGML_BACKEND_DEVICE_TYPE_GPU || type == GGML_BACKEND_DEVICE_TYPE_IGPU) {
            backend = compiled_gpu_backend();
        } else if (type != GGML_BACKEND_DEVICE_TYPE_CPU) {
            continue;
        }
        ggml_backend_dev_props properties{};
        ggml_backend_dev_get_props(device, &properties);
        const char* registry = ggml_backend_reg_name(ggml_backend_dev_backend_reg(device));
        DeviceRecord record;
        record.backend = backend;
        record.backend_index = 0;
        record.device = device;
        record.id = properties.device_id ? properties.device_id
                                         : std::to_string(backend) + ":pending";
        record.name = ggml_backend_dev_name(device) ? ggml_backend_dev_name(device) : "unknown";
        record.vendor = registry ? registry : "ggml";
        result.push_back(std::move(record));
    }
    result = assign_device_indices(std::move(result));
    for (DeviceRecord& record : result) {
        if (record.id == std::to_string(record.backend) + ":pending")
            record.id = std::to_string(record.backend) + ":" +
                        std::to_string(record.backend_index);
    }
    return result;
}

BatchPlan plan_batch(const std::vector<SequenceView>& sequences, size_t capacity,
                     size_t start_index) {
    BatchPlan result;
    if (sequences.empty() || capacity == 0) return result;
    const size_t count = sequences.size();
    size_t start = start_index % count;
    std::vector<size_t> cursors;
    std::vector<bool> pending_consumed(count, false);
    cursors.reserve(count);
    for (const SequenceView& sequence : sequences) cursors.push_back(sequence.prompt_cursor);
    const auto eligible = [&](size_t index) {
        const SequenceView& sequence = sequences[index];
        return (sequence.prompt_tokens && cursors[index] < sequence.prompt_tokens->size()) ||
               (sequence.has_pending_token && !pending_consumed[index]);
    };
    while (result.items.size() < capacity) {
        size_t eligible_count = 0;
        for (size_t index = 0; index < count; ++index)
            if (eligible(index)) ++eligible_count;
        if (eligible_count == 0) break;
        const size_t quota = std::max<size_t>(
            1, (capacity - result.items.size()) / eligible_count);
        bool made_progress = false;
        size_t next_start = start;
        for (size_t offset = 0; offset < count && result.items.size() < capacity; ++offset) {
            const size_t index = (start + offset) % count;
            if (!eligible(index)) continue;
            const SequenceView& sequence = sequences[index];
            size_t emitted = 0;
            if (sequence.prompt_tokens && cursors[index] < sequence.prompt_tokens->size()) {
                const size_t available = sequence.prompt_tokens->size() - cursors[index];
                const size_t take = std::min({quota, available,
                    capacity - result.items.size()});
                for (size_t item = 0; item < take; ++item) {
                    const size_t token_index = cursors[index]++;
                    result.items.push_back(BatchItem{sequence.handle, sequence.seq_id,
                        (*sequence.prompt_tokens)[token_index],
                        static_cast<llama_pos>(sequence.next_position +
                            token_index - sequence.prompt_cursor),
                        token_index + 1 == sequence.prompt_tokens->size()});
                }
                emitted = take;
            } else if (sequence.has_pending_token && !pending_consumed[index]) {
                pending_consumed[index] = true;
                result.items.push_back(BatchItem{sequence.handle, sequence.seq_id,
                    sequence.pending_token, static_cast<llama_pos>(sequence.next_position), true});
                emitted = 1;
            }
            if (emitted != 0) {
                made_progress = true;
                next_start = (index + 1) % count;
            }
        }
        if (!made_progress) break;
        start = next_start;
    }
    result.next_start = start;
    return result;
}

std::vector<LogitOwner> collect_logit_owners(const BatchPlan& plan) {
    std::vector<LogitOwner> owners;
    for (size_t index = 0; index < plan.items.size(); ++index) {
        if (plan.items[index].logits) {
            owners.push_back(LogitOwner{
                plan.items[index].handle, static_cast<int32_t>(index)});
        }
    }
    return owners;
}

uint32_t effective_generation_budget(size_t prompt_tokens, uint32_t requested_tokens,
                                     uint32_t context_tokens_per_slot) {
    if (prompt_tokens >= context_tokens_per_slot) return 0;
    const uint64_t available = static_cast<uint64_t>(context_tokens_per_slot) - prompt_tokens;
    return static_cast<uint32_t>(std::min<uint64_t>(requested_tokens, available));
}

std::optional<StopMatch> find_stop_match(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops) {
    std::optional<StopMatch> best;
    for (size_t index = 0; index < stops.size(); ++index) {
        if (stops[index].empty()) continue;
        const auto found = std::search(output.begin(), output.end(),
                                       stops[index].begin(), stops[index].end());
        if (found == output.end()) continue;
        const size_t position = static_cast<size_t>(found - output.begin());
        const StopMatch candidate{position, index, stops[index].size()};
        if (!best || candidate.position < best->position ||
            (candidate.position == best->position && candidate.length > best->length)) {
            best = candidate;
        }
    }
    return best;
}

size_t safe_output_prefix(const std::vector<uint8_t>& output,
                          const std::vector<std::vector<uint8_t>>& stops) {
    size_t retained = 0;
    for (const auto& stop : stops) {
        if (stop.empty()) continue;
        const size_t limit = std::min(output.size(), stop.size() - 1);
        for (size_t length = limit; length > retained; --length) {
            if (std::equal(output.end() - static_cast<std::ptrdiff_t>(length), output.end(),
                           stop.begin(), stop.begin() + static_cast<std::ptrdiff_t>(length))) {
                retained = length;
                break;
            }
        }
    }
    return output.size() - retained;
}

void accept_history_tokens(const std::vector<llama_token>& tokens,
                           const std::function<void(llama_token)>& accept) {
    for (const llama_token token : tokens) accept(token);
}

void accept_history_token(llama_token token,
                          const std::function<void(llama_token)>& accept) {
    accept(token);
}

LlamaEngine::LlamaEngine(ModelConfig config, std::function<void(float)> progress)
    : config_(std::move(config)) {
    std::string error;
    if (validate_model_config(config_, error) != LLW_OK) throw std::invalid_argument(error);
    acquire_backend();
    backend_acquired_ = true;
    try {
        const std::vector<DeviceRecord> devices = enumerate_pack_devices(config_.backend_directory);
        const auto selected = select_device(
            devices, config_.backend, config_.device_index, compiled_gpu_backend());
        if (!selected) throw std::invalid_argument("selected backend device was not found");
        struct ProgressState { std::function<void(float)>* callback; } state{&progress};
        const auto progress_bridge = [](float value, void* user_data) -> bool {
            auto& context = *static_cast<ProgressState*>(user_data);
            (*context.callback)(value);
            return true;
        };
        ggml_backend_dev_t selected_devices[2] = {selected->device, nullptr};
        llama_model_params model_params = llama_model_default_params();
        model_params.devices = selected_devices;
        model_params.n_gpu_layers = config_.n_gpu_layers;
        model_params.main_gpu = 0;
        model_params.use_mmap = config_.use_mmap;
        model_params.use_mlock = config_.use_mlock;
        model_params.check_tensors = config_.check_tensors;
        model_params.progress_callback = progress_bridge;
        model_params.progress_callback_user_data = &state;
        model_ = llama_model_load_from_file(config_.path.c_str(), model_params);
        if (!model_) throw std::runtime_error("llama_model_load_from_file failed");

        llama_context_params context_params = llama_context_default_params();
        context_params.n_ctx = config_.context_tokens_per_slot * config_.slots;
        context_params.n_batch = config_.logical_batch_tokens;
        context_params.n_ubatch = config_.physical_batch_tokens;
        context_params.n_seq_max = config_.slots;
        context_params.n_threads = config_.n_threads;
        context_params.n_threads_batch = config_.n_threads_batch;
        context_params.embeddings = false;
        context_params.no_perf = false;
        context_ = llama_init_from_model(model_, context_params);
        if (!context_) throw std::runtime_error("llama_init_from_model failed");
        vocab_ = llama_model_get_vocab(model_);
        if (!vocab_) throw std::runtime_error("llama_model_get_vocab failed");
        batch_ = llama_batch_init(static_cast<int32_t>(config_.logical_batch_tokens), 0, 1);
        if (!batch_.token || !batch_.pos || !batch_.n_seq_id || !batch_.seq_id || !batch_.logits)
            throw std::bad_alloc();
    } catch (...) {
        if (batch_.token || batch_.embd) llama_batch_free(batch_);
        if (context_) llama_free(context_);
        if (model_) llama_model_free(model_);
        context_ = nullptr;
        model_ = nullptr;
        release_backend();
        backend_acquired_ = false;
        throw;
    }
}

LlamaEngine::~LlamaEngine() {
    std::lock_guard lock(mutex_);
    for (const auto& [handle, sequence] : sequences_) {
        (void)handle;
        llama_memory_seq_rm(llama_get_memory(context_),
                            static_cast<llama_seq_id>(sequence->seq_id), -1, -1);
    }
    sequences_.clear();
    if (batch_.token || batch_.embd) llama_batch_free(batch_);
    if (context_) llama_free(context_);
    if (model_) llama_model_free(model_);
    if (backend_acquired_) release_backend();
}

uint64_t LlamaEngine::start(EngineRequest request) {
    std::lock_guard lock(mutex_);
    if (sequences_.count(request.handle) != 0) throw std::invalid_argument("duplicate request handle");
    if (request.prompt.size() > static_cast<size_t>(std::numeric_limits<int32_t>::max()))
        throw std::invalid_argument("prompt is too large for llama_tokenize");
    int32_t count = llama_tokenize(vocab_, reinterpret_cast<const char*>(request.prompt.data()),
        static_cast<int32_t>(request.prompt.size()), nullptr, 0, true, true);
    if (count == std::numeric_limits<int32_t>::min() || count >= 0)
        throw std::runtime_error("token count query failed");
    auto sequence = std::make_unique<Sequence>();
    sequence->prompt_tokens.resize(static_cast<size_t>(-count));
    count = llama_tokenize(vocab_, reinterpret_cast<const char*>(request.prompt.data()),
        static_cast<int32_t>(request.prompt.size()), sequence->prompt_tokens.data(),
        static_cast<int32_t>(sequence->prompt_tokens.size()), true, true);
    if (count < 0) throw std::runtime_error("prompt tokenization failed");
    sequence->prompt_tokens.resize(static_cast<size_t>(count));
    const uint32_t budget = effective_generation_budget(
        sequence->prompt_tokens.size(), request.max_new_tokens,
        config_.context_tokens_per_slot);
    if (budget == 0)
        throw std::invalid_argument("prompt leaves no generation token in its slot context");
    sequence->handle = request.handle;
    sequence->seq_id = request.seq_id;
    sequence->max_new_tokens = request.max_new_tokens;
    sequence->prompt_token_count = static_cast<uint32_t>(sequence->prompt_tokens.size());
    sequence->effective_generation_budget = budget;
    sequence->stops = std::move(request.stops);
    sequence->sampler = make_sampler(request.sampling);
    accept_history_tokens(sequence->prompt_tokens, [sampler = sequence->sampler](llama_token token) {
        llama_sampler_accept(sampler, token);
    });
    const uint64_t prompt_tokens = sequence->prompt_tokens.size();
    sequences_.emplace(request.handle, std::move(sequence));
    return prompt_tokens;
}

std::vector<EngineStep> LlamaEngine::decode(const std::vector<llw_handle_t>& active) {
    std::lock_guard lock(mutex_);
    std::vector<SequenceView> views;
    for (const llw_handle_t handle : active) {
        const auto found = sequences_.find(handle);
        if (found == sequences_.end()) continue;
        const Sequence& sequence = *found->second;
        views.push_back(SequenceView{handle, sequence.seq_id, &sequence.prompt_tokens,
            sequence.prompt_cursor, sequence.next_position, sequence.pending_token.has_value(),
            sequence.pending_token.value_or(0)});
    }
    const BatchPlan plan = plan_batch(views, config_.logical_batch_tokens, batch_start_);
    batch_start_ = plan.next_start;
    if (plan.items.empty()) return {};
    batch_.n_tokens = static_cast<int32_t>(plan.items.size());
    const std::vector<LogitOwner> logit_owners = collect_logit_owners(plan);
    for (size_t index = 0; index < plan.items.size(); ++index) {
        const BatchItem& item = plan.items[index];
        batch_.token[index] = item.token;
        batch_.pos[index] = item.position;
        batch_.n_seq_id[index] = 1;
        batch_.seq_id[index][0] = static_cast<llama_seq_id>(item.seq_id);
        batch_.logits[index] = item.logits ? 1 : 0;
        Sequence& sequence = *sequences_.at(item.handle);
        if (sequence.prompt_cursor < sequence.prompt_tokens.size()) ++sequence.prompt_cursor;
        else sequence.pending_token.reset();
        ++sequence.next_position;
    }
    const int32_t decode_result = llama_decode(context_, batch_);
    if (decode_result != 0) {
        std::vector<EngineStep> failed;
        for (const llw_handle_t handle : active)
            failed.push_back(EngineStep{handle, {}, false, true,
                "llama_decode returned " + std::to_string(decode_result)});
        return failed;
    }

    std::vector<EngineStep> result;
    for (const LogitOwner& owner : logit_owners) {
        Sequence& sequence = *sequences_.at(owner.handle);
        const llama_token token = llama_sampler_sample(sequence.sampler, context_,
                                                       owner.batch_index);
        accept_history_token(token, [sampler = sequence.sampler](llama_token accepted) {
            llama_sampler_accept(sampler, accepted);
        });
        ++sequence.generated;
        EngineStep step;
        step.handle = owner.handle;
        bool done = llama_vocab_is_eog(vocab_, token) ||
                    sequence.generated >= sequence.effective_generation_budget;
        step.finish_reason = llama_vocab_is_eog(vocab_, token) ? "stop" :
            (sequence.generated >= sequence.effective_generation_budget ? "length" : "");
        if (!llama_vocab_is_eog(vocab_, token)) {
            const std::vector<uint8_t> piece = token_piece(vocab_, token);
            sequence.pending_output.insert(sequence.pending_output.end(), piece.begin(), piece.end());
            if (const auto match = find_stop_match(sequence.pending_output, sequence.stops)) {
                step.token_bytes.assign(sequence.pending_output.begin(),
                    sequence.pending_output.begin() + static_cast<std::ptrdiff_t>(match->position));
                sequence.pending_output.clear();
                step.finish_reason = "stop";
                done = true;
            }
            if (!done) {
                const size_t emit = safe_output_prefix(sequence.pending_output, sequence.stops);
                if (emit != 0) {
                    step.token_bytes.assign(sequence.pending_output.begin(),
                                            sequence.pending_output.begin() + emit);
                    sequence.pending_output.erase(sequence.pending_output.begin(),
                                                  sequence.pending_output.begin() + emit);
                }
            }
        }
        if (done) {
            step.token_bytes.insert(step.token_bytes.end(), sequence.pending_output.begin(),
                                    sequence.pending_output.end());
            sequence.pending_output.clear();
            step.finished = true;
        } else {
            sequence.pending_token = token;
        }
        result.push_back(std::move(step));
    }
    return result;
}

void LlamaEngine::cleanup(llw_handle_t handle, uint32_t seq_id) {
    std::lock_guard lock(mutex_);
    const auto found = sequences_.find(handle);
    if (found == sequences_.end()) return;
    if (found->second->seq_id != seq_id) throw std::invalid_argument("sequence ID mismatch");
    const bool removed = llama_memory_seq_rm(
        llama_get_memory(context_), static_cast<llama_seq_id>(seq_id), -1, -1);
    sequences_.erase(found);
    if (!removed)
        throw std::runtime_error("failed to clear sequence memory");
}
```

- [ ] **Step 4: Add the engine test target**

Append to CMake:

```cmake
add_executable(llw_llama_engine_test tests/llama_engine_test.cpp src/llama_engine.cpp)
target_include_directories(llw_llama_engine_test PRIVATE include src)
target_compile_definitions(llw_llama_engine_test PRIVATE
  LLW_BACKEND_PACK_NAME="${LLW_BACKEND_PACK}"
)
target_link_libraries(llw_llama_engine_test PRIVATE llama ggml Threads::Threads)
add_test(NAME llw_llama_engine_test COMMAND llw_llama_engine_test)
set_tests_properties(llw_llama_engine_test PROPERTIES TIMEOUT 30)
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

### Task 6: Verify Shared Batch Planning And Independent Sequence State

**Files:**
- Modify: `native/llm-runtime/tests/llama_engine_test.cpp`

- [ ] **Step 1: Replace the helper test with complete batch-plan coverage**

Replace `native/llm-runtime/tests/llama_engine_test.cpp` with the complete file below. It retains Task 5's bounds/device cases and adds two-sequence, capacity, logits, position, and pending-token assertions.

```cpp
#include "llama_engine.h"
#include <cstdio>
#include <string>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "%s:%d failed: %s\n", __FILE__, __LINE__, #condition); return 1; \
} } while (false)

ModelConfig valid_config() {
    ModelConfig config;
    config.path = "model.gguf";
    config.backend_directory = ".";
    config.backend = LLW_BACKEND_CPU;
    config.device_index = 0;
    config.slots = 2;
    config.context_tokens_per_slot = 4096;
    config.logical_batch_tokens = 512;
    config.physical_batch_tokens = 128;
    config.n_threads = 4;
    config.n_threads_batch = 4;
    config.n_gpu_layers = 0;
    config.use_mmap = true;
    return config;
}

int main() {
    std::string error;
    ModelConfig config = valid_config();
    CHECK(validate_model_config(config, error) == LLW_OK);
    config.backend = 99;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 0;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 5;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.context_tokens_per_slot = 511;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.physical_batch_tokens = 513;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.n_threads_batch = 257;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);

    const std::vector<DeviceRecord> devices = assign_device_indices({
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:a", "CUDA A", "ggml-cuda"},
        {LLW_BACKEND_CPU, 99, nullptr, "cpu:a", "CPU A", "ggml-cpu"},
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:b", "CUDA B", "ggml-cuda"},
        {LLW_BACKEND_CPU, 99, nullptr, "cpu:b", "CPU B", "ggml-cpu"},
    });
    CHECK(devices[0].backend_index == 0);
    CHECK(devices[1].backend_index == 0);
    CHECK(devices[2].backend_index == 1);
    CHECK(devices[3].backend_index == 1);
    CHECK(select_device(devices, LLW_BACKEND_CUDA, 1, LLW_BACKEND_CUDA)->id == "cuda:b");
    CHECK(select_device(devices, LLW_BACKEND_AUTO, 0, LLW_BACKEND_CUDA)->id == "cuda:a");
    const std::vector<DeviceRecord> cpu_only = assign_device_indices({
        {LLW_BACKEND_CPU, 77, nullptr, "cpu:only", "CPU", "ggml-cpu"},
    });
    CHECK(select_device(cpu_only, LLW_BACKEND_AUTO, 0, LLW_BACKEND_CUDA)->id == "cpu:only");
    CHECK(!select_device(devices, LLW_BACKEND_VULKAN, 0, LLW_BACKEND_CUDA).has_value());

    const std::vector<llama_token> first_tokens = {10, 11, 12};
    const std::vector<llama_token> second_tokens = {20, 21};
    const std::vector<SequenceView> prompt_views = {
        {101, 0, &first_tokens, 0, 0, false, 0},
        {202, 1, &second_tokens, 0, 0, false, 0},
    };
    const BatchPlan prompt_plan = plan_batch(prompt_views, 5, 0);
    CHECK(prompt_plan.items.size() == 5);
    CHECK(prompt_plan.items[0].handle == 101 && prompt_plan.items[0].position == 0);
    CHECK(prompt_plan.items[1].handle == 101 && prompt_plan.items[1].position == 1);
    CHECK(prompt_plan.items[2].handle == 202 && prompt_plan.items[2].position == 0);
    CHECK(prompt_plan.items[3].handle == 202 && prompt_plan.items[3].position == 1);
    CHECK(prompt_plan.items[3].logits);
    CHECK(prompt_plan.items[4].handle == 101 && prompt_plan.items[4].position == 2);
    CHECK(prompt_plan.items[4].logits);
    const std::vector<LogitOwner> prompt_owners = collect_logit_owners(prompt_plan);
    CHECK(prompt_owners.size() == 2);
    CHECK(prompt_owners[0].handle == 202 && prompt_owners[0].batch_index == 3);
    CHECK(prompt_owners[1].handle == 101 && prompt_owners[1].batch_index == 4);

    const std::vector<SequenceView> capacity_views = {
        {101, 0, &first_tokens, 1, 8, false, 0},
        {202, 1, &second_tokens, 1, 4, false, 0},
    };
    const BatchPlan capacity_plan = plan_batch(capacity_views, 2, 0);
    CHECK(capacity_plan.items.size() == 2);
    CHECK(capacity_plan.items[0].handle == 101 && capacity_plan.items[0].token == 11);
    CHECK(capacity_plan.items[1].handle == 202 && capacity_plan.items[1].token == 21);

    const std::vector<llama_token> third_tokens = {30, 31, 32};
    const std::vector<SequenceView> small_capacity_views = {
        {101, 0, &first_tokens, 0, 0, false, 0},
        {202, 1, &second_tokens, 0, 0, false, 0},
        {303, 2, &third_tokens, 0, 0, false, 0},
    };
    const BatchPlan first_small = plan_batch(small_capacity_views, 2, 0);
    CHECK(first_small.items.size() == 2);
    CHECK(first_small.items[0].handle == 101 && first_small.items[1].handle == 202);
    const BatchPlan second_small = plan_batch(small_capacity_views, 2, first_small.next_start);
    CHECK(second_small.items.size() == 2);
    CHECK(second_small.items[0].handle == 303);
    CHECK(second_small.items[1].handle == 101);

    const std::vector<SequenceView> exhausted_views = {
        {101, 0, &first_tokens, first_tokens.size(), 3, false, 0},
        {202, 1, &second_tokens, 1, 4, false, 0},
    };
    const BatchPlan larger_than_work = plan_batch(exhausted_views, 8, 0);
    CHECK(larger_than_work.items.size() == 1);
    CHECK(larger_than_work.items[0].handle == 202 && larger_than_work.items[0].token == 21);

    const std::vector<SequenceView> generation_views = {
        {101, 0, &first_tokens, first_tokens.size(), 3, true, 31},
        {202, 1, &second_tokens, second_tokens.size(), 2, true, 41},
    };
    const BatchPlan generation_plan = plan_batch(generation_views, 2, 0);
    CHECK(generation_plan.items.size() == 2);
    CHECK(generation_plan.items[0].token == 31 && generation_plan.items[0].logits);
    CHECK(generation_plan.items[1].token == 41 && generation_plan.items[1].logits);
    CHECK(generation_plan.items[0].seq_id != generation_plan.items[1].seq_id);

    CHECK(effective_generation_budget(510, 1000, 512) == 2);
    CHECK(effective_generation_budget(512, 1, 512) == 0);
    std::vector<llama_token> accepted;
    accept_history_tokens(first_tokens, [&accepted](llama_token token) { accepted.push_back(token); });
    accept_history_token(31, [&accepted](llama_token token) { accepted.push_back(token); });
    CHECK(accepted == std::vector<llama_token>({10, 11, 12, 31}));

    const std::vector<std::vector<uint8_t>> stops = {
        {'a', 'b'}, {'a', 'b', 'c'}, {'b', 'c'}, {'a', 'b', 'c'},
    };
    const std::vector<uint8_t> overlapping = {'z', 'a', 'b', 'c', 'q'};
    const auto stop = find_stop_match(overlapping, stops);
    CHECK(stop.has_value());
    CHECK(stop->position == 1 && stop->length == 3 && stop->stop_index == 1);
    const std::vector<uint8_t> partial = {'x', 'a'};
    CHECK(safe_output_prefix(partial, stops) == 1);
    const std::vector<uint8_t> no_prefix = {'x', 'y'};
    CHECK(safe_output_prefix(no_prefix, stops) == 2);
    return 0;
}
```

- [ ] **Step 2: Run engine and fake scheduler tests**

```powershell
cmake --build .cmake-build/llm-cpu --config Debug --target llw_llama_engine_test llw_scheduler_test
ctest --test-dir .cmake-build/llm-cpu -C Debug -R "llw_(llama_engine|scheduler)_test" --output-on-failure
```

Expected: batch-plan fairness/capacity/logits/sequence assertions and fake scheduler cancellation/concurrency tests pass.

- [ ] **Step 3: Commit shared decode verification**

```powershell
git add native/llm-runtime/tests/llama_engine_test.cpp
git commit -m "test: verify shared llama batch planning"
```

### Task 7: Implement The Complete C ABI Facade

**Files:**
- Modify: `native/llm-runtime/CMakeLists.txt`
- Delete: `native/llm-runtime/src/fake_runtime.cpp`
- Create: `native/llm-runtime/src/runtime.cpp`
- Modify: `native/llm-runtime/tests/abi_layout_test.cpp`

- [ ] **Step 1: Add failing export and lifecycle tests**

Add this complete function above `main` in `native/llm-runtime/tests/abi_layout_test.cpp`, then call `CHECK(test_v11_exports() == 0);` immediately before the existing `return 0;`:

```cpp
int test_v11_exports() {
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 2;
    create.scheduler.request_queue_capacity = 2;
    create.scheduler.event_queue_capacity = 32;
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    CHECK(runtime != nullptr);

    llw_buffer_t schema{};
    schema.struct_size = sizeof(schema);
    CHECK(llw_runtime_get_option_schema(runtime, &schema, &error) == LLW_ERR_BUFFER_TOO_SMALL);
    CHECK(schema.len > 0);
    std::vector<uint8_t> schema_bytes(static_cast<size_t>(schema.len));
    schema.data = schema_bytes.data();
    schema.capacity = schema_bytes.size();
    CHECK(llw_runtime_get_option_schema(runtime, &schema, &error) == LLW_OK);
    const std::string schema_text(schema_bytes.begin(), schema_bytes.end());
    CHECK(schema_text.find("\"eventQueueCapacity\"") != std::string::npos);
    CHECK(schema_text.find("\"nThreadsBatch\"") != std::string::npos);
    CHECK(schema_text.find("\"maxTotalBytes\":2048") != std::string::npos);

    llw_request_params_t request{};
    request.struct_size = sizeof(request);
    request.model_handle = 1;
    const uint8_t prompt[] = {'x'};
    request.prompt = prompt;
    request.prompt_len = sizeof(prompt);
    request.max_new_tokens = 1;
    request.temperature = 0;
    request.top_p = 1;
    request.repeat_penalty = 1;
    llw_handle_t request_handle{99};
    CHECK(llw_request_submit(runtime, &request, &request_handle, &error) == LLW_ERR_INVALID_STATE);
    CHECK(request_handle == 0);
    CHECK(llw_model_unload(runtime, 1, &error) == LLW_ERR_NOT_FOUND);

    const std::string missing_model = "llw-test-bad-alloc.gguf";
    llw_model_load_params_t failing_model{};
    failing_model.struct_size = sizeof(failing_model);
    failing_model.path_utf8 = reinterpret_cast<const uint8_t*>(missing_model.data());
    failing_model.path_len = missing_model.size();
    failing_model.backend = LLW_BACKEND_CPU;
    failing_model.context_tokens_per_slot = 512;
    failing_model.logical_batch_tokens = 64;
    failing_model.physical_batch_tokens = 64;
    failing_model.n_threads = 1;
    failing_model.n_threads_batch = 1;
    failing_model.use_mmap = 1;
    llw_handle_t failed_model{};
    CHECK(llw_model_load(runtime, &failing_model, &failed_model, &error) != LLW_OK);
    CHECK(failed_model == 0);
    const llw_result_t retry = llw_model_load(runtime, &failing_model, &failed_model, &error);
    CHECK(retry != LLW_ERR_BUSY);
    CHECK(retry != LLW_OK);

    llw_scheduler_config_t undersized{};
    undersized.struct_size = sizeof(undersized) - 1;
    create.scheduler = undersized;
    llw_runtime_t* rejected{};
    CHECK(llw_runtime_create(&create, &rejected, &error) == LLW_ERR_INVALID_ARGUMENT);
    CHECK(rejected == nullptr);
    llw_runtime_destroy(runtime);
    llw_runtime_destroy(nullptr);
    return 0;
}
```

Add these includes to the existing test include list:

```cpp
#include <string>
#include <vector>
```

Run the Windows export-table command:

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
  $<$<CONFIG:Debug>:LLW_RUNTIME_TESTING>
)
target_include_directories(local_llm_runtime PUBLIC include PRIVATE src)
target_link_libraries(local_llm_runtime PRIVATE llama ggml Threads::Threads)

set(LLW_PACK_DESTINATION ".")
install(TARGETS local_llm_runtime llama ggml ggml-base
  RUNTIME DESTINATION "${LLW_PACK_DESTINATION}"
  LIBRARY DESTINATION "${LLW_PACK_DESTINATION}"
)
foreach(backend_target IN ITEMS ggml-cpu ggml-cuda ggml-vulkan)
  if(TARGET ${backend_target})
    install(TARGETS ${backend_target}
      RUNTIME DESTINATION "${LLW_PACK_DESTINATION}"
      LIBRARY DESTINATION "${LLW_PACK_DESTINATION}"
    )
  endif()
endforeach()
```

At the pinned commit, the shared core targets are exactly `llama`, `ggml`, and `ggml-base`;
`ggml_add_backend_library` creates the dynamically loaded `ggml-cpu`, `ggml-cuda`, and
`ggml-vulkan` targets. The conditional backend installs allow each configured pack to contain CPU
plus only its selected GPU backend.

Delete the superseded fake facade after the new target builds:

```powershell
Remove-Item -LiteralPath native/llm-runtime/src/fake_runtime.cpp
```

- [ ] **Step 3: Implement runtime ownership, validation, the exact schema, and all fourteen exports**

Create `native/llm-runtime/src/runtime.cpp` with the complete code below. The option schema concatenates the compile-time pack name directly into the JSON. Lifecycle fields (`modelHandle`), correlation pointers (`requestUserData`), ABI mechanics (`structSize`, `flags`, `reserved*`), and borrowed byte pointers are contracts rather than configurable options; their user inputs (`modelPath`, `promptBytes`, and stop bounds) are represented explicitly.

```cpp
#include "event_dispatcher.h"
#include "llama_engine.h"
#include "llw_runtime.h"
#include "scheduler.h"
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <iterator>
#include <limits>
#include <memory>
#include <mutex>
#include <new>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#endif

struct llw_runtime_t {
    llw_callback_table_t callbacks{};
    llw_scheduler_config_t config{};
    std::unique_ptr<EventDispatcher> dispatcher;
    std::unique_ptr<LlamaEngine> engine;
    std::unique_ptr<Scheduler> scheduler;
    llw_handle_t model_handle{};
    llw_handle_t next_model_handle{1};
    bool model_loading{};
    std::string backend_directory;
    std::mutex mutex;
};

struct ModelLoadingReset {
    llw_runtime_t& runtime;
    std::unique_lock<std::mutex>& lock;
    bool active{true};
    ~ModelLoadingReset() {
        if (!active) return;
        if (lock.owns_lock()) {
            runtime.model_loading = false;
        } else {
            lock.lock();
            runtime.model_loading = false;
            lock.unlock();
        }
    }
    void release() { active = false; }
};

namespace {
constexpr size_t RUNTIME_CREATE_V1_0_SIZE = offsetof(llw_runtime_create_params_t, scheduler);
int module_anchor{};

template <size_t N> bool zeroed(const uint64_t (&values)[N]) {
    return std::all_of(values, values + N, [](uint64_t value) { return value == 0; });
}

void clear_error(llw_error_t* error) {
    if (!error || error->struct_size < sizeof(uint32_t) + sizeof(int32_t)) return;
    error->code = LLW_OK;
    if (error->struct_size >= sizeof(llw_error_t)) {
        error->flags = 0;
        error->message[0] = '\0';
        std::fill(std::begin(error->reserved), std::end(error->reserved), uint64_t{0});
    }
}

llw_result_t fail(llw_error_t* error, llw_result_t code, const std::string& message) {
    if (error && error->struct_size >= sizeof(uint32_t) + sizeof(int32_t)) {
        error->code = code;
        if (error->struct_size >= sizeof(llw_error_t)) {
            error->flags = 0;
            std::strncpy(error->message, message.c_str(), sizeof(error->message) - 1);
            error->message[sizeof(error->message) - 1] = '\0';
        }
    }
    return code;
}

template <class F> llw_result_t guarded(llw_error_t* error, F&& body) noexcept {
    try { clear_error(error); return body(); }
    catch (const std::invalid_argument& exception) {
        return fail(error, LLW_ERR_INVALID_ARGUMENT, exception.what());
    } catch (const std::bad_alloc&) {
        return fail(error, LLW_ERR_INTERNAL, "allocation failed");
    } catch (const std::exception& exception) {
        return fail(error, LLW_ERR_INTERNAL, exception.what());
    } catch (...) {
        return fail(error, LLW_ERR_INTERNAL, "unknown native exception");
    }
}

bool valid_utf8(const uint8_t* data, size_t size) {
    size_t index = 0;
    while (index < size) {
        const uint8_t first = data[index++];
        if (first < 0x80) continue;
        uint32_t codepoint = 0;
        size_t continuation = 0;
        if ((first & 0xe0) == 0xc0) { codepoint = first & 0x1f; continuation = 1; }
        else if ((first & 0xf0) == 0xe0) { codepoint = first & 0x0f; continuation = 2; }
        else if ((first & 0xf8) == 0xf0) { codepoint = first & 0x07; continuation = 3; }
        else return false;
        if (index + continuation > size) return false;
        for (size_t offset = 0; offset < continuation; ++offset) {
            const uint8_t next = data[index++];
            if ((next & 0xc0) != 0x80) return false;
            codepoint = (codepoint << 6) | (next & 0x3f);
        }
        if ((continuation == 1 && codepoint < 0x80) ||
            (continuation == 2 && codepoint < 0x800) ||
            (continuation == 3 && codepoint < 0x10000) ||
            codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) return false;
    }
    return true;
}

std::string backend_directory() {
#ifdef _WIN32
    HMODULE module{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&module_anchor), &module)) {
        throw std::runtime_error("GetModuleHandleExW failed");
    }
    std::vector<wchar_t> buffer(32768);
    const DWORD length = GetModuleFileNameW(module, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) throw std::runtime_error("GetModuleFileNameW failed");
    return std::filesystem::path(std::wstring(buffer.data(), length)).parent_path().u8string();
#else
    return std::filesystem::current_path().u8string();
#endif
}

std::string pack_name() { return LLW_BACKEND_PACK_NAME; }

int32_t pack_backend() {
    const std::string pack = pack_name();
    if (pack == "CUDA") return LLW_BACKEND_CUDA;
    if (pack == "VULKAN") return LLW_BACKEND_VULKAN;
    return LLW_BACKEND_CPU;
}

void copy_text(char* destination, size_t capacity, const std::string& source) {
    if (capacity == 0) return;
    const size_t count = std::min(capacity - 1, source.size());
    std::memcpy(destination, source.data(), count);
    destination[count] = '\0';
}

void publish_runtime_event(llw_runtime_t& runtime, int32_t type, uint32_t format,
                           llw_handle_t model, std::string payload) {
    OwnedEvent event;
    event.type = type;
    event.data_format = format;
    event.model = model;
    event.sequence = 0;
    event.data.assign(payload.begin(), payload.end());
    if (!runtime.dispatcher->publish(std::move(event)))
        throw std::runtime_error("event dispatcher stopped");
}

llw_scheduler_config_t scheduler_config(const llw_runtime_create_params_t& params) {
    llw_scheduler_config_t config{};
    config.struct_size = sizeof(config);
    config.slot_count = 1;
    config.request_queue_capacity = 16;
    config.event_queue_capacity = 1024;
    if (params.struct_size >= sizeof(llw_runtime_create_params_t)) config = params.scheduler;
    if (config.struct_size < sizeof(config) || config.flags != 0 || config.reserved0 != 0 ||
        !zeroed(config.reserved) || config.slot_count < 1 || config.slot_count > LLW_MAX_SLOTS ||
        config.request_queue_capacity < 1 ||
        config.request_queue_capacity > LLW_MAX_QUEUE_CAPACITY ||
        config.event_queue_capacity < 16 ||
        config.event_queue_capacity > LLW_MAX_EVENT_QUEUE_CAPACITY) {
        throw std::invalid_argument("invalid scheduler configuration");
    }
    return config;
}

void validate_model(const llw_model_load_params_t& params) {
    if (params.struct_size < sizeof(params) || params.flags != 0 || params.reserved0 != 0 ||
        !zeroed(params.reserved)) throw std::invalid_argument("invalid model structure");
    if (!params.path_utf8 || params.path_len < 1 || params.path_len > LLW_MAX_MODEL_PATH_BYTES ||
        std::find(params.path_utf8, params.path_utf8 + params.path_len, uint8_t{0}) !=
            params.path_utf8 + params.path_len || !valid_utf8(params.path_utf8, params.path_len))
        throw std::invalid_argument("invalid UTF-8 model path");
    if (params.backend < LLW_BACKEND_AUTO || params.backend > LLW_BACKEND_VULKAN ||
        params.device_index > LLW_MAX_DEVICE_INDEX || params.context_tokens_per_slot < 512 ||
        params.context_tokens_per_slot > 262144 || params.logical_batch_tokens < 1 ||
        params.logical_batch_tokens > 8192 || params.physical_batch_tokens < 1 ||
        params.physical_batch_tokens > params.logical_batch_tokens || params.n_threads < 1 ||
        params.n_threads > 256 || params.n_threads_batch < 1 || params.n_threads_batch > 256 ||
        params.n_gpu_layers < -1 || params.n_gpu_layers > 65535 || params.use_mmap > 1 ||
        params.use_mlock > 1 || params.check_tensors > 1)
        throw std::invalid_argument("model option is outside its declared bounds");
}

void validate_request(const llw_request_params_t& params) {
    if (params.struct_size < sizeof(params) || params.flags != 0 || params.reserved0 != 0 ||
        !zeroed(params.reserved)) throw std::invalid_argument("invalid request structure");
    if (!params.prompt || params.prompt_len < 1 || params.prompt_len > LLW_MAX_PROMPT_BYTES ||
        params.max_new_tokens < 1 || params.max_new_tokens > 1048576 ||
        !std::isfinite(params.temperature) || params.temperature < 0 || params.temperature > 10 ||
        params.top_k < 0 || params.top_k > 100000 || !std::isfinite(params.top_p) ||
        params.top_p < 0 || params.top_p > 1 || !std::isfinite(params.min_p) ||
        params.min_p < 0 || params.min_p > 1 || params.repeat_last_n < 0 ||
        params.repeat_last_n > 262144 || !std::isfinite(params.repeat_penalty) ||
        params.repeat_penalty < 0 || params.repeat_penalty > 10 ||
        !std::isfinite(params.frequency_penalty) || params.frequency_penalty < -2 ||
        params.frequency_penalty > 2 || !std::isfinite(params.presence_penalty) ||
        params.presence_penalty < -2 || params.presence_penalty > 2 ||
        params.stop_count > LLW_MAX_STOP_SEQUENCES ||
        (params.stop_count != 0 && !params.stop_sequences))
        throw std::invalid_argument("request option is outside its declared bounds");
    uint64_t total = 0;
    for (uint32_t index = 0; index < params.stop_count; ++index) {
        const llw_bytes_t& stop = params.stop_sequences[index];
        if (stop.struct_size < sizeof(stop) || stop.flags != 0 || !zeroed(stop.reserved) ||
            !stop.data || stop.len < 1 || stop.len > LLW_MAX_STOP_BYTES)
            throw std::invalid_argument("invalid stop sequence");
        total += stop.len;
        if (total > LLW_MAX_STOP_TOTAL_BYTES)
            throw std::invalid_argument("stop sequence bytes exceed total bound");
    }
}

std::string option_schema() {
    std::string schema = std::string(R"json({"abiMinor":1,"backendPack":")json") + pack_name() +
        R"json(","model":{"modelPath":{"type":"utf8Bytes","minBytes":1,"maxBytes":32768,"default":null,"apply":"modelReload"},"backend":{"type":"enum","values":{"auto":0,"cpu":1,"cuda":2,"vulkan":3},"default":0,"apply":"modelReload"},"deviceIndex":{"type":"uint32","min":0,"max":255,"default":0,"apply":"modelReload"},"contextTokensPerSlot":{"type":"uint32","min":512,"max":262144,"default":4096,"apply":"modelReload"},"logicalBatchTokens":{"type":"uint32","min":1,"max":8192,"default":512,"apply":"modelReload"},"physicalBatchTokens":{"type":"uint32","min":1,"maxField":"logicalBatchTokens","default":128,"apply":"modelReload"},"nThreads":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nThreadsBatch":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nGpuLayers":{"type":"int32","min":-1,"max":65535,"default":0,"apply":"modelReload"},"useMmap":{"type":"boolean","default":true,"apply":"modelReload"},"useMlock":{"type":"boolean","default":false,"apply":"modelReload"},"checkTensors":{"type":"boolean","default":false,"apply":"modelReload"}},"scheduler":{"slotCount":{"type":"uint32","min":1,"max":4,"default":1,"apply":"runtimeRestart"},"requestQueueCapacity":{"type":"uint32","min":1,"max":1024,"default":16,"apply":"runtimeRestart"},"eventQueueCapacity":{"type":"uint32","min":16,"max":65536,"default":1024,"apply":"runtimeRestart"}},"request":{"promptBytes":{"type":"bytes","minBytes":1,"maxBytes":16777216,"default":null,"apply":"nextRequest"},"maxNewTokens":{"type":"uint32","min":1,"max":1048576,"default":256,"apply":"nextRequest"},"seed":{"type":"uint32","min":0,"max":4294967295,"default":4294967295,"apply":"nextRequest"},"temperature":{"type":"float32","min":0.0,"max":10.0,"default":0.8,"apply":"nextRequest"},"topK":{"type":"int32","min":0,"max":100000,"default":40,"apply":"nextRequest"},"topP":{"type":"float32","min":0.0,"max":1.0,"default":0.95,"apply":"nextRequest"},"minP":{"type":"float32","min":0.0,"max":1.0,"default":0.05,"apply":"nextRequest"},"repeatLastN":{"type":"int32","min":0,"max":262144,"default":64,"apply":"nextRequest"},"repeatPenalty":{"type":"float32","min":0.0,"max":10.0,"default":1.1,"apply":"nextRequest"},"frequencyPenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"presencePenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"stopSequences":{"type":"bytesArray","minCount":0,"maxCount":8,"minBytesEach":1,"maxBytesEach":256,"maxTotalBytes":2048,"default":[],"apply":"nextRequest"}}})json";
    return schema;
}
} // namespace

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query, llw_abi_info_t* out_info, llw_error_t* out_error) {
    return guarded(out_error, [&] {
        if (!query || !out_info || query->struct_size < sizeof(*query) ||
            out_info->struct_size < sizeof(*out_info))
            throw std::invalid_argument("invalid ABI query");
        if (query->requested_major != LLW_ABI_MAJOR)
            return fail(out_error, LLW_ERR_ABI_MISMATCH, "unsupported ABI major");
        *out_info = {};
        out_info->struct_size = sizeof(*out_info);
        out_info->abi_major = LLW_ABI_MAJOR;
        out_info->abi_minor = LLW_ABI_MINOR;
        out_info->min_supported_major = LLW_ABI_MAJOR;
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) { return "0.2.0"; }
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) {
    return LLW_LLAMA_CPP_COMMIT;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params, llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    if (out_runtime) *out_runtime = nullptr;
    return guarded(out_error, [&] {
        if (!params || !out_runtime || params->struct_size < RUNTIME_CREATE_V1_0_SIZE ||
            params->flags != 0 || !zeroed(params->reserved) ||
            params->callbacks.struct_size < sizeof(llw_callback_table_t) ||
            params->callbacks.flags != 0 || !zeroed(params->callbacks.reserved))
            throw std::invalid_argument("invalid runtime create parameters");
        if (params->struct_size >= sizeof(*params) && !zeroed(params->reserved_v1))
            throw std::invalid_argument("runtime reserved fields must be zero");
        auto runtime = std::make_unique<llw_runtime_t>();
        runtime->callbacks = params->callbacks;
        runtime->config = scheduler_config(*params);
        runtime->backend_directory = backend_directory();
        runtime->dispatcher = std::make_unique<EventDispatcher>(runtime->callbacks,
            runtime->config.event_queue_capacity);
        publish_runtime_event(*runtime, LLW_EVENT_LOG, LLW_EVENT_DATA_UTF8, 0,
                              "runtime pack initialized: " + pack_name());
        *out_runtime = runtime.release();
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) {
    if (!runtime) return;
    try {
        std::unique_ptr<Scheduler> scheduler;
        std::unique_ptr<LlamaEngine> engine;
        {
            std::lock_guard lock(runtime->mutex);
            scheduler = std::move(runtime->scheduler);
            engine = std::move(runtime->engine);
            runtime->model_handle = 0;
        }
        if (scheduler) scheduler->cancel_all_and_wait();
        scheduler.reset();
        engine.reset();
        runtime->dispatcher->stop();
    } catch (...) {}
    delete runtime;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime, llw_capabilities_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out))
            throw std::invalid_argument("invalid capabilities output");
        *out = {};
        out->struct_size = sizeof(*out);
        out->supports_cpu = 1;
        out->supports_cuda = pack_backend() == LLW_BACKEND_CUDA;
        out->supports_vulkan = pack_backend() == LLW_BACKEND_VULKAN;
        out->supports_streaming = 1;
        out->supports_cancellation = 1;
        out->max_parallel_slots = LLW_MAX_SLOTS;
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime, int32_t backend, llw_device_list_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) ||
            backend < LLW_BACKEND_AUTO || backend > LLW_BACKEND_VULKAN)
            throw std::invalid_argument("invalid device list output");
        std::vector<DeviceRecord> devices = enumerate_pack_devices(runtime->backend_directory);
        devices.erase(std::remove_if(devices.begin(), devices.end(), [backend](const DeviceRecord& d) {
            return backend != LLW_BACKEND_AUTO && d.backend != backend;
        }), devices.end());
        out->count = 0;
        out->required_count = devices.size();
        if (devices.empty()) return LLW_OK;
        if (!out->devices || out->capacity < devices.size() ||
            out->element_size < sizeof(llw_device_info_t))
            return fail(error, LLW_ERR_BUFFER_TOO_SMALL, "device buffer is too small");
        for (size_t index = 0; index < devices.size(); ++index) {
            if (out->devices[index].struct_size < sizeof(llw_device_info_t))
                throw std::invalid_argument("device element is undersized");
            llw_device_info_t value{};
            value.struct_size = sizeof(value);
            value.backend = devices[index].backend;
            value.device_index = devices[index].backend_index;
            copy_text(value.id, sizeof(value.id), devices[index].id);
            copy_text(value.name, sizeof(value.name), devices[index].name);
            copy_text(value.vendor, sizeof(value.vendor), devices[index].vendor);
            out->devices[index] = value;
        }
        out->count = static_cast<uint32_t>(devices.size());
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_option_schema(
    llw_runtime_t* runtime, llw_buffer_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            !zeroed(out->reserved)) throw std::invalid_argument("invalid schema output");
        const std::string schema = option_schema();
        out->len = schema.size();
        if (!out->data || out->capacity < schema.size())
            return fail(error, LLW_ERR_BUFFER_TOO_SMALL, "schema buffer is too small");
        std::memcpy(out->data, schema.data(), schema.size());
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_load(
    llw_runtime_t* runtime, const llw_model_load_params_t* params, llw_handle_t* out_model,
    llw_error_t* error) {
    if (out_model) *out_model = 0;
    return guarded(error, [&] {
        if (!runtime || !params || !out_model) throw std::invalid_argument("invalid model load call");
        validate_model(*params);
        std::unique_lock lock(runtime->mutex);
        if (runtime->model_handle != 0 || runtime->model_loading)
            return fail(error, LLW_ERR_BUSY, "a model is already loaded or loading");
        runtime->model_loading = true;
        ModelLoadingReset loading_reset{*runtime, lock};
        llw_handle_t handle = runtime->next_model_handle++;
        if (handle == 0) handle = runtime->next_model_handle++;
        const std::string path(reinterpret_cast<const char*>(params->path_utf8), params->path_len);
#ifdef LLW_RUNTIME_TESTING
        if (path == "llw-test-bad-alloc.gguf") throw std::bad_alloc();
#endif
        ModelConfig config;
        config.path = path;
        config.backend_directory = runtime->backend_directory;
        config.backend = params->backend;
        config.device_index = params->device_index;
        config.slots = runtime->config.slot_count;
        config.context_tokens_per_slot = params->context_tokens_per_slot;
        config.logical_batch_tokens = params->logical_batch_tokens;
        config.physical_batch_tokens = params->physical_batch_tokens;
        config.n_threads = params->n_threads;
        config.n_threads_batch = params->n_threads_batch;
        config.n_gpu_layers = params->n_gpu_layers;
        config.use_mmap = params->use_mmap != 0;
        config.use_mlock = params->use_mlock != 0;
        config.check_tensors = params->check_tensors != 0;
        lock.unlock();
        std::unique_ptr<LlamaEngine> engine;
        std::unique_ptr<Scheduler> scheduler;
        engine = std::make_unique<LlamaEngine>(config, [runtime, handle](float progress) {
            publish_runtime_event(*runtime, LLW_EVENT_MODEL_PROGRESS,
                LLW_EVENT_DATA_JSON_UTF8, handle,
                "{\"progress\":" + std::to_string(progress) + "}");
        });
        scheduler = std::make_unique<Scheduler>(runtime->config.slot_count,
            runtime->config.request_queue_capacity, *engine, *runtime->dispatcher);
        lock.lock();
        runtime->engine = std::move(engine);
        runtime->scheduler = std::move(scheduler);
        runtime->model_handle = handle;
        runtime->model_loading = false;
        loading_reset.release();
        *out_model = handle;
        publish_runtime_event(*runtime, LLW_EVENT_LOG, LLW_EVENT_DATA_UTF8, handle,
                              "model loaded on backend " + std::to_string(config.backend) +
                                  " device " + std::to_string(config.device_index));
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_unload(
    llw_runtime_t* runtime, llw_handle_t model, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || model == 0) throw std::invalid_argument("invalid model unload call");
        std::unique_ptr<Scheduler> scheduler;
        std::unique_ptr<LlamaEngine> engine;
        {
            std::lock_guard lock(runtime->mutex);
            if (runtime->model_handle != model)
                return fail(error, LLW_ERR_NOT_FOUND, "model handle was not found");
            scheduler = std::move(runtime->scheduler);
            engine = std::move(runtime->engine);
            runtime->model_handle = 0;
        }
        scheduler->cancel_all_and_wait();
        scheduler.reset();
        engine.reset();
        runtime->dispatcher->flush();
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_submit(
    llw_runtime_t* runtime, const llw_request_params_t* params, llw_handle_t* out_request,
    llw_error_t* error) {
    if (out_request) *out_request = 0;
    return guarded(error, [&] {
        if (!runtime || !params || !out_request)
            throw std::invalid_argument("invalid request submit call");
        validate_request(*params);
        std::lock_guard lock(runtime->mutex);
        if (!runtime->scheduler || runtime->model_handle != params->model_handle)
            return fail(error, LLW_ERR_INVALID_STATE, "requested model is not loaded");
        std::string message;
        const llw_result_t result = runtime->scheduler->submit(*params, *out_request, message);
        return result == LLW_OK ? LLW_OK : fail(error, result, message);
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_cancel(
    llw_runtime_t* runtime, llw_handle_t request, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || request == 0) throw std::invalid_argument("invalid request cancel call");
        std::lock_guard lock(runtime->mutex);
        if (!runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        std::string message;
        const llw_result_t result = runtime->scheduler->cancel(request, message);
        return result == LLW_OK ? LLW_OK : fail(error, result, message);
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_scheduler_snapshot(
    llw_runtime_t* runtime, llw_scheduler_snapshot_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out))
            throw std::invalid_argument("invalid scheduler snapshot output");
        std::lock_guard lock(runtime->mutex);
        if (!runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        *out = runtime->scheduler->snapshot();
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_metrics(
    llw_runtime_t* runtime, llw_metrics_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out))
            throw std::invalid_argument("invalid metrics output");
        std::lock_guard lock(runtime->mutex);
        if (!runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        *out = runtime->scheduler->metrics();
        return LLW_OK;
    });
}
```

The following schema key reference uses `CPU|CUDA|VULKAN` only as compact documentation notation for the three concrete strings emitted by the complete `option_schema()` code above; it is not copied into source:

```json
{"abiMinor":1,"backendPack":"CPU|CUDA|VULKAN","model":{"modelPath":{"type":"utf8Bytes","minBytes":1,"maxBytes":32768,"default":null,"apply":"modelReload"},"backend":{"type":"enum","values":{"auto":0,"cpu":1,"cuda":2,"vulkan":3},"default":0,"apply":"modelReload"},"deviceIndex":{"type":"uint32","min":0,"max":255,"default":0,"apply":"modelReload"},"contextTokensPerSlot":{"type":"uint32","min":512,"max":262144,"default":4096,"apply":"modelReload"},"logicalBatchTokens":{"type":"uint32","min":1,"max":8192,"default":512,"apply":"modelReload"},"physicalBatchTokens":{"type":"uint32","min":1,"maxField":"logicalBatchTokens","default":128,"apply":"modelReload"},"nThreads":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nThreadsBatch":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nGpuLayers":{"type":"int32","min":-1,"max":65535,"default":0,"apply":"modelReload"},"useMmap":{"type":"boolean","default":true,"apply":"modelReload"},"useMlock":{"type":"boolean","default":false,"apply":"modelReload"},"checkTensors":{"type":"boolean","default":false,"apply":"modelReload"}},"scheduler":{"slotCount":{"type":"uint32","min":1,"max":4,"default":1,"apply":"runtimeRestart"},"requestQueueCapacity":{"type":"uint32","min":1,"max":1024,"default":16,"apply":"runtimeRestart"},"eventQueueCapacity":{"type":"uint32","min":16,"max":65536,"default":1024,"apply":"runtimeRestart"}},"request":{"promptBytes":{"type":"bytes","minBytes":1,"maxBytes":16777216,"default":null,"apply":"nextRequest"},"maxNewTokens":{"type":"uint32","min":1,"max":1048576,"default":256,"apply":"nextRequest"},"seed":{"type":"uint32","min":0,"max":4294967295,"default":4294967295,"apply":"nextRequest"},"temperature":{"type":"float32","min":0.0,"max":10.0,"default":0.8,"apply":"nextRequest"},"topK":{"type":"int32","min":0,"max":100000,"default":40,"apply":"nextRequest"},"topP":{"type":"float32","min":0.0,"max":1.0,"default":0.95,"apply":"nextRequest"},"minP":{"type":"float32","min":0.0,"max":1.0,"default":0.05,"apply":"nextRequest"},"repeatLastN":{"type":"int32","min":0,"max":262144,"default":64,"apply":"nextRequest"},"repeatPenalty":{"type":"float32","min":0.0,"max":10.0,"default":1.1,"apply":"nextRequest"},"frequencyPenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"presencePenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"stopSequences":{"type":"bytesArray","minCount":0,"maxCount":8,"minBytesEach":1,"maxBytesEach":256,"maxTotalBytes":2048,"default":[],"apply":"nextRequest"}}}
```

- [ ] **Step 4: Verify exports and native tests**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
cmake --install .cmake-build/llm-cpu --config Debug --prefix .runtime-packs/cpu-debug
$pack = (Resolve-Path '.runtime-packs/cpu-debug').Path
$required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll'
$missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
if ($missing) { throw "CPU pack is missing: $($missing -join ', ')" }
$unexpected = 'ggml-cuda.dll','ggml-vulkan.dll' | Where-Object { Test-Path (Join-Path $pack $_) }
if ($unexpected) { throw "CPU pack contains another pack backend: $($unexpected -join ', ')" }
Get-ChildItem -File $pack | Sort-Object Name | Select-Object Name,Length
$exports = dumpbin /exports (Join-Path $pack 'local_llm_runtime.dll') | Select-String ' llw_'
$exports.Count
$exports
```

Expected: all non-model native tests pass and export count is exactly 14, one occurrence for each declared function.

- [ ] **Step 5: Commit the ABI facade**

```powershell
git add native/llm-runtime
git commit -m "feat: expose native model and request lifecycle"
```

### Task 8: Load The New Exports And Copy Callback Data In Rust

**Files:**
- Modify: `crates/llm-runtime-sys/src/lib.rs`
- Modify: `crates/llm-runtime/Cargo.toml`
- Modify: `crates/llm-runtime/src/lib.rs`

- [ ] **Step 1: Write failing raw-loader and callback-copy tests**

Add this test to `crates/llm-runtime-sys/src/lib.rs`'s existing test module:

```rust
#[test]
fn loader_names_each_required_export_once() {
    let source = include_str!("lib.rs");
    let loader = source.split("#[cfg(test)]").next().expect("loader source");
    for symbol in [
        "llw_get_abi_info\\0", "llw_runtime_version\\0", "llw_llama_cpp_commit\\0",
        "llw_runtime_create\\0", "llw_runtime_destroy\\0",
        "llw_runtime_get_capabilities\\0", "llw_runtime_list_devices\\0",
        "llw_runtime_get_option_schema\\0", "llw_model_load\\0", "llw_model_unload\\0",
        "llw_request_submit\\0", "llw_request_cancel\\0",
        "llw_get_scheduler_snapshot\\0", "llw_get_metrics\\0",
    ] {
        assert_eq!(loader.matches(symbol).count(), 1, "symbol count for {symbol}");
    }
}
```

Add these tests to `crates/llm-runtime/src/lib.rs`'s existing test module after Task 8 Step 3 adds `CallbackState`:

```rust
fn callback_state(capacity: usize, max_outstanding: usize)
    -> (CallbackState, Receiver<RuntimeEvent>, Receiver<u64>) {
    let (regular_sender, regular) = crossbeam_channel::bounded(capacity);
    let (cancel_sender, cancel_receiver) = crossbeam_channel::bounded(max_outstanding);
    (CallbackState { regular_sender, registry: Mutex::new(RequestRegistry::default()),
        cancel_sender: Mutex::new(Some(cancel_sender)), max_outstanding,
        invariant_violations: Arc::new(AtomicUsize::new(0)), test_hook: None },
     regular, cancel_receiver)
}

fn raw_event(event_type: i32, request: u64, sequence: u64, data: &[u8]) -> sys::Event {
    sys::Event { struct_size: std::mem::size_of::<sys::Event>() as u32,
        flags: if event_type == sys::EVENT_TOKEN { sys::EVENT_DATA_BYTES }
            else { sys::EVENT_DATA_JSON_UTF8 }, event_type, error_code: 0,
        model_handle: 1, request_handle: request, slot_id: 0, reserved0: 0,
        sequence_number: sequence, data: data.as_ptr(), data_len: data.len() as u64,
        request_user_data: std::ptr::null_mut(), reserved: [0; 8] }
}

fn invoke(state: &CallbackState, event: &sys::Event) {
    unsafe { event_trampoline(event, (state as *const CallbackState).cast_mut().cast()) };
}

#[test]
fn callback_copies_stack_backed_payload_before_return() {
    let (state, events, _cancellations) = callback_state(4, 2);
    let mut payload = [0xf0, 0x9f, 0x92, 0xa1];
    let event = raw_event(sys::EVENT_TOKEN, 2, 2, &payload);
    invoke(&state, &event);
    payload.fill(0);
    assert_eq!(events.recv_timeout(Duration::from_secs(1)).unwrap().payload,
               vec![0xf0, 0x9f, 0x92, 0xa1]);
}

#[test]
fn callback_contains_panics_from_test_consumer() {
    let (mut state, _events, _cancellations) = callback_state(1, 1);
    state.test_hook = Some(Arc::new(|_| panic!("test panic")));
    let event = raw_event(sys::EVENT_DONE, 2, 3, &[]);
    let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        event_trampoline(&event, (&state as *const CallbackState).cast_mut().cast())
    }));
    assert!(escaped.is_ok());
}

#[test]
fn terminal_before_registration_is_atomic_and_removed() {
    let (state, _events, _cancellations) = callback_state(2, 2);
    invoke(&state, &raw_event(sys::EVENT_DONE, 7, 2, &[]));
    let request_state = Arc::new(RequestState::default());
    let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
    state.register(7, terminal_sender, request_state.clone());
    assert!(request_state.native_done.load(Ordering::Acquire));
    assert_eq!(terminal.recv_timeout(Duration::from_secs(1)).unwrap().request_handle, 7);
    let registry = state.registry.lock().unwrap();
    assert!(registry.entries.is_empty());
}

#[test]
fn overflow_worker_cancels_and_native_terminal_cleans_without_duplicate() {
    let (state, events, cancellations) = callback_state(1, 2);
    assert_eq!(state.regular_sender.capacity(), Some(1));
    assert_eq!(cancellations.capacity(), Some(2));
    let request_state = Arc::new(RequestState::default());
    let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
    state.register(9, terminal_sender, request_state.clone());
    let (cancelled_sender, cancelled) = crossbeam_channel::bounded(1);
    let worker = std::thread::spawn(move || run_cancellation_worker(cancellations,
        move |handle| { let _ = cancelled_sender.try_send(handle); }));
    invoke(&state, &raw_event(sys::EVENT_QUEUED, 9, 1, &[]));
    invoke(&state, &raw_event(sys::EVENT_TOKEN, 9, 2, b"a"));
    invoke(&state, &raw_event(sys::EVENT_TOKEN, 9, 3, b"b"));
    assert!(request_state.delivery_failed.load(Ordering::Acquire));
    assert!(!request_state.native_done.load(Ordering::Acquire));
    assert!(request_state.native_cancel_requested.load(Ordering::Acquire));
    let first = events.recv_timeout(Duration::from_secs(1)).unwrap();
    let overflow = terminal.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(first.kind, EventKind::Queued);
    assert_eq!(overflow.kind, EventKind::Error);
    assert!(String::from_utf8_lossy(&overflow.payload).contains("rustEventOverflow"));
    assert_eq!(cancelled.recv_timeout(Duration::from_secs(1)).unwrap(), 9);
    invoke(&state, &raw_event(sys::EVENT_DONE, 9, 4, &[]));
    assert!(request_state.native_done.load(Ordering::Acquire));
    assert!(terminal.recv_timeout(Duration::from_millis(20)).is_err());
    let registry = state.registry.lock().unwrap();
    assert!(registry.entries.is_empty());
    drop(registry);
    state.close_cancel_queue();
    worker.join().unwrap();
    assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
}

#[test]
fn abandoned_terminal_receiver_never_blocks_or_retains_registry() {
    let (state, _events, _cancellations) = callback_state(1, 1);
    let request_state = Arc::new(RequestState::default());
    let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
    state.register(1, terminal_sender, request_state);
    drop(terminal);
    invoke(&state, &raw_event(sys::EVENT_DONE, 1, 2, &[]));
    assert!(state.registry.lock().unwrap().entries.is_empty());
}

#[test]
fn abandoned_regular_receiver_reports_overflow_and_queues_cancel() {
    let (state, events, cancellations) = callback_state(1, 1);
    drop(events);
    let request_state = Arc::new(RequestState::default());
    let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
    state.register(4, terminal_sender, request_state.clone());
    invoke(&state, &raw_event(sys::EVENT_TOKEN, 4, 2, b"lost"));
    let overflow = terminal.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(overflow.kind, EventKind::Error);
    assert!(request_state.delivery_failed.load(Ordering::Acquire));
    assert!(!request_state.native_done.load(Ordering::Acquire));
    assert_eq!(cancellations.recv_timeout(Duration::from_secs(1)).unwrap(), 4);
    invoke(&state, &raw_event(sys::EVENT_CANCELLED, 4, 3, &[]));
    assert!(request_state.native_done.load(Ordering::Acquire));
    assert!(state.registry.lock().unwrap().entries.is_empty());
}

#[test]
fn sequential_terminals_exceed_max_without_shared_queue_loss() {
    let (state, _events, _cancellations) = callback_state(2, 4);
    let mut terminals = Vec::new();
    for handle in 1..=100 {
        let request_state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        state.register(handle, terminal_sender, request_state.clone());
        invoke(&state, &raw_event(sys::EVENT_DONE, handle, 1, &[]));
        assert!(request_state.native_done.load(Ordering::Acquire));
        assert!(state.registry.lock().unwrap().entries.is_empty());
        terminals.push((handle, terminal));
    }
    for (handle, terminal) in terminals {
        assert_eq!(terminal.recv_timeout(Duration::from_secs(1)).unwrap().request_handle, handle);
    }
    assert_eq!(state.invariant_violations.load(Ordering::Relaxed), 0);
}
```

Run:

```powershell
cargo test -p llm-runtime --all-targets
```

Expected: FAIL because model/request/event APIs do not exist.

- [ ] **Step 2: Load each new export exactly once**

Replace `Api` and its loader with this complete fourteen-export implementation. All aliases are defined in Task 2 before this block is used:

```rust
pub struct Api {
    _library: libloading::Library,
    pub get_abi_info: GetAbiInfoFn,
    pub runtime_version: RuntimeVersionFn,
    pub llama_commit: LlamaCommitFn,
    pub runtime_create: RuntimeCreateFn,
    pub runtime_destroy: RuntimeDestroyFn,
    pub runtime_get_capabilities: RuntimeGetCapabilitiesFn,
    pub runtime_list_devices: RuntimeListDevicesFn,
    pub runtime_get_option_schema: RuntimeGetOptionSchemaFn,
    pub model_load: ModelLoadFn,
    pub model_unload: ModelUnloadFn,
    pub request_submit: RequestSubmitFn,
    pub request_cancel: RequestCancelFn,
    pub get_scheduler_snapshot: GetSchedulerSnapshotFn,
    pub get_metrics: GetMetricsFn,
}

impl Api {
    /// # Safety
    ///
    /// `path` must name an LLW ABI 1.x library. Every runtime and handle created through this
    /// object must be destroyed before the object is dropped.
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, libloading::Error> {
        let library = unsafe { libloading::Library::new(path)? };
        let get_abi_info = unsafe { *library.get::<GetAbiInfoFn>(b"llw_get_abi_info\0")? };
        let runtime_version = unsafe {
            *library.get::<RuntimeVersionFn>(b"llw_runtime_version\0")?
        };
        let llama_commit = unsafe {
            *library.get::<LlamaCommitFn>(b"llw_llama_cpp_commit\0")?
        };
        let runtime_create = unsafe {
            *library.get::<RuntimeCreateFn>(b"llw_runtime_create\0")?
        };
        let runtime_destroy = unsafe {
            *library.get::<RuntimeDestroyFn>(b"llw_runtime_destroy\0")?
        };
        let runtime_get_capabilities = unsafe {
            *library.get::<RuntimeGetCapabilitiesFn>(b"llw_runtime_get_capabilities\0")?
        };
        let runtime_list_devices = unsafe {
            *library.get::<RuntimeListDevicesFn>(b"llw_runtime_list_devices\0")?
        };
        let runtime_get_option_schema = unsafe {
            *library.get::<RuntimeGetOptionSchemaFn>(b"llw_runtime_get_option_schema\0")?
        };
        let model_load = unsafe { *library.get::<ModelLoadFn>(b"llw_model_load\0")? };
        let model_unload = unsafe { *library.get::<ModelUnloadFn>(b"llw_model_unload\0")? };
        let request_submit = unsafe {
            *library.get::<RequestSubmitFn>(b"llw_request_submit\0")?
        };
        let request_cancel = unsafe {
            *library.get::<RequestCancelFn>(b"llw_request_cancel\0")?
        };
        let get_scheduler_snapshot = unsafe {
            *library.get::<GetSchedulerSnapshotFn>(b"llw_get_scheduler_snapshot\0")?
        };
        let get_metrics = unsafe { *library.get::<GetMetricsFn>(b"llw_get_metrics\0")? };
        Ok(Self {
            _library: library,
            get_abi_info,
            runtime_version,
            llama_commit,
            runtime_create,
            runtime_destroy,
            runtime_get_capabilities,
            runtime_list_devices,
            runtime_get_option_schema,
            model_load,
            model_unload,
            request_submit,
            request_cancel,
            get_scheduler_snapshot,
            get_metrics,
        })
    }
}
```

- [ ] **Step 3: Add safe owned types and callback delivery**

Apply these exact dependency table edits:

```toml
# Cargo.toml [workspace.dependencies]
crossbeam-channel = "0.5"
libloading = "0.8"
serde = { version = "1", features = ["derive"] }
thiserror = "2"
```

```toml
# crates/llm-runtime/Cargo.toml [dependencies]
crossbeam-channel.workspace = true
libloading.workspace = true
llm-runtime-sys = { path = "../llm-runtime-sys" }
thiserror.workspace = true
```

Add these imports and complete owned type definitions to `crates/llm-runtime/src/lib.rs` after the existing probe types:

```rust
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub slot_count: u32,
    pub request_queue_capacity: u32,
    pub event_queue_capacity: u32,
}
impl Default for RuntimeOptions {
    fn default() -> Self {
        Self { slot_count: 1, request_queue_capacity: 16, event_queue_capacity: 1024 }
    }
}

#[derive(Debug, Clone)]
pub struct ModelOptions {
    pub backend: Backend,
    pub device_index: u32,
    pub context_tokens_per_slot: u32,
    pub logical_batch_tokens: u32,
    pub physical_batch_tokens: u32,
    pub n_threads: i32,
    pub n_threads_batch: i32,
    pub n_gpu_layers: i32,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub check_tensors: bool,
}
impl Default for ModelOptions {
    fn default() -> Self {
        Self { backend: Backend::Auto, device_index: 0, context_tokens_per_slot: 4096,
            logical_batch_tokens: 512, physical_batch_tokens: 128, n_threads: 8,
            n_threads_batch: 8, n_gpu_layers: 0, use_mmap: true, use_mlock: false,
            check_tensors: false }
    }
}

#[derive(Debug, Clone)]
pub struct GenerationOptions {
    pub max_new_tokens: u32,
    pub seed: u32,
    pub temperature: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub min_p: f32,
    pub repeat_last_n: i32,
    pub repeat_penalty: f32,
    pub frequency_penalty: f32,
    pub presence_penalty: f32,
    pub stop_sequences: Vec<Vec<u8>>,
}
impl Default for GenerationOptions {
    fn default() -> Self {
        Self { max_new_tokens: 256, seed: u32::MAX, temperature: 0.8, top_k: 40,
            top_p: 0.95, min_p: 0.05, repeat_last_n: 64, repeat_penalty: 1.1,
            frequency_penalty: 0.0, presence_penalty: 0.0, stop_sequences: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind { ModelProgress, Queued, Token, Metrics, Done, Cancelled, Error, Log }

#[derive(Debug, Clone)]
pub struct RuntimeEvent {
    pub kind: EventKind,
    pub data_format: u32,
    pub error_code: i32,
    pub model_handle: u64,
    pub request_handle: u64,
    pub slot_id: u32,
    pub sequence_number: u64,
    pub request_user_data: usize,
    pub payload: Vec<u8>,
}
impl RuntimeEvent {
    fn from_raw(raw: &sys::Event, payload: Vec<u8>) -> Option<Self> {
        let kind = match raw.event_type {
            1 => EventKind::ModelProgress, 2 => EventKind::Queued, 3 => EventKind::Token,
            4 => EventKind::Metrics, 5 => EventKind::Done, 6 => EventKind::Cancelled,
            7 => EventKind::Error, 8 => EventKind::Log, _ => return None,
        };
        Some(Self { kind, data_format: raw.flags, error_code: raw.error_code,
            model_handle: raw.model_handle, request_handle: raw.request_handle,
            slot_id: raw.slot_id, sequence_number: raw.sequence_number,
            request_user_data: raw.request_user_data as usize, payload })
    }
    fn terminal(&self) -> bool {
        matches!(self.kind, EventKind::Done | EventKind::Cancelled | EventKind::Error)
    }
    fn overflow_error(dropped: &Self) -> Self {
        Self { kind: EventKind::Error, data_format: sys::EVENT_DATA_JSON_UTF8,
            error_code: sys::ERR_INTERNAL, model_handle: dropped.model_handle,
            request_handle: dropped.request_handle, slot_id: dropped.slot_id,
            sequence_number: dropped.sequence_number,
            request_user_data: dropped.request_user_data,
            payload: br#"{"state":"error","reason":"rustEventOverflow"}"#.to_vec() }
    }
    fn invariant_error(handle: u64) -> Self {
        Self { kind: EventKind::Error, data_format: sys::EVENT_DATA_JSON_UTF8,
            error_code: sys::ERR_INTERNAL, model_handle: 0, request_handle: handle,
            slot_id: u32::MAX, sequence_number: 0, request_user_data: 0,
            payload: br#"{"state":"error","reason":"requestRegistryInvariant"}"#.to_vec() }
    }
}

#[derive(Default)]
struct RequestState {
    native_done: AtomicBool,
    delivery_failed: AtomicBool,
    native_cancel_requested: AtomicBool,
}

enum TerminalRoute {
    Unregistered,
    Sender(Sender<RuntimeEvent>),
    Early(RuntimeEvent),
    Delivered,
}

struct RegistryEntry {
    route: TerminalRoute,
    state: Option<Arc<RequestState>>,
    native_done: bool,
    delivery_failed: bool,
    cancel_queued: bool,
}
impl Default for RegistryEntry {
    fn default() -> Self {
        Self { route: TerminalRoute::Unregistered, state: None, native_done: false,
            delivery_failed: false, cancel_queued: false }
    }
}

#[derive(Default)]
struct RequestRegistry { entries: HashMap<u64, RegistryEntry> }

struct CallbackState {
    regular_sender: Sender<RuntimeEvent>,
    registry: Mutex<RequestRegistry>,
    cancel_sender: Mutex<Option<Sender<u64>>>,
    max_outstanding: usize,
    invariant_violations: Arc<AtomicUsize>,
    test_hook: Option<Arc<dyn Fn(&RuntimeEvent) + Send + Sync>>,
}

impl CallbackState {
    fn send_or_store(&self, route: &mut TerminalRoute, event: RuntimeEvent) -> bool {
        match std::mem::replace(route, TerminalRoute::Delivered) {
            TerminalRoute::Sender(sender) => {
                if matches!(sender.try_send(event), Err(TrySendError::Full(_))) {
                    self.invariant_violations.fetch_add(1, Ordering::Relaxed);
                }
                true
            }
            TerminalRoute::Unregistered => {
                *route = TerminalRoute::Early(event);
                false
            }
            TerminalRoute::Early(existing) => {
                *route = TerminalRoute::Early(existing);
                false
            }
            TerminalRoute::Delivered => true,
        }
    }

    fn register(&self, handle: u64, sender: Sender<RuntimeEvent>, state: Arc<RequestState>) {
        let mut registry = self.registry.lock().expect("request registry poisoned");
        if !registry.entries.contains_key(&handle) && registry.entries.len() >= self.max_outstanding {
            self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            state.native_done.store(true, Ordering::Release);
            let _ = sender.try_send(RuntimeEvent::invariant_error(handle));
            return;
        }
        let entry = registry.entries.entry(handle).or_default();
        entry.state = Some(state.clone());
        state.native_done.store(entry.native_done, Ordering::Release);
        state.delivery_failed.store(entry.delivery_failed, Ordering::Release);
        state.native_cancel_requested.store(entry.cancel_queued, Ordering::Release);
        match std::mem::replace(&mut entry.route, TerminalRoute::Sender(sender)) {
            TerminalRoute::Early(event) => {
                if let TerminalRoute::Sender(sender) =
                    std::mem::replace(&mut entry.route, TerminalRoute::Delivered) {
                    let _ = sender.try_send(event);
                }
            }
            TerminalRoute::Unregistered => {}
            TerminalRoute::Sender(existing) => {
                entry.route = TerminalRoute::Sender(existing);
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            }
            TerminalRoute::Delivered => entry.route = TerminalRoute::Delivered,
        }
        if entry.native_done {
            registry.entries.remove(&handle);
        }
    }

    fn native_terminal(&self, event: RuntimeEvent) {
        let handle = event.request_handle;
        let mut registry = self.registry.lock().expect("request registry poisoned");
        if !registry.entries.contains_key(&handle) && registry.entries.len() >= self.max_outstanding {
            self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let entry = registry.entries.entry(handle).or_default();
        entry.native_done = true;
        if let Some(state) = &entry.state {
            state.native_done.store(true, Ordering::Release);
        }
        if !entry.delivery_failed {
            self.send_or_store(&mut entry.route, event);
        }
        if entry.state.is_some() {
            registry.entries.remove(&handle);
        }
    }

    fn overflow(&self, dropped: RuntimeEvent) {
        let handle = dropped.request_handle;
        let mut queue_cancel = false;
        {
            let mut registry = self.registry.lock().expect("request registry poisoned");
            if !registry.entries.contains_key(&handle) &&
                registry.entries.len() >= self.max_outstanding {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
                return;
            }
            let entry = registry.entries.entry(handle).or_default();
            if entry.delivery_failed { return; }
            entry.delivery_failed = true;
            if let Some(state) = &entry.state {
                state.delivery_failed.store(true, Ordering::Release);
            }
            let overflow = RuntimeEvent::overflow_error(&dropped);
            self.send_or_store(&mut entry.route, overflow);
            if !entry.cancel_queued {
                entry.cancel_queued = true;
                queue_cancel = match &entry.state {
                    Some(state) =>
                        !state.native_cancel_requested.swap(true, Ordering::AcqRel),
                    None => true,
                };
            }
        }
        if queue_cancel {
            let sender = self.cancel_sender.lock().expect("cancel sender poisoned").clone();
            match sender.map(|sender| sender.try_send(handle)) {
                Some(Ok(())) => {}
                Some(Err(TrySendError::Full(_))) | Some(Err(TrySendError::Disconnected(_))) |
                None => {
                    self.invariant_violations.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    fn deliver(&self, event: RuntimeEvent) {
        if event.terminal() && event.request_handle != 0 {
            self.native_terminal(event);
            return;
        }
        if event.request_handle != 0 {
            let registry = self.registry.lock().expect("request registry poisoned");
            if registry.entries.get(&event.request_handle)
                .is_some_and(|entry| entry.delivery_failed) { return; }
        }
        match self.regular_sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(dropped) | TrySendError::Disconnected(dropped))
                if dropped.request_handle != 0 => self.overflow(dropped),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.invariant_violations.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn close_cancel_queue(&self) {
        self.cancel_sender.lock().expect("cancel sender poisoned").take();
    }
}

unsafe extern "C" fn event_trampoline(event: *const sys::Event, user_data: *mut c_void) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(raw) = (unsafe { event.as_ref() }) else { return };
        if raw.struct_size < std::mem::size_of::<sys::Event>() as u32 { return; }
        let payload = if raw.data.is_null() || raw.data_len == 0 {
            Vec::new()
        } else {
            let Ok(len) = usize::try_from(raw.data_len) else { return };
            unsafe { std::slice::from_raw_parts(raw.data, len) }.to_vec()
        };
        let state = unsafe { &*(user_data.cast::<CallbackState>()) };
        let Some(event) = RuntimeEvent::from_raw(raw, payload) else { return };
        if let Some(hook) = &state.test_hook { hook(&event); }
        state.deliver(event);
    }));
}

fn run_cancellation_worker<F>(receiver: Receiver<u64>, mut cancel: F)
where F: FnMut(u64) {
    while let Ok(handle) = receiver.recv() {
        cancel(handle);
    }
}

struct RuntimeInner {
    api: sys::Api,
    runtime: *mut sys::Runtime,
    callback_state: Box<CallbackState>,
    call_lock: Mutex<()>,
    cancel_worker: Option<std::thread::JoinHandle<()>>,
}
// RuntimeInner deliberately has no Send or Sync implementation. Arc ownership keeps the native
// runtime alive through every Model and Request, and call_lock serializes calls on its owner thread.
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        self.callback_state.close_cancel_queue();
        if let Some(worker) = self.cancel_worker.take() {
            let _ = worker.join();
        }
        let _guard = self.call_lock.lock().expect("runtime call lock poisoned");
        if !self.runtime.is_null() {
            unsafe { (self.api.runtime_destroy)(self.runtime) };
            self.runtime = std::ptr::null_mut();
        }
    }
}

pub struct InferenceRuntime { inner: Arc<RuntimeInner>, events: Receiver<RuntimeEvent> }
struct ModelState { runtime: Arc<RuntimeInner>, handle: u64 }
impl Drop for ModelState {
    fn drop(&mut self) {
        let _guard = self.runtime.call_lock.lock().expect("runtime call lock poisoned");
        let mut error = sys::Error::default();
        unsafe { (self.runtime.api.model_unload)(self.runtime.runtime, self.handle, &mut error) };
    }
}
pub struct Model { state: Arc<ModelState> }
pub struct RequestStream {
    model: Arc<ModelState>,
    handle: u64,
    state: Arc<RequestState>,
    terminal: Receiver<RuntimeEvent>,
}
impl RequestStream {
    fn request_native_cancel(&self) -> Result<(), Error> {
        if self.state.native_done.load(Ordering::Acquire) ||
            self.state.native_cancel_requested.swap(true, Ordering::AcqRel) { return Ok(()); }
        let _guard = self.model.runtime.call_lock.lock().expect("runtime call lock poisoned");
        let mut error = sys::Error::default();
        check_result(unsafe { (self.model.runtime.api.request_cancel)(
            self.model.runtime.runtime, self.handle, &mut error) }, &error)
    }
    pub fn handle(&self) -> u64 { self.handle }
    pub fn terminal_receiver(&self) -> Receiver<RuntimeEvent> { self.terminal.clone() }
    pub fn recv_terminal_timeout(&self, timeout: Duration)
        -> Result<RuntimeEvent, RecvTimeoutError> { self.terminal.recv_timeout(timeout) }
    pub fn delivery_failed(&self) -> bool {
        self.state.delivery_failed.load(Ordering::Acquire)
    }
    pub fn cancel(&self) -> Result<(), Error> { self.request_native_cancel() }
}
impl Drop for RequestStream {
    fn drop(&mut self) {
        let _ = self.request_native_cancel();
    }
}
```

`RuntimeInner` closes the cancellation queue, joins its worker, and only then destroys the native
runtime. `RequestStream` holds `Arc<ModelState>`, so model unload occurs only after the `Model` and all
request streams are dropped.

The global Rust channel contains only nonterminal events and is bounded to `event_queue_capacity`.
Every accepted request owns a private `bounded(1)` terminal one-shot. Under one registry mutex, a
handle contains either its one-shot sender or one early terminal event until registration. Native
terminal handling takes the sender, performs one nonblocking send, marks `native_done`, and removes
the entry; an abandoned receiver cannot block or retain registry state. On regular-channel overflow,
`delivery_failed` becomes true, exactly one synthetic `rustEventOverflow` error is sent through that
request's one-shot, and the handle is queued once to a cancellation worker bounded by the maximum
native outstanding request count. The callback never invokes native cancellation. The worker owns
runtime lifetime, calls `llw_request_cancel` outside callback context, and is joined before destroy.
The later native terminal marks `native_done` and removes internal state without a second user
terminal. Explicit cancel and `Drop` consult `native_done` and `native_cancel_requested`, never the
user-visible overflow terminal.

- [ ] **Step 4: Implement safe load, submit, cancel, snapshot, and metrics methods**

Add these complete method implementations after the types above. `InferenceRuntime::load` remains unsafe, like the existing probe loader; application code must first resolve a project-managed pack ID.

```rust
impl InferenceRuntime {
    /// # Safety
    /// `path` must be a trusted project-managed runtime pack DLL implementing LLW ABI 1.1.
    pub unsafe fn load(path: &Path, options: RuntimeOptions) -> Result<Self, Error> {
        if !(1..=4).contains(&options.slot_count) ||
            !(1..=1024).contains(&options.request_queue_capacity) ||
            !(16..=65536).contains(&options.event_queue_capacity) {
            return Err(Error::Runtime { code: sys::ERR_INVALID_ARGUMENT,
                message: "runtime queue options are outside ABI bounds".into() });
        }
        let api = unsafe { sys::Api::load(path)? };
        let query = sys::AbiQuery::default();
        let mut info = sys::AbiInfo::default();
        let mut raw_error = sys::Error::default();
        check_result(unsafe { (api.get_abi_info)(&query, &mut info, &mut raw_error) }, &raw_error)?;
        if info.abi_major != sys::ABI_MAJOR {
            return Err(Error::AbiMismatch { expected: sys::ABI_MAJOR, actual: info.abi_major });
        }
        if info.abi_minor < 1 {
            return Err(Error::Runtime { code: sys::ERR_UNSUPPORTED,
                message: "runtime does not support inference ABI 1.1".into() });
        }
        let max_outstanding = usize::try_from(options.slot_count + options.request_queue_capacity)
            .expect("runtime bounds fit usize");
        let regular_capacity = usize::try_from(options.event_queue_capacity)
            .expect("runtime bounds fit usize");
        let (regular_sender, events) = crossbeam_channel::bounded(regular_capacity);
        let (cancel_sender, cancel_receiver) = crossbeam_channel::bounded(max_outstanding);
        let invariant_violations = Arc::new(AtomicUsize::new(0));
        let mut callback_state = Box::new(CallbackState {
            regular_sender, registry: Mutex::new(RequestRegistry::default()),
            cancel_sender: Mutex::new(Some(cancel_sender)), max_outstanding,
            invariant_violations: invariant_violations.clone(), test_hook: None });
        let callbacks = sys::CallbackTable { struct_size: std::mem::size_of::<sys::CallbackTable>() as u32,
            flags: 0, on_event: Some(event_trampoline),
            user_data: (&mut *callback_state as *mut CallbackState).cast(), reserved: [0; 8] };
        let create = sys::RuntimeCreateParams { struct_size: std::mem::size_of::<sys::RuntimeCreateParams>() as u32,
            flags: 0, callbacks, reserved: [0; 8],
            scheduler: sys::SchedulerConfig { struct_size: std::mem::size_of::<sys::SchedulerConfig>() as u32,
                flags: 0, slot_count: options.slot_count,
                request_queue_capacity: options.request_queue_capacity,
                event_queue_capacity: options.event_queue_capacity, reserved0: 0, reserved: [0; 8] },
            reserved_v1: [0; 8] };
        let mut runtime = std::ptr::null_mut();
        let mut raw_error = sys::Error::default();
        let code = unsafe { (api.runtime_create)(&create, &mut runtime, &mut raw_error) };
        let runtime = finish_runtime_create(code, runtime, &raw_error,
            |value| unsafe { (api.runtime_destroy)(value) })?;
        let runtime_address = runtime as usize;
        let cancel = api.request_cancel;
        let worker_violations = invariant_violations.clone();
        let cancel_worker = std::thread::spawn(move || run_cancellation_worker(
            cancel_receiver, move |handle| {
                let runtime = runtime_address as *mut sys::Runtime;
                let mut error = sys::Error::default();
                let result = unsafe { cancel(runtime, handle, &mut error) };
                if result != sys::OK && result != sys::ERR_NOT_FOUND &&
                    result != sys::ERR_INVALID_STATE {
                    worker_violations.fetch_add(1, Ordering::Relaxed);
                }
            }));
        Ok(Self { inner: Arc::new(RuntimeInner { api, runtime, callback_state,
            call_lock: Mutex::new(()), cancel_worker: Some(cancel_worker) }), events })
    }

    pub fn events(&self) -> Receiver<RuntimeEvent> { self.events.clone() }

    pub fn load_model(&self, path: &Path, options: ModelOptions) -> Result<Model, Error> {
        let canonical = path.canonicalize().map_err(|error| Error::Runtime {
            code: -1, message: format!("failed to canonicalize model path: {error}") })?;
        let utf8 = canonical.to_str().ok_or_else(|| Error::Runtime {
            code: -1, message: "model path is not representable as UTF-8".into() })?;
        let params = sys::ModelLoadParams { struct_size: std::mem::size_of::<sys::ModelLoadParams>() as u32,
            flags: 0, path_utf8: utf8.as_ptr(), path_len: utf8.len() as u64,
            backend: options.backend.raw(), device_index: options.device_index,
            context_tokens_per_slot: options.context_tokens_per_slot,
            logical_batch_tokens: options.logical_batch_tokens,
            physical_batch_tokens: options.physical_batch_tokens, n_threads: options.n_threads,
            n_threads_batch: options.n_threads_batch, n_gpu_layers: options.n_gpu_layers,
            use_mmap: u32::from(options.use_mmap), use_mlock: u32::from(options.use_mlock),
            check_tensors: u32::from(options.check_tensors), reserved0: 0, reserved: [0; 12] };
        let _guard = self.inner.call_lock.lock().expect("runtime call lock poisoned");
        let mut handle = 0;
        let mut error = sys::Error::default();
        check_result(unsafe { (self.inner.api.model_load)(self.inner.runtime, &params,
            &mut handle, &mut error) }, &error)?;
        Ok(Model { state: Arc::new(ModelState { runtime: self.inner.clone(), handle }) })
    }

    pub fn scheduler_snapshot(&self) -> Result<sys::SchedulerSnapshot, Error> {
        let _guard = self.inner.call_lock.lock().expect("runtime call lock poisoned");
        let mut value = sys::SchedulerSnapshot::default();
        let mut error = sys::Error::default();
        check_result(unsafe { (self.inner.api.get_scheduler_snapshot)(self.inner.runtime,
            &mut value, &mut error) }, &error)?;
        Ok(value)
    }

    pub fn metrics(&self) -> Result<sys::Metrics, Error> {
        let _guard = self.inner.call_lock.lock().expect("runtime call lock poisoned");
        let mut value = sys::Metrics::default();
        let mut error = sys::Error::default();
        check_result(unsafe { (self.inner.api.get_metrics)(self.inner.runtime,
            &mut value, &mut error) }, &error)?;
        Ok(value)
    }
}

impl Model {
    pub fn handle(&self) -> u64 { self.state.handle }

    pub fn submit(&self, prompt: &[u8], options: GenerationOptions) -> Result<RequestStream, Error> {
        let stop_storage = options.stop_sequences;
        let stop_ffi: Vec<sys::Bytes> = stop_storage.iter().map(|stop| sys::Bytes {
            struct_size: std::mem::size_of::<sys::Bytes>() as u32, flags: 0,
            data: stop.as_ptr(), len: stop.len() as u64, reserved: [0; 8] }).collect();
        let params = sys::RequestParams { struct_size: std::mem::size_of::<sys::RequestParams>() as u32,
            flags: 0, model_handle: self.state.handle, prompt: prompt.as_ptr(),
            prompt_len: prompt.len() as u64, max_new_tokens: options.max_new_tokens,
            seed: options.seed, temperature: options.temperature, top_k: options.top_k,
            top_p: options.top_p, min_p: options.min_p, repeat_last_n: options.repeat_last_n,
            repeat_penalty: options.repeat_penalty, frequency_penalty: options.frequency_penalty,
            presence_penalty: options.presence_penalty, stop_count: stop_ffi.len() as u32,
            reserved0: 0, stop_sequences: if stop_ffi.is_empty() { std::ptr::null() }
                else { stop_ffi.as_ptr() }, request_user_data: std::ptr::null_mut(),
            reserved: [0; 12] };
        let _guard = self.state.runtime.call_lock.lock().expect("runtime call lock poisoned");
        let mut handle = 0;
        let mut error = sys::Error::default();
        let state = Arc::new(RequestState::default());
        let (terminal_sender, terminal) = crossbeam_channel::bounded(1);
        check_result(unsafe { (self.state.runtime.api.request_submit)(self.state.runtime.runtime,
            &params, &mut handle, &mut error) }, &error)?;
        self.state.runtime.callback_state.register(handle, terminal_sender, state.clone());
        Ok(RequestStream { model: self.state.clone(), handle, state, terminal })
    }
}
```

- [ ] **Step 5: Run Rust and DLL integration tests**

```powershell
cmake --install .cmake-build/llm-cpu --config Debug --prefix .runtime-packs/cpu-debug
$env:LLW_TEST_RUNTIME = (Resolve-Path '.runtime-packs/cpu-debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Expected: callback copying, bounded overflow handling, atomic terminal registration, panic
containment, existing probe, ABI layout, and native tests pass. Task 9 adds the required
checksum-pinned model-backed request-drop and concurrent-generation integration coverage.

- [ ] **Step 6: Commit the Rust boundary**

```powershell
git add Cargo.toml Cargo.lock crates/llm-runtime-sys crates/llm-runtime
git commit -m "feat: wrap native inference callbacks safely"
```

### Task 9: Add A Reproducible Required Tiny GGUF CPU Test

**Files:**
- Create: `native/llm-runtime/tests/fixtures/model.json`
- Create: `scripts/acquire-test-model.ps1`
- Create: `native/llm-runtime/tests/runtime_backend_test.cpp`
- Modify: `native/llm-runtime/CMakeLists.txt`
- Create: `crates/llm-runtime/tests/native_runtime.rs`
- Modify: `.gitignore`

- [ ] **Step 1: Record the non-redistributed fixture**

Create `native/llm-runtime/tests/fixtures/model.json`:

```json
{
  "name": "tiny-random-f16.gguf",
  "repository": "amakhov/tiny-random-llama",
  "repositoryCommit": "fbf68d33cf68a9d1d4b71b3d098ae82c8c14443b",
  "repositoryPath": "gguf/tiny-random-f16.gguf",
  "url": "https://huggingface.co/amakhov/tiny-random-llama/resolve/fbf68d33cf68a9d1d4b71b3d098ae82c8c14443b/gguf/tiny-random-f16.gguf",
  "sha256": "1010fc48b2a1880a01fa5e267eb35bf586e3e3ad5539ff5b0e025e4f63616a82",
  "size": 9083072,
  "license": "Apache-2.0",
  "licenseSource": "https://huggingface.co/api/models/amakhov/tiny-random-llama/revision/fbf68d33cf68a9d1d4b71b3d098ae82c8c14443b",
  "redistribution": "not committed; checksum-acquired by tests"
}
```

The repository does not redistribute the GGUF. During plan editing, the official Hugging Face revision
API returned commit `fbf68d33cf68a9d1d4b71b3d098ae82c8c14443b` and model-card license
`apache-2.0`; the pinned download returned 9,083,072 bytes and SHA-256
`1010fc48b2a1880a01fa5e267eb35bf586e3e3ad5539ff5b0e025e4f63616a82`.

- [ ] **Step 2: Create checksum-verified acquisition**

Create `scripts/acquire-test-model.ps1`:

```powershell
param([string]$Destination = '.test-models/tiny-random-f16.gguf')
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -Raw 'native/llm-runtime/tests/fixtures/model.json' | ConvertFrom-Json
$destinationPath = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Destination))
$directory = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force $directory | Out-Null
if (Test-Path $destinationPath) {
  $existing = Get-Item $destinationPath
  $existingHash = (Get-FileHash -Algorithm SHA256 $destinationPath).Hash.ToLowerInvariant()
  if ($existing.Length -eq [int64]$manifest.size -and $existingHash -eq $manifest.sha256) {
    Write-Output $destinationPath
    exit 0
  }
  Remove-Item -LiteralPath $destinationPath
}
$temporary = "$destinationPath.download"
Invoke-WebRequest -Uri $manifest.url -OutFile $temporary
$file = Get-Item $temporary
if ($file.Length -ne [int64]$manifest.size) { Remove-Item -LiteralPath $temporary; throw "fixture size mismatch" }
$actual = (Get-FileHash -Algorithm SHA256 $temporary).Hash.ToLowerInvariant()
if ($actual -ne $manifest.sha256) { Remove-Item -LiteralPath $temporary; throw "fixture SHA-256 mismatch: $actual" }
Move-Item -Force -LiteralPath $temporary -Destination $destinationPath
Write-Output $destinationPath
```

Append this exact entry to `.gitignore`:

```gitignore
.test-models/
.runtime-packs/
```

- [ ] **Step 3: Write the CPU end-to-end test**

Create `native/llm-runtime/tests/runtime_backend_test.cpp`:

```cpp
#include "llw_runtime.h"
#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "%s:%d failed: %s\n", __FILE__, __LINE__, #condition); return 1; \
} } while (false)

struct RequestResult { std::vector<uint8_t> bytes; uint32_t terminals{}; bool error{}; };
struct Events {
    std::mutex mutex;
    std::condition_variable changed;
    std::map<llw_handle_t, RequestResult> requests;
    bool block_callbacks{};
    bool callback_entered{};
    bool release_callbacks{};
};

void LLW_CALL collect_backend_event(const llw_event_t* event, void* user_data) {
    if (!event) return;
    auto& events = *static_cast<Events*>(user_data);
    {
        std::unique_lock lock(events.mutex);
        if (event->request_handle != 0) {
            RequestResult& result = events.requests[event->request_handle];
            if (event->event_type == LLW_EVENT_TOKEN && event->data)
                result.bytes.insert(result.bytes.end(), event->data, event->data + event->data_len);
            if (event->event_type == LLW_EVENT_DONE || event->event_type == LLW_EVENT_CANCELLED ||
                event->event_type == LLW_EVENT_ERROR) ++result.terminals;
            if (event->event_type == LLW_EVENT_ERROR) result.error = true;
        }
        if (events.block_callbacks && event->event_type == LLW_EVENT_MODEL_PROGRESS) {
            events.callback_entered = true;
            events.changed.notify_all();
            events.changed.wait(lock, [&events] { return events.release_callbacks; });
        }
    }
    events.changed.notify_all();
}

int32_t selected_backend() {
    const char* value = std::getenv("LLW_TEST_BACKEND");
    if (!value || std::string(value) == "CPU") return LLW_BACKEND_CPU;
    if (std::string(value) == "CUDA") return LLW_BACKEND_CUDA;
    if (std::string(value) == "VULKAN") return LLW_BACKEND_VULKAN;
    return -1;
}

llw_request_params_t generation(llw_handle_t model, const std::string& prompt) {
    llw_request_params_t params{};
    params.struct_size = sizeof(params);
    params.model_handle = model;
    params.prompt = reinterpret_cast<const uint8_t*>(prompt.data());
    params.prompt_len = prompt.size();
    params.max_new_tokens = 8;
    params.seed = 7;
    params.temperature = 0;
    params.top_k = 40;
    params.top_p = 0.95f;
    params.min_p = 0.05f;
    params.repeat_last_n = 64;
    params.repeat_penalty = 1.1f;
    return params;
}

int main(int argc, char** argv) {
    CHECK(argc == 2);
    const int32_t backend = selected_backend();
    CHECK(backend >= LLW_BACKEND_CPU && backend <= LLW_BACKEND_VULKAN);
    Events events;
    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    create.callbacks.struct_size = sizeof(create.callbacks);
    create.callbacks.on_event = collect_backend_event;
    create.callbacks.user_data = &events;
    create.scheduler.struct_size = sizeof(create.scheduler);
    create.scheduler.slot_count = 2;
    create.scheduler.request_queue_capacity = 4;
    create.scheduler.event_queue_capacity = 1024;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    llw_runtime_t* runtime{};
    CHECK(llw_runtime_create(&create, &runtime, &error) == LLW_OK);

    const std::string path = argv[1];
    llw_model_load_params_t model_params{};
    model_params.struct_size = sizeof(model_params);
    model_params.path_utf8 = reinterpret_cast<const uint8_t*>(path.data());
    model_params.path_len = path.size();
    model_params.backend = backend;
    model_params.device_index = 0;
    model_params.context_tokens_per_slot = 512;
    model_params.logical_batch_tokens = 128;
    model_params.physical_batch_tokens = 64;
    const unsigned hardware = std::thread::hardware_concurrency();
    model_params.n_threads = static_cast<int32_t>(std::clamp(hardware == 0 ? 1u : hardware, 1u, 8u));
    model_params.n_threads_batch = model_params.n_threads;
    model_params.n_gpu_layers = backend == LLW_BACKEND_CPU ? 0 : -1;
    model_params.use_mmap = 1;
    llw_handle_t model{};
    CHECK(llw_model_load(runtime, &model_params, &model, &error) == LLW_OK);
    CHECK(model != 0);

    const std::string first_prompt = "Once";
    const std::string second_prompt = "The";
    llw_request_params_t first_params = generation(model, first_prompt);
    llw_request_params_t second_params = generation(model, second_prompt);
    llw_handle_t first{};
    llw_handle_t second{};
    CHECK(llw_request_submit(runtime, &first_params, &first, &error) == LLW_OK);
    CHECK(llw_request_submit(runtime, &second_params, &second, &error) == LLW_OK);

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(60);
    for (;;) {
        {
            std::unique_lock lock(events.mutex);
            if (events.requests[first].terminals == 1 && events.requests[second].terminals == 1) break;
            if (events.changed.wait_until(lock, deadline) == std::cv_status::timeout) {
                lock.unlock();
                llw_request_cancel(runtime, first, &error);
                llw_request_cancel(runtime, second, &error);
                CHECK(false);
            }
        }
    }
    {
        std::lock_guard lock(events.mutex);
        CHECK(!events.requests[first].bytes.empty());
        CHECK(!events.requests[second].bytes.empty());
        CHECK(events.requests[first].terminals == 1);
        CHECK(events.requests[second].terminals == 1);
        CHECK(!events.requests[first].error && !events.requests[second].error);
    }

    std::string oversized_prompt;
    oversized_prompt.reserve(32768);
    for (size_t index = 0; index < 32768; ++index)
        oversized_prompt.push_back(static_cast<char>('!' + (index % 90)));
    const std::string isolated_prompt = "Healthy peer";
    llw_request_params_t oversized_params = generation(model, oversized_prompt);
    llw_request_params_t isolated_params = generation(model, isolated_prompt);
    llw_handle_t oversized{}, isolated{};
    CHECK(llw_request_submit(runtime, &oversized_params, &oversized, &error) == LLW_OK);
    CHECK(llw_request_submit(runtime, &isolated_params, &isolated, &error) == LLW_OK);
    const auto isolation_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(60);
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_until(lock, isolation_deadline, [&] {
            return events.requests[oversized].terminals == 1 &&
                   events.requests[isolated].terminals == 1;
        }));
        CHECK(events.requests[oversized].error);
        CHECK(!events.requests[isolated].error);
        CHECK(!events.requests[isolated].bytes.empty());
    }
    llw_metrics_t metrics{};
    metrics.struct_size = sizeof(metrics);
    CHECK(llw_get_metrics(runtime, &metrics, &error) == LLW_OK);
    CHECK(metrics.prompt_tokens > 0);
    CHECK(metrics.generated_tokens > 0);
    CHECK(metrics.decode_calls > 0);
    CHECK(llw_model_unload(runtime, model, &error) == LLW_OK);

    {
        std::lock_guard lock(events.mutex);
        events.block_callbacks = true;
        events.callback_entered = false;
        events.release_callbacks = false;
    }
    llw_handle_t lifecycle_model{};
    CHECK(llw_model_load(runtime, &model_params, &lifecycle_model, &error) == LLW_OK);
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_for(lock, std::chrono::seconds(10), [&] {
            return events.callback_entered;
        }));
    }
    llw_request_params_t long_params = generation(lifecycle_model, first_prompt);
    long_params.max_new_tokens = 1024;
    llw_handle_t active_one{}, active_two{}, queued{};
    CHECK(llw_request_submit(runtime, &long_params, &active_one, &error) == LLW_OK);
    long_params.seed = 8;
    CHECK(llw_request_submit(runtime, &long_params, &active_two, &error) == LLW_OK);
    long_params.seed = 9;
    CHECK(llw_request_submit(runtime, &long_params, &queued, &error) == LLW_OK);
    const auto lifecycle_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
    for (;;) {
        llw_scheduler_snapshot_t snapshot{};
        snapshot.struct_size = sizeof(snapshot);
        CHECK(llw_get_scheduler_snapshot(runtime, &snapshot, &error) == LLW_OK);
        if (snapshot.active_count >= 1 && snapshot.queued_count >= 1) break;
        CHECK(std::chrono::steady_clock::now() < lifecycle_deadline);
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    std::atomic<bool> unload_done{false};
    llw_result_t unload_result{LLW_ERR_INTERNAL};
    std::thread unload_thread([&] {
        llw_error_t unload_error{};
        unload_error.struct_size = sizeof(unload_error);
        unload_result = llw_model_unload(runtime, lifecycle_model, &unload_error);
        unload_done.store(true, std::memory_order_release);
    });
    llw_error_t cancel_error{};
    cancel_error.struct_size = sizeof(cancel_error);
    const llw_result_t cancel_result = llw_request_cancel(runtime, active_one, &cancel_error);
    std::this_thread::sleep_for(std::chrono::milliseconds(50));
    const bool unload_returned_before_callback = unload_done.load(std::memory_order_acquire);
    {
        std::lock_guard lock(events.mutex);
        events.release_callbacks = true;
    }
    events.changed.notify_all();
    unload_thread.join();
    CHECK(!unload_returned_before_callback);
    CHECK(unload_result == LLW_OK);
    CHECK(cancel_result == LLW_OK || cancel_result == LLW_ERR_INVALID_STATE ||
          cancel_result == LLW_ERR_NOT_FOUND);
    const auto terminal_deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
    {
        std::unique_lock lock(events.mutex);
        CHECK(events.changed.wait_until(lock, terminal_deadline, [&] {
            return events.requests[active_one].terminals == 1 &&
                   events.requests[active_two].terminals == 1 &&
                   events.requests[queued].terminals == 1;
        }));
    }
    CHECK(llw_request_cancel(runtime, queued, &error) == LLW_ERR_INVALID_STATE);
    llw_runtime_destroy(runtime);
    return 0;
}
```

The lifecycle section deliberately blocks a model-progress callback, starts unload, verifies unload
has not returned, releases the callback, and then requires unload plus all three request terminals to
complete. No callback can remain in flight when `llw_model_unload` returns.

Add the target and checksum-fixture registration:

```cmake
add_executable(llw_runtime_backend_test tests/runtime_backend_test.cpp)
target_include_directories(llw_runtime_backend_test PRIVATE include)
target_link_libraries(llw_runtime_backend_test PRIVATE local_llm_runtime Threads::Threads)
install(TARGETS llw_runtime_backend_test RUNTIME DESTINATION ".")
if(DEFINED ENV{LLW_TEST_GGUF} AND EXISTS "$ENV{LLW_TEST_GGUF}")
  add_test(NAME llw_runtime_backend_test COMMAND llw_runtime_backend_test "$ENV{LLW_TEST_GGUF}")
  set_tests_properties(llw_runtime_backend_test PROPERTIES TIMEOUT 180 LABELS "model;required-ci")
endif()
```

Create `crates/llm-runtime/tests/native_runtime.rs` with the complete Rust-to-DLL drop-cancellation test below. CI supplies the staged `LLW_TEST_RUNTIME` and checksum-verified `LLW_TEST_GGUF`:

```rust
use std::path::PathBuf;
use std::time::Duration;

use llm_runtime::{
    Backend, EventKind, GenerationOptions, InferenceRuntime, ModelOptions, RuntimeOptions,
};

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or_else(|| {
        panic!("{name} must be set by the explicit model-backed test command")
    })
}

#[test]
fn dropping_a_queued_request_cancels_it_once() {
    let dll = required_path("LLW_TEST_RUNTIME");
    let gguf = required_path("LLW_TEST_GGUF");
    let options = RuntimeOptions {
        slot_count: 1,
        request_queue_capacity: 4,
        event_queue_capacity: 1024,
    };
    // SAFETY: both paths are supplied by this repository's explicit, checksum-verified test flow.
    let runtime = unsafe { InferenceRuntime::load(&dll, options) }.expect("load runtime");
    let model = runtime
        .load_model(
            &gguf,
            ModelOptions {
                backend: Backend::Cpu,
                context_tokens_per_slot: 512,
                logical_batch_tokens: 128,
                physical_batch_tokens: 64,
                n_gpu_layers: 0,
                ..ModelOptions::default()
            },
        )
        .expect("load tiny model");
    let long = GenerationOptions {
        max_new_tokens: 1024,
        seed: 11,
        ..GenerationOptions::default()
    };
    let blocker = model.submit(b"Once", long).expect("submit active request");
    let queued = model
        .submit(
            b"The",
            GenerationOptions {
                max_new_tokens: 32,
                seed: 12,
                ..GenerationOptions::default()
            },
        )
        .expect("submit queued request");
    let queued_handle = queued.handle();
    let queued_terminal = queued.terminal_receiver();
    drop(queued);
    let event = queued_terminal
        .recv_timeout(Duration::from_secs(30))
        .expect("queued terminal before timeout");
    assert_eq!(event.request_handle, queued_handle);
    assert_eq!(event.kind, EventKind::Cancelled);
    assert!(queued_terminal.recv_timeout(Duration::from_millis(250)).is_err());
    blocker.cancel().expect("cancel active request");
    let blocker_event = blocker
        .recv_terminal_timeout(Duration::from_secs(30))
        .expect("active cancellation terminal before timeout");
    assert!(matches!(blocker_event.kind, EventKind::Cancelled | EventKind::Done));
}
```

- [ ] **Step 4: Acquire and run the required CPU E2E**

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
cmake --install .cmake-build/llm-cpu --config Debug --prefix .runtime-packs/cpu-debug
$pack = (Resolve-Path '.runtime-packs/cpu-debug').Path
$required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll','llw_runtime_backend_test.exe'
$missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
if ($missing) { throw "CPU E2E pack is missing: $($missing -join ', ')" }
$env:LLW_TEST_BACKEND = 'CPU'
& (Join-Path $pack 'llw_runtime_backend_test.exe') $env:LLW_TEST_GGUF
if ($LASTEXITCODE -ne 0) { throw "installed CPU backend test failed: $LASTEXITCODE" }
$env:LLW_TEST_RUNTIME = (Join-Path $pack 'local_llm_runtime.dll')
cargo test --locked --workspace
```

Expected: native and Rust suites pass; CPU E2E proves two concurrent requests, streaming,
terminal uniqueness, context/lifecycle isolation, and unload. The normal PR CI job always acquires the
checksum-pinned fixture before CMake configure, so this CTest is required there. Local runs register it
whenever `LLW_TEST_GGUF` is explicitly set by the acquisition command above.

The real-model test requires both submitted requests to complete correctly but does not infer overlap
from a timing-sensitive snapshot. Deterministic two-slot concurrency is proven in
`llw_scheduler_test`: `FakeEngine::wait_for_batch_size(2)` is a test barrier that does not release
decode until both handles share one batch, after which the test asserts distinct slots, independent
terminal sequences, cleanup, and exactly one terminal per request. No production ABI test gate is added.

- [ ] **Step 5: Commit the required CPU fixture strategy**

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
cmake --install .cmake-build/llm-cpu --config Release --prefix .runtime-packs/cpu-release

## CUDA compile smoke
cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA
cmake --build .cmake-build/llm-cuda --config Release
cmake --install .cmake-build/llm-cuda --config Release --prefix .runtime-packs/cuda-release

## Vulkan compile smoke
$version = '1.4.350.0'
$url = 'https://sdk.lunarg.com/sdk/download/1.4.350.0/windows/vulkansdk-windows-X64-1.4.350.0.exe'
$sha256 = '855b27ba05d2d8119c5114c5d4ff870ca38f2c632b11e1bb9923b9b7e6ecfe7b'
$installer = Join-Path $env:RUNNER_TEMP 'vulkan-sdk.exe'
Invoke-WebRequest -Uri $url -OutFile $installer
if ((Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant() -ne $sha256) { throw 'Vulkan SDK checksum mismatch' }
$root = "C:\VulkanSDK\$version"
$process = Start-Process -Wait -PassThru -FilePath $installer -ArgumentList '--root', $root, '--accept-licenses', '--default-answer', '--confirm-command', 'install'
if ($process.ExitCode -ne 0) { throw "Vulkan SDK installer failed: $($process.ExitCode)" }
$env:VULKAN_SDK = $root
cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN
cmake --build .cmake-build/llm-vulkan --config Release
cmake --install .cmake-build/llm-vulkan --config Release --prefix .runtime-packs/vulkan-release

## Pack contents
$packs = @(
  @{ Path = '.runtime-packs/cpu-release'; Backend = 'ggml-cpu.dll' },
  @{ Path = '.runtime-packs/cuda-release'; Backend = 'ggml-cuda.dll' },
  @{ Path = '.runtime-packs/vulkan-release'; Backend = 'ggml-vulkan.dll' }
)
foreach ($entry in $packs) {
  if (-not (Test-Path $entry.Path)) { continue }
  $required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll',$entry.Backend
  $missing = $required | Select-Object -Unique | Where-Object { -not (Test-Path (Join-Path $entry.Path $_)) }
  if ($missing) { throw "$($entry.Path) is missing: $($missing -join ', ')" }
  Get-ChildItem -File $entry.Path | Sort-Object Name | Select-Object Name,Length
}

## Hardware-gated runtime checks
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
$env:LLW_TEST_BACKEND = 'CUDA' # use VULKAN and its installed pack on a Vulkan-capable host
& .runtime-packs/cuda-release/llw_runtime_backend_test.exe $env:LLW_TEST_GGUF
if ($LASTEXITCODE -ne 0) { throw "hardware runtime test failed: $LASTEXITCODE" }

The Vulkan SDK source, version, 324012984-byte size, and SHA-256 are pinned from
`https://vulkan.lunarg.com/sdk/files.json`; unattended arguments are from
`https://vulkan.lunarg.com/doc/view/1.4.350.0/windows/getting_started.html`.
The CUDA command requires the self-hosted runner labels `Windows`, `X64`, and `cuda`, plus `nvcc` and
`CUDA_PATH`. Compile smoke does not claim runtime GPU validation. Runtime CUDA/Vulkan tests use the explicit
hardware-gated command above. Metal is reserved for a future ABI-compatible macOS plan and is not configured,
compiled, or tested here.
```

- [ ] **Step 2: Add realistic CI jobs**

Replace `.github/workflows/ci.yml` with this complete workflow. Vulkan uses the public `windows-2025` runner and an official checksum-pinned SDK. CUDA is an explicit `workflow_dispatch` job on a pre-provisioned self-hosted runner; it cannot accidentally run or claim validation on standard hardware.

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:
  workflow_dispatch:
    inputs:
      vulkan_compile_smoke:
        description: Install LunarG SDK 1.4.350.0 and compile the Vulkan pack
        required: true
        type: boolean
        default: false
      cuda_compile_smoke:
        description: Compile on a self-hosted Windows CUDA runner
        required: true
        type: boolean
        default: false

permissions:
  contents: read

jobs:
  windows-cpu-contract:
    runs-on: windows-2025
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - uses: dtolnay/rust-toolchain@4cda84d5c5c54efe2404f9d843567869ab1699d4 # stable
        with:
          toolchain: 1.93.0
          components: rustfmt, clippy
      - uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 # v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: apps/desktop/package-lock.json
      - name: Install frontend dependencies
        run: npm ci --prefix apps/desktop
      - name: Build frontend
        run: npm run build --prefix apps/desktop
      - name: Cache checksum-pinned CPU test model
        uses: actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830 # v4
        with:
          path: .test-models/tiny-random-f16.gguf
          key: tiny-random-f16-1010fc48b2a1880a01fa5e267eb35bf586e3e3ad5539ff5b0e025e4f63616a82
      - name: Acquire checksum-pinned test model
        shell: pwsh
        run: |
          $path = & scripts/acquire-test-model.ps1
          "LLW_TEST_GGUF=$path" | Out-File -FilePath $env:GITHUB_ENV -Append
      - name: Configure CPU runtime
        run: cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
      - name: Build CPU runtime
        run: cmake --build .cmake-build/llm-cpu --config Debug
      - name: Test native runtime
        run: ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
      - name: Stage and verify CPU pack
        shell: pwsh
        run: |
          cmake --install .cmake-build/llm-cpu --config Debug --prefix .runtime-packs/cpu-debug
          $pack = '.runtime-packs/cpu-debug'
          $required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll','llw_runtime_backend_test.exe'
          $missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
          if ($missing) { throw "CPU pack is missing: $($missing -join ', ')" }
          $unexpected = 'ggml-cuda.dll','ggml-vulkan.dll' | Where-Object { Test-Path (Join-Path $pack $_) }
          if ($unexpected) { throw "CPU pack has unexpected backend DLLs: $($unexpected -join ', ')" }
          Get-ChildItem -File $pack | Sort-Object Name | Select-Object Name,Length
      - name: Test installed CPU backend
        shell: pwsh
        run: |
          $env:LLW_TEST_BACKEND = 'CPU'
          & .runtime-packs/cpu-debug/llw_runtime_backend_test.exe $env:LLW_TEST_GGUF
          if ($LASTEXITCODE -ne 0) { throw "installed CPU backend test failed: $LASTEXITCODE" }
      - name: Check Rust formatting
        run: cargo fmt --all --check
      - name: Lint Rust
        run: cargo clippy --locked --workspace --all-targets -- -D warnings
      - name: Test Rust with native runtime
        shell: pwsh
        run: |
          $env:LLW_TEST_RUNTIME = (Resolve-Path '.runtime-packs/cpu-debug/local_llm_runtime.dll')
          cargo test --locked --workspace

  windows-vulkan-compile:
    if: github.event_name == 'workflow_dispatch' && inputs.vulkan_compile_smoke
    runs-on: windows-2025
    timeout-minutes: 90
    env:
      VULKAN_SDK_VERSION: 1.4.350.0
      VULKAN_SDK_URL: https://sdk.lunarg.com/sdk/download/1.4.350.0/windows/vulkansdk-windows-X64-1.4.350.0.exe
      VULKAN_SDK_SHA256: 855b27ba05d2d8119c5114c5d4ff870ca38f2c632b11e1bb9923b9b7e6ecfe7b
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - name: Install pinned LunarG Vulkan SDK
        shell: pwsh
        run: |
          $installer = Join-Path $env:RUNNER_TEMP 'vulkan-sdk.exe'
          Invoke-WebRequest -Uri $env:VULKAN_SDK_URL -OutFile $installer
          $actual = (Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant()
          if ($actual -ne $env:VULKAN_SDK_SHA256) { throw "Vulkan SDK checksum mismatch: $actual" }
          $root = "C:\VulkanSDK\$env:VULKAN_SDK_VERSION"
          $process = Start-Process -Wait -PassThru -FilePath $installer -ArgumentList '--root', $root, '--accept-licenses', '--default-answer', '--confirm-command', 'install'
          if ($process.ExitCode -ne 0) { throw "Vulkan SDK installer failed: $($process.ExitCode)" }
          "VULKAN_SDK=$root" | Out-File -FilePath $env:GITHUB_ENV -Append
          "$root\Bin" | Out-File -FilePath $env:GITHUB_PATH -Append
      - name: Configure Vulkan runtime pack
        run: cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN
      - name: Compile Vulkan runtime pack
        run: cmake --build .cmake-build/llm-vulkan --config Release
      - name: Stage and verify Vulkan runtime pack
        shell: pwsh
        run: |
          cmake --install .cmake-build/llm-vulkan --config Release --prefix .runtime-packs/vulkan-release
          $pack = '.runtime-packs/vulkan-release'
          $required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll','ggml-vulkan.dll'
          $missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
          if ($missing) { throw "Vulkan pack is missing: $($missing -join ', ')" }
          if (Test-Path (Join-Path $pack 'ggml-cuda.dll')) { throw 'Vulkan pack contains ggml-cuda.dll' }
          Get-ChildItem -File $pack | Sort-Object Name | Select-Object Name,Length

  windows-cuda-compile:
    if: github.event_name == 'workflow_dispatch' && inputs.cuda_compile_smoke
    runs-on: [self-hosted, Windows, X64, cuda]
    timeout-minutes: 120
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
      - name: Verify pre-provisioned CUDA toolkit
        shell: pwsh
        run: |
          if (-not $env:CUDA_PATH) { throw 'CUDA_PATH is not set on the self-hosted runner' }
          $nvcc = Get-Command nvcc -ErrorAction Stop
          & $nvcc.Source --version
      - name: Configure CUDA runtime pack
        run: cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA
      - name: Compile CUDA runtime pack
        run: cmake --build .cmake-build/llm-cuda --config Release
      - name: Stage and verify CUDA runtime pack
        shell: pwsh
        run: |
          cmake --install .cmake-build/llm-cuda --config Release --prefix .runtime-packs/cuda-release
          $pack = '.runtime-packs/cuda-release'
          $required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll','ggml-cuda.dll'
          $missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
          if ($missing) { throw "CUDA pack is missing: $($missing -join ', ')" }
          if (Test-Path (Join-Path $pack 'ggml-vulkan.dll')) { throw 'CUDA pack contains ggml-vulkan.dll' }
          Get-ChildItem -File $pack | Sort-Object Name | Select-Object Name,Length
```

- [ ] **Step 3: Configure every available pack locally**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Release
cmake --install .cmake-build/llm-cpu --config Release --prefix .runtime-packs/cpu-release
$env:VULKAN_SDK = 'C:\VulkanSDK\1.4.350.0'
cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN
cmake --build .cmake-build/llm-vulkan --config Release
cmake --install .cmake-build/llm-vulkan --config Release --prefix .runtime-packs/vulkan-release
if (-not $env:CUDA_PATH) { throw 'Run CUDA smoke only on the labeled self-hosted CUDA runner' }
cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA
cmake --build .cmake-build/llm-cuda --config Release
cmake --install .cmake-build/llm-cuda --config Release --prefix .runtime-packs/cuda-release
```

Expected: CPU and the pinned Vulkan SDK build succeed. CUDA succeeds only on the explicitly provisioned self-hosted runner. These are compile checks, not runtime GPU claims.

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

Add this complete test inside the existing `tests` module in `apps/desktop/src-tauri/src/runtime_probe.rs`:

```rust
#[test]
fn command_accepts_pack_id_not_runtime_library_path() {
    let source = include_str!("runtime_probe.rs");
    let start = source
        .find("pub async fn probe_runtime(")
        .expect("probe_runtime command must exist");
    let remainder = &source[start..];
    let end = remainder
        .find(") -> Result<RuntimeInfoDto, String>")
        .expect("probe_runtime signature must retain its result type");
    let signature = &remainder[..end];
    assert!(signature.contains("runtime_pack_id: String"));
    assert!(!signature.contains("PathBuf"));
    assert!(!signature.contains("dll_path"));
}
```

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

Map evidence to: one loaded model; opaque nonzero numeric handles; explicit bounded model/request
params; copied caller buffers; generic event encodings/lifetimes/thread rules; bounded native and Rust
event queues; atomic bounded request registry; configured 1-4 slots; per-slot context budgets; one
shared batch decode call per tick; exact logits indices; independent sequence IDs/KV/samplers;
prompt and generated penalty history; deterministic stop matching; queued and active cancellation;
exactly one terminal event; terminal request erasure; lifecycle exception/race handling; isolated
oversized peer behavior; required checksum-pinned CPU E2E; hardware-gated CUDA/Vulkan.

- [ ] **Step 3: Audit scope and security**

Search implementation for SQLite/RAG/download/release/UI additions and remove them. Confirm no Tauri command accepts an arbitrary DLL path. Confirm Metal appears only as future ABI-extensibility documentation and `GGML_METAL OFF`.

- [ ] **Step 4: Run fresh full verification**

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run build
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Debug
ctest --test-dir .cmake-build/llm-cpu -C Debug --output-on-failure
cmake --install .cmake-build/llm-cpu --config Debug --prefix .runtime-packs/cpu-debug
$pack = (Resolve-Path '.runtime-packs/cpu-debug').Path
$required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll','llw_runtime_backend_test.exe'
$missing = $required | Where-Object { -not (Test-Path (Join-Path $pack $_)) }
if ($missing) { throw "CPU verification pack is missing: $($missing -join ', ')" }
$env:LLW_TEST_BACKEND = 'CPU'
& (Join-Path $pack 'llw_runtime_backend_test.exe') $env:LLW_TEST_GGUF
if ($LASTEXITCODE -ne 0) { throw "installed CPU backend test failed: $LASTEXITCODE" }
$env:LLW_TEST_RUNTIME = (Join-Path $pack 'local_llm_runtime.dll')
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
