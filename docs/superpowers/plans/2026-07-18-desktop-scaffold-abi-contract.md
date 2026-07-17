# Desktop Scaffold and ABI Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scaffold the Windows Tauri desktop application and prove that Rust can safely load and negotiate a versioned native runtime DLL without exposing llama.cpp types.

**Architecture:** `create-tauri-app` creates the React + TypeScript Tauri shell under `apps/desktop`. A root Cargo workspace contains a raw FFI crate and a safe wrapper crate. A CMake-built fake runtime DLL implements only ABI discovery, capabilities, and device enumeration so the loader contract can be tested before llama.cpp and scheduling are introduced.

**Tech Stack:** Tauri 2, React, TypeScript, Vite, Rust 1.93 MSVC, C++17, CMake 4, Windows `LoadLibraryExW` through `libloading`, Serde, thiserror

---

## Scope And Follow-up Plans

This is the first of four implementation plans derived from the approved MVP design.

1. This plan: desktop scaffold, ABI contract, fake DLL, Rust loader, Tauri probe.
2. Native inference plan: pinned llama.cpp, model lifecycle, shared batch scheduler, callbacks, cancellation, CPU/CUDA/Vulkan.
3. Application plan: SQLite, conversations, settings, telemetry, and the Claude-approved UI.
4. Distribution plan: signed runtime manifests, GitHub Releases, installation, update, and rollback.

Claude output is not required for this plan because no product UI is designed. Stop before plan 3 and ask the user to run Claude with `docs/design/claude-design-brief.md`.

## File Map

```text
Cargo.toml                                      Rust workspace membership and shared metadata
.gitignore                                      Rust, CMake, and generated runtime artifacts
apps/desktop/                                   create-tauri-app React/TypeScript scaffold
apps/desktop/src-tauri/Cargo.toml               Tauri app dependencies and workspace linkage
apps/desktop/src-tauri/src/lib.rs                Tauri builder and command registration
apps/desktop/src-tauri/src/runtime_probe.rs      Serializable runtime probe command
crates/llm-runtime-sys/Cargo.toml                Raw loader crate manifest
crates/llm-runtime-sys/src/lib.rs                repr(C) ABI declarations and dynamic symbols
crates/llm-runtime/Cargo.toml                    Safe wrapper crate manifest
crates/llm-runtime/src/lib.rs                    ABI negotiation and safe runtime information
native/llm-runtime/CMakeLists.txt                Fake DLL and C++ test targets
native/llm-runtime/include/llw_runtime.h          Stable public C ABI
native/llm-runtime/src/fake_runtime.cpp           ABI-only fake runtime implementation
native/llm-runtime/tests/abi_layout_test.cpp      C++ ABI layout and behavior test
.github/workflows/ci.yml                          Frontend, Rust, and native Windows checks
```

### Task 1: Verify The Windows Toolchain

**Files:**
- Read: `docs/superpowers/specs/2026-07-18-local-llm-desktop-mvp-design.md`
- Read: `docs/design/claude-design-brief.md`

- [ ] **Step 1: Verify installed tools**

Run from the repository root:

```powershell
rustc --version
cargo --version
rustup show active-toolchain
node --version
npm --version
cmake --version
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
```

Expected: Rust uses `x86_64-pc-windows-msvc`, Node and npm print versions, CMake prints a version, and Visual Studio returns an installation directory.

- [ ] **Step 2: Enter the Visual Studio developer environment**

Run in the PowerShell session used for the remaining native build commands:

```powershell
$vsInstall = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
Import-Module (Join-Path $vsInstall 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsInstall -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
where.exe cl
where.exe link
```

Expected: both `cl.exe` and `link.exe` resolve inside the selected Visual Studio installation. If either is absent, stop and ask the user to install the official Tauri Windows prerequisites before continuing: <https://v2.tauri.app/start/prerequisites/>.

### Task 2: Scaffold The Tauri 2 React Application

**Files:**
- Create: `apps/desktop/`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`

- [ ] **Step 1: Run create-tauri-app non-interactively**

```powershell
New-Item -ItemType Directory -Force apps | Out-Null
Push-Location apps
npm create tauri-app@latest desktop -- --manager npm --template react-ts --identifier io.github.soomin-sung-estsoft.local-llm-wiki --tauri-version 2 --yes
Pop-Location
npm --prefix apps/desktop install
```

Expected: `apps/desktop/package.json`, `apps/desktop/src/`, and `apps/desktop/src-tauri/` exist. The official scaffold options match the current `create-tauri-app` interface: <https://v2.tauri.app/start/create-project/>.

- [ ] **Step 2: Verify the untouched scaffold**

```powershell
npm --prefix apps/desktop run build
Push-Location apps/desktop/src-tauri
cargo check
Pop-Location
```

Expected: the Vite build and Rust check both exit with code 0.

- [ ] **Step 3: Set stable package names without changing the generated UI**

Update `apps/desktop/package.json`:

```json
{
  "name": "local-llm-wiki-desktop",
  "private": true,
  "version": "0.1.0",
  "type": "module"
}
```

Preserve the scaffold-generated `scripts`, `dependencies`, and `devDependencies` entries after changing only the shown metadata fields.

Update the `[package]` section in `apps/desktop/src-tauri/Cargo.toml`:

```toml
[package]
name = "local-llm-wiki-desktop"
version = "0.1.0"
description = "Local-first desktop interface for Local LLM Wiki"
authors = ["Local LLM Wiki contributors"]
edition = "2021"
```

Update the matching fields in `apps/desktop/src-tauri/tauri.conf.json`:

```json
{
  "productName": "Local LLM Wiki",
  "version": "0.1.0",
  "identifier": "io.github.soomin-sung-estsoft.local-llm-wiki"
}
```

Preserve all scaffold-generated build, app, bundle, window, and security configuration.

- [ ] **Step 4: Re-run scaffold checks**

```powershell
npm --prefix apps/desktop run build
Push-Location apps/desktop/src-tauri
cargo check
Pop-Location
```

Expected: both commands pass after the metadata changes.

- [ ] **Step 5: Commit the scaffold**

```powershell
git add apps/desktop
git commit -m "chore: scaffold Tauri desktop app"
```

### Task 3: Create The Rust Workspace And Crate Boundaries

**Files:**
- Create: `Cargo.toml`
- Create: `crates/llm-runtime-sys/Cargo.toml`
- Create: `crates/llm-runtime-sys/src/lib.rs`
- Create: `crates/llm-runtime/Cargo.toml`
- Create: `crates/llm-runtime/src/lib.rs`
- Modify: `.gitignore`
- Modify: `apps/desktop/src-tauri/Cargo.toml`

- [ ] **Step 1: Add failing workspace membership check**

Run before creating the root workspace:

```powershell
cargo metadata --no-deps --format-version 1
```

Expected: FAIL because the repository root does not contain `Cargo.toml`.

- [ ] **Step 2: Create the root workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
  "apps/desktop/src-tauri",
  "crates/llm-runtime-sys",
  "crates/llm-runtime",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
rust-version = "1.93"

[workspace.dependencies]
libloading = "0.8"
serde = { version = "1", features = ["derive"] }
thiserror = "2"
```

- [ ] **Step 3: Create the raw FFI crate skeleton**

Create `crates/llm-runtime-sys/Cargo.toml`:

```toml
[package]
name = "llm-runtime-sys"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
libloading.workspace = true
```

Create `crates/llm-runtime-sys/src/lib.rs`:

```rust
#![deny(unsafe_op_in_unsafe_fn)]

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 0;
```

- [ ] **Step 4: Create the safe wrapper crate skeleton**

Create `crates/llm-runtime/Cargo.toml`:

```toml
[package]
name = "llm-runtime"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
llm-runtime-sys = { path = "../llm-runtime-sys" }
thiserror.workspace = true
```

Create `crates/llm-runtime/src/lib.rs`:

```rust
pub fn expected_abi() -> (u32, u32) {
    (llm_runtime_sys::ABI_MAJOR, llm_runtime_sys::ABI_MINOR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_expected_abi() {
        assert_eq!(super::expected_abi(), (1, 0));
    }
}
```

- [ ] **Step 5: Link the Tauri crate to the safe wrapper**

Add to `apps/desktop/src-tauri/Cargo.toml` dependencies:

```toml
llm-runtime = { path = "../../../crates/llm-runtime" }
serde.workspace = true
```

- [ ] **Step 6: Extend generated artifact ignores**

Append to `.gitignore`:

```gitignore
# Rust/native build outputs
target/
.cmake-build/
native/llm-runtime/out/
```

- [ ] **Step 7: Verify the workspace**

```powershell
cargo metadata --no-deps --format-version 1 | Out-Null
cargo test --workspace
```

Expected: metadata succeeds and the `exposes_expected_abi` test passes.

- [ ] **Step 8: Commit workspace boundaries**

```powershell
git add Cargo.toml .gitignore apps/desktop/src-tauri/Cargo.toml crates
git commit -m "chore: add runtime workspace crates"
```

### Task 4: Define And Validate The Stable C ABI

**Files:**
- Create: `native/llm-runtime/include/llw_runtime.h`
- Create: `native/llm-runtime/tests/abi_layout_test.cpp`
- Create: `native/llm-runtime/CMakeLists.txt`

- [ ] **Step 1: Write the ABI layout test first**

Create `native/llm-runtime/tests/abi_layout_test.cpp`:

```cpp
#include "llw_runtime.h"

#include <cassert>
#include <cstddef>
#include <cstdint>

int main() {
    static_assert(sizeof(void*) == 8);
    static_assert(LLW_ABI_MAJOR == 1u);
    static_assert(LLW_ABI_MINOR == 0u);
    static_assert(offsetof(llw_abi_info_t, struct_size) == 0u);
    static_assert(offsetof(llw_event_t, data_len) % 8u == 0u);
    static_assert(sizeof(llw_handle_t) == sizeof(std::uint64_t));

    llw_abi_info_t info{};
    info.struct_size = sizeof(info);
    assert(info.struct_size >= sizeof(std::uint32_t));
    return 0;
}
```

- [ ] **Step 2: Run CMake to prove the missing header fails**

Create a temporary minimal `native/llm-runtime/CMakeLists.txt` containing only:

```cmake
cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime LANGUAGES C CXX)
enable_testing()
add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test)
```

Run:

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-runtime -A x64
cmake --build .cmake-build/llm-runtime --config Debug
```

Expected: FAIL because `llw_runtime.h` does not exist.

- [ ] **Step 3: Implement the public C header**

Create `native/llm-runtime/include/llw_runtime.h`:

```c
#ifndef LLW_RUNTIME_H
#define LLW_RUNTIME_H

#include <stddef.h>
#include <stdint.h>

#if UINTPTR_MAX != UINT64_MAX
#error "Local LLM runtime ABI supports only 64-bit targets"
#endif

#ifdef _WIN32
#define LLW_CALL __cdecl
#ifdef LLW_RUNTIME_BUILD
#define LLW_EXPORT __declspec(dllexport)
#else
#define LLW_EXPORT __declspec(dllimport)
#endif
#else
#define LLW_CALL
#ifdef LLW_RUNTIME_BUILD
#define LLW_EXPORT __attribute__((visibility("default")))
#else
#define LLW_EXPORT
#endif
#endif

#ifdef __cplusplus
#define LLW_EXTERN_C extern "C"
#else
#define LLW_EXTERN_C
#endif

#define LLW_ABI_MAJOR 1u
#define LLW_ABI_MINOR 0u

typedef uint64_t llw_handle_t;
typedef int32_t llw_result_t;

#define LLW_OK ((llw_result_t)0)
#define LLW_ERR_INVALID_ARGUMENT ((llw_result_t)1)
#define LLW_ERR_ABI_MISMATCH ((llw_result_t)2)
#define LLW_ERR_BUFFER_TOO_SMALL ((llw_result_t)3)
#define LLW_ERR_INTERNAL ((llw_result_t)1000)

#define LLW_BACKEND_AUTO ((int32_t)0)
#define LLW_BACKEND_CPU ((int32_t)1)
#define LLW_BACKEND_CUDA ((int32_t)2)
#define LLW_BACKEND_VULKAN ((int32_t)3)

#define LLW_EVENT_MODEL_PROGRESS ((int32_t)1)
#define LLW_EVENT_QUEUED ((int32_t)2)
#define LLW_EVENT_TOKEN ((int32_t)3)
#define LLW_EVENT_METRICS ((int32_t)4)
#define LLW_EVENT_DONE ((int32_t)5)
#define LLW_EVENT_CANCELLED ((int32_t)6)
#define LLW_EVENT_ERROR ((int32_t)7)
#define LLW_EVENT_LOG ((int32_t)8)

#pragma pack(push, 8)

typedef struct llw_error_t {
    uint32_t struct_size;
    int32_t code;
    uint32_t flags;
    char message[512];
    uint64_t reserved[8];
} llw_error_t;

typedef struct llw_abi_query_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t requested_major;
    uint32_t requested_minor;
    uint64_t reserved[8];
} llw_abi_query_t;

typedef struct llw_abi_info_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t abi_major;
    uint32_t abi_minor;
    uint32_t min_supported_major;
    uint32_t min_supported_minor;
    uint64_t feature_flags;
    uint64_t reserved[8];
} llw_abi_info_t;

typedef struct llw_capabilities_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t supports_cpu;
    uint32_t supports_cuda;
    uint32_t supports_vulkan;
    uint32_t supports_streaming;
    uint32_t supports_cancellation;
    uint32_t max_parallel_slots;
    uint64_t reserved[8];
} llw_capabilities_t;

typedef struct llw_device_info_t {
    uint32_t struct_size;
    uint32_t flags;
    int32_t backend;
    uint32_t device_index;
    char id[64];
    char name[128];
    char vendor[64];
    uint64_t reserved[8];
} llw_device_info_t;

typedef struct llw_device_list_t {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t capacity;
    uint32_t count;
    uint32_t element_size;
    uint32_t reserved0;
    llw_device_info_t* devices;
    uint64_t required_count;
    uint64_t reserved[8];
} llw_device_list_t;

typedef struct llw_event_t {
    uint32_t struct_size;
    uint32_t flags;
    int32_t event_type;
    int32_t error_code;
    llw_handle_t model_handle;
    llw_handle_t request_handle;
    uint32_t slot_id;
    uint32_t reserved0;
    uint64_t sequence_number;
    const uint8_t* data;
    uint64_t data_len;
    void* request_user_data;
    uint64_t reserved[8];
} llw_event_t;

typedef void(LLW_CALL* llw_event_callback_t)(const llw_event_t* event, void* user_data);

typedef struct llw_callback_table_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_event_callback_t on_event;
    void* user_data;
    uint64_t reserved[8];
} llw_callback_table_t;

typedef struct llw_runtime_create_params_t {
    uint32_t struct_size;
    uint32_t flags;
    llw_callback_table_t callbacks;
    uint64_t reserved[8];
} llw_runtime_create_params_t;

#pragma pack(pop)

typedef struct llw_runtime_t llw_runtime_t;

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query,
    llw_abi_info_t* out_info,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void);
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params,
    llw_runtime_t** out_runtime,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime,
    llw_capabilities_t* out_capabilities,
    llw_error_t* out_error);
LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime,
    int32_t backend,
    llw_device_list_t* out_devices,
    llw_error_t* out_error);

#endif
```

- [ ] **Step 4: Restore the complete CMake contract target**

Replace `native/llm-runtime/CMakeLists.txt` with:

```cmake
cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test)
```

- [ ] **Step 5: Build and run the ABI test**

```powershell
cmake -S native/llm-runtime -B .cmake-build/llm-runtime -A x64
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug --output-on-failure
```

Expected: `llw_abi_layout_test` passes.

- [ ] **Step 6: Commit the ABI contract**

```powershell
git add native/llm-runtime
git commit -m "feat: define native runtime ABI"
```

### Task 5: Build An ABI-Only Fake Runtime DLL

**Files:**
- Create: `native/llm-runtime/src/fake_runtime.cpp`
- Modify: `native/llm-runtime/tests/abi_layout_test.cpp`
- Modify: `native/llm-runtime/CMakeLists.txt`

- [ ] **Step 1: Add failing runtime behavior assertions**

Append to `main()` in `native/llm-runtime/tests/abi_layout_test.cpp` before `return 0`:

```cpp
    llw_abi_query_t query{};
    query.struct_size = sizeof(query);
    query.requested_major = LLW_ABI_MAJOR;
    query.requested_minor = LLW_ABI_MINOR;
    llw_error_t error{};
    error.struct_size = sizeof(error);
    assert(llw_get_abi_info(&query, &info, &error) == LLW_OK);
    assert(info.abi_major == LLW_ABI_MAJOR);
    assert(info.abi_minor == LLW_ABI_MINOR);

    llw_runtime_create_params_t create{};
    create.struct_size = sizeof(create);
    llw_runtime_t* runtime = nullptr;
    assert(llw_runtime_create(&create, &runtime, &error) == LLW_OK);
    assert(runtime != nullptr);

    llw_capabilities_t capabilities{};
    capabilities.struct_size = sizeof(capabilities);
    assert(llw_runtime_get_capabilities(runtime, &capabilities, &error) == LLW_OK);
    assert(capabilities.supports_cpu == 1u);
    assert(capabilities.max_parallel_slots == 4u);

    llw_device_list_t devices{};
    devices.struct_size = sizeof(devices);
    assert(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_ERR_BUFFER_TOO_SMALL);
    assert(devices.required_count == 1u);

    llw_device_info_t storage[1]{};
    storage[0].struct_size = sizeof(llw_device_info_t);
    devices.capacity = 1u;
    devices.devices = storage;
    devices.element_size = sizeof(llw_device_info_t);
    assert(llw_runtime_list_devices(runtime, LLW_BACKEND_CPU, &devices, &error) == LLW_OK);
    assert(devices.count == 1u);
    assert(storage[0].backend == LLW_BACKEND_CPU);

    llw_runtime_destroy(runtime);
```

- [ ] **Step 2: Link the test before implementing exports**

Update `native/llm-runtime/CMakeLists.txt`:

```cmake
add_library(local_llm_runtime SHARED src/fake_runtime.cpp)
target_compile_definitions(local_llm_runtime PRIVATE LLW_RUNTIME_BUILD)
target_include_directories(local_llm_runtime PUBLIC include)

target_link_libraries(llw_abi_layout_test PRIVATE local_llm_runtime)
```

Create `native/llm-runtime/src/fake_runtime.cpp` with only:

```cpp
#include "llw_runtime.h"
```

Run:

```powershell
cmake --build .cmake-build/llm-runtime --config Debug
```

Expected: FAIL with unresolved `llw_*` symbols.

- [ ] **Step 3: Implement the fake runtime exports**

Replace `native/llm-runtime/src/fake_runtime.cpp` with:

```cpp
#include "llw_runtime.h"

#include <algorithm>
#include <cstring>
#include <new>

struct llw_runtime_t {
    llw_callback_table_t callbacks{};
};

namespace {

llw_result_t fail(llw_error_t* error, llw_result_t code, const char* message) {
    if (error && error->struct_size >= sizeof(uint32_t) + sizeof(int32_t)) {
        error->code = code;
        if (error->struct_size >= sizeof(llw_error_t)) {
            std::strncpy(error->message, message, sizeof(error->message) - 1u);
            error->message[sizeof(error->message) - 1u] = '\0';
        }
    }
    return code;
}

void copy_text(char* destination, size_t capacity, const char* source) {
    if (capacity == 0u) {
        return;
    }
    std::strncpy(destination, source, capacity - 1u);
    destination[capacity - 1u] = '\0';
}

}  // namespace

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query,
    llw_abi_info_t* out_info,
    llw_error_t* out_error) {
    if (!query || !out_info || query->struct_size < sizeof(llw_abi_query_t) ||
        out_info->struct_size < sizeof(llw_abi_info_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid ABI query");
    }
    if (query->requested_major != LLW_ABI_MAJOR) {
        return fail(out_error, LLW_ERR_ABI_MISMATCH, "unsupported ABI major");
    }
    out_info->abi_major = LLW_ABI_MAJOR;
    out_info->abi_minor = LLW_ABI_MINOR;
    out_info->min_supported_major = LLW_ABI_MAJOR;
    out_info->min_supported_minor = 0u;
    out_info->feature_flags = 0u;
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) {
    return "0.1.0-fake";
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) {
    return "not-linked";
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params,
    llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    if (!params || !out_runtime || params->struct_size < sizeof(llw_runtime_create_params_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid runtime create parameters");
    }
    auto* runtime = new (std::nothrow) llw_runtime_t{};
    if (!runtime) {
        return fail(out_error, LLW_ERR_INTERNAL, "runtime allocation failed");
    }
    runtime->callbacks = params->callbacks;
    *out_runtime = runtime;
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) {
    delete runtime;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime,
    llw_capabilities_t* out_capabilities,
    llw_error_t* out_error) {
    if (!runtime || !out_capabilities || out_capabilities->struct_size < sizeof(llw_capabilities_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid capabilities output");
    }
    out_capabilities->supports_cpu = 1u;
    out_capabilities->supports_cuda = 0u;
    out_capabilities->supports_vulkan = 0u;
    out_capabilities->supports_streaming = 0u;
    out_capabilities->supports_cancellation = 0u;
    out_capabilities->max_parallel_slots = 4u;
    return LLW_OK;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime,
    int32_t backend,
    llw_device_list_t* out_devices,
    llw_error_t* out_error) {
    if (!runtime || !out_devices || out_devices->struct_size < sizeof(llw_device_list_t)) {
        return fail(out_error, LLW_ERR_INVALID_ARGUMENT, "invalid device list output");
    }
    if (backend != LLW_BACKEND_AUTO && backend != LLW_BACKEND_CPU) {
        out_devices->count = 0u;
        out_devices->required_count = 0u;
        return LLW_OK;
    }
    out_devices->required_count = 1u;
    if (!out_devices->devices || out_devices->capacity < 1u ||
        out_devices->element_size < sizeof(llw_device_info_t)) {
        return fail(out_error, LLW_ERR_BUFFER_TOO_SMALL, "device buffer is too small");
    }
    auto& device = out_devices->devices[0];
    device.struct_size = sizeof(device);
    device.backend = LLW_BACKEND_CPU;
    device.device_index = 0u;
    copy_text(device.id, sizeof(device.id), "cpu:0");
    copy_text(device.name, sizeof(device.name), "Fake CPU");
    copy_text(device.vendor, sizeof(device.vendor), "Local LLM Wiki");
    out_devices->count = 1u;
    return LLW_OK;
}
```

- [ ] **Step 4: Build and run C++ tests**

```powershell
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug --output-on-failure
```

Expected: the DLL builds and `llw_abi_layout_test` passes.

- [ ] **Step 5: Verify exported symbols**

```powershell
dumpbin /exports .cmake-build/llm-runtime/Debug/local_llm_runtime.dll | Select-String 'llw_'
```

Expected: all seven declared `llw_` functions appear as undecorated exports.

- [ ] **Step 6: Commit the fake DLL**

```powershell
git add native/llm-runtime
git commit -m "test: add ABI-only fake runtime"
```

### Task 6: Implement The Raw Rust Loader

**Files:**
- Modify: `crates/llm-runtime-sys/src/lib.rs`
- Test: `crates/llm-runtime-sys/src/lib.rs`

- [ ] **Step 1: Write a failing ABI layout test**

Append to `crates/llm-runtime-sys/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_layout_starts_with_struct_size() {
        assert_eq!(std::mem::offset_of!(AbiInfo, struct_size), 0);
        assert_eq!(std::mem::size_of::<Handle>(), 8);
    }
}
```

Run:

```powershell
cargo test -p llm-runtime-sys
```

Expected: FAIL because `AbiInfo` and `Handle` are not defined.

- [ ] **Step 2: Define raw ABI types and symbol signatures**

Replace the crate body above the test module with the Rust equivalents of every public C type in `llw_runtime.h`. Use this exact pattern for each structure:

```rust
#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::{c_char, c_void};

pub const ABI_MAJOR: u32 = 1;
pub const ABI_MINOR: u32 = 0;
pub const OK: i32 = 0;
pub const ERR_BUFFER_TOO_SMALL: i32 = 3;
pub const BACKEND_AUTO: i32 = 0;
pub const BACKEND_CPU: i32 = 1;
pub const BACKEND_CUDA: i32 = 2;
pub const BACKEND_VULKAN: i32 = 3;

pub type Handle = u64;

#[repr(C)]
pub struct Runtime {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Error {
    pub struct_size: u32,
    pub code: i32,
    pub flags: u32,
    pub message: [c_char; 512],
    pub reserved: [u64; 8],
}

impl Default for Error {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            code: 0,
            flags: 0,
            message: [0; 512],
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct AbiQuery {
    pub struct_size: u32,
    pub flags: u32,
    pub requested_major: u32,
    pub requested_minor: u32,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Default)]
pub struct AbiInfo {
    pub struct_size: u32,
    pub flags: u32,
    pub abi_major: u32,
    pub abi_minor: u32,
    pub min_supported_major: u32,
    pub min_supported_minor: u32,
    pub feature_flags: u64,
    pub reserved: [u64; 8],
}

pub type EventCallback = unsafe extern "C" fn(event: *const Event, user_data: *mut c_void);

#[repr(C)]
pub struct Event {
    pub struct_size: u32,
    pub flags: u32,
    pub event_type: i32,
    pub error_code: i32,
    pub model_handle: Handle,
    pub request_handle: Handle,
    pub slot_id: u32,
    pub reserved0: u32,
    pub sequence_number: u64,
    pub data: *const u8,
    pub data_len: u64,
    pub request_user_data: *mut c_void,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CallbackTable {
    pub struct_size: u32,
    pub flags: u32,
    pub on_event: Option<EventCallback>,
    pub user_data: *mut c_void,
    pub reserved: [u64; 8],
}

impl Default for CallbackTable {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            on_event: None,
            user_data: std::ptr::null_mut(),
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct RuntimeCreateParams {
    pub struct_size: u32,
    pub flags: u32,
    pub callbacks: CallbackTable,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Default)]
pub struct Capabilities {
    pub struct_size: u32,
    pub flags: u32,
    pub supports_cpu: u32,
    pub supports_cuda: u32,
    pub supports_vulkan: u32,
    pub supports_streaming: u32,
    pub supports_cancellation: u32,
    pub max_parallel_slots: u32,
    pub reserved: [u64; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceInfo {
    pub struct_size: u32,
    pub flags: u32,
    pub backend: i32,
    pub device_index: u32,
    pub id: [c_char; 64],
    pub name: [c_char; 128],
    pub vendor: [c_char; 64],
    pub reserved: [u64; 8],
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            backend: BACKEND_AUTO,
            device_index: 0,
            id: [0; 64],
            name: [0; 128],
            vendor: [0; 64],
            reserved: [0; 8],
        }
    }
}

#[repr(C)]
pub struct DeviceList {
    pub struct_size: u32,
    pub flags: u32,
    pub capacity: u32,
    pub count: u32,
    pub element_size: u32,
    pub reserved0: u32,
    pub devices: *mut DeviceInfo,
    pub required_count: u64,
    pub reserved: [u64; 8],
}

impl Default for DeviceList {
    fn default() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>() as u32,
            flags: 0,
            capacity: 0,
            count: 0,
            element_size: std::mem::size_of::<DeviceInfo>() as u32,
            reserved0: 0,
            devices: std::ptr::null_mut(),
            required_count: 0,
            reserved: [0; 8],
        }
    }
}

pub type GetAbiInfoFn =
    unsafe extern "C" fn(*const AbiQuery, *mut AbiInfo, *mut Error) -> i32;
pub type RuntimeVersionFn = unsafe extern "C" fn() -> *const c_char;
pub type LlamaCommitFn = unsafe extern "C" fn() -> *const c_char;
pub type RuntimeCreateFn = unsafe extern "C" fn(
    *const RuntimeCreateParams,
    *mut *mut Runtime,
    *mut Error,
) -> i32;
pub type RuntimeDestroyFn = unsafe extern "C" fn(*mut Runtime);
pub type RuntimeGetCapabilitiesFn =
    unsafe extern "C" fn(*mut Runtime, *mut Capabilities, *mut Error) -> i32;
pub type RuntimeListDevicesFn =
    unsafe extern "C" fn(*mut Runtime, i32, *mut DeviceList, *mut Error) -> i32;
```

- [ ] **Step 3: Add the function table loader**

Add:

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
}

impl Api {
    pub unsafe fn load(path: &std::path::Path) -> Result<Self, libloading::Error> {
        let library = unsafe { libloading::Library::new(path)? };
        let get_abi_info = unsafe { *library.get::<GetAbiInfoFn>(b"llw_get_abi_info\0")? };
        let runtime_version = unsafe { *library.get::<RuntimeVersionFn>(b"llw_runtime_version\0")? };
        let llama_commit = unsafe { *library.get::<LlamaCommitFn>(b"llw_llama_cpp_commit\0")? };
        let runtime_create = unsafe { *library.get::<RuntimeCreateFn>(b"llw_runtime_create\0")? };
        let runtime_destroy = unsafe { *library.get::<RuntimeDestroyFn>(b"llw_runtime_destroy\0")? };
        let runtime_get_capabilities = unsafe {
            *library.get::<RuntimeGetCapabilitiesFn>(b"llw_runtime_get_capabilities\0")?
        };
        let runtime_list_devices = unsafe {
            *library.get::<RuntimeListDevicesFn>(b"llw_runtime_list_devices\0")?
        };
        Ok(Self {
            _library: library,
            get_abi_info,
            runtime_version,
            llama_commit,
            runtime_create,
            runtime_destroy,
            runtime_get_capabilities,
            runtime_list_devices,
        })
    }
}
```

- [ ] **Step 4: Run formatting, lint, and tests**

```powershell
cargo fmt --all --check
cargo clippy -p llm-runtime-sys --all-targets -- -D warnings
cargo test -p llm-runtime-sys
```

Expected: all commands pass.

- [ ] **Step 5: Commit the raw loader**

```powershell
git add crates/llm-runtime-sys
git commit -m "feat: load native runtime ABI from Rust"
```

### Task 7: Add The Safe ABI Probe Wrapper

**Files:**
- Modify: `crates/llm-runtime/src/lib.rs`
- Test: `crates/llm-runtime/tests/fake_runtime.rs`

- [ ] **Step 1: Write a failing integration test**

Create `crates/llm-runtime/tests/fake_runtime.rs`:

```rust
use llm_runtime::{Backend, RuntimeLibrary};

#[test]
fn probes_fake_runtime() {
    let path = std::env::var_os("LLW_TEST_RUNTIME")
        .map(std::path::PathBuf::from)
        .expect("LLW_TEST_RUNTIME must point to the fake DLL");
    let runtime = RuntimeLibrary::load(&path).expect("load fake runtime");
    let info = runtime.info();
    assert_eq!(info.abi_major, 1);
    assert_eq!(info.runtime_version, "0.1.0-fake");
    assert_eq!(info.llama_cpp_commit, "not-linked");
    assert!(info.capabilities.supports_cpu);
    assert_eq!(info.capabilities.max_parallel_slots, 4);
    let devices = runtime.devices(Backend::Cpu).expect("list CPU devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, "cpu:0");
}
```

Run:

```powershell
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
cargo test -p llm-runtime --test fake_runtime
```

Expected: FAIL because `Backend` and `RuntimeLibrary` do not exist.

- [ ] **Step 2: Implement the safe types and error conversion**

Replace `crates/llm-runtime/src/lib.rs` with safe public types:

```rust
use std::ffi::CStr;
use std::path::Path;
use std::ptr;

use llm_runtime_sys as sys;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to load runtime DLL: {0}")]
    Load(#[from] libloading::Error),
    #[error("runtime ABI mismatch: expected {expected}, got {actual}")]
    AbiMismatch { expected: u32, actual: u32 },
    #[error("runtime call failed with code {code}: {message}")]
    Runtime { code: i32, message: String },
    #[error("runtime returned invalid UTF-8")]
    InvalidUtf8,
}

#[derive(Debug, Clone, Copy)]
pub enum Backend {
    Auto,
    Cpu,
    Cuda,
    Vulkan,
}

impl Backend {
    fn raw(self) -> i32 {
        match self {
            Self::Auto => sys::BACKEND_AUTO,
            Self::Cpu => sys::BACKEND_CPU,
            Self::Cuda => sys::BACKEND_CUDA,
            Self::Vulkan => sys::BACKEND_VULKAN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub supports_cpu: bool,
    pub supports_cuda: bool,
    pub supports_vulkan: bool,
    pub supports_streaming: bool,
    pub supports_cancellation: bool,
    pub max_parallel_slots: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub backend: i32,
    pub device_index: u32,
    pub id: String,
    pub name: String,
    pub vendor: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeInfo {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub runtime_version: String,
    pub llama_cpp_commit: String,
    pub capabilities: Capabilities,
}
```

Add `libloading.workspace = true` to `crates/llm-runtime/Cargo.toml` because the safe error type exposes loader failures.

- [ ] **Step 3: Implement RuntimeLibrary ownership and ABI negotiation**

Add the following complete ownership and negotiation implementation below the public data types:

```rust
pub struct RuntimeLibrary {
    api: sys::Api,
    runtime: *mut sys::Runtime,
    info: RuntimeInfo,
}

impl Drop for RuntimeLibrary {
    fn drop(&mut self) {
        if !self.runtime.is_null() {
            unsafe { (self.api.runtime_destroy)(self.runtime) };
        }
    }
}

impl RuntimeLibrary {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let api = unsafe { sys::Api::load(path)? };
        let query = sys::AbiQuery {
            struct_size: std::mem::size_of::<sys::AbiQuery>() as u32,
            flags: 0,
            requested_major: sys::ABI_MAJOR,
            requested_minor: sys::ABI_MINOR,
            reserved: [0; 8],
        };
        let mut abi = sys::AbiInfo {
            struct_size: std::mem::size_of::<sys::AbiInfo>() as u32,
            ..Default::default()
        };
        let mut raw_error = sys::Error::default();
        let code = unsafe { (api.get_abi_info)(&query, &mut abi, &mut raw_error) };
        check_result(code, &raw_error)?;
        if abi.abi_major != sys::ABI_MAJOR {
            return Err(Error::AbiMismatch {
                expected: sys::ABI_MAJOR,
                actual: abi.abi_major,
            });
        }

        let runtime_version = unsafe { read_static_string((api.runtime_version)())? };
        let llama_cpp_commit = unsafe { read_static_string((api.llama_commit)())? };

        let create = sys::RuntimeCreateParams {
            struct_size: std::mem::size_of::<sys::RuntimeCreateParams>() as u32,
            flags: 0,
            callbacks: sys::CallbackTable::default(),
            reserved: [0; 8],
        };
        let mut runtime = ptr::null_mut();
        let code = unsafe { (api.runtime_create)(&create, &mut runtime, &mut raw_error) };
        check_result(code, &raw_error)?;
        if runtime.is_null() {
            return Err(Error::Runtime {
                code: -1,
                message: "runtime returned a null handle".into(),
            });
        }

        let mut capabilities = sys::Capabilities {
            struct_size: std::mem::size_of::<sys::Capabilities>() as u32,
            ..Default::default()
        };
        let code = unsafe {
            (api.runtime_get_capabilities)(runtime, &mut capabilities, &mut raw_error)
        };
        if let Err(error) = check_result(code, &raw_error) {
            unsafe { (api.runtime_destroy)(runtime) };
            return Err(error);
        }

        let info = RuntimeInfo {
            abi_major: abi.abi_major,
            abi_minor: abi.abi_minor,
            runtime_version,
            llama_cpp_commit,
            capabilities: Capabilities {
                supports_cpu: capabilities.supports_cpu != 0,
                supports_cuda: capabilities.supports_cuda != 0,
                supports_vulkan: capabilities.supports_vulkan != 0,
                supports_streaming: capabilities.supports_streaming != 0,
                supports_cancellation: capabilities.supports_cancellation != 0,
                max_parallel_slots: capabilities.max_parallel_slots,
            },
        };
        Ok(Self { api, runtime, info })
    }

    pub fn info(&self) -> &RuntimeInfo {
        &self.info
    }

    pub fn devices(&self, backend: Backend) -> Result<Vec<DeviceInfo>, Error> {
        let mut raw_error = sys::Error::default();
        let mut list = sys::DeviceList::default();
        let first = unsafe {
            (self.api.runtime_list_devices)(
                self.runtime,
                backend.raw(),
                &mut list,
                &mut raw_error,
            )
        };
        if first != sys::OK && first != sys::ERR_BUFFER_TOO_SMALL {
            return Err(runtime_error(first, &raw_error));
        }
        if list.required_count == 0 {
            return Ok(Vec::new());
        }

        let mut storage = vec![sys::DeviceInfo::default(); list.required_count as usize];
        list.capacity = storage.len() as u32;
        list.devices = storage.as_mut_ptr();
        let second = unsafe {
            (self.api.runtime_list_devices)(
                self.runtime,
                backend.raw(),
                &mut list,
                &mut raw_error,
            )
        };
        check_result(second, &raw_error)?;
        storage
            .into_iter()
            .take(list.count as usize)
            .map(|raw| {
                Ok(DeviceInfo {
                    backend: raw.backend,
                    device_index: raw.device_index,
                    id: read_fixed_string(&raw.id)?,
                    name: read_fixed_string(&raw.name)?,
                    vendor: read_fixed_string(&raw.vendor)?,
                })
            })
            .collect()
    }
}

fn check_result(code: i32, error: &sys::Error) -> Result<(), Error> {
    if code == sys::OK {
        Ok(())
    } else {
        Err(runtime_error(code, error))
    }
}

fn runtime_error(code: i32, error: &sys::Error) -> Error {
    Error::Runtime {
        code,
        message: read_fixed_string(&error.message).unwrap_or_else(|_| "unknown runtime error".into()),
    }
}

fn read_fixed_string(value: &[std::ffi::c_char]) -> Result<String, Error> {
    let bytes: Vec<u8> = value
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
}

unsafe fn read_static_string(value: *const std::ffi::c_char) -> Result<String, Error> {
    if value.is_null() {
        return Err(Error::Runtime {
            code: -1,
            message: "runtime returned a null string".into(),
        });
    }
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| Error::InvalidUtf8)
}
```

- [ ] **Step 4: Run the fake DLL integration test**

```powershell
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy -p llm-runtime --all-targets -- -D warnings
cargo test -p llm-runtime --test fake_runtime
```

Expected: the integration test passes and reports one fake CPU device.

- [ ] **Step 5: Commit the safe wrapper**

```powershell
git add crates/llm-runtime
git commit -m "feat: negotiate runtime ABI safely"
```

### Task 8: Expose A Tauri Runtime Probe Command

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_probe.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/src/runtime_probe.rs`

- [ ] **Step 1: Write the failing DTO serialization test**

Create `apps/desktop/src-tauri/src/runtime_probe.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_info_serializes_with_camel_case_fields() {
        let dto = RuntimeInfoDto {
            abi_major: 1,
            abi_minor: 0,
            runtime_version: "0.1.0-fake".into(),
            llama_cpp_commit: "not-linked".into(),
            max_parallel_slots: 4,
        };
        let value = serde_json::to_value(dto).expect("serialize runtime info");
        assert_eq!(value["runtimeVersion"], "0.1.0-fake");
        assert_eq!(value["maxParallelSlots"], 4);
    }
}
```

Add `serde_json = "1"` under `[dev-dependencies]` in `apps/desktop/src-tauri/Cargo.toml`.

Run:

```powershell
cargo test -p local-llm-wiki-desktop runtime_info_serializes_with_camel_case_fields
```

Expected: FAIL because `RuntimeInfoDto` is not defined.

- [ ] **Step 2: Implement the probe DTO and command**

Add above the test module:

```rust
use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfoDto {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub runtime_version: String,
    pub llama_cpp_commit: String,
    pub max_parallel_slots: u32,
}

#[tauri::command]
pub async fn probe_runtime(path: PathBuf) -> Result<RuntimeInfoDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = llm_runtime::RuntimeLibrary::load(&path).map_err(|error| error.to_string())?;
        let info = runtime.info();
        Ok(RuntimeInfoDto {
            abi_major: info.abi_major,
            abi_minor: info.abi_minor,
            runtime_version: info.runtime_version.clone(),
            llama_cpp_commit: info.llama_cpp_commit.clone(),
            max_parallel_slots: info.capabilities.max_parallel_slots,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}
```

The async command follows Tauri's guidance to keep blocking native work off the UI thread: <https://v2.tauri.app/develop/calling-rust/>.

- [ ] **Step 3: Register the command**

Add `mod runtime_probe;` to `apps/desktop/src-tauri/src/lib.rs` and include the command in the generated handler:

```rust
.invoke_handler(tauri::generate_handler![runtime_probe::probe_runtime])
```

Do not change the scaffold-generated React UI. Claude owns visual design in plan 3.

- [ ] **Step 4: Run Rust and frontend checks**

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
cargo test --workspace
npm --prefix apps/desktop run build
```

Expected: all commands pass.

- [ ] **Step 5: Commit the Tauri bridge**

```powershell
git add apps/desktop/src-tauri
git commit -m "feat: expose native runtime probe command"
```

### Task 9: Add Windows CI For The Contract

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create the CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  windows-contract:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.93.0
          components: rustfmt, clippy
      - uses: actions/setup-node@v4
        with:
          node-version: 24
          cache: npm
          cache-dependency-path: apps/desktop/package-lock.json
      - name: Install frontend dependencies
        run: npm ci --prefix apps/desktop
      - name: Build frontend
        run: npm run build --prefix apps/desktop
      - name: Configure fake runtime
        run: cmake -S native/llm-runtime -B .cmake-build/llm-runtime -A x64
      - name: Build fake runtime
        run: cmake --build .cmake-build/llm-runtime --config Debug
      - name: Test native ABI
        run: ctest --test-dir .cmake-build/llm-runtime -C Debug --output-on-failure
      - name: Check Rust formatting
        run: cargo fmt --all --check
      - name: Lint Rust
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test Rust with fake runtime
        shell: pwsh
        run: |
          $env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
          cargo test --workspace
```

- [ ] **Step 2: Validate workflow structure locally**

```powershell
Get-Content -Raw .github/workflows/ci.yml | ConvertFrom-Yaml | Out-Null
```

Expected: exit code 0 when `ConvertFrom-Yaml` is available. If the cmdlet is unavailable, run `npx prettier --check .github/workflows/ci.yml` after adding no project dependency.

- [ ] **Step 3: Run the full local verification sequence**

```powershell
npm --prefix apps/desktop run build
cmake -S native/llm-runtime -B .cmake-build/llm-runtime -A x64
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug --output-on-failure
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: frontend build succeeds, native test reports 100% passed, Rust tests pass, clippy reports no warnings, and `git diff --check` prints nothing.

- [ ] **Step 4: Commit CI**

```powershell
git add .github/workflows/ci.yml
git commit -m "ci: verify desktop runtime contract"
```

### Task 10: Final Contract Verification

**Files:**
- Read: `docs/superpowers/specs/2026-07-18-local-llm-desktop-mvp-design.md`
- Read: `native/llm-runtime/include/llw_runtime.h`
- Read: `crates/llm-runtime-sys/src/lib.rs`

- [ ] **Step 1: Compare ABI names and field order**

Confirm every C structure in `llw_runtime.h` has a field-for-field `#[repr(C)]` equivalent in `llm-runtime-sys`. Confirm every required export is loaded exactly once by `sys::Api`.

- [ ] **Step 2: Verify scope boundaries**

Confirm this plan has not added model loading, token generation, scheduling, SQLite, runtime downloading, or product UI. Those belong to later plans.

- [ ] **Step 3: Run fresh verification**

```powershell
npm --prefix apps/desktop run build
cmake --build .cmake-build/llm-runtime --config Debug
ctest --test-dir .cmake-build/llm-runtime -C Debug --output-on-failure
$env:LLW_TEST_RUNTIME = (Resolve-Path '.cmake-build/llm-runtime/Debug/local_llm_runtime.dll')
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git status --short --branch
```

Expected: all builds and tests pass; Git reports a clean feature branch ahead only by the intentional commits from this plan.

- [ ] **Step 4: Prepare the next plan**

Use the approved design and the proven ABI foundation to write `docs/superpowers/plans/2026-07-18-native-llama-scheduler.md`. That plan must introduce model handles, request parameters, generic callback events, bounded queues, 1-4 slots, shared batch decode, cancellation, and CPU/CUDA/Vulkan device selection using a pinned llama.cpp commit.
