cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime VERSION 0.2.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)

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
  GIT_CONFIG core.longpaths=true
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

find_package(Threads REQUIRED)
add_executable(llw_scheduler_test tests/scheduler_test.cpp src/event_dispatcher.cpp)
target_include_directories(llw_scheduler_test PRIVATE include src tests)
target_compile_definitions(llw_scheduler_test PRIVATE LLW_RUNTIME_TESTING)
target_link_libraries(llw_scheduler_test PRIVATE Threads::Threads)
add_test(NAME llw_scheduler_test COMMAND llw_scheduler_test)
set_tests_properties(llw_scheduler_test PROPERTIES TIMEOUT 30)
