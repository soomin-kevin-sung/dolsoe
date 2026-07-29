cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime VERSION 0.3.0 LANGUAGES C CXX)

if(MSVC)
  add_compile_options(/utf-8)
endif()

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/llama-baseline.json" LLW_LLAMA_BASELINE_JSON)
string(JSON LLAMA_CPP_COMMIT GET "${LLW_LLAMA_BASELINE_JSON}" commit)

set(LLW_LLAMA_INCLUDE_DIR "" CACHE PATH "Pinned llama.cpp public header directory")
if(NOT IS_DIRECTORY "${LLW_LLAMA_INCLUDE_DIR}")
  message(FATAL_ERROR "LLW_LLAMA_INCLUDE_DIR must point to the prepared pinned header SDK")
endif()
foreach(required_header IN ITEMS llama.h ggml.h ggml-cpu.h ggml-backend.h ggml-alloc.h ggml-opt.h gguf.h)
  if(NOT EXISTS "${LLW_LLAMA_INCLUDE_DIR}/${required_header}")
    message(FATAL_ERROR "Pinned llama.cpp header is missing: ${required_header}")
  endif()
endforeach()

set(LLW_BACKEND_PACK "CPU" CACHE STRING "Runtime backend pack: CPU, CUDA, or VULKAN")
set_property(CACHE LLW_BACKEND_PACK PROPERTY STRINGS CPU CUDA VULKAN)
string(TOUPPER "${LLW_BACKEND_PACK}" LLW_BACKEND_PACK)
if(NOT LLW_BACKEND_PACK MATCHES "^(CPU|CUDA|VULKAN)$")
  message(FATAL_ERROR "LLW_BACKEND_PACK must be CPU, CUDA, or VULKAN")
endif()

enable_testing()
find_package(Threads REQUIRED)

add_library(local_llm_runtime SHARED
  src/event_dispatcher.cpp
  src/llama_api.cpp
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
target_include_directories(local_llm_runtime PUBLIC include PRIVATE src "${LLW_LLAMA_INCLUDE_DIR}")
target_link_libraries(local_llm_runtime PRIVATE Threads::Threads)

set(LLW_PACK_DESTINATION ".")
install(TARGETS local_llm_runtime
  RUNTIME DESTINATION "${LLW_PACK_DESTINATION}"
  LIBRARY DESTINATION "${LLW_PACK_DESTINATION}"
)

add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
target_compile_definitions(llw_abi_layout_test PRIVATE
  $<$<CONFIG:Debug>:LLW_RUNTIME_TESTING>
)
target_link_libraries(llw_abi_layout_test PRIVATE local_llm_runtime)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test CONFIGURATIONS Debug)
set_tests_properties(llw_abi_layout_test PROPERTIES TIMEOUT 30)

add_executable(llw_scheduler_test
  tests/scheduler_test.cpp
  src/event_dispatcher.cpp
  src/scheduler.cpp
)
target_include_directories(llw_scheduler_test PRIVATE include src tests)
target_compile_definitions(llw_scheduler_test PRIVATE LLW_RUNTIME_TESTING)
target_link_libraries(llw_scheduler_test PRIVATE Threads::Threads)
add_test(NAME llw_scheduler_test COMMAND llw_scheduler_test)
set_tests_properties(llw_scheduler_test PROPERTIES TIMEOUT 30)

add_executable(llw_llama_engine_test
  tests/llama_engine_test.cpp
  src/llama_api.cpp
  src/llama_engine.cpp
)
target_include_directories(llw_llama_engine_test PRIVATE
  include src "${LLW_LLAMA_INCLUDE_DIR}"
)
target_compile_definitions(llw_llama_engine_test PRIVATE
  LLW_RUNTIME_TESTING
  LLW_BACKEND_PACK_NAME="${LLW_BACKEND_PACK}"
)
target_link_libraries(llw_llama_engine_test PRIVATE Threads::Threads)
add_test(NAME llw_llama_engine_test COMMAND llw_llama_engine_test)
set_tests_properties(llw_llama_engine_test PROPERTIES
  TIMEOUT 30
  ENVIRONMENT "LLW_TEST_RUNTIME_DIR=$<TARGET_FILE_DIR:local_llm_runtime>"
)

add_executable(llw_runtime_backend_test tests/runtime_backend_test.cpp)
target_include_directories(llw_runtime_backend_test PRIVATE include)
target_compile_definitions(llw_runtime_backend_test PRIVATE
  $<$<CONFIG:Debug>:LLW_RUNTIME_TESTING>
)
target_link_libraries(llw_runtime_backend_test PRIVATE local_llm_runtime Threads::Threads)
install(TARGETS llw_runtime_backend_test RUNTIME DESTINATION ".")
if(DEFINED ENV{LLW_TEST_GGUF} AND EXISTS "$ENV{LLW_TEST_GGUF}")
  add_test(NAME llw_runtime_backend_test COMMAND llw_runtime_backend_test "$ENV{LLW_TEST_GGUF}")
  set_tests_properties(llw_runtime_backend_test PROPERTIES TIMEOUT 180 LABELS "model;required-ci")
endif()
