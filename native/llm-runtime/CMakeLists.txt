cmake_minimum_required(VERSION 3.24)
project(local_llm_runtime VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

add_library(local_llm_runtime SHARED src/fake_runtime.cpp)
target_compile_definitions(local_llm_runtime PRIVATE LLW_RUNTIME_BUILD)
target_include_directories(local_llm_runtime PUBLIC include)

add_executable(llw_abi_layout_test tests/abi_layout_test.cpp)
target_include_directories(llw_abi_layout_test PRIVATE include)
target_link_libraries(llw_abi_layout_test PRIVATE local_llm_runtime)
add_test(NAME llw_abi_layout_test COMMAND llw_abi_layout_test)
